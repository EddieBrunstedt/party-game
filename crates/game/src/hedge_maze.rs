//! Presentation for Hedge Maze. Reads `ActiveRun<HedgeMaze>` only; never writes simulation
//! state. The maze is dark: only cells the simulation has lit show their hedges, which grow up out
//! of the ground as the light reaches them. Positions are blended between the last two ticks.

use std::f32::consts::{PI, TAU};

use bevy::camera::ScalingMode;
use bevy::prelude::*;
use minigame_hedge_maze::{
    CELLS, Dir, FLARE_RADIUS, HedgeMaze, MAZES, MIDDLE, Phase, Pos, ROUNDS_TO_WIN, SIZE, Side,
    State, UNIT, Walker, cell_at, middle,
};
use sim_core::{Format, Seat, TICK_HZ};

use crate::audio::{Sfx, Sound};
use crate::characters::{Character, Pose, Roster};
use crate::flow::AppState;
use crate::i18n::{self, bot_tag, halted_banner};
use crate::palette::{MOONLIGHT, NIGHT_SKY, seat_color, seat_label};
use crate::particles::{self, Effects, hdr};
use crate::runner::{ActiveRun, MinigameSet, Seats};
use crate::scenery::{emissive_material, night_camera, noise, spawn_lantern_folk};
use crate::tr;

pub struct HedgeMazeViewPlugin;

impl Plugin for HedgeMazeViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            (spawn_scene, spawn_hud)
                .after(MinigameSet::Start)
                .run_if(resource_exists::<ActiveRun<HedgeMaze>>),
        )
        .add_systems(
            Update,
            (
                animate_hedges,
                animate_tiles,
                animate_beacon,
                animate_walkers,
                fade_flares,
                update_hud,
            )
                .run_if(resource_exists::<ActiveRun<HedgeMaze>>),
        )
        .add_systems(
            FixedUpdate,
            react
                .in_set(MinigameSet::React)
                .run_if(resource_exists::<ActiveRun<HedgeMaze>>),
        );
    }
}

const SCOPE: DespawnOnExit<AppState> = DespawnOnExit(AppState::Minigame);
/// A cell's width in world units.
const CELL: f32 = 1.6;
const HEDGE_HEIGHT: f32 = 0.9;
const HEDGE_THICKNESS: f32 = 0.4;
/// Hedges take this long to grow once lit.
const GROW_SECS: f32 = 0.35;
const FOLK_SCALE: f32 = 0.62;
const GHOST_SCALE: f32 = 0.78;
/// The camera's pitch above the ground: steep, so hedges hide little of the corridors behind them.
const PITCH: f32 = 62.0 * PI / 180.0;
const CAMERA_DISTANCE: f32 = 40.0;
/// World units either side of the maze kept clear for the seats' panels.
const PANEL_ROOM: f32 = 8.0;
/// The camera looks a little beyond the middle, leaving room for the banner at the top.
const CAMERA_TARGET: Vec3 = Vec3::new(0.0, 0.0, -1.1);
const BEACON_WARM: LinearRgba = LinearRgba::rgb(1.0, 0.78, 0.4);
const FLARE_SECS: f32 = 1.4;
/// Team colours for the panels' borders in team formats.
const TEAM_COLORS: [LinearRgba; 2] = [
    LinearRgba::rgb(1.0, 0.78, 0.3),
    LinearRgba::rgb(0.55, 0.45, 1.0),
];

/// Simulation positions (milli-cells, from the top-left cell) to world space, the middle cell at
/// the origin.
fn world(pos: Pos) -> Vec3 {
    let unit = UNIT as f32;
    Vec3::new(
        (pos.x as f32 / unit - MIDDLE as f32) * CELL,
        0.0,
        (pos.y as f32 / unit - MIDDLE as f32) * CELL,
    )
}

fn blend(fixed: &Time<Fixed>) -> f32 {
    fixed.overstep_fraction().clamp(0.0, 1.0)
}

