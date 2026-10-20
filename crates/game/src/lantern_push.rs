//! Presentation for Lantern Push. Reads `ActiveRun<LanternPush>` only; never writes simulation
//! state. Positions are blended between the last two ticks so motion stays smooth.

use std::f32::consts::TAU;

use bevy::prelude::*;
use minigame_lantern_push::{
    DASH_COOLDOWN, FALL_TICKS, Fighter, LanternPush, MIN_RADIUS, PLAY_TICKS, Phase, SHRINK_START,
    START_RADIUS, State, Status, platform_radius,
};
use sim_core::physics::Vec2 as SimVec2;
use sim_core::{Format, Seat, TICK_HZ};

use crate::audio::{Sfx, Sound};
use crate::characters::{Character, Pose, Roster};
use crate::flow::AppState;
use crate::i18n::{self, bot_tag, halted_banner};
use crate::palette::{LANTERN_WARM, MOONLIGHT, NIGHT_SKY, seat_color, seat_label};
use crate::particles::{self, Effects, hdr};
use crate::runner::{ActiveRun, MinigameSet, Seats};
use crate::scenery::{emissive_material, night_camera, noise, spawn_fireflies, spawn_lantern_folk};
use crate::tr;

pub struct LanternPushViewPlugin;

impl Plugin for LanternPushViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            (spawn_scene, spawn_hud)
                .after(MinigameSet::Start)
                .run_if(resource_exists::<ActiveRun<LanternPush>>),
        )
        .add_systems(
            Update,
            (
                animate_platform,
                crumble_edge,
                animate_debris,
                animate_fighters,
                animate_sky_lanterns,
                (fit_camera, shake_camera).chain(),
                update_hud,
            )
                .run_if(resource_exists::<ActiveRun<LanternPush>>),
        )
        .add_systems(
            FixedUpdate,
            react
                .in_set(MinigameSet::React)
                .run_if(resource_exists::<ActiveRun<LanternPush>>),
        );
    }
}

/// Sounds and sparks, once per event: dashes, braces, hits, falls, the shrinking platform,
/// the last seconds and the win.
fn react(
    mut commands: Commands,
    active: Res<ActiveRun<LanternPush>>,
    effects: Res<Effects>,
    sfx: Sfx,
) {
    let Some((before, now)) = active.ticked() else {
        return;
    };
    let mut spark = |pos: Vec3, color: Vec4| {
        particles::burst(
            &mut commands,
            &effects,
            DespawnOnExit(AppState::Minigame),
            pos,
            color,
        );
    };
    for seat in now.players.seats() {
        let (was, is) = (before.fighter(seat), now.fighter(seat));
        let variant = now.tick + seat.0 as u32;
        if was.dash == 0 && is.dash > 0 {
            sfx.play_varied(Sound::Dash, variant);
            spark(
                world(is.body.pos),
                hdr(LinearRgba::rgb(0.6, 0.55, 0.5), 0.8),
            );
        }
        if !was.bracing() && is.bracing() {
            sfx.play_varied(Sound::Brace, variant);
        }
        if is.last_hit != was.last_hit
            && let Some(by) = is.last_hit_by
        {
            // The one hit last also gets `last_hit` when their dash bounced off a brace.
            let other = now.fighter(by);
            if other.bracing() {
                sfx.play_varied(Sound::Rebound, variant);
            } else {
                sfx.play_varied(Sound::Hit, variant);
            }
            let contact = world(is.body.pos.lerp(other.body.pos, 0.5)) + Vec3::Y * 0.7;
            spark(contact, hdr(seat_color(by), 5.0));
        }
        if was.standing() && matches!(is.status, Status::Falling { .. }) {
            sfx.play_varied(Sound::Fall, variant);
            spark(
                world(is.body.pos) + Vec3::Y * 0.5,
                hdr(seat_color(seat), 5.0),
            );
        }
    }
    if now.phase == Phase::Playing {
        if before.tick < SHRINK_START && now.tick >= SHRINK_START {
            sfx.play(Sound::Rumble);
        }
        let left = PLAY_TICKS.saturating_sub(now.tick);
        if left > 0 && left <= 5 * TICK_HZ && left % TICK_HZ == 0 && before.tick != now.tick {
            sfx.play(Sound::Tick);
        }
    }
    if before.outcome.is_none()
        && let Some(outcome) = now.outcome
    {
        sfx.play(Sound::Win);
        for seat in outcome.winners() {
            let fighter = now.fighter(seat);
            if fighter.standing() {
                spark(
                    world(fighter.body.pos) + Vec3::Y * 1.6,
                    hdr(seat_color(seat), 6.0),
                );
            }
        }
    }
}

