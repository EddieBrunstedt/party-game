//! Presentation for Sky Lanterns. Reads `ActiveRun<SkyLanterns>` only; never writes simulation
//! state. The sky is the simulation's plane seen straight on through an orthographic camera, so a
//! sight drawn over a lantern is over it in the simulation too. Lanterns rise from behind a dark
//! hill, and the lantern folk watch from its foot.

use bevy::camera::ScalingMode;
use bevy::prelude::*;
use minigame_sky_lanterns::{
    FLASH_TICKS, Flyer, HITS_TO_WIN, Lantern, Phase, SKY, Shot, SkyLanterns, State,
};
use sim_core::physics::Vec2 as SimVec2;
use sim_core::{Format, Seat, TICK_HZ};

use crate::audio::{Sfx, Sound};
use crate::characters::{Pose, Roster};
use crate::flow::AppState;
use crate::i18n::{self, bot_tag, halted_banner};
use crate::palette::{LANTERN_WARM, MOONLIGHT, NIGHT_SKY, seat_color, seat_label};
use crate::particles::{self, Effects, hdr};
use crate::runner::{ActiveRun, MinigameSet, Seats};
use crate::scenery::{
    emissive_material, flat_marker_mesh, night_camera, noise, spawn_fireflies, spawn_lantern_folk,
};
use crate::tr;

pub struct SkyLanternsViewPlugin;

impl Plugin for SkyLanternsViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            (spawn_scene, spawn_hud)
                .after(MinigameSet::Start)
                .run_if(resource_exists::<ActiveRun<SkyLanterns>>),
        )
        .add_systems(
            Update,
            (
                sync_lanterns,
                animate_lanterns,
                animate_sights,
                animate_flyers,
                animate_folk,
                update_hud,
            )
                .chain()
                .run_if(resource_exists::<ActiveRun<SkyLanterns>>),
        )
        .add_systems(
            FixedUpdate,
            react
                .in_set(MinigameSet::React)
                .run_if(resource_exists::<ActiveRun<SkyLanterns>>),
        );
    }
}

const SCOPE: DespawnOnExit<AppState> = DespawnOnExit(AppState::Minigame);
/// Depths: the three's lanterns fly behind the others, the hill hides lanterns still rising, the
/// folk stand in front of it, and the sights are in front of everything.
const FLYER_Z: f32 = -1.5;
const HILL_Z: f32 = 1.0;
const FOLK_Z: f32 = 2.0;
const SIGHT_Z: f32 = 4.0;
/// Where the folk stand, below the hill's top (the horizon, the sky's bottom edge).
const FOLK_Y: f32 = -SKY.y - 2.0;
const SIGHT_RADIUS: f32 = 0.36;
/// How long a lit lantern or a hit shows on whoever did it, in ticks.
const REACTION_TICKS: u32 = 30;
const PAPER: LinearRgba = LinearRgba::rgb(1.0, 0.82, 0.6);
const GOLD: LinearRgba = LinearRgba::rgb(1.0, 0.75, 0.15);
const FIZZLE_GREY: LinearRgba = LinearRgba::rgb(0.4, 0.4, 0.45);
const STAR_COUNT: i32 = 45;
const FIREFLY_COUNT: usize = 16;
/// Team colours for the panel borders in 2v2.
const TEAM_COLORS: [LinearRgba; 2] = [
    LinearRgba::rgb(1.0, 0.78, 0.3),
    LinearRgba::rgb(0.55, 0.45, 1.0),
];