/// Seconds since a play tick, for one-shot effects.
fn since(game: &State, tick: u32) -> f32 {
    game.tick.saturating_sub(tick) as f32 / TICK_HZ as f32
}

/// How far a cell lit at `tick` has grown in: what's lit before the race starts is there
/// already.
fn grown(game: &State, tick: u32) -> f32 {
    match game.phase {
        Phase::Intro { .. } | Phase::Ready { .. } => 1.0,
        _ => (since(game, tick) / GROW_SECS).min(1.0),
    }
}

/// Sounds and sparks, once per event: flares, catches, getting home and the results.
fn react(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    active: Res<ActiveRun<HedgeMaze>>,
    effects: Res<Effects>,
    style: Option<Res<FlareStyle>>,
    sfx: Sfx,
) {
    let Some((before, now)) = active.ticked() else {
        return;
    };
    let mut spark = |pos: Vec3, color: Vec4| {
        particles::burst(&mut commands, &effects, SCOPE, pos, color);
    };
    let mut flares = Vec::new();
    for seat in now.playing() {
        let (was, is) = (before.walker(seat), now.walker(seat));
        let variant = now.tick + seat.0 as u32;
        if was.flared_at.is_none() && is.flared_at.is_some() {
            sfx.play_varied(Sound::Ignite, variant);
            spark(world(is.pos) + Vec3::Y * 1.6, hdr(seat_color(seat), 8.0));
            flares.push((seat, world(is.pos)));
        }
        if is.caught_at != was.caught_at {
            sfx.play_varied(Sound::Fall, variant);
            spark(world(was.pos) + Vec3::Y * 0.6, hdr(seat_color(seat), 5.0));
        }
        if !was.finished() && is.finished() {
            sfx.play_varied(Sound::CoinsUp, variant);
            spark(world(is.pos) + Vec3::Y * 1.2, hdr(seat_color(seat), 6.0));
        }
    }
    let scored =
        matches!(before.phase, Phase::Racing { .. }) && matches!(now.phase, Phase::Result { .. });
    if scored && now.outcome.is_none() {
        sfx.play(Sound::RoundWin);
    }
    if before.outcome.is_none()
        && let Some(outcome) = now.outcome
    {
        sfx.play(Sound::Win);
        for seat in outcome.winners() {
            spark(
                world(now.walker(seat).pos) + Vec3::Y * 1.4,
                hdr(seat_color(seat), 6.0),
            );
        }
    }
    let (Some(style), false) = (style, flares.is_empty()) else {
        return;
    };
    let ball = meshes.add(Sphere::new(0.18));
    for (seat, at) in flares {
        commands.spawn((
            MeshMaterial3d(style.0.clone()),
            SCOPE,
            Flare { age: 0.0 },
            PointLight {
                color: Color::from(seat_color(seat) * 0.4 + LinearRgba::WHITE * 0.6),
                intensity: 0.0,
                range: FLARE_RADIUS as f32 / UNIT as f32 * CELL * 1.4,
                ..default()
            },
            Mesh3d(ball.clone()),
            Transform::from_translation(at + Vec3::Y * 2.0),
        ));
    }
}

/// A flare's light: it flashes up and fades.
#[derive(Component)]
struct Flare {
    age: f32,
}

fn fade_flares(
    mut commands: Commands,
    time: Res<Time>,
    mut flares: Query<(Entity, &mut Flare, &mut PointLight, &mut Transform)>,
) {
    for (entity, mut flare, mut light, mut transform) in &mut flares {
        flare.age += time.delta_secs();
        let left = 1.0 - flare.age / FLARE_SECS;
        if left <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        light.intensity = 3_000_000.0 * left * left;
        transform.translation.y = 2.0 + flare.age * 1.2;
    }
}

/// What a flare looks like: a white-hot ball.
#[derive(Resource)]
struct FlareStyle(Handle<StandardMaterial>);

