//! Presentation for Quick Draw. Reads `ActiveRun<QuickDraw>` only; never writes simulation state.

use std::f32::consts::PI;

use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use minigame_quick_draw::{POINTS_TO_WIN, Phase, QuickDraw, Reaction, State};
use sim_core::{Seat, TICK_HZ};

use crate::palette::{LANTERN_WARM, MOONLIGHT, NIGHT_SKY, seat_color, seat_label};
use crate::runner::{ActiveRun, Seats};

pub struct QuickDrawViewPlugin;

impl Plugin for QuickDrawViewPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(NIGHT_SKY))
            .insert_resource(GlobalAmbientLight {
                color: MOONLIGHT,
                brightness: 30.0,
                ..default()
            })
            .add_systems(Startup, (spawn_scene, spawn_hud))
            .add_systems(
                Update,
                (
                    animate_lantern,
                    animate_characters,
                    animate_fireflies,
                    (fit_camera, shake_camera).chain(),
                    update_hud,
                ),
            );
    }
}

const LANTERN_POS: Vec3 = Vec3::new(0.0, 3.7, -2.6);
const CHARACTER_SPACING: f32 = 2.3;
const FIREFLY_COUNT: usize = 48;

#[derive(Component)]
struct Lantern {
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct LanternLight;

#[derive(Component)]
struct CameraRig {
    base: Transform,
}

#[derive(Component)]
struct Character {
    seat: Seat,
    base: Vec3,
    head_material: Handle<StandardMaterial>,
    head_light: Entity,
    /// Reaction last seen, to trigger a hop the moment a press registers.
    seen: Reaction,
    seen_generation: u32,
    hop_started: Option<f32>,
}

#[derive(Component)]
struct Marker {
    base_y: f32,
}

#[derive(Component)]
struct Firefly {
    origin: Vec3,
    phase: f32,
    speed: f32,
    radius: f32,
}

#[derive(Component)]
enum HudText {
    Banner,
    SubBanner,
    SeatPanel(Seat),
}

/// Cheap deterministic noise for scenery layout. Presentation only, so any function will do.
fn noise(i: i32, j: i32, salt: u32) -> f32 {
    let mut h = (i as u32).wrapping_mul(0x8da6_b343)
        ^ (j as u32).wrapping_mul(0xd816_3841)
        ^ salt.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 65535.0
}

fn emissive_material(color: LinearRgba, strength: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgb(0.08, 0.07, 0.06),
        emissive: color * strength,
        perceptual_roughness: 0.6,
        ..default()
    }
}

fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    active: Res<ActiveRun<QuickDraw>>,
) {
    let camera = camera_transform(1.0);
    commands.spawn((
        Camera3d::default(),
        Hdr,
        Tonemapping::TonyMcMapface,
        Bloom {
            intensity: 0.22,
            ..Bloom::NATURAL
        },
        DistanceFog {
            color: Color::srgb(0.02, 0.025, 0.06),
            falloff: FogFalloff::Linear {
                start: 12.0,
                end: 34.0,
            },
            ..default()
        },
        camera,
        CameraRig { base: camera },
    ));

    commands.spawn((
        DirectionalLight {
            color: MOONLIGHT,
            illuminance: 350.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-6.0, 10.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    spawn_ground(&mut commands, &mut meshes, &mut materials);
    spawn_lantern(&mut commands, &mut meshes, &mut materials);
    spawn_side_lanterns(&mut commands, &mut meshes, &mut materials);
    spawn_fireflies(&mut commands, &mut meshes, &mut materials);

    let players = active.run.players;
    let count = players.count() as f32;
    for (i, seat) in players.seats().enumerate() {
        let x = (i as f32 - (count - 1.0) / 2.0) * CHARACTER_SPACING;
        spawn_character(
            &mut commands,
            &mut meshes,
            &mut materials,
            seat,
            Vec3::new(x, 0.0, 1.6),
        );
    }
}

fn spawn_ground(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let tile = meshes.add(Cuboid::new(0.96, 0.4, 0.96));
    let shades: Vec<_> = [
        (0.055, 0.06, 0.075),
        (0.045, 0.05, 0.065),
        (0.07, 0.068, 0.08),
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
    for i in -9..=9 {
        for j in -7..=5 {
            let shade = &shades[(noise(i, j, 1) * 2.999) as usize];
            let y = -0.2 + (noise(i, j, 2) - 0.5) * 0.06;
            commands.spawn((
                Mesh3d(tile.clone()),
                MeshMaterial3d(shade.clone()),
                Transform::from_xyz(i as f32, y, j as f32),
            ));
        }
    }
}

fn spawn_lantern(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let wood = materials.add(StandardMaterial {
        base_color: Color::srgb(0.09, 0.06, 0.045),
        perceptual_roughness: 0.85,
        ..default()
    });
    let iron = materials.add(StandardMaterial {
        base_color: Color::srgb(0.05, 0.05, 0.055),
        metallic: 0.6,
        perceptual_roughness: 0.4,
        ..default()
    });
    let post_height = LANTERN_POS.y - 0.6;
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.3, post_height, 0.3))),
        MeshMaterial3d(wood.clone()),
        Transform::from_xyz(LANTERN_POS.x, post_height / 2.0, LANTERN_POS.z),
    ));
    let glow = materials.add(emissive_material(LANTERN_WARM, 1.5));
    commands
        .spawn((
            Lantern {
                material: glow.clone(),
            },
            Mesh3d(meshes.add(Cuboid::new(0.9, 1.0, 0.9))),
            MeshMaterial3d(glow),
            Transform::from_translation(LANTERN_POS),
        ))
        .with_children(|lantern| {
            lantern.spawn((
                Mesh3d(meshes.add(Cuboid::new(1.15, 0.16, 1.15))),
                MeshMaterial3d(iron.clone()),
                Transform::from_xyz(0.0, 0.58, 0.0),
            ));
            lantern.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.5, 0.2, 0.5))),
                MeshMaterial3d(iron.clone()),
                Transform::from_xyz(0.0, 0.76, 0.0),
            ));
            lantern.spawn((
                Mesh3d(meshes.add(Cuboid::new(1.05, 0.12, 1.05))),
                MeshMaterial3d(iron),
                Transform::from_xyz(0.0, -0.56, 0.0),
            ));
            lantern.spawn((
                LanternLight,
                PointLight {
                    color: Color::from(LANTERN_WARM),
                    intensity: 60_000.0,
                    range: 22.0,
                    radius: 0.4,
                    shadow_maps_enabled: true,
                    ..default()
                },
                Transform::from_xyz(0.0, 0.0, 0.8),
            ));
        });
}

/// Small coloured paper lanterns around the edge: cheap unshadowed lights that give the
/// night scene its colour.
fn spawn_side_lanterns(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let pole_mesh = meshes.add(Cuboid::new(0.12, 2.4, 0.12));
    let paper_mesh = meshes.add(Cuboid::new(0.42, 0.55, 0.42));
    let pole = materials.add(StandardMaterial {
        base_color: Color::srgb(0.06, 0.045, 0.035),
        perceptual_roughness: 0.9,
        ..default()
    });
    let spots = [
        (Vec3::new(-6.5, 0.0, -4.5), LinearRgba::rgb(1.0, 0.3, 0.55)),
        (Vec3::new(6.5, 0.0, -4.5), LinearRgba::rgb(0.25, 0.75, 1.0)),
        (Vec3::new(-7.5, 0.0, 0.5), LANTERN_WARM),
        (Vec3::new(7.5, 0.0, 0.5), LinearRgba::rgb(1.0, 0.3, 0.55)),
        (Vec3::new(-3.6, 0.0, -5.5), LinearRgba::rgb(0.25, 0.75, 1.0)),
        (Vec3::new(3.6, 0.0, -5.5), LANTERN_WARM),
    ];
    for (pos, color) in spots {
        commands.spawn((
            Mesh3d(pole_mesh.clone()),
            MeshMaterial3d(pole.clone()),
            Transform::from_translation(pos + Vec3::Y * 1.2),
        ));
        commands.spawn((
            Mesh3d(paper_mesh.clone()),
            MeshMaterial3d(materials.add(emissive_material(color, 2.2))),
            Transform::from_translation(pos + Vec3::Y * 2.55),
            children![(
                PointLight {
                    color: Color::from(color),
                    intensity: 80_000.0,
                    range: 7.0,
                    ..default()
                },
                Transform::default(),
            )],
        ));
    }
}