#[derive(Component)]
struct LanternView {
    id: u32,
    generation: u32,
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct SightView {
    seat: Seat,
    ring: Handle<StandardMaterial>,
}

#[derive(Component)]
struct FlyerView {
    seat: Seat,
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct FolkView {
    seat: Seat,
    head_material: Handle<StandardMaterial>,
    head_light: Entity,
}

#[derive(Resource)]
struct LanternMeshes {
    body: Handle<Mesh>,
    cap: Handle<Mesh>,
    cap_material: Handle<StandardMaterial>,
}

#[derive(Component)]
enum HudText {
    Banner,
    SubBanner,
    SeatPanel(Seat),
}

/// A point in the sky, in the world.
fn world(p: SimVec2, z: f32) -> Vec3 {
    Vec3::new(p.x, p.y, z)
}

fn blend(fixed: &Time<Fixed>) -> f32 {
    fixed.overstep_fraction().clamp(0.0, 1.0)
}

/// Seats that play, in seat order: everyone but those outside a duel.
fn playing(game: &State) -> Vec<Seat> {
    game.players
        .seats()
        .filter(|s| game.format.team(*s).is_some())
        .collect()
}

fn team_index(format: &Format, seat: Seat) -> Option<u8> {
    match format {
        Format::TwoVsTwo { .. } => format.team(seat),
        _ => None,
    }
}

fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    active: Res<ActiveRun<SkyLanterns>>,
    roster: Res<Roster>,
) {
    commands.insert_resource(ClearColor(NIGHT_SKY));
    commands.insert_resource(GlobalAmbientLight {
        color: MOONLIGHT,
        brightness: 60.0,
        ..default()
    });
    commands.spawn((
        SCOPE,
        night_camera(camera_transform(), (40.0, 80.0)),
        camera_projection(),
    ));
    commands.spawn((
        SCOPE,
        DirectionalLight {
            color: MOONLIGHT,
            illuminance: 500.0,
            ..default()
        },
        Transform::from_xyz(4.0, 6.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Stars, far behind everything.
    let star = meshes.add(Circle::new(0.04));
    let star_material = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: LinearRgba::rgb(0.8, 0.85, 1.0) * 0.9,
        ..default()
    });
    for k in 0..STAR_COUNT {
        let x = (noise(k, 1, 20) * 2.0 - 1.0) * 11.0;
        let y = -SKY.y + noise(k, 1, 21) * (SKY.y * 2.0 + 1.0);
        commands.spawn((
            SCOPE,
            Mesh3d(star.clone()),
            MeshMaterial3d(star_material.clone()),
            Transform::from_xyz(x, y, -8.0).with_scale(Vec3::splat(0.6 + noise(k, 1, 22))),
        ));
    }

    spawn_hill(&mut commands, &mut meshes, &mut materials);
    spawn_fireflies(
        &mut commands,
        &mut meshes,
        &mut materials,
        AppState::Minigame,
        FIREFLY_COUNT,
        Vec3::new(-10.0, FOLK_Y, FOLK_Z - 0.5),
        Vec3::new(10.0, -SKY.y - 0.3, FOLK_Z + 0.5),
    );

    commands.insert_resource(LanternMeshes {
        body: meshes.add(Sphere::new(1.0).mesh().uv(24, 16)),
        cap: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        cap_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.08, 0.05, 0.03),
            emissive: LANTERN_WARM * 0.15,
            perceptual_roughness: 0.9,
            ..default()
        }),
    });

    let game = &active.run.game;
    let seats = playing(game);
    let ring = meshes.add(Annulus::new(SIGHT_RADIUS - 0.06, SIGHT_RADIUS));
    let dot = meshes.add(Circle::new(0.05));
    for (index, seat) in seats.iter().enumerate() {
        let x = (index as f32 - (seats.len() as f32 - 1.0) / 2.0) * 3.6;
        let folk = spawn_lantern_folk(
            &mut commands,
            &mut meshes,
            &mut materials,
            AppState::Minigame,
            *seat,
            roster.of(*seat),
            Transform::from_xyz(x, FOLK_Y, FOLK_Z).with_scale(Vec3::splat(0.85)),
        );
        commands.entity(folk.root).insert(FolkView {
            seat: *seat,
            head_material: folk.head_material,
            head_light: folk.head_light,
        });

        if game.sights[seat.index()].is_some() {
            let ring_material = materials.add(StandardMaterial {
                base_color: Color::BLACK,
                emissive: seat_color(*seat) * 4.0,
                ..default()
            });
            let marker = flat_marker_mesh(&mut meshes, *seat);
            commands
                .spawn((
                    SCOPE,
                    SightView {
                        seat: *seat,
                        ring: ring_material.clone(),
                    },
                    Mesh3d(ring.clone()),
                    MeshMaterial3d(ring_material.clone()),
                    Transform::from_xyz(0.0, 0.0, SIGHT_Z),
                ))
                .with_children(|sight| {
                    sight.spawn((
                        Mesh3d(dot.clone()),
                        MeshMaterial3d(ring_material.clone()),
                        Transform::default(),
                    ));
                    // The seat's shape beside the ring, so colour is never the only cue.
                    sight.spawn((
                        Mesh3d(marker),
                        MeshMaterial3d(ring_material.clone()),
                        Transform::from_xyz(SIGHT_RADIUS + 0.12, SIGHT_RADIUS + 0.12, 0.0),
                    ));
                });
        }
    }

    for flyer in &game.flyers {
        let material = materials.add(emissive_material(seat_color(flyer.seat), 1.2));
        let marker_material = materials.add(StandardMaterial {
            base_color: Color::BLACK,
            emissive: seat_color(flyer.seat) * 4.0,
            ..default()
        });
        let marker = flat_marker_mesh(&mut meshes, flyer.seat);
        let body = meshes.add(Sphere::new(flyer.body.radius).mesh().uv(24, 16));
        commands
            .spawn((
                SCOPE,
                FlyerView {
                    seat: flyer.seat,
                    material: material.clone(),
                },
                Mesh3d(body),
                MeshMaterial3d(material),
                Transform::from_translation(world(flyer.body.pos, FLYER_Z)),
            ))
            .with_child((
                Mesh3d(marker),
                MeshMaterial3d(marker_material),
                Transform::from_xyz(0.0, flyer.body.radius + 0.22, 0.0),
            ));
    }
}

