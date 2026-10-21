//! Presentation for Musical Stools. Reads `ActiveRun<MusicalStools>` only; never writes
//! simulation state. Positions are blended between the last two ticks so motion stays smooth.
//! The music follows the simulation: it plays while the simulation says it does, slows down
//! through the warning and stops on the stop's tick. The stop also shows on screen: the fence
//! drops, the lights go down and the free stools blaze.

use std::f32::consts::TAU;

use bevy::prelude::*;
use minigame_musical_stools::{
    CAT_ROUNDS_TO_WIN, DASH_COOLDOWN, Dancer, FENCE_RADIUS, FLOOR_RADIUS, MusicalStools, Phase,
    SIT_REACH, STOOL_RADIUS, STOOL_RING, Side, State, stool_position,
};
use sim_core::physics::Vec2 as SimVec2;
use sim_core::{Format, Seat, TICK_HZ};

use crate::audio::{MusicPlayer, Sfx, Sound};
use crate::characters::{Character, Pose, Roster};
use crate::flow::AppState;
use crate::i18n::{self, bot_tag, halted_banner};
use crate::palette::{LANTERN_WARM, MOONLIGHT, NIGHT_SKY, seat_color, seat_label};
use crate::particles::{self, Effects, hdr};
use crate::runner::{ActiveRun, MinigameSet, Seats};
use crate::scenery::{emissive_material, night_camera, spawn_fireflies, spawn_lantern_folk};
use crate::tr;

pub struct MusicalStoolsViewPlugin;

impl Plugin for MusicalStoolsViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            (spawn_scene, spawn_hud)
                .after(MinigameSet::Start)
                .run_if(resource_exists::<ActiveRun<MusicalStools>>),
        )
        .add_systems(OnExit(AppState::Minigame), stop_music)
        .add_systems(
            Update,
            (
                drive_music,
                animate_arena,
                animate_fence,
                animate_dancers,
                fit_camera,
                update_hud,
            )
                .run_if(resource_exists::<ActiveRun<MusicalStools>>),
        )
        .add_systems(
            FixedUpdate,
            react
                .in_set(MinigameSet::React)
                .run_if(resource_exists::<ActiveRun<MusicalStools>>),
        );
    }
}

const SCOPE: DespawnOnExit<AppState> = DespawnOnExit(AppState::Minigame);
/// Team colours for the floor discs in team formats (players keep their own colours elsewhere).
const TEAM_COLORS: [LinearRgba; 2] = [
    LinearRgba::rgb(1.0, 0.78, 0.3),
    LinearRgba::rgb(0.55, 0.45, 1.0),
];
const STOOL_HEIGHT: f32 = 0.45;
/// The dancers' floor discs float this high, clear of the turntable's top (0.08) and spokes
/// (0.105), so they never z-fight with it.
const DISC_Y: f32 = 0.14;
const TURNTABLE_RADIUS: f32 = STOOL_RING + 0.9;
const FENCE_POSTS: usize = 28;
const FIREFLY_COUNT: usize = 30;
const OVERHEAD_INTENSITY: f32 = 2_000_000.0;
/// The tune's tempo, for dancing to it.
const BEATS_PER_SECOND: f32 = 160.0 / 60.0;
/// How slow the music gets at the end of the warning.
const SLOWEST_RATE: f64 = 0.6;
const STOOL_FREE: LinearRgba = LinearRgba::rgb(1.0, 0.82, 0.45);
const DIM: LinearRgba = LinearRgba::rgb(0.45, 0.45, 0.5);

