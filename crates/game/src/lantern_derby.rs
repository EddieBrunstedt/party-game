//! Presentation for Lantern Derby. Reads `ActiveRun<LanternDerby>` only; never writes simulation
//! state. The boards stand in a row facing the camera with their throwers below them, and the
//! race track runs across the top, one lane per runner. Beside each board a meter rises while A is
//! held, its top level with where the ball would stop, and a line across the board shows the same
//! height, so the dots of the aim and that line cross where the ball is going.

use std::f32::consts::FRAC_PI_2;

use bevy::camera::ScalingMode;
use bevy::prelude::*;
use minigame_lantern_derby::{
    HALF_WIDTH, HOLES, Hand, LENGTH, LanternDerby, Lid, MAX_POWER, Phase, RAIL, RETURN_TICKS,
    SINK_TICKS, Shot, State, power_at, roll_ticks,
};
use sim_core::{Format, Seat, TICK_HZ};

use crate::audio::{Sfx, Sound};
use crate::characters::{Pose, Roster};
use crate::flow::AppState;
use crate::i18n::{self, bot_tag, halted_banner};
use crate::palette::{LANTERN_WARM, MOONLIGHT, NIGHT_SKY, seat_color, seat_label};
use crate::particles::{self, Effects, hdr};
use crate::runner::{ActiveRun, MinigameSet, Seats};
use crate::scenery::{emissive_material, night_camera, spawn_fireflies, spawn_lantern_folk};
use crate::tr;

pub struct LanternDerbyViewPlugin;

impl Plugin for LanternDerbyViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            (spawn_scene, spawn_hud)
                .after(MinigameSet::Start)
                .run_if(resource_exists::<ActiveRun<LanternDerby>>),
        )
        .add_systems(
            Update,
            (
                animate_balls,
                animate_aims,
                animate_meters,
                animate_lid,
                animate_runners,
                animate_throwers,
                update_hud,
            )
                .run_if(resource_exists::<ActiveRun<LanternDerby>>),
        )
        .add_systems(
            FixedUpdate,
            react
                .in_set(MinigameSet::React)
                .run_if(resource_exists::<ActiveRun<LanternDerby>>),
        );
    }
}

const SCOPE: DespawnOnExit<AppState> = DespawnOnExit(AppState::Minigame);
/// World units per simulation milli-unit.
const SCALE: f32 = 0.0028;
const BALL_RADIUS: f32 = 0.11;
/// Dots along each board's aim.
const AIM_DOTS: usize = 12;
/// The meter stands this far (milli-units) outside a board's right rail.
const METER_GAP: i32 = 90;
/// The race track: its lanes' height, the lowest lane's, and how far either side it runs.
const LANE: f32 = 0.95;
const TRACK_Y: f32 = 5.0;
const TRACK_HALF: f32 = 6.6;
const THROWER_SCALE: f32 = 0.55;
const RUNNER_SCALE: f32 = 0.34;
const THROWER_Y: f32 = -2.2;
/// How long a thrower's last ball shows on their panel (and a hop on them), in ticks.
const SHOT_TICKS: u32 = 70;
const GOAL_WHITE: LinearRgba = LinearRgba::rgb(0.85, 0.9, 1.0);
const OVER_RED: LinearRgba = LinearRgba::rgb(1.0, 0.1, 0.06);
/// Each hole's colour by its value (1, 2, 3).
const HOLE_COLORS: [LinearRgba; 3] = [
    LinearRgba::rgb(1.0, 0.62, 0.18),
    LinearRgba::rgb(0.3, 0.8, 1.0),
    LinearRgba::rgb(1.0, 0.25, 0.65),
];
/// Team colours for the panel borders in team formats.
const TEAM_COLORS: [LinearRgba; 2] = [
    LinearRgba::rgb(1.0, 0.78, 0.3),
    LinearRgba::rgb(0.55, 0.45, 1.0),
];

fn hole_color(hole: usize) -> LinearRgba {
    HOLE_COLORS[HOLES[hole].value as usize - 1]
}

/// A point on the board in `slot` (milli-units from its throwing line's middle) in world space.
fn board_point(game: &State, slot: u8, x: f32, y: f32) -> Vec3 {
    Vec3::new((game.slot_x(slot) as f32 + x) * SCALE, y * SCALE, 0.0)
}

/// Where a lane's runner stands, `progress` (0 to 1) along it.
fn lane_point(game: &State, side: usize, progress: f32) -> Vec3 {
    let lane = game.racing_sides().position(|s| s == side).unwrap_or(0);
    Vec3::new(
        -TRACK_HALF + progress * 2.0 * TRACK_HALF,
        TRACK_Y + lane as f32 * LANE,
        0.2,
    )
}

