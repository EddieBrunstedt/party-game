//! Presentation for Lantern Tower. Reads `ActiveRun<LanternTower>` only; never writes simulation
//! state. The towers stand in a row facing the camera with their builders at their feet, and
//! every floor glows in the colour of whoever placed it.

use bevy::camera::ScalingMode;
use bevy::prelude::*;
use minigame_lantern_tower::{
    Block, FALL_TICKS, GOAL_FLOORS, GUST_CHARGE, Landing, LanternTower, Phase, State, Tower,
};
use sim_core::{Format, MAX_PLAYERS, Seat, TICK_HZ};

use crate::audio::{Sfx, Sound};
use crate::characters::{Pose, Roster};
use crate::flow::AppState;
use crate::i18n::{self, bot_tag, halted_banner, names};
use crate::palette::{LANTERN_WARM, MOONLIGHT, NIGHT_SKY, seat_color, seat_label};
use crate::particles::{self, Effects, hdr};
use crate::runner::{ActiveRun, MinigameSet, Seats};
use crate::scenery::{
    emissive_material, night_camera, noise, spawn_fireflies, spawn_lantern_folk,
    spawn_paper_lantern,
};
use crate::tr;

pub struct LanternTowerViewPlugin;

impl Plugin for LanternTowerViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            (spawn_scene, spawn_hud)
                .after(MinigameSet::Start)
                .run_if(resource_exists::<ActiveRun<LanternTower>>),
        )
        .add_systems(
            Update,
            (
                animate_floors,
                animate_blocks,
                animate_pips,
                animate_goals,
                animate_builders,
                animate_debris,
                update_hud,
            )
                .run_if(resource_exists::<ActiveRun<LanternTower>>),
        )
        .add_systems(
            FixedUpdate,
            react
                .in_set(MinigameSet::React)
                .run_if(resource_exists::<ActiveRun<LanternTower>>),
        );
    }
}

const SCOPE: DespawnOnExit<AppState> = DespawnOnExit(AppState::Minigame);
/// World units per simulation milli-unit.
const SCALE: f32 = 1.2 / 1000.0;
const FLOOR_HEIGHT: f32 = 0.3;
/// Front to back, for every floor and block.
const DEPTH: f32 = 1.1;
/// The top of the base, where the first floor sits.
const BASE_TOP: f32 = 0.45;
/// A sliding block hovers this far above the top floor.
const HOVER: f32 = 0.55;
/// Pieces that fall off tumble for this long.
const DEBRIS_SECS: f32 = 1.6;
const GOAL_WHITE: LinearRgba = LinearRgba::rgb(0.85, 0.9, 1.0);
const MISS_RED: LinearRgba = LinearRgba::rgb(1.0, 0.08, 0.05);
const GUST_BLUE: LinearRgba = LinearRgba::rgb(0.55, 0.85, 1.0);
const FIREFLY_COUNT: usize = 30;
/// How long a perfect drop or a miss shows on its builder (hop or slump), in ticks.
const REACTION_TICKS: u32 = 40;
/// How long "Gust on P2!" stays up, in ticks.
const GUST_BANNER_TICKS: u32 = 60;
/// Team colours for the panel borders in team formats.
const TEAM_COLORS: [LinearRgba; 2] = [
    LinearRgba::rgb(1.0, 0.78, 0.3),
    LinearRgba::rgb(0.55, 0.45, 1.0),
];

/// One floor of one tower (1 is the lowest above the base).
#[derive(Component)]
struct FloorView {
    tower: usize,
    floor: u8,
    material: Handle<StandardMaterial>,
}

/// A tower's sliding or falling block.
#[derive(Component)]
struct BlockView {
    tower: usize,
    material: Handle<StandardMaterial>,
}

/// Wind streaks trailing a gusted block.
#[derive(Component)]
struct Streak {
    tower: usize,
    index: usize,
}

