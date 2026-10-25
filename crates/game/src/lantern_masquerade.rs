//! Presentation for Lantern Masquerade. Reads `ActiveRun<LanternMasquerade>` only; never writes
//! simulation state. Every folk in the crowd is drawn exactly alike, players included: who's who
//! comes only from `State::revealed`, so the view can't give a player away before the game does.
//! A folk's look changes only with what everyone can see: walking, lying down, striking, being
//! lit by the seeker's lantern.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;
use minigame_lantern_masquerade::{
    BELL_POSITIONS, BELL_REACH, CATCH_REACH, DARK_TICKS, Ending, FOLK, Folk, HALF_DEPTH,
    HALF_WIDTH, LanternMasquerade, Phase, RING_TICKS, ROUNDS_TO_WIN, SHUFFLE_TICKS, State, dir_vec,
};
use sim_core::physics::Vec2 as SimVec2;
use sim_core::{Format, MAX_PLAYERS, Seat, TICK_HZ};

use crate::audio::{Sfx, Sound};
use crate::characters::{Character, CharacterModel, Pose};
use crate::flow::AppState;
use crate::i18n::{self, bot_tag, halted_banner};
use crate::palette::{LANTERN_WARM, MOONLIGHT, NIGHT_SKY, seat_color, seat_label};
use crate::particles::{self, Effects, hdr};
use crate::runner::{ActiveRun, MinigameSet, Seats};
use crate::scenery::{
    Marker, emissive_material, night_camera, noise, spawn_fireflies, spawn_paper_lantern,
};
use crate::tr;

pub struct LanternMasqueradeViewPlugin;

impl Plugin for LanternMasqueradeViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            (spawn_scene, spawn_hud)
                .after(MinigameSet::Start)
                .run_if(resource_exists::<ActiveRun<LanternMasquerade>>),
        )
        .add_systems(
            Update,
            (
                animate_folk,
                animate_bells,
                animate_sight,
                fit_camera,
                update_hud,
            )
                .run_if(resource_exists::<ActiveRun<LanternMasquerade>>),
        )
        .add_systems(
            FixedUpdate,
            react
                .in_set(MinigameSet::React)
                .run_if(resource_exists::<ActiveRun<LanternMasquerade>>),
        );
    }
}

const SCOPE: DespawnOnExit<AppState> = DespawnOnExit(AppState::Minigame);
/// Everyone's lantern: one colour for the whole crowd.
const CROWD_GLOW: LinearRgba = LinearRgba::rgb(1.0, 0.72, 0.42);
const CROWD_BODY: Color = Color::srgb(0.07, 0.06, 0.07);
/// Folk shown to be strollers glow less.
const CLEARED_GLOW: f32 = 0.7;
const GLOW: f32 = 1.6;
/// Everyone is drawn as the same character, so nobody stands out.
const CROWD_CHARACTER: Character = Character::PaperLantern;
const FOLK_SCALE: f32 = 0.85;
const BELL_HEIGHT: f32 = 2.7;
const BELL_COLOR: LinearRgba = LinearRgba::rgb(1.0, 0.8, 0.35);
/// How long a strike's lunge and a hit's flash show, in seconds.
const LUNGE_SECONDS: f32 = 0.3;
const FLASH_SECONDS: f32 = 0.5;
const FIREFLY_COUNT: usize = 24;
/// Where the folk's heads are.
const SIGHT_RING_HEIGHT: f32 = 1.25;