/// One hedge slot: the line between two cells (or a cell and the outside). It shows when it's a
/// hedge in this maze and a cell next to it is lit.
#[derive(Component)]
struct Hedge {
    /// The cells on either side; `None` outside the maze.
    a: Option<usize>,
    b: Option<usize>,
    /// The way from `a` to `b`.
    dir: Dir,
}

/// A lit cell's floor.
#[derive(Component)]
struct Tile(usize);

#[derive(Component)]
struct Beacon {
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct BeaconLight;

/// The glowing pad in a runner's start corner.
#[derive(Component)]
struct StartPad(Seat);

#[derive(Component)]
struct WalkerView {
    seat: Seat,
    head_material: Handle<StandardMaterial>,
    head_light: Entity,
    scale: f32,
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
    active: Res<ActiveRun<HedgeMaze>>,
    roster: Res<Roster>,
) {
    commands.insert_resource(ClearColor(NIGHT_SKY));
    commands.insert_resource(GlobalAmbientLight {
        color: MOONLIGHT,
        brightness: 12.0,
        ..default()
    });
    commands.insert_resource(FlareStyle(materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: LinearRgba::rgb(1.0, 0.95, 0.85) * 30.0,
        ..default()
    })));
    commands.spawn((
        SCOPE,
        night_camera(camera_transform(), (60.0, 120.0)),
        camera_projection(),
    ));
    commands.spawn((
        SCOPE,
        DirectionalLight {
            color: MOONLIGHT,
            illuminance: 90.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-6.0, 14.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let game = &active.run.game;
    spawn_ground(&mut commands, &mut meshes, &mut materials);
    spawn_hedges(&mut commands, &mut meshes, &mut materials);
    spawn_beacon(&mut commands, &mut meshes, &mut materials);
    for seat in game.runners() {
        commands.spawn((
            SCOPE,
            StartPad(seat),
            Mesh3d(meshes.add(Cylinder::new(CELL * 0.36, 0.03))),
            MeshMaterial3d(materials.add(emissive_material(seat_color(seat), 2.0))),
            Transform::from_translation(world(Pos::of(game.walker(seat).start)) + Vec3::Y * 0.03),
        ));
    }
    for seat in game.playing() {
        spawn_walker(
            &mut commands,
            &mut meshes,
            &mut materials,
            game,
            seat,
            roster.of(seat),
        );
    }
}

fn spawn_ground(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let side = SIZE as f32 * CELL;
    commands.spawn((
        SCOPE,
        Mesh3d(meshes.add(Cuboid::new(side + 6.0, 0.2, side + 6.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.02, 0.022, 0.03),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.1, 0.0),
    ));
    // Lit cells' floors: packed earth with a faint warm glow, so the lit part of the maze reads
    // even away from any lantern.
    let tile = meshes.add(Cuboid::new(CELL, 0.02, CELL));
    let shades: Vec<_> = [(0.06, 0.05, 0.045), (0.055, 0.046, 0.042)]
        .into_iter()
        .map(|(r, g, b)| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                emissive: LinearRgba::rgb(0.025, 0.018, 0.012),
                perceptual_roughness: 0.9,
                ..default()
            })
        })
        .collect();
    for cell in 0..CELLS {
        let (x, y) = (cell % SIZE, cell / SIZE);
        commands.spawn((
            SCOPE,
            Tile(cell),
            Mesh3d(tile.clone()),
            MeshMaterial3d(shades[(x + y) % 2].clone()),
            Transform::from_translation(world(Pos::of(cell)) + Vec3::Y * 0.01),
            Visibility::Hidden,
        ));
    }
}

