//! Presentation for Triple Jump. Reads `ActiveRun<TripleJump>` only; never writes simulation
//! state. The lanes run left to right at night, side by side away from the camera (the first lane
//! furthest back, at the top of the screen): the runway, the take-off board with its foul line,
//! and the glowing sand pit. A bar on the ground beside each runner shows its speed, a needle at
//! its feet the angle gauge (with a faint one at 45°), and a post in the sand its best mark. In
//! 1v3 a line across the three's lanes marks the solo's jump, and the solo's fan stands at the far
//! end of the lane it blows down.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::camera::ScalingMode;
use bevy::prelude::*;
use minigame_triple_jump::{
    Athlete, BEST_ANGLE, Grade, MM, Mark, Motion, Next, Phase, QUALIFIERS, RUNWAY, State,
    TOP_SPEED, TripleJump, peak_height,
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

pub struct TripleJumpViewPlugin;

impl Plugin for TripleJumpViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            (spawn_scene, spawn_hud)
                .after(MinigameSet::Start)
                .run_if(resource_exists::<ActiveRun<TripleJump>>),
        )
        .add_systems(
            Update,
            (
                animate_athletes,
                animate_speed_bars,
                animate_gauges,
                animate_marks,
                animate_wind,
                place_labels,
                update_hud,
            )
                .run_if(resource_exists::<ActiveRun<TripleJump>>),
        )
        .add_systems(
            FixedUpdate,
            react
                .in_set(MinigameSet::React)
                .run_if(resource_exists::<ActiveRun<TripleJump>>),
        );
    }
}

const SCOPE: DespawnOnExit<AppState> = DespawnOnExit(AppState::Minigame);
/// The runway starts here (metres from the take-off line; one world unit is a metre)...
const RUN_START: f32 = -(RUNWAY / (1000 * MM)) as f32;
/// ...and the sand pit runs between these.
const PIT_START: f32 = 6.0;
const PIT_END: f32 = 18.5;
/// Where the 1v3 fan stands, past the pit.
const FAN_X: f32 = 20.5;
/// Lanes stand this far apart, front to back.
const LANE_GAP: f32 = 9.5;
const LANE_WIDTH: f32 = 5.0;
const ATHLETE_SCALE: f32 = 1.5;
/// Jumps are drawn this much higher than the simulation's arc, so they read at this distance.
const JUMP_LIFT: f32 = 1.6;
/// The speed bar on the ground in front of a runner, at full speed.
const BAR_LENGTH: f32 = 3.0;
const NEEDLE_LENGTH: f32 = 3.0;
/// How long a launch's angle, a touchdown's grade or a result stays up, in ticks.
const EVENT_TICKS: u32 = 50;
/// Distances marked along the top lane, in metres.
const DISTANCE_MARKS: [u32; 6] = [6, 8, 10, 12, 14, 16];
const TRACK_RED: LinearRgba = LinearRgba::rgb(0.8, 0.22, 0.14);
const SAND: LinearRgba = LinearRgba::rgb(1.0, 0.7, 0.35);
const BOARD_WHITE: LinearRgba = LinearRgba::rgb(0.85, 0.9, 1.0);
const FOUL_RED: LinearRgba = LinearRgba::rgb(1.0, 0.1, 0.06);
/// Team colours for the panel borders in team formats.
const TEAM_COLORS: [LinearRgba; 2] = [
    LinearRgba::rgb(1.0, 0.78, 0.3),
    LinearRgba::rgb(0.55, 0.45, 1.0),
];

fn metres(um: i32) -> f32 {
    um as f32 / (1000 * MM) as f32
}

fn lane_z(game: &State, lane: usize) -> f32 {
    (lane as f32 - (game.lanes() as f32 - 1.0) / 2.0) * LANE_GAP
}

/// The seat whose colour a lane wears: its first member.
fn lane_seat(athlete: &Athlete) -> Seat {
    athlete.members[0]
}

fn blend(fixed: &Time<Fixed>) -> f32 {
    fixed.overstep_fraction().clamp(0.0, 1.0)
}

/// "12.34", from mm.
fn format_metres(mm: i32) -> String {
    format!("{}.{:02}", mm / 1000, (mm % 1000) / 10)
}

/// Whether `athlete` is the 1v3 solo, blowing the wind after its own jump.
fn is_wind(game: &State, athlete: &Athlete) -> bool {
    game.wind.is_some_and(|w| athlete.plays(w.seat))
}

#[derive(Component)]
struct CameraRig;

#[derive(Component)]
struct AthleteView {
    lane: usize,
    /// Which member: in 2v2 the runner is on the track and the coach stands by the board.
    seat: Seat,
}

#[derive(Component)]
struct SpeedBar {
    lane: usize,
    fill: bool,
}

#[derive(Component)]
enum GaugeView {
    Needle(usize),
    Target(usize),
}

#[derive(Component)]
enum MarkView {
    /// The best mark's post in the sand.
    Best(usize),
    /// Where this attempt landed.
    Last(usize),
}

#[derive(Component)]
enum WindView {
    Fan,
    Blades,
    /// One of the streaks that blow down the lane, by index.
    Streak(usize),
    /// 1v3: the solo's mark, across the three's lanes.
    Line,
    /// A glow over the lane the wind blows down, as strong as the wind.
    Glow,
}

const STREAKS: usize = 18;