fn spawn_fireflies(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let mesh = meshes.add(Cuboid::new(0.06, 0.06, 0.06));
    let glow = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: LinearRgba::rgb(0.75, 1.0, 0.3) * 10.0,
        ..default()
    });
    for k in 0..FIREFLY_COUNT as i32 {
        let origin = Vec3::new(
            (noise(k, 0, 10) - 0.5) * 17.0,
            0.5 + noise(k, 0, 11) * 3.2,
            -6.0 + noise(k, 0, 12) * 9.0,
        );
        commands.spawn((
            Firefly {
                origin,
                phase: noise(k, 0, 13) * 100.0,
                speed: 0.25 + noise(k, 0, 14) * 0.5,
                radius: 0.3 + noise(k, 0, 15) * 0.7,
            },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(glow.clone()),
            Transform::from_translation(origin),
        ));
    }
}

/// Placeholder "lantern folk": every character shares this body plan; the glowing head is
/// where their personality (and player colour) lives.
fn spawn_character(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    seat: Seat,
    base: Vec3,
) {
    let color = seat_color(seat);
    let body = materials.add(StandardMaterial {
        base_color: Color::from(color * 0.12 + LinearRgba::rgb(0.03, 0.03, 0.04)),
        perceptual_roughness: 0.8,
        ..default()
    });
    let dark = materials.add(StandardMaterial {
        base_color: Color::srgb(0.02, 0.02, 0.025),
        perceptual_roughness: 0.5,
        ..default()
    });
    let head_material = materials.add(emissive_material(color, 1.8));
    let marker_material = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: color * 4.0,
        ..default()
    });
    let marker_mesh = match seat.0 {
        0 => meshes.add(Sphere::new(0.15)),
        1 => meshes.add(Cone::new(0.17, 0.32)),
        2 => meshes.add(Cuboid::new(0.26, 0.26, 0.26)),
        _ => meshes.add(Cuboid::new(0.22, 0.22, 0.22)),
    };
    let marker_rotation = if seat.0 == 3 {
        Quat::from_rotation_z(PI / 4.0)
    } else {
        Quat::IDENTITY
    };

    let head_light = commands
        .spawn((
            PointLight {
                color: Color::from(color),
                intensity: 40_000.0,
                range: 4.0,
                ..default()
            },
            Transform::from_xyz(0.0, 1.42, 0.5),
        ))
        .id();

    let leg = meshes.add(Cuboid::new(0.22, 0.32, 0.26));
    let arm = meshes.add(Cuboid::new(0.18, 0.46, 0.22));
    let eye = meshes.add(Cuboid::new(0.1, 0.15, 0.04));
    let rim = meshes.add(Cuboid::new(0.82, 0.09, 0.72));

    commands
        .spawn((
            Character {
                seat,
                base,
                head_material: head_material.clone(),
                head_light,
                seen: Reaction::Waiting,
                seen_generation: 0,
                hop_started: None,
            },
            Transform::from_translation(base),
            Visibility::default(),
        ))
        .with_children(|root| {
            for x in [-0.2, 0.2] {
                root.spawn((
                    Mesh3d(leg.clone()),
                    MeshMaterial3d(body.clone()),
                    Transform::from_xyz(x, 0.16, 0.0),
                ));
            }
            root.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.8, 0.74, 0.6))),
                MeshMaterial3d(body.clone()),
                Transform::from_xyz(0.0, 0.68, 0.0),
            ));
            for x in [-0.5, 0.5] {
                root.spawn((
                    Mesh3d(arm.clone()),
                    MeshMaterial3d(body.clone()),
                    Transform::from_xyz(x, 0.68, 0.0),
                ));
            }
            root.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.72, 0.62, 0.62))),
                MeshMaterial3d(head_material),
                Transform::from_xyz(0.0, 1.42, 0.0),
            ));
            for y in [1.08, 1.77] {
                root.spawn((
                    Mesh3d(rim.clone()),
                    MeshMaterial3d(dark.clone()),
                    Transform::from_xyz(0.0, y, 0.0),
                ));
            }
            for x in [-0.15, 0.15] {
                root.spawn((
                    Mesh3d(eye.clone()),
                    MeshMaterial3d(dark.clone()),
                    Transform::from_xyz(x, 1.45, 0.32),
                ));
            }
            root.spawn((
                Marker { base_y: 2.35 },
                Mesh3d(marker_mesh),
                MeshMaterial3d(marker_material),
                Transform::from_xyz(0.0, 2.35, 0.0).with_rotation(marker_rotation),
            ));
        })
        .add_child(head_light);
}

/// The unlit lantern smoulders faintly, so the moment it lights is unmistakable.
fn ember(time: f32) -> f32 {
    0.05 + (time * 7.3).sin() * 0.01 + (time * 13.1).sin() * 0.008
}

