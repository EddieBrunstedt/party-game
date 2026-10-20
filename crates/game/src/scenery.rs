//! Scenery shared by every scene: the night camera, the lantern folk, drifting fireflies and
//! paper lanterns. The folk are Blender models (`crate::characters`); the rest are code-built
//! placeholders, replaced piece by piece as real assets arrive.

use std::f32::consts::PI;

use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use sim_core::Seat;

use crate::characters::{Character, CharacterModel, Pose};
use crate::flow::AppState;
use crate::palette::seat_color;

pub struct SceneryPlugin;

impl Plugin for SceneryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (animate_fireflies, animate_markers));
    }
}

/// Cheap deterministic noise for scenery layout. Presentation only, so any function will do.
pub fn noise(i: i32, j: i32, salt: u32) -> f32 {
    let mut h = (i as u32).wrapping_mul(0x8da6_b343)
        ^ (j as u32).wrapping_mul(0xd816_3841)
        ^ salt.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 65535.0
}

pub fn emissive_material(color: LinearRgba, strength: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgb(0.08, 0.07, 0.06),
        emissive: color * strength,
        perceptual_roughness: 0.6,
        ..default()
    }
}

/// The HDR, bloom and fog setup every night scene shares.
pub fn night_camera(transform: Transform, fog: (f32, f32)) -> impl Bundle {
    (
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
                start: fog.0,
                end: fog.1,
            },
            ..default()
        },
        transform,
    )
}

/// A seat's floating marker shape (P1 ●, P2 ▲, P3 ■, P4 ◆), so colour is never the only cue.
#[derive(Component)]
pub struct Marker {
    pub base_y: f32,
}

/// The handles a view needs to animate a lantern-folk character.
pub struct LanternFolk {
    pub root: Entity,
    pub head_material: Handle<StandardMaterial>,
    pub head_light: Entity,
}

/// A lantern-folk character in its seat's colours, with the seat's marker floating overhead.
/// The root sits at the feet, facing +z; its [`Pose`] says how the body moves.
pub fn spawn_lantern_folk(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    scope: AppState,
    seat: Seat,
    character: Character,
    transform: Transform,
) -> LanternFolk {
    let color = seat_color(seat);
    let body = materials.add(StandardMaterial {
        base_color: Color::from(color * 0.12 + LinearRgba::rgb(0.03, 0.03, 0.04)),
        perceptual_roughness: 0.8,
        ..default()
    });
    let head_material = materials.add(emissive_material(color, 1.8));
    let accent = materials.add(emissive_material(color, 6.0));
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

    let root = commands
        .spawn((
            DespawnOnExit(scope),
            transform,
            Visibility::default(),
            Pose::default(),
        ))
        .with_child((
            Marker { base_y: 2.55 },
            Mesh3d(marker_mesh),
            MeshMaterial3d(marker_material),
            Transform::from_xyz(0.0, 2.55, 0.0).with_rotation(marker_rotation),
        ))
        .add_child(head_light)
        .id();
    commands.entity(root).insert(CharacterModel {
        character,
        body,
        glow: head_material.clone(),
        accent,
        head_light,
    });
    LanternFolk {
        root,
        head_material,
        head_light,
    }
}

#[derive(Component)]
pub struct AmbientFirefly {
    origin: Vec3,
    phase: f32,
    speed: f32,
    radius: f32,
}

/// Fireflies drifting around inside a box, purely decorative.
pub fn spawn_fireflies(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    scope: AppState,
    count: usize,
    min: Vec3,
    max: Vec3,
) {
    let mesh = meshes.add(Cuboid::new(0.06, 0.06, 0.06));
    let glow = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: LinearRgba::rgb(0.75, 1.0, 0.3) * 10.0,
        ..default()
    });
    let size = max - min;
    for k in 0..count as i32 {
        let origin = min
            + Vec3::new(
                noise(k, 0, 10) * size.x,
                noise(k, 0, 11) * size.y,
                noise(k, 0, 12) * size.z,
            );
        commands.spawn((
            DespawnOnExit(scope),
            AmbientFirefly {
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

/// A coloured paper lantern on a pole, lit by a cheap unshadowed light.
pub fn spawn_paper_lantern(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    scope: AppState,
    pos: Vec3,
    color: LinearRgba,
) {
    let pole = materials.add(StandardMaterial {
        base_color: Color::srgb(0.06, 0.045, 0.035),
        perceptual_roughness: 0.9,
        ..default()
    });
    commands.spawn((
        DespawnOnExit(scope),
        Mesh3d(meshes.add(Cuboid::new(0.12, 2.4, 0.12))),
        MeshMaterial3d(pole),
        Transform::from_translation(pos + Vec3::Y * 1.2),
    ));
    commands.spawn((
        DespawnOnExit(scope),
        Mesh3d(meshes.add(Cuboid::new(0.42, 0.55, 0.42))),
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

fn animate_fireflies(time: Res<Time>, mut fireflies: Query<(&AmbientFirefly, &mut Transform)>) {
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
}

fn animate_markers(time: Res<Time>, mut markers: Query<(&Marker, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (marker, mut transform) in &mut markers {
        transform.translation.y = marker.base_y + (t * 2.0).sin() * 0.06;
        transform.rotate_y(time.delta_secs() * 1.2);
    }
}