#[derive(Component, Clone, Copy)]
enum Label {
    /// Beside each lane's start: whose lane it is.
    Lane(usize),
    /// Over each athlete: the angle, the grade, the result.
    Event(usize),
    /// Along the top lane's pit.
    Distance(u32),
    /// 1v3: by the solo's line.
    Line,
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
    active: Res<ActiveRun<TripleJump>>,
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
        CameraRig,
        night_camera(camera_transform(), (45.0, 110.0)),
        camera_projection(),
    ));
    commands.spawn((
        SCOPE,
        DirectionalLight {
            color: MOONLIGHT,
            illuminance: 300.0,
            ..default()
        },
        Transform::from_xyz(-6.0, 12.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    spawn_fireflies(
        &mut commands,
        &mut meshes,
        &mut materials,
        AppState::Minigame,
        30,
        Vec3::new(-24.0, 1.0, -14.0),
        Vec3::new(22.0, 6.0, -10.0),
    );

    let game = &active.run.game;
    let unit = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    for athlete in game.athletes() {
        let lane = athlete.lane as usize;
        spawn_lane(&mut commands, &mut materials, &unit, game, athlete);
        for &member in &athlete.members[..athlete.member_count as usize] {
            let folk = spawn_lantern_folk(
                &mut commands,
                &mut meshes,
                &mut materials,
                AppState::Minigame,
                member,
                roster.of(member),
                Transform::from_xyz(RUN_START, 0.0, lane_z(game, lane))
                    .with_scale(Vec3::splat(ATHLETE_SCALE)),
            );
            commands
                .entity(folk.root)
                .insert(AthleteView { lane, seat: member });
        }
        let seat = lane_seat(athlete);

        let color = seat_color(seat);
        let bar_z = lane_z(game, lane) + LANE_WIDTH / 2.0 - 0.35;
        for fill in [false, true] {
            commands.spawn((
                SCOPE,
                SpeedBar { lane, fill },
                Mesh3d(unit.clone()),
                MeshMaterial3d(materials.add(if fill {
                    emissive_material(color, 3.0)
                } else {
                    emissive_material(color, 0.25)
                })),
                Transform::from_xyz(0.0, 0.0, bar_z),
                Visibility::Hidden,
            ));
        }
        commands.spawn((
            SCOPE,
            GaugeView::Target(lane),
            Mesh3d(unit.clone()),
            MeshMaterial3d(materials.add(emissive_material(BOARD_WHITE, 1.6))),
            Transform::default(),
            Visibility::Hidden,
        ));
        commands.spawn((
            SCOPE,
            GaugeView::Needle(lane),
            Mesh3d(unit.clone()),
            MeshMaterial3d(materials.add(emissive_material(color, 4.0))),
            Transform::default(),
            Visibility::Hidden,
        ));
        commands.spawn((
            SCOPE,
            MarkView::Best(lane),
            Mesh3d(meshes.add(Cylinder::new(0.06, 1.0))),
            MeshMaterial3d(materials.add(emissive_material(color, 2.5))),
            Transform::default(),
            Visibility::Hidden,
        ));
        commands.spawn((
            SCOPE,
            MarkView::Last(lane),
            Mesh3d(meshes.add(Cylinder::new(0.3, 0.04))),
            MeshMaterial3d(materials.add(emissive_material(BOARD_WHITE, 2.0))),
            Transform::default(),
            Visibility::Hidden,
        ));
    }
    if let Some(wind) = game.wind {
        spawn_wind(&mut commands, &mut meshes, &mut materials, &unit, wind.seat);
    }
}

fn spawn_lane(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    unit: &Handle<Mesh>,
    game: &State,
    athlete: &Athlete,
) {
    let z = lane_z(game, athlete.lane as usize);
    let color = seat_color(lane_seat(athlete));
    let mut slab = |material: Handle<StandardMaterial>, from: f32, to: f32, y: f32, depth: f32| {
        commands.spawn((
            SCOPE,
            Mesh3d(unit.clone()),
            MeshMaterial3d(material),
            Transform::from_xyz((from + to) / 2.0, y, z).with_scale(Vec3::new(
                to - from,
                0.1,
                depth,
            )),
        ));
    };
    let runway = materials.add(emissive_material(TRACK_RED, 0.16));
    slab(runway, RUN_START - 0.8, PIT_START, -0.05, LANE_WIDTH);
    slab(
        materials.add(emissive_material(SAND, 0.5)),
        PIT_START,
        PIT_END,
        -0.04,
        LANE_WIDTH,
    );
    // The take-off board, and the foul line just past it.
    slab(
        materials.add(emissive_material(BOARD_WHITE, 1.2)),
        -0.25,
        0.0,
        -0.03,
        LANE_WIDTH,
    );
    slab(
        materials.add(emissive_material(FOUL_RED, 2.0)),
        0.0,
        0.08,
        -0.02,
        LANE_WIDTH,
    );
    // The lane's edges in its colour.
    let edge = materials.add(emissive_material(color, 1.0));
    for side in [-1.0, 1.0] {
        commands.spawn((
            SCOPE,
            Mesh3d(unit.clone()),
            MeshMaterial3d(edge.clone()),
            Transform::from_xyz(
                (RUN_START - 0.8 + PIT_END) / 2.0,
                0.0,
                z + side * LANE_WIDTH / 2.0,
            )
            .with_scale(Vec3::new(PIT_END - RUN_START + 0.8, 0.06, 0.06)),
        ));
    }
    // A line across the pit every metre, brighter every other.
    for m in PIT_START as i32..=PIT_END as i32 {
        let strength = if m % 2 == 0 { 0.9 } else { 0.3 };
        commands.spawn((
            SCOPE,
            Mesh3d(unit.clone()),
            MeshMaterial3d(materials.add(emissive_material(BOARD_WHITE, strength))),
            Transform::from_xyz(m as f32, 0.0, z).with_scale(Vec3::new(0.03, 0.04, LANE_WIDTH)),
        ));
    }
}

fn spawn_wind(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    unit: &Handle<Mesh>,
    seat: Seat,
) {
    let color = seat_color(seat);
    // A ring facing down the lanes, with blades that spin as it blows.
    commands
        .spawn((
            SCOPE,
            WindView::Fan,
            // Turned part way to the camera, so the ring reads as a fan rather than a bar.
            Transform::from_xyz(FAN_X, 1.4, 0.0).with_rotation(Quat::from_rotation_y(-0.8)),
            Visibility::Hidden,
        ))
        .with_children(|fan| {
            fan.spawn((
                Mesh3d(meshes.add(Torus::new(1.05, 1.25))),
                MeshMaterial3d(materials.add(emissive_material(color, 2.5))),
                Transform::from_rotation(Quat::from_rotation_z(FRAC_PI_2)),
            ));
            fan.spawn((
                Mesh3d(unit.clone()),
                MeshMaterial3d(materials.add(emissive_material(color, 0.4))),
                Transform::from_xyz(0.35, -0.75, 0.0).with_scale(Vec3::new(0.2, 1.4, 0.2)),
            ));
            fan.spawn((
                WindView::Blades,
                Transform::default(),
                Visibility::Inherited,
            ))
            .with_children(|blades| {
                for k in 0..3 {
                    blades.spawn((
                        Mesh3d(unit.clone()),
                        MeshMaterial3d(materials.add(emissive_material(color, 1.2))),
                        Transform::from_rotation(Quat::from_rotation_x(k as f32 * TAU / 3.0))
                            * Transform::from_xyz(0.0, 0.5, 0.0)
                                .with_scale(Vec3::new(0.06, 0.95, 0.28)),
                    ));
                }
            });
        });
    let streak = materials.add(emissive_material(color, 1.0));
    for index in 0..STREAKS {
        commands.spawn((
            SCOPE,
            WindView::Streak(index),
            Mesh3d(unit.clone()),
            MeshMaterial3d(streak.clone()),
            Transform::from_scale(Vec3::new(1.4, 0.04, 0.04)),
            Visibility::Hidden,
        ));
    }
    commands.spawn((
        SCOPE,
        WindView::Glow,
        Mesh3d(unit.clone()),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::BLACK,
            alpha_mode: AlphaMode::Add,
            unlit: true,
            ..default()
        })),
        Transform::from_xyz((RUN_START + FAN_X) / 2.0, 0.02, 0.0).with_scale(Vec3::new(
            FAN_X - RUN_START,
            0.02,
            LANE_WIDTH,
        )),
        Visibility::Hidden,
    ));
    commands.spawn((
        SCOPE,
        WindView::Line,
        Mesh3d(unit.clone()),
        MeshMaterial3d(materials.add(emissive_material(color, 3.0))),
        Transform::default(),
        Visibility::Hidden,
    ));
}