fn blend(fixed: &Time<Fixed>) -> f32 {
    fixed.overstep_fraction().clamp(0.0, 1.0)
}

#[derive(Component)]
struct BallView {
    seat: Seat,
}

#[derive(Component)]
struct AimDot {
    seat: Seat,
    index: usize,
    material: Handle<StandardMaterial>,
}

/// The meter beside a board, and the line across it at the same height.
#[derive(Component)]
enum MeterView {
    Fill(Seat, Handle<StandardMaterial>),
    Line(Seat),
}

#[derive(Component)]
enum LidView {
    Cursor(Handle<StandardMaterial>),
    Cover,
}

#[derive(Component)]
struct RunnerView {
    side: usize,
    /// Where it's drawn, chasing the side's progress.
    shown: f32,
}

#[derive(Component)]
struct ThrowerView {
    seat: Seat,
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
    active: Res<ActiveRun<LanternDerby>>,
    roster: Res<Roster>,
) {
    commands.insert_resource(ClearColor(NIGHT_SKY));
    commands.insert_resource(GlobalAmbientLight {
        color: MOONLIGHT,
        brightness: 30.0,
        ..default()
    });
    commands.spawn((
        SCOPE,
        night_camera(camera_transform(), (30.0, 60.0)),
        camera_projection(),
    ));
    commands.spawn((
        SCOPE,
        DirectionalLight {
            color: MOONLIGHT,
            illuminance: 250.0,
            ..default()
        },
        Transform::from_xyz(-4.0, 8.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    spawn_fireflies(
        &mut commands,
        &mut meshes,
        &mut materials,
        AppState::Minigame,
        24,
        Vec3::new(-10.0, -2.0, -2.0),
        Vec3::new(10.0, 8.0, -0.5),
    );

    let game = &active.run.game;
    let unit = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    for board in game.throwers() {
        spawn_board(
            &mut commands,
            &mut meshes,
            &mut materials,
            &unit,
            game,
            board.slot,
            board.seat,
        );
        let folk = spawn_lantern_folk(
            &mut commands,
            &mut meshes,
            &mut materials,
            AppState::Minigame,
            board.seat,
            roster.of(board.seat),
            Transform::from_translation(
                board_point(game, board.slot, -(HALF_WIDTH as f32) * 0.55, 0.0).with_y(THROWER_Y)
                    + Vec3::Z * 0.6,
            )
            .with_scale(Vec3::splat(THROWER_SCALE)),
        );
        commands
            .entity(folk.root)
            .insert(ThrowerView { seat: board.seat });
    }
    if let Some(barker) = game.barker {
        let (_, right) = game.cursor_bounds();
        let folk = spawn_lantern_folk(
            &mut commands,
            &mut meshes,
            &mut materials,
            AppState::Minigame,
            barker.seat,
            roster.of(barker.seat),
            Transform::from_xyz(right as f32 * SCALE + 1.0, THROWER_Y, 0.6)
                .with_rotation(Quat::from_rotation_y(-0.4))
                .with_scale(Vec3::splat(THROWER_SCALE * 1.2)),
        );
        commands
            .entity(folk.root)
            .insert(ThrowerView { seat: barker.seat });
        let color = seat_color(barker.seat);
        let material = materials.add(emissive_material(color, 3.0));
        commands.spawn((
            SCOPE,
            LidView::Cursor(material.clone()),
            Mesh3d(meshes.add(Annulus::new(0.3, 0.38))),
            MeshMaterial3d(material),
            Transform::from_xyz(0.0, 0.0, 0.3),
        ));
        commands.spawn((
            SCOPE,
            LidView::Cover,
            Mesh3d(meshes.add(Circle::new(1.0))),
            MeshMaterial3d(materials.add(emissive_material(color, 1.4))),
            Transform::from_xyz(0.0, 0.0, 0.25),
            Visibility::Hidden,
        ));
    }
    spawn_track(
        &mut commands,
        &mut meshes,
        &mut materials,
        &unit,
        game,
        &roster,
    );
}

#[allow(clippy::too_many_arguments)]
fn spawn_board(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    unit: &Handle<Mesh>,
    game: &State,
    slot: u8,
    seat: Seat,
) {
    let color = seat_color(seat);
    let middle = board_point(game, slot, 0.0, LENGTH as f32 / 2.0);
    let (width, length) = (2.0 * HALF_WIDTH as f32 * SCALE, LENGTH as f32 * SCALE);
    // The board, then its rails in the thrower's colour.
    commands.spawn((
        SCOPE,
        Mesh3d(unit.clone()),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.07, 0.055, 0.06),
            perceptual_roughness: 0.85,
            ..default()
        })),
        Transform::from_translation(middle - Vec3::Z * 0.06)
            .with_scale(Vec3::new(width, length, 0.1)),
    ));
    let rail = materials.add(emissive_material(color, 1.2));
    for (offset, scale) in [
        (
            Vec3::new(-width / 2.0, 0.0, 0.0),
            Vec3::new(0.06, length, 0.12),
        ),
        (
            Vec3::new(width / 2.0, 0.0, 0.0),
            Vec3::new(0.06, length, 0.12),
        ),
        (
            Vec3::new(0.0, length / 2.0, 0.0),
            Vec3::new(width + 0.06, 0.06, 0.12),
        ),
    ] {
        commands.spawn((
            SCOPE,
            Mesh3d(unit.clone()),
            MeshMaterial3d(rail.clone()),
            Transform::from_translation(middle + offset).with_scale(scale),
        ));
    }
    // The gutter the balls come back to.
    commands.spawn((
        SCOPE,
        Mesh3d(unit.clone()),
        MeshMaterial3d(materials.add(emissive_material(color, 0.3))),
        Transform::from_translation(board_point(game, slot, 0.0, -50.0))
            .with_scale(Vec3::new(width, 0.1, 0.12)),
    ));

    let dark = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        perceptual_roughness: 1.0,
        ..default()
    });
    let pip = meshes.add(Circle::new(0.035));
    for (index, hole) in HOLES.iter().enumerate() {
        let at = board_point(game, slot, hole.x as f32, hole.y as f32);
        let radius = hole.radius as f32 * SCALE;
        let glow = materials.add(emissive_material(hole_color(index), 2.5));
        commands.spawn((
            SCOPE,
            Mesh3d(meshes.add(Circle::new(radius))),
            MeshMaterial3d(dark.clone()),
            Transform::from_translation(at + Vec3::Z * 0.01),
        ));
        commands.spawn((
            SCOPE,
            Mesh3d(meshes.add(Annulus::new(radius, radius + 0.05))),
            MeshMaterial3d(glow.clone()),
            Transform::from_translation(at + Vec3::Z * 0.02),
        ));
        // Its value in dots under it.
        for k in 0..hole.value {
            let dx = (k as f32 - (hole.value as f32 - 1.0) / 2.0) * 0.1;
            commands.spawn((
                SCOPE,
                Mesh3d(pip.clone()),
                MeshMaterial3d(glow.clone()),
                Transform::from_translation(at + Vec3::new(dx, -radius - 0.1, 0.02)),
            ));
        }
    }

    // The ball, the aim and the meter.
    commands.spawn((
        SCOPE,
        BallView { seat },
        Mesh3d(meshes.add(Sphere::new(BALL_RADIUS))),
        MeshMaterial3d(materials.add(emissive_material(color, 4.0))),
        Transform::default(),
    ));
    let dot = meshes.add(Circle::new(0.03));
    for index in 0..AIM_DOTS {
        let material = materials.add(emissive_material(color, 1.5));
        commands.spawn((
            SCOPE,
            AimDot {
                seat,
                index,
                material: material.clone(),
            },
            Mesh3d(dot.clone()),
            MeshMaterial3d(material),
            Transform::default(),
        ));
    }
    let meter_x = (HALF_WIDTH + METER_GAP) as f32;
    commands.spawn((
        SCOPE,
        Mesh3d(unit.clone()),
        MeshMaterial3d(materials.add(emissive_material(color, 0.15))),
        Transform::from_translation(
            board_point(game, slot, meter_x, MAX_POWER as f32 / 2.0) - Vec3::Z * 0.02,
        )
        .with_scale(Vec3::new(0.14, MAX_POWER as f32 * SCALE, 0.05)),
    ));
    // Past the top: the meter's last stretch is red.
    commands.spawn((
        SCOPE,
        Mesh3d(unit.clone()),
        MeshMaterial3d(materials.add(emissive_material(OVER_RED, 0.5))),
        Transform::from_translation(
            board_point(game, slot, meter_x, (LENGTH + MAX_POWER) as f32 / 2.0) - Vec3::Z * 0.01,
        )
        .with_scale(Vec3::new(0.14, (MAX_POWER - LENGTH) as f32 * SCALE, 0.05)),
    ));
    let fill = materials.add(emissive_material(color, 3.0));
    commands.spawn((
        SCOPE,
        MeterView::Fill(seat, fill.clone()),
        Mesh3d(unit.clone()),
        MeshMaterial3d(fill),
        Transform::default(),
        Visibility::Hidden,
    ));
    commands.spawn((
        SCOPE,
        MeterView::Line(seat),
        Mesh3d(unit.clone()),
        MeshMaterial3d(materials.add(emissive_material(color, 0.9))),
        Transform::default(),
        Visibility::Hidden,
    ));
}

