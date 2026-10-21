//! Presentation for Tabletop Rally. Reads `ActiveRun<TabletopRally>` only; never writes
//! simulation state. Positions are blended between the last two ticks so motion stays smooth.
//!
//! The camera is the simulation's: an orthographic view tilted towards the table, centred on
//! the simulation's camera and sized so the knockout rectangle (the "screen" a car must stay
//! on) fills the window with a little to spare. Everything outside the rectangle is darkened,
//! so you see a car about to drop out, and the rectangle visibly closes in late in a round.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::asset::RenderAssetUsages;
use bevy::camera::ScalingMode;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use minigame_tabletop_rally::track::Prop;
use minigame_tabletop_rally::{
    Car, Out, Phase, PropKind, RoundEnd, State, TABLE_HALF, TabletopRally, Theme, Track,
    VIEW_HALF_START, WIN_POINTS,
};
use sim_core::physics::Vec2 as SimVec2;
use sim_core::{Format, Seat};

use crate::audio::{Sfx, Sound};
use crate::characters::{Pose, Roster};
use crate::flow::AppState;
use crate::i18n::{self, bot_tag, halted_banner};
use crate::palette::{LANTERN_WARM, MOONLIGHT, NIGHT_SKY, seat_color, seat_label};
use crate::particles::{self, Effects, hdr};
use crate::runner::{ActiveRun, MinigameSet, Seats};
use crate::scenery::{emissive_material, night_camera, noise, spawn_lantern_folk};
use crate::tr;

pub struct TabletopRallyViewPlugin;

impl Plugin for TabletopRallyViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            (spawn_scene, spawn_hud)
                .after(MinigameSet::Start)
                .run_if(resource_exists::<ActiveRun<TabletopRally>>),
        )
        .add_systems(
            Update,
            (
                follow_camera,
                frame_view,
                animate_cars,
                flash_light_bars,
                update_hud,
            )
                .run_if(resource_exists::<ActiveRun<TabletopRally>>),
        )
        .add_systems(
            FixedUpdate,
            react
                .in_set(MinigameSet::React)
                .run_if(resource_exists::<ActiveRun<TabletopRally>>),
        );
    }
}

const SCOPE: DespawnOnExit<AppState> = DespawnOnExit(AppState::Minigame);
/// How steeply the camera looks down at the table.
const PITCH: f32 = 58.0 * PI / 180.0;
const CAMERA_DISTANCE: f32 = 60.0;
/// The window shows this much more than the knockout rectangle across, so its edge is always
/// in view...
const FRAME_MARGIN_X: f32 = 1.05;
/// ...and this much more up and down, leaving bands for the banner above and the panels below.
const FRAME_MARGIN_Y: f32 = 1.24;
const ROAD_Y: f32 = 0.012;
const MARK_Y: f32 = 0.022;
const KERB_WIDTH: f32 = 0.28;
/// Bends tighter than this get red and white kerbs.
const KERB_CURVATURE: f32 = 0.1;
/// The lantern folk drive at this size.
const DRIVER_SCALE: f32 = 0.27;
/// How long a car takes to fall off the table, in seconds.
const FALL_SECS: f32 = 0.7;
const LAMP_HEIGHT: Vec3 = Vec3::new(0.0, 12.0, 2.0);
/// Team colours for the panel borders in team formats.
const TEAM_COLORS: [LinearRgba; 2] = [
    LinearRgba::rgb(1.0, 0.78, 0.3),
    LinearRgba::rgb(0.55, 0.45, 1.0),
];
const OUTSIDE: Color = Color::srgba(0.0, 0.0, 0.02, 0.72);
const FRAME_WARM: Color = Color::srgba(1.0, 0.85, 0.55, 0.55);
const FRAME_CLOSING: Color = Color::srgba(1.0, 0.3, 0.2, 0.85);

fn world(v: SimVec2) -> Vec3 {
    Vec3::new(v.x, 0.0, v.y)
}

fn blend(fixed: &Time<Fixed>) -> f32 {
    fixed.overstep_fraction().clamp(0.0, 1.0)
}

/// `a` to `b` by `t`, or straight to `b` after a jump (a car put back on the grid).
fn smooth(a: SimVec2, b: SimVec2, t: f32) -> SimVec2 {
    if a.distance(b) > 3.0 { b } else { a.lerp(b, t) }
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let d = (b - a + PI).rem_euclid(2.0 * PI) - PI;
    a + d * t
}

/// Turns a kart (built facing +x) to face `heading`, the simulation's angle from +x towards +y.
fn heading_rotation(heading: f32) -> Quat {
    Quat::from_rotation_y(-heading)
}

fn camera_transform(target: SimVec2) -> Transform {
    let target = world(target);
    let offset = Vec3::new(0.0, PITCH.sin(), PITCH.cos()) * CAMERA_DISTANCE;
    Transform::from_translation(target + offset).looking_at(target, Vec3::Y)
}

/// The window's minimum view: the knockout rectangle with a margin, as seen from the tilt.
fn framed_size(half: SimVec2) -> (f32, f32) {
    (
        half.x * 2.0 * FRAME_MARGIN_X,
        half.y * 2.0 * FRAME_MARGIN_Y * PITCH.sin(),
    )
}

/// What the camera shows for a knockout rectangle of `half`: the camera zooms with it.
fn scaling_mode(half: SimVec2) -> ScalingMode {
    let (min_width, min_height) = framed_size(half);
    ScalingMode::AutoMin {
        min_width,
        min_height,
    }
}

