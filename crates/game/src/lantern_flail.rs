//! Presentation for Lantern Flail. Reads `ActiveRun<LanternFlail>` only; never writes simulation
//! state. Positions are blended between the last two ticks so motion stays smooth, and a hit-stop
//! (the simulation standing still for a few ticks) simply shows as everything freezing.

use std::collections::VecDeque;
use std::f32::consts::TAU;

use bevy::prelude::*;
use minigame_lantern_flail::{
    BarrelStatus, FALL_TICKS, FOLK_RADIUS, GIANT_GOAL, HIT_MIN, Kind, Lantern, LanternFlail,
    MAX_LANTERN_SPEED, PLAY_TICKS, Phase, RAFT_RADIUS, ROPE_TAUT, SAFE_TICKS, State, Status,
    TRIO_GOAL,
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

pub struct LanternFlailViewPlugin;

impl Plugin for LanternFlailViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            (spawn_scene, spawn_hud)
                .after(MinigameSet::Start)
                .run_if(resource_exists::<ActiveRun<LanternFlail>>),
        )
        .add_systems(
            Update,
            (
                animate_folk,
                (animate_lanterns, animate_ropes).chain(),
                animate_barrels,
                animate_ripples,
                animate_water_lanterns,
                (fit_camera, shake_camera).chain(),
                update_hud,
            )
                .run_if(resource_exists::<ActiveRun<LanternFlail>>),
        )
        .add_systems(
            FixedUpdate,
            react
                .in_set(MinigameSet::React)
                .run_if(resource_exists::<ActiveRun<LanternFlail>>),
        );
    }
}

const SCOPE: DespawnOnExit<AppState> = DespawnOnExit(AppState::Minigame);
/// Team colours for the bolas and the floor discs in team formats.
const TEAM_COLORS: [LinearRgba; 2] = [
    LinearRgba::rgb(1.0, 0.78, 0.3),
    LinearRgba::rgb(0.55, 0.45, 1.0),
];
const WATER_Y: f32 = -0.45;
const SPLASH: LinearRgba = LinearRgba::rgb(0.55, 0.8, 1.0);
/// Where ropes leave the hand, and where lanterns hang, above the raft.
const HAND_Y: f32 = 1.0;
const HANG_Y: f32 = 0.7;
const BEADS: usize = 10;
const GHOSTS: usize = 6;
const WATER_LANTERNS: usize = 9;
const FIREFLY_COUNT: usize = 36;

/// Sounds and sparks, once per event: lunges, hits, clangs, splashes, points, sudden death,
/// the last seconds and the win.
fn react(
    mut commands: Commands,
    active: Res<ActiveRun<LanternFlail>>,
    effects: Res<Effects>,
    sfx: Sfx,
) {
    let Some((before, now)) = active.ticked() else {
        return;
    };
    let mut spark = |pos: Vec3, color: Vec4| {
        particles::burst(&mut commands, &effects, SCOPE, pos, color);
    };
    let mut ripples = Vec::new();
    for seat in now.players.seats() {
        let (was, is) = (before.folk(seat), now.folk(seat));
        let variant = now.tick + seat.0 as u32;
        if was.lunge == 0 && is.lunge > 0 {
            sfx.play_varied(Sound::Dash, variant);
        }
        if is.last_hit != was.last_hit
            && let Some(hit) = is.last_hit
        {
            let power = hit.power as f32 / 10.0;
            // Harder hits sound lower and louder.
            let rate = (1.15 - power / 40.0).clamp(0.7, 1.1) as f64;
            sfx.play_with(Sound::Hit, rate, (power / 3.0 - 4.0).clamp(-8.0, 3.0));
            let pos = world(is.body.pos) + Vec3::Y * 0.8;
            spark(pos, hdr(seat_color(hit.by), 3.0 + power * 0.4));
            if power >= minigame_lantern_flail::HITSTOP_POWER {
                spark(pos, hdr(LANTERN_WARM, 8.0));
            }
        }
        if was.standing() && matches!(is.status, Status::Falling { .. }) {
            sfx.play_varied(Sound::Fall, variant);
            spark(world(is.body.pos) + Vec3::Y * 0.2, hdr(SPLASH, 4.0));
            ripples.push(is.body.pos);
        }
        if matches!(was.status, Status::Waiting { .. }) && is.standing() {
            sfx.play_varied(Sound::Ready, variant);
        }
    }
    for (l, lantern) in now.lanterns.iter().enumerate() {
        if lantern.last_clang != before.lanterns[l].last_clang && lantern.last_clang.is_some() {
            sfx.play_varied(Sound::Rebound, now.tick + l as u32);
            spark(lantern_world(now, lantern), hdr(LinearRgba::WHITE, 4.0));
        }
    }
    for (b, barrel) in now.barrels.iter().enumerate() {
        let was = before.barrels[b].status;
        if was == BarrelStatus::Floating && matches!(barrel.status, BarrelStatus::Sinking { .. }) {
            sfx.play_varied(Sound::Fall, now.tick + 7 + b as u32);
            ripples.push(barrel.body.pos);
        }
    }
    if now.points != before.points {
        sfx.play(Sound::CoinsUp);
    }
    if now.phase == Phase::Playing {
        if !before.sudden_death() && now.sudden_death() {
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
            let folk = now.folk(seat);
            if folk.standing() {
                spark(
                    world(folk.body.pos) + Vec3::Y * 1.8,
                    hdr(seat_color(seat), 6.0),
                );
            }
        }
    }
    for pos in ripples {
        commands.spawn(RippleSpawn(pos));
    }
}

