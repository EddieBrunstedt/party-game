//! Presentation for Lantern Run. Reads `ActiveRun<LanternRun>` only; never writes simulation
//! state. The camera rides just behind the kill line looking down the course, pulled back far
//! enough to see the whole canyon or cave across. Walls and obstacles are built in chunks ahead
//! of the camera and dropped behind it.
//!
//! The simulation's course runs along `z` with `x` right and `y` up across it; the world puts it
//! down `-Z` (the way a Bevy camera looks), with the middle of the course at `y = 0`.

use std::collections::VecDeque;
use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use minigame_lantern_run::course::CAVE_Z;
use minigame_lantern_run::{
    CLOSE_IN_AFTER, CRUISE, GHOST_TICKS, LanternRun, METER_MAX, MINE_RADIUS, Obstacle, Phase,
    Shape, State, Status, half_extents,
};
use sim_core::physics::Vec2 as SimVec2;
use sim_core::{Format, Seat, TICK_HZ};

use crate::audio::{Sfx, Sound};
use crate::flow::AppState;
use crate::i18n::{self, bot_tag, halted_banner};
use crate::palette::{LANTERN_WARM, MOONLIGHT, NIGHT_SKY, seat_color, seat_label};
use crate::particles::{self, Effects, hdr};
use crate::runner::{ActiveRun, MinigameSet, Seats};
use crate::scenery::{emissive_material, night_camera, noise};
use crate::tr;

pub struct LanternRunViewPlugin;

impl Plugin for LanternRunViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            (spawn_scene, spawn_hud)
                .after(MinigameSet::Start)
                .run_if(resource_exists::<ActiveRun<LanternRun>>),
        )
        .add_systems(
            Update,
            (
                build_course,
                fade_obstacles,
                animate_ships,
                animate_ropes,
                animate_mines,
                animate_kill_line,
                follow_camera,
                update_hud,
            )
                .chain()
                .run_if(resource_exists::<ActiveRun<LanternRun>>),
        )
        .add_systems(
            FixedUpdate,
            react
                .in_set(MinigameSet::React)
                .run_if(resource_exists::<ActiveRun<LanternRun>>),
        );
    }
}

const SCOPE: DespawnOnExit<AppState> = DespawnOnExit(AppState::Minigame);
/// Obstacles' glowing edges: a pale cream, unlike any seat's colour.
const HAZARD: LinearRgba = LinearRgba::rgb(1.0, 0.86, 0.68);
const DANGER: LinearRgba = LinearRgba::rgb(1.0, 0.12, 0.08);
const PUSH: LinearRgba = LinearRgba::rgb(0.35, 0.55, 1.0);
const TEAM_COLORS: [LinearRgba; 2] = [
    LinearRgba::rgb(1.0, 0.78, 0.3),
    LinearRgba::rgb(0.55, 0.45, 1.0),
];
/// Walls are built in pieces this long, this far ahead of the camera.
const SEGMENT: f32 = 6.0;
const BUILD_AHEAD: f32 = 110.0;
const WALL_LANTERN_EVERY: i32 = 5;
const GHOSTS: usize = 6;
const BEADS: usize = 12;
/// The camera sits a little above the middle, looking this far down the course.
const LOOK_AHEAD: f32 = 26.0;
const CAMERA_LIFT: f32 = 0.35;
/// A narrow lens from further back, so gliders at the kill line and the leader well ahead of
/// it are drawn at nearly the same size.
const FOV_DEGREES: f32 = 26.0;

/// Sounds and sparks, once per event.
fn react(
    mut commands: Commands,
    active: Res<ActiveRun<LanternRun>>,
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
        let (was, is) = (before.ship(seat), now.ship(seat));
        let variant = now.tick + seat.0 as u32;
        let at = world(is.body.pos, is.z);
        if is.crashes > was.crashes {
            sfx.play_varied(Sound::Hit, variant);
            spark(at, hdr(HAZARD, 6.0));
            if is.snagged_at != was.snagged_at {
                sfx.play_varied(Sound::Rebound, variant);
            }
        }
        if is.throttle && !was.throttle {
            sfx.play_varied(Sound::Dash, variant);
            spark(at, hdr(seat_color(seat), 3.0));
        }
        if was.hook.is_none() && is.hook.is_some() {
            sfx.play_varied(Sound::Brace, variant);
        }
        if is.grazing && now.tick % 6 == seat.0 as u32 {
            spark(at, hdr(LinearRgba::WHITE, 2.5));
        }
        if was.flying() && matches!(is.status, Status::Out { .. }) {
            sfx.play_varied(Sound::Fall, variant);
            spark(at, hdr(seat_color(seat), 4.0));
        }
        if is.cut_at != was.cut_at {
            sfx.play_varied(Sound::Rebound, variant);
        }
        if is.stun > was.stun && is.crashes == was.crashes {
            // Rammed aside by a glider flat out.
            sfx.play_varied(Sound::Hit, variant + 3);
            spark(at, hdr(LinearRgba::WHITE, 4.0));
        }
    }
    if now.mines.len() > before.mines.len() {
        sfx.play(Sound::Step);
    }
    if now.phase == Phase::Playing {
        let entered = |game: &State| game.camera_z >= CAVE_Z;
        if !entered(before) && entered(now) {
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
            let ship = now.ship(seat);
            if ship.flying() {
                spark(
                    world(ship.body.pos, ship.z) + Vec3::Y * 0.8,
                    hdr(seat_color(seat), 6.0),
                );
            }
        }
    }
}