/// Team colours for the floor discs in team formats (players keep their own colours elsewhere).
const TEAM_COLORS: [LinearRgba; 2] = [
    LinearRgba::rgb(1.0, 0.78, 0.3),
    LinearRgba::rgb(0.55, 0.45, 1.0),
];
const PLATFORM_DEPTH: f32 = 1.2;
const SKY_LANTERNS: usize = 10;
const FIREFLY_COUNT: usize = 40;

/// Scaled every frame so it matches the simulated platform radius exactly.
#[derive(Component)]
struct ScalesWithRadius;

#[derive(Component)]
struct EdgeRing {
    material: Handle<StandardMaterial>,
}

/// A decorative floor tile; it drops away once the shrinking edge reaches it.
#[derive(Component)]
struct Tile {
    /// The tile's farthest corner from the centre.
    reach: f32,
    home: Vec3,
    spin: Vec3,
}

#[derive(Component)]
struct Debris {
    velocity: Vec3,
    spin: Vec3,
    born: f32,
}

#[derive(Component)]
struct SkyLantern {
    origin: Vec3,
    phase: f32,
}

#[derive(Component)]
struct FighterView {
    seat: Seat,
    head_material: Handle<StandardMaterial>,
    head_light: Entity,
    disc: Entity,
    disc_material: Handle<StandardMaterial>,
    disc_color: LinearRgba,
    brace_ring: Entity,
}

#[derive(Component)]
struct CameraRig {
    base: Transform,
}

#[derive(Component)]
enum HudText {
    Banner,
    SubBanner,
    SeatPanel(Seat),
}

fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    active: Res<ActiveRun<LanternPush>>,
    roster: Res<Roster>,
) {
    commands.insert_resource(ClearColor(NIGHT_SKY));
    commands.insert_resource(GlobalAmbientLight {
        color: MOONLIGHT,
        brightness: 40.0,
        ..default()
    });
    let camera = camera_transform(1.0);
    commands.spawn((
        DespawnOnExit(AppState::Minigame),
        night_camera(camera, (18.0, 46.0)),
        CameraRig { base: camera },
    ));
    commands.spawn((
        DespawnOnExit(AppState::Minigame),
        DirectionalLight {
            color: MOONLIGHT,
            illuminance: 450.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-5.0, 12.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // A warm lantern hanging over the middle, so the arena reads as the brightest thing on screen.
    commands.spawn((
        DespawnOnExit(AppState::Minigame),
        PointLight {
            color: Color::from(LANTERN_WARM),
            intensity: 2_500_000.0,
            range: 30.0,
            radius: 0.5,
            ..default()
        },
        Transform::from_xyz(0.0, 9.0, 0.0),
    ));

    spawn_platform(&mut commands, &mut meshes, &mut materials);
    spawn_sky_lanterns(&mut commands, &mut meshes, &mut materials);
    spawn_fireflies(
        &mut commands,
        &mut meshes,
        &mut materials,
        AppState::Minigame,
        FIREFLY_COUNT,
        Vec3::new(-11.0, -2.0, -9.0),
        Vec3::new(11.0, 4.0, 7.0),
    );

    let game = &active.run.game;
    for seat in game.players.seats() {
        spawn_fighter(
            &mut commands,
            &mut meshes,
            &mut materials,
            game,
            seat,
            roster.of(seat),
        );
    }
}

fn spawn_platform(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    // The disc is scaled to the simulated radius every frame, so its edge is the real edge.
    commands.spawn((
        DespawnOnExit(AppState::Minigame),
        ScalesWithRadius,
        Mesh3d(meshes.add(Cylinder::new(1.0, PLATFORM_DEPTH).mesh().resolution(64))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.07, 0.065, 0.08),
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(0.0, -PLATFORM_DEPTH / 2.0, 0.0).with_scale(Vec3::new(
            START_RADIUS,
            1.0,
            START_RADIUS,
        )),
    ));
    let ring_material = materials.add(emissive_material(LANTERN_WARM, 4.0));
    commands.spawn((
        DespawnOnExit(AppState::Minigame),
        ScalesWithRadius,
        EdgeRing {
            material: ring_material.clone(),
        },
        Mesh3d(meshes.add(Torus::new(0.94, 1.0).mesh().major_resolution(96))),
        MeshMaterial3d(ring_material),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));

    let tile_mesh = meshes.add(Cuboid::new(0.94, 0.12, 0.94));
    let shades: Vec<_> = [
        (0.10, 0.095, 0.11),
        (0.085, 0.08, 0.095),
        (0.12, 0.11, 0.115),
    ]
    .into_iter()
    .map(|(r, g, b)| {
        materials.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            perceptual_roughness: 0.85,
            ..default()
        })
    })
    .collect();
    let half = START_RADIUS.ceil() as i32;
    for i in -half..=half {
        for j in -half..=half {
            let centre = Vec2::new(i as f32, j as f32);
            let reach = centre.abs().length() + 0.67;
            if reach > START_RADIUS + 0.05 {
                continue;
            }
            let home = Vec3::new(centre.x, -0.03 + noise(i, j, 3) * 0.04, centre.y);
            commands.spawn((
                DespawnOnExit(AppState::Minigame),
                Tile {
                    reach,
                    home,
                    spin: Vec3::new(
                        noise(i, j, 4) - 0.5,
                        noise(i, j, 5) - 0.5,
                        noise(i, j, 6) - 0.5,
                    ) * 4.0,
                },
                Mesh3d(tile_mesh.clone()),
                MeshMaterial3d(shades[(noise(i, j, 1) * 2.999) as usize].clone()),
                Transform::from_translation(home),
            ));
        }
    }
}

fn spawn_sky_lanterns(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let mesh = meshes.add(Cuboid::new(0.5, 0.65, 0.5));
    let colors = [
        LANTERN_WARM,
        LinearRgba::rgb(1.0, 0.3, 0.55),
        LinearRgba::rgb(0.25, 0.75, 1.0),
    ];
    for k in 0..SKY_LANTERNS as i32 {
        let angle = k as f32 / SKY_LANTERNS as f32 * TAU + noise(k, 0, 20) * 0.4;
        let distance = START_RADIUS + 3.0 + noise(k, 0, 21) * 4.0;
        let origin = Vec3::new(
            angle.cos() * distance,
            -1.0 + noise(k, 0, 22) * 5.0,
            angle.sin() * distance,
        );
        let color = colors[k as usize % colors.len()];
        commands.spawn((
            DespawnOnExit(AppState::Minigame),
            SkyLantern {
                origin,
                phase: noise(k, 0, 23) * TAU,
            },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(materials.add(emissive_material(color, 2.5))),
            Transform::from_translation(origin),
            children![(
                PointLight {
                    color: Color::from(color),
                    intensity: 120_000.0,
                    range: 9.0,
                    ..default()
                },
                Transform::default(),
            )],
        ));
    }
}

fn team_index(format: &Format, seat: Seat) -> Option<u8> {
    match format {
        Format::FreeForAll | Format::Duel { .. } => None,
        _ => format.team(seat),
    }
}

fn spawn_fighter(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    game: &State,
    seat: Seat,
    character: Character,
) {
    let fighter = game.fighter(seat);
    let scale = fighter.body.radius / minigame_lantern_push::RADIUS;
    let folk = spawn_lantern_folk(
        commands,
        meshes,
        materials,
        AppState::Minigame,
        seat,
        character,
        Transform::from_translation(world(fighter.body.pos)).with_scale(Vec3::splat(scale)),
    );
    // The disc under each fighter is their collision footprint; it glows when a dash is ready.
    let disc_color = match team_index(&game.format, seat) {
        Some(team) => TEAM_COLORS[team as usize],
        None => seat_color(seat),
    };
    let disc_material = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: disc_color * 2.0,
        ..default()
    });
    let disc = commands
        .spawn((
            DespawnOnExit(AppState::Minigame),
            Mesh3d(meshes.add(Cylinder::new(fighter.body.radius, 0.04))),
            MeshMaterial3d(disc_material.clone()),
            Transform::from_translation(world(fighter.body.pos) + Vec3::Y * 0.05),
        ))
        .id();
    let brace_ring = commands
        .spawn((
            DespawnOnExit(AppState::Minigame),
            Mesh3d(meshes.add(Torus::new(
                fighter.body.radius * 1.15,
                fighter.body.radius * 1.35,
            ))),
            MeshMaterial3d(materials.add(emissive_material(LinearRgba::rgb(0.8, 0.9, 1.0), 6.0))),
            Transform::from_translation(world(fighter.body.pos)),
            Visibility::Hidden,
        ))
        .id();
    commands.entity(folk.root).insert(FighterView {
        seat,
        head_material: folk.head_material,
        head_light: folk.head_light,
        disc,
        disc_material,
        disc_color,
        brace_ring,
    });
}