/// Asks [`animate_ripples`] for a ring on the water (spawned there, where meshes are at hand).
#[derive(Component)]
struct RippleSpawn(SimVec2);

#[derive(Component)]
struct Ripple {
    born: f32,
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct FolkView {
    seat: Seat,
    head_material: Handle<StandardMaterial>,
    head_light: Entity,
    /// The footprint disc, glowing while a lunge is ready.
    disc: Entity,
    disc_material: Handle<StandardMaterial>,
    disc_color: LinearRgba,
    /// Grows hotter with damage.
    heat: Entity,
    heat_material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct LanternView {
    index: usize,
    color: LinearRgba,
    material: Handle<StandardMaterial>,
    light: Entity,
    ghosts: [(Entity, Handle<StandardMaterial>); GHOSTS],
    /// Where it was drawn in recent frames, newest first.
    history: VecDeque<Vec3>,
}

#[derive(Component)]
struct RopeBead {
    lantern: usize,
    slot: usize,
    k: usize,
}

#[derive(Component)]
struct BarrelView(usize);

#[derive(Component)]
struct WaterLantern {
    origin: Vec3,
    phase: f32,
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

#[derive(Resource)]
struct FlailMeshes {
    ripple: Handle<Mesh>,
}

fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    active: Res<ActiveRun<LanternFlail>>,
    roster: Res<Roster>,
) {
    commands.insert_resource(ClearColor(NIGHT_SKY));
    commands.insert_resource(GlobalAmbientLight {
        color: MOONLIGHT,
        brightness: 40.0,
        ..default()
    });
    commands.insert_resource(FlailMeshes {
        ripple: meshes.add(Torus::new(0.94, 1.0).mesh().major_resolution(48)),
    });
    let camera = camera_transform(1.0);
    commands.spawn((
        SCOPE,
        night_camera(camera, (20.0, 48.0)),
        CameraRig { base: camera },
    ));
    commands.spawn((
        SCOPE,
        DirectionalLight {
            color: MOONLIGHT,
            illuminance: 450.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-5.0, 12.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // A warm lantern hanging over the raft, so it reads as the brightest thing on screen.
    commands.spawn((
        SCOPE,
        PointLight {
            color: Color::from(LANTERN_WARM),
            intensity: 2_000_000.0,
            range: 30.0,
            radius: 0.5,
            ..default()
        },
        Transform::from_xyz(0.0, 9.0, 0.0),
    ));

    spawn_pond(&mut commands, &mut meshes, &mut materials);
    spawn_raft(&mut commands, &mut meshes, &mut materials);
    spawn_fireflies(
        &mut commands,
        &mut meshes,
        &mut materials,
        AppState::Minigame,
        FIREFLY_COUNT,
        Vec3::new(-12.0, 0.0, -10.0),
        Vec3::new(12.0, 4.0, 8.0),
    );

    let game = &active.run.game;
    for seat in game.playing() {
        spawn_folk(
            &mut commands,
            &mut meshes,
            &mut materials,
            game,
            seat,
            roster.of(seat),
        );
    }
    for (index, lantern) in game.lanterns.iter().enumerate() {
        spawn_lantern(
            &mut commands,
            &mut meshes,
            &mut materials,
            game,
            index,
            lantern,
        );
    }
    spawn_barrels(&mut commands, &mut meshes, &mut materials, game);
}

fn spawn_pond(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    // Dark, glossy water: the lanterns' lights glint in it.
    commands.spawn((
        SCOPE,
        Mesh3d(meshes.add(Cuboid::new(70.0, 0.02, 70.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.03, 0.08, 0.14),
            emissive: LinearRgba::rgb(0.004, 0.012, 0.03),
            perceptual_roughness: 0.2,
            reflectance: 0.7,
            ..default()
        })),
        Transform::from_xyz(0.0, WATER_Y, 0.0),
    ));
    let pad = materials.add(StandardMaterial {
        base_color: Color::srgb(0.04, 0.12, 0.06),
        perceptual_roughness: 0.7,
        ..default()
    });
    let pad_mesh = meshes.add(Cylinder::new(0.5, 0.03));
    for k in 0..14 {
        let angle = noise(k, 0, 40) * TAU;
        let distance = RAFT_RADIUS + 1.5 + noise(k, 0, 41) * 6.0;
        let scale = 0.6 + noise(k, 0, 42) * 0.8;
        commands.spawn((
            SCOPE,
            Mesh3d(pad_mesh.clone()),
            MeshMaterial3d(pad.clone()),
            Transform::from_xyz(
                angle.cos() * distance,
                WATER_Y + 0.02,
                angle.sin() * distance,
            )
            .with_scale(Vec3::new(scale, 1.0, scale)),
        ));
    }
    // Paper lanterns floating on the water round the raft.
    let mesh = meshes.add(Cuboid::new(0.4, 0.45, 0.4));
    let colors = [
        LANTERN_WARM,
        LinearRgba::rgb(1.0, 0.3, 0.55),
        LinearRgba::rgb(0.25, 0.75, 1.0),
    ];
    for k in 0..WATER_LANTERNS as i32 {
        let angle = k as f32 / WATER_LANTERNS as f32 * TAU + noise(k, 0, 43) * 0.4;
        let distance = RAFT_RADIUS + 2.5 + noise(k, 0, 44) * 4.0;
        let origin = Vec3::new(
            angle.cos() * distance,
            WATER_Y + 0.2,
            angle.sin() * distance,
        );
        let color = colors[k as usize % colors.len()];
        commands.spawn((
            SCOPE,
            WaterLantern {
                origin,
                phase: noise(k, 0, 45) * TAU,
            },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(materials.add(emissive_material(color, 2.5))),
            Transform::from_translation(origin),
            children![(
                PointLight {
                    color: Color::from(color),
                    intensity: 60_000.0,
                    range: 6.0,
                    ..default()
                },
                Transform::from_xyz(0.0, 0.3, 0.0),
            )],
        ));
    }
}

fn spawn_raft(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    commands.spawn((
        SCOPE,
        Mesh3d(meshes.add(Cylinder::new(RAFT_RADIUS, 0.5).mesh().resolution(64))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.05, 0.035, 0.025),
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.33, 0.0),
    ));
    // Planks across the raft, cut to its round edge.
    let shades: Vec<_> = [(0.13, 0.085, 0.05), (0.11, 0.07, 0.045), (0.15, 0.1, 0.06)]
        .into_iter()
        .map(|(r, g, b)| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                perceptual_roughness: 0.85,
                ..default()
            })
        })
        .collect();
    let width = 0.62;
    let count = (2.0 * RAFT_RADIUS / width) as i32;
    for i in 0..count {
        let x = -RAFT_RADIUS + width * (i as f32 + 0.5);
        let half = (RAFT_RADIUS * RAFT_RADIUS - x.abs().powi(2))
            .max(0.0)
            .sqrt()
            - 0.08;
        if half < 0.3 {
            continue;
        }
        commands.spawn((
            SCOPE,
            Mesh3d(meshes.add(Cuboid::new(width - 0.06, 0.08, half * 2.0))),
            MeshMaterial3d(shades[(noise(i, 0, 46) * 2.999) as usize].clone()),
            Transform::from_xyz(x, -0.04 + noise(i, 0, 47) * 0.02, 0.0),
        ));
    }
    // The edge glows, so everyone can see how close to the water they are.
    commands.spawn((
        SCOPE,
        Mesh3d(
            meshes.add(
                Torus::new(RAFT_RADIUS - 0.07, RAFT_RADIUS + 0.07)
                    .mesh()
                    .major_resolution(96),
            ),
        ),
        MeshMaterial3d(materials.add(emissive_material(LANTERN_WARM, 3.0))),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
}

fn team_index(format: &Format, seat: Seat) -> Option<u8> {
    match format {
        Format::FreeForAll | Format::Duel { .. } => None,
        _ => format.team(seat),
    }
}

fn folk_scale(game: &State, seat: Seat) -> f32 {
    game.folk(seat).body.radius / FOLK_RADIUS
}

fn spawn_folk(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    game: &State,
    seat: Seat,
    character: Character,
) {
    let folk = game.folk(seat);
    let lantern_folk = spawn_lantern_folk(
        commands,
        meshes,
        materials,
        AppState::Minigame,
        seat,
        character,
        Transform::from_translation(world(folk.body.pos))
            .with_scale(Vec3::splat(folk_scale(game, seat))),
    );
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
            SCOPE,
            Mesh3d(meshes.add(Cylinder::new(folk.body.radius, 0.04))),
            MeshMaterial3d(disc_material.clone()),
            Transform::from_translation(world(folk.body.pos) + Vec3::Y * 0.05),
        ))
        .id();
    let heat_material = materials.add(emissive_material(LANTERN_WARM, 0.0));
    let heat = commands
        .spawn((
            SCOPE,
            Mesh3d(meshes.add(Torus::new(folk.body.radius * 1.1, folk.body.radius * 1.3))),
            MeshMaterial3d(heat_material.clone()),
            Transform::from_translation(world(folk.body.pos) + Vec3::Y * 0.06),
        ))
        .id();
    commands.entity(lantern_folk.root).insert(FolkView {
        seat,
        head_material: lantern_folk.head_material,
        head_light: lantern_folk.head_light,
        disc,
        disc_material,
        disc_color,
        heat,
        heat_material,
    });
}