fn spawn_track(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    unit: &Handle<Mesh>,
    game: &State,
    roster: &Roster,
) {
    let sides: Vec<usize> = game.racing_sides().collect();
    for side in sides {
        // The runner: the side's first seat (the barker in 1v3's team 0).
        let Some(seat) = game
            .players
            .seats()
            .find(|s| game.format.team(*s) == Some(side as u8))
        else {
            continue;
        };
        let start = lane_point(game, side, 0.0);
        commands.spawn((
            SCOPE,
            Mesh3d(unit.clone()),
            MeshMaterial3d(materials.add(emissive_material(seat_color(seat), 0.5))),
            Transform::from_translation(Vec3::new(0.0, start.y - 0.04, -0.2))
                .with_scale(Vec3::new(2.0 * TRACK_HALF + 0.6, 0.05, 0.05)),
        ));
        // Marks at each quarter of the way.
        for k in 1..4 {
            commands.spawn((
                SCOPE,
                Mesh3d(unit.clone()),
                MeshMaterial3d(materials.add(emissive_material(seat_color(seat), 0.8))),
                Transform::from_translation(
                    lane_point(game, side, k as f32 / 4.0).with_z(-0.2) - Vec3::Y * 0.04,
                )
                .with_scale(Vec3::new(0.04, 0.16, 0.05)),
            ));
        }
        let folk = spawn_lantern_folk(
            commands,
            meshes,
            materials,
            AppState::Minigame,
            seat,
            roster.of(seat),
            Transform::from_translation(start)
                .with_rotation(Quat::from_rotation_y(FRAC_PI_2))
                .with_scale(Vec3::splat(RUNNER_SCALE)),
        );
        commands
            .entity(folk.root)
            .insert(RunnerView { side, shown: 0.0 });
    }
    // The finish line, across every lane.
    let lanes = game.racing_sides().count() as f32;
    commands.spawn((
        SCOPE,
        Mesh3d(unit.clone()),
        MeshMaterial3d(materials.add(emissive_material(GOAL_WHITE, 2.0))),
        Transform::from_xyz(
            TRACK_HALF + 0.12,
            TRACK_Y + (lanes - 1.0) * LANE / 2.0 + 0.2,
            -0.1,
        )
        .with_scale(Vec3::new(0.06, lanes * LANE + 0.2, 0.05)),
    ));
}