fn spawn_hedges(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let along_x = meshes.add(Cuboid::new(
        CELL + HEDGE_THICKNESS,
        HEDGE_HEIGHT,
        HEDGE_THICKNESS,
    ));
    let along_z = meshes.add(Cuboid::new(
        HEDGE_THICKNESS,
        HEDGE_HEIGHT,
        CELL + HEDGE_THICKNESS,
    ));
    let shades: Vec<_> = [(0.1, 0.36, 0.12), (0.08, 0.3, 0.1), (0.12, 0.4, 0.14)]
        .into_iter()
        .map(|(r, g, b)| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                emissive: LinearRgba::rgb(r, g, b) * 0.3,
                perceptual_roughness: 0.85,
                ..default()
            })
        })
        .collect();
    let inside = |x: i32, y: i32| {
        ((0..SIZE as i32).contains(&x) && (0..SIZE as i32).contains(&y))
            .then(|| cell_at(x as usize, y as usize))
    };
    let half = CELL / 2.0;
    for y in -1..SIZE as i32 {
        for x in -1..SIZE as i32 {
            // The hedge right of (x, y), between it and (x + 1, y)...
            if (0..SIZE as i32).contains(&y) {
                let centre = Vec3::new(
                    (x as f32 - MIDDLE as f32) * CELL + half,
                    0.0,
                    (y as f32 - MIDDLE as f32) * CELL,
                );
                commands.spawn((
                    SCOPE,
                    Hedge {
                        a: inside(x, y),
                        b: inside(x + 1, y),
                        dir: Dir::Right,
                    },
                    Mesh3d(along_z.clone()),
                    MeshMaterial3d(shades[(noise(x, y, 1) * 2.99) as usize].clone()),
                    Transform::from_translation(centre),
                    Visibility::Hidden,
                ));
            }
            // ...and the one below it, between it and (x, y + 1).
            if (0..SIZE as i32).contains(&x) {
                let centre = Vec3::new(
                    (x as f32 - MIDDLE as f32) * CELL,
                    0.0,
                    (y as f32 - MIDDLE as f32) * CELL + half,
                );
                commands.spawn((
                    SCOPE,
                    Hedge {
                        a: inside(x, y),
                        b: inside(x, y + 1),
                        dir: Dir::Down,
                    },
                    Mesh3d(along_x.clone()),
                    MeshMaterial3d(shades[(noise(x, y, 2) * 2.99) as usize].clone()),
                    Transform::from_translation(centre),
                    Visibility::Hidden,
                ));
            }
        }
    }
}

fn spawn_beacon(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let material = materials.add(emissive_material(BEACON_WARM, 8.0));
    let at = world(Pos::of(middle()));
    commands
        .spawn((
            SCOPE,
            Beacon {
                material: material.clone(),
            },
            Transform::from_translation(at),
            Visibility::default(),
        ))
        .with_children(|beacon| {
            beacon.spawn((
                Mesh3d(meshes.add(Cylinder::new(CELL * 0.42, 0.04))),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(0.0, 0.03, 0.0),
            ));
            beacon.spawn((
                Mesh3d(meshes.add(Cylinder::new(0.06, 1.3))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.1, 0.08, 0.06),
                    ..default()
                })),
                Transform::from_xyz(0.0, 0.65, 0.0),
            ));
            beacon.spawn((
                Mesh3d(meshes.add(Sphere::new(0.28))),
                MeshMaterial3d(material),
                Transform::from_xyz(0.0, 1.45, 0.0),
            ));
            beacon.spawn((
                BeaconLight,
                PointLight {
                    color: Color::from(BEACON_WARM),
                    intensity: 600_000.0,
                    range: CELL * 3.0,
                    ..default()
                },
                Transform::from_xyz(0.0, 1.6, 0.0),
            ));
        });
}