/// Where an athlete is drawn and how: its spot along the lane, its height off the ground, how
/// far it leans forward, and how squashed it is (crouched to take off).
fn athlete_pose(athlete: &Athlete, before: &Athlete, alpha: f32) -> (f32, f32, f32, f32) {
    let x = metres(athlete.x);
    let was = metres(before.x);
    // Blend between ticks, but not across a jump back to the start.
    let x = if (x - was).abs() < 2.0 {
        was + (x - was) * alpha
    } else {
        x
    };
    let pace = athlete.speed as f32 / TOP_SPEED as f32;
    match athlete.motion {
        Motion::Running => {
            let stride = (x * 2.2).sin().abs() * 0.15 * pace.min(1.0);
            (x, stride, 0.2 * pace.min(1.0), 1.0)
        }
        Motion::Planted { gauge, .. } => (
            x,
            0.0,
            0.1,
            0.92 - 0.1 * (gauge.ticks as f32 / 40.0).min(1.0),
        ),
        Motion::Flying { t, ticks, .. } => {
            let p = ((t as f32 + alpha) / ticks as f32).min(1.0);
            let height = metres(peak_height(ticks)) * JUMP_LIFT * 4.0 * p * (1.0 - p);
            (x, height, 0.1, 1.0)
        }
        Motion::Landed { .. } => (x, 0.0, 0.15, 0.88),
        Motion::Done => (x, 0.0, 0.0, 1.0),
    }
}

fn animate_athletes(
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<TripleJump>>,
    mut athletes: Query<(&AthleteView, &mut Transform, &mut Pose)>,
) {
    let alpha = blend(&fixed);
    let game = &active.run.game;
    let winners: Vec<Seat> = game
        .outcome
        .map(|o| o.winners().collect())
        .unwrap_or_default();
    for (view, mut transform, mut pose) in &mut athletes {
        let (Some(athlete), Some(before)) = (
            game.athletes[view.lane],
            active.previous.athletes[view.lane],
        ) else {
            continue;
        };
        let z = lane_z(game, view.lane);
        if athlete.runner(game.attempt) != view.seat {
            // 2v2: the coach stands behind the board, facing the camera, watching the gauge.
            transform.translation = Vec3::new(-2.0, 0.0, z - LANE_WIDTH / 2.0 - 0.3);
            transform.rotation = Quat::IDENTITY;
            transform.scale = Vec3::splat(ATHLETE_SCALE * 0.8);
        } else if is_wind(game, &athlete) && game.attempt > 0 {
            // Done jumping: the solo minds the fan, facing back down the lanes.
            let fan_z = game
                .wind
                .map_or(z, |w| fan_z(game, w.lane, w.from, w.travel));
            transform.translation = Vec3::new(FAN_X + 1.6, 0.0, fan_z);
            transform.rotation = Quat::from_rotation_y(-FAN_FACING);
            transform.scale = Vec3::splat(ATHLETE_SCALE);
        } else {
            let (x, y, lean, squash) = athlete_pose(&athlete, &before, alpha);
            transform.translation = Vec3::new(x, y, z);
            transform.rotation = Quat::from_rotation_y(FRAC_PI_2) * Quat::from_rotation_x(lean);
            transform.scale = Vec3::new(1.0, squash, 1.0) * ATHLETE_SCALE;
        }
        let mark = athlete.marks[game.attempt as usize];
        *pose = if game.outcome.is_some() {
            if athlete.members.iter().any(|s| winners.contains(s)) {
                Pose::Celebrate
            } else {
                Pose::Slump
            }
        } else if athlete.motion == Motion::Done && matches!(mark, Mark::Foul | Mark::Stumble(_)) {
            Pose::Slump
        } else {
            Pose::Idle
        };
    }
}