/// Sounds and sparks, once per event: rings, strikes, catches and the results.
fn react(
    mut commands: Commands,
    active: Res<ActiveRun<LanternMasquerade>>,
    effects: Res<Effects>,
    sfx: Sfx,
) {
    let Some((before, now)) = active.ticked() else {
        return;
    };
    let mut spark = |pos: Vec3, color: Vec4| {
        particles::burst(&mut commands, &effects, SCOPE, pos, color);
    };
    if before.can_act() != now.can_act() && now.can_act() {
        sfx.play(Sound::Go);
    }
    for (b, (was, is)) in before.bells.iter().zip(&now.bells).enumerate() {
        if is.rung_at.is_some() && is.rung_at != was.rung_at {
            sfx.play_varied(Sound::Star, b as u32);
            let ringer = Seat((is.rung_by & !was.rung_by).trailing_zeros().min(3) as u8);
            spark(
                world(BELL_POSITIONS[b]) + Vec3::Y * BELL_HEIGHT,
                hdr(seat_color(ringer), 6.0),
            );
        }
    }
    for f in 0..FOLK {
        let (was, is) = (&before.folk[f], &now.folk[f]);
        let variant = now.tick + f as u32;
        if is.struck_at.is_some() && is.struck_at != was.struck_at {
            sfx.play_varied(Sound::Dash, variant);
        }
        if is.hit_at.is_some() && is.hit_at != was.hit_at {
            let at = world(is.pos) + Vec3::Y * 1.2;
            match now.revealed(f) {
                Some(seat) => {
                    sfx.play_varied(Sound::Fall, variant);
                    spark(at, hdr(seat_color(seat), 6.0));
                }
                None if is.down > 0 => {
                    sfx.play_varied(Sound::Hit, variant);
                    spark(at, hdr(LinearRgba::rgb(0.6, 0.6, 0.7), 3.0));
                }
                None => {}
            }
        }
    }
    if now.sight.missed_at.is_some() && now.sight.missed_at != before.sight.missed_at {
        sfx.play(Sound::FalseStart);
    }
    let scored =
        matches!(before.phase, Phase::Play { .. }) && matches!(now.phase, Phase::Result { .. });
    if scored {
        if now.outcome.is_some() {
            sfx.play(Sound::Win);
        } else {
            sfx.play(Sound::RoundWin);
        }
        for f in 0..FOLK {
            if let Some(seat) = now.revealed(f)
                && now.round_winners & 1 << seat.0 != 0
            {
                spark(
                    world(now.folk[f].pos) + Vec3::Y * 2.0,
                    hdr(seat_color(seat), 6.0),
                );
            }
        }
    }
}

#[derive(Component)]
struct FolkView {
    index: usize,
    body: Handle<StandardMaterial>,
    glow: Handle<StandardMaterial>,
}

/// A seat's marker, floating over that player's folk once they're revealed.
#[derive(Component)]
struct SeatMarker(Seat);

#[derive(Component)]
struct BellView {
    index: usize,
    material: Handle<StandardMaterial>,
    ring_material: Handle<StandardMaterial>,
}

/// The pivot a bell swings from.
#[derive(Component)]
struct BellSwing(usize);

/// A flat marker by a bell's spot, shown once that seat has rung it.
#[derive(Component)]
struct BellMark {
    bell: usize,
    seat: Seat,
}

#[derive(Component)]
struct SightView {
    material: Handle<StandardMaterial>,
    light: Entity,
}

#[derive(Component)]
struct CameraRig;

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
    active: Res<ActiveRun<LanternMasquerade>>,
) {
    commands.insert_resource(ClearColor(NIGHT_SKY));
    commands.insert_resource(GlobalAmbientLight {
        color: MOONLIGHT,
        brightness: 60.0,
        ..default()
    });
    commands.spawn((
        SCOPE,
        night_camera(camera_transform(1.0), (30.0, 70.0)),
        CameraRig,
    ));
    commands.spawn((
        SCOPE,
        DirectionalLight {
            color: MOONLIGHT,
            illuminance: 700.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-6.0, 14.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        SCOPE,
        PointLight {
            color: Color::from(LANTERN_WARM),
            intensity: 3_000_000.0,
            range: 40.0,
            radius: 1.0,
            ..default()
        },
        Transform::from_xyz(0.0, 12.0, 2.0),
    ));

    let game = &active.run.game;
    spawn_market(&mut commands, &mut meshes, &mut materials);
    spawn_bells(&mut commands, &mut meshes, &mut materials, game);
    spawn_folk(&mut commands, &mut materials);
    spawn_fireflies(
        &mut commands,
        &mut meshes,
        &mut materials,
        AppState::Minigame,
        FIREFLY_COUNT,
        Vec3::new(-13.0, 2.0, -9.0),
        Vec3::new(13.0, 5.0, 9.0),
    );
    for seat in game.hiders() {
        let mesh = match seat.0 {
            0 => meshes.add(Sphere::new(0.15)),
            1 => meshes.add(Cone::new(0.17, 0.32)),
            2 => meshes.add(Cuboid::new(0.26, 0.26, 0.26)),
            _ => meshes.add(Cuboid::new(0.22, 0.22, 0.22)),
        };
        let rotation = if seat.0 == 3 {
            Quat::from_rotation_z(TAU / 8.0)
        } else {
            Quat::IDENTITY
        };
        commands.spawn((
            SCOPE,
            SeatMarker(seat),
            Marker { base_y: 2.4 },
            Mesh3d(mesh),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::BLACK,
                emissive: seat_color(seat) * 4.0,
                ..default()
            })),
            Transform::from_xyz(0.0, 2.4, 0.0).with_rotation(rotation),
            Visibility::Hidden,
        ));
    }
    if let Some(seeker) = game.seeker() {
        spawn_sight(&mut commands, &mut meshes, &mut materials, seeker);
    }
}