fn camera_projection() -> Projection {
    Projection::Orthographic(OrthographicProjection {
        scaling_mode: scaling_mode(VIEW_HALF_START),
        ..OrthographicProjection::default_3d()
    })
}

/// The knockout rectangle's size between the last two ticks.
fn view_half(active: &ActiveRun<TabletopRally>, fixed: &Time<Fixed>) -> SimVec2 {
    active
        .previous
        .view_half()
        .lerp(active.run.game.view_half(), blend(fixed))
}

/// The share of the window's width and height the knockout rectangle `half` covers, for a
/// window of `aspect` (width over height).
fn rectangle_share(half: SimVec2, aspect: f32) -> (f32, f32) {
    let (min_width, min_height) = framed_size(half);
    let (width, height) = if aspect >= min_width / min_height {
        (min_height * aspect, min_height)
    } else {
        (min_width, min_width / aspect)
    };
    let floor_depth = height / PITCH.sin();
    (
        (half.x * 2.0 / width).min(1.0),
        (half.y * 2.0 / floor_depth).min(1.0),
    )
}

#[derive(Component)]
struct CameraRig;

/// The lamp over the table, hanging above wherever the camera looks.
#[derive(Component)]
struct TableLamp;

#[derive(Component)]
struct CarView {
    seat: Seat,
    driver: Entity,
}

/// A chaser's flashing light: which half (0 red, 1 blue).
#[derive(Component)]
struct LightBar {
    half: usize,
    material: Handle<StandardMaterial>,
}

/// The darkened bands round the knockout rectangle, and its outline.
#[derive(Component)]
enum Frame {
    Top,
    Bottom,
    Left,
    Right,
    Outline,
}

#[derive(Component)]
enum HudText {
    Banner,
    SubBanner,
    SeatPanel(Seat),
}

/// Sounds and sparks, once per event: the start, bumps, cars dropping out, the results.
fn react(
    mut commands: Commands,
    active: Res<ActiveRun<TabletopRally>>,
    effects: Res<Effects>,
    sfx: Sfx,
) {
    let Some((before, now)) = active.ticked() else {
        return;
    };
    let mut spark = |pos: Vec3, color: Vec4| {
        particles::burst(&mut commands, &effects, SCOPE, pos, color);
    };
    let started = |phase: Phase| matches!(phase, Phase::Racing { t: 0 });
    if started(now.phase) && !started(before.phase) {
        sfx.play(Sound::Go);
    }
    if now.closing_in() && !before.closing_in() {
        sfx.play(Sound::Rumble);
    }
    for seat in now.playing() {
        let (was, is) = (before.car(seat), now.car(seat));
        let variant = now.tick + seat.0 as u32;
        if is.last_bump != was.last_bump && is.racing() {
            sfx.play_varied(Sound::Hit, variant);
        }
        if was.out.is_none()
            && let Some((why, _)) = is.out
        {
            let at = match why {
                // Spark where it left the screen, so everyone sees who went.
                Out::Screen => clamp_to_view(now, is.body.pos),
                Out::Table | Out::Caught => is.body.pos,
            };
            sfx.play_varied(
                if why == Out::Caught {
                    Sound::Brace
                } else {
                    Sound::Fall
                },
                variant,
            );
            spark(world(at) + Vec3::Y * 0.5, hdr(seat_color(seat), 6.0));
        }
    }
    let scored =
        !matches!(before.phase, Phase::Result { .. }) && matches!(now.phase, Phase::Result { .. });
    if scored && now.outcome.is_none() {
        sfx.play(Sound::RoundWin);
    }
    if before.outcome.is_none()
        && let Some(outcome) = now.outcome
    {
        sfx.play(Sound::Win);
        for seat in outcome.winners() {
            let car = now.car(seat);
            if car.racing() {
                spark(
                    world(car.body.pos) + Vec3::Y * 1.0,
                    hdr(seat_color(seat), 6.0),
                );
            }
        }
    }
}

/// The point of the knockout rectangle nearest `pos`.
fn clamp_to_view(game: &State, pos: SimVec2) -> SimVec2 {
    let half = game.view_half();
    let d = pos - game.camera;
    game.camera + SimVec2::new(d.x.clamp(-half.x, half.x), d.y.clamp(-half.y, half.y))
}

fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    active: Res<ActiveRun<TabletopRally>>,
    roster: Res<Roster>,
) {
    let game = &active.run.game;
    let track = game.track();
    commands.insert_resource(ClearColor(NIGHT_SKY));
    commands.insert_resource(GlobalAmbientLight {
        color: MOONLIGHT,
        brightness: 70.0,
        ..default()
    });
    commands.spawn((
        SCOPE,
        CameraRig,
        night_camera(camera_transform(game.camera), (200.0, 300.0)),
        camera_projection(),
    ));
    commands.spawn((
        SCOPE,
        TableLamp,
        PointLight {
            color: Color::from(LANTERN_WARM),
            intensity: 6_000_000.0,
            range: 60.0,
            radius: 1.0,
            ..default()
        },
        Transform::from_translation(world(game.camera) + LAMP_HEIGHT),
    ));
    commands.spawn((
        SCOPE,
        DirectionalLight {
            color: MOONLIGHT,
            illuminance: 650.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-6.0, 14.0, 9.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    spawn_table(&mut commands, &mut meshes, &mut materials, track.theme);
    spawn_road(&mut commands, &mut meshes, &mut materials, track);
    for prop in &track.props {
        spawn_prop(&mut commands, &mut meshes, &mut materials, prop);
    }
    spawn_decor(&mut commands, &mut meshes, &mut materials, track);
    for seat in game.playing() {
        spawn_kart(
            &mut commands,
            &mut meshes,
            &mut materials,
            game,
            seat,
            roster.of(seat),
        );
    }
}

/// A flat mesh of coloured quads on the floor at `y`: each quad is four corners and a colour.
fn quad_mesh(quads: &[([Vec3; 4], LinearRgba)]) -> Mesh {
    let mut positions = Vec::with_capacity(quads.len() * 4);
    let mut colors = Vec::with_capacity(quads.len() * 4);
    let mut indices = Vec::with_capacity(quads.len() * 6);
    for (corners, color) in quads {
        let base = positions.len() as u32;
        for corner in corners {
            positions.push(corner.to_array());
            colors.push(color.to_f32_array());
        }
        // Wound counter-clockwise seen from above, whichever way the corners run.
        let facing_down = (corners[1] - corners[0])
            .cross(corners[2] - corners[0])
            .y
            .is_sign_negative();
        if facing_down {
            indices.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
        } else {
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

fn flat_material(
    materials: &mut Assets<StandardMaterial>,
    roughness: f32,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: roughness,
        ..default()
    })
}

/// The tablecloth (or the desk's wood), and the table's thickness under it.
fn spawn_table(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    theme: Theme,
) {
    let tile = 2.0;
    let (nx, ny) = (
        (TABLE_HALF.x * 2.0 / tile) as i32,
        (TABLE_HALF.y * 2.0 / tile) as i32,
    );
    let mut quads = Vec::new();
    for i in 0..nx {
        for j in 0..ny {
            let x0 = -TABLE_HALF.x + i as f32 * tile;
            let z0 = -TABLE_HALF.y + j as f32 * tile;
            let color = match theme {
                // Blue gingham on white.
                Theme::Breakfast => match (i % 2, j % 2) {
                    (0, 0) => LinearRgba::rgb(0.08, 0.13, 0.3),
                    (1, 1) => LinearRgba::rgb(0.42, 0.44, 0.48),
                    _ => LinearRgba::rgb(0.2, 0.25, 0.42),
                },
                // Planks along the desk, each its own shade.
                Theme::Desk => {
                    let plank = noise(0, j / 2, 11);
                    let grain = noise(i, j, 12) * 0.04;
                    LinearRgba::rgb(0.26 + plank * 0.08 + grain, 0.14 + plank * 0.04, 0.07)
                }
                // Red gingham.
                Theme::Picnic => match (i % 2, j % 2) {
                    (0, 0) => LinearRgba::rgb(0.42, 0.04, 0.04),
                    (1, 1) => LinearRgba::rgb(0.45, 0.42, 0.4),
                    _ => LinearRgba::rgb(0.42, 0.16, 0.14),
                },
            };
            quads.push((
                [
                    Vec3::new(x0, 0.0, z0),
                    Vec3::new(x0 + tile, 0.0, z0),
                    Vec3::new(x0 + tile, 0.0, z0 + tile),
                    Vec3::new(x0, 0.0, z0 + tile),
                ],
                color,
            ));
        }
    }
    commands.spawn((
        SCOPE,
        Mesh3d(meshes.add(quad_mesh(&quads))),
        MeshMaterial3d(flat_material(materials, 0.85)),
    ));
    // The table's edge: a slab under the cloth.
    commands.spawn((
        SCOPE,
        Mesh3d(meshes.add(Cuboid::new(TABLE_HALF.x * 2.0, 1.2, TABLE_HALF.y * 2.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.12, 0.07, 0.04),
            perceptual_roughness: 0.7,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.61, 0.0),
    ));
}

/// The road: a paper ribbon with a dashed middle line, white edges, red and white kerbs on
/// the bends, and a chequered line at the start.
fn spawn_road(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    track: &Track,
) {
    let n = track.samples();
    let half = track.width / 2.0;
    let edge = |i: usize, side: f32, y: f32| {
        let p = track.centre[i] + track.tangent[i].perp() * side;
        Vec3::new(p.x, y, p.y)
    };
    let mut road = Vec::with_capacity(n);
    let mut marks = Vec::new();
    for i in 0..n {
        let j = (i + 1) % n;
        let shade = 0.36 + noise(i as i32, 0, 21) * 0.02;
        road.push((
            [
                edge(i, -half, ROAD_Y),
                edge(j, -half, ROAD_Y),
                edge(j, half, ROAD_Y),
                edge(i, half, ROAD_Y),
            ],
            LinearRgba::rgb(shade, shade * 0.97, shade * 0.9),
        ));
        // Dashes down the middle: two samples on, two off.
        if i % 4 < 2 {
            marks.push((
                [
                    edge(i, -0.06, MARK_Y),
                    edge(j, -0.06, MARK_Y),
                    edge(j, 0.06, MARK_Y),
                    edge(i, 0.06, MARK_Y),
                ],
                LinearRgba::rgb(0.75, 0.7, 0.45),
            ));
        }
        let bend = track.curvature[i].abs() > KERB_CURVATURE;
        for side in [-1.0, 1.0] {
            let color = if bend && (i / 2) % 2 == 0 {
                LinearRgba::rgb(0.75, 0.06, 0.04)
            } else {
                LinearRgba::rgb(0.8, 0.8, 0.78)
            };
            let width = if bend { KERB_WIDTH } else { 0.1 };
            marks.push((
                [
                    edge(i, side * half, MARK_Y),
                    edge(j, side * half, MARK_Y),
                    edge(j, side * (half - width), MARK_Y),
                    edge(i, side * (half - width), MARK_Y),
                ],
                color,
            ));
        }
    }
    // The start line: two rows of black and white squares across the road.
    let squares = 8;
    let size = track.width / squares as f32;
    let (t, normal) = (track.tangent[0], track.tangent[0].perp());
    for row in 0..2 {
        for k in 0..squares {
            let across = -half + k as f32 * size;
            let along = row as f32 * size;
            let corner = |a: f32, c: f32| {
                let p = track.centre[0] + t * a + normal * c;
                Vec3::new(p.x, MARK_Y + 0.002, p.y)
            };
            let white = (row + k) % 2 == 0;
            marks.push((
                [
                    corner(along, across),
                    corner(along + size, across),
                    corner(along + size, across + size),
                    corner(along, across + size),
                ],
                if white {
                    LinearRgba::rgb(0.85, 0.85, 0.85)
                } else {
                    LinearRgba::rgb(0.02, 0.02, 0.02)
                },
            ));
        }
    }
    let material = flat_material(materials, 0.9);
    commands.spawn((
        SCOPE,
        Mesh3d(meshes.add(quad_mesh(&road))),
        MeshMaterial3d(material.clone()),
    ));
    commands.spawn((
        SCOPE,
        Mesh3d(meshes.add(quad_mesh(&marks))),
        MeshMaterial3d(material),
    ));
}

fn plain(
    materials: &mut Assets<StandardMaterial>,
    r: f32,
    g: f32,
    b: f32,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: Color::srgb(r, g, b),
        perceptual_roughness: 0.6,
        ..default()
    })
}

/// A solid prop, built so its footprint matches the simulation's circle or capsule.
fn spawn_prop(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    prop: &Prop,
) {
    let centre = world(prop.centre());
    let r = prop.radius;
    // A pencil lies along its capsule (built along +x); anything else turns any old way.
    let along = world(prop.b) - world(prop.a);
    let turn = if prop.kind == PropKind::Pencil {
        -along.z.atan2(along.x)
    } else {
        noise((centre.x * 10.0) as i32, (centre.z * 10.0) as i32, 31) * PI
    };
    let root = commands
        .spawn((
            SCOPE,
            Transform::from_translation(centre).with_rotation(Quat::from_rotation_y(turn)),
            Visibility::default(),
        ))
        .id();
    let mut part = |mesh: Mesh, material: Handle<StandardMaterial>, transform: Transform| {
        let child = commands
            .spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material),
                transform,
            ))
            .id();
        commands.entity(root).add_child(child);
    };
    match prop.kind {
        PropKind::Teacup => {
            let china = plain(materials, 0.85, 0.85, 0.8);
            part(
                Cylinder::new(r + 0.35, 0.06).mesh().resolution(40).build(),
                china.clone(),
                Transform::from_xyz(0.0, 0.03, 0.0),
            );
            part(
                Cylinder::new(r, 0.9).mesh().resolution(40).build(),
                china.clone(),
                Transform::from_xyz(0.0, 0.45, 0.0),
            );
            part(
                Cylinder::new(r * 0.86, 0.02).mesh().resolution(40).build(),
                plain(materials, 0.3, 0.14, 0.05),
                Transform::from_xyz(0.0, 0.8, 0.0),
            );
            part(
                Cylinder::new(r + 0.01, 0.12).mesh().resolution(40).build(),
                plain(materials, 0.2, 0.35, 0.7),
                Transform::from_xyz(0.0, 0.7, 0.0),
            );
            part(
                Torus::new(0.12, 0.3).into(),
                china,
                Transform::from_xyz(r + 0.15, 0.5, 0.0)
                    .with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
            );
        }
        PropKind::Spool => {
            let wood = plain(materials, 0.55, 0.38, 0.2);
            part(
                Cylinder::new(r * 0.82, 0.7).mesh().resolution(28).build(),
                plain(materials, 0.15, 0.5, 0.35),
                Transform::from_xyz(0.0, 0.45, 0.0),
            );
            for y in [0.06, 0.84] {
                part(
                    Cylinder::new(r, 0.12).mesh().resolution(28).build(),
                    wood.clone(),
                    Transform::from_xyz(0.0, y, 0.0),
                );
            }
        }
        PropKind::SugarCube => {
            part(
                Cuboid::new(r * 1.4, r * 1.4, r * 1.4).into(),
                plain(materials, 0.92, 0.92, 0.88),
                Transform::from_xyz(0.0, r * 0.7, 0.0),
            );
        }
        PropKind::Apple => {
            part(
                Sphere::new(r).mesh().uv(24, 16),
                plain(materials, 0.6, 0.06, 0.04),
                Transform::from_xyz(0.0, r * 0.9, 0.0),
            );
            part(
                Cylinder::new(0.05, 0.35).into(),
                plain(materials, 0.25, 0.15, 0.05),
                Transform::from_xyz(0.0, r * 1.9, 0.0),
            );
        }
        PropKind::Eraser => {
            part(
                Cuboid::new(r * 1.9, 0.35, r * 1.4).into(),
                plain(materials, 0.9, 0.45, 0.5),
                Transform::from_xyz(0.0, 0.18, 0.0),
            );
        }
        PropKind::Pencil => {
            let length = along.length();
            let lie = Quat::from_rotation_z(FRAC_PI_2);
            let body = length * 0.8;
            part(
                Cylinder::new(r, body).mesh().resolution(6).build(),
                plain(materials, 0.85, 0.65, 0.1),
                Transform::from_xyz(-length * 0.1, r, 0.0).with_rotation(lie),
            );
            part(
                Cone::new(r, length * 0.12).into(),
                plain(materials, 0.8, 0.65, 0.45),
                Transform::from_xyz(length * 0.36, r, 0.0)
                    .with_rotation(Quat::from_rotation_z(-FRAC_PI_2)),
            );
            part(
                Cylinder::new(r, length * 0.08).into(),
                plain(materials, 0.85, 0.45, 0.5),
                Transform::from_xyz(-length * 0.46, r, 0.0).with_rotation(lie),
            );
        }
    }
}