/// Where a board's ball is drawn (milli-units on the board), and how big.
fn ball_pose(hand: Hand, before: Hand, alpha: f32) -> (Vec2, f32) {
    let rest = Vec2::new(0.0, -50.0);
    match hand {
        Hand::Ready { .. } | Hand::Charging { .. } => (rest, 1.0),
        Hand::Rolling { t, x, y } => {
            // Rolls up and slows to a stop, the way a ball does uphill.
            let p = ((t as f32 + alpha) / roll_ticks(y) as f32).min(1.0);
            let eased = 1.0 - (1.0 - p) * (1.0 - p);
            (rest.lerp(Vec2::new(x as f32, y as f32), eased), 1.0)
        }
        Hand::Sinking { t, hole } => {
            let hole = HOLES[hole as usize];
            let p = ((t as f32 + alpha) / SINK_TICKS as f32).min(1.0);
            // From where it stopped, into the middle as it drops.
            let from = match before {
                Hand::Rolling { x, y, .. } => Vec2::new(x as f32, y as f32),
                _ => Vec2::new(hole.x as f32, hole.y as f32),
            };
            let to = Vec2::new(hole.x as f32, hole.y as f32);
            (from.lerp(to, p.min(0.5) * 2.0), 1.0 - p)
        }
        Hand::Returning { t, x, y } => {
            // Back down, faster and faster.
            let p = ((t as f32 + alpha) / RETURN_TICKS as f32).min(1.0);
            let from = Vec2::new(x as f32, y as f32);
            (from.lerp(Vec2::new(x as f32, rest.y), p * p), 1.0)
        }
    }
}