/// The square: cobbles, a low kerb, and stalls with paper lanterns round the edge.
fn spawn_market(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let margin = 0.6;
    commands.spawn((
        SCOPE,
        Mesh3d(meshes.add(Cuboid::new(
            (HALF_WIDTH + 6.0) * 2.0,
            0.4,
            (HALF_DEPTH + 6.0) * 2.0,
        ))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.04, 0.035, 0.045),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.2, 0.0),
    ));
    // Cobbles in a few shades, so the floor reads as a square without competing with the crowd.
    let cobble = meshes.add(Cuboid::new(0.95, 0.04, 0.95));
    let shades: Vec<_> = (0..3)
        .map(|k| {
            let v = 0.07 + k as f32 * 0.018;
            materials.add(StandardMaterial {
                base_color: Color::srgb(v, v * 0.92, v * 0.85),
                perceptual_roughness: 0.9,
                ..default()
            })
        })
        .collect();
    let (cols, rows) = (
        ((HALF_WIDTH + margin) * 2.0).ceil() as i32,
        ((HALF_DEPTH + margin) * 2.0).ceil() as i32,
    );
    for i in 0..cols {
        for j in 0..rows {
            let x = -HALF_WIDTH - margin + 0.5 + i as f32;
            let z = -HALF_DEPTH - margin + 0.5 + j as f32;
            let shade = (noise(i, j, 3) * 3.0) as usize % 3;
            commands.spawn((
                SCOPE,
                Mesh3d(cobble.clone()),
                MeshMaterial3d(shades[shade].clone()),
                Transform::from_xyz(x, 0.0, z),
            ));
        }
    }
    // The kerb round the square.
    let kerb = materials.add(emissive_material(LANTERN_WARM, 0.6));
    let (w, d) = (HALF_WIDTH + margin + 0.1, HALF_DEPTH + margin + 0.1);
    for (size, pos) in [
        (
            Vec3::new(w * 2.0 + 0.2, 0.12, 0.2),
            Vec3::new(0.0, 0.06, -d),
        ),
        (Vec3::new(w * 2.0 + 0.2, 0.12, 0.2), Vec3::new(0.0, 0.06, d)),
        (Vec3::new(0.2, 0.12, d * 2.0), Vec3::new(-w, 0.06, 0.0)),
        (Vec3::new(0.2, 0.12, d * 2.0), Vec3::new(w, 0.06, 0.0)),
    ] {
        commands.spawn((
            SCOPE,
            Mesh3d(meshes.add(Cuboid::from_size(size))),
            MeshMaterial3d(kerb.clone()),
            Transform::from_translation(pos),
        ));
    }
    // Stalls along the back and the sides, with striped awnings.
    let counter = materials.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.08, 0.06),
        perceptual_roughness: 0.8,
        ..default()
    });
    let awnings = [
        LinearRgba::rgb(0.8, 0.2, 0.25),
        LinearRgba::rgb(0.25, 0.45, 0.8),
        LinearRgba::rgb(0.85, 0.65, 0.2),
        LinearRgba::rgb(0.3, 0.7, 0.4),
    ]
    .map(|c| {
        materials.add(StandardMaterial {
            base_color: Color::from(c * 0.35),
            emissive: c * 0.25,
            perceptual_roughness: 0.7,
            ..default()
        })
    });
    let counter_mesh = meshes.add(Cuboid::new(2.6, 1.0, 1.2));
    let awning_mesh = meshes.add(Cuboid::new(2.9, 0.08, 1.6));
    let mut stalls = Vec::new();
    for k in 0..7 {
        stalls.push((Vec3::new(-9.0 + k as f32 * 3.0, 0.0, -d - 1.4), 0.0f32));
    }
    for k in 0..4 {
        let z = -4.5 + k as f32 * 3.0;
        stalls.push((Vec3::new(-w - 1.4, 0.0, z), FRAC_PI_2));
        stalls.push((Vec3::new(w + 1.4, 0.0, z), -FRAC_PI_2));
    }
    for (k, (pos, turn)) in stalls.into_iter().enumerate() {
        let rotation = Quat::from_rotation_y(turn);
        commands.spawn((
            SCOPE,
            Mesh3d(counter_mesh.clone()),
            MeshMaterial3d(counter.clone()),
            Transform::from_translation(pos + Vec3::Y * 0.5).with_rotation(rotation),
        ));
        commands.spawn((
            SCOPE,
            Mesh3d(awning_mesh.clone()),
            MeshMaterial3d(awnings[k % awnings.len()].clone()),
            Transform::from_translation(pos + Vec3::Y * 2.1 + rotation * Vec3::Z * 0.3)
                .with_rotation(rotation * Quat::from_rotation_x(-0.25)),
        ));
    }
    let colors = [
        LinearRgba::rgb(1.0, 0.35, 0.2),
        LinearRgba::rgb(1.0, 0.75, 0.3),
        LinearRgba::rgb(0.5, 0.6, 1.0),
        LinearRgba::rgb(1.0, 0.45, 0.7),
    ];
    for (k, (x, z)) in [(-w, -d), (w, -d), (-w, d), (w, d)].into_iter().enumerate() {
        spawn_paper_lantern(
            commands,
            meshes,
            materials,
            AppState::Minigame,
            Vec3::new(x + x.signum() * 0.4, 0.0, z + z.signum() * 0.4),
            colors[k],
        );
    }
}