/// Simulation coordinates (across, along) to world space.
fn world(across: SimVec2, z: f32) -> Vec3 {
    Vec3::new(across.x, across.y, -z)
}

fn blend(fixed: &Time<Fixed>) -> f32 {
    fixed.overstep_fraction().clamp(0.0, 1.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[derive(Component)]
struct ShipView {
    seat: Seat,
    color: LinearRgba,
    material: Handle<StandardMaterial>,
    light: Entity,
    ghosts: [(Entity, Handle<StandardMaterial>); GHOSTS],
    /// Where it was drawn in recent frames, newest first.
    history: VecDeque<Vec3>,
}

#[derive(Component)]
struct RopeBead {
    seat: Seat,
    k: usize,
}

#[derive(Component)]
struct MineView {
    /// The mine's place along the course, which tells mines apart.
    z: f32,
    material: Handle<StandardMaterial>,
}

/// The kill line's frame round the course, and its four edges.
#[derive(Component)]
struct KillLine {
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct KillEdge(usize);

#[derive(Component)]
struct ChaseCamera {
    /// How far behind the kill line it rides, eased towards what the course's width needs.
    back: f32,
}

/// A wall piece or obstacle, dropped once it's behind the camera (an obstacle, once it's behind
/// the kill line: nobody flying can hit it, and it would only block the view).
#[derive(Component)]
struct CoursePiece {
    end: f32,
    obstacle: bool,
}

/// An obstacle's own stone, which fades as it nears the kill line so it doesn't hide the
/// gliders behind it.
#[derive(Component)]
struct Fade {
    z: f32,
    material: Handle<StandardMaterial>,
}

/// Obstacles fade over this distance in front of the kill line, to this opacity.
const FADE_DISTANCE: f32 = 8.0;
const FADE_MIN: f32 = 0.2;

/// How far the course has been built, for which run.
#[derive(Resource)]
struct Built {
    generation: u32,
    walls_to: f32,
    obstacles_to: usize,
    rock: Handle<StandardMaterial>,
    hazard: Handle<StandardMaterial>,
    wall: Handle<StandardMaterial>,
    floor: Handle<StandardMaterial>,
    cube: Handle<Mesh>,
    ball: Handle<Mesh>,
}

#[derive(Component)]
enum HudText {
    Banner,
    SubBanner,
    SeatPanel(Seat),
}

/// A bar in a seat's panel: its throttle's meter, or how much room it has before the kill line.
#[derive(Component)]
enum PanelBar {
    Meter(Seat),
    Room(Seat),
}

fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    active: Res<ActiveRun<LanternRun>>,
) {
    commands.insert_resource(ClearColor(NIGHT_SKY));
    commands.insert_resource(GlobalAmbientLight {
        color: MOONLIGHT,
        brightness: 160.0,
        ..default()
    });
    let game = &active.run.game;
    commands.spawn((
        SCOPE,
        night_camera(camera_transform(game.camera_z, 20.0), (55.0, 120.0)),
        Projection::Perspective(PerspectiveProjection {
            fov: FOV_DEGREES.to_radians(),
            ..default()
        }),
        ChaseCamera { back: 20.0 },
    ));
    commands.spawn((
        SCOPE,
        DirectionalLight {
            color: MOONLIGHT,
            illuminance: 900.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(-3.0, 10.0, 4.0).looking_at(Vec3::new(0.0, 0.0, -10.0), Vec3::Y),
    ));
    // Obstacles glow faintly from inside, so their shapes read against the dark.
    let rock = materials.add(StandardMaterial {
        base_color: Color::srgb(0.32, 0.3, 0.33),
        emissive: HAZARD * 0.18,
        perceptual_roughness: 0.85,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    commands.insert_resource(Built {
        generation: u32::MAX,
        walls_to: 0.0,
        obstacles_to: 0,
        rock,
        hazard: materials.add(emissive_material(HAZARD, 3.0)),
        wall: materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.19, 0.22),
            perceptual_roughness: 0.95,
            ..default()
        }),
        floor: materials.add(StandardMaterial {
            base_color: Color::srgb(0.1, 0.12, 0.13),
            perceptual_roughness: 0.9,
            ..default()
        }),
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        ball: meshes.add(Sphere::new(1.0).mesh().ico(2).unwrap()),
    });

    for seat in game.playing() {
        spawn_ship(&mut commands, &mut meshes, &mut materials, game, seat);
    }

    let line = materials.add(emissive_material(PUSH, 0.8));
    commands
        .spawn((
            SCOPE,
            KillLine {
                material: line.clone(),
            },
            Transform::default(),
            Visibility::default(),
        ))
        .with_children(|frame| {
            for edge in 0..4 {
                frame.spawn((
                    KillEdge(edge),
                    Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
                    MeshMaterial3d(line.clone()),
                    Transform::default(),
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

fn spawn_ship(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    game: &State,
    seat: Seat,
) {
    let ship = game.ship(seat);
    let color = seat_color(seat);
    let material = materials.add(emissive_material(color, 2.5));
    let dark = materials.add(StandardMaterial {
        base_color: Color::srgb(0.07, 0.06, 0.06),
        perceptual_roughness: 0.7,
        ..default()
    });
    let wing_edge = match team_index(&game.format, seat) {
        Some(team) => TEAM_COLORS[team as usize],
        None => color,
    };
    let edge = materials.add(emissive_material(wing_edge, 4.0));
    let light = commands
        .spawn((
            PointLight {
                color: Color::from(color),
                intensity: 90_000.0,
                range: 7.0,
                ..default()
            },
            Transform::default(),
        ))
        .id();
    let ghost_mesh = meshes.add(Sphere::new(0.16).mesh().ico(1).unwrap());
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
                Transform::from_translation(world(ship.body.pos, ship.z)),
                Visibility::Hidden,
            ))
            .id();
        (ghost, ghost_material)
    });
    let r = ship.body.radius;
    let marker = match seat.0 {
        0 => meshes.add(Sphere::new(0.12)),
        1 => meshes.add(Cone::new(0.13, 0.25)),
        2 => meshes.add(Cuboid::new(0.2, 0.2, 0.2)),
        _ => meshes.add(Cuboid::new(0.17, 0.17, 0.17)),
    };
    let marker_rotation = if seat.0 == 3 {
        Quat::from_rotation_z(TAU / 8.0)
    } else {
        Quat::IDENTITY
    };
    let mut root = commands.spawn((
        SCOPE,
        Transform::from_translation(world(ship.body.pos, ship.z)),
        Visibility::default(),
    ));
    root.add_child(light);
    if ship.barge {
        // A long dark hull with a row of lanterns along its deck.
        root.with_child((
            Mesh3d(meshes.add(Cuboid::new(r * 2.0, r * 0.8, r * 2.6))),
            MeshMaterial3d(dark.clone()),
        ));
        root.with_child((
            Mesh3d(meshes.add(Cuboid::new(r * 2.1, r * 0.12, r * 2.7))),
            MeshMaterial3d(edge.clone()),
            Transform::from_xyz(0.0, r * 0.4, 0.0),
        ));
        for k in 0..3 {
            root.with_child((
                Mesh3d(meshes.add(Cuboid::new(r * 0.45, r * 0.55, r * 0.45))),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(0.0, r * 0.75, (k as f32 - 1.0) * r * 0.8),
            ));
        }
    } else {
        // A paper lantern slung under a pair of wings.
        root.with_child((
            Mesh3d(meshes.add(Cuboid::new(r * 0.95, r * 1.05, r * 1.25))),
            MeshMaterial3d(material.clone()),
        ));
        root.with_child((
            Mesh3d(meshes.add(Cuboid::new(r * 2.2, r * 0.08, r * 0.8))),
            MeshMaterial3d(dark.clone()),
            Transform::from_xyz(0.0, r * 0.55, 0.1),
        ));
        for side in [-1.0, 1.0] {
            root.with_child((
                Mesh3d(meshes.add(Cuboid::new(r * 0.12, r * 0.1, r * 0.8))),
                MeshMaterial3d(edge.clone()),
                Transform::from_xyz(side * r * 1.1, r * 0.55, 0.1),
            ));
        }
    }
    root.with_child((
        Mesh3d(marker),
        MeshMaterial3d(edge.clone()),
        Transform::from_xyz(0.0, r * 1.7, 0.0).with_rotation(marker_rotation),
    ));
    root.insert(ShipView {
        seat,
        color,
        material,
        light,
        ghosts,
        history: VecDeque::new(),
    });

    let rope = materials.add(emissive_material(color, 1.5));
    let bead = meshes.add(Sphere::new(0.05).mesh().ico(1).unwrap());
    for k in 0..BEADS {
        commands.spawn((
            SCOPE,
            RopeBead { seat, k },
            Mesh3d(bead.clone()),
            MeshMaterial3d(rope.clone()),
            Transform::default(),
            Visibility::Hidden,
        ));
    }
}

/// A flat plate filling the course's cross-section at `half`, but for a round hole: what a gate
/// looks like from the front. Built round the hole, out to the edge of the rectangle.
fn gate_mesh(half: SimVec2, center: SimVec2, radius: f32) -> Mesh {
    const SIDES: usize = 40;
    let to_edge = |dir: Vec2| {
        // How far from the hole's centre a ray in `dir` meets the rectangle.
        let tx = if dir.x > 1e-6 {
            (half.x - center.x) / dir.x
        } else if dir.x < -1e-6 {
            (-half.x - center.x) / dir.x
        } else {
            f32::MAX
        };
        let ty = if dir.y > 1e-6 {
            (half.y - center.y) / dir.y
        } else if dir.y < -1e-6 {
            (-half.y - center.y) / dir.y
        } else {
            f32::MAX
        };
        Vec2::new(center.x, center.y) + dir * tx.min(ty)
    };
    let corners = [
        Vec2::new(half.x, half.y),
        Vec2::new(-half.x, half.y),
        Vec2::new(-half.x, -half.y),
        Vec2::new(half.x, -half.y),
    ];
    let c = Vec2::new(center.x, center.y);
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for i in 0..SIDES {
        let a0 = TAU * i as f32 / SIDES as f32;
        let a1 = TAU * (i + 1) as f32 / SIDES as f32;
        let (d0, d1) = (Vec2::from_angle(a0), Vec2::from_angle(a1));
        let (h0, h1) = (c + d0 * radius, c + d1 * radius);
        let (e0, e1) = (to_edge(d0), to_edge(d1));
        let base = positions.len() as u32;
        for p in [h0, e0, e1, h1] {
            positions.push([p.x, p.y, 0.0]);
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        // A corner of the rectangle between the two rays: fill it in.
        for corner in corners {
            let angle = (corner - c).to_angle().rem_euclid(TAU);
            if angle > a0 && angle <= a1 {
                let base = positions.len() as u32;
                for p in [e0, corner, e1] {
                    positions.push([p.x, p.y, 0.0]);
                }
                indices.extend([base, base + 1, base + 2]);
            }
        }
    }
    let normals = vec![[0.0, 0.0, 1.0]; positions.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_indices(Indices::U32(indices))
}

fn spawn_obstacle(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    built: &Built,
    obstacle: &Obstacle,
) {
    let stone = materials.add(StandardMaterial {
        alpha_mode: AlphaMode::Blend,
        ..materials
            .get(&built.rock)
            .cloned()
            .expect("the stone material exists")
    });
    let fade = || Fade {
        z: obstacle.z,
        material: stone.clone(),
    };
    let half = half_extents(obstacle.z);
    let front = -obstacle.z;
    let middle = -(obstacle.z + obstacle.depth * 0.5);
    let piece = CoursePiece {
        end: obstacle.z + obstacle.depth,
        obstacle: true,
    };
    let strip = 0.07;
    match obstacle.shape {
        Shape::Rock { center, radius } => {
            commands
                .spawn((
                    SCOPE,
                    piece,
                    fade(),
                    Transform::from_xyz(center.x, center.y, middle),
                    Visibility::default(),
                ))
                .with_children(|rock| {
                    rock.spawn((
                        Mesh3d(built.ball.clone()),
                        MeshMaterial3d(stone.clone()),
                        Transform::from_scale(Vec3::new(
                            radius,
                            radius,
                            radius.max(obstacle.depth * 0.5),
                        )),
                    ));
                });
        }
        Shape::Pillar { x, half_width } => {
            commands
                .spawn((
                    SCOPE,
                    piece,
                    fade(),
                    Transform::from_xyz(x, 0.0, middle),
                    Visibility::default(),
                ))
                .with_children(|pillar| {
                    pillar.spawn((
                        Mesh3d(built.cube.clone()),
                        MeshMaterial3d(stone.clone()),
                        Transform::from_scale(Vec3::new(
                            half_width * 2.0,
                            half.y * 2.0 + 2.0,
                            obstacle.depth,
                        )),
                    ));
                    for side in [-1.0, 1.0] {
                        pillar.spawn((
                            Mesh3d(built.cube.clone()),
                            MeshMaterial3d(built.hazard.clone()),
                            Transform::from_xyz(side * half_width, 0.0, obstacle.depth * 0.5)
                                .with_scale(Vec3::new(strip, half.y * 2.0 + 2.0, strip)),
                        ));
                    }
                });
        }
        Shape::Beam { y, half_height } => {
            commands
                .spawn((
                    SCOPE,
                    piece,
                    fade(),
                    Transform::from_xyz(0.0, y, middle),
                    Visibility::default(),
                ))
                .with_children(|beam| {
                    beam.spawn((
                        Mesh3d(built.cube.clone()),
                        MeshMaterial3d(stone.clone()),
                        Transform::from_scale(Vec3::new(
                            half.x * 2.0 + 2.0,
                            half_height * 2.0,
                            obstacle.depth,
                        )),
                    ));
                    for side in [-1.0, 1.0] {
                        beam.spawn((
                            Mesh3d(built.cube.clone()),
                            MeshMaterial3d(built.hazard.clone()),
                            Transform::from_xyz(0.0, side * half_height, obstacle.depth * 0.5)
                                .with_scale(Vec3::new(half.x * 2.0 + 2.0, strip, strip)),
                        ));
                    }
                });
        }
        Shape::Gate { center, radius } => {
            // Slightly larger than the course, so its edges hide in the walls.
            let plate = gate_mesh(half + SimVec2::new(1.0, 1.0), center, radius);
            commands
                .spawn((
                    SCOPE,
                    piece,
                    fade(),
                    Transform::from_xyz(0.0, 0.0, front),
                    Visibility::default(),
                ))
                .with_children(|gate| {
                    gate.spawn((Mesh3d(meshes.add(plate)), MeshMaterial3d(stone.clone())));
                    gate.spawn((
                        Mesh3d(meshes.add(Torus::new(radius, radius + 0.1))),
                        MeshMaterial3d(built.hazard.clone()),
                        Transform::from_xyz(center.x, center.y, 0.02)
                            .with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
                    ));
                });
        }
    }
}

/// One piece of canyon or cave from `z` to `z + SEGMENT`.
fn spawn_segment(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    built: &Built,
    k: i32,
    z: f32,
) {
    let half = half_extents(z + SEGMENT * 0.5);
    let middle = -(z + SEGMENT * 0.5);
    let cave = z >= CAVE_Z;
    let piece = || CoursePiece {
        end: z + SEGMENT,
        obstacle: false,
    };
    let jitter = |salt: u32| noise(k, 0, salt) * 0.6;
    // Walls stand just outside the course: canyon walls rise into the night, cave walls meet a
    // roof.
    let height = if cave { half.y * 2.0 + 1.0 } else { 14.0 };
    let wall_y = if cave { 0.0 } else { -half.y + 7.0 };
    for (side, salt) in [(-1.0, 1), (1.0, 2)] {
        let thickness = 1.0 + jitter(salt);
        commands.spawn((
            SCOPE,
            piece(),
            Mesh3d(built.cube.clone()),
            MeshMaterial3d(built.wall.clone()),
            Transform::from_xyz(side * (half.x + thickness * 0.5), wall_y, middle)
                .with_scale(Vec3::new(thickness, height, SEGMENT + 0.05)),
        ));
    }
    commands.spawn((
        SCOPE,
        piece(),
        Mesh3d(built.cube.clone()),
        MeshMaterial3d(built.floor.clone()),
        Transform::from_xyz(0.0, -half.y - 0.5, middle).with_scale(Vec3::new(
            half.x * 2.0 + 4.0,
            1.0,
            SEGMENT + 0.05,
        )),
    ));
    if cave {
        commands.spawn((
            SCOPE,
            piece(),
            Mesh3d(built.cube.clone()),
            MeshMaterial3d(built.wall.clone()),
            Transform::from_xyz(0.0, half.y + 0.5 + jitter(3) * 0.5, middle).with_scale(Vec3::new(
                half.x * 2.0 + 4.0,
                1.0,
                SEGMENT + 0.05,
            )),
        ));
    }
    // Lanterns hung on the walls light the way, every few pieces, on alternate sides.
    if k % WALL_LANTERN_EVERY == 0 {
        let side = if (k / WALL_LANTERN_EVERY) % 2 == 0 {
            -1.0
        } else {
            1.0
        };
        let colors = [
            LANTERN_WARM,
            LinearRgba::rgb(1.0, 0.3, 0.55),
            LinearRgba::rgb(0.25, 0.75, 1.0),
        ];
        let color = colors[(noise(k, 0, 4) * 2.999) as usize];
        let y = if cave { half.y * 0.6 } else { -half.y + 1.5 };
        commands.spawn((
            SCOPE,
            piece(),
            Mesh3d(built.cube.clone()),
            MeshMaterial3d(materials.add(emissive_material(color, 2.5))),
            Transform::from_xyz(side * (half.x - 0.05), y, middle)
                .with_scale(Vec3::new(0.18, 0.4, 0.3)),
            children![(
                PointLight {
                    color: Color::from(color),
                    intensity: 150_000.0,
                    range: 12.0,
                    ..default()
                },
                Transform::from_xyz(-side * 1.5, 0.0, 0.0),
            )],
        ));
    }
}

/// Builds walls and obstacles ahead of the camera and drops what it has passed; starts over
/// when the run restarts (a new seed means a new course).
fn build_course(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    active: Res<ActiveRun<LanternRun>>,
    built: Option<ResMut<Built>>,
    pieces: Query<(Entity, &CoursePiece)>,
) {
    let Some(mut built) = built else {
        return;
    };
    let game = &active.run.game;
    if built.generation != active.generation {
        for (entity, _) in &pieces {
            commands.entity(entity).despawn();
        }
        built.generation = active.generation;
        built.walls_to = (game.camera_z / SEGMENT).floor() * SEGMENT - SEGMENT * 3.0;
        built.obstacles_to = 0;
    }
    let behind = game.camera_z - 20.0;
    for (entity, piece) in &pieces {
        let gone = if piece.obstacle {
            game.camera_z
        } else {
            behind
        };
        if piece.end < gone {
            commands.entity(entity).despawn();
        }
    }
    let ahead = game.camera_z + BUILD_AHEAD;
    while built.walls_to < ahead {
        let z = built.walls_to;
        spawn_segment(
            &mut commands,
            &mut materials,
            &built,
            (z / SEGMENT).round() as i32,
            z,
        );
        built.walls_to += SEGMENT;
    }
    let obstacles = game.course.obstacles.clone();
    while let Some(obstacle) = obstacles.get(built.obstacles_to) {
        if obstacle.z > ahead {
            break;
        }
        if obstacle.z + obstacle.depth > game.camera_z {
            spawn_obstacle(&mut commands, &mut meshes, &mut materials, &built, obstacle);
        }
        built.obstacles_to += 1;
    }
}

fn fade_obstacles(
    active: Res<ActiveRun<LanternRun>>,
    fades: Query<&Fade>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let camera_z = active.run.game.camera_z;
    for fade in &fades {
        let opacity = ((fade.z - camera_z) / FADE_DISTANCE).clamp(FADE_MIN, 1.0);
        if let Some(mut material) = materials.get_mut(&fade.material) {
            material.base_color.set_alpha(opacity);
            material.emissive = HAZARD * (0.18 * opacity);
        }
    }
}

fn animate_ships(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternRun>>,
    mut ships: Query<(&mut ShipView, &mut Transform, &mut Visibility)>,
    mut ghosts: Query<(&mut Transform, &mut Visibility), Without<ShipView>>,
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
    for (mut view, mut transform, mut visibility) in &mut ships {
        let seat = view.seat;
        let (was, is) = (active.previous.ship(seat), game.ship(seat));
        let pos = world(
            was.body.pos.lerp(is.body.pos, alpha),
            lerp(was.z, is.z, alpha),
        );
        transform.translation = pos;
        // Bank into turns and pitch with climbs; spin while spun out; tumble once out.
        let vel = is.body.vel;
        let mut rotation =
            Quat::from_rotation_z(-vel.x * 0.07) * Quat::from_rotation_x(vel.y * 0.05);
        if is.stun > 0 {
            rotation = Quat::from_rotation_z(now * 18.0) * rotation;
        }
        if let Status::Out { t } = is.status {
            let secs = t as f32 / TICK_HZ as f32;
            rotation = Quat::from_rotation_x(secs * 6.0) * Quat::from_rotation_z(secs * 4.0);
        }
        transform.rotation = rotation;
        // Passing through after a crash, it blinks.
        let blinking =
            is.flying() && is.ghost > 0 && is.ghost < GHOST_TICKS && (is.ghost / 4) % 2 == 1;
        let out_long = matches!(is.status, Status::Out { t } if t > 120);
        *visibility = if blinking || out_long || is.status == Status::Benched {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        let celebrating = winners.contains(&seat) && is.flying();
        let glow = match is.status {
            Status::Flying if celebrating => 6.0 + (now * 8.0).sin() * 2.0,
            Status::Flying if is.throttle => 6.0,
            Status::Flying => 2.5 + is.meter as f32 / METER_MAX as f32 * 1.5,
            _ => 0.6,
        };
        if let Some(mut material) = materials.get_mut(&view.material) {
            material.emissive = view.color * glow;
        }
        if let Ok(mut light) = lights.get_mut(view.light) {
            light.intensity = 40_000.0 * glow;
        }

        // A trail of fading lights behind, longer the faster it goes; white in a slipstream.
        if view.history.front() != Some(&pos) {
            view.history.push_front(pos);
            view.history.truncate(GHOSTS * 2 + 1);
        }
        let fast = ((is.speed - CRUISE * 0.8) / (CRUISE * 0.7)).clamp(0.0, 1.0);
        let tint = if is.drafting {
            view.color.mix(&LinearRgba::WHITE, 0.6)
        } else {
            view.color
        };
        for (k, (ghost, ghost_material)) in view.ghosts.iter().enumerate() {
            let Ok((mut ghost_transform, mut ghost_visibility)) = ghosts.get_mut(*ghost) else {
                continue;
            };
            let sample = view.history.get(k * 2 + 1).copied();
            *ghost_visibility = match sample {
                Some(_) if is.flying() => Visibility::Inherited,
                _ => Visibility::Hidden,
            };
            if let Some(sample) = sample {
                let fade = 1.0 - (k as f32 + 1.0) / (GHOSTS as f32 + 1.0);
                ghost_transform.translation = sample;
                ghost_transform.scale = Vec3::splat(0.5 + fade * 0.8);
                if let Some(mut material) = materials.get_mut(ghost_material) {
                    material.emissive = tint * ((0.4 + fast * 3.0) * fade);
                }
            }
        }
    }
}

/// Grapple ropes: a line of beads from the thrower to the one it caught.
fn animate_ropes(
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternRun>>,
    mut beads: Query<(&RopeBead, &mut Transform, &mut Visibility)>,
) {
    let alpha = blend(&fixed);
    let game = &active.run.game;
    let at = |seat: Seat| {
        let (was, is) = (active.previous.ship(seat), game.ship(seat));
        world(
            was.body.pos.lerp(is.body.pos, alpha),
            lerp(was.z, is.z, alpha),
        )
    };
    for (bead, mut transform, mut visibility) in &mut beads {
        let Some(hook) = game.ship(bead.seat).hook else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *visibility = Visibility::Inherited;
        let (from, to) = (at(bead.seat), at(hook.target));
        let t = (bead.k as f32 + 0.5) / BEADS as f32;
        transform.translation = from.lerp(to, t) - Vec3::Y * (0.25 * 4.0 * t * (1.0 - t));
    }
}

fn animate_mines(
    mut commands: Commands,
    time: Res<Time>,
    active: Res<ActiveRun<LanternRun>>,
    built: Option<Res<Built>>,
    mut mines: Query<(Entity, &MineView, &mut Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(built) = built else {
        return;
    };
    let game = &active.run.game;
    let t = time.elapsed_secs();
    let mut seen = Vec::new();
    for (entity, view, mut transform) in &mut mines {
        if !game.mines.iter().any(|m| m.z == view.z) {
            commands.entity(entity).despawn();
            continue;
        }
        seen.push(view.z);
        transform.rotation = Quat::from_rotation_y(t * 2.0) * Quat::from_rotation_x(t * 1.3);
        if let Some(mut material) = materials.get_mut(&view.material) {
            material.emissive = DANGER * (3.0 + (t * 10.0).sin() * 1.5);
        }
    }
    for mine in &game.mines {
        if seen.contains(&mine.z) {
            continue;
        }
        let Shape::Rock { center, .. } = mine.shape else {
            continue;
        };
        let material = materials.add(emissive_material(DANGER, 3.0));
        commands
            .spawn((
                SCOPE,
                MineView {
                    z: mine.z,
                    material: material.clone(),
                },
                Transform::from_translation(world(center, mine.z + mine.depth * 0.5)),
                Visibility::default(),
            ))
            .with_children(|m| {
                m.spawn((
                    Mesh3d(built.ball.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_scale(Vec3::splat(MINE_RADIUS * 0.75)),
                ));
                // Spikes.
                for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                    m.spawn((
                        Mesh3d(built.cube.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::from_scale(Vec3::splat(0.1) + axis * MINE_RADIUS * 1.9),
                    ));
                }
            });
    }
}

/// The frame round the course where the camera's kill line is: blue in the canyon (it pushes
/// you on), red in the cave (it puts you out).
fn animate_kill_line(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternRun>>,
    mut frame: Query<(&KillLine, &mut Transform), Without<KillEdge>>,
    mut edges: Query<(&KillEdge, &mut Transform), Without<KillLine>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let alpha = blend(&fixed);
    let game = &active.run.game;
    let z = lerp(active.previous.camera_z, game.camera_z, alpha);
    let half = half_extents(z.max(0.0));
    let deadly = z >= CAVE_Z;
    // Brighter the nearer anyone flying is to it.
    let nearest = game
        .flying()
        .map(|s| game.ship(s).z - game.camera_z)
        .fold(f32::MAX, f32::min);
    let warn = (1.0 - nearest / 6.0).clamp(0.0, 1.0);
    for (line, mut transform) in &mut frame {
        transform.translation = Vec3::new(0.0, 0.0, -z);
        if let Some(mut material) = materials.get_mut(&line.material) {
            material.emissive = if deadly {
                DANGER * (1.5 + warn * (4.0 + (time.elapsed_secs() * 12.0).sin() * 2.0))
            } else {
                PUSH * 0.8
            };
        }
    }
    let thick = if deadly { 0.08 } else { 0.04 };
    for (edge, mut transform) in &mut edges {
        *transform = match edge.0 {
            0 | 1 => {
                let side = if edge.0 == 0 { 1.0 } else { -1.0 };
                Transform::from_xyz(0.0, side * half.y, 0.0).with_scale(Vec3::new(
                    half.x * 2.0,
                    thick,
                    thick,
                ))
            }
            _ => {
                let side = if edge.0 == 2 { 1.0 } else { -1.0 };
                Transform::from_xyz(side * half.x, 0.0, 0.0).with_scale(Vec3::new(
                    thick,
                    half.y * 2.0,
                    thick,
                ))
            }
        };
    }
}

fn camera_transform(kill_z: f32, back: f32) -> Transform {
    let half = half_extents(kill_z.max(0.0));
    let eye = Vec3::new(0.0, half.y * CAMERA_LIFT, -kill_z + back);
    Transform::from_translation(eye).looking_at(Vec3::new(0.0, 0.0, -kill_z - LOOK_AHEAD), Vec3::Y)
}

/// Ride behind the kill line, far enough back to see the course across at the line, and shake
/// on crashes nearby.
fn follow_camera(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternRun>>,
    windows: Query<&Window>,
    mut camera: Query<(&mut ChaseCamera, &mut Transform, &Projection)>,
) {
    let alpha = blend(&fixed);
    let game = &active.run.game;
    let z = lerp(active.previous.camera_z, game.camera_z, alpha);
    let aspect = windows
        .iter()
        .next()
        .map(|w| w.width() / w.height().max(1.0))
        .unwrap_or(16.0 / 10.0);
    let half = half_extents(z.max(0.0) + 4.0);
    let shake = game
        .players
        .seats()
        .filter_map(|s| game.ship(s).crashed_at)
        .map(|tick| (0.12 - game.tick.saturating_sub(tick) as f32 / TICK_HZ as f32 * 0.4).max(0.0))
        .fold(0.0, f32::max);
    let t = time.elapsed_secs();
    for (mut rig, mut transform, projection) in &mut camera {
        let Projection::Perspective(perspective) = projection else {
            continue;
        };
        let tan = (perspective.fov / 2.0).tan();
        let needed = (half.y * 1.1 / tan).max(half.x * 1.05 / (tan * aspect));
        let ease = (time.delta_secs() * 2.0).min(1.0);
        rig.back += (needed - rig.back) * ease;
        *transform = camera_transform(z, rig.back);
        transform.translation += Vec3::new((t * 91.0).sin(), (t * 73.0).cos(), 0.0) * shake;
    }
}

fn spawn_hud(mut commands: Commands, active: Res<ActiveRun<LanternRun>>) {
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
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(6.0),
                        ..default()
                    },
                    BorderColor::all(border),
                    BackgroundColor(Color::from(seat_color(seat) * 0.25).with_alpha(0.55)),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        HudText::SeatPanel(seat),
                        Text::new(""),
                        TextFont::from_font_size(18.0),
                        TextColor(Color::WHITE),
                    ));
                    if game.ship(seat).barge {
                        return;
                    }
                    for (caption, bar, color) in [
                        (
                            tr!("lantern-run-meter"),
                            PanelBar::Meter(seat),
                            Color::from(seat_color(seat)),
                        ),
                        (tr!("lantern-run-room"), PanelBar::Room(seat), Color::WHITE),
                    ] {
                        panel
                            .spawn(Node {
                                column_gap: Val::Px(6.0),
                                align_items: AlignItems::Center,
                                ..default()
                            })
                            .with_children(|row| {
                                row.spawn((
                                    Text::new(caption),
                                    TextFont::from_font_size(12.0),
                                    TextColor(Color::srgba(1.0, 1.0, 1.0, 0.75)),
                                    Node {
                                        width: Val::Px(62.0),
                                        ..default()
                                    },
                                ));
                                row.spawn((
                                    Node {
                                        width: Val::Px(100.0),
                                        height: Val::Px(8.0),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.15)),
                                ))
                                .with_child((
                                    bar,
                                    Node {
                                        width: Val::Percent(0.0),
                                        height: Val::Percent(100.0),
                                        ..default()
                                    },
                                    BackgroundColor(color),
                                ));
                            });
                    }
                });
            }
        });
}