/// A lantern's own colour: the bolas in its team's, the others in their holder's.
fn lantern_color(game: &State, lantern: &Lantern) -> LinearRgba {
    match lantern.kind {
        Kind::Bolas => TEAM_COLORS[lantern.team as usize % 2],
        _ => lantern.holders[0].map(seat_color).unwrap_or(LANTERN_WARM),
    }
    .mix(
        &LinearRgba::WHITE,
        if game.format == Format::FreeForAll {
            0.0
        } else {
            0.1
        },
    )
}

fn spawn_lantern(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    game: &State,
    index: usize,
    lantern: &Lantern,
) {
    let color = lantern_color(game, lantern);
    let r = lantern.body.radius;
    let material = materials.add(emissive_material(color, 2.0));
    let dark = materials.add(StandardMaterial {
        base_color: Color::srgb(0.05, 0.045, 0.05),
        perceptual_roughness: 0.6,
        metallic: 0.6,
        ..default()
    });
    let light = commands
        .spawn((
            PointLight {
                color: Color::from(color),
                intensity: 60_000.0,
                range: 5.0,
                ..default()
            },
            Transform::default(),
        ))
        .id();
    let ghost_mesh = meshes.add(Sphere::new(r * 0.7).mesh().ico(2).unwrap());
    let ghosts = std::array::from_fn(|_| {
        let ghost_material = materials.add(StandardMaterial {
            base_color: Color::NONE,
            emissive: color * 0.0,
            alpha_mode: AlphaMode::Add,
            unlit: true,
            ..default()
        });
        let ghost = commands
            .spawn((
                SCOPE,
                Mesh3d(ghost_mesh.clone()),
                MeshMaterial3d(ghost_material.clone()),
                Transform::from_translation(lantern_world(game, lantern)),
                Visibility::Hidden,
            ))
            .id();
        (ghost, ghost_material)
    });
    let mut root = commands.spawn((
        SCOPE,
        Transform::from_translation(lantern_world(game, lantern)),
        Visibility::default(),
    ));
    root.add_child(light);
    match lantern.kind {
        Kind::WreckingBall => {
            // An iron ball with a glowing band round it.
            root.with_child((
                Mesh3d(meshes.add(Sphere::new(r).mesh().uv(24, 16))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.12, 0.11, 0.12),
                    emissive: color * 0.25,
                    perceptual_roughness: 0.35,
                    metallic: 0.7,
                    ..default()
                })),
            ));
            root.with_child((
                Mesh3d(meshes.add(Torus::new(r * 0.92, r * 1.08))),
                MeshMaterial3d(material.clone()),
                Transform::from_rotation(Quat::from_rotation_x(TAU / 4.0)),
            ));
            root.with_child((
                Mesh3d(meshes.add(Torus::new(r * 0.92, r * 1.08))),
                MeshMaterial3d(material.clone()),
            ));
        }
        Kind::Flail | Kind::Bolas => {
            // A paper lantern with a dark cap and base.
            root.with_child((
                Mesh3d(meshes.add(Cuboid::new(r * 1.35, r * 1.6, r * 1.35))),
                MeshMaterial3d(material.clone()),
            ));
            for y in [-r * 0.85, r * 0.85] {
                root.with_child((
                    Mesh3d(meshes.add(Cuboid::new(r * 1.1, r * 0.18, r * 1.1))),
                    MeshMaterial3d(dark.clone()),
                    Transform::from_xyz(0.0, y, 0.0),
                ));
            }
        }
    }
    root.insert(LanternView {
        index,
        color,
        material,
        light,
        ghosts,
        history: VecDeque::new(),
    });

    let rope = materials.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.18, 0.1),
        emissive: LANTERN_WARM * 0.25,
        perceptual_roughness: 0.9,
        ..default()
    });
    let bead = meshes.add(Sphere::new(0.05).mesh().ico(1).unwrap());
    for slot in 0..2 {
        if lantern.holders[slot].is_none() {
            continue;
        }
        for k in 0..BEADS {
            commands.spawn((
                SCOPE,
                RopeBead {
                    lantern: index,
                    slot,
                    k,
                },
                Mesh3d(bead.clone()),
                MeshMaterial3d(rope.clone()),
                Transform::default(),
            ));
        }
    }
}