/// Things on the table well away from the road, for the look only: plates, bowls and toast at
/// breakfast; books and paper on the desk; plates, napkins and grapes at the picnic.
fn spawn_decor(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    track: &Track,
) {
    const CELL: f32 = 7.0;
    let salt = track.theme as u32 * 100;
    let (nx, ny) = (
        (TABLE_HALF.x * 2.0 / CELL) as i32,
        (TABLE_HALF.y * 2.0 / CELL) as i32,
    );
    for i in 0..nx {
        for j in 0..ny {
            if noise(i, j, salt + 1) < 0.35 {
                continue;
            }
            let x = -TABLE_HALF.x + (i as f32 + 0.25 + noise(i, j, salt + 2) * 0.5) * CELL;
            let z = -TABLE_HALF.y + (j as f32 + 0.25 + noise(i, j, salt + 3) * 0.5) * CELL;
            let at = SimVec2::new(x, z);
            let size = 1.8;
            let road = track.centre[track.nearest_anywhere(at)].distance(at);
            let props = track
                .props
                .iter()
                .map(|p| p.closest(at).distance(at) - p.radius)
                .fold(f32::MAX, f32::min);
            let inside = x.abs() < TABLE_HALF.x - size && z.abs() < TABLE_HALF.y - size;
            if road < track.width / 2.0 + size + 0.6 || props < size + 0.3 || !inside {
                continue;
            }
            let kind = (noise(i, j, salt + 4) * 3.0) as u32;
            let turn = Quat::from_rotation_y(noise(i, j, salt + 5) * PI);
            let mut put = |mesh: Mesh, material: Handle<StandardMaterial>, offset: Vec3| {
                commands.spawn((
                    SCOPE,
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(material),
                    Transform::from_translation(Vec3::new(x, 0.0, z) + turn * offset)
                        .with_rotation(turn),
                ));
            };
            match (track.theme, kind) {
                (Theme::Breakfast | Theme::Picnic, 0) => {
                    put(
                        Cylinder::new(1.7, 0.08).mesh().resolution(40).build(),
                        plain(materials, 0.8, 0.8, 0.76),
                        Vec3::Y * 0.04,
                    );
                    put(
                        Cylinder::new(1.2, 0.1).mesh().resolution(40).build(),
                        plain(materials, 0.7, 0.7, 0.68),
                        Vec3::Y * 0.06,
                    );
                }
                (Theme::Breakfast, 1) => {
                    put(
                        Cylinder::new(1.2, 0.7).mesh().resolution(36).build(),
                        plain(materials, 0.75, 0.55, 0.3),
                        Vec3::Y * 0.35,
                    );
                    put(
                        Cylinder::new(1.05, 0.02).mesh().resolution(36).build(),
                        plain(materials, 0.85, 0.65, 0.25),
                        Vec3::Y * 0.68,
                    );
                }
                (Theme::Breakfast, _) => {
                    put(
                        Cuboid::new(1.5, 0.18, 1.4).into(),
                        plain(materials, 0.7, 0.45, 0.2),
                        Vec3::Y * 0.09,
                    );
                }
                (Theme::Desk, 0 | 1) => {
                    let hue = noise(i, j, salt + 6);
                    put(
                        Cuboid::new(3.2, 0.5, 2.3).into(),
                        plain(materials, 0.2 + hue * 0.4, 0.12, 0.3 - hue * 0.2),
                        Vec3::Y * 0.25,
                    );
                    put(
                        Cuboid::new(3.0, 0.42, 2.2).into(),
                        plain(materials, 0.8, 0.78, 0.7),
                        Vec3::new(0.12, 0.25, 0.0),
                    );
                }
                (Theme::Desk, _) => {
                    put(
                        Cuboid::new(2.1, 0.02, 2.9).into(),
                        plain(materials, 0.75, 0.75, 0.72),
                        Vec3::Y * 0.01,
                    );
                }
                (Theme::Picnic, 1) => {
                    put(
                        Cuboid::new(2.2, 0.03, 2.2).into(),
                        plain(materials, 0.85, 0.85, 0.8),
                        Vec3::Y * 0.015,
                    );
                }
                (Theme::Picnic, _) => {
                    let grape = plain(materials, 0.3, 0.08, 0.35);
                    for k in 0..9 {
                        let a = k as f32 * 2.4;
                        let d = 0.15 + 0.08 * k as f32;
                        put(
                            Sphere::new(0.28).mesh().uv(12, 8),
                            grape.clone(),
                            Vec3::new(a.cos() * d, 0.26 + (k % 3) as f32 * 0.05, a.sin() * d),
                        );
                    }
                }
            }
        }
    }
}