fn spawn_bells(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    game: &State,
) {
    let bell_mesh = meshes.add(Cone::new(0.42, 0.6));
    let clapper = meshes.add(Sphere::new(0.1));
    let rope = meshes.add(Cylinder::new(0.02, 1.2));
    let rope_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.16, 0.12),
        ..default()
    });
    let ring_mesh = meshes.add(
        Torus::new(BELL_REACH - 0.05, BELL_REACH + 0.05)
            .mesh()
            .major_resolution(64),
    );
    for (index, spot) in BELL_POSITIONS.iter().enumerate() {
        let at = world(*spot);
        let material = materials.add(emissive_material(BELL_COLOR, 1.5));
        let ring_material = materials.add(emissive_material(BELL_COLOR, 1.2));
        commands.spawn((
            SCOPE,
            BellView {
                index,
                material: material.clone(),
                ring_material: ring_material.clone(),
            },
            Mesh3d(ring_mesh.clone()),
            MeshMaterial3d(ring_material),
            Transform::from_translation(at + Vec3::Y * 0.05),
        ));
        commands
            .spawn((
                SCOPE,
                BellSwing(index),
                Transform::from_translation(at + Vec3::Y * (BELL_HEIGHT + 0.6)),
                Visibility::default(),
            ))
            .with_children(|pivot| {
                pivot.spawn((
                    Mesh3d(rope.clone()),
                    MeshMaterial3d(rope_material.clone()),
                    Transform::from_xyz(0.0, 0.6, 0.0),
                ));
                pivot.spawn((
                    Mesh3d(bell_mesh.clone()),
                    MeshMaterial3d(material),
                    Transform::from_xyz(0.0, -0.4, 0.0),
                ));
                pivot.spawn((
                    Mesh3d(clapper.clone()),
                    MeshMaterial3d(rope_material.clone()),
                    Transform::from_xyz(0.0, -0.75, 0.0),
                ));
            });
        // Who has rung it: each seat's marker, flat by the spot, in front of it.
        for seat in game.hiders() {
            let angle = TAU / 4.0 + (seat.0 as f32 - 1.5) * 0.42;
            let offset = Vec3::new(angle.cos(), 0.0, angle.sin()) * (BELL_REACH + 0.3);
            commands.spawn((
                SCOPE,
                BellMark { bell: index, seat },
                Mesh3d(crate::scenery::flat_marker_mesh(meshes, seat)),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::from(seat_color(seat)),
                    unlit: true,
                    ..default()
                })),
                Transform::from_translation(at + offset + Vec3::Y * 0.08)
                    .with_rotation(Quat::from_rotation_x(-FRAC_PI_2))
                    .with_scale(Vec3::splat(1.6)),
                Visibility::Hidden,
            ));
        }
    }
}