/// Simulation floor coordinates to world space.
fn world(v: SimVec2) -> Vec3 {
    Vec3::new(v.x, 0.0, v.y)
}

fn blend(fixed: &Time<Fixed>) -> f32 {
    fixed.overstep_fraction().clamp(0.0, 1.0)
}

/// Platform radius blended between the last two ticks.
fn shown_radius(active: &ActiveRun<LanternPush>, alpha: f32) -> f32 {
    let previous = platform_radius(active.previous.tick);
    previous + (active.run.game.radius() - previous) * alpha
}

fn animate_platform(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternPush>>,
    mut scaled: Query<&mut Transform, With<ScalesWithRadius>>,
    ring: Query<&EdgeRing>,
    mut tiles: Query<(&Tile, &mut Transform, &mut Visibility), Without<ScalesWithRadius>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let radius = shown_radius(&active, blend(&fixed));
    let now = time.elapsed_secs();
    for mut transform in &mut scaled {
        transform.scale = Vec3::new(radius, 1.0, radius);
    }
    // The edge pulses faster the smaller the platform gets.
    let urgency = 1.0 - (radius - MIN_RADIUS) / (START_RADIUS - MIN_RADIUS);
    let pulse = 0.5 + 0.5 * (now * (3.0 + urgency * 9.0)).sin();
    for ring in &ring {
        if let Some(mut material) = materials.get_mut(&ring.material) {
            let hot = LANTERN_WARM.mix(&LinearRgba::rgb(1.0, 0.15, 0.08), urgency);
            material.emissive = hot * (3.0 + pulse * (2.0 + urgency * 6.0));
        }
    }
    for (tile, mut transform, mut visibility) in &mut tiles {
        // Tiles shake just before the edge reaches them, then drop away.
        let margin = radius - tile.reach;
        if margin >= 0.6 {
            transform.translation = tile.home;
            transform.rotation = Quat::IDENTITY;
            *visibility = Visibility::Inherited;
        } else if margin >= 0.0 {
            let shake = (0.6 - margin) * 0.05;
            transform.translation = tile.home
                + Vec3::new(
                    (now * 53.0 + tile.home.x).sin(),
                    0.0,
                    (now * 47.0 + tile.home.z).cos(),
                ) * shake;
            *visibility = Visibility::Inherited;
        } else {
            let fall = (-margin * 2.5).min(4.0);
            transform.translation = tile.home - Vec3::Y * fall * fall * 2.0;
            transform.rotation = Quat::from_scaled_axis(tile.spin * fall * 0.3);
            *visibility = if fall >= 4.0 {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
        }
    }
}

/// Chunks break off the rim as it shrinks. Purely decorative.
fn crumble_edge(
    mut commands: Commands,
    time: Res<Time>,
    active: Res<ActiveRun<LanternPush>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut last: Local<Option<(u32, f32, u32)>>,
) {
    let radius = active.run.game.radius();
    let (generation, mut crumbled_at, mut count) =
        last.unwrap_or((active.generation, START_RADIUS, 0));
    if generation != active.generation || active.is_added() {
        crumbled_at = START_RADIUS;
    }
    if crumbled_at - radius >= 0.08 {
        crumbled_at = radius;
        let mesh = meshes.add(Cuboid::new(0.35, 0.3, 0.35));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.08, 0.075, 0.09),
            emissive: LANTERN_WARM * 0.6,
            ..default()
        });
        for k in 0..5 {
            count += 1;
            let angle = noise(count as i32, k, 30) * TAU;
            let dir = Vec3::new(angle.cos(), 0.0, angle.sin());
            commands.spawn((
                DespawnOnExit(AppState::Minigame),
                Debris {
                    velocity: dir * (0.5 + noise(count as i32, k, 31)) + Vec3::Y * 1.5,
                    spin: Vec3::new(noise(count as i32, k, 32), 1.0, noise(count as i32, k, 33))
                        * 5.0,
                    born: time.elapsed_secs(),
                },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(dir * (radius + 0.2) - Vec3::Y * 0.15),
            ));
        }
    }
    *last = Some((active.generation, crumbled_at, count));
}