fn lantern_glow(phase: Phase, time: f32) -> (f32, f32) {
    let tick = 1.0 / TICK_HZ as f32;
    match phase {
        Phase::Drawn { t } => {
            let decay = (-(t as f32) * tick * 3.0).exp();
            (8.0 + 40.0 * decay, 700_000.0 + 2_500_000.0 * decay)
        }
        Phase::RoundOver { t } => {
            let fade = (1.0 - t as f32 * tick * 1.5).max(0.0);
            (ember(time) + 8.0 * fade, 15_000.0 + 700_000.0 * fade)
        }
        Phase::Finished => (6.0, 500_000.0),
        Phase::Intro { .. } | Phase::Waiting { .. } => (ember(time), 15_000.0),
    }
}

fn animate_lantern(
    time: Res<Time>,
    active: Res<ActiveRun<QuickDraw>>,
    lantern: Query<&Lantern>,
    mut light: Query<&mut PointLight, With<LanternLight>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (glow, lumens) = lantern_glow(active.run.game.phase, time.elapsed_secs());
    for lantern in &lantern {
        if let Some(mut material) = materials.get_mut(&lantern.material) {
            material.emissive = LANTERN_WARM * glow;
        }
    }
    for mut light in &mut light {
        light.intensity = lumens;
    }
}

const HOP_SECONDS: f32 = 0.35;

fn animate_characters(
    time: Res<Time>,
    active: Res<ActiveRun<QuickDraw>>,
    mut characters: Query<(&mut Character, &mut Transform)>,
    mut lights: Query<&mut PointLight, Without<LanternLight>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let now = time.elapsed_secs();
    let game = &active.run.game;
    for (mut character, mut transform) in &mut characters {
        let seat = character.seat;
        let reaction = game.reactions[seat.index()];
        if character.seen_generation != active.generation {
            character.seen_generation = active.generation;
            character.seen = Reaction::Waiting;
            character.hop_started = None;
        }
        if reaction != character.seen {
            if matches!(reaction, Reaction::Fired { .. }) {
                character.hop_started = Some(now);
            }
            character.seen = reaction;
        }

        let celebrating = match game.phase {
            Phase::RoundOver { .. } => game.round_winners[seat.index()],
            Phase::Finished => game.scores[seat.index()] == *game.scores.iter().max().unwrap_or(&0),
            _ => false,
        };
        let idle = (now * 2.2 + seat.0 as f32 * 1.7).sin() * 0.03;
        let hop = match character.hop_started {
            Some(start) if now - start < HOP_SECONDS => {
                ((now - start) / HOP_SECONDS * PI).sin() * 0.7
            }
            _ if celebrating => (now * 9.0).sin().abs() * 0.35,
            _ => 0.0,
        };
        transform.translation = character.base + Vec3::Y * (idle + hop);
        transform.rotation = if reaction == Reaction::FalseStart {
            Quat::from_rotation_x(-0.35)
        } else {
            Quat::IDENTITY
        };

        let glow = match reaction {
            Reaction::FalseStart => 0.15,
            Reaction::TooSlow => 0.8,
            _ if celebrating => 5.0,
            Reaction::Fired { .. } => 3.0,
            Reaction::Waiting => 1.8 + (now * 5.0 + seat.0 as f32).sin() * 0.1,
        };
        if let Some(mut material) = materials.get_mut(&character.head_material) {
            material.emissive = seat_color(seat) * glow;
        }
        if let Ok(mut light) = lights.get_mut(character.head_light) {
            light.intensity = 22_000.0 * glow;
        }
    }
}

fn animate_fireflies(
    time: Res<Time>,
    mut fireflies: Query<(&Firefly, &mut Transform), Without<Marker>>,
    mut markers: Query<(&Marker, &mut Transform), Without<Firefly>>,
) {
    let t = time.elapsed_secs();
    for (fly, mut transform) in &mut fireflies {
        let p = t * fly.speed + fly.phase;
        transform.translation = fly.origin
            + Vec3::new(
                p.sin() * fly.radius,
                (p * 1.3).sin() * fly.radius * 0.5,
                (p * 0.7).cos() * fly.radius,
            );
        let twinkle = 0.6 + 0.4 * (t * 3.0 + fly.phase).sin().max(0.0);
        transform.scale = Vec3::splat(twinkle);
    }
    for (marker, mut transform) in &mut markers {
        transform.translation.y = marker.base_y + (t * 2.0).sin() * 0.06;
        transform.rotate_y(time.delta_secs() * 1.2);
    }
}