/// The solo turns to face the three from beside its fan.
const FAN_FACING: f32 = FRAC_PI_2;

/// Where the fan is across the lanes, moving `travel` ticks from `from` to `lane`.
fn fan_z(game: &State, lane: u8, from: u8, travel: u16) -> f32 {
    let p = travel as f32 / minigame_triple_jump::FAN_MOVE_TICKS as f32;
    lane_z(game, lane as usize) * (1.0 - p) + lane_z(game, from as usize) * p
}

fn animate_speed_bars(
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<TripleJump>>,
    mut bars: Query<(&SpeedBar, &mut Transform, &mut Visibility)>,
) {
    let alpha = blend(&fixed);
    let game = &active.run.game;
    for (bar, mut transform, mut visibility) in &mut bars {
        let (Some(athlete), Some(before)) =
            (game.athletes[bar.lane], active.previous.athletes[bar.lane])
        else {
            continue;
        };
        let on_runway = matches!(athlete.motion, Motion::Running | Motion::Planted { .. })
            && matches!(game.phase, Phase::Jumping { .. } | Phase::Ready { .. });
        if !on_runway {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = Visibility::Inherited;
        let (x, ..) = athlete_pose(&athlete, &before, alpha);
        let share = if bar.fill {
            (athlete.speed as f32 / TOP_SPEED as f32).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let length = (BAR_LENGTH * share).max(0.001);
        // Starting a little behind the runner and growing forwards.
        let start = x - BAR_LENGTH / 2.0;
        transform.translation.x = start + length / 2.0;
        transform.translation.y = if bar.fill { 0.05 } else { 0.03 };
        transform.scale = Vec3::new(length, 0.05, if bar.fill { 0.22 } else { 0.3 });
    }
}

/// The angle on an athlete's gauge right now, if one is showing: climbing, or just launched.
fn shown_angle(game: &State, athlete: &Athlete) -> Option<u16> {
    let rate = game.angle_rate();
    match athlete.motion {
        Motion::Planted { gauge, .. } => Some(gauge.angle(rate)),
        Motion::Flying {
            next: Next::Early { gauge, .. },
            ..
        } => Some(gauge.angle(rate)),
        _ => athlete
            .last_angle
            .filter(|(tick, _)| game.tick - tick < EVENT_TICKS / 2)
            .map(|(_, angle)| angle),
    }
}

fn animate_gauges(
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<TripleJump>>,
    mut gauges: Query<(&GaugeView, &mut Transform, &mut Visibility)>,
) {
    let alpha = blend(&fixed);
    let game = &active.run.game;
    for (view, mut transform, mut visibility) in &mut gauges {
        let lane = match view {
            GaugeView::Needle(lane) | GaugeView::Target(lane) => *lane,
        };
        let (Some(athlete), Some(before)) = (game.athletes[lane], active.previous.athletes[lane])
        else {
            continue;
        };
        let Some(angle) = shown_angle(game, &athlete) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *visibility = Visibility::Inherited;
        let (degrees, length, thickness) = match view {
            GaugeView::Needle(_) => (angle as f32 / 10.0, NEEDLE_LENGTH, 0.09),
            GaugeView::Target(_) => (BEST_ANGLE as f32 / 10.0, NEEDLE_LENGTH * 1.1, 0.06),
        };
        let (x, ..) = athlete_pose(&athlete, &before, alpha);
        // From just in front of the feet, towards the camera so the body doesn't hide it.
        let pivot = Vec3::new(x + 0.4, 0.1, lane_z(game, lane) + 1.2);
        let rotation = Quat::from_rotation_z(degrees.to_radians());
        transform.translation = pivot + rotation * Vec3::X * (length / 2.0);
        transform.rotation = rotation;
        transform.scale = Vec3::new(length, thickness, thickness);
    }
}

fn animate_marks(
    active: Res<ActiveRun<TripleJump>>,
    mut marks: Query<(&MarkView, &mut Transform, &mut Visibility)>,
) {
    let game = &active.run.game;
    for (view, mut transform, mut visibility) in &mut marks {
        let lane = match view {
            MarkView::Best(lane) | MarkView::Last(lane) => *lane,
        };
        let Some(athlete) = game.athletes[lane] else {
            continue;
        };
        let z = lane_z(game, lane);
        let at = match view {
            MarkView::Best(_) => athlete.best().0,
            MarkView::Last(_) => athlete.marks[game.attempt as usize].distance(),
        };
        let Some(mm) = at else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *visibility = Visibility::Inherited;
        let x = mm as f32 / 1000.0;
        *transform = match view {
            MarkView::Best(_) => Transform::from_xyz(x, 0.5, z - 0.9),
            MarkView::Last(_) => Transform::from_xyz(x, 0.02, z),
        };
    }
}

fn animate_wind(
    time: Res<Time>,
    active: Res<ActiveRun<TripleJump>>,
    mut views: Query<(&WindView, &mut Transform, &mut Visibility)>,
    tinted: Query<(&WindView, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut spin: Local<f32>,
) {
    let game = &active.run.game;
    let Some(wind) = game.wind else {
        return;
    };
    let windy = game.attempt > 0;
    let strength = wind.strength as f32 / 1000.0;
    let blowing = windy && wind.travel == 0 && wind.strength > 0;
    let z = fan_z(game, wind.lane, wind.from, wind.travel);
    let now = time.elapsed_secs();
    *spin += time.delta_secs() * (1.0 + 18.0 * strength);
    let line_lanes = game.lanes() as usize - 1;
    for (view, mut transform, mut visibility) in &mut views {
        match view {
            WindView::Fan => {
                *visibility = if windy {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                transform.translation.z = z;
            }
            WindView::Blades => transform.rotation = Quat::from_rotation_x(*spin),
            WindView::Streak(index) => {
                if !blowing {
                    *visibility = Visibility::Hidden;
                    continue;
                }
                *visibility = Visibility::Inherited;
                // Each streak loops down the lane from the fan, at its own height and pace.
                let k = *index as f32;
                let span = FAN_X - RUN_START;
                let speed = 10.0 + 25.0 * strength;
                let along = (now * speed + k * 37.3) % span;
                let y = 0.3 + (k * 0.618).fract() * 0.9;
                let side = ((k * 0.377).fract() - 0.5) * LANE_WIDTH * 0.6;
                transform.translation = Vec3::new(FAN_X - 1.0 - along, y, z + side);
            }
            WindView::Glow => {
                *visibility = if blowing {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                transform.translation.z = z;
            }
            WindView::Line => {
                let Some(line) = game.line() else {
                    *visibility = Visibility::Hidden;
                    continue;
                };
                *visibility = Visibility::Inherited;
                // Across the three's lanes (1 and on).
                let first = lane_z(game, 1);
                let last = lane_z(game, line_lanes);
                transform.translation = Vec3::new(line as f32 / 1000.0, 0.4, (first + last) / 2.0);
                transform.scale = Vec3::new(0.08, 0.8, last - first + LANE_WIDTH);
            }
        }
    }
    // The streaks (which share a material) and the glow brighten the harder it blows.
    let color = seat_color(wind.seat);
    for (view, handle) in &tinted {
        let Some(mut material) = materials.get_mut(&handle.0) else {
            continue;
        };
        match view {
            WindView::Streak(0) => material.emissive = color * (0.4 + 2.6 * strength),
            WindView::Glow => material.base_color = Color::from(color * (0.05 + 0.3 * strength)),
            _ => {}
        }
    }
}

/// Sounds and sparks, once per event: take-offs and launches, touchdowns, landings, fouls and
/// stumbles, gusts, a new longest jump, and the win. Bots' sounds are quieter.
fn react(
    mut commands: Commands,
    active: Res<ActiveRun<TripleJump>>,
    seats: Res<Seats>,
    effects: Res<Effects>,
    sfx: Sfx,
) {
    let Some((before, now)) = active.ticked() else {
        return;
    };
    for lane in 0..now.athletes.len() {
        let (Some(was), Some(is)) = (before.athletes[lane], now.athletes[lane]) else {
            continue;
        };
        let runner = is.runner(now.attempt);
        let volume = if seats.is_bot(runner) { -9.0 } else { 0.0 };
        let at = Vec3::new(metres(is.x), 0.3, lane_z(now, lane));
        // Footsteps, one a stride.
        if is.motion == Motion::Running
            && was.motion == Motion::Running
            && ((metres(is.x) / 2.4).floor() != (metres(was.x) / 2.4).floor())
        {
            let pace = is.speed as f32 / TOP_SPEED as f32;
            sfx.play_with(Sound::Step, 0.8 + 0.5 * pace as f64, volume - 8.0);
        }
        match (was.motion, is.motion) {
            (Motion::Running, Motion::Planted { .. }) => {
                sfx.play_with(Sound::Brace, 1.2, volume);
            }
            (Motion::Planted { .. } | Motion::Flying { .. }, Motion::Flying { t: 0, .. }) => {
                sfx.play_with(Sound::Dash, 1.1, volume);
            }
            _ => {}
        }
        if is.last_grade != was.last_grade
            && let Some((_, grade)) = is.last_grade
        {
            let (sound, rate) = match grade {
                Grade::Perfect => (Sound::CoinsUp, 1.3),
                Grade::Good => (Sound::Step, 1.2),
                Grade::Off => (Sound::Step, 0.8),
            };
            sfx.play_with(sound, rate, volume);
            if grade == Grade::Perfect {
                particles::burst(
                    &mut commands,
                    &effects,
                    SCOPE,
                    at,
                    hdr(seat_color(runner), 2.5),
                );
            }
        }
        let attempt = now.attempt as usize;
        if was.marks[attempt] != is.marks[attempt] {
            match is.marks[attempt] {
                Mark::Landed(_) => {
                    sfx.play_with(Sound::Fall, 0.8, volume);
                    particles::burst(&mut commands, &effects, SCOPE, at, hdr(SAND, 3.0));
                }
                Mark::Stumble(_) => sfx.play_with(Sound::Fall, 1.25, volume),
                Mark::Foul => sfx.play_with(Sound::FalseStart, 1.0, volume),
                Mark::Pending => {}
            }
        }
    }
    if let (Some(was), Some(is)) = (before.wind, now.wind)
        && is.last_gust != was.last_gust
    {
        sfx.play(Sound::Rumble);
        if let Some(athlete) = now.athletes[is.lane as usize] {
            particles::burst(
                &mut commands,
                &effects,
                SCOPE,
                Vec3::new(metres(athlete.x), 1.2, lane_z(now, is.lane as usize)),
                hdr(seat_color(is.seat), 5.0),
            );
        }
    }
    // A new longest jump of the match, as the marks go up.
    if matches!(now.phase, Phase::Marks { t: 0 }) && longest(now) > longest(before) {
        sfx.play(Sound::CoinsUp);
    }
    if before.outcome.is_none()
        && let Some(outcome) = now.outcome
    {
        sfx.play(Sound::Win);
        for athlete in now.athletes() {
            if athlete
                .members
                .iter()
                .any(|s| outcome.placement[s.index()] == Some(1))
            {
                particles::burst(
                    &mut commands,
                    &effects,
                    SCOPE,
                    Vec3::new(metres(athlete.x), 2.0, lane_z(now, athlete.lane as usize)),
                    hdr(seat_color(lane_seat(athlete)), 6.0),
                );
            }
        }
        particles::burst(
            &mut commands,
            &effects,
            SCOPE,
            Vec3::new((PIT_START + PIT_END) / 2.0, 2.0, 0.0),
            hdr(LANTERN_WARM, 6.0),
        );
    }
}

/// The longest mark so far, in mm, of anyone jumping for themselves.
fn longest(game: &State) -> i32 {
    game.athletes()
        .filter(|a| !is_wind(game, a))
        .filter_map(|a| a.best().0)
        .max()
        .unwrap_or(0)
}

/// The camera looks across the lanes from in front and above, at the middle of the whole lane.
const CAMERA_TARGET: Vec3 = Vec3::new((RUN_START + FAN_X) / 2.0 - 0.5, 3.0, 0.0);
const CAMERA_PITCH: f32 = 0.45;
const CAMERA_DISTANCE: f32 = 70.0;
/// From the runway's start to the fan, plus margin.
const FRAMED_WIDTH: f32 = 54.0;
/// The lanes, the jumps over the top lane, and bands for the banner and panels.
const FRAMED_HEIGHT: f32 = 32.0;

fn camera_transform() -> Transform {
    let offset = Vec3::new(0.0, CAMERA_PITCH.sin(), CAMERA_PITCH.cos()) * CAMERA_DISTANCE;
    Transform::from_translation(CAMERA_TARGET + offset).looking_at(CAMERA_TARGET, Vec3::Y)
}

/// Orthographic, with the lanes running straight across the screen: every lane is seen at the
/// same scale, so the board, the gauge and the marks read the same in any lane.
fn camera_projection() -> Projection {
    Projection::Orthographic(OrthographicProjection {
        scaling_mode: ScalingMode::AutoMin {
            min_width: FRAMED_WIDTH,
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

/// Seats with a panel, in lane order (partners together).
fn shown_seats(game: &State) -> Vec<Seat> {
    game.athletes()
        .flat_map(|a| a.members[..a.member_count as usize].to_vec())
        .collect()
}

const LABEL_SIZE: Vec2 = Vec2::new(240.0, 60.0);

fn spawn_hud(mut commands: Commands, active: Res<ActiveRun<TripleJump>>) {
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
    let mut labels: Vec<(Label, f32, Color)> = Vec::new();
    for athlete in game.athletes() {
        let lane = athlete.lane as usize;
        let color = Color::from(seat_color(lane_seat(athlete)) * 1.6);
        labels.push((Label::Lane(lane), 24.0, color));
        labels.push((Label::Event(lane), 24.0, Color::srgb(1.0, 0.95, 0.85)));
    }
    for m in DISTANCE_MARKS {
        labels.push((Label::Distance(m), 16.0, Color::srgba(0.85, 0.9, 1.0, 0.7)));
    }
    if let Some(wind) = game.wind {
        labels.push((Label::Line, 20.0, Color::from(seat_color(wind.seat) * 1.6)));
    }
    for (label, size, color) in labels {
        commands
            .spawn((
                SCOPE,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(LABEL_SIZE.x),
                    height: Val::Px(LABEL_SIZE.y),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
            ))
            .with_child((
                label,
                Text::new(""),
                TextFont::from_font_size(size),
                TextColor(color),
                TextLayout::justify(Justify::Center),
            ));
    }

    commands
        .spawn((
            SCOPE,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(12.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(14.0),
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
                        width: Val::Px(220.0),
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

/// What an athlete just did, while it's fresh: its result once it's down, a press too early,
/// then the newest of its last touchdown's grade and its last launch's angle.
fn event_text(game: &State, athlete: &Athlete) -> String {
    if !game.takes_part(athlete, game.attempt) {
        return String::new();
    }
    if athlete.motion == Motion::Done
        && matches!(game.phase, Phase::Jumping { .. } | Phase::Marks { .. })
    {
        return match athlete.marks[game.attempt as usize] {
            Mark::Landed(mm) => tr!("triple-jump-metres", metres = format_metres(mm)),
            Mark::Stumble(mm) => tr!("triple-jump-stumble", metres = format_metres(mm.max(0))),
            Mark::Foul => tr!("triple-jump-foul"),
            Mark::Pending => String::new(),
        };
    }
    if matches!(
        athlete.motion,
        Motion::Flying {
            next: Next::Locked,
            ..
        }
    ) {
        return tr!("triple-jump-early");
    }
    let fresh = |tick: u32| game.tick - tick < EVENT_TICKS;
    let grade = athlete.last_grade.filter(|(t, _)| fresh(*t));
    let angle = athlete.last_angle.filter(|(t, _)| fresh(*t));
    match (grade, angle) {
        (Some((g, grade)), angle) if angle.is_none_or(|(a, _)| g > a) => match grade {
            Grade::Perfect => tr!("triple-jump-perfect"),
            Grade::Good => tr!("triple-jump-good"),
            Grade::Off => tr!("triple-jump-ok"),
        },
        (_, Some((_, angle))) => tr!("triple-jump-angle", angle = (angle / 10) as u32),
        _ => String::new(),
    }
}

/// Lane names: "P1", or the runner then the coach in 2v2.
fn lane_name(game: &State, athlete: &Athlete) -> String {
    match athlete.coach(game.attempt) {
        Some(coach) => format!(
            "{} + {}",
            seat_label(athlete.runner(game.attempt)),
            seat_label(coach)
        ),
        None => seat_label(lane_seat(athlete)),
    }
}

/// Pin each label to its spot on the track, and fill it in.
fn place_labels(
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<TripleJump>>,
    camera: Query<(&Camera, &GlobalTransform), With<CameraRig>>,
    mut labels: Query<(&Label, &mut Text, &ChildOf)>,
    mut boxes: Query<&mut Node>,
) {
    let Some((camera, camera_transform)) = camera.iter().next() else {
        return;
    };
    let alpha = blend(&fixed);
    let game = &active.run.game;
    for (label, mut text, parent) in &mut labels {
        let at = match *label {
            Label::Lane(lane) => {
                let Some(athlete) = game.athletes[lane] else {
                    continue;
                };
                text.0 = lane_name(game, &athlete);
                Vec3::new(RUN_START - 2.6, 0.0, lane_z(game, lane))
            }
            Label::Event(lane) => {
                let (Some(athlete), Some(before)) =
                    (game.athletes[lane], active.previous.athletes[lane])
                else {
                    continue;
                };
                text.0 = event_text(game, &athlete);
                let (x, y, ..) = athlete_pose(&athlete, &before, alpha);
                Vec3::new(x, y + 5.2, lane_z(game, lane))
            }
            Label::Distance(m) => {
                text.0 = format!("{m} m");
                Vec3::new(m as f32, 0.0, lane_z(game, 0) - LANE_WIDTH / 2.0 - 0.6)
            }
            Label::Line => {
                let Some(line) = game.line() else {
                    text.0 = String::new();
                    continue;
                };
                let solo = game.wind.map_or(Seat(0), |w| w.seat);
                text.0 = tr!(
                    "triple-jump-line",
                    player = seat_label(solo),
                    metres = format_metres(line)
                );
                let last = lane_z(game, game.lanes() as usize - 1);
                Vec3::new(line as f32 / 1000.0, 0.0, last + LANE_WIDTH / 2.0 + 0.8)
            }
        };
        if let (Ok(screen), Ok(mut node)) = (
            camera.world_to_viewport(camera_transform, at),
            boxes.get_mut(parent.parent()),
        ) {
            node.left = Val::Px(screen.x - LABEL_SIZE.x / 2.0);
            node.top = Val::Px(screen.y - LABEL_SIZE.y / 2.0);
        }
    }
}

/// The attempt the three are on in 1v3 (the solo's jump is attempt 0), or anyone's otherwise.
fn shown_attempt(game: &State) -> (u32, u32) {
    if game.wind.is_some() {
        (game.attempt as u32, game.attempts as u32 - 1)
    } else {
        (game.attempt as u32 + 1, game.attempts as u32)
    }
}

fn banner_text(active: &ActiveRun<TripleJump>) -> (String, String) {
    let run = &active.run;
    let game = &run.game;
    if let Some(banner) = halted_banner(run) {
        return banner;
    }
    let solo = game.wind.map(|w| w.seat);
    let chase = || {
        let line = game.line().unwrap_or(0);
        tr!(
            "triple-jump-chase",
            player = seat_label(solo.unwrap_or(Seat(0))),
            metres = format_metres(line),
            count = game.qualified() as u32,
            needed = QUALIFIERS as u32
        )
    };
    let attempt = || {
        let (attempt, attempts) = shown_attempt(game);
        tr!(
            "triple-jump-attempt",
            attempt = attempt,
            attempts = attempts
        )
    };
    match game.phase {
        // The shared countdown (crate::intro) is drawn over this.
        Phase::Intro { .. } => (String::new(), String::new()),
        Phase::Ready { .. } => (
            attempt(),
            if solo.is_some() {
                chase()
            } else {
                String::new()
            },
        ),
        Phase::Jumping { t } => {
            let title = if t < 45 {
                tr!("triple-jump-go")
            } else {
                String::new()
            };
            let subtitle = match solo {
                Some(solo) if game.attempt == 0 => {
                    tr!("triple-jump-sets", player = seat_label(solo))
                }
                Some(_) => chase(),
                None => attempt(),
            };
            (title, subtitle)
        }
        Phase::Marks { .. } => {
            let title = match solo {
                Some(solo) if game.attempt == 0 => tr!(
                    "triple-jump-mark-set",
                    player = seat_label(solo),
                    metres = format_metres(game.line().unwrap_or(0))
                ),
                Some(_) => chase(),
                None => leader_text(game),
            };
            (title, String::new())
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

/// "Longest so far: P3, 13.21 m".
fn leader_text(game: &State) -> String {
    let best = longest(game);
    if best == 0 {
        return tr!("triple-jump-no-marks");
    }
    let leaders = game
        .athletes()
        .filter(|a| a.best().0 == Some(best))
        .flat_map(|a| a.members[..a.member_count as usize].to_vec());
    tr!(
        "triple-jump-leader",
        names = i18n::names(leaders),
        metres = format_metres(best)
    )
}

/// Each attempt's mark: "12.34   foul   -".
fn marks_line(game: &State, athlete: &Athlete) -> String {
    let attempts = if game.wind.is_some() {
        1..game.attempts as usize
    } else {
        0..game.attempts as usize
    };
    athlete.marks[attempts]
        .iter()
        .map(|mark| match mark {
            Mark::Landed(mm) => format_metres(*mm),
            Mark::Stumble(mm) => format_metres((*mm).max(0)),
            Mark::Foul => tr!("triple-jump-foul-short"),
            Mark::Pending => "-".into(),
        })
        .collect::<Vec<_>>()
        .join("   ")
}

fn update_hud(
    active: Res<ActiveRun<TripleJump>>,
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
                let Some(athlete) = game.athlete_of(seat) else {
                    continue;
                };
                let who = bot_tag(seats.is_bot(seat));
                let wind = game.wind.filter(|w| w.seat == seat);
                let role = if wind.is_some() {
                    tr!("solo-tag")
                } else {
                    String::new()
                };
                let best = match athlete.best().0 {
                    Some(mm) => tr!("triple-jump-best", metres = format_metres(mm)),
                    None => tr!("triple-jump-no-mark"),
                };
                let detail = if active.run.controller_lost(seat) {
                    tr!("controller-lost")
                } else if let (Some(outcome), true) = (
                    game.outcome,
                    matches!(game.phase, Phase::Decided { .. } | Phase::Finished),
                ) {
                    match outcome.placement[seat.index()] {
                        Some(place) => tr!("place", place = place as u32),
                        None => String::new(),
                    }
                } else if let Some(wind) = wind {
                    if game.attempt == 0 {
                        tr!("triple-jump-solo-first")
                    } else if wind.cooldown == 0 {
                        tr!("triple-jump-gust-ready")
                    } else {
                        tr!(
                            "triple-jump-gust-wait",
                            seconds = (wind.cooldown as u32).div_ceil(TICK_HZ)
                        )
                    }
                } else if athlete.member_count == 2 {
                    if athlete.runner(game.attempt) == seat {
                        tr!("triple-jump-runner")
                    } else {
                        tr!("triple-jump-coach")
                    }
                } else if game.qualifies(athlete) {
                    tr!("triple-jump-past")
                } else {
                    marks_line(game, athlete)
                };
                let middle = if wind.is_some() {
                    match game.line() {
                        Some(mm) => tr!("triple-jump-mark", metres = format_metres(mm)),
                        None => tr!("triple-jump-wind"),
                    }
                } else {
                    best
                };
                format!("{}{who}{role}\n{middle}\n{detail}", seat_label(seat))
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{Minigame, PlayerSetup};

    /// Fairness across lanes: with a parallel projection whose right is the lanes' direction,
    /// a metre along any lane is the same distance on screen.
    #[test]
    fn every_lane_is_seen_the_same_way() {
        assert!(matches!(camera_projection(), Projection::Orthographic(_)));
        let camera = camera_transform();
        assert!(camera.right().abs_diff_eq(Vec3::X, 1e-6));
    }

    #[test]
    fn the_lanes_fit_the_frame() {
        const { assert!(RUN_START - 4.0 > CAMERA_TARGET.x - FRAMED_WIDTH / 2.0) };
        const { assert!(FAN_X + 2.5 < CAMERA_TARGET.x + FRAMED_WIDTH / 2.0) };
        let game = TripleJump::new(1, PlayerSetup::first(4), Format::FreeForAll);
        // The back lane's highest jump and the front lane's speed bar, on screen, leave room
        // for the banner and the panels.
        let camera = camera_transform();
        let up = camera.up();
        let screen_y = |p: Vec3| (p - CAMERA_TARGET).dot(*up);
        let top = screen_y(Vec3::new(0.0, 5.5 + 2.4 * JUMP_LIFT, lane_z(&game, 0)));
        let bottom = screen_y(Vec3::new(0.0, 0.0, lane_z(&game, 3) + LANE_WIDTH));
        assert!(top < FRAMED_HEIGHT / 2.0 - 3.0, "{top}");
        assert!(bottom > -FRAMED_HEIGHT / 2.0 + 3.5, "{bottom}");
    }

    #[test]
    fn panels_follow_the_lanes() {
        let teams = Format::TwoVsTwo {
            team_of: [0, 1, 0, 1],
        };
        let game = TripleJump::new(1, PlayerSetup::first(4), teams);
        assert_eq!(shown_seats(&game), [Seat(0), Seat(2), Seat(1), Seat(3)]);
        let solo = Format::OneVsThree { solo: Seat(2) };
        let game = TripleJump::new(1, PlayerSetup::first(4), solo);
        assert_eq!(shown_seats(&game), [Seat(2), Seat(0), Seat(1), Seat(3)]);
    }

    #[test]
    fn marks_read_in_metres() {
        assert_eq!(format_metres(12_345), "12.34");
        assert_eq!(format_metres(9_050), "9.05");
        assert_eq!(RUN_START, -22.0);
    }
}