fn animate_balls(
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternDerby>>,
    mut balls: Query<(&BallView, &mut Transform)>,
) {
    let alpha = blend(&fixed);
    let game = &active.run.game;
    for (view, mut transform) in &mut balls {
        let (Some(board), Some(was)) = (game.board(view.seat), active.previous.board(view.seat))
        else {
            continue;
        };
        let (at, size) = ball_pose(board.hand, was.hand, alpha);
        transform.translation = board_point(game, board.slot, at.x, at.y) + Vec3::Z * 0.15;
        transform.scale = Vec3::splat(size.max(0.0));
    }
}

fn animate_aims(
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternDerby>>,
    mut dots: Query<(&AimDot, &mut Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let alpha = blend(&fixed);
    let game = &active.run.game;
    for (dot, mut transform) in &mut dots {
        let (Some(board), Some(was)) = (game.board(dot.seat), active.previous.board(dot.seat))
        else {
            continue;
        };
        let aim = was.aim as f32 + (board.aim - was.aim) as f32 * alpha;
        // Up the board to the top, kept inside the rails like the ball.
        let y = LENGTH as f32 * (dot.index + 1) as f32 / AIM_DOTS as f32;
        let across = (HALF_WIDTH - RAIL) as f32;
        let x = (y * aim / 1000.0).clamp(-across, across);
        transform.translation = board_point(game, board.slot, x, y) + Vec3::Z * 0.05;
        // Bright when the next ball can go, dim while one rolls.
        let glow = match board.hand {
            Hand::Ready { .. } | Hand::Charging { .. } => 2.2,
            _ => 0.5,
        };
        if let Some(mut material) = materials.get_mut(&dot.material) {
            material.emissive = seat_color(dot.seat) * glow;
        }
    }
}

fn animate_meters(
    active: Res<ActiveRun<LanternDerby>>,
    mut meters: Query<(&MeterView, &mut Transform, &mut Visibility)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let game = &active.run.game;
    for (view, mut transform, mut visibility) in &mut meters {
        let seat = match view {
            MeterView::Fill(seat, _) | MeterView::Line(seat) => *seat,
        };
        let Some(board) = game.board(seat) else {
            continue;
        };
        let Hand::Charging { t } = board.hand else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *visibility = Visibility::Inherited;
        let power = power_at(t) as f32;
        match view {
            MeterView::Fill(_, material) => {
                let x = (HALF_WIDTH + METER_GAP) as f32;
                transform.translation =
                    board_point(game, board.slot, x, power / 2.0) + Vec3::Z * 0.03;
                transform.scale = Vec3::new(0.16, power * SCALE, 0.06);
                if let Some(mut material) = materials.get_mut(material) {
                    let color = if power as i32 > LENGTH {
                        OVER_RED
                    } else {
                        seat_color(seat)
                    };
                    material.emissive = color * 3.0;
                }
            }
            MeterView::Line(_) => {
                let y = power.min(LENGTH as f32);
                transform.translation = board_point(game, board.slot, 0.0, y) + Vec3::Z * 0.04;
                transform.scale = Vec3::new(2.0 * HALF_WIDTH as f32 * SCALE, 0.025, 0.02);
            }
        }
    }
}

fn animate_lid(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternDerby>>,
    mut lids: Query<(&LidView, &mut Transform, &mut Visibility)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let game = &active.run.game;
    let (Some(barker), Some(was)) = (game.barker, active.previous.barker) else {
        return;
    };
    let alpha = blend(&fixed);
    let now = time.elapsed_secs();
    for (view, mut transform, mut visibility) in &mut lids {
        match view {
            LidView::Cursor(material) => {
                let x = was.x as f32 + (barker.x - was.x) as f32 * alpha;
                let y = was.y as f32 + (barker.y - was.y) as f32 * alpha;
                transform.translation = Vec3::new(x * SCALE, y * SCALE, 0.3);
                let glow = match barker.lid {
                    Lid::Ready => 3.0 + 0.8 * (now * 6.0).sin(),
                    _ => 0.5,
                };
                if let Some(mut material) = materials.get_mut(material) {
                    material.emissive = seat_color(barker.seat) * glow;
                }
            }
            LidView::Cover => {
                let Lid::Shut { slot, hole, .. } = barker.lid else {
                    *visibility = Visibility::Hidden;
                    continue;
                };
                *visibility = Visibility::Inherited;
                let hole = HOLES[hole as usize];
                transform.translation =
                    board_point(game, slot, hole.x as f32, hole.y as f32) + Vec3::Z * 0.25;
                transform.scale = Vec3::splat(hole.radius as f32 * SCALE + 0.06);
            }
        }
    }
}

fn animate_runners(
    time: Res<Time>,
    active: Res<ActiveRun<LanternDerby>>,
    mut runners: Query<(&mut RunnerView, &mut Transform, &mut Pose)>,
) {
    let game = &active.run.game;
    let dt = time.delta_secs();
    let winners: Vec<Seat> = game
        .outcome
        .map(|o| o.winners().collect())
        .unwrap_or_default();
    for (mut runner, mut transform, mut pose) in &mut runners {
        let target = game.sides[runner.side].progress() as f32 / 10_000.0;
        // Trots up to where its points put it.
        let moving = target > runner.shown + 0.002;
        runner.shown += (target - runner.shown) * (dt * 4.0).min(1.0);
        transform.translation = lane_point(game, runner.side, runner.shown);
        let side_won = winners
            .iter()
            .any(|s| game.format.team(*s) == Some(runner.side as u8));
        *pose = match game.outcome {
            Some(_) if side_won => Pose::Celebrate,
            Some(_) => Pose::Slump,
            None if moving => Pose::Hop,
            None => Pose::Idle,
        };
    }
}

fn animate_throwers(
    active: Res<ActiveRun<LanternDerby>>,
    mut throwers: Query<(&ThrowerView, &mut Pose)>,
) {
    let game = &active.run.game;
    let winners: Vec<Seat> = game
        .outcome
        .map(|o| o.winners().collect())
        .unwrap_or_default();
    for (view, mut pose) in &mut throwers {
        let scored = game
            .board(view.seat)
            .and_then(|b| b.last)
            .is_some_and(|(tick, shot)| {
                game.tick - tick < SHOT_TICKS && matches!(shot, Shot::In { .. })
            });
        let blocked = game.barker.is_some_and(|b| b.seat == view.seat)
            && game.throwers().filter_map(|b| b.last).any(|(tick, shot)| {
                game.tick - tick < SHOT_TICKS && matches!(shot, Shot::Shut { .. })
            });
        *pose = if game.outcome.is_some() {
            if winners.contains(&view.seat) {
                Pose::Celebrate
            } else {
                Pose::Slump
            }
        } else if scored || blocked {
            Pose::Hop
        } else {
            Pose::Idle
        };
    }
}

/// Sounds and sparks, once per event: throws, balls dropping in or bouncing out, the lid, the
/// last seconds and the win. Bots' balls are quieter, so four boards don't drown out a person's.
fn react(
    mut commands: Commands,
    active: Res<ActiveRun<LanternDerby>>,
    seats: Res<Seats>,
    runners: Query<(&RunnerView, &Transform)>,
    effects: Res<Effects>,
    sfx: Sfx,
) {
    let Some((before, now)) = active.ticked() else {
        return;
    };
    for seat in Seat::ALL {
        let (Some(was), Some(is)) = (before.board(seat), now.board(seat)) else {
            continue;
        };
        let volume = if seats.is_bot(seat) { -8.0 } else { 0.0 };
        if matches!(was.hand, Hand::Charging { .. }) && matches!(is.hand, Hand::Rolling { .. }) {
            sfx.play_with(Sound::Dash, 1.2, volume - 4.0);
        }
        if was.last == is.last {
            continue;
        }
        let Some((_, shot)) = is.last else {
            continue;
        };
        let at = |hole: u8| {
            let hole = HOLES[hole as usize];
            board_point(now, is.slot, hole.x as f32, hole.y as f32) + Vec3::Z * 0.3
        };
        match shot {
            Shot::In { hole } => {
                let value = HOLES[hole as usize].value;
                sfx.play_with(Sound::CoinsUp, 0.85 + 0.15 * value as f64, volume);
                let strength = 2.0 + 2.0 * value as f32;
                particles::burst(
                    &mut commands,
                    &effects,
                    SCOPE,
                    at(hole),
                    hdr(hole_color(hole as usize), strength),
                );
            }
            Shot::Rim { hole } => {
                sfx.play_with(Sound::Rebound, 1.3, volume);
                particles::burst(
                    &mut commands,
                    &effects,
                    SCOPE,
                    at(hole),
                    hdr(hole_color(hole as usize), 1.0),
                );
            }
            Shot::Shut { hole } => {
                sfx.play_with(Sound::Hit, 1.0, 0.0);
                let barker = now.barker.map_or(seat, |b| b.seat);
                particles::burst(
                    &mut commands,
                    &effects,
                    SCOPE,
                    at(hole),
                    hdr(seat_color(barker), 5.0),
                );
            }
            Shot::Wall => sfx.play_with(Sound::Rebound, 0.8, volume),
            Shot::Short => sfx.play_with(Sound::Step, 0.7, volume - 4.0),
        }
    }
    if let (Some(was), Some(is)) = (before.barker, now.barker)
        && was.lid == Lid::Ready
        && matches!(is.lid, Lid::Shut { .. })
    {
        sfx.play(Sound::Brace);
    }
    if now.phase == Phase::Racing {
        let left = now.ticks_left();
        if left > 0 && left <= 5 * TICK_HZ && left % TICK_HZ == 0 && before.tick != now.tick {
            sfx.play(Sound::Tick);
        }
    }
    if before.outcome.is_none()
        && let Some(outcome) = now.outcome
    {
        sfx.play(Sound::Win);
        for seat in outcome.winners() {
            let Some(team) = now.format.team(seat) else {
                continue;
            };
            if let Some((_, transform)) = runners.iter().find(|(r, _)| r.side == team as usize) {
                particles::burst(
                    &mut commands,
                    &effects,
                    SCOPE,
                    transform.translation + Vec3::Y * 0.6,
                    hdr(seat_color(seat), 6.0),
                );
            }
        }
        particles::burst(
            &mut commands,
            &effects,
            SCOPE,
            Vec3::new(TRACK_HALF, TRACK_Y + 0.6, 0.3),
            hdr(LANTERN_WARM, 6.0),
        );
    }
}

/// The camera looks straight at the boards from in front, centred between the throwers' feet and
/// the race track.
const CAMERA_TARGET: Vec3 = Vec3::new(0.0, 3.1, 0.0);
const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 0.0, 30.0);
/// Half-width of four boards with their meters, the track and the barker (plus margin).
const FRAMED_HALF_WIDTH: f32 = 8.4;
/// From below the throwers (room for the panels) to above the track (room for the banner).
const FRAMED_HEIGHT: f32 = 14.6;