/// A toy kart in the seat's colour with its lantern-folk driver; chasers in 1v3 carry a light
/// bar. Built facing +x.
fn spawn_kart(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    game: &State,
    seat: Seat,
    character: crate::characters::Character,
) {
    let car = game.car(seat);
    let color = seat_color(seat);
    let paint = materials.add(StandardMaterial {
        base_color: Color::from(color * 0.55),
        emissive: color * 0.35,
        perceptual_roughness: 0.35,
        ..default()
    });
    let dark = plain(materials, 0.04, 0.04, 0.05);
    let lamp = materials.add(emissive_material(LANTERN_WARM, 8.0));
    let tail = materials.add(emissive_material(LinearRgba::rgb(1.0, 0.05, 0.02), 4.0));
    let driver = spawn_lantern_folk(
        commands,
        meshes,
        materials,
        AppState::Minigame,
        seat,
        character,
        Transform::from_xyz(-0.08, 0.16, 0.0)
            .with_rotation(Quat::from_rotation_y(FRAC_PI_2))
            .with_scale(Vec3::splat(DRIVER_SCALE)),
    )
    .root;
    let root = commands
        .spawn((
            SCOPE,
            CarView { seat, driver },
            Transform::from_translation(world(car.body.pos))
                .with_rotation(heading_rotation(car.heading)),
            Visibility::default(),
        ))
        .add_child(driver)
        .id();
    let mut part = |mesh: Mesh, material: Handle<StandardMaterial>, transform: Transform| {
        let child = commands
            .spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material),
                transform,
            ))
            .id();
        commands.entity(root).add_child(child);
    };
    part(
        Cuboid::new(0.95, 0.14, 0.52).into(),
        paint.clone(),
        Transform::from_xyz(0.0, 0.15, 0.0),
    );
    part(
        Cuboid::new(0.3, 0.12, 0.46).into(),
        paint.clone(),
        Transform::from_xyz(0.3, 0.24, 0.0),
    );
    part(
        Cuboid::new(0.08, 0.3, 0.42).into(),
        paint,
        Transform::from_xyz(-0.36, 0.32, 0.0),
    );
    for (x, z) in [(0.32, 0.29), (0.32, -0.29), (-0.3, 0.3), (-0.3, -0.3)] {
        part(
            Cylinder::new(0.13, 0.11).mesh().resolution(16).build(),
            dark.clone(),
            Transform::from_xyz(x, 0.13, z).with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
        );
    }
    for z in [0.16, -0.16] {
        part(
            Sphere::new(0.05).into(),
            lamp.clone(),
            Transform::from_xyz(0.48, 0.19, z),
        );
        part(
            Cuboid::new(0.03, 0.05, 0.1).into(),
            tail.clone(),
            Transform::from_xyz(-0.48, 0.17, z),
        );
    }
    if game.getaway().is_some() && !car.getaway {
        for (half, z) in [(0usize, 0.1), (1, -0.1)] {
            let material = materials.add(emissive_material(light_bar_color(half), 6.0));
            let child = commands
                .spawn((
                    LightBar {
                        half,
                        material: material.clone(),
                    },
                    Mesh3d(meshes.add(Cuboid::new(0.12, 0.07, 0.17))),
                    MeshMaterial3d(material),
                    Transform::from_xyz(-0.36, 0.5, z),
                ))
                .id();
            commands.entity(root).add_child(child);
        }
    }
    let headlight = commands
        .spawn((
            SpotLight {
                color: Color::from(LANTERN_WARM),
                intensity: 60_000.0,
                range: 7.0,
                outer_angle: 0.6,
                inner_angle: 0.3,
                ..default()
            },
            Transform::from_xyz(0.5, 0.3, 0.0).looking_at(Vec3::new(4.0, -0.4, 0.0), Vec3::Y),
        ))
        .id();
    commands.entity(root).add_child(headlight);
}