fn spawn_walker(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    game: &State,
    seat: Seat,
    character: Character,
) {
    let walker = game.walker(seat);
    let scale = if walker.ghost {
        GHOST_SCALE
    } else {
        FOLK_SCALE
    };
    let folk = spawn_lantern_folk(
        commands,
        meshes,
        materials,
        AppState::Minigame,
        seat,
        character,
        Transform::from_translation(world(walker.pos)).with_scale(Vec3::splat(scale)),
    );
    if walker.ghost {
        // A see-through sheet over the folk, so the solo reads as the ghost at a glance.
        let sheet = commands
            .spawn((
                Mesh3d(meshes.add(Cone::new(0.62, 2.1))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgba(0.85, 0.9, 1.0, 0.35),
                    emissive: seat_color(seat) * 0.8 + LinearRgba::rgb(0.3, 0.3, 0.35),
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                })),
                Transform::from_xyz(0.0, 1.05, 0.0),
            ))
            .id();
        commands.entity(folk.root).add_child(sheet);
    }
    commands.entity(folk.root).insert(WalkerView {
        seat,
        head_material: folk.head_material,
        head_light: folk.head_light,
        scale,
    });
}

/// When cell `cell` was lit, if it is.
fn lit(game: &State, cell: Option<usize>) -> Option<u32> {
    cell.and_then(|c| game.lit_at[c])
}

fn is_hedge(game: &State, hedge: &Hedge) -> bool {
    match (hedge.a, hedge.b) {
        (Some(a), Some(_)) => !game.maze.is_open(a, hedge.dir),
        _ => true,
    }
}