fn spawn_barrels(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    game: &State,
) {
    let r = minigame_lantern_flail::BARREL_RADIUS;
    let wood = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.27, 0.13),
        emissive: LinearRgba::rgb(0.06, 0.03, 0.012),
        perceptual_roughness: 0.75,
        ..default()
    });
    // Glowing hoops, so a barrel stands out against the planks at night.
    let hoop = materials.add(emissive_material(LANTERN_WARM, 1.5));
    let body = meshes.add(Cylinder::new(r, 0.8).mesh().resolution(20));
    let hoop_mesh = meshes.add(Torus::new(r - 0.02, r + 0.03));
    for (index, barrel) in game.barrels.iter().enumerate() {
        commands
            .spawn((
                SCOPE,
                BarrelView(index),
                Transform::from_translation(world(barrel.body.pos)),
                Visibility::default(),
            ))
            .with_children(|b| {
                b.spawn((
                    Mesh3d(body.clone()),
                    MeshMaterial3d(wood.clone()),
                    Transform::from_xyz(0.0, 0.4, 0.0),
                ));
                for y in [0.18, 0.62] {
                    b.spawn((
                        Mesh3d(hoop_mesh.clone()),
                        MeshMaterial3d(hoop.clone()),
                        Transform::from_xyz(0.0, y, 0.0),
                    ));
                }
            });
    }
}