/// Sounds and sparks, once per event: the stop, dashes, hits, sitting, catches and the results.
fn react(
    mut commands: Commands,
    active: Res<ActiveRun<MusicalStools>>,
    effects: Res<Effects>,
    sfx: Sfx,
) {
    let Some((before, now)) = active.ticked() else {
        return;
    };
    let mut spark = |pos: Vec3, color: Vec4| {
        particles::burst(&mut commands, &effects, SCOPE, pos, color);
    };
    if before.stopped_at.is_none() && now.stopped_at.is_some() {
        sfx.play(Sound::Go);
    }
    for seat in now.playing() {
        let (was, is) = (before.dancer(seat), now.dancer(seat));
        let variant = now.tick + seat.0 as u32;
        if was.dash == 0 && is.dash > 0 {
            sfx.play_varied(Sound::Dash, variant);
        }
        if is.last_hit != was.last_hit
            && let Some(by) = is.last_hit_by
        {
            sfx.play_varied(Sound::Hit, variant);
            let contact = world(is.body.pos.lerp(now.dancer(by).body.pos, 0.5)) + Vec3::Y * 0.7;
            spark(contact, hdr(seat_color(by), 5.0));
        }
        if !was.seated() && is.seated() {
            sfx.play_varied(Sound::CoinsUp, variant);
            spark(
                world(is.body.pos) + Vec3::Y * (STOOL_HEIGHT + 0.4),
                hdr(seat_color(seat), 6.0),
            );
        }
        if !was.caught && is.caught {
            sfx.play_varied(Sound::Fall, variant);
        }
    }
    let scored =
        !matches!(before.phase, Phase::Result { .. }) && matches!(now.phase, Phase::Result { .. });
    if scored && now.outcome.is_none() {
        let left_out = now
            .sitters()
            .any(|s| !now.dancer(s).seated() && !now.dancer(s).caught);
        match now.round_winner {
            Some(_) => sfx.play(Sound::RoundWin),
            None if left_out => sfx.play(Sound::TooSlow),
            None => {}
        }
    }
    if before.outcome.is_none()
        && let Some(outcome) = now.outcome
    {
        sfx.play(Sound::Win);
        for seat in outcome.winners() {
            spark(
                world(now.dancer(seat).body.pos) + Vec3::Y * 1.8,
                hdr(seat_color(seat), 6.0),
            );
        }
    }
}

/// Play the tune while the simulation's music plays, slowing it through the warning.
fn drive_music(
    active: Res<ActiveRun<MusicalStools>>,
    music: MusicPlayer,
    mut playing: Local<Option<(u32, u8)>>,
    mut rate: Local<f64>,
) {
    let game = &active.run.game;
    let wanted = game.music_playing() && !active.run.halted();
    let round = (active.generation, game.round);
    if wanted && *playing != Some(round) {
        music.play_dance();
        *playing = Some(round);
        *rate = 1.0;
    } else if !wanted && playing.is_some() {
        music.stop();
        *playing = None;
    }
    if let Phase::Dancing { t } = game.phase
        && wanted
    {
        let target = if game.warning() && game.warn_ticks > 0 {
            let into = (t + game.warn_ticks).saturating_sub(game.dance_ticks) as f64;
            1.0 - (1.0 - SLOWEST_RATE) * (into / game.warn_ticks as f64)
        } else {
            1.0
        };
        if (target - *rate).abs() > 0.01 {
            music.set_rate(target);
            *rate = target;
        }
    }
}

fn stop_music(music: MusicPlayer) {
    music.stop();
}

#[derive(Component)]
struct Turntable;

#[derive(Component)]
struct StoolView {
    index: usize,
    material: Handle<StandardMaterial>,
    /// A ring on the floor showing how close you must get, lit while the stool is free to take.
    ring: Entity,
}