/// A dark hill along the bottom: lanterns rise from behind it.
fn spawn_hill(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let tile = meshes.add(Cuboid::new(1.02, 1.0, 1.0));
    let shades: Vec<_> = [(0.03, 0.035, 0.05), (0.025, 0.03, 0.042)]
        .into_iter()
        .map(|(r, g, b)| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                perceptual_roughness: 0.95,
                ..default()
            })
        })
        .collect();
    for i in -13..=13 {
        // The top stays at the horizon over the sky's width, so lanterns appear as they enter it.
        let bump = if (i as f32).abs() < SKY.x {
            0.0
        } else {
            noise(i, 0, 30) * 0.6
        };
        let top = -SKY.y + bump;
        let height = top - (FOLK_Y - 3.0);
        commands.spawn((
            SCOPE,
            Mesh3d(tile.clone()),
            MeshMaterial3d(shades[(noise(i, 0, 31) * 1.999) as usize].clone()),
            Transform::from_xyz(i as f32, top - height / 2.0, HILL_Z)
                .with_scale(Vec3::new(1.0, height, 1.0)),
        ));
    }
}

/// Spawn views for new lanterns and remove views whose lantern is gone.
fn sync_lanterns(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    meshes: Res<LanternMeshes>,
    active: Res<ActiveRun<SkyLanterns>>,
    views: Query<(Entity, &LanternView)>,
) {
    let game = &active.run.game;
    for (entity, view) in &views {
        let alive =
            view.generation == active.generation && game.lanterns.iter().any(|l| l.id == view.id);
        if !alive {
            commands.entity(entity).despawn();
        }
    }
    for lantern in &game.lanterns {
        if views
            .iter()
            .any(|(_, v)| v.id == lantern.id && v.generation == active.generation)
        {
            continue;
        }
        let material = materials.add(emissive_material(PAPER, 0.0));
        let r = lantern.radius;
        commands
            .spawn((
                SCOPE,
                LanternView {
                    id: lantern.id,
                    generation: active.generation,
                    material: material.clone(),
                },
                Mesh3d(meshes.body.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(world(lantern.pos, lantern_z(lantern)))
                    .with_scale(Vec3::splat(r)),
            ))
            .with_children(|body| {
                // Dark rims top and bottom, in the body's unit space.
                for y in [-0.95, 0.95] {
                    body.spawn((
                        Mesh3d(meshes.cap.clone()),
                        MeshMaterial3d(meshes.cap_material.clone()),
                        Transform::from_xyz(0.0, y, 0.0).with_scale(Vec3::new(0.7, 0.14, 0.7)),
                    ));
                }
            });
    }
}

/// Front lanterns are nearer the camera, so they're drawn over the ones behind.
fn lantern_z(lantern: &Lantern) -> f32 {
    -(lantern.depth as f32 / 255.0)
}

/// Who lit a lantern, for its colour: one seat, or white when sparks tied.
fn lit_color(lantern: &Lantern) -> Option<LinearRgba> {
    let lit = lantern.lit?;
    let mut seats = Seat::ALL.into_iter().filter(|s| lit.by[s.index()]);
    let first = seats.next()?;
    Some(if seats.next().is_some() {
        LinearRgba::WHITE
    } else {
        seat_color(first)
    })
}

fn animate_lanterns(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<SkyLanterns>>,
    mut views: Query<(&LanternView, &mut Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let alpha = blend(&fixed);
    let now = time.elapsed_secs();
    let game = &active.run.game;
    for (view, mut transform) in &mut views {
        let Some(lantern) = game.lanterns.iter().find(|l| l.id == view.id) else {
            continue;
        };
        let was = active
            .previous
            .lanterns
            .iter()
            .find(|l| l.id == view.id)
            .map_or(lantern.pos, |l| l.pos);
        let pos = was.lerp(lantern.pos, alpha);
        transform.translation = world(pos, lantern_z(lantern));
        // A slight sway, for paper in the wind; the hit circle stays where it is.
        transform.rotation = Quat::from_rotation_z(0.08 * (now * 1.3 + view.id as f32).sin());
        let (color, glow) = match lit_color(lantern) {
            // 1v3: cover the solo burned through glows plainly; only hits on the three count.
            Some(_) if game.is_one_vs_three() => (PAPER, 1.2),
            Some(color) => {
                let age = game.tick.saturating_sub(lantern.lit.map_or(0, |l| l.tick)) as f32;
                // A flare as it catches, settling to a steady glow.
                (color, 3.0 + 6.0 * (-age / 10.0).exp())
            }
            None if lantern.is_golden() => (GOLD, 1.4 + 0.5 * (now * 7.0).sin()),
            None => (PAPER, 0.35),
        };
        if let Some(mut material) = materials.get_mut(&view.material) {
            material.emissive = color * glow;
        }
    }
}

fn animate_sights(
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<SkyLanterns>>,
    mut views: Query<(&SightView, &mut Transform, &mut Visibility)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let alpha = blend(&fixed);
    let game = &active.run.game;
    for (view, mut transform, mut visibility) in &mut views {
        let (Some(sight), Some(was)) = (
            game.sights[view.seat.index()],
            active.previous.sights[view.seat.index()],
        ) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *visibility = Visibility::Inherited;
        transform.translation = world(was.pos.lerp(sight.pos, alpha), SIGHT_Z);
        let fizzling = matches!(sight.shot, Some((_, Shot::Miss))) && sight.cooldown > 0;
        let wasted = fizzling
            || (game.is_one_vs_three()
                && matches!(sight.shot, Some((_, Shot::Lit(_))))
                && sight.cooldown > 0);
        // Ready: bright. Just fired: a pulse outwards. Fizzling: grey and small.
        let fired = sight
            .shot
            .map_or(u32::MAX, |(tick, _)| game.tick.saturating_sub(tick));
        let (scale, color) = if wasted {
            (0.75, FIZZLE_GREY * 2.0)
        } else if fired < 8 {
            (
                1.0 + 0.5 * (1.0 - fired as f32 / 8.0),
                seat_color(view.seat) * 8.0,
            )
        } else if sight.cooldown > 0 {
            (0.9, seat_color(view.seat) * 2.0)
        } else {
            (1.0, seat_color(view.seat) * 4.0)
        };
        transform.scale = Vec3::splat(scale);
        if let Some(mut material) = materials.get_mut(&view.ring) {
            material.emissive = color;
        }
    }
}

fn flyer_of(game: &State, seat: Seat) -> Option<&Flyer> {
    game.flyers.iter().find(|f| f.seat == seat)
}

fn animate_flyers(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<SkyLanterns>>,
    mut views: Query<(&FlyerView, &mut Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let alpha = blend(&fixed);
    let now = time.elapsed_secs();
    let game = &active.run.game;
    for (view, mut transform) in &mut views {
        let (Some(flyer), Some(was)) = (
            flyer_of(game, view.seat),
            flyer_of(&active.previous, view.seat),
        ) else {
            continue;
        };
        transform.translation = world(was.body.pos.lerp(flyer.body.pos, alpha), FLYER_Z);
        // Stretched along a dash.
        transform.scale = if flyer.dash > 0 {
            Vec3::new(1.25, 0.85, 1.0)
        } else {
            Vec3::ONE
        };
        let glow = if flyer.flash > 0 {
            // Lit by the solo: blazing, then flickering out.
            let left = flyer.flash as f32 / FLASH_TICKS as f32;
            if left < 0.35 {
                2.0 + 3.0 * (now * 30.0).sin().max(0.0)
            } else {
                8.0 * left
            }
        } else {
            1.2
        };
        if let Some(mut material) = materials.get_mut(&view.material) {
            material.emissive = seat_color(view.seat) * glow;
        }
    }
}

fn animate_folk(
    time: Res<Time>,
    active: Res<ActiveRun<SkyLanterns>>,
    mut folk: Query<(&FolkView, &mut Pose)>,
    mut lights: Query<&mut PointLight>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let game = &active.run.game;
    let now = time.elapsed_secs();
    let winners: Vec<Seat> = game
        .outcome
        .map(|o| o.winners().collect())
        .unwrap_or_default();
    for (view, mut pose) in &mut folk {
        let seat = view.seat;
        let recent = |tick: u32| game.tick.saturating_sub(tick) < REACTION_TICKS;
        let shot = game.sights[seat.index()].and_then(|s| s.shot);
        // A hop for lighting a lantern (in 1v3, for hitting one of the three).
        let scored = match shot {
            Some((tick, Shot::Lit(_))) => recent(tick) && !game.is_one_vs_three(),
            Some((tick, Shot::Flyer(_))) => recent(tick),
            _ => false,
        };
        let was_hit =
            flyer_of(game, seat).is_some_and(|f| f.flash > FLASH_TICKS - REACTION_TICKS as u16);
        *pose = if game.outcome.is_some() {
            if winners.contains(&seat) {
                Pose::Celebrate
            } else {
                Pose::Slump
            }
        } else if was_hit {
            Pose::Slump
        } else if scored {
            Pose::Hop
        } else {
            Pose::Idle
        };
        let glow = 1.6 + 0.3 * (now * 3.0 + seat.0 as f32).sin();
        if let Some(mut material) = materials.get_mut(&view.head_material) {
            material.emissive = seat_color(seat) * glow;
        }
        if let Ok(mut light) = lights.get_mut(view.head_light) {
            light.intensity = 18_000.0 * glow;
        }
    }
}

/// Sounds and sparks, once per event: sparks fired, lanterns lit, flyers hit, dashes, the last
/// seconds and the result. Bots' sparks are quieter, so a person hears their own.
fn react(
    mut commands: Commands,
    active: Res<ActiveRun<SkyLanterns>>,
    seats: Res<Seats>,
    effects: Res<Effects>,
    sfx: Sfx,
) {
    let Some((before, now)) = active.ticked() else {
        return;
    };
    for seat in Seat::ALL {
        let (Some(was), Some(sight)) = (before.sights[seat.index()], now.sights[seat.index()])
        else {
            continue;
        };
        if sight.shot == was.shot {
            continue;
        }
        let Some((_, shot)) = sight.shot else {
            continue;
        };
        let volume = if seats.is_bot(seat) { -8.0 } else { 0.0 };
        let at = world(sight.pos, SIGHT_Z - 0.5);
        match shot {
            Shot::Lit(id) => {
                let lantern = now.lanterns.iter().find(|l| l.id == id);
                let golden = lantern.is_some_and(Lantern::is_golden);
                let cover = now.is_one_vs_three();
                sfx.play_with(
                    if cover {
                        Sound::FalseStart
                    } else {
                        Sound::Ignite
                    },
                    if golden { 1.5 } else { 1.0 },
                    volume,
                );
                let color = if golden { GOLD } else { seat_color(seat) };
                let strength = if cover { 1.5 } else { 4.0 };
                particles::burst(&mut commands, &effects, SCOPE, at, hdr(color, strength));
            }
            Shot::Flyer(_) => {
                sfx.play_with(Sound::Hit, 1.0, volume);
                particles::burst(
                    &mut commands,
                    &effects,
                    SCOPE,
                    at,
                    hdr(seat_color(seat), 6.0),
                );
                particles::burst(&mut commands, &effects, SCOPE, at, hdr(LANTERN_WARM, 4.0));
            }
            Shot::Miss => {
                sfx.play_with(Sound::FalseStart, 1.3, volume.min(-4.0));
                particles::burst(&mut commands, &effects, SCOPE, at, hdr(FIZZLE_GREY, 1.0));
            }
        }
    }
    for flyer in &now.flyers {
        if let Some(was) = before.flyers.iter().find(|f| f.seat == flyer.seat)
            && flyer.dash > 0
            && was.dash == 0
        {
            let volume = if seats.is_bot(flyer.seat) { -8.0 } else { 0.0 };
            sfx.play_with(Sound::Dash, 1.2, volume);
        }
    }
    if now.phase == Phase::Playing {
        let left = now.ticks_left();
        if left > 0 && left <= 5 * TICK_HZ && left % TICK_HZ == 0 && before.tick != now.tick {
            sfx.play(Sound::Tick);
        }
    }
    if before.outcome.is_none() && now.outcome.is_some() {
        sfx.play(Sound::Win);
    }
}

const CAMERA_Y: f32 = -1.4;
/// The sky (plus a margin) and the folk below it must stay on screen.
const FRAMED_WIDTH: f32 = SKY.x * 2.0 + 1.0;
const FRAMED_HEIGHT: f32 = 13.4;

fn camera_transform() -> Transform {
    Transform::from_xyz(0.0, CAMERA_Y, 20.0).looking_at(Vec3::new(0.0, CAMERA_Y, 0.0), Vec3::Y)
}

/// Orthographic and straight on, so the sky on screen is the simulation's sky: a sight over a
/// lantern is over it, at any depth and anywhere on screen.
fn camera_projection() -> Projection {
    Projection::Orthographic(OrthographicProjection {
        scaling_mode: ScalingMode::AutoMin {
            min_width: FRAMED_WIDTH,
            min_height: FRAMED_HEIGHT,
        },
        ..OrthographicProjection::default_3d()
    })
}

fn spawn_hud(mut commands: Commands, active: Res<ActiveRun<SkyLanterns>>) {
    commands
        .spawn((
            SCOPE,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(24.0),
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
                TextFont::from_font_size(48.0),
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
            SCOPE,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(14.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(16.0),
                ..default()
            },
        ))
        .with_children(|row| {
            for seat in playing(game) {
                let border = match team_index(&game.format, seat) {
                    Some(team) => Color::from(TEAM_COLORS[team as usize]),
                    None => Color::NONE,
                };
                row.spawn((
                    Node {
                        width: Val::Px(200.0),
                        padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)),
                        border: UiRect::all(Val::Px(3.0)),
                        ..default()
                    },
                    BorderColor::all(border),
                    BackgroundColor(Color::from(seat_color(seat) * 0.25).with_alpha(0.55)),
                ))
                .with_child((
                    HudText::SeatPanel(seat),
                    Text::new(""),
                    TextFont::from_font_size(17.0),
                    TextColor(Color::WHITE),
                ));
            }
        });
}

fn clock(ticks: u32) -> String {
    let secs = ticks.div_ceil(TICK_HZ);
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn banner_text(active: &ActiveRun<SkyLanterns>) -> (String, String) {
    let run = &active.run;
    let game = &run.game;
    if let Some(banner) = halted_banner(run) {
        return banner;
    }
    match game.phase {
        // The shared countdown (crate::intro) is drawn over this.
        Phase::Intro { .. } => (String::new(), String::new()),
        Phase::Playing => {
            let title = if game.tick < 45 {
                tr!("sky-lanterns-go")
            } else {
                String::new()
            };
            (title, clock(game.ticks_left()))
        }
        Phase::Decided { .. } | Phase::Finished => {
            let outcome = game.outcome.expect("decided games have an outcome");
            let winners: Vec<Seat> = outcome.winners().collect();
            let prompt = if game.phase == Phase::Finished {
                active.continue_prompt()
            } else {
                String::new()
            };
            (i18n::winners(&winners), prompt)
        }
    }
}

fn update_hud(
    active: Res<ActiveRun<SkyLanterns>>,
    seats: Res<Seats>,
    mut texts: Query<(&HudText, &mut Text)>,
) {
    let (title, subtitle) = banner_text(&active);
    let game = &active.run.game;
    let teams = game.team_scores();
    for (hud, mut text) in &mut texts {
        text.0 = match *hud {
            HudText::Banner => title.clone(),
            HudText::SubBanner => subtitle.clone(),
            HudText::SeatPanel(seat) => {
                let who = bot_tag(seats.is_bot(seat));
                let solo = matches!(game.format, Format::OneVsThree { solo } if solo == seat);
                let role = if solo { tr!("solo-tag") } else { String::new() };
                let score = if let Some(flyer) = flyer_of(game, seat) {
                    tr!("sky-lanterns-lit-times", times = flyer.hits)
                } else if solo {
                    tr!(
                        "sky-lanterns-hits",
                        hits = game.solo_hits(),
                        needed = HITS_TO_WIN
                    )
                } else {
                    let points = game.sights[seat.index()].map_or(0, |s| s.score);
                    tr!("sky-lanterns-points", points = points)
                };
                let status = if active.run.controller_lost(seat) {
                    tr!("controller-lost")
                } else if let (Some(outcome), true) = (game.outcome, game.phase != Phase::Playing) {
                    match outcome.placement[seat.index()] {
                        Some(place) => tr!("place", place = place as u32),
                        None => String::new(),
                    }
                } else if let Some(flyer) = flyer_of(game, seat) {
                    if flyer.dash_cooldown == 0 {
                        tr!("sky-lanterns-dash-ready")
                    } else {
                        String::new()
                    }
                } else if let Some(team) = team_index(&game.format, seat) {
                    tr!("sky-lanterns-team", points = teams[team as usize])
                } else {
                    String::new()
                };
                format!("{}{who}{role}\n{score}\n{status}", seat_label(seat))
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sight drawn over a lantern must be over it in the simulation, whatever the lantern's
    /// depth: a parallel projection looking straight down the z axis.
    #[test]
    fn the_sky_is_seen_straight_on() {
        assert!(matches!(camera_projection(), Projection::Orthographic(_)));
        let forward = camera_transform().forward();
        assert!(forward.abs_diff_eq(Vec3::NEG_Z, 1e-6), "{forward}");
        let right = camera_transform().right();
        assert!(right.abs_diff_eq(Vec3::X, 1e-6), "{right}");
    }
}