/// Simulation floor coordinates to world space.
fn world(v: SimVec2) -> Vec3 {
    Vec3::new(v.x, 0.0, v.y)
}

fn blend(fixed: &Time<Fixed>) -> f32 {
    fixed.overstep_fraction().clamp(0.0, 1.0)
}

/// How high a lantern flies: a fast one swings out and up on its rope.
fn hang_height(speed: f32) -> f32 {
    HANG_Y + (speed / MAX_LANTERN_SPEED).min(1.0) * 0.35
}

fn lantern_world(_game: &State, lantern: &Lantern) -> Vec3 {
    world(lantern.body.pos) + Vec3::Y * hang_height(lantern.speed())
}

/// Seconds since a tick of play, for one-shot effects.
fn since(game: &State, tick: u32) -> f32 {
    game.tick.saturating_sub(tick) as f32 / TICK_HZ as f32
}

fn facing_rotation(facing: SimVec2) -> Quat {
    Quat::from_rotation_y(facing.x.atan2(facing.y))
}

/// How deep a folk has sunk, and how far it has tipped, while falling in.
fn sinking(status: Status, alpha: f32) -> (f32, f32) {
    match status {
        Status::Falling { t } => {
            let secs = (t as f32 + alpha) / TICK_HZ as f32;
            ((secs * 3.5).min(2.5), (secs * 4.0).min(1.4))
        }
        _ => (0.0, 0.0),
    }
}