/// One of the lights on a tower's base that count perfect drops towards a gust.
#[derive(Component)]
struct ChargePip {
    tower: usize,
    index: u8,
    material: Handle<StandardMaterial>,
}

/// The line at the goal height over each tower.
#[derive(Component)]
struct GoalLine {
    tower: usize,
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct BuilderView {
    seat: Seat,
    head_material: Handle<StandardMaterial>,
    head_light: Entity,
}

/// A cut-off piece, a missed block or a knocked-off floor tumbling away. Presentation only.
#[derive(Component)]
struct Debris {
    velocity: Vec3,
    spin: Vec3,
    age: f32,
}

#[derive(Resource)]
struct UnitCube(Handle<Mesh>);

#[derive(Component)]
enum HudText {
    Banner,
    SubBanner,
    SeatPanel(Seat),
}

/// Seats that build, in seat order: everyone but those outside a duel.
fn builders(game: &State) -> Vec<Seat> {
    game.players
        .seats()
        .filter(|s| game.tower_of(*s).is_some())
        .collect()
}

fn tower_x(game: &State, tower: usize) -> f32 {
    let built: Vec<usize> = game.built().collect();
    let gap = match built.len() {
        4 => 4.2,
        3 => 4.8,
        _ => 6.0,
    };
    let index = built.iter().position(|t| *t == tower).unwrap_or(0) as f32;
    (index - (built.len() as f32 - 1.0) / 2.0) * gap
}

/// Where a tower's members stand, from its centre: the first on the left, then the right, then
/// further left.
fn stand_x(member: usize) -> f32 {
    [-1.15, 1.15, -2.4, 2.4][member]
}

/// The top of floor `k` (0 is the base).
fn floor_top(k: f32) -> f32 {
    BASE_TOP + k * FLOOR_HEIGHT
}

fn team_index(format: &Format, seat: Seat) -> Option<u8> {
    match format {
        Format::FreeForAll | Format::Duel { .. } => None,
        _ => format.team(seat),
    }
}

/// The member who dropped the last block that landed.
fn last_builder(tower: &Tower) -> Seat {
    tower.members[((tower.turn + tower.member_count - 1) % tower.member_count) as usize]
}

fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    active: Res<ActiveRun<LanternTower>>,
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
        night_camera(camera_transform(), (16.0, 40.0)),
        camera_projection(),
    ));
    commands.spawn((
        SCOPE,
        DirectionalLight {
            color: MOONLIGHT,
            illuminance: 350.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-5.0, 10.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let unit = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    commands.insert_resource(UnitCube(unit.clone()));
    spawn_ground(&mut commands, &mut meshes, &mut materials);
    for (pos, color) in [
        (Vec3::new(-9.6, 0.0, -3.0), LinearRgba::rgb(1.0, 0.3, 0.55)),
        (Vec3::new(9.6, 0.0, -3.0), LinearRgba::rgb(0.25, 0.75, 1.0)),
        (Vec3::new(-10.2, 0.0, 2.0), LANTERN_WARM),
        (Vec3::new(10.2, 0.0, 2.0), LANTERN_WARM),
    ] {
        spawn_paper_lantern(
            &mut commands,
            &mut meshes,
            &mut materials,
            AppState::Minigame,
            pos,
            color,
        );
    }
    spawn_fireflies(
        &mut commands,
        &mut meshes,
        &mut materials,
        AppState::Minigame,
        FIREFLY_COUNT,
        Vec3::new(-10.0, 0.5, -6.0),
        Vec3::new(10.0, 8.0, 2.0),
    );

    let game = &active.run.game;
    for tower in game.built() {
        spawn_tower(&mut commands, &mut materials, &unit, game, tower);
        for (member, seat) in game.towers[tower].members().iter().enumerate() {
            let x = tower_x(game, tower) + stand_x(member);
            // Turned a little towards their tower.
            let turn = if stand_x(member) < 0.0 { 0.35 } else { -0.35 };
            let folk = spawn_lantern_folk(
                &mut commands,
                &mut meshes,
                &mut materials,
                AppState::Minigame,
                *seat,
                roster.of(*seat),
                Transform::from_xyz(x, 0.0, 0.7).with_rotation(Quat::from_rotation_y(turn)),
            );
            commands.entity(folk.root).insert(BuilderView {
                seat: *seat,
                head_material: folk.head_material,
                head_light: folk.head_light,
            });
        }
    }
}

fn spawn_ground(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let tile = meshes.add(Cuboid::new(0.96, 0.4, 0.96));
    let shades: Vec<_> = [
        (0.04, 0.045, 0.06),
        (0.035, 0.04, 0.052),
        (0.05, 0.05, 0.065),
    ]
    .into_iter()
    .map(|(r, g, b)| {
        materials.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            perceptual_roughness: 0.92,
            ..default()
        })
    })
    .collect();
    for i in -12..=12 {
        for j in -8..=4 {
            let shade = &shades[(noise(i, j, 7) * 2.999) as usize];
            let y = -0.2 + (noise(i, j, 8) - 0.5) * 0.06;
            commands.spawn((
                SCOPE,
                Mesh3d(tile.clone()),
                MeshMaterial3d(shade.clone()),
                Transform::from_xyz(i as f32, y, j as f32),
            ));
        }
    }
}