fn animate_debris(
    mut commands: Commands,
    time: Res<Time>,
    mut debris: Query<(Entity, &mut Debris, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut chunk, mut transform) in &mut debris {
        chunk.velocity.y -= 14.0 * dt;
        transform.translation += chunk.velocity * dt;
        transform.rotate(Quat::from_scaled_axis(chunk.spin * dt));
        if time.elapsed_secs() - chunk.born > 2.5 {
            commands.entity(entity).despawn();
        }
    }
}

fn facing_rotation(facing: SimVec2) -> Quat {
    Quat::from_rotation_y(facing.x.atan2(facing.y))
}

/// Seconds since a tick of play, for one-shot effects.
fn since(game: &State, tick: u32) -> f32 {
    game.tick.saturating_sub(tick) as f32 / TICK_HZ as f32
}

fn animate_fighters(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternPush>>,
    mut fighters: Query<(&FighterView, &mut Transform, &mut Visibility, &mut Pose)>,
    mut parts: Query<(&mut Transform, &mut Visibility), Without<FighterView>>,
    mut lights: Query<&mut PointLight>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let alpha = blend(&fixed);
    let now = time.elapsed_secs();
    let game = &active.run.game;
    let winners: Vec<Seat> = game
        .outcome
        .map(|o| o.winners().collect())
        .unwrap_or_default();
    for (view, mut transform, mut visibility, mut pose) in &mut fighters {
        let seat = view.seat;
        let fighter: &Fighter = game.fighter(seat);
        let before: &Fighter = active.previous.fighter(seat);
        let pos = before.body.pos.lerp(fighter.body.pos, alpha);
        let base_scale = fighter.body.radius / minigame_lantern_push::RADIUS;

        let (drop, tumble) = match fighter.status {
            Status::Falling { t } => {
                let secs = (t as f32 + alpha) / TICK_HZ as f32;
                (secs * secs * 9.0, secs * 6.0)
            }
            _ => (0.0, 0.0),
        };
        *visibility = if fighter.status == Status::Out {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };

        let celebrating = winners.contains(&seat) && fighter.standing();
        *pose = if celebrating {
            Pose::Celebrate
        } else if game.outcome.is_some() {
            Pose::Slump
        } else {
            Pose::Idle
        };
        let mut rotation = facing_rotation(fighter.facing);
        let mut scale = Vec3::ONE;
        if fighter.dash > 0 {
            rotation *= Quat::from_rotation_x(0.45);
            scale = Vec3::new(0.9, 0.95, 1.2);
        }
        if fighter.bracing() {
            scale = Vec3::new(1.2, 0.78, 1.2);
        }
        if fighter.stun > 0 {
            rotation *= Quat::from_rotation_z((now * 30.0).sin() * 0.25);
        }
        if tumble > 0.0 {
            rotation *= Quat::from_rotation_x(-tumble);
        }
        transform.translation = world(pos) - Vec3::Y * drop;
        transform.rotation = rotation;
        transform.scale = scale * base_scale;

        // Head flares when hit, dims when out of the game.
        let hit_flash = fighter
            .last_hit
            .map(|tick| (1.0 - since(game, tick) * 3.0).max(0.0))
            .unwrap_or(0.0);
        let glow = match fighter.status {
            Status::Standing if celebrating => 5.0,
            Status::Standing => 1.8 + hit_flash * 6.0,
            Status::Falling { t } => 1.8 * (1.0 - t as f32 / FALL_TICKS as f32),
            Status::Out => 0.0,
        };
        if let Some(mut material) = materials.get_mut(&view.head_material) {
            material.emissive = seat_color(seat) * glow;
        }
        if let Ok(mut light) = lights.get_mut(view.head_light) {
            light.intensity = 22_000.0 * glow;
        }

        let ready = fighter.dash_cooldown == 0;
        if let Some(mut material) = materials.get_mut(&view.disc_material) {
            let charge = 1.0 - fighter.dash_cooldown as f32 / DASH_COOLDOWN as f32;
            let strength = if ready {
                3.0 + (now * 6.0).sin() * 0.5
            } else {
                0.4 + charge
            };
            material.emissive = view.disc_color * strength;
        }
        if let Ok((mut disc, mut disc_visibility)) = parts.get_mut(view.disc) {
            disc.translation = world(pos) + Vec3::Y * 0.05;
            *disc_visibility = if fighter.standing() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
        if let Ok((mut ring, mut ring_visibility)) = parts.get_mut(view.brace_ring) {
            ring.translation = world(pos) + Vec3::Y * 0.4;
            ring.rotation = Quat::from_rotation_y(now * 4.0);
            *ring_visibility = if fighter.bracing() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
}

fn animate_sky_lanterns(time: Res<Time>, mut lanterns: Query<(&SkyLantern, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (lantern, mut transform) in &mut lanterns {
        transform.translation =
            lantern.origin + Vec3::new(0.0, (t * 0.6 + lantern.phase).sin() * 0.4, 0.0);
        transform.rotation = Quat::from_rotation_y(t * 0.3 + lantern.phase);
    }
}

const CAMERA_TARGET: Vec3 = Vec3::new(0.0, -0.4, 1.6);
const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 11.5, 9.6);
/// Half-width of the platform (plus margin) that must stay on screen.
const FRAMED_HALF_WIDTH: f32 = START_RADIUS + 1.0;

fn camera_transform(pull_back: f32) -> Transform {
    Transform::from_translation(CAMERA_TARGET + CAMERA_OFFSET * pull_back)
        .looking_at(CAMERA_TARGET, Vec3::Y)
}

/// Pull the camera back on narrow windows so the whole platform stays in frame.
fn fit_camera(
    windows: Query<&Window, Changed<Window>>,
    mut camera: Query<(&mut CameraRig, &Projection)>,
) {
    let Some(window) = windows.iter().next() else {
        return;
    };
    let aspect = window.width() / window.height().max(1.0);
    for (mut rig, projection) in &mut camera {
        let Projection::Perspective(perspective) = projection else {
            continue;
        };
        let visible_half_width = (perspective.fov / 2.0).tan() * aspect * CAMERA_OFFSET.length();
        rig.base = camera_transform((FRAMED_HALF_WIDTH / visible_half_width).max(1.0));
    }
}

/// A short kick whenever someone takes a hit.
fn shake_camera(
    active: Res<ActiveRun<LanternPush>>,
    time: Res<Time>,
    mut camera: Query<(&CameraRig, &mut Transform)>,
) {
    let game = &active.run.game;
    let latest_hit = game
        .players
        .seats()
        .filter_map(|s| game.fighter(s).last_hit)
        .max();
    let strength = latest_hit
        .map(|tick| (0.12 - since(game, tick) * 0.5).max(0.0))
        .unwrap_or(0.0);
    let t = time.elapsed_secs();
    for (rig, mut transform) in &mut camera {
        *transform = rig.base;
        transform.translation += Vec3::new((t * 91.0).sin(), (t * 73.0).cos(), 0.0) * strength;
    }
}

fn spawn_hud(mut commands: Commands, active: Res<ActiveRun<LanternPush>>) {
    commands
        .spawn((
            DespawnOnExit(AppState::Minigame),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(30.0),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .with_children(|top| {
            top.spawn((
                HudText::Banner,
                Text::new(""),
                TextFont::from_font_size(54.0),
                TextColor(Color::srgb(1.0, 0.93, 0.8)),
                TextLayout::justify(Justify::Center),
            ));
            top.spawn((
                HudText::SubBanner,
                Text::new(""),
                TextFont::from_font_size(20.0),
                TextColor(Color::srgba(0.85, 0.88, 1.0, 0.85)),
                TextLayout::justify(Justify::Center),
            ));
        });

    let game = &active.run.game;
    commands
        .spawn((
            DespawnOnExit(AppState::Minigame),
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(20.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(16.0),
                ..default()
            },
        ))
        .with_children(|row| {
            for seat in game.players.seats() {
                let border = match team_index(&game.format, seat) {
                    Some(team) => Color::from(TEAM_COLORS[team as usize]),
                    None => Color::NONE,
                };
                row.spawn((
                    Node {
                        width: Val::Px(200.0),
                        padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
                        border: UiRect::all(Val::Px(3.0)),
                        ..default()
                    },
                    BorderColor::all(border),
                    BackgroundColor(Color::from(seat_color(seat) * 0.25).with_alpha(0.55)),
                ))
                .with_child((
                    HudText::SeatPanel(seat),
                    Text::new(""),
                    TextFont::from_font_size(18.0),
                    TextColor(Color::WHITE),
                ));
            }
        });
}

fn ordinal(place: u8) -> String {
    tr!("place", place = place as u32)
}

fn clock(ticks: u32) -> String {
    let secs = ticks.div_ceil(TICK_HZ);
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn banner_text(active: &ActiveRun<LanternPush>) -> (String, String) {
    let run = &active.run;
    let game = &run.game;
    if let Some(banner) = halted_banner(run) {
        return banner;
    }
    match game.phase {
        // The shared countdown (crate::intro) is drawn over this.
        Phase::Intro { .. } => (String::new(), String::new()),
        Phase::Playing => {
            // Announce the latest knockout for a moment.
            let fallen = game
                .players
                .seats()
                .filter_map(|s| game.fell_at[s.index()].map(|t| (t, s)))
                .max();
            match fallen {
                Some((tick, seat)) if since(game, tick) < 1.5 => (
                    tr!("lantern-push-out", player = seat_label(seat)),
                    clock(game.ticks_left()),
                ),
                _ if game.tick < 45 => (tr!("lantern-push-go"), clock(game.ticks_left())),
                _ => (String::new(), clock(game.ticks_left())),
            }
        }
        Phase::Decided { .. } | Phase::Finished => {
            let outcome = game.outcome.expect("decided games have an outcome");
            let winners: Vec<Seat> = outcome.winners().collect();
            let title = if winners.len()
                == game
                    .players
                    .seats()
                    .filter(|s| game.format.team(*s).is_some())
                    .count()
            {
                tr!("lantern-push-everyone")
            } else {
                i18n::winners(&winners)
            };
            let prompt = if game.phase == Phase::Finished {
                active.continue_prompt()
            } else {
                String::new()
            };
            (title, prompt)
        }
    }
}

fn update_hud(
    active: Res<ActiveRun<LanternPush>>,
    seats: Res<Seats>,
    mut texts: Query<(&HudText, &mut Text)>,
) {
    let (title, subtitle) = banner_text(&active);
    let game = &active.run.game;
    for (hud, mut text) in &mut texts {
        text.0 = match *hud {
            HudText::Banner => title.clone(),
            HudText::SubBanner => subtitle.clone(),
            HudText::SeatPanel(seat) => {
                let who = bot_tag(seats.is_bot(seat));
                let role = match game.format {
                    Format::OneVsThree { solo } if solo == seat => tr!("solo-tag"),
                    _ => String::new(),
                };
                let fighter = game.fighter(seat);
                let status = if active.run.controller_lost(seat) {
                    tr!("controller-lost")
                } else if let (Some(outcome), true) = (game.outcome, game.phase != Phase::Playing) {
                    match outcome.placement[seat.index()] {
                        Some(place) => ordinal(place),
                        None => String::new(),
                    }
                } else {
                    match fighter.status {
                        Status::Standing if fighter.bracing() => tr!("lantern-push-bracing"),
                        Status::Standing if fighter.stun > 0 => tr!("lantern-push-reeling"),
                        Status::Standing => tr!("lantern-push-standing"),
                        Status::Falling { .. } | Status::Out => tr!("lantern-push-is-out"),
                    }
                };
                format!("{}{who}{role}\n{status}", seat_label(seat))
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facing_rotation_turns_the_model_front_towards_facing() {
        let front = facing_rotation(SimVec2::new(1.0, 0.0)) * Vec3::Z;
        assert!((front - Vec3::X).length() < 1e-5);
        let front = facing_rotation(SimVec2::new(0.0, -1.0)) * Vec3::Z;
        assert!((front + Vec3::Z).length() < 1e-5);
    }
}