fn light_bar_color(half: usize) -> LinearRgba {
    if half == 0 {
        LinearRgba::rgb(1.0, 0.08, 0.05)
    } else {
        LinearRgba::rgb(0.1, 0.25, 1.0)
    }
}

fn follow_camera(
    active: Res<ActiveRun<TabletopRally>>,
    fixed: Res<Time<Fixed>>,
    mut camera: Query<(&mut Transform, &mut Projection), With<CameraRig>>,
    mut lamp: Query<&mut Transform, (With<TableLamp>, Without<CameraRig>)>,
) {
    let at = smooth(
        active.previous.camera,
        active.run.game.camera,
        blend(&fixed),
    );
    let mode = scaling_mode(view_half(&active, &fixed));
    for (mut transform, mut projection) in &mut camera {
        *transform = camera_transform(at);
        if let Projection::Orthographic(ortho) = projection.as_mut() {
            ortho.scaling_mode = mode;
        }
    }
    for mut transform in &mut lamp {
        transform.translation = world(at) + LAMP_HEIGHT;
    }
}

/// Darken everything outside the knockout rectangle and outline it.
fn frame_view(
    active: Res<ActiveRun<TabletopRally>>,
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    windows: Query<&Window>,
    mut frames: Query<(&Frame, &mut Node, Option<&mut BorderColor>)>,
) {
    let Some(window) = windows.iter().next() else {
        return;
    };
    let aspect = window.width() / window.height().max(1.0);
    let game = &active.run.game;
    let half = active
        .previous
        .view_half()
        .lerp(game.view_half(), blend(&fixed));
    let (w, h) = rectangle_share(half, aspect);
    let (side, band) = ((1.0 - w) * 50.0, (1.0 - h) * 50.0);
    let closing = game.closing_in() && game.outcome.is_none();
    for (frame, mut node, border) in &mut frames {
        match frame {
            Frame::Top => node.height = Val::Percent(band),
            Frame::Bottom => node.height = Val::Percent(band),
            Frame::Left | Frame::Right => {
                node.width = Val::Percent(side);
                node.top = Val::Percent(band);
                node.height = Val::Percent(100.0 - 2.0 * band);
            }
            Frame::Outline => {
                node.left = Val::Percent(side);
                node.top = Val::Percent(band);
                node.width = Val::Percent(100.0 - 2.0 * side);
                node.height = Val::Percent(100.0 - 2.0 * band);
                if let Some(mut border) = border {
                    let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 8.0).sin();
                    *border = BorderColor::all(if closing {
                        FRAME_CLOSING.with_alpha(0.4 + 0.5 * pulse)
                    } else {
                        FRAME_WARM
                    });
                }
            }
        }
    }
}