fn spawn_tower(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    unit: &Handle<Mesh>,
    game: &State,
    tower: usize,
) {
    let x = tower_x(game, tower);
    let base = game.towers[tower].floors[0];
    let base_width = base.width() as f32 * SCALE;

    // A stone plinth, and the base in the first member's colour.
    let stone = materials.add(StandardMaterial {
        base_color: Color::srgb(0.07, 0.065, 0.075),
        perceptual_roughness: 0.9,
        ..default()
    });
    commands.spawn((
        SCOPE,
        Mesh3d(unit.clone()),
        MeshMaterial3d(stone),
        Transform::from_xyz(x, BASE_TOP / 2.0 - 0.1, 0.0).with_scale(Vec3::new(
            base_width + 0.5,
            BASE_TOP + 0.2,
            DEPTH + 0.4,
        )),
    ));
    commands.spawn((
        SCOPE,
        Mesh3d(unit.clone()),
        MeshMaterial3d(materials.add(emissive_material(seat_color(base.seat), 0.6))),
        Transform::from_xyz(x, BASE_TOP - 0.06, 0.0).with_scale(Vec3::new(base_width, 0.12, DEPTH)),
    ));

    // The gust charge: one light per perfect drop, on the plinth's front.
    for index in 0..GUST_CHARGE {
        let material = materials.add(emissive_material(GUST_BLUE, 0.0));
        let offset = (index as f32 - (GUST_CHARGE as f32 - 1.0) / 2.0) * 0.32;
        commands.spawn((
            SCOPE,
            ChargePip {
                tower,
                index,
                material: material.clone(),
            },
            Mesh3d(unit.clone()),
            MeshMaterial3d(material),
            Transform::from_xyz(x + offset, BASE_TOP / 2.0 - 0.05, DEPTH / 2.0 + 0.22)
                .with_scale(Vec3::splat(0.18)),
        ));
    }

    for floor in 1..=GOAL_FLOORS {
        let material = materials.add(emissive_material(LinearRgba::BLACK, 0.0));
        commands.spawn((
            SCOPE,
            FloorView {
                tower,
                floor,
                material: material.clone(),
            },
            Mesh3d(unit.clone()),
            MeshMaterial3d(material),
            Transform::from_xyz(x, 0.0, 0.0),
            Visibility::Hidden,
        ));
    }

    let material = materials.add(emissive_material(LinearRgba::BLACK, 0.0));
    commands.spawn((
        SCOPE,
        BlockView {
            tower,
            material: material.clone(),
        },
        Mesh3d(unit.clone()),
        MeshMaterial3d(material),
        Transform::from_xyz(x, 0.0, 0.0),
        Visibility::Hidden,
    ));
    let streak = materials.add(emissive_material(GUST_BLUE, 3.0));
    for index in 0..3 {
        commands.spawn((
            SCOPE,
            Streak { tower, index },
            Mesh3d(unit.clone()),
            MeshMaterial3d(streak.clone()),
            Transform::default(),
            Visibility::Hidden,
        ));
    }

    // The goal: a thin bright frame where the last floor's top will be.
    let material = materials.add(emissive_material(GOAL_WHITE, 0.8));
    let top = floor_top(GOAL_FLOORS as f32);
    for (dx, width) in [(-0.75, 0.3), (0.75, 0.3)] {
        commands.spawn((
            SCOPE,
            GoalLine {
                tower,
                material: material.clone(),
            },
            Mesh3d(unit.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(x + dx * base_width, top, DEPTH / 2.0 + 0.05)
                .with_scale(Vec3::new(width * base_width, 0.05, 0.05)),
        ));
    }
}

fn blend(fixed: &Time<Fixed>) -> f32 {
    fixed.overstep_fraction().clamp(0.0, 1.0)
}

/// Where a tower's block is drawn: its centre, its width, and whether it slides right and fast.
struct BlockPose {
    centre: Vec3,
    width: f32,
    right: bool,
    gusted: bool,
}

fn block_pose(now: &State, before: &State, tower: usize, alpha: f32) -> Option<BlockPose> {
    let t = &now.towers[tower];
    let width = t.top().width() as f32 * SCALE;
    let x = tower_x(now, tower);
    let rest = floor_top(t.height as f32) + FLOOR_HEIGHT / 2.0;
    let left_edge = |lo: i32| x + lo as f32 * SCALE;
    let (lo, y, right) = match (before.towers[tower].block, t.block) {
        (Block::Sliding { lo: was, .. }, Block::Sliding { lo, right }) => {
            // At a bounce the two ends are on the same side; blend anyway, it's one tick.
            let lo = left_edge(was) + (left_edge(lo) - left_edge(was)) * alpha;
            (lo, rest + HOVER, right)
        }
        (_, Block::Sliding { lo, right }) => (left_edge(lo), rest + HOVER, right),
        (_, Block::Falling { lo, t: fallen }) => {
            // Falls the way things fall: slowly, then faster.
            let progress = ((fallen as f32 + alpha) / FALL_TICKS as f32).min(1.0);
            (
                left_edge(lo),
                rest + HOVER * (1.0 - progress * progress),
                true,
            )
        }
        (_, Block::Resting { .. }) => return None,
    };
    Some(BlockPose {
        centre: Vec3::new(lo + width / 2.0, y, 0.0),
        width,
        right,
        gusted: t.gusted > 0 && matches!(t.block, Block::Sliding { .. }),
    })
}

fn animate_floors(
    active: Res<ActiveRun<LanternTower>>,
    mut floors: Query<(&FloorView, &mut Transform, &mut Visibility)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let game = &active.run.game;
    for (view, mut transform, mut visibility) in &mut floors {
        let tower = &game.towers[view.tower];
        if view.floor > tower.height {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = Visibility::Inherited;
        let floor = tower.floors[view.floor as usize];
        let width = floor.width() as f32 * SCALE;
        transform.translation = Vec3::new(
            tower_x(game, view.tower) + floor.lo as f32 * SCALE + width / 2.0,
            floor_top(view.floor as f32) - FLOOR_HEIGHT / 2.0,
            0.0,
        );
        transform.scale = Vec3::new(width, FLOOR_HEIGHT * 0.94, DEPTH);
        // The top floor glows brightest, so the edge to drop on is easy to see; the rest
        // alternate, so floors can be counted.
        let glow = if view.floor == tower.height {
            2.2
        } else if view.floor.is_multiple_of(2) {
            1.0
        } else {
            0.6
        };
        if let Some(mut material) = materials.get_mut(&view.material) {
            material.emissive = seat_color(floor.seat) * glow;
        }
    }
}

fn animate_blocks(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternTower>>,
    mut blocks: Query<(&BlockView, &mut Transform, &mut Visibility), Without<Streak>>,
    mut streaks: Query<(&Streak, &mut Transform, &mut Visibility), Without<BlockView>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let alpha = blend(&fixed);
    let now = time.elapsed_secs();
    let game = &active.run.game;
    let mut poses: [Option<BlockPose>; MAX_PLAYERS] = Default::default();
    for (view, mut transform, mut visibility) in &mut blocks {
        let Some(pose) = block_pose(game, &active.previous, view.tower, alpha) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *visibility = Visibility::Inherited;
        transform.translation = pose.centre;
        transform.scale = Vec3::new(pose.width, FLOOR_HEIGHT * 0.94, DEPTH);
        let builder = game.towers[view.tower].builder();
        let glow = if pose.gusted {
            3.0 + (now * 24.0).sin()
        } else {
            3.0 + 0.4 * (now * 6.0).sin()
        };
        if let Some(mut material) = materials.get_mut(&view.material) {
            material.emissive = seat_color(builder) * glow;
        }
        poses[view.tower] = Some(pose);
    }
    for (streak, mut transform, mut visibility) in &mut streaks {
        let Some(pose) = poses[streak.tower].as_ref().filter(|p| p.gusted) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *visibility = Visibility::Inherited;
        // Thin lines behind the block, flickering back and forth.
        let behind = if pose.right { -1.0 } else { 1.0 };
        let length = 0.5 + 0.2 * (now * 30.0 + streak.index as f32 * 2.0).sin();
        let y = (streak.index as f32 - 1.0) * FLOOR_HEIGHT * 0.3;
        transform.translation = pose.centre
            + Vec3::new(
                behind * (pose.width / 2.0 + 0.15 + length / 2.0),
                y,
                DEPTH / 2.0,
            );
        transform.scale = Vec3::new(length, 0.03, 0.03);
    }
}

fn animate_pips(
    time: Res<Time>,
    active: Res<ActiveRun<LanternTower>>,
    pips: Query<&ChargePip>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let game = &active.run.game;
    let now = time.elapsed_secs();
    for pip in &pips {
        let charge = game.towers[pip.tower].charge;
        let glow = if charge >= GUST_CHARGE {
            // Ready to blow: all three pulse.
            3.0 + 2.0 * (now * 8.0).sin()
        } else if pip.index < charge {
            2.5
        } else {
            0.06
        };
        if let Some(mut material) = materials.get_mut(&pip.material) {
            material.emissive = GUST_BLUE * glow;
        }
    }
}

fn animate_goals(
    time: Res<Time>,
    active: Res<ActiveRun<LanternTower>>,
    lines: Query<&GoalLine>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let game = &active.run.game;
    let now = time.elapsed_secs();
    for line in &lines {
        let tower = &game.towers[line.tower];
        let (color, glow) = if tower.height >= GOAL_FLOORS {
            (GOAL_WHITE, 8.0 + 3.0 * (now * 10.0).sin())
        } else if tower.height + 3 >= GOAL_FLOORS {
            // Nearly there: the goal beckons.
            (GOAL_WHITE, 1.6 + 0.8 * (now * 5.0).sin())
        } else {
            (GOAL_WHITE, 0.8)
        };
        if let Some(mut material) = materials.get_mut(&line.material) {
            material.emissive = color * glow;
        }
    }
}

fn animate_builders(
    time: Res<Time>,
    active: Res<ActiveRun<LanternTower>>,
    mut builders: Query<(&BuilderView, &mut Pose)>,
    mut lights: Query<&mut PointLight>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let game = &active.run.game;
    let now = time.elapsed_secs();
    let winners: Vec<Seat> = game
        .outcome
        .map(|o| o.winners().collect())
        .unwrap_or_default();
    for (view, mut pose) in &mut builders {
        let seat = view.seat;
        let Some(tower) = game.tower_of(seat) else {
            continue;
        };
        let reaction = tower
            .landing
            .filter(|(tick, _)| game.tick - tick < REACTION_TICKS && last_builder(tower) == seat)
            .map(|(_, landing)| landing);
        *pose = if game.outcome.is_some() {
            if winners.contains(&seat) {
                Pose::Celebrate
            } else {
                Pose::Slump
            }
        } else {
            match reaction {
                Some(Landing::Perfect) => Pose::Hop,
                Some(Landing::Miss { .. }) => Pose::Slump,
                _ => Pose::Idle,
            }
        };
        // Whoever drops next glows brightest.
        let glow = if game.phase == Phase::Racing && tower.builder() == seat {
            3.2 + 0.4 * (now * 6.0).sin()
        } else {
            1.0
        };
        if let Some(mut material) = materials.get_mut(&view.head_material) {
            material.emissive = seat_color(seat) * glow;
        }
        if let Ok(mut light) = lights.get_mut(view.head_light) {
            light.intensity = 22_000.0 * glow;
        }
    }
}

fn animate_debris(
    mut commands: Commands,
    time: Res<Time>,
    mut debris: Query<(Entity, &mut Debris, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut piece, mut transform) in &mut debris {
        piece.age += dt;
        piece.velocity.y -= 14.0 * dt;
        transform.translation += piece.velocity * dt;
        transform.rotate(Quat::from_scaled_axis(piece.spin * dt));
        if piece.age > DEBRIS_SECS {
            commands.entity(entity).despawn();
        }
    }
}

/// Sounds, sparks and falling pieces, once per event: landings, gusts, the last seconds and the
/// win. Bots' landings are quieter, so four towers don't drown out a person's own.
#[allow(clippy::too_many_arguments)]
fn react(
    mut commands: Commands,
    active: Res<ActiveRun<LanternTower>>,
    seats: Res<Seats>,
    unit: Res<UnitCube>,
    builders: Query<(&BuilderView, &Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    effects: Res<Effects>,
    sfx: Sfx,
) {
    let Some((before, now)) = active.ticked() else {
        return;
    };
    let head = |seat: Seat| {
        builders
            .iter()
            .find(|(b, _)| b.seat == seat)
            .map(|(_, t)| t.translation + Vec3::Y * 1.5)
    };
    let mut debris = |commands: &mut Commands, tower: usize, lo: i32, hi: i32, k: u8, color| {
        let x = tower_x(now, tower);
        let width = (hi - lo) as f32 * SCALE;
        let centre = x + lo as f32 * SCALE + width / 2.0;
        let outwards = if centre >= x { 1.0 } else { -1.0 };
        commands.spawn((
            SCOPE,
            Debris {
                velocity: Vec3::new(outwards * 1.6, 1.5, 0.6),
                spin: Vec3::new(0.0, 0.0, -outwards * 5.0),
                age: 0.0,
            },
            Mesh3d(unit.0.clone()),
            MeshMaterial3d(materials.add(emissive_material(color, 1.6))),
            Transform::from_xyz(centre, floor_top(k as f32) - FLOOR_HEIGHT / 2.0, 0.0)
                .with_scale(Vec3::new(width, FLOOR_HEIGHT * 0.94, DEPTH)),
        ));
    };
    for tower in now.built() {
        let (was, is) = (&before.towers[tower], &now.towers[tower]);
        let x = tower_x(now, tower);
        let top = Vec3::new(x, floor_top(is.height as f32) + 0.2, DEPTH / 2.0);
        if is.landing != was.landing
            && let Some((_, landing)) = is.landing
        {
            let seat = was.builder();
            let volume = if seats.is_bot(seat) { -8.0 } else { 0.0 };
            let color = seat_color(seat);
            match landing {
                Landing::Perfect => {
                    // A rising chime as the gust charges.
                    let rate = [1.0, 1.26, 1.5][(is.charge.max(1) - 1) as usize];
                    sfx.play_with(Sound::Count, rate, volume);
                    particles::burst(&mut commands, &effects, SCOPE, top, hdr(GOAL_WHITE, 4.0));
                    if is.charge >= GUST_CHARGE && was.charge < GUST_CHARGE {
                        sfx.play_with(Sound::Ready, 1.0, volume);
                    }
                }
                Landing::Cut { lo, hi } => {
                    sfx.play_with(Sound::Step, 0.8, volume);
                    debris(&mut commands, tower, lo, hi, is.height, color);
                }
                Landing::Miss { lo, hi, knocked } => {
                    sfx.play_with(Sound::Fall, 1.0, volume.min(-2.0));
                    debris(&mut commands, tower, lo, hi, is.height + 1, color);
                    if let Some(floor) = knocked {
                        debris(
                            &mut commands,
                            tower,
                            floor.lo,
                            floor.hi,
                            is.height + 1,
                            seat_color(floor.seat),
                        );
                    }
                    particles::burst(&mut commands, &effects, SCOPE, top, hdr(MISS_RED, 1.5));
                }
            }
        }
        if is.gust_hit != was.gust_hit
            && let Some(pose) = block_pose(now, before, tower, 1.0)
        {
            particles::burst(
                &mut commands,
                &effects,
                SCOPE,
                pose.centre,
                hdr(GUST_BLUE, 3.0),
            );
        }
        if was.charge >= GUST_CHARGE && is.charge < GUST_CHARGE {
            sfx.play(Sound::Dash);
            particles::burst(
                &mut commands,
                &effects,
                SCOPE,
                Vec3::new(x, BASE_TOP, DEPTH / 2.0 + 0.3),
                hdr(GUST_BLUE, 4.0),
            );
        }
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
            if let Some(at) = head(seat) {
                particles::burst(
                    &mut commands,
                    &effects,
                    SCOPE,
                    at,
                    hdr(seat_color(seat), 6.0),
                );
            }
        }
        for tower in now.built() {
            if now.towers[tower].height >= GOAL_FLOORS {
                let at = Vec3::new(
                    tower_x(now, tower),
                    floor_top(GOAL_FLOORS as f32) + 0.4,
                    0.5,
                );
                particles::burst(&mut commands, &effects, SCOPE, at, hdr(LANTERN_WARM, 6.0));
                particles::burst(&mut commands, &effects, SCOPE, at, hdr(GOAL_WHITE, 6.0));
            }
        }
    }
}

const CAMERA_TARGET: Vec3 = Vec3::new(0.0, 3.6, 0.0);
const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 2.2, 12.8);
/// Half-width of four towers' swings and their builders (plus margin) that must stay on screen.
const FRAMED_HALF_WIDTH: f32 = 8.8;
/// Height of the goal floors, the hovering block and the builders (plus margin).
const FRAMED_HEIGHT: f32 = 10.8;

fn camera_transform() -> Transform {
    Transform::from_translation(CAMERA_TARGET + CAMERA_OFFSET).looking_at(CAMERA_TARGET, Vec3::Y)
}

/// Orthographic, so every tower is seen the same way: on screen a block lines up with the floor
/// below exactly when it does in the simulation, wherever its tower stands. (Under perspective
/// the outer towers were seen from the side and harder to judge.) Narrow windows show more
/// height rather than cutting off the outer towers.
fn camera_projection() -> Projection {
    Projection::Orthographic(OrthographicProjection {
        scaling_mode: ScalingMode::AutoMin {
            min_width: FRAMED_HALF_WIDTH * 2.0,
            min_height: FRAMED_HEIGHT,
        },
        ..OrthographicProjection::default_3d()
    })
}

fn spawn_hud(mut commands: Commands, active: Res<ActiveRun<LanternTower>>) {
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
            for seat in builders(game) {
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

/// "Gust on P2 & P3!" while a gust that just hit is fresh.
fn gust_banner(game: &State) -> Option<String> {
    let latest = game.built().filter_map(|t| game.towers[t].gust_hit).max()?;
    if game.tick - latest >= GUST_BANNER_TICKS {
        return None;
    }
    let hit = game
        .built()
        .filter(|t| game.towers[*t].gust_hit == Some(latest))
        .flat_map(|t| game.towers[t].members().to_vec());
    Some(tr!("lantern-tower-gust", players = names(hit)))
}

fn banner_text(active: &ActiveRun<LanternTower>) -> (String, String) {
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
                tr!("lantern-tower-go")
            } else {
                gust_banner(game).unwrap_or_default()
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
    active: Res<ActiveRun<LanternTower>>,
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
                let Some(tower) = game.tower_of(seat) else {
                    continue;
                };
                let floors = tr!(
                    "lantern-tower-floors",
                    floors = tower.height as u32,
                    goal = GOAL_FLOORS as u32
                );
                let status = if active.run.controller_lost(seat) {
                    tr!("controller-lost")
                } else if let (Some(outcome), true) = (game.outcome, game.phase != Phase::Racing) {
                    match outcome.placement[seat.index()] {
                        Some(place) => tr!("place", place = place as u32),
                        None => String::new(),
                    }
                } else if tower.charge >= GUST_CHARGE {
                    tr!("lantern-tower-gust-ready")
                } else if tower.gusted > 0 {
                    tr!("lantern-tower-gusted")
                } else if tower.member_count > 1 && tower.builder() == seat {
                    tr!("lantern-tower-your-turn")
                } else {
                    tr!(
                        "lantern-tower-charge",
                        charge = tower.charge as u32,
                        needed = GUST_CHARGE as u32
                    )
                };
                format!("{}{who}{role}\n{floors}\n{status}", seat_label(seat))
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{Minigame, PlayerSetup};

    /// Fairness across seats: with a parallel projection and the camera's right axis along the
    /// row of towers, screen x depends on world x alone, so a block's edge and the edge below it
    /// line up on screen exactly when they do in the simulation, for every tower.
    #[test]
    fn every_tower_is_seen_the_same_way() {
        assert!(matches!(camera_projection(), Projection::Orthographic(_)));
        let right = camera_transform().right();
        assert!(right.abs_diff_eq(Vec3::X, 1e-6), "{right}");
    }

    #[test]
    fn towers_and_their_builders_stay_apart() {
        for (players, format) in [
            (4, Format::FreeForAll),
            (4, Format::OneVsThree { solo: Seat(0) }),
            (
                4,
                Format::TwoVsTwo {
                    team_of: [0, 1, 0, 1],
                },
            ),
            (
                2,
                Format::Duel {
                    a: Seat(0),
                    b: Seat(1),
                },
            ),
        ] {
            let game = LanternTower::new(1, PlayerSetup::first(players), format);
            let mut spots: Vec<f32> = Vec::new();
            for tower in game.built() {
                for member in 0..game.towers[tower].member_count as usize {
                    spots.push(tower_x(&game, tower) + stand_x(member));
                }
            }
            spots.sort_by(f32::total_cmp);
            assert!(
                spots.windows(2).all(|w| w[1] - w[0] > 1.1),
                "{format:?}: {spots:?}"
            );
            // Nobody stands inside a neighbouring tower's base.
            for tower in game.built() {
                let x = tower_x(&game, tower);
                assert!(spots.iter().all(|s| (s - x).abs() > 0.9), "{format:?}");
            }
        }
    }

    #[test]
    fn the_last_builder_is_the_one_before_the_turn() {
        let mut game = LanternTower::new(
            1,
            PlayerSetup::first(4),
            Format::OneVsThree { solo: Seat(3) },
        );
        let tower = &mut game.towers[1];
        tower.turn = 0;
        assert_eq!(last_builder(tower), Seat(2));
        tower.turn = 1;
        assert_eq!(last_builder(tower), Seat(0));
    }
}