#[derive(Component)]
struct Fence {
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct DancerView {
    seat: Seat,
    head_material: Handle<StandardMaterial>,
    head_light: Entity,
    disc: Entity,
    disc_material: Handle<StandardMaterial>,
    disc_color: LinearRgba,
}

#[derive(Component)]
struct CameraRig;

/// The warm light over the floor, which goes down when the music stops.
#[derive(Component)]
struct Overhead;

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
    active: Res<ActiveRun<MusicalStools>>,
    roster: Res<Roster>,
) {
    commands.insert_resource(ClearColor(NIGHT_SKY));
    commands.insert_resource(GlobalAmbientLight {
        color: MOONLIGHT,
        brightness: 40.0,
        ..default()
    });
    commands.spawn((
        SCOPE,
        night_camera(camera_transform(1.0), (18.0, 46.0)),
        CameraRig,
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
    commands.spawn((
        SCOPE,
        Overhead,
        PointLight {
            color: Color::from(LANTERN_WARM),
            intensity: OVERHEAD_INTENSITY,
            range: 30.0,
            radius: 0.5,
            ..default()
        },
        Transform::from_xyz(0.0, 9.0, 0.0),
    ));

    let game = &active.run.game;
    spawn_floor(&mut commands, &mut meshes, &mut materials);
    spawn_stools(&mut commands, &mut meshes, &mut materials, game);
    spawn_fence(&mut commands, &mut meshes, &mut materials);
    spawn_fireflies(
        &mut commands,
        &mut meshes,
        &mut materials,
        AppState::Minigame,
        FIREFLY_COUNT,
        Vec3::new(-11.0, -1.0, -9.0),
        Vec3::new(11.0, 4.0, 7.0),
    );
    for seat in game.playing() {
        spawn_dancer(
            &mut commands,
            &mut meshes,
            &mut materials,
            game,
            seat,
            roster.of(seat),
        );
    }
}

fn spawn_floor(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    commands.spawn((
        SCOPE,
        Mesh3d(meshes.add(Cylinder::new(FLOOR_RADIUS, 0.6).mesh().resolution(72))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.075, 0.065, 0.08),
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.3, 0.0),
    ));
    // A ring of floorboards between the fence and the wall, so the dance floor reads as one.
    let board = meshes.add(Cuboid::new(0.9, 0.04, 0.5));
    let shades: Vec<_> = [(0.13, 0.1, 0.09), (0.1, 0.08, 0.075)]
        .into_iter()
        .map(|(r, g, b)| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                perceptual_roughness: 0.8,
                ..default()
            })
        })
        .collect();
    for ring in 0..4 {
        let radius = FENCE_RADIUS + 0.6 + ring as f32 * 0.85;
        let count = (radius * TAU / 0.95) as usize;
        for k in 0..count {
            let angle = (k as f32 + ring as f32 * 0.5) / count as f32 * TAU;
            commands.spawn((
                SCOPE,
                Mesh3d(board.clone()),
                MeshMaterial3d(shades[(k + ring) % 2].clone()),
                Transform::from_xyz(angle.cos() * radius, 0.0, angle.sin() * radius)
                    .with_rotation(Quat::from_rotation_y(-angle + TAU / 4.0)),
            ));
        }
    }
    // The wall round the floor.
    commands.spawn((
        SCOPE,
        Mesh3d(
            meshes.add(
                Torus::new(FLOOR_RADIUS - 0.08, FLOOR_RADIUS + 0.08)
                    .mesh()
                    .major_resolution(96),
            ),
        ),
        MeshMaterial3d(materials.add(emissive_material(LANTERN_WARM, 2.5))),
        Transform::from_xyz(0.0, 0.08, 0.0),
    ));
    // The turntable: a disc with spokes, so its spin shows.
    let spoke = meshes.add(Cuboid::new(TURNTABLE_RADIUS * 1.9, 0.03, 0.12));
    let spoke_material = materials.add(emissive_material(LinearRgba::rgb(0.5, 0.3, 0.9), 1.2));
    commands
        .spawn((
            SCOPE,
            Turntable,
            Mesh3d(meshes.add(Cylinder::new(TURNTABLE_RADIUS, 0.1).mesh().resolution(64))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.12, 0.08, 0.16),
                perceptual_roughness: 0.6,
                ..default()
            })),
            Transform::from_xyz(0.0, 0.03, 0.0),
        ))
        .with_children(|table| {
            for k in 0..3 {
                table.spawn((
                    Mesh3d(spoke.clone()),
                    MeshMaterial3d(spoke_material.clone()),
                    Transform::from_xyz(0.0, 0.06, 0.0)
                        .with_rotation(Quat::from_rotation_y(k as f32 * TAU / 6.0)),
                ));
            }
        });
}

