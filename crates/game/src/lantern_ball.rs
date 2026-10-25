//! Presentation for Lantern Ball. Reads `ActiveRun<LanternBall>` only; never writes simulation
//! state. The pitch is built from the simulation's sides (each a straight wall, some with a goal
//! in it), and positions are blended between the last two ticks so motion stays smooth.

use std::collections::VecDeque;
use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use minigame_lantern_ball::{
    BALL_RADIUS, BUMPER_RADIUS, Ball, BallStatus, CHARGE_TICKS, DROP_TICKS, FOLK_RADIUS, LIVES,
    LanternBall, MAX_BALL_SPEED, POST_RADIUS, Phase, RESTLESS_TICKS, SCORED_TICKS,
    SECOND_BALL_TICK, Side, State, Status, TRIO_GOAL,
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
use crate::scenery::{emissive_material, night_camera, spawn_fireflies, spawn_lantern_folk};
use crate::tr;

pub struct LanternBallViewPlugin;

impl Plugin for LanternBallViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            (spawn_scene, spawn_hud)
                .after(MinigameSet::Start)
                .run_if(resource_exists::<ActiveRun<LanternBall>>),
        )
        .add_systems(
            Update,
            (
                animate_folk,
                animate_balls,
                animate_bumpers,
                animate_goals,
                (fit_camera, shake_camera).chain(),
                update_hud,
            )
                .run_if(resource_exists::<ActiveRun<LanternBall>>),
        )
        .add_systems(
            FixedUpdate,
            react
                .in_set(MinigameSet::React)
                .run_if(resource_exists::<ActiveRun<LanternBall>>),
        );
    }
}

const SCOPE: DespawnOnExit<AppState> = DespawnOnExit(AppState::Minigame);
/// Team colours for goals and floor discs in team formats (the 1v3 keeper is team 0).
const TEAM_COLORS: [LinearRgba; 2] = [
    LinearRgba::rgb(1.0, 0.78, 0.3),
    LinearRgba::rgb(0.55, 0.45, 1.0),
];
/// The ball glows pale, unlike any seat's colour.
const BALL_GLOW: LinearRgba = LinearRgba::rgb(1.0, 0.95, 0.82);
const WALL_HEIGHT: f32 = 0.45;
const WALL_THICKNESS: f32 = 0.3;
/// How far back a goal's net is from its mouth, and how tall it is.
const GOAL_DEPTH: f32 = 1.4;
const GOAL_HEIGHT: f32 = 1.6;
const DROP_HEIGHT: f32 = 6.0;
const GHOSTS: usize = 5;
const FIREFLY_COUNT: usize = 30;

/// Sounds and sparks, once per event: kicks, dashes, knocks, bounces, goals, outs, the second
/// ball, sudden death, the last seconds and the win.
fn react(
    mut commands: Commands,
    active: Res<ActiveRun<LanternBall>>,
    effects: Res<Effects>,
    sfx: Sfx,
) {
    let Some((before, now)) = active.ticked() else {
        return;
    };
    let mut spark = |pos: Vec3, color: Vec4| {
        particles::burst(&mut commands, &effects, SCOPE, pos, color);
    };
    for seat in now.players.seats() {
        let (was, is) = (before.folk(seat), now.folk(seat));
        let variant = now.tick + seat.0 as u32;
        if was.dash == 0 && is.dash > 0 {
            sfx.play_varied(Sound::Dash, variant);
        }
        if is.kicked_at != was.kicked_at {
            // A harder kick sounds lower and louder.
            let share = was.charge_share();
            sfx.play_with(Sound::Hit, (1.2 - share * 0.4) as f64, -6.0 + share * 8.0);
            spark(
                world(is.body.pos) + Vec3::Y * 0.6,
                hdr(seat_color(seat), 2.0 + share * 4.0),
            );
        }
        if is.knocked != was.knocked
            && let Some((_, by)) = is.knocked
        {
            sfx.play_with(Sound::Hit, 0.75, 0.0);
            let color = by.map(seat_color).unwrap_or(BALL_GLOW);
            spark(world(is.body.pos) + Vec3::Y * 0.9, hdr(color, 5.0));
        }
        if was.playing() && matches!(is.status, Status::Out { .. }) {
            sfx.play(Sound::Rumble);
            spark(
                world(is.body.pos) + Vec3::Y * 0.5,
                hdr(seat_color(seat), 6.0),
            );
        }
    }
    for (b, ball) in now.balls.iter().enumerate() {
        let was = &before.balls[b];
        if ball.bounced_at != was.bounced_at && ball.rolling() {
            sfx.play_varied(Sound::Rebound, now.tick + b as u32);
        }
        if matches!(was.status, BallStatus::Waiting | BallStatus::Scored { .. })
            && matches!(ball.status, BallStatus::Dropping { .. })
        {
            sfx.play_varied(Sound::Brace, now.tick + b as u32);
        }
        if matches!(was.status, BallStatus::Dropping { .. }) && ball.rolling() {
            sfx.play_varied(Sound::Fall, now.tick + b as u32);
            spark(Vec3::Y * 0.2, hdr(BALL_GLOW, 4.0));
        }
        if was.rolling()
            && let BallStatus::Scored { side, .. } = ball.status
        {
            let side = &now.sides[side as usize];
            let owner = side.goal.expect("a goal side").owner;
            sfx.play(Sound::CoinsDown);
            spark(
                world(side.center() + side.normal * 0.5) + Vec3::Y * 0.8,
                hdr(goal_color(now, owner), 8.0),
            );
        }
    }
    if now.phase == Phase::Playing {
        if before.tick < SECOND_BALL_TICK && now.tick >= SECOND_BALL_TICK {
            sfx.play(Sound::Go);
        }
        if !before.sudden_death() && now.sudden_death() {
            sfx.play(Sound::Rumble);
        }
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
            let folk = now.folk(seat);
            if folk.playing() {
                spark(
                    world(folk.body.pos) + Vec3::Y * 1.8,
                    hdr(seat_color(seat), 6.0),
                );
            }
        }
    }
}