/// The crowd: every folk the same character in the same colours.
fn spawn_folk(commands: &mut Commands, materials: &mut Assets<StandardMaterial>) {
    for index in 0..FOLK {
        let body = materials.add(StandardMaterial {
            base_color: CROWD_BODY,
            perceptual_roughness: 0.8,
            ..default()
        });
        let glow = materials.add(emissive_material(CROWD_GLOW, GLOW));
        let accent = materials.add(emissive_material(CROWD_GLOW, 3.0));
        // No light on the head: thirty would cost more than they show. `CharacterModel` still
        // wants something to carry there.
        let head = commands.spawn(Transform::from_xyz(0.0, 1.42, 0.5)).id();
        let root = commands
            .spawn((
                SCOPE,
                FolkView {
                    index,
                    body: body.clone(),
                    glow: glow.clone(),
                },
                Transform::from_scale(Vec3::splat(FOLK_SCALE)),
                Visibility::default(),
                Pose::default(),
            ))
            .add_child(head)
            .id();
        commands.entity(root).insert(CharacterModel {
            character: CROWD_CHARACTER,
            body,
            glow,
            accent,
            head_light: head,
        });
    }
}

fn spawn_sight(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    seeker: Seat,
) {
    let material = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: seat_color(seeker) * 5.0,
        ..default()
    });
    let light = commands
        .spawn((
            SpotLight {
                color: Color::from(seat_color(seeker)),
                intensity: 600_000.0,
                range: 14.0,
                outer_angle: 0.16,
                inner_angle: 0.08,
                ..default()
            },
            Transform::from_xyz(0.0, 8.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z),
        ))
        .id();
    commands
        .spawn((
            SCOPE,
            SightView {
                material: material.clone(),
                light,
            },
            Transform::default(),
            Visibility::default(),
        ))
        .add_child(light)
        .with_children(|sight| {
            // The catch's reach on the floor, and again at head height: it rings the heads of
            // exactly the folk whose feet are in reach, where the floor ring hides behind them.
            let ring = meshes.add(
                Torus::new(CATCH_REACH - 0.05, CATCH_REACH + 0.05)
                    .mesh()
                    .major_resolution(48),
            );
            for height in [0.1, SIGHT_RING_HEIGHT] {
                sight.spawn((
                    Mesh3d(ring.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(0.0, height, 0.0),
                ));
            }
            // The seeker's lantern, hanging over the sight.
            sight.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.35, 0.45, 0.35))),
                MeshMaterial3d(material),
                Transform::from_xyz(0.0, 3.6, 0.0),
            ));
        });
}

/// Simulation floor coordinates to world space.
fn world(v: SimVec2) -> Vec3 {
    Vec3::new(v.x, 0.0, v.y)
}

fn blend(fixed: &Time<Fixed>) -> f32 {
    fixed.overstep_fraction().clamp(0.0, 1.0)
}

/// Seconds since a play tick, for one-shot effects.
fn since(game: &State, tick: u32) -> f32 {
    game.tick.saturating_sub(tick) as f32 / TICK_HZ as f32
}

fn facing_rotation(facing: SimVec2) -> Quat {
    Quat::from_rotation_y(facing.x.atan2(facing.y))
}

/// Where a folk stands this frame: blended between ticks, unless the round just dealt it a
/// new spot.
fn folk_position(before: &Folk, now: &Folk, alpha: f32) -> SimVec2 {
    if before.pos.distance(now.pos) > 0.5 {
        now.pos
    } else {
        before.pos.lerp(now.pos, alpha)
    }
}

/// Seats on the side that won the match, once it's over.
fn match_winners(game: &State) -> u8 {
    game.outcome
        .map(|o| o.winners().fold(0u8, |m, s| m | 1 << s.0))
        .unwrap_or(0)
}