fn spawn_stools(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    game: &State,
) {
    let leg = meshes.add(Cylinder::new(STOOL_RADIUS * 0.45, STOOL_HEIGHT));
    let seat = meshes.add(Cylinder::new(STOOL_RADIUS, 0.12));
    let leg_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.1, 0.07, 0.05),
        perceptual_roughness: 0.8,
        ..default()
    });
    let ring_mesh = meshes.add(
        Torus::new(SIT_REACH + 0.02, SIT_REACH + 0.1)
            .mesh()
            .major_resolution(48),
    );
    let ring_material = materials.add(emissive_material(STOOL_FREE, 5.0));
    for index in 0..game.stool_count {
        let material = materials.add(emissive_material(STOOL_FREE, 2.0));
        let ring = commands
            .spawn((
                Mesh3d(ring_mesh.clone()),
                MeshMaterial3d(ring_material.clone()),
                Transform::from_xyz(0.0, 0.1, 0.0),
                Visibility::Hidden,
            ))
            .id();
        commands
            .spawn((
                SCOPE,
                StoolView {
                    index,
                    material: material.clone(),
                    ring,
                },
                Transform::from_translation(world(game.stool(index))),
                Visibility::default(),
            ))
            .add_child(ring)
            .with_children(|stool| {
                stool.spawn((
                    Mesh3d(leg.clone()),
                    MeshMaterial3d(leg_material.clone()),
                    Transform::from_xyz(0.0, STOOL_HEIGHT / 2.0, 0.0),
                ));
                stool.spawn((
                    Mesh3d(seat.clone()),
                    MeshMaterial3d(material),
                    Transform::from_xyz(0.0, STOOL_HEIGHT, 0.0),
                ));
            });
    }
}

fn spawn_fence(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let material = materials.add(emissive_material(LinearRgba::rgb(0.55, 0.45, 1.0), 3.0));
    let post = meshes.add(Cuboid::new(0.09, 0.55, 0.09));
    commands
        .spawn((
            SCOPE,
            Fence {
                material: material.clone(),
            },
            Transform::default(),
            Visibility::default(),
        ))
        .with_children(|fence| {
            fence.spawn((
                Mesh3d(
                    meshes.add(
                        Torus::new(FENCE_RADIUS - 0.04, FENCE_RADIUS + 0.04)
                            .mesh()
                            .major_resolution(96),
                    ),
                ),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(0.0, 0.5, 0.0),
            ));
            for k in 0..FENCE_POSTS {
                let angle = k as f32 / FENCE_POSTS as f32 * TAU;
                fence.spawn((
                    Mesh3d(post.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(
                        angle.cos() * FENCE_RADIUS,
                        0.27,
                        angle.sin() * FENCE_RADIUS,
                    ),
                ));
            }
        });
}

fn team_index(format: &Format, seat: Seat) -> Option<u8> {
    match format {
        Format::FreeForAll | Format::Duel { .. } => None,
        _ => format.team(seat),
    }
}

fn spawn_dancer(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    game: &State,
    seat: Seat,
    character: Character,
) {
    let dancer = game.dancer(seat);
    let scale = dancer.body.radius / minigame_musical_stools::RADIUS;
    let folk = spawn_lantern_folk(
        commands,
        meshes,
        materials,
        AppState::Minigame,
        seat,
        character,
        Transform::from_translation(world(dancer.body.pos)).with_scale(Vec3::splat(scale)),
    );
    if dancer.cat {
        // Cat ears, so the solo reads as the cat at a glance.
        let ear = meshes.add(Cone::new(0.13, 0.3));
        let ear_material = materials.add(emissive_material(seat_color(seat), 3.0));
        for side in [-1.0, 1.0] {
            let ear = commands
                .spawn((
                    Mesh3d(ear.clone()),
                    MeshMaterial3d(ear_material.clone()),
                    Transform::from_xyz(side * 0.24, 1.98, 0.0)
                        .with_rotation(Quat::from_rotation_z(-side * 0.3)),
                ))
                .id();
            commands.entity(folk.root).add_child(ear);
        }
    }
    // The disc under each dancer is their footprint; it glows when a dash is ready.
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
            Mesh3d(meshes.add(Cylinder::new(dancer.body.radius, 0.04))),
            MeshMaterial3d(disc_material.clone()),
            Transform::from_translation(world(dancer.body.pos) + Vec3::Y * DISC_Y),
        ))
        .id();
    commands.entity(folk.root).insert(DancerView {
        seat,
        head_material: folk.head_material,
        head_light: folk.head_light,
        disc,
        disc_material,
        disc_color,
    });
}