fn camera_transform() -> Transform {
    Transform::from_translation(CAMERA_TARGET + CAMERA_OFFSET).looking_at(CAMERA_TARGET, Vec3::Y)
}

/// Orthographic and straight on, so every board is seen the same way: the meter's top, the line
/// across the board and the hole it's level with line up on screen exactly as in the simulation.
fn camera_projection() -> Projection {
    Projection::Orthographic(OrthographicProjection {
        scaling_mode: ScalingMode::AutoMin {
            min_width: FRAMED_HALF_WIDTH * 2.0,
            min_height: FRAMED_HEIGHT,
        },
        ..OrthographicProjection::default_3d()
    })
}

fn team_index(format: &Format, seat: Seat) -> Option<u8> {
    match format {
        Format::FreeForAll | Format::Duel { .. } => None,
        _ => format.team(seat),
    }
}

/// Seats with a panel, in the order they stand: the throwers left to right under their boards,
/// then the barker on the right.
fn shown_seats(game: &State) -> Vec<Seat> {
    let mut boards: Vec<_> = game.throwers().collect();
    boards.sort_by_key(|b| b.slot);
    boards
        .into_iter()
        .map(|b| b.seat)
        .chain(game.barker.map(|b| b.seat))
        .collect()
}

fn spawn_hud(mut commands: Commands, active: Res<ActiveRun<LanternDerby>>) {
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

    let game = &active.run.game;
    commands
        .spawn((
            SCOPE,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(12.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(16.0),
                ..default()
            },
        ))
        .with_children(|row| {
            for seat in shown_seats(game) {
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

fn banner_text(active: &ActiveRun<LanternDerby>) -> (String, String) {
    let run = &active.run;
    let game = &run.game;
    if let Some(banner) = halted_banner(run) {
        return banner;
    }
    match game.phase {
        // The shared countdown (crate::intro) is drawn over this.
        Phase::Intro { .. } => (String::new(), String::new()),
        Phase::Racing => {
            let title = if game.tick < 45 {
                tr!("lantern-derby-go")
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

/// What a thrower's last ball did, while it's fresh.
fn last_shot(game: &State, seat: Seat) -> Option<String> {
    let (tick, shot) = game.board(seat)?.last?;
    if game.tick - tick >= SHOT_TICKS {
        return None;
    }
    Some(match shot {
        Shot::In { hole } => tr!(
            "lantern-derby-in",
            value = HOLES[hole as usize].value as u32
        ),
        Shot::Rim { .. } => tr!("lantern-derby-rim"),
        Shot::Shut { .. } => tr!("lantern-derby-shut"),
        Shot::Short => tr!("lantern-derby-short"),
        Shot::Wall => tr!("lantern-derby-wall"),
    })
}

fn update_hud(
    active: Res<ActiveRun<LanternDerby>>,
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
                let Some(side) = game.side_of(seat) else {
                    continue;
                };
                let barker = game.barker.filter(|b| b.seat == seat);
                let role = if barker.is_some() {
                    tr!("solo-tag")
                } else {
                    String::new()
                };
                let score = match (barker, side.members) {
                    (Some(_), _) => tr!(
                        "lantern-derby-barker",
                        points = side.points as u32,
                        goal = side.goal as u32
                    ),
                    (None, 1) => tr!(
                        "lantern-derby-points",
                        points = side.points as u32,
                        goal = side.goal as u32
                    ),
                    (None, _) => tr!(
                        "lantern-derby-team",
                        points = side.points as u32,
                        goal = side.goal as u32
                    ),
                };
                let status = if active.run.controller_lost(seat) {
                    tr!("controller-lost")
                } else if let (Some(outcome), true) = (game.outcome, game.phase != Phase::Racing) {
                    match outcome.placement[seat.index()] {
                        Some(place) => tr!("place", place = place as u32),
                        None => String::new(),
                    }
                } else if let Some(barker) = barker {
                    match barker.lid {
                        Lid::Ready => tr!("lantern-derby-lid-ready"),
                        Lid::Shut { .. } => tr!("lantern-derby-lid-shut"),
                        Lid::Cooling { .. } => tr!("lantern-derby-lid-cooling"),
                    }
                } else {
                    last_shot(game, seat).unwrap_or_default()
                };
                format!("{}{who}{role}\n{score}\n{status}", seat_label(seat))
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{Minigame, PlayerSetup};

    /// Fairness across seats: with a parallel projection looking straight at the boards, screen
    /// position depends on world x and y alone, so every board is seen the same way.
    #[test]
    fn every_board_is_seen_the_same_way() {
        assert!(matches!(camera_projection(), Projection::Orthographic(_)));
        let camera = camera_transform();
        assert!(camera.right().abs_diff_eq(Vec3::X, 1e-6));
        assert!(camera.up().abs_diff_eq(Vec3::Y, 1e-6));
    }

    #[test]
    fn boards_their_meters_and_the_track_fit_the_frame() {
        for (players, format) in [
            (4, Format::FreeForAll),
            (4, Format::OneVsThree { solo: Seat(0) }),
            (
                4,
                Format::TwoVsTwo {
                    team_of: [0, 1, 0, 1],
                },
            ),
        ] {
            let game = LanternDerby::new(1, PlayerSetup::first(players), format);
            let mut right = 0.0f32;
            for board in game.throwers() {
                let meter = board_point(&game, board.slot, (HALF_WIDTH + METER_GAP) as f32, 0.0);
                right = right.max(meter.x + 0.1);
            }
            if game.barker.is_some() {
                right = right.max(game.cursor_bounds().1 as f32 * SCALE + 1.6);
            }
            right = right.max(TRACK_HALF + 0.4);
            assert!(right < FRAMED_HALF_WIDTH, "{format:?}: {right}");
            // The top lane's runner stays under the banner.
            let top = lane_point(&game, game.racing_sides().last().unwrap(), 1.0).y + 1.0;
            assert!(
                top < CAMERA_TARGET.y + FRAMED_HEIGHT / 2.0 - 1.2,
                "{format:?}: {top}"
            );
        }
    }

    #[test]
    fn panels_follow_the_boards_then_the_barker() {
        let teams = Format::TwoVsTwo {
            team_of: [0, 1, 0, 1],
        };
        let game = LanternDerby::new(1, PlayerSetup::first(4), teams);
        assert_eq!(shown_seats(&game), [Seat(0), Seat(2), Seat(1), Seat(3)]);
        let solo = Format::OneVsThree { solo: Seat(0) };
        let game = LanternDerby::new(1, PlayerSetup::first(4), solo);
        assert_eq!(shown_seats(&game), [Seat(1), Seat(2), Seat(3), Seat(0)]);
    }

    #[test]
    fn a_rolling_ball_ends_where_the_simulation_stops_it() {
        let hand = Hand::Rolling {
            t: roll_ticks(900) - 1,
            x: 120,
            y: 900,
        };
        let (at, _) = ball_pose(hand, hand, 1.0);
        assert!(at.abs_diff_eq(Vec2::new(120.0, 900.0), 1e-3), "{at}");
    }
}