#[allow(clippy::type_complexity)]
fn animate_folk(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternMasquerade>>,
    mut folk: Query<(&FolkView, &mut Transform, &mut Pose)>,
    mut markers: Query<(&SeatMarker, &mut Transform, &mut Visibility), Without<FolkView>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let alpha = blend(&fixed);
    let now = time.elapsed_secs();
    let game = &active.run.game;
    let showing = matches!(game.phase, Phase::Result { .. } | Phase::Finished);
    let winners = if game.outcome.is_some() {
        match_winners(game)
    } else {
        game.round_winners
    };
    let mut marked = [None; MAX_PLAYERS];
    for (view, mut transform, mut pose) in &mut folk {
        let (was, is) = (&active.previous.folk[view.index], &game.folk[view.index]);
        let pos = folk_position(was, is, alpha);
        let revealed = game.revealed(view.index);
        let mut rotation = facing_rotation(dir_vec(is.dir));
        let mut lift = 0.0;
        if is.moving && is.standing() {
            // A stroll's bob, the same for everyone.
            let step = now * 7.0 + view.index as f32 * 1.3;
            lift = step.sin().abs() * 0.07;
            rotation *= Quat::from_rotation_z((step * 0.5).sin() * 0.06);
        }
        if is
            .struck_at
            .is_some_and(|tick| since(game, tick) < LUNGE_SECONDS)
        {
            rotation *= Quat::from_rotation_x(0.6);
        }
        if !is.standing() {
            // Lying flat on its back.
            rotation *= Quat::from_rotation_x(-FRAC_PI_2);
            lift = 0.25;
        }
        transform.translation = world(pos) + Vec3::Y * lift;
        transform.rotation = rotation;
        *pose = match revealed {
            Some(seat) if showing && winners & 1 << seat.0 != 0 => Pose::Celebrate,
            Some(_) if showing => Pose::Slump,
            _ => Pose::Idle,
        };

        let flash = is
            .hit_at
            .map(|tick| (1.0 - since(game, tick) / FLASH_SECONDS).max(0.0))
            .unwrap_or(0.0);
        let (color, strength) = match revealed {
            Some(seat) => (seat_color(seat), if is.out { 1.2 } else { 3.0 }),
            None if is.cleared => (CROWD_GLOW, CLEARED_GLOW + flash * 5.0),
            None => (CROWD_GLOW, GLOW + flash * 5.0),
        };
        if let Some(mut material) = materials.get_mut(&view.glow) {
            material.emissive = color * strength;
        }
        if let Some(mut material) = materials.get_mut(&view.body) {
            material.base_color = match revealed {
                Some(seat) => {
                    Color::from(seat_color(seat) * 0.15 + LinearRgba::rgb(0.03, 0.03, 0.04))
                }
                None => CROWD_BODY,
            };
        }
        if let Some(seat) = revealed {
            marked[seat.index()] = Some((world(pos), is.standing()));
        }
    }
    for (marker, mut transform, mut visibility) in &mut markers {
        match marked[marker.0.index()] {
            Some((at, standing)) => {
                transform.translation.x = at.x;
                transform.translation.z = at.z;
                *visibility = Visibility::Inherited;
                // Over the lying body, a little lower.
                transform.scale = Vec3::splat(if standing { 1.0 } else { 0.8 });
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

fn animate_bells(
    time: Res<Time>,
    active: Res<ActiveRun<LanternMasquerade>>,
    bells: Query<&BellView>,
    mut swings: Query<(&BellSwing, &mut Transform)>,
    mut marks: Query<(&BellMark, &mut Visibility)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let now = time.elapsed_secs();
    let game = &active.run.game;
    for (swing, mut transform) in &mut swings {
        let bell = &game.bells[swing.0];
        let pulling = bell.pull as f32 / RING_TICKS as f32;
        let rang = bell
            .rung_at
            .map(|tick| (1.0 - since(game, tick) / 1.5).max(0.0))
            .unwrap_or(0.0);
        let angle = (now * 9.0).sin() * (0.12 + 0.35 * pulling) * (pulling > 0.0) as u8 as f32
            + (now * 12.0).sin() * 0.5 * rang;
        transform.rotation = Quat::from_rotation_z(angle);
    }
    for view in &bells {
        let bell = &game.bells[view.index];
        let rang = bell
            .rung_at
            .map(|tick| (1.0 - since(game, tick)).max(0.0))
            .unwrap_or(0.0);
        let pulling = bell.pull as f32 / RING_TICKS as f32;
        if let Some(mut material) = materials.get_mut(&view.material) {
            material.emissive = BELL_COLOR * (1.5 + 2.0 * pulling + 8.0 * rang);
        }
        if let Some(mut material) = materials.get_mut(&view.ring_material) {
            material.emissive = BELL_COLOR * (0.8 + 2.5 * pulling + 5.0 * rang);
        }
    }
    for (mark, mut visibility) in &mut marks {
        *visibility = if game.bells[mark.bell].rung_by & 1 << mark.seat.0 != 0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

fn animate_sight(
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternMasquerade>>,
    mut sights: Query<(&SightView, &mut Transform)>,
    mut lights: Query<&mut SpotLight>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let game = &active.run.game;
    let Some(seeker) = game.seeker() else {
        return;
    };
    let alpha = blend(&fixed);
    for (view, mut transform) in &mut sights {
        let pos = active.previous.sight.pos.lerp(game.sight.pos, alpha);
        transform.translation = world(pos);
        let dark = game.sight.dark as f32 / DARK_TICKS as f32;
        let lit = if game.can_act() && game.sight.dark == 0 {
            1.0
        } else {
            0.25 + 0.3 * (1.0 - dark)
        };
        if let Some(mut material) = materials.get_mut(&view.material) {
            material.emissive = seat_color(seeker) * 5.0 * lit;
        }
        if let Ok(mut light) = lights.get_mut(view.light) {
            light.intensity = 600_000.0 * lit;
        }
    }
}

const CAMERA_TARGET: Vec3 = Vec3::new(0.0, 0.0, 0.3);
const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 15.5, 15.0);
/// Half-width of the square (plus the stalls) that must stay on screen.
const FRAMED_HALF_WIDTH: f32 = HALF_WIDTH + 2.0;

fn camera_transform(pull_back: f32) -> Transform {
    Transform::from_translation(CAMERA_TARGET + CAMERA_OFFSET * pull_back)
        .looking_at(CAMERA_TARGET, Vec3::Y)
}

/// Pull the camera back on narrow windows so the whole square stays in frame.
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

fn spawn_hud(mut commands: Commands, active: Res<ActiveRun<LanternMasquerade>>) {
    commands
        .spawn((
            SCOPE,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(18.0),
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
                TextFont::from_font_size(46.0),
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
            for seat in game.players.seats() {
                let border = match game.format {
                    Format::TwoVsTwo { .. } | Format::OneVsThree { .. } => {
                        Color::from(team_color(game.team(seat)))
                    }
                    _ => Color::NONE,
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
                    TextFont::from_font_size(18.0),
                    TextColor(Color::WHITE),
                ));
            }
        });
}

fn team_color(team: u8) -> LinearRgba {
    if team == 0 {
        LinearRgba::rgb(1.0, 0.78, 0.3)
    } else {
        LinearRgba::rgb(0.55, 0.45, 1.0)
    }
}

/// Rounds won so far, per side.
fn tally(game: &State) -> String {
    let rounds = |team: u8| game.team_rounds[team as usize] as u32;
    match game.format {
        Format::OneVsThree { .. } => tr!(
            "lantern-masquerade-rounds-solo",
            seeker = rounds(0),
            trio = rounds(1)
        ),
        Format::TwoVsTwo { .. } => (0..2u8)
            .map(|team| {
                let members = game.hiders().filter(|s| game.team(*s) == team);
                format!("{} {}", i18n::names(members), rounds(team))
            })
            .collect::<Vec<_>>()
            .join("  :  "),
        Format::FreeForAll | Format::Duel { .. } => game
            .hiders()
            .map(|s| format!("{} {}", seat_label(s), rounds(game.team(s))))
            .collect::<Vec<_>>()
            .join("   "),
    }
}

/// What ended the round, for the banner.
fn round_result(game: &State) -> String {
    let winners = game
        .playing()
        .filter(|s| game.round_winners & 1 << s.0 != 0);
    let players = i18n::names(winners);
    let solo = game.seeker().is_some();
    match game.ended_by {
        Some(Ending::Bells) if solo => tr!("lantern-masquerade-trio-round"),
        Some(Ending::Bells) => tr!("lantern-masquerade-rang-all", players = players),
        Some(Ending::LastStanding) => tr!("lantern-masquerade-last-standing", players = players),
        Some(Ending::Caught) => tr!("lantern-masquerade-seeker-round"),
        Some(Ending::TimeUp) if solo => tr!("lantern-masquerade-seeker-time"),
        Some(Ending::TimeUp) => tr!("lantern-masquerade-time-up", players = players),
        None => String::new(),
    }
}

fn banner_text(active: &ActiveRun<LanternMasquerade>) -> (String, String) {
    let run = &active.run;
    let game = &run.game;
    if let Some(banner) = halted_banner(run) {
        return banner;
    }
    let round = tr!(
        "lantern-masquerade-round",
        round = game.round as u32 + 1,
        needed = ROUNDS_TO_WIN as u32
    );
    match game.phase {
        // The shared countdown (crate::intro) is drawn over this.
        Phase::Intro { .. } => (String::new(), String::new()),
        Phase::Play { t } if t < SHUFFLE_TICKS => (tr!("lantern-masquerade-shuffle"), round),
        Phase::Play { t } => {
            let left = (game.round_ticks() - t) as u32 / TICK_HZ;
            let title = if t < SHUFFLE_TICKS + 60 {
                if game.seeker().is_some() {
                    tr!("lantern-masquerade-go-solo")
                } else {
                    tr!("lantern-masquerade-go")
                }
            } else {
                tr!("lantern-masquerade-time", seconds = left)
            };
            (title, format!("{round}     {}", tally(game)))
        }
        Phase::Result { .. } if game.outcome.is_none() => (round_result(game), tally(game)),
        Phase::Result { .. } | Phase::Finished => {
            let outcome = game.outcome.expect("the last round has an outcome");
            let winners: Vec<Seat> = outcome.winners().collect();
            let prompt = if game.phase == Phase::Finished {
                active.continue_prompt()
            } else {
                format!("{}     {}", round_result(game), tally(game))
            };
            (i18n::winners(&winners), prompt)
        }
    }
}

fn update_hud(
    active: Res<ActiveRun<LanternMasquerade>>,
    seats: Res<Seats>,
    mut texts: Query<(&HudText, &mut Text)>,
) {
    let (title, subtitle) = banner_text(&active);
    let game = &active.run.game;
    for (hud, mut text) in &mut texts {
        text.0 = match *hud {
            HudText::Banner => title.clone(),
            HudText::SubBanner => subtitle.clone(),
            HudText::SeatPanel(seat) => seat_panel(&active, &seats, game, seat),
        };
    }
}

fn seat_panel(
    active: &ActiveRun<LanternMasquerade>,
    seats: &Seats,
    game: &State,
    seat: Seat,
) -> String {
    let who = bot_tag(seats.is_bot(seat));
    let player = game.seat(seat);
    if !player.playing {
        // Sitting out a duel.
        return format!("{}{who}", seat_label(seat));
    }
    let role = if player.seeker {
        tr!("solo-tag")
    } else {
        String::new()
    };
    let score = if player.seeker {
        tr!("lantern-masquerade-seeker")
    } else {
        let bells = match game.format {
            Format::TwoVsTwo { .. } | Format::OneVsThree { .. } => {
                game.team_bells(game.team(seat)).count_ones()
            }
            _ => player.bells.count_ones(),
        };
        tr!("lantern-masquerade-bells", bells = bells)
    };
    let status = if active.run.controller_lost(seat) {
        tr!("controller-lost")
    } else if let (Some(outcome), Phase::Finished) = (game.outcome, game.phase) {
        outcome.placement[seat.index()]
            .map(|place| tr!("place", place = place as u32))
            .unwrap_or_default()
    } else if player.seeker {
        if game.sight.dark > 0 {
            tr!("lantern-masquerade-lantern-dark")
        } else {
            tr!("lantern-masquerade-lantern-ready")
        }
    } else if player.out {
        tr!("lantern-masquerade-out")
    } else {
        tr!("lantern-masquerade-hiding")
    };
    format!("{}{who}{role}\n{score}\n{status}", seat_label(seat))
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
    fn the_whole_square_is_framed() {
        let camera = camera_transform(1.0);
        let forward = camera.forward();
        // The camera looks down at the square from the front, steeply enough that folk in the
        // back rows aren't hidden behind the front ones.
        assert!(forward.y < -0.65, "{forward:?}");
    }
}