fn animate_cars(
    active: Res<ActiveRun<TabletopRally>>,
    fixed: Res<Time<Fixed>>,
    mut cars: Query<(&CarView, &mut Transform, &mut Visibility)>,
    mut poses: Query<&mut Pose>,
) {
    let game = &active.run.game;
    let t = blend(&fixed);
    for (view, mut transform, mut visibility) in &mut cars {
        let (was, is): (&Car, &Car) = (active.previous.car(view.seat), game.car(view.seat));
        let pos = smooth(was.body.pos, is.body.pos, t);
        let mut at = world(pos);
        let mut shown = true;
        if let Some((why, tick)) = is.out {
            let secs = (game.tick - tick) as f32 / sim_core::TICK_HZ as f32 + t / 60.0;
            match why {
                // Over the edge and down into the dark.
                Out::Table => {
                    at.y -= 9.0 * secs * secs;
                    shown = secs < FALL_SECS;
                }
                Out::Screen => shown = false,
                // The caught getaway car sits where it was caught until the next round.
                Out::Caught => {}
            }
        }
        transform.translation = at;
        transform.rotation = heading_rotation(lerp_angle(was.heading, is.heading, t));
        *visibility = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let pose = match (game.phase, game.outcome) {
            (Phase::Result { .. } | Phase::Finished, Some(outcome)) => {
                match outcome.placement[view.seat.index()] {
                    Some(1) => Pose::Celebrate,
                    _ => Pose::Slump,
                }
            }
            (Phase::Result { .. }, None) => {
                if game.round_winner.is_some() && game.format.team(view.seat) == game.round_winner {
                    Pose::Celebrate
                } else {
                    Pose::Idle
                }
            }
            _ => Pose::Idle,
        };
        if let Ok(mut current) = poses.get_mut(view.driver)
            && *current != pose
        {
            *current = pose;
        }
    }
}

fn flash_light_bars(
    time: Res<Time>,
    bars: Query<&LightBar>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let on = (time.elapsed_secs() * 4.0) as usize % 2;
    for bar in &bars {
        if let Some(mut material) = materials.get_mut(&bar.material) {
            let strength = if bar.half == on { 9.0 } else { 0.6 };
            material.emissive = light_bar_color(bar.half) * strength;
        }
    }
}

fn team_index(format: &Format, seat: Seat) -> Option<u8> {
    match format {
        Format::FreeForAll | Format::Duel { .. } => None,
        _ => format.team(seat),
    }
}

fn spawn_hud(mut commands: Commands, active: Res<ActiveRun<TabletopRally>>) {
    // The darkened outside, under everything else.
    for frame in [Frame::Top, Frame::Bottom, Frame::Left, Frame::Right] {
        let node = match frame {
            Frame::Top => Node {
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                ..default()
            },
            Frame::Bottom => Node {
                bottom: Val::Px(0.0),
                width: Val::Percent(100.0),
                ..default()
            },
            Frame::Left => Node {
                left: Val::Px(0.0),
                ..default()
            },
            _ => Node {
                right: Val::Px(0.0),
                ..default()
            },
        };
        commands.spawn((
            SCOPE,
            frame,
            Node {
                position_type: PositionType::Absolute,
                ..node
            },
            BackgroundColor(OUTSIDE),
        ));
    }
    commands.spawn((
        SCOPE,
        Frame::Outline,
        Node {
            position_type: PositionType::Absolute,
            border: UiRect::all(Val::Px(3.0)),
            ..default()
        },
        BorderColor::all(FRAME_WARM),
    ));

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
                bottom: Val::Px(8.0),
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
                        width: Val::Px(250.0),
                        padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
                        border: UiRect::all(Val::Px(3.0)),
                        ..default()
                    },
                    BorderColor::all(border),
                    BackgroundColor(Color::from(seat_color(seat) * 0.25).with_alpha(0.6)),
                ))
                .with_child((
                    HudText::SeatPanel(seat),
                    Text::new(""),
                    TextFont::from_font_size(16.0),
                    TextColor(Color::WHITE),
                ));
            }
        });
}