fn clock(ticks: u32) -> String {
    let secs = ticks.div_ceil(TICK_HZ);
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// The line under the banner: the clock, and in 1v3 how far ahead the barge is.
fn scoreline(game: &State) -> String {
    let time = clock(game.ticks_left());
    match (game.barge(), game.leader_z()) {
        (Some(barge), Some(chase)) => {
            let gap = (game.ship(barge).z - chase).max(0.0).ceil() as u32;
            format!("{time}   {}", tr!("lantern-run-barge-ahead", gap = gap))
        }
        _ => time,
    }
}

/// Seconds since a tick of play, for one-shot messages.
fn since(game: &State, tick: u32) -> f32 {
    game.tick.saturating_sub(tick) as f32 / TICK_HZ as f32
}

fn banner_text(active: &ActiveRun<LanternRun>) -> (String, String) {
    let run = &active.run;
    let game = &run.game;
    if let Some(banner) = halted_banner(run) {
        return banner;
    }
    match game.phase {
        // The shared countdown (crate::intro) is drawn over this.
        Phase::Intro { .. } => (String::new(), String::new()),
        Phase::Playing => {
            let latest_out = game
                .playing()
                .filter_map(|s| game.ship(s).out_at.map(|t| (t, s)))
                .max();
            let cut = game
                .barge()
                .and_then(|b| game.ship(b).cut_at)
                .filter(|t| since(game, *t) < 1.0);
            // When the kill line reached the cave, roughly: the leader that far ahead of it.
            let in_cave = game.camera_z >= CAVE_Z;
            let title = match latest_out {
                Some((tick, seat)) if since(game, tick) < 1.5 => {
                    tr!("lantern-run-out", player = seat_label(seat))
                }
                _ if cut.is_some() => tr!("lantern-run-cut"),
                _ if in_cave && game.camera_z < CAVE_Z + CRUISE * 2.0 => tr!("lantern-run-cave"),
                _ if game.tick >= CLOSE_IN_AFTER && game.tick < CLOSE_IN_AFTER + 2 * TICK_HZ => {
                    tr!("lantern-run-closing")
                }
                _ if game.tick < 45 => tr!("lantern-run-go"),
                _ => String::new(),
            };
            let sub = if in_cave && game.camera_z < CAVE_Z + CRUISE * 2.0 {
                tr!("lantern-run-cave-sub")
            } else {
                scoreline(game)
            };
            (title, sub)
        }
        Phase::Decided { .. } | Phase::Finished => {
            let outcome = game.outcome.expect("decided games have an outcome");
            let winners: Vec<Seat> = outcome.winners().collect();
            let title = i18n::winners(&winners);
            let prompt = if game.phase == Phase::Finished {
                active.continue_prompt()
            } else if game.barge().is_some_and(|b| winners.contains(&b)) {
                tr!("lantern-run-barge-held")
            } else if game.barge().is_some() {
                tr!("lantern-run-barge-passed")
            } else {
                String::new()
            };
            (title, prompt)
        }
    }
}

fn status_text(game: &State, seat: Seat) -> String {
    let ship = game.ship(seat);
    let hooked_by = game
        .flying()
        .find(|s| game.ship(*s).hook.is_some_and(|h| h.target == seat));
    match ship.status {
        Status::Out { .. } => tr!("lantern-run-is-out"),
        Status::Benched => String::new(),
        Status::Flying => {
            if ship.stun > 0 {
                tr!("lantern-run-spinning")
            } else if let Some(hook) = ship.hook {
                tr!("lantern-run-towing", player = seat_label(hook.target))
            } else if let Some(by) = hooked_by {
                tr!("lantern-run-hooked", player = seat_label(by))
            } else if ship.throttle {
                tr!("lantern-run-throttle")
            } else if ship.drafting {
                tr!("lantern-run-slipstream")
            } else if ship.barge {
                tr!("lantern-run-barge-tag")
            } else {
                String::new()
            }
        }
    }
}

fn update_hud(
    active: Res<ActiveRun<LanternRun>>,
    seats: Res<Seats>,
    mut texts: Query<(&HudText, &mut Text)>,
    time: Res<Time>,
    mut bars: Query<(&PanelBar, &mut Node, &mut BackgroundColor)>,
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
                let status = if active.run.controller_lost(seat) {
                    tr!("controller-lost")
                } else if let (Some(outcome), true) = (game.outcome, game.phase != Phase::Playing) {
                    match outcome.placement[seat.index()] {
                        Some(place) => tr!("place", place = place as u32),
                        None => String::new(),
                    }
                } else {
                    status_text(game, seat)
                };
                format!("{}{who}{role}\n{status}", seat_label(seat))
            }
        };
    }
    let deadly = game.camera_z >= CAVE_Z;
    let pulse = (time.elapsed_secs() * 10.0).sin() * 0.5 + 0.5;
    for (bar, mut node, mut color) in &mut bars {
        match *bar {
            PanelBar::Meter(seat) => {
                let meter = game.ship(seat).meter.min(METER_MAX);
                node.width = Val::Percent(meter as f32 / METER_MAX as f32 * 100.0);
            }
            PanelBar::Room(seat) => {
                let ship = game.ship(seat);
                let room = if ship.flying() {
                    ((ship.z - game.camera_z) / game.window()).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                node.width = Val::Percent(room * 100.0);
                // Red and flashing once the kill line is close behind you in the cave.
                color.0 = if !deadly {
                    Color::from(PUSH)
                } else if room < 0.3 {
                    Color::srgb(1.0, 0.15 + pulse * 0.5, 0.1 + pulse * 0.4)
                } else {
                    Color::srgb(1.0 - room * 0.6, 0.9, 0.5)
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_course_runs_away_from_the_camera() {
        let near = world(SimVec2::new(1.0, 2.0), 10.0);
        let far = world(SimVec2::new(1.0, 2.0), 20.0);
        assert_eq!((near.x, near.y), (1.0, 2.0));
        assert!(far.z < near.z, "further along is further down -Z");
    }

    #[test]
    fn the_camera_can_see_the_course_across_at_the_kill_line() {
        let back = 12.0;
        let camera = camera_transform(100.0, back);
        assert!(camera.translation.z > -100.0);
        assert!(camera.forward().z < -0.9);
    }
}