#[derive(Component)]
struct FolkView {
    seat: Seat,
    head_material: Handle<StandardMaterial>,
    head_light: Entity,
    /// The footprint disc, glowing while a dash is ready.
    disc: Entity,
    disc_material: Handle<StandardMaterial>,
    disc_color: LinearRgba,
    /// Grows and brightens as a kick charges, with an arrow the way it will go.
    charge: Entity,
    charge_material: Handle<StandardMaterial>,
    arrow: Entity,
}

#[derive(Component)]
struct BallView {
    index: usize,
    material: Handle<StandardMaterial>,
    light: Entity,
    /// The ring on the ground where a dropping ball will land.
    shadow: Entity,
    shadow_material: Handle<StandardMaterial>,
    ghosts: [(Entity, Handle<StandardMaterial>); GHOSTS],
    history: VecDeque<Vec3>,
    /// The ball's spin, built up from how far it has rolled.
    spin: Quat,
}

#[derive(Component)]
struct BumperView {
    index: usize,
    material: Handle<StandardMaterial>,
}

/// A goal's mouth and net, glowing in its owner's colour, and the barrier that walls it up.
#[derive(Component)]
struct GoalView {
    side: usize,
    /// Frame, floor, net and crease line, brightened together.
    glow: Vec<Handle<StandardMaterial>>,
    color: LinearRgba,
    parts: GoalParts,
}

/// The entities of a goal that follow its width.
struct GoalParts {
    /// The side's walls either side of the mouth, and the glowing strips along their tops.
    rails: [Entity; 2],
    strips: [Entity; 2],
    posts: [Entity; 2],
    /// The net's sides, back and floor, and the crossbar.
    nets: [Entity; 2],
    back: Entity,
    floor: Entity,
    bar: Entity,
    barrier: Entity,
    crease: Entity,
    /// Where the side's ends are along it, from the mouth's middle.
    ends: (f32, f32),
    /// The crease's radius the arc mesh was built at.
    base_crease: f32,
}