/// Simulation floor coordinates to world space.
fn world(v: SimVec2) -> Vec3 {
    Vec3::new(v.x, 0.0, v.y)
}

fn blend(fixed: &Time<Fixed>) -> f32 {
    fixed.overstep_fraction().clamp(0.0, 1.0)
}

/// Ticks since a play tick, in seconds, for one-shot effects.
fn since(game: &State, tick: u32) -> f32 {
    game.tick.saturating_sub(tick) as f32 / TICK_HZ as f32
}

/// How far the fence has sunk into the floor: 0 while it's up, 1 once it's down.
fn fence_sunk(game: &State) -> f32 {
    match (game.fence(), game.stopped_at) {
        (Some(_), _) => 0.0,
        (None, Some(stop)) => (since(game, stop) * 6.0).min(1.0),
        (None, None) => 1.0,
    }
}

fn animate_arena(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<MusicalStools>>,
    mut turntable: Query<&mut Transform, With<Turntable>>,
    mut stools: Query<(&StoolView, &mut Transform), Without<Turntable>>,
    mut rings: Query<&mut Visibility, Without<StoolView>>,
    mut overhead: Query<&mut PointLight, With<Overhead>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let game = &active.run.game;
    let alpha = blend(&fixed);
    let now = time.elapsed_secs();
    let angle = active.previous.turntable.angle
        + (game.turntable.angle - active.previous.turntable.angle) * alpha;
    for mut transform in &mut turntable {
        transform.rotation = Quat::from_rotation_y(-angle);
    }
    let scramble = matches!(game.phase, Phase::Scramble { .. });
    for mut light in &mut overhead {
        light.intensity = OVERHEAD_INTENSITY * if scramble { 0.25 } else { 1.0 };
    }
    for (stool, mut transform) in &mut stools {
        transform.translation = world(stool_position(angle, stool.index, game.stool_count));
        let glow = match game.stools[stool.index] {
            Some(seat) => seat_color(seat) * 3.0,
            // Free stools blaze once the music stops.
            None if scramble => STOOL_FREE * (6.0 + (now * 14.0).sin() * 2.0),
            None if game.music_playing() => STOOL_FREE * 1.6,
            None => STOOL_FREE * 0.8,
        };
        if let Some(mut material) = materials.get_mut(&stool.material) {
            material.emissive = glow;
        }
        if let Ok(mut ring) = rings.get_mut(stool.ring) {
            *ring = if scramble && game.stools[stool.index].is_none() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
}

/// The fence rises between rounds, pulses to the beat, and sinks into the floor at the stop.
fn animate_fence(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<MusicalStools>>,
    mut fence: Query<(&Fence, &mut Transform, &mut Visibility)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let game = &active.run.game;
    let now = time.elapsed_secs();
    let shown = game.fence().map(|r| {
        let before = active.previous.fence().unwrap_or(r);
        before + (r - before) * blend(&fixed)
    });
    let sunk = fence_sunk(game);
    for (fence, mut transform, mut visibility) in &mut fence {
        let radius = shown.unwrap_or(FENCE_RADIUS);
        transform.scale = Vec3::new(radius / FENCE_RADIUS, 1.0, radius / FENCE_RADIUS);
        transform.translation.y = -0.6 * sunk;
        *visibility = if sunk >= 1.0 || radius < 0.05 {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if let Some(mut material) = materials.get_mut(&fence.material) {
            let beat = (now * BEATS_PER_SECOND * TAU).sin().max(0.0);
            let strength = if game.music_playing() {
                2.5 + beat * 1.5
            } else {
                2.0
            };
            material.emissive = LinearRgba::rgb(0.55, 0.45, 1.0) * strength;
        }
    }
}

fn facing_rotation(facing: SimVec2) -> Quat {
    Quat::from_rotation_y(facing.x.atan2(facing.y))
}

fn animate_dancers(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<MusicalStools>>,
    mut dancers: Query<(&DancerView, &mut Transform, &mut Pose)>,
    mut discs: Query<&mut Transform, Without<DancerView>>,
    mut lights: Query<&mut PointLight>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let alpha = blend(&fixed);
    let now = time.elapsed_secs();
    let game = &active.run.game;
    let winners: Vec<Seat> = match (game.outcome, game.phase) {
        (Some(outcome), Phase::Finished) | (Some(outcome), Phase::Result { .. }) => {
            outcome.winners().collect()
        }
        _ => Vec::new(),
    };
    for (view, mut transform, mut pose) in &mut dancers {
        let seat = view.seat;
        let dancer: &Dancer = game.dancer(seat);
        let before: &Dancer = active.previous.dancer(seat);
        let pos = if dancer.seated() {
            dancer.body.pos
        } else {
            before.body.pos.lerp(dancer.body.pos, alpha)
        };
        let base_scale = dancer.body.radius / minigame_musical_stools::RADIUS;
        let celebrating = winners.contains(&seat);
        let just_sat = dancer.sat_at.is_some_and(|tick| since(game, tick) < 0.6);
        *pose = if celebrating || just_sat {
            Pose::Celebrate
        } else if dancer.caught || (game.outcome.is_some() && !winners.is_empty()) {
            Pose::Slump
        } else {
            Pose::Idle
        };
        let mut rotation = if dancer.seated() {
            // Sit facing out from the middle.
            facing_rotation(dancer.body.pos.normalize_or_zero())
        } else {
            facing_rotation(dancer.facing)
        };
        let mut lift = 0.0;
        if game.music_playing() && dancer.moving() {
            // Bob and sway to the beat.
            let beat = now * BEATS_PER_SECOND * TAU + seat.0 as f32;
            lift = beat.sin().abs() * 0.12;
            rotation *= Quat::from_rotation_z((beat * 0.5).sin() * 0.12);
        }
        if dancer.seated() {
            lift = STOOL_HEIGHT + 0.06;
        }
        if dancer.dash > 0 {
            rotation *= Quat::from_rotation_x(0.45);
        }
        if dancer.stun > 0 {
            rotation *= Quat::from_rotation_z((now * 30.0).sin() * 0.25);
        }
        transform.translation = world(pos) + Vec3::Y * lift;
        transform.rotation = rotation;
        transform.scale = Vec3::splat(base_scale);

        let hit_flash = dancer
            .last_hit
            .map(|tick| (1.0 - since(game, tick) * 3.0).max(0.0))
            .unwrap_or(0.0);
        let glow = if dancer.caught {
            0.4
        } else if celebrating || dancer.seated() {
            4.0
        } else {
            1.8 + hit_flash * 6.0
        };
        let color = if dancer.caught { DIM } else { seat_color(seat) };
        if let Some(mut material) = materials.get_mut(&view.head_material) {
            material.emissive = color * glow;
        }
        if let Ok(mut light) = lights.get_mut(view.head_light) {
            light.intensity = 22_000.0 * glow;
        }
        let cooldown = if dancer.cat {
            minigame_musical_stools::CAT_DASH_COOLDOWN
        } else {
            DASH_COOLDOWN
        };
        if let Some(mut material) = materials.get_mut(&view.disc_material) {
            let strength = if dancer.caught || dancer.seated() {
                0.3
            } else if dancer.dash_cooldown == 0 {
                3.0 + (now * 6.0).sin() * 0.5
            } else {
                0.4 + 1.0 - dancer.dash_cooldown as f32 / cooldown as f32
            };
            material.emissive = view.disc_color * strength;
        }
        if let Ok(mut disc) = discs.get_mut(view.disc) {
            disc.translation =
                world(pos) + Vec3::Y * (DISC_Y + if dancer.seated() { STOOL_HEIGHT } else { 0.0 });
        }
    }
}

const CAMERA_TARGET: Vec3 = Vec3::new(0.0, -0.4, 0.9);
const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 11.2, 9.4);
/// Half-width of the floor (plus margin) that must stay on screen.
const FRAMED_HALF_WIDTH: f32 = FLOOR_RADIUS + 1.0;

fn camera_transform(pull_back: f32) -> Transform {
    Transform::from_translation(CAMERA_TARGET + CAMERA_OFFSET * pull_back)
        .looking_at(CAMERA_TARGET, Vec3::Y)
}

/// Pull the camera back on narrow windows so the whole floor stays in frame.
fn fit_camera(
    windows: Query<&Window, Changed<Window>>,
    mut camera: Query<(&mut Transform, &Projection), With<CameraRig>>,
) {
    let Some(window) = windows.iter().next() else {
        return;
    };
    let aspect = window.width() / window.height().max(1.0);
    for (mut transform, projection) in &mut camera {
        let Projection::Perspective(perspective) = projection else {
            continue;
        };
        let visible_half_width = (perspective.fov / 2.0).tan() * aspect * CAMERA_OFFSET.length();
        *transform = camera_transform((FRAMED_HALF_WIDTH / visible_half_width).max(1.0));
    }
}

fn spawn_hud(mut commands: Commands, active: Res<ActiveRun<MusicalStools>>) {
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

/// The round's tally under the banner: 1v3's rounds, 2v2's team points.
fn tally(game: &State) -> String {
    match game.format {
        Format::OneVsThree { .. } => tr!(
            "musical-stools-rounds",
            cat = game.cat_rounds as u32,
            trio = game.trio_rounds as u32,
            needed = CAT_ROUNDS_TO_WIN as u32
        ),
        Format::TwoVsTwo { .. } => (0..2u8)
            .map(|team| {
                let members: Vec<Seat> = game
                    .playing()
                    .filter(|s| game.format.team(*s) == Some(team))
                    .collect();
                let points: u32 = members.iter().map(|s| game.dancer(*s).points).sum();
                format!(
                    "{} {}",
                    i18n::names(members.into_iter()),
                    tr!("musical-stools-team", points = points)
                )
            })
            .collect::<Vec<_>>()
            .join("     "),
        _ => String::new(),
    }
}

fn banner_text(active: &ActiveRun<MusicalStools>) -> (String, String) {
    let run = &active.run;
    let game = &run.game;
    if let Some(banner) = halted_banner(run) {
        return banner;
    }
    let round = tr!(
        "musical-stools-round",
        round = game.round as u32 + 1,
        rounds = game.rounds as u32
    );
    match game.phase {
        // The shared countdown (crate::intro) is drawn over this.
        Phase::Intro { .. } => (String::new(), String::new()),
        Phase::Regroup { .. } => (round, tally(game)),
        Phase::Dancing { t } => {
            let opening = t < 50 && game.round == 0;
            let title = if game.warning() {
                tr!("musical-stools-slowing")
            } else if opening {
                round.clone()
            } else {
                tr!("musical-stools-dance")
            };
            let sub = match tally(game) {
                tally if tally.is_empty() && !opening => round,
                tally => tally,
            };
            (title, sub)
        }
        Phase::Scramble { .. } => (tr!("musical-stools-sit"), tally(game)),
        Phase::Result { .. } if game.outcome.is_none() => {
            let title = match game.round_winner {
                Some(Side::Cat) => tr!("musical-stools-cat-round"),
                Some(Side::Trio) => tr!("musical-stools-trio-round"),
                None => {
                    let first = game.dancers.iter().filter_map(|d| d.sat_at).min();
                    let firsts: Vec<Seat> = game
                        .sitters()
                        .filter(|s| first.is_some() && game.dancer(*s).sat_at == first)
                        .collect();
                    if firsts.is_empty() {
                        String::new()
                    } else {
                        tr!(
                            "musical-stools-first",
                            players = i18n::names(firsts.into_iter())
                        )
                    }
                }
            };
            let standing: Vec<Seat> = game
                .sitters()
                .filter(|s| !game.dancer(*s).seated() && !game.dancer(*s).caught)
                .collect();
            let tally = tally(game);
            let sub = if !tally.is_empty() || standing.is_empty() {
                tally
            } else {
                tr!(
                    "musical-stools-standing",
                    players = i18n::names(standing.into_iter())
                )
            };
            (title, sub)
        }
        Phase::Result { .. } | Phase::Finished => {
            let outcome = game.outcome.expect("the last round has an outcome");
            let winners: Vec<Seat> = outcome.winners().collect();
            let prompt = if game.phase == Phase::Finished {
                active.continue_prompt()
            } else {
                tally(game)
            };
            (i18n::winners(&winners), prompt)
        }
    }
}

fn ordinal(place: u8) -> String {
    tr!("place", place = place as u32)
}

fn update_hud(
    active: Res<ActiveRun<MusicalStools>>,
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
                let dancer = game.dancer(seat);
                let role = if dancer.cat {
                    tr!("solo-tag")
                } else {
                    String::new()
                };
                let score = if dancer.cat {
                    tr!("musical-stools-cat")
                } else {
                    tr!("musical-stools-points", points = dancer.points)
                };
                let status = if active.run.controller_lost(seat) {
                    tr!("controller-lost")
                } else if let (Some(outcome), Phase::Finished) = (game.outcome, game.phase) {
                    outcome.placement[seat.index()]
                        .map(ordinal)
                        .unwrap_or_default()
                } else if dancer.caught {
                    tr!("musical-stools-caught")
                } else if dancer.seated() {
                    let gain = match game.phase {
                        Phase::Result { .. } => dancer.gained,
                        _ => 1,
                    };
                    tr!("musical-stools-seated", gain = gain)
                } else if matches!(game.phase, Phase::Result { .. }) && !dancer.cat {
                    tr!("musical-stools-left-out")
                } else {
                    tr!("musical-stools-dancing")
                };
                if !dancer.playing {
                    // Sitting out a duel.
                    format!("{}{who}", seat_label(seat))
                } else {
                    format!("{}{who}{role}\n{score}\n{status}", seat_label(seat))
                }
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

    #[test]
    fn stools_on_screen_turn_with_the_turntable() {
        // The turntable mesh turns by -angle about +y; a stool at angle a must sit on it.
        let angle = 0.7;
        let stool = world(stool_position(angle, 0, 3));
        let on_table = Quat::from_rotation_y(-angle) * Vec3::new(STOOL_RING, 0.0, 0.0);
        assert!((stool - on_table).length() < 1e-5);
    }
}