fn animate_folk(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternFlail>>,
    mut folk: Query<(&FolkView, &mut Transform, &mut Visibility, &mut Pose)>,
    mut parts: Query<(&mut Transform, &mut Visibility), Without<FolkView>>,
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
    for (view, mut transform, mut visibility, mut pose) in &mut folk {
        let seat = view.seat;
        let is = game.folk(seat);
        let was = active.previous.folk(seat);
        let pos = if was.standing() == is.standing() {
            was.body.pos.lerp(is.body.pos, alpha)
        } else {
            is.body.pos
        };
        let (drop, tumble) = sinking(is.status, alpha);
        // Climbing back on, you blink while nothing can hit you.
        let blinking = is.standing() && is.safe > 0 && (is.safe / 5) % 2 == 1;
        let shown = matches!(is.status, Status::Standing | Status::Falling { .. }) && !blinking;
        *visibility = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let celebrating = winners.contains(&seat) && is.standing();
        *pose = if celebrating {
            Pose::Celebrate
        } else if game.outcome.is_some() {
            Pose::Slump
        } else {
            Pose::Idle
        };
        let mut rotation = facing_rotation(is.facing);
        let mut scale = Vec3::ONE;
        if is.lunge > 0 {
            rotation *= Quat::from_rotation_x(0.45);
            scale = Vec3::new(0.9, 0.95, 1.2);
        }
        if is.reeling && is.standing() {
            rotation *= Quat::from_rotation_x(-0.2);
        }
        if is.stun > 0 {
            rotation *= Quat::from_rotation_z((now * 30.0).sin() * 0.3);
        }
        if tumble > 0.0 {
            rotation *= Quat::from_rotation_x(-tumble);
        }
        transform.translation = world(pos) - Vec3::Y * drop;
        transform.rotation = rotation;
        transform.scale = scale * folk_scale(game, seat);

        let hit_flash = is
            .last_hit
            .map(|hit| (1.0 - since(game, hit.tick) * 3.0).max(0.0))
            .unwrap_or(0.0);
        let glow = match is.status {
            Status::Standing if celebrating => 5.0,
            Status::Standing => 1.8 + hit_flash * 6.0,
            Status::Falling { t } => 1.8 * (1.0 - t as f32 / FALL_TICKS as f32),
            Status::Waiting { .. } | Status::Benched => 0.0,
        };
        if let Some(mut material) = materials.get_mut(&view.head_material) {
            material.emissive = seat_color(seat) * glow;
        }
        if let Ok(mut light) = lights.get_mut(view.head_light) {
            light.intensity = 22_000.0 * glow;
        }

        let on_raft = is.standing();
        if let Some(mut material) = materials.get_mut(&view.disc_material) {
            let ready = is.lunge_cooldown == 0;
            let strength = if ready {
                2.6 + (now * 6.0).sin() * 0.4
            } else {
                0.5
            };
            material.emissive = view.disc_color * strength;
        }
        if let Ok((mut disc, mut disc_visibility)) = parts.get_mut(view.disc) {
            disc.translation = world(pos) + Vec3::Y * 0.05;
            *disc_visibility = if on_raft {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
        // The heat ring: warm when lightly knocked about, red-hot and pulsing when one more
        // good hit sends you flying.
        let heat = (is.damage as f32 / 150.0).min(1.0);
        if let Some(mut material) = materials.get_mut(&view.heat_material) {
            let hot = LANTERN_WARM.mix(&LinearRgba::rgb(1.0, 0.1, 0.05), heat);
            let pulse = if heat > 0.6 {
                1.0 + (now * (6.0 + heat * 8.0)).sin() * 0.5
            } else {
                1.0
            };
            material.emissive = hot * (heat * 7.0 * pulse);
        }
        if let Ok((mut ring, mut ring_visibility)) = parts.get_mut(view.heat) {
            ring.translation = world(pos) + Vec3::Y * 0.07;
            ring.scale = Vec3::splat(1.0 + heat * 0.35);
            *ring_visibility = if on_raft && is.damage > 0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
}

/// Where a holder's hand is drawn this frame.
fn hand_at(active: &ActiveRun<LanternFlail>, seat: Seat, alpha: f32) -> Vec3 {
    let (was, is) = (active.previous.folk(seat), active.run.game.folk(seat));
    let scale = folk_scale(&active.run.game, seat);
    world(was.body.pos.lerp(is.body.pos, alpha)) + Vec3::Y * HAND_Y * scale
}

fn lantern_at(active: &ActiveRun<LanternFlail>, index: usize, alpha: f32) -> Vec3 {
    let (was, is) = (
        &active.previous.lanterns[index],
        &active.run.game.lanterns[index],
    );
    let pos = if was.down == is.down {
        was.body.pos.lerp(is.body.pos, alpha)
    } else {
        is.body.pos
    };
    let speed = was.speed() + (is.speed() - was.speed()) * alpha;
    world(pos) + Vec3::Y * hang_height(speed)
}

fn animate_lanterns(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternFlail>>,
    mut lanterns: Query<(&mut LanternView, &mut Transform, &mut Visibility)>,
    mut ghosts: Query<(&mut Transform, &mut Visibility), Without<LanternView>>,
    mut lights: Query<&mut PointLight>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let alpha = blend(&fixed);
    let game = &active.run.game;
    for (mut view, mut transform, mut visibility) in &mut lanterns {
        let lantern = &game.lanterns[view.index];
        let pos = lantern_at(&active, view.index, alpha);
        let speed = lantern.speed();
        transform.translation = pos;
        // It swings round on the rope, tilting along the way it flies.
        let tilt = (speed / MAX_LANTERN_SPEED).min(1.0) * 0.6;
        let across = Vec3::new(-lantern.body.vel.y, 0.0, lantern.body.vel.x).normalize_or_zero();
        transform.rotation =
            if across == Vec3::ZERO {
                Quat::IDENTITY
            } else {
                Quat::from_axis_angle(across, tilt)
            } * Quat::from_rotation_y(time.elapsed_secs() * 0.5 + view.index as f32);
        *visibility = if lantern.down {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        // Faster is brighter: a loaded swing is easy to see coming.
        let charge = ((speed - HIT_MIN) / (MAX_LANTERN_SPEED - HIT_MIN)).clamp(0.0, 1.0);
        if let Some(mut material) = materials.get_mut(&view.material) {
            material.emissive = view.color * (2.0 + charge * 8.0);
        }
        if let Ok(mut light) = lights.get_mut(view.light) {
            light.intensity = 60_000.0 + charge * 180_000.0;
        }

        if view.history.front() != Some(&pos) {
            view.history.push_front(pos);
            view.history.truncate(GHOSTS * 2 + 1);
        }
        if lantern.down {
            view.history.clear();
        }
        for (k, (ghost, ghost_material)) in view.ghosts.iter().enumerate() {
            let Ok((mut ghost_transform, mut ghost_visibility)) = ghosts.get_mut(*ghost) else {
                continue;
            };
            let sample = view.history.get(k * 2 + 1).copied();
            *ghost_visibility = match sample {
                Some(_) if charge > 0.0 && !lantern.down => Visibility::Inherited,
                _ => Visibility::Hidden,
            };
            if let Some(sample) = sample {
                let fade = 1.0 - (k as f32 + 1.0) / (GHOSTS as f32 + 1.0);
                ghost_transform.translation = sample;
                ghost_transform.scale = Vec3::splat(0.4 + fade * 0.6);
                if let Some(mut material) = materials.get_mut(ghost_material) {
                    material.emissive = view.color * (charge * fade * 3.0);
                }
            }
        }
    }
}

fn animate_ropes(
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternFlail>>,
    mut beads: Query<(&RopeBead, &mut Transform, &mut Visibility)>,
) {
    let alpha = blend(&fixed);
    let game = &active.run.game;
    for (bead, mut transform, mut visibility) in &mut beads {
        let lantern = &game.lanterns[bead.lantern];
        let Some(holder) = lantern.holders[bead.slot] else {
            continue;
        };
        if lantern.down || !game.folk(holder).standing() {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = Visibility::Inherited;
        let hand = hand_at(&active, holder, alpha);
        let end = lantern_at(&active, bead.lantern, alpha);
        let length = lantern.rope_length(bead.slot);
        let span = Vec2::new(end.x - hand.x, end.z - hand.z).length();
        // A slack rope sags; a taut one runs straight.
        let slack = (1.0 - span / (length * ROPE_TAUT)).max(0.0);
        let sag = slack * length * 0.5;
        let t = (bead.k as f32 + 0.5) / BEADS as f32;
        let mut pos = hand.lerp(end, t) - Vec3::Y * (sag * 4.0 * t * (1.0 - t));
        pos.y = pos.y.max(0.04);
        transform.translation = pos;
    }
}

fn animate_barrels(
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternFlail>>,
    mut barrels: Query<(&BarrelView, &mut Transform, &mut Visibility)>,
) {
    let alpha = blend(&fixed);
    let game = &active.run.game;
    for (view, mut transform, mut visibility) in &mut barrels {
        let (was, is) = (&active.previous.barrels[view.0], &game.barrels[view.0]);
        let pos = if was.status == is.status || matches!(is.status, BarrelStatus::Sinking { .. }) {
            was.body.pos.lerp(is.body.pos, alpha)
        } else {
            is.body.pos
        };
        let (drop, tip) = match is.status {
            BarrelStatus::Sinking { t } => {
                let secs = (t as f32 + alpha) / TICK_HZ as f32;
                ((secs * 2.5).min(2.0), (secs * 3.0).min(1.2))
            }
            _ => (0.0, 0.0),
        };
        *visibility = if matches!(is.status, BarrelStatus::Gone { .. }) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        // Barrels slide upright; turning them with where they are reads as rolling about.
        transform.translation = world(pos) - Vec3::Y * drop;
        transform.rotation =
            Quat::from_rotation_y((pos.x - pos.y) * 0.8) * Quat::from_rotation_x(tip);
    }
}

fn animate_ripples(
    mut commands: Commands,
    time: Res<Time>,
    meshes: Option<Res<FlailMeshes>>,
    requests: Query<(Entity, &RippleSpawn)>,
    mut ripples: Query<(Entity, &Ripple, &mut Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let now = time.elapsed_secs();
    let Some(meshes) = meshes else {
        return;
    };
    for (entity, request) in &requests {
        commands.entity(entity).despawn();
        // The ring goes where they went in: just outside the edge.
        let at = request.0.normalize_or_zero() * (RAFT_RADIUS + 0.4);
        let material = materials.add(emissive_material(SPLASH, 3.0));
        commands.spawn((
            SCOPE,
            Ripple {
                born: now,
                material: material.clone(),
            },
            Mesh3d(meshes.ripple.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(world(at) + Vec3::Y * (WATER_Y + 0.03))
                .with_scale(Vec3::new(0.3, 1.0, 0.3)),
        ));
    }
    for (entity, ripple, mut transform) in &mut ripples {
        let age = now - ripple.born;
        if age > 1.2 {
            commands.entity(entity).despawn();
            continue;
        }
        let size = 0.3 + age * 2.0;
        transform.scale = Vec3::new(size, 1.0, size);
        if let Some(mut material) = materials.get_mut(&ripple.material) {
            material.emissive = SPLASH * (3.0 * (1.0 - age / 1.2));
        }
    }
}

fn animate_water_lanterns(time: Res<Time>, mut lanterns: Query<(&WaterLantern, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (lantern, mut transform) in &mut lanterns {
        transform.translation =
            lantern.origin + Vec3::new(0.0, (t * 1.1 + lantern.phase).sin() * 0.05, 0.0);
        transform.rotation = Quat::from_rotation_z((t * 0.9 + lantern.phase).sin() * 0.08)
            * Quat::from_rotation_y(t * 0.15 + lantern.phase);
    }
}

const CAMERA_TARGET: Vec3 = Vec3::new(0.0, -0.4, 1.0);
const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 14.0, 10.4);
/// Half-width of the raft (plus room for lanterns swung out over the water) kept on screen.
const FRAMED_HALF_WIDTH: f32 = RAFT_RADIUS + 1.8;

fn camera_transform(pull_back: f32) -> Transform {
    Transform::from_translation(CAMERA_TARGET + CAMERA_OFFSET * pull_back)
        .looking_at(CAMERA_TARGET, Vec3::Y)
}

/// Pull the camera back on narrow windows so the whole raft stays in frame.
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

/// A kick on every hit, harder for harder hits.
fn shake_camera(
    active: Res<ActiveRun<LanternFlail>>,
    time: Res<Time>,
    mut camera: Query<(&CameraRig, &mut Transform)>,
) {
    let game = &active.run.game;
    let strength = game
        .players
        .seats()
        .filter_map(|s| game.folk(s).last_hit)
        .map(|hit| {
            let power = hit.power as f32 / 10.0;
            ((0.02 + power * 0.012) - since(game, hit.tick) * 0.6).max(0.0)
        })
        .fold(0.0, f32::max);
    let t = time.elapsed_secs();
    for (rig, mut transform) in &mut camera {
        *transform = rig.base;
        transform.translation += Vec3::new((t * 91.0).sin(), (t * 73.0).cos(), 0.0) * strength;
    }
}

fn spawn_hud(mut commands: Commands, active: Res<ActiveRun<LanternFlail>>) {
    commands
        .spawn((
            SCOPE,
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
            SCOPE,
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
            for seat in game.playing() {
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

fn clock(ticks: u32) -> String {
    let secs = ticks.div_ceil(TICK_HZ);
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// The score line under the banner: the clock, and in 1v3 how near each side is to its goal.
fn scoreline(game: &State) -> String {
    let time = clock(game.ticks_left());
    match game.format {
        Format::OneVsThree { .. } => format!(
            "{time}   {}",
            tr!(
                "lantern-flail-solo-score",
                giant = game.points[0],
                giant_goal = GIANT_GOAL,
                trio = game.points[1],
                trio_goal = TRIO_GOAL
            )
        ),
        _ => time,
    }
}

fn banner_text(active: &ActiveRun<LanternFlail>) -> (String, String) {
    let run = &active.run;
    let game = &run.game;
    if let Some(banner) = halted_banner(run) {
        return banner;
    }
    match game.phase {
        // The shared countdown (crate::intro) is drawn over this.
        Phase::Intro { .. } => (String::new(), String::new()),
        Phase::Playing => {
            let latest = game
                .playing()
                .filter_map(|s| game.fell_at[s.index()].map(|t| (t, s)))
                .max();
            let title = match latest {
                Some((tick, seat)) if since(game, tick) < 1.5 => {
                    // Credited to whoever hit them last, if it was recent enough to count.
                    let by = game
                        .folk(seat)
                        .last_hit
                        .filter(|hit| hit.tick + minigame_lantern_flail::CREDIT_TICKS > tick);
                    match by {
                        Some(hit) => tr!(
                            "lantern-flail-knocked",
                            by = seat_label(hit.by),
                            player = seat_label(seat)
                        ),
                        None => tr!("lantern-flail-fell", player = seat_label(seat)),
                    }
                }
                _ if game.sudden_death() => tr!("lantern-flail-sudden-death"),
                _ if game.tick < 45 => tr!("lantern-flail-go"),
                _ => String::new(),
            };
            let sub = if game.sudden_death() {
                tr!("lantern-flail-sudden-death-sub")
            } else {
                scoreline(game)
            };
            (title, sub)
        }
        Phase::Decided { .. } | Phase::Finished => {
            let outcome = game.outcome.expect("decided games have an outcome");
            let winners: Vec<Seat> = outcome.winners().collect();
            let title = if winners.len() == game.playing().count() {
                tr!("lantern-flail-everyone")
            } else {
                i18n::winners(&winners)
            };
            let prompt = if game.phase == Phase::Finished {
                active.continue_prompt()
            } else {
                scoreline(game)
            };
            (title, prompt)
        }
    }
}

fn update_hud(
    active: Res<ActiveRun<LanternFlail>>,
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
                let folk = game.folk(seat);
                let points = match game.format {
                    Format::FreeForAll => game.points[seat.index()],
                    _ => game.knockoffs[seat.index()],
                };
                let status = if active.run.controller_lost(seat) {
                    tr!("controller-lost")
                } else if let (Some(outcome), true) = (game.outcome, game.phase != Phase::Playing) {
                    match outcome.placement[seat.index()] {
                        Some(place) => tr!("place", place = place as u32),
                        None => String::new(),
                    }
                } else {
                    match folk.status {
                        Status::Falling { .. } | Status::Waiting { .. } => {
                            tr!("lantern-flail-in-pond")
                        }
                        Status::Standing if folk.safe > SAFE_TICKS / 2 => {
                            tr!("lantern-flail-safe")
                        }
                        _ => tr!("lantern-flail-damage", damage = folk.damage as u32),
                    }
                };
                format!(
                    "{}{who}{role}\n{}\n{status}",
                    seat_label(seat),
                    tr!("lantern-flail-points", points = points)
                )
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
    }
}