const CAMERA_TARGET: Vec3 = Vec3::new(0.0, 1.7, 0.0);
const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 2.7, 10.0);
/// Half-width of the character line (plus margin) that must stay on screen.
const FRAMED_HALF_WIDTH: f32 = 4.7;

fn camera_transform(pull_back: f32) -> Transform {
    Transform::from_translation(CAMERA_TARGET + CAMERA_OFFSET * pull_back)
        .looking_at(CAMERA_TARGET, Vec3::Y)
}

/// Pull the camera back on narrow windows so all four characters stay in frame.
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

fn shake_camera(
    active: Res<ActiveRun<QuickDraw>>,
    time: Res<Time>,
    mut camera: Query<(&CameraRig, &mut Transform)>,
) {
    let strength = match active.run.game.phase {
        Phase::Drawn { t } if t < 12 => 0.07 * (1.0 - t as f32 / 12.0),
        _ => 0.0,
    };
    let t = time.elapsed_secs();
    for (rig, mut transform) in &mut camera {
        *transform = rig.base;
        transform.translation += Vec3::new((t * 91.0).sin(), (t * 73.0).cos(), 0.0) * strength;
    }
}

fn spawn_hud(mut commands: Commands, active: Res<ActiveRun<QuickDraw>>) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(36.0),
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(6.0),
            ..default()
        })
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

    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(20.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            column_gap: Val::Px(16.0),
            ..default()
        })
        .with_children(|row| {
            for seat in active.run.players.seats() {
                row.spawn((
                    Node {
                        width: Val::Px(200.0),
                        padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
                        ..default()
                    },
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

fn ms(ticks: u16) -> u32 {
    ticks as u32 * 1000 / TICK_HZ
}

fn names(seats: impl Iterator<Item = Seat>) -> String {
    seats.map(seat_label).collect::<Vec<_>>().join(" & ")
}

fn banner_text(active: &ActiveRun<QuickDraw>) -> (String, String) {
    let run = &active.run;
    let game: &State = &run.game;
    if run.waiting_for_controller() {
        let lost = names(run.players.seats().filter(|s| run.controller_lost(*s)));
        return (
            "Controller disconnected".into(),
            format!("Reconnect {lost}'s controller"),
        );
    }
    if run.paused {
        return ("Paused".into(), "Press Start to resume".into());
    }
    match game.phase {
        Phase::Intro { .. } => (
            "QUICK DRAW".into(),
            format!("Press A the instant the lantern lights. First to {POINTS_TO_WIN} wins!"),
        ),
        Phase::Waiting { .. } => ("Wait for it...".into(), format!("Round {}", game.round)),
        Phase::Drawn { .. } => ("DRAW!".into(), String::new()),
        Phase::RoundOver { .. } => {
            let winners: Vec<Seat> = game
                .players
                .seats()
                .filter(|s| game.round_winners[s.index()])
                .collect();
            match winners.first().map(|s| game.reactions[s.index()]) {
                Some(Reaction::Fired { ticks }) => (
                    format!("{} takes the round!", names(winners.into_iter())),
                    format!("{} ms", ms(ticks)),
                ),
                _ => ("Nobody scores".into(), String::new()),
            }
        }
        Phase::Finished => {
            let outcome = run.outcome().expect("finished games have an outcome");
            let winners: Vec<Seat> = outcome.winners().collect();
            let verb = if winners.len() == 1 { "wins" } else { "win" };
            let prompt = if active.can_restart() {
                "Press A to play again"
            } else {
                ""
            };
            (
                format!("{} {verb}!", names(winners.into_iter())),
                prompt.into(),
            )
        }
    }
}

fn update_hud(
    active: Res<ActiveRun<QuickDraw>>,
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
                let who = if seats.is_bot(seat) { " (bot)" } else { "" };
                let score = game.scores[seat.index()];
                let pips: String = (0..POINTS_TO_WIN)
                    .map(|i| if i < score { '#' } else { '-' })
                    .collect();
                let status = if active.run.controller_lost(seat) {
                    "controller lost".to_string()
                } else {
                    match game.reactions[seat.index()] {
                        Reaction::Fired { ticks } => format!("{} ms", ms(ticks)),
                        Reaction::FalseStart => "FALSE START".into(),
                        Reaction::TooSlow => "too slow".into(),
                        Reaction::Waiting => String::new(),
                    }
                };
                format!("{}{who}   [{pips}]\n{status}", seat_label(seat))
            }
        };
    }
}