#[derive(Component)]
struct CameraRig {
    base: Transform,
    /// Half the pitch's width and depth, with room round them.
    framed: Vec2,
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
    active: Res<ActiveRun<LanternBall>>,
    roster: Res<Roster>,
) {
    let game = &active.run.game;
    commands.insert_resource(ClearColor(NIGHT_SKY));
    commands.insert_resource(GlobalAmbientLight {
        color: MOONLIGHT,
        brightness: 40.0,
        ..default()
    });
    let corners = corners(&game.sides);
    let framed =
        corners.iter().fold(Vec2::ZERO, |m, c| m.max(c.abs())) + Vec2::splat(GOAL_DEPTH + 0.6);
    let camera = camera_transform(1.0);
    commands.spawn((
        SCOPE,
        night_camera(camera, (24.0, 60.0)),
        CameraRig {
            base: camera,
            framed,
        },
    ));
    commands.spawn((
        SCOPE,
        DirectionalLight {
            color: MOONLIGHT,
            illuminance: 450.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-5.0, 14.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        SCOPE,
        PointLight {
            color: Color::from(LANTERN_WARM),
            intensity: 2_500_000.0,
            range: 34.0,
            radius: 0.5,
            ..default()
        },
        Transform::from_xyz(0.0, 11.0, 0.0),
    ));

    spawn_pitch(&mut commands, &mut meshes, &mut materials, game, &corners);
    spawn_fireflies(
        &mut commands,
        &mut meshes,
        &mut materials,
        AppState::Minigame,
        FIREFLY_COUNT,
        Vec3::new(-14.0, 0.5, -11.0),
        Vec3::new(14.0, 4.0, 9.0),
    );
    for seat in game.taking_part() {
        spawn_folk(
            &mut commands,
            &mut meshes,
            &mut materials,
            game,
            seat,
            roster.of(seat),
        );
    }
    for index in 0..game.balls.len() {
        spawn_ball(&mut commands, &mut meshes, &mut materials, game, index);
    }
}

/// The pitch's corners, round it: where each side meets the next.
fn corners(sides: &[Side]) -> Vec<Vec2> {
    let mut sorted: Vec<&Side> = sides.iter().collect();
    sorted.sort_by(|a, b| angle(a).total_cmp(&angle(b)));
    (0..sorted.len())
        .map(|i| meet(sorted[i], sorted[(i + 1) % sorted.len()]))
        .collect()
}

fn angle(side: &Side) -> f32 {
    side.normal.y.atan2(side.normal.x).rem_euclid(TAU)
}

/// Where two sides' lines cross.
fn meet(a: &Side, b: &Side) -> Vec2 {
    let (n1, n2) = (vec2(a.normal.x, a.normal.y), vec2(b.normal.x, b.normal.y));
    let det = n1.x * n2.y - n1.y * n2.x;
    vec2(
        (a.offset * n2.y - b.offset * n1.y) / det,
        (n1.x * b.offset - n2.x * a.offset) / det,
    )
}

/// Each side's two ends, in the order of its tangent.
fn ends(side: &Side, corners: &[Vec2]) -> (Vec2, Vec2) {
    let on: Vec<Vec2> = corners
        .iter()
        .copied()
        .filter(|c| (c.x * side.normal.x + c.y * side.normal.y - side.offset).abs() < 1e-3)
        .collect();
    let t = vec2(side.tangent().x, side.tangent().y);
    let (a, b) = (on[0], on[1]);
    if a.dot(t) < b.dot(t) { (a, b) } else { (b, a) }
}

/// A flat polygon on the floor (convex, round its centre), wound so it faces up.
fn floor_mesh(points: &[Vec2], y: f32) -> Mesh {
    let center = points.iter().copied().sum::<Vec2>() / points.len() as f32;
    let mut positions = vec![[center.x, y, center.y]];
    positions.extend(points.iter().map(|p| [p.x, y, p.y]));
    let mut indices = Vec::new();
    for i in 0..points.len() {
        let (a, b) = (1 + i as u32, 1 + ((i + 1) % points.len()) as u32);
        let (pa, pb) = (points[i] - center, points[(i + 1) % points.len()] - center);
        // Seen from above (+y), a counter-clockwise triangle faces up.
        if pa.x * pb.y - pa.y * pb.x < 0.0 {
            indices.extend([0, a, b]);
        } else {
            indices.extend([0, b, a]);
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_indices(Indices::U32(indices))
}

/// A box along the floor from `a` to `b`, `thickness` across and `height` tall, its inner face on
/// the line.
fn wall_between(
    a: Vec2,
    b: Vec2,
    outward: Vec2,
    thickness: f32,
    height: f32,
) -> (Cuboid, Transform) {
    let along = b - a;
    let middle = (a + b) / 2.0 + outward * (thickness / 2.0);
    (
        Cuboid::new(along.length(), height, thickness),
        Transform::from_xyz(middle.x, height / 2.0, middle.y)
            .with_rotation(Quat::from_rotation_y((-along.y).atan2(along.x))),
    )
}

/// A goal glows in its owner's colour: the seat's (a duel's two seats are teams 0 and 1), or
/// the team's.
fn goal_color(game: &State, owner: u8) -> LinearRgba {
    match game.format {
        Format::FreeForAll => seat_color(Seat(owner)),
        Format::Duel { a, b } => seat_color(if owner == 0 { a } else { b }),
        _ => TEAM_COLORS[owner as usize % 2],
    }
}

fn spawn_pitch(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    game: &State,
    corners: &[Vec2],
) {
    // The ground round the pitch, then the pitch itself, a little lighter.
    commands.spawn((
        SCOPE,
        Mesh3d(meshes.add(Cuboid::new(80.0, 0.02, 80.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.02, 0.03, 0.03),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.02, 0.0),
    ));
    commands.spawn((
        SCOPE,
        Mesh3d(meshes.add(floor_mesh(corners, 0.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.07, 0.16, 0.1),
            perceptual_roughness: 0.85,
            ..default()
        })),
    ));
    // Pitch markings: a centre circle (where the ball drops) and a line out to every goal.
    let line = materials.add(emissive_material(LinearRgba::rgb(0.6, 0.85, 0.7), 0.6));
    commands.spawn((
        SCOPE,
        Mesh3d(meshes.add(Torus::new(2.0, 2.08).mesh().major_resolution(64))),
        MeshMaterial3d(line.clone()),
        Transform::from_xyz(0.0, 0.01, 0.0),
    ));
    for side in game.sides.iter().filter(|s| s.goal.is_some()) {
        let mouth = vec2(side.center().x, side.center().y);
        let along = mouth.normalize_or_zero();
        let (start, end) = (along * 2.1, mouth - along * 0.2);
        let (cuboid, transform) = wall_between(start, end, along.perp(), 0.06, 0.005);
        commands.spawn((
            SCOPE,
            Mesh3d(meshes.add(cuboid)),
            MeshMaterial3d(line.clone()),
            transform.with_translation(transform.translation + Vec3::new(0.0, 0.005, 0.0)),
        ));
    }

    let rail = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.07, 0.07),
        perceptual_roughness: 0.7,
        ..default()
    });
    let rail_glow = materials.add(emissive_material(LANTERN_WARM, 2.0));
    for (index, side) in game.sides.iter().enumerate() {
        let (a, b) = ends(side, corners);
        let outward = vec2(side.normal.x, side.normal.y);
        if side.goal.is_some() {
            // Its walls go with it: they reach in to the posts, wherever those are.
            spawn_goal(commands, meshes, materials, game, index, side, corners);
            continue;
        }
        let (cuboid, transform) = wall_between(a, b, outward, WALL_THICKNESS, WALL_HEIGHT);
        commands.spawn((
            SCOPE,
            Mesh3d(meshes.add(cuboid)),
            MeshMaterial3d(rail.clone()),
            transform,
        ));
        // A glowing strip along the top, so the walls read at night.
        let (strip, strip_at) = wall_between(a, b, outward, 0.12, 0.06);
        commands.spawn((
            SCOPE,
            Mesh3d(meshes.add(strip)),
            MeshMaterial3d(rail_glow.clone()),
            strip_at.with_translation(strip_at.translation + Vec3::Y * WALL_HEIGHT),
        ));
    }

    let bumper_mesh = meshes.add(Cylinder::new(BUMPER_RADIUS, 0.7).mesh().resolution(28));
    let cap = meshes.add(
        Cylinder::new(BUMPER_RADIUS * 0.6, 0.08)
            .mesh()
            .resolution(20),
    );
    let ring = meshes.add(Torus::new(BUMPER_RADIUS * 0.92, BUMPER_RADIUS * 1.05));
    for (index, bumper) in game.bumpers.iter().enumerate() {
        let material = materials.add(emissive_material(LinearRgba::rgb(0.3, 1.0, 0.85), 2.5));
        commands
            .spawn((
                SCOPE,
                BumperView {
                    index,
                    material: material.clone(),
                },
                Transform::from_translation(world(*bumper)),
                Visibility::default(),
            ))
            .with_children(|b| {
                b.spawn((
                    Mesh3d(bumper_mesh.clone()),
                    MeshMaterial3d(rail.clone()),
                    Transform::from_xyz(0.0, 0.35, 0.0),
                ));
                b.spawn((
                    Mesh3d(cap.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(0.0, 0.72, 0.0),
                ));
                for y in [0.2, 0.55] {
                    b.spawn((
                        Mesh3d(ring.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::from_xyz(0.0, y, 0.0),
                    ));
                }
            });
    }
}

/// A goal, built in its side's frame: x along the side (its tangent), -z out of the pitch,
/// centred on the mouth. Its parts are unit-sized where they stretch with the goal's width, and
/// [`animate_goals`] places them for the width it has now.
fn spawn_goal(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    game: &State,
    index: usize,
    side: &Side,
    corners: &[Vec2],
) {
    let goal = side.goal.expect("a goal side");
    let color = goal_color(game, goal.owner);
    let frame = materials.add(emissive_material(color, 3.0));
    let floor = materials.add(emissive_material(color, 0.35));
    let net = materials.add(StandardMaterial {
        base_color: Color::NONE,
        emissive: color * 0.6,
        alpha_mode: AlphaMode::Add,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let crease = materials.add(emissive_material(color, 1.2));
    let rail = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.07, 0.07),
        perceptual_roughness: 0.7,
        ..default()
    });
    let rail_glow = materials.add(emissive_material(LANTERN_WARM, 2.0));
    let unit = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let post = meshes.add(
        Cylinder::new(POST_RADIUS, GOAL_HEIGHT)
            .mesh()
            .resolution(12),
    );
    let center = vec2(side.center().x, side.center().y);
    let outward = Vec3::new(side.normal.x, 0.0, side.normal.y);
    let root = commands
        .spawn((
            SCOPE,
            Transform::from_xyz(center.x, 0.0, center.y).looking_to(outward, Vec3::Y),
            Visibility::default(),
        ))
        .id();
    let mut part = |mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, shown: bool| {
        let visibility = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let id = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                visibility,
            ))
            .id();
        commands.entity(root).add_child(id);
        id
    };
    let rails = [part(&unit, &rail, true), part(&unit, &rail, true)];
    let strips = [part(&unit, &rail_glow, true), part(&unit, &rail_glow, true)];
    let posts = [part(&post, &frame, true), part(&post, &frame, true)];
    let nets = [part(&unit, &net, true), part(&unit, &net, true)];
    let bar = part(&unit, &frame, true);
    let back = part(&unit, &net, true);
    let inside = part(&unit, &floor, true);
    let barrier_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.11, 0.12),
        emissive: color * 0.2,
        perceptual_roughness: 0.6,
        ..default()
    });
    let barrier = part(&unit, &barrier_material, false);
    let base_crease = side.crease().expect("open at the start");
    let arc = meshes.add(arc_mesh(base_crease, 0.08, 40));
    let crease_line = part(&arc, &crease, true);
    let (a, b) = ends(side, corners);
    let t = vec2(side.tangent().x, side.tangent().y);
    commands.entity(root).insert(GoalView {
        side: index,
        glow: vec![frame, floor, net, crease],
        color,
        parts: GoalParts {
            rails,
            strips,
            posts,
            nets,
            bar,
            back,
            floor: inside,
            barrier,
            crease: crease_line,
            ends: ((a - center).dot(t), (b - center).dot(t)),
            base_crease,
        },
    });
}