/// Hedges grow out of the ground once the light reaches them.
fn animate_hedges(
    active: Res<ActiveRun<HedgeMaze>>,
    mut hedges: Query<(&Hedge, &mut Transform, &mut Visibility)>,
) {
    let game = &active.run.game;
    for (hedge, mut transform, mut visibility) in &mut hedges {
        let lit_at = match (lit(game, hedge.a), lit(game, hedge.b)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let grown = match lit_at {
            Some(tick) if is_hedge(game, hedge) => grown(game, tick),
            _ => 0.0,
        };
        if grown <= 0.0 {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = Visibility::Inherited;
        let height = 0.15 + 0.85 * grown;
        transform.scale = Vec3::new(1.0, height, 1.0);
        transform.translation.y = HEDGE_HEIGHT * height / 2.0;
    }
}

fn animate_tiles(
    active: Res<ActiveRun<HedgeMaze>>,
    mut tiles: Query<(&Tile, &mut Visibility, &mut Transform), Without<StartPad>>,
    mut pads: Query<(&StartPad, &mut Transform), Without<Tile>>,
) {
    let game = &active.run.game;
    for (tile, mut visibility, mut transform) in &mut tiles {
        let shown = game.lit_at[tile.0].map(|t| grown(game, t));
        match shown {
            Some(grown) => {
                *visibility = Visibility::Inherited;
                transform.scale = Vec3::new(0.9 + 0.1 * grown, 1.0, 0.9 + 0.1 * grown);
            }
            None => *visibility = Visibility::Hidden,
        }
    }
    // Corners change hands each maze.
    for (pad, mut transform) in &mut pads {
        transform.translation = world(Pos::of(game.walker(pad.0).start)) + Vec3::Y * 0.03;
    }
}

fn animate_beacon(
    time: Res<Time>,
    active: Res<ActiveRun<HedgeMaze>>,
    beacons: Query<&Beacon>,
    mut lights: Query<&mut PointLight, With<BeaconLight>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let game = &active.run.game;
    let pulse = (time.elapsed_secs() * 2.4).sin() * 0.5 + 0.5;
    let racing = matches!(game.phase, Phase::Racing { .. });
    let strength = if racing { 6.0 + pulse * 4.0 } else { 5.0 };
    for beacon in &beacons {
        if let Some(mut material) = materials.get_mut(&beacon.material) {
            material.emissive = BEACON_WARM * strength;
        }
    }
    for mut light in &mut lights {
        light.intensity = 450_000.0 + pulse * 250_000.0;
    }
}

fn facing_rotation(dir: Dir) -> Quat {
    let (x, y) = dir.delta();
    Quat::from_rotation_y((x as f32).atan2(y as f32))
}

/// Where a runner stands once home: round the beacon, so several home don't overlap.
fn home_spot(seat: Seat) -> Vec3 {
    let angle = TAU * (seat.0 as f32 + 0.5) / 4.0;
    world(Pos::of(middle())) + Vec3::new(angle.cos(), 0.0, angle.sin()) * CELL * 0.3
}

fn animate_walkers(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<HedgeMaze>>,
    mut walkers: Query<(&WalkerView, &mut Transform, &mut Pose, &mut Visibility)>,
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
    for (view, mut transform, mut pose, mut visibility) in &mut walkers {
        let seat = view.seat;
        let walker: &Walker = game.walker(seat);
        let before: &Walker = active.previous.walker(seat);
        // A catch or a new maze moves a walker in one tick: don't slide across the maze.
        let jumped = before.pos.distance_squared(walker.pos) > (2 * UNIT as i64).pow(2);
        let pos = if jumped {
            world(walker.pos)
        } else {
            world(before.pos).lerp(world(walker.pos), alpha)
        };
        let pos = if walker.finished() {
            home_spot(seat)
        } else {
            pos
        };
        let celebrating = winners.contains(&seat)
            || walker
                .finished_at
                .is_some_and(|tick| since(game, tick) < 1.0);
        *pose = if celebrating || walker.finished() {
            Pose::Celebrate
        } else if walker.frozen > 0 || (game.outcome.is_some() && !winners.is_empty()) {
            Pose::Slump
        } else {
            Pose::Idle
        };
        let mut lift = 0.0;
        let mut rotation = facing_rotation(walker.heading);
        if walker.finished() {
            rotation = Quat::from_rotation_y(-(TAU * (seat.0 as f32 + 0.5) / 4.0) + PI / 2.0);
        }
        if walker.moving {
            // A trot.
            let step = now * 14.0 + seat.0 as f32;
            lift = step.sin().abs() * 0.1;
            rotation *= Quat::from_rotation_z(step.sin() * 0.08);
        }
        if walker.ghost {
            // The ghost floats.
            lift = 0.25 + (now * 3.0).sin() * 0.08;
        }
        transform.translation = pos + Vec3::Y * lift;
        transform.rotation = rotation;
        transform.scale = Vec3::splat(view.scale);
        // Blink while safe from the ghost.
        *visibility = if walker.safe > 0 && walker.frozen == 0 && (now * 10.0).sin() < -0.3 {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };

        let glow = if walker.frozen > 0 {
            0.5
        } else if celebrating {
            4.0
        } else {
            2.0
        };
        if let Some(mut material) = materials.get_mut(&view.head_material) {
            material.emissive = seat_color(seat) * glow;
        }
        if let Ok(mut light) = lights.get_mut(view.head_light) {
            light.intensity = 30_000.0 * glow;
            light.range = CELL * 2.2;
        }
    }
}

/// Orthographic, so every corner's quarter of the maze looks the same. The width leaves room for
/// the seats' column beside the maze; narrow windows show more height instead of cutting it.
fn camera_projection() -> Projection {
    let side = SIZE as f32 * CELL;
    Projection::Orthographic(OrthographicProjection {
        scaling_mode: ScalingMode::AutoMin {
            min_width: side + 2.0 * PANEL_ROOM,
            min_height: side * PITCH.sin() + HEDGE_HEIGHT * PITCH.cos() + 4.4,
        },
        ..OrthographicProjection::default_3d()
    })
}

fn camera_transform() -> Transform {
    let offset = Vec3::new(0.0, PITCH.sin(), PITCH.cos()) * CAMERA_DISTANCE;
    Transform::from_translation(CAMERA_TARGET + offset).looking_at(CAMERA_TARGET, Vec3::Y)
}

fn spawn_hud(mut commands: Commands, active: Res<ActiveRun<HedgeMaze>>) {
    commands
        .spawn((
            SCOPE,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(14.0),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(4.0),
                ..default()
            },
        ))
        .with_children(|top| {
            top.spawn((
                HudText::Banner,
                Text::new(""),
                TextFont::from_font_size(44.0),
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

    // The seats down the right, beside the maze.
    let game = &active.run.game;
    commands
        .spawn((
            SCOPE,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(20.0),
                top: Val::Percent(24.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(12.0),
                ..default()
            },
        ))
        .with_children(|column| {
            for seat in game.players.seats() {
                let border = match team_index(&game.format, seat) {
                    Some(team) => Color::from(TEAM_COLORS[team as usize]),
                    None => Color::NONE,
                };
                column
                    .spawn((
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

fn team_index(format: &Format, seat: Seat) -> Option<u8> {
    match format {
        Format::FreeForAll | Format::Duel { .. } => None,
        _ => format.team(seat),
    }
}

/// 1v3's mazes, 2v2's team points.
fn tally(game: &State) -> String {
    match game.format {
        Format::OneVsThree { .. } => tr!(
            "hedge-maze-rounds",
            ghost = game.ghost_rounds as u32,
            trio = game.trio_rounds as u32,
            needed = ROUNDS_TO_WIN as u32
        ),
        Format::TwoVsTwo { .. } => (0..2u8)
            .map(|team| {
                let members: Vec<Seat> = game
                    .playing()
                    .filter(|s| game.format.team(*s) == Some(team))
                    .collect();
                let points: u32 = members.iter().map(|s| game.walker(*s).points).sum();
                format!(
                    "{} {}",
                    i18n::names(members.into_iter()),
                    tr!("hedge-maze-team", points = points)
                )
            })
            .collect::<Vec<_>>()
            .join("     "),
        _ => String::new(),
    }
}

fn with_tally(game: &State, line: String) -> String {
    match tally(game) {
        tally if tally.is_empty() => line,
        tally => format!("{line}     {tally}"),
    }
}

/// Runners home this maze, in the order they got there.
fn home_order(game: &State) -> Vec<Seat> {
    let mut home: Vec<Seat> = game
        .runners()
        .filter(|s| game.walker(*s).finished())
        .collect();
    home.sort_by_key(|s| game.walker(*s).finished_at);
    home
}

fn banner_text(active: &ActiveRun<HedgeMaze>) -> (String, String) {
    let run = &active.run;
    let game = &run.game;
    if let Some(banner) = halted_banner(run) {
        return banner;
    }
    let maze = tr!(
        "hedge-maze-maze",
        maze = game.round as u32 + 1,
        mazes = MAZES as u32
    );
    let clock = tr!(
        "hedge-maze-time",
        seconds = game.ticks_left().div_ceil(TICK_HZ as u16) as u32
    );
    match game.phase {
        // The shared countdown (crate::intro) is drawn over this.
        Phase::Intro { .. } => (String::new(), String::new()),
        Phase::Ready { .. } => (maze, tally(game)),
        Phase::Racing { t } => {
            let title = match home_order(game).last() {
                Some(seat)
                    if game
                        .walker(*seat)
                        .finished_at
                        .is_some_and(|tick| since(game, tick) < 1.5) =>
                {
                    tr!("hedge-maze-home", player = seat_label(*seat))
                }
                _ if t < 90 => tr!("hedge-maze-go"),
                _ => maze.clone(),
            };
            (title, with_tally(game, clock))
        }
        Phase::Result { .. } if game.outcome.is_none() => {
            let title = match game.round_winner {
                Some(Side::Ghost) => tr!("hedge-maze-ghost-maze"),
                Some(Side::Trio) => tr!("hedge-maze-trio-maze"),
                None => {
                    let order = home_order(game);
                    let first = order.first().and_then(|s| game.walker(*s).finished_at);
                    let firsts = order
                        .into_iter()
                        .filter(|s| first.is_some() && game.walker(*s).finished_at == first);
                    match first {
                        Some(_) => tr!("hedge-maze-first", players = i18n::names(firsts)),
                        None => tr!("hedge-maze-nobody"),
                    }
                }
            };
            (title, tally(game))
        }
        Phase::Result { .. } | Phase::Finished => {
            let outcome = game.outcome.expect("the last maze has an outcome");
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
    active: Res<ActiveRun<HedgeMaze>>,
    seats: Res<Seats>,
    mut texts: Query<(&HudText, &mut Text)>,
) {
    let (title, subtitle) = banner_text(&active);
    let game = &active.run.game;
    let order = home_order(game);
    for (hud, mut text) in &mut texts {
        text.0 = match *hud {
            HudText::Banner => title.clone(),
            HudText::SubBanner => subtitle.clone(),
            HudText::SeatPanel(seat) => {
                let who = bot_tag(seats.is_bot(seat));
                let walker = game.walker(seat);
                if !walker.playing {
                    // Sitting out a duel.
                    text.0 = format!("{}{who}", seat_label(seat));
                    continue;
                }
                let role = if walker.ghost {
                    tr!("solo-tag")
                } else {
                    String::new()
                };
                let score = if walker.ghost {
                    tr!("hedge-maze-ghost", catches = walker.catches as u32)
                } else if game.ghost().is_some() {
                    tr!("hedge-maze-runner")
                } else {
                    tr!("hedge-maze-points", points = walker.points)
                };
                let status = if active.run.controller_lost(seat) {
                    tr!("controller-lost")
                } else if let (Some(outcome), Phase::Finished) = (game.outcome, game.phase) {
                    outcome.placement[seat.index()]
                        .map(ordinal)
                        .unwrap_or_default()
                } else if walker.finished() {
                    // Getting home on the same tick shares the place.
                    let place = order
                        .iter()
                        .filter(|s| game.walker(**s).finished_at < walker.finished_at)
                        .count() as u8
                        + 1;
                    match game.phase {
                        Phase::Result { .. } if game.ghost().is_none() => tr!(
                            "hedge-maze-home-gain",
                            place = ordinal(place),
                            gain = walker.gained
                        ),
                        _ => tr!("hedge-maze-home-place", place = ordinal(place)),
                    }
                } else if walker.frozen > 0 {
                    tr!("hedge-maze-caught")
                } else if matches!(game.phase, Phase::Result { .. }) && !walker.ghost {
                    tr!("hedge-maze-lost")
                } else if walker.flare_used {
                    tr!("hedge-maze-flare-used")
                } else {
                    tr!("hedge-maze-flare-ready")
                };
                format!("{}{who}{role}\n{score}\n{status}", seat_label(seat))
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use minigame_hedge_maze::neighbour;

    #[test]
    fn facing_rotation_turns_the_model_front_towards_the_heading() {
        // The model's front is +z; heading right is +x in world space.
        let front = facing_rotation(Dir::Right) * Vec3::Z;
        assert!((front - Vec3::X).length() < 1e-5);
        let front = facing_rotation(Dir::Down) * Vec3::Z;
        assert!((front - Vec3::Z).length() < 1e-5);
    }

    #[test]
    fn the_middle_cell_is_the_origin_and_cells_are_a_cell_apart() {
        assert_eq!(world(Pos::of(middle())), Vec3::ZERO);
        let right = neighbour(middle(), Dir::Right).unwrap();
        assert!((world(Pos::of(right)) - Vec3::X * CELL).length() < 1e-5);
        let down = neighbour(middle(), Dir::Down).unwrap();
        assert!((world(Pos::of(down)) - Vec3::Z * CELL).length() < 1e-5);
    }

    #[test]
    fn the_camera_is_orthographic_and_frames_the_whole_maze() {
        let Projection::Orthographic(ortho) = camera_projection() else {
            panic!("the maze is seen orthographically");
        };
        let ScalingMode::AutoMin {
            min_width,
            min_height,
        } = ortho.scaling_mode
        else {
            panic!("fits the window");
        };
        let side = SIZE as f32 * CELL;
        assert!(min_width > side + PANEL_ROOM);
        assert!(min_height > side * PITCH.sin());
    }
}