/// Who took the round, for the banner.
fn round_text(game: &State) -> String {
    let Some(team) = game.round_winner else {
        return String::new();
    };
    match game.round_end {
        Some(RoundEnd::Caught) => tr!("tabletop-rally-caught"),
        Some(RoundEnd::OffTable) => tr!("tabletop-rally-off-table"),
        Some(RoundEnd::Escaped) => tr!("tabletop-rally-escaped"),
        _ => {
            let seats: Vec<Seat> = game
                .playing()
                .filter(|s| game.format.team(*s) == Some(team))
                .collect();
            tr!(
                "tabletop-rally-takes",
                names = i18n::names(seats.iter().copied()),
                count = seats.len() as u32
            )
        }
    }
}

fn banner_text(active: &ActiveRun<TabletopRally>) -> (String, String) {
    let run = &active.run;
    let game = &run.game;
    if let Some(banner) = halted_banner(run) {
        return banner;
    }
    let round = || tr!("tabletop-rally-round", round = game.round as u32 + 1);
    match game.phase {
        // The shared countdown (crate::intro) is drawn over this.
        Phase::Intro { .. } => (String::new(), String::new()),
        Phase::Grid { .. } => (round(), tr!("tabletop-rally-ready")),
        Phase::Racing { t } => {
            let title = if t < 45 {
                tr!("tabletop-rally-go")
            } else {
                String::new()
            };
            let sub = if game.closing_in() {
                tr!("tabletop-rally-closing")
            } else if t < 90 {
                round()
            } else {
                String::new()
            };
            (title, sub)
        }
        Phase::Result { .. } | Phase::Finished => {
            if let Some(outcome) = game.outcome {
                let winners: Vec<Seat> = outcome.winners().collect();
                let prompt = if game.phase == Phase::Finished {
                    active.continue_prompt()
                } else {
                    round_text(game)
                };
                (i18n::winners(&winners), prompt)
            } else {
                let how = match game.round_end {
                    Some(RoundEnd::TimeUp) if game.getaway().is_none() => {
                        tr!("tabletop-rally-time")
                    }
                    Some(RoundEnd::PhotoFinish) => tr!("tabletop-rally-photo"),
                    _ => String::new(),
                };
                (round_text(game), how)
            }
        }
    }
}

fn update_hud(
    active: Res<ActiveRun<TabletopRally>>,
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
                let car = game.car(seat);
                let role = match game.getaway() {
                    Some(getaway) if getaway == seat => {
                        format!("  {}", tr!("tabletop-rally-getaway"))
                    }
                    Some(_) => format!("  {}", tr!("tabletop-rally-chaser")),
                    None => String::new(),
                };
                let points = game
                    .format
                    .team(seat)
                    .map_or(0, |team| game.points[team as usize]);
                let status = if active.run.controller_lost(seat) {
                    tr!("controller-lost")
                } else if let (Some(outcome), true) = (
                    game.outcome,
                    matches!(game.phase, Phase::Result { .. } | Phase::Finished),
                ) {
                    match outcome.placement[seat.index()] {
                        Some(place) => tr!("place", place = place as u32),
                        None => String::new(),
                    }
                } else {
                    match car.out {
                        Some((Out::Screen, _)) => tr!("tabletop-rally-out-screen"),
                        Some((Out::Table, _)) => tr!("tabletop-rally-out-table"),
                        Some((Out::Caught, _)) => tr!("tabletop-rally-out-caught"),
                        None => String::new(),
                    }
                };
                let points = tr!("tabletop-rally-points", points = points, goal = WIN_POINTS);
                let status = if status.is_empty() {
                    status
                } else {
                    format!("  {status}")
                };
                format!("{}{who}{role}\n{points}{status}", seat_label(seat))
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn karts_face_their_heading() {
        for heading in [0.0, 1.0, -2.5, PI / 2.0] {
            let nose = heading_rotation(heading) * Vec3::X;
            let want = world(SimVec2::from_angle(heading));
            assert!(nose.abs_diff_eq(want, 1e-5), "{heading}: {nose} vs {want}");
        }
    }

    #[test]
    fn the_camera_looks_down_at_its_target_with_the_screen_up_away_from_it() {
        let camera = camera_transform(SimVec2::new(3.0, -2.0));
        let ahead = camera.forward();
        // Straight down the middle of the screen lands on the target.
        let to_floor = camera.translation.y / -ahead.y;
        let hit = camera.translation + *ahead * to_floor;
        assert!(hit.abs_diff_eq(Vec3::new(3.0, 0.0, -2.0), 1e-3), "{hit}");
        // Up on screen is up the table (towards -y in the simulation).
        assert!(camera.up().z < 0.0 && camera.right().abs_diff_eq(Vec3::X, 1e-5));
    }

    #[test]
    fn the_knockout_rectangle_fits_any_window_with_its_edge_in_view() {
        for aspect in [16.0 / 10.0, 16.0 / 9.0, 4.0 / 3.0, 21.0 / 9.0] {
            let (w, h) = rectangle_share(VIEW_HALF_START, aspect);
            assert!(w < 1.0 && h < 1.0, "{aspect}: {w} {h}");
            // At least one side is framed with exactly its margin, and the other with more.
            let tight_x = (w - 1.0 / FRAME_MARGIN_X).abs() < 1e-4;
            let tight_y = (h - 1.0 / FRAME_MARGIN_Y).abs() < 1e-4;
            assert!(tight_x || tight_y, "{aspect}: {w} {h}");
            assert!(w <= 1.0 / FRAME_MARGIN_X + 1e-4 && h <= 1.0 / FRAME_MARGIN_Y + 1e-4);
        }
    }
}