/// A flat half-ring on the floor in a goal's frame, from one side of the mouth round into the
/// pitch (+z) to the other, wound to face up.
fn arc_mesh(radius: f32, width: f32, segments: usize) -> Mesh {
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for k in 0..=segments {
        let angle = std::f32::consts::PI * k as f32 / segments as f32;
        let (c, s) = (angle.cos(), angle.sin());
        for r in [radius - width / 2.0, radius + width / 2.0] {
            positions.push([r * c, 0.012, r * s]);
        }
        if k > 0 {
            let i = (2 * k) as u32;
            indices.extend([i - 2, i, i - 1, i - 1, i, i + 1]);
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_indices(Indices::U32(indices))
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
    let r = folk.body.radius;
    let disc = commands
        .spawn((
            SCOPE,
            Mesh3d(meshes.add(Cylinder::new(r, 0.04))),
            MeshMaterial3d(disc_material.clone()),
            Transform::from_translation(world(folk.body.pos) + Vec3::Y * 0.03),
        ))
        .id();
    let charge_material = materials.add(emissive_material(seat_color(seat), 0.0));
    let charge = commands
        .spawn((
            SCOPE,
            Mesh3d(meshes.add(Torus::new(r * 1.15, r * 1.32))),
            MeshMaterial3d(charge_material.clone()),
            Transform::from_translation(world(folk.body.pos) + Vec3::Y * 0.05),
            Visibility::Hidden,
        ))
        .id();
    // An arrowhead on the floor ahead of the folk, the way the kick will go.
    let arrow = commands
        .spawn((
            SCOPE,
            Mesh3d(meshes.add(Triangle3d::new(
                Vec3::new(0.0, 0.0, 0.45),
                Vec3::new(0.3, 0.0, -0.1),
                Vec3::new(-0.3, 0.0, -0.1),
            ))),
            MeshMaterial3d(charge_material.clone()),
            Transform::default(),
            Visibility::Hidden,
        ))
        .id();
    commands.entity(lantern_folk.root).insert(FolkView {
        seat,
        head_material: lantern_folk.head_material,
        head_light: lantern_folk.head_light,
        disc,
        disc_material,
        disc_color,
        charge,
        charge_material,
        arrow,
    });
}

fn spawn_ball(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    game: &State,
    index: usize,
) {
    let material = materials.add(emissive_material(BALL_GLOW, 2.5));
    let band = materials.add(emissive_material(LANTERN_WARM, 4.0));
    let dark = materials.add(StandardMaterial {
        base_color: Color::srgb(0.06, 0.05, 0.05),
        metallic: 0.5,
        perceptual_roughness: 0.5,
        ..default()
    });
    let light = commands
        .spawn((
            PointLight {
                color: Color::from(BALL_GLOW),
                intensity: 150_000.0,
                range: 8.0,
                ..default()
            },
            Transform::default(),
        ))
        .id();
    let ghost_mesh = meshes.add(Sphere::new(BALL_RADIUS * 0.6).mesh().ico(2).unwrap());
    let ghosts = std::array::from_fn(|_| {
        let ghost_material = materials.add(StandardMaterial {
            base_color: Color::NONE,
            emissive: BALL_GLOW * 0.0,
            alpha_mode: AlphaMode::Add,
            unlit: true,
            ..default()
        });
        let ghost = commands
            .spawn((
                SCOPE,
                Mesh3d(ghost_mesh.clone()),
                MeshMaterial3d(ghost_material.clone()),
                Transform::default(),
                Visibility::Hidden,
            ))
            .id();
        (ghost, ghost_material)
    });
    let shadow_material = materials.add(StandardMaterial {
        base_color: Color::NONE,
        emissive: BALL_GLOW * 0.0,
        alpha_mode: AlphaMode::Add,
        unlit: true,
        ..default()
    });
    let shadow = commands
        .spawn((
            SCOPE,
            Mesh3d(meshes.add(Torus::new(BALL_RADIUS * 0.9, BALL_RADIUS * 1.1))),
            MeshMaterial3d(shadow_material.clone()),
            Transform::from_xyz(0.0, 0.02, 0.0),
            Visibility::Hidden,
        ))
        .id();
    let ball = &game.balls[index];
    let mut root = commands.spawn((
        SCOPE,
        Transform::from_translation(ball_world(ball.body.pos)),
        Visibility::default(),
    ));
    root.add_child(light);
    // A paper lantern ball: a glowing sphere with dark caps and a band round it, so it's seen
    // to roll.
    let r = BALL_RADIUS;
    root.with_children(|b| {
        b.spawn((
            Mesh3d(meshes.add(Sphere::new(r).mesh().uv(28, 18))),
            MeshMaterial3d(material.clone()),
        ));
        b.spawn((
            Mesh3d(meshes.add(Torus::new(r * 0.97, r * 1.06).mesh().major_resolution(40))),
            MeshMaterial3d(band.clone()),
        ));
        for y in [-r * 0.93, r * 0.93] {
            b.spawn((
                Mesh3d(meshes.add(Cylinder::new(r * 0.38, r * 0.12))),
                MeshMaterial3d(dark.clone()),
                Transform::from_xyz(0.0, y, 0.0),
            ));
        }
    });
    root.insert(BallView {
        index,
        material,
        light,
        shadow,
        shadow_material,
        ghosts,
        history: VecDeque::new(),
        spin: Quat::IDENTITY,
    });
}

/// Simulation floor coordinates to world space.
fn world(v: SimVec2) -> Vec3 {
    Vec3::new(v.x, 0.0, v.y)
}

fn ball_world(v: SimVec2) -> Vec3 {
    world(v) + Vec3::Y * BALL_RADIUS
}

fn vec2(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn blend(fixed: &Time<Fixed>) -> f32 {
    fixed.overstep_fraction().clamp(0.0, 1.0)
}

/// Seconds since a tick of play, for one-shot effects.
fn since(game: &State, tick: u32) -> f32 {
    game.tick.saturating_sub(tick) as f32 / TICK_HZ as f32
}

fn facing_rotation(facing: SimVec2) -> Quat {
    Quat::from_rotation_y(facing.x.atan2(facing.y))
}

fn animate_folk(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternBall>>,
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
        let pos = was.body.pos.lerp(is.body.pos, alpha);
        // The out sink into the floor.
        let sunk = match is.status {
            Status::Out { t } => (t as f32 + alpha) / TICK_HZ as f32 * 2.5,
            _ => 0.0,
        };
        *visibility = if sunk < 2.5 && is.status != Status::Benched {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let celebrating = winners.contains(&seat) && is.playing();
        *pose = if celebrating {
            Pose::Celebrate
        } else if game.outcome.is_some() || !is.playing() {
            Pose::Slump
        } else {
            Pose::Idle
        };
        let mut rotation = facing_rotation(is.facing);
        let mut scale = Vec3::ONE;
        if is.dash > 0 {
            rotation *= Quat::from_rotation_x(0.45);
            scale = Vec3::new(0.9, 0.95, 1.2);
        }
        if is.charging {
            // Winding up: leaning back, squashing a little more the longer it's held.
            let share = is.charge_share();
            rotation *= Quat::from_rotation_x(-0.25 * share);
            scale = Vec3::new(1.0 + share * 0.08, 1.0 - share * 0.1, 1.0);
        }
        if is.stun > 0 {
            rotation *= Quat::from_rotation_z((now * 30.0).sin() * 0.35);
        }
        transform.translation = world(pos) - Vec3::Y * sunk;
        transform.rotation = rotation;
        transform.scale = scale * folk_scale(game, seat);

        let knock_flash = is
            .knocked
            .map(|(tick, _)| (1.0 - since(game, tick) * 3.0).max(0.0))
            .unwrap_or(0.0);
        let glow = match is.status {
            Status::Playing if celebrating => 5.0,
            Status::Playing => 1.8 + knock_flash * 6.0,
            Status::Out { .. } => (1.8 - sunk).max(0.0),
            Status::Benched => 0.0,
        };
        if let Some(mut material) = materials.get_mut(&view.head_material) {
            material.emissive = seat_color(seat) * glow;
        }
        if let Ok(mut light) = lights.get_mut(view.head_light) {
            light.intensity = 22_000.0 * glow;
        }

        let on_pitch = is.playing();
        if let Some(mut material) = materials.get_mut(&view.disc_material) {
            let ready = is.dash_cooldown == 0;
            let strength = if ready {
                2.6 + (now * 6.0).sin() * 0.4
            } else {
                0.5
            };
            material.emissive = view.disc_color * strength;
        }
        let shown = |on: bool| {
            if on {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            }
        };
        if let Ok((mut disc, mut disc_visibility)) = parts.get_mut(view.disc) {
            disc.translation = world(pos) + Vec3::Y * 0.03;
            *disc_visibility = shown(on_pitch);
        }
        // The charge ring swells and brightens; full, it pulses.
        let share = is.charge_share();
        let full = is.charge >= CHARGE_TICKS;
        if let Some(mut material) = materials.get_mut(&view.charge_material) {
            let pulse = if full {
                1.0 + (now * 14.0).sin() * 0.4
            } else {
                1.0
            };
            material.emissive = seat_color(seat).mix(&LinearRgba::WHITE, share * 0.5)
                * ((1.5 + share * 6.0) * pulse);
        }
        if let Ok((mut ring, mut ring_visibility)) = parts.get_mut(view.charge) {
            ring.translation = world(pos) + Vec3::Y * 0.05;
            ring.scale = Vec3::splat(1.0 + share * 0.5);
            *ring_visibility = shown(on_pitch && is.charging);
        }
        if let Ok((mut arrow, mut arrow_visibility)) = parts.get_mut(view.arrow) {
            let reach = is.body.radius * (1.6 + share * 0.5) + 0.2;
            arrow.translation = world(pos + is.facing * reach) + Vec3::Y * 0.06;
            arrow.rotation = facing_rotation(is.facing);
            arrow.scale = Vec3::splat(folk_scale(game, seat) * (0.8 + share * 0.5));
            *arrow_visibility = shown(on_pitch && is.charging);
        }
    }
}

/// Where a ball is drawn this frame, and how high.
fn ball_at(active: &ActiveRun<LanternBall>, index: usize, alpha: f32) -> Vec3 {
    let game = &active.run.game;
    let (was, is) = (&active.previous.balls[index], &game.balls[index]);
    match is.status {
        BallStatus::Dropping { t } => {
            let fall = ((t as f32 + alpha) / DROP_TICKS as f32).min(1.0);
            ball_world(is.body.pos) + Vec3::Y * (DROP_HEIGHT * (1.0 - fall * fall))
        }
        BallStatus::Scored { side, .. } => {
            // Into the net, no further than its back.
            let side = &game.sides[side as usize];
            let mut pos = was.body.pos.lerp(is.body.pos, alpha);
            let past = side.depth(pos) - (GOAL_DEPTH - BALL_RADIUS);
            if past > 0.0 {
                pos -= side.normal * past;
            }
            ball_world(pos)
        }
        _ if was.status == is.status => ball_world(was.body.pos.lerp(is.body.pos, alpha)),
        _ => ball_world(is.body.pos),
    }
}

fn animate_balls(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternBall>>,
    mut balls: Query<(&mut BallView, &mut Transform, &mut Visibility)>,
    mut parts: Query<(&mut Transform, &mut Visibility), Without<BallView>>,
    mut lights: Query<&mut PointLight>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let alpha = blend(&fixed);
    let now = time.elapsed_secs();
    let game = &active.run.game;
    for (mut view, mut transform, mut visibility) in &mut balls {
        let ball: &Ball = &game.balls[view.index];
        let pos = ball_at(&active, view.index, alpha);
        let waiting = ball.status == BallStatus::Waiting;
        *visibility = if waiting {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        // Roll: turn about the axis across the way it moved, by how far it went.
        let moved = pos - transform.translation;
        let flat = Vec3::new(moved.x, 0.0, moved.z);
        if flat.length() > 1e-4 && flat.length() < 2.0 {
            let axis = Vec3::new(flat.z, 0.0, -flat.x).normalize();
            view.spin = Quat::from_axis_angle(axis, flat.length() / BALL_RADIUS) * view.spin;
        }
        transform.translation = pos;
        // A ball left alone trembles, more and more, before it hops off by itself.
        if ball.rolling() && ball.still > RESTLESS_TICKS / 2 {
            let urge = (ball.still - RESTLESS_TICKS / 2) as f32 / (RESTLESS_TICKS / 2) as f32;
            transform.translation += Vec3::new(
                (now * 47.0).sin(),
                (now * 31.0).sin().abs(),
                (now * 53.0).cos(),
            ) * (urge * 0.08);
        }
        // A scored ball shrinks away in the net.
        let fade = match ball.status {
            BallStatus::Scored { t, .. } => 1.0 - (t as f32 / SCORED_TICKS as f32).powi(3),
            _ => 1.0,
        };
        transform.rotation = view.spin;
        transform.scale = Vec3::splat(fade.max(0.05));

        let speed = ball.body.vel.length();
        let charge = if ball.rolling() {
            (speed / MAX_BALL_SPEED).min(1.0)
        } else {
            0.0
        };
        if let Some(mut material) = materials.get_mut(&view.material) {
            material.emissive = BALL_GLOW * (2.5 + charge * 6.0);
        }
        if let Ok(mut light) = lights.get_mut(view.light) {
            light.intensity = if waiting {
                0.0
            } else {
                150_000.0 + charge * 250_000.0
            };
        }
        // Where a dropping ball will land: a ring that tightens and brightens as it falls.
        if let Ok((mut ring, mut ring_visibility)) = parts.get_mut(view.shadow) {
            if let BallStatus::Dropping { t } = ball.status {
                let fall = t as f32 / DROP_TICKS as f32;
                *ring_visibility = Visibility::Inherited;
                ring.scale = Vec3::new(2.4 - fall * 1.4, 1.0, 2.4 - fall * 1.4);
                if let Some(mut material) = materials.get_mut(&view.shadow_material) {
                    material.emissive =
                        BALL_GLOW * ((1.0 + fall * 4.0) * (1.0 + (now * 10.0).sin() * 0.3));
                }
            } else {
                *ring_visibility = Visibility::Hidden;
            }
        }
        // A trail behind a fast ball.
        if view.history.front() != Some(&pos) {
            view.history.push_front(pos);
            view.history.truncate(GHOSTS * 2 + 1);
        }
        if !ball.rolling() {
            view.history.clear();
        }
        let fast = ((speed - 8.0) / (MAX_BALL_SPEED - 8.0)).clamp(0.0, 1.0);
        for (k, (ghost, ghost_material)) in view.ghosts.iter().enumerate() {
            let Ok((mut ghost_transform, mut ghost_visibility)) = parts.get_mut(*ghost) else {
                continue;
            };
            let sample = view.history.get(k * 2 + 1).copied();
            *ghost_visibility = match sample {
                Some(_) if fast > 0.0 && ball.rolling() => Visibility::Inherited,
                _ => Visibility::Hidden,
            };
            if let Some(sample) = sample {
                let fade = 1.0 - (k as f32 + 1.0) / (GHOSTS as f32 + 1.0);
                ghost_transform.translation = sample;
                ghost_transform.scale = Vec3::splat(0.5 + fade * 0.6);
                if let Some(mut material) = materials.get_mut(ghost_material) {
                    material.emissive = BALL_GLOW * (fast * fade * 2.5);
                }
            }
        }
    }
}

/// Bumpers flash when a ball comes off them.
fn animate_bumpers(
    active: Res<ActiveRun<LanternBall>>,
    bumpers: Query<&BumperView>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let game = &active.run.game;
    for view in &bumpers {
        let at = game.bumpers[view.index];
        let flash = game
            .balls
            .iter()
            .filter(|b| b.rolling() && b.body.pos.distance(at) < BALL_RADIUS + BUMPER_RADIUS + 0.6)
            .filter_map(|b| b.bounced_at)
            .map(|tick| (1.0 - since(game, tick) * 4.0).max(0.0))
            .fold(0.0, f32::max);
        if let Some(mut material) = materials.get_mut(&view.material) {
            material.emissive = LinearRgba::rgb(0.3, 1.0, 0.85).mix(&LinearRgba::WHITE, flash)
                * (2.5 + flash * 12.0);
        }
    }
}

/// Goals follow their width (it grows, and shrinks for every goal their owner scores), flash
/// when scored in, and go dark behind a barrier once walled up.
fn animate_goals(
    active: Res<ActiveRun<LanternBall>>,
    goals: Query<&GoalView>,
    mut parts: Query<(&mut Transform, &mut Visibility), Without<GoalView>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let game = &active.run.game;
    for view in &goals {
        let side = &game.sides[view.side];
        let goal = side.goal.expect("a goal side");
        let half = goal.half_width;
        let p = &view.parts;
        // Local frame: x along the side, -z out of the pitch.
        let mut place = |entity: Entity, transform: Transform, shown: bool| {
            if let Ok((mut t, mut v)) = parts.get_mut(entity) {
                *t = transform;
                *v = if shown {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        };
        for (k, (end, post)) in [(p.ends.0, -half), (p.ends.1, half)]
            .into_iter()
            .enumerate()
        {
            let length = (end - post).abs().max(0.01);
            let middle = (end + post) / 2.0;
            place(
                p.rails[k],
                Transform::from_xyz(middle, WALL_HEIGHT / 2.0, -WALL_THICKNESS / 2.0)
                    .with_scale(Vec3::new(length, WALL_HEIGHT, WALL_THICKNESS)),
                true,
            );
            place(
                p.strips[k],
                Transform::from_xyz(middle, WALL_HEIGHT + 0.03, -0.06)
                    .with_scale(Vec3::new(length, 0.06, 0.12)),
                true,
            );
            place(
                p.posts[k],
                Transform::from_xyz(post, GOAL_HEIGHT / 2.0, 0.0),
                true,
            );
            place(
                p.nets[k],
                Transform::from_xyz(post, GOAL_HEIGHT / 2.0, -GOAL_DEPTH / 2.0)
                    .with_scale(Vec3::new(0.05, GOAL_HEIGHT, GOAL_DEPTH)),
                true,
            );
        }
        let width = half * 2.0;
        place(
            p.bar,
            Transform::from_xyz(0.0, GOAL_HEIGHT, 0.0).with_scale(Vec3::new(
                width,
                POST_RADIUS * 1.6,
                POST_RADIUS * 1.6,
            )),
            true,
        );
        place(
            p.back,
            Transform::from_xyz(0.0, GOAL_HEIGHT / 2.0, -GOAL_DEPTH).with_scale(Vec3::new(
                width,
                GOAL_HEIGHT,
                0.05,
            )),
            true,
        );
        place(
            p.floor,
            Transform::from_xyz(0.0, 0.005, -GOAL_DEPTH / 2.0)
                .with_scale(Vec3::new(width, 0.01, GOAL_DEPTH)),
            true,
        );
        place(
            p.barrier,
            Transform::from_xyz(0.0, 0.6, -0.175).with_scale(Vec3::new(width, 1.2, 0.35)),
            !goal.open,
        );
        let crease = side.crease().unwrap_or(p.base_crease) / p.base_crease;
        place(
            p.crease,
            Transform::from_scale(Vec3::new(crease, 1.0, crease)),
            goal.open,
        );

        let flash = game
            .last_goal
            .filter(|g| g.against == goal.owner)
            .map(|g| (1.0 - since(game, g.tick) * 1.5).max(0.0))
            .unwrap_or(0.0);
        let strength = if goal.open { 1.0 + flash * 4.0 } else { 0.15 };
        for (k, handle) in view.glow.iter().enumerate() {
            let base = [3.0, 0.35, 0.6, 1.2][k];
            if let Some(mut material) = materials.get_mut(handle) {
                material.emissive =
                    view.color.mix(&LinearRgba::WHITE, flash * 0.5) * (base * strength);
            }
        }
    }
}

const CAMERA_TARGET: Vec3 = Vec3::new(0.0, -0.4, 1.8);
const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 16.0, 10.0);

/// Room on the pitch's near and far sides for the HUD, in floor units.
const HUD_MARGIN: f32 = 2.6;

fn camera_transform(pull_back: f32) -> Transform {
    Transform::from_translation(CAMERA_TARGET + CAMERA_OFFSET * pull_back)
        .looking_at(CAMERA_TARGET, Vec3::Y)
}

/// Pull the camera back until the whole pitch, goals included, is in frame.
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
        let distance = CAMERA_OFFSET.length();
        let half_height = (perspective.fov / 2.0).tan() * distance;
        let wide = rig.framed.x / (half_height * aspect);
        // Seen at a slant, the pitch's depth is foreshortened; the banner and the seat panels
        // take a strip off the top and bottom.
        let slant = CAMERA_OFFSET.y / distance;
        let deep = (rig.framed.y + HUD_MARGIN) * slant / half_height;
        rig.base = camera_transform(wide.max(deep).max(1.0));
    }
}

/// A kick on every knock-down and every goal.
fn shake_camera(
    active: Res<ActiveRun<LanternBall>>,
    time: Res<Time>,
    mut camera: Query<(&CameraRig, &mut Transform)>,
) {
    let game = &active.run.game;
    let knocks = game
        .players
        .seats()
        .filter_map(|s| game.folk(s).knocked)
        .map(|(tick, _)| (0.08 - since(game, tick) * 0.4).max(0.0));
    let goal = game
        .last_goal
        .map(|g| (0.18 - since(game, g.tick) * 0.4).max(0.0));
    let strength = knocks.chain(goal).fold(0.0, f32::max);
    let t = time.elapsed_secs();
    for (rig, mut transform) in &mut camera {
        *transform = rig.base;
        transform.translation += Vec3::new((t * 91.0).sin(), (t * 73.0).cos(), 0.0) * strength;
    }
}

fn spawn_hud(mut commands: Commands, active: Res<ActiveRun<LanternBall>>) {
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
            for seat in game.taking_part() {
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

/// The score line under the banner: the clock, and in 1v3 how near the three are.
fn scoreline(game: &State) -> String {
    let time = clock(game.ticks_left());
    match game.format {
        Format::OneVsThree { .. } => format!(
            "{time}   {}",
            tr!(
                "lantern-ball-trio-score",
                goals = game.trio_goals,
                goal = TRIO_GOAL
            )
        ),
        _ => time,
    }
}

/// Who a goal's owner is, by name: the seat in free-for-all, otherwise someone on the team.
fn owner_label(game: &State, owner: u8) -> String {
    match game.format {
        Format::FreeForAll => seat_label(Seat(owner)),
        _ => {
            let team: Vec<String> = game
                .taking_part()
                .filter(|s| game.owner_of(*s) == Some(owner))
                .map(seat_label)
                .collect();
            team.join(" & ")
        }
    }
}

fn goal_banner(game: &State) -> Option<String> {
    let goal = game.last_goal.filter(|g| since(game, g.tick) < 1.6)?;
    let against = owner_label(game, goal.against);
    Some(match goal.by {
        Some(by) if game.owner_of(by) == Some(goal.against) => {
            tr!("lantern-ball-own-goal", player = seat_label(by))
        }
        Some(by) if game.keeper() == Some(by) => tr!("lantern-ball-taken-back"),
        Some(by) if game.format == Format::FreeForAll => tr!(
            "lantern-ball-scored-on",
            by = seat_label(by),
            player = against
        ),
        Some(by) => tr!("lantern-ball-scores", by = seat_label(by)),
        None => tr!("lantern-ball-goal-against", player = against),
    })
}

fn banner_text(active: &ActiveRun<LanternBall>) -> (String, String) {
    let run = &active.run;
    let game = &run.game;
    if let Some(banner) = halted_banner(run) {
        return banner;
    }
    match game.phase {
        // The shared countdown (crate::intro) is drawn over this.
        Phase::Intro { .. } => (String::new(), String::new()),
        Phase::Playing => {
            let out = game
                .taking_part()
                .filter_map(|s| game.out_at[s.index()].map(|t| (t, s)))
                .max()
                .filter(|(t, _)| since(game, *t) < 2.0);
            let title = if let Some((_, seat)) = out {
                tr!("lantern-ball-is-out", player = seat_label(seat))
            } else if let Some(goal) = goal_banner(game) {
                goal
            } else if game.tick >= SECOND_BALL_TICK && since(game, SECOND_BALL_TICK) < 1.5 {
                tr!("lantern-ball-second-ball")
            } else if game.sudden_death() {
                tr!("lantern-ball-sudden-death")
            } else if game.tick < 45 {
                tr!("lantern-ball-go")
            } else {
                String::new()
            };
            let sub = if game.sudden_death() {
                tr!("lantern-ball-sudden-death-sub")
            } else {
                scoreline(game)
            };
            (title, sub)
        }
        Phase::Decided { .. } | Phase::Finished => {
            let outcome = game.outcome.expect("decided games have an outcome");
            let winners: Vec<Seat> = outcome.winners().collect();
            let title = if winners.len() == game.taking_part().count() {
                tr!("lantern-ball-everyone")
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
    active: Res<ActiveRun<LanternBall>>,
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
                let tally = match (game.format, game.owner_of(seat)) {
                    (Format::OneVsThree { .. }, _) | (_, None) => {
                        tr!("lantern-ball-goals", goals = game.scored[seat.index()])
                    }
                    (_, Some(owner)) => tr!(
                        "lantern-ball-lives",
                        lives = game.lives[owner as usize] as u32,
                        max = LIVES as u32
                    ),
                };
                let status = if active.run.controller_lost(seat) {
                    tr!("controller-lost")
                } else if let (Some(outcome), true) = (game.outcome, game.phase != Phase::Playing) {
                    match outcome.placement[seat.index()] {
                        Some(place) => tr!("place", place = place as u32),
                        None => String::new(),
                    }
                } else if !folk.playing() {
                    tr!("lantern-ball-out")
                } else if folk.stun > 0 {
                    tr!("lantern-ball-stunned")
                } else if folk.charging {
                    tr!("lantern-ball-charging")
                } else {
                    String::new()
                };
                format!("{}{who}{role}\n{tally}\n{status}", seat_label(seat))
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use minigame_lantern_ball::{FFA_HALF, FIELD_HALF};
    use sim_core::{Minigame, PlayerSetup};

    #[test]
    fn facing_rotation_turns_the_model_front_towards_facing() {
        let front = facing_rotation(SimVec2::new(1.0, 0.0)) * Vec3::Z;
        assert!((front - Vec3::X).length() < 1e-5);
    }

    #[test]
    fn corners_close_the_pitch_and_every_side_has_two() {
        for (players, format) in [
            (4, Format::FreeForAll),
            (3, Format::FreeForAll),
            (2, Format::FreeForAll),
            (4, Format::OneVsThree { solo: Seat(0) }),
        ] {
            let game = LanternBall::new(1, PlayerSetup::first(players), format);
            let corners = corners(&game.sides);
            assert_eq!(corners.len(), game.sides.len());
            for side in &game.sides {
                let (a, b) = ends(side, &corners);
                assert!(a.distance(b) > 0.5, "{format:?}");
            }
        }
        let square = LanternBall::new(1, PlayerSetup::first(4), Format::FreeForAll);
        let far = corners(&square.sides)
            .iter()
            .fold(0.0f32, |m, c| m.max(c.x.abs()));
        assert!((far - FFA_HALF).abs() < 1e-3);
        let field = LanternBall::new(
            1,
            PlayerSetup::first(4),
            Format::OneVsThree { solo: Seat(0) },
        );
        let far = corners(&field.sides)
            .iter()
            .fold(0.0f32, |m, c| m.max(c.y.abs()));
        assert!((far - FIELD_HALF.y).abs() < 1e-3);
    }

    #[test]
    fn the_floor_and_the_creases_face_up() {
        let game = LanternBall::new(1, PlayerSetup::first(3), Format::FreeForAll);
        for mesh in [floor_mesh(&corners(&game.sides), 0.0), arc_mesh(3.0, 0.1, 12)] {
            faces_up(&mesh);
        }
    }

    fn faces_up(mesh: &Mesh) {
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions");
        };
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("indices");
        };
        for tri in indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|k| Vec3::from(positions[tri[k] as usize]));
            assert!((b - a).cross(c - a).y > 0.0);
        }
    }
}
