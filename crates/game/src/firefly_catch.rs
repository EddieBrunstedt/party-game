//! Presentation for Firefly Catch. Reads `ActiveRun<FireflyCatch>` only; never writes
//! simulation state. Fireflies are matched to entities by their simulation id, so spawning,
//! catching and leaving all animate from the state alone.

use std::collections::BTreeSet;
use std::f32::consts::PI;

use bevy::prelude::*;
use bevy_hanabi::prelude::*;
use minigame_firefly_catch::{
    ARENA_HALF_EXTENTS, CAUGHT_LINGER_TICKS, FIREFLY_RADIUS, Firefly, FireflyCatch, FireflyKind,
    FireflyStatus, HOP_TICKS, LEAVING_LINGER_TICKS, NET_COOLDOWN_TICKS, NET_RADIUS, NET_REACH,
    PLAYER_RADIUS, Phase, State, net_reaches,
};
use sim_core::physics::Vec2 as SimVec2;
use sim_core::{Format, Seat, TICK_HZ};

use crate::characters::{Character, Roster};
use crate::flow::AppState;
use crate::i18n::{self, bot_tag, halted_banner, names};
use crate::palette::{LANTERN_WARM, MOONLIGHT, NIGHT_SKY, seat_color, seat_label};
use crate::particles::{self, Effects, hdr};
use crate::runner::{ActiveRun, MinigameSet, Seats};
use crate::scenery::{
    emissive_material, night_camera, noise, spawn_lantern_folk, spawn_paper_lantern,
};
use crate::tr;

pub struct FireflyCatchViewPlugin;

impl Plugin for FireflyCatchViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            (spawn_scene, spawn_hud)
                .after(MinigameSet::Start)
                .run_if(resource_exists::<ActiveRun<FireflyCatch>>),
        )
        .add_systems(
            Update,
            (
                sync_fireflies,
                animate_fireflies,
                animate_players,
                catch_effects,
                fit_camera,
                update_popups,
                update_hud,
            )
                .chain()
                .run_if(resource_exists::<ActiveRun<FireflyCatch>>),
        );
    }
}

const SCOPE: DespawnOnExit<AppState> = DespawnOnExit(AppState::Minigame);
const FIREFLY_GLOW: LinearRgba = LinearRgba::rgb(0.75, 1.0, 0.3);
const GOLDEN_GLOW: LinearRgba = LinearRgba::rgb(1.0, 0.75, 0.15);
const TEAM_COLORS: [LinearRgba; 2] = [
    LinearRgba::rgb(1.0, 0.78, 0.3),
    LinearRgba::rgb(0.55, 0.45, 1.0),
];
const HOVER: f32 = 0.9;

#[derive(Component)]
struct FireflyView {
    id: u32,
    material: Handle<StandardMaterial>,
    light: Entity,
    /// The glowing spot on the ground under it; see [`FireflySpot`].
    spot: Entity,
    generation: u32,
}

/// Marks where a firefly really is on the meadow floor, which is hard to judge from the
/// floating glow alone. It pulses while the firefly is tired, the moment to catch it.
#[derive(Component)]
struct FireflySpot {
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct PlayerView {
    seat: Seat,
    net: Entity,
    disc: Entity,
    /// The ring on the ground that shows exactly where the net catches; see [`NetReach`].
    reach: Entity,
}

/// A firefly whose ground spot touches this ring is caught by a swing. It glows brighter when
/// a catchable firefly is inside, and flashes while the net is open.
#[derive(Component)]
struct NetReach {
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct ScorePopup {
    at: Vec3,
    born: f32,
}

#[derive(Component)]
struct CameraRig;

#[derive(Component)]
enum HudText {
    Banner,
    SubBanner,
    SeatPanel(Seat),
}

fn world(v: SimVec2) -> Vec3 {
    Vec3::new(v.x, 0.0, v.y)
}

fn blend(fixed: &Time<Fixed>) -> f32 {
    fixed.overstep_fraction().clamp(0.0, 1.0)
}

fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    effects: Res<Effects>,
    active: Res<ActiveRun<FireflyCatch>>,
    roster: Res<Roster>,
) {
    commands.insert_resource(ClearColor(NIGHT_SKY));
    commands.insert_resource(GlobalAmbientLight {
        color: MOONLIGHT,
        brightness: 110.0,
        ..default()
    });
    commands.spawn((
        SCOPE,
        night_camera(camera_transform(1.0), (20.0, 44.0)),
        CameraRig,
    ));
    commands.spawn((
        SCOPE,
        DirectionalLight {
            color: MOONLIGHT,
            illuminance: 450.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-5.0, 12.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let half = Vec3::new(ARENA_HALF_EXTENTS.x, 0.0, ARENA_HALF_EXTENTS.y);
    // Meadow floor: grass tiles, a little uneven.
    let tile = meshes.add(Cuboid::new(0.98, 0.4, 0.98));
    let grass: Vec<_> = [(0.09, 0.17, 0.1), (0.075, 0.15, 0.09), (0.11, 0.19, 0.11)]
        .into_iter()
        .map(|(r, g, b)| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                perceptual_roughness: 0.95,
                ..default()
            })
        })
        .collect();
    for i in -(half.x as i32)..(half.x as i32) {
        for j in -(half.z as i32)..(half.z as i32) {
            commands.spawn((
                SCOPE,
                Mesh3d(tile.clone()),
                MeshMaterial3d(grass[(noise(i, j, 50) * 2.999) as usize].clone()),
                Transform::from_xyz(
                    i as f32 + 0.5,
                    -0.2 + noise(i, j, 51) * 0.03,
                    j as f32 + 0.5,
                ),
            ));
        }
    }
    // Hedges round the edge are the walls.
    let hedge = materials.add(StandardMaterial {
        base_color: Color::srgb(0.03, 0.07, 0.045),
        perceptual_roughness: 0.9,
        ..default()
    });
    let trim = materials.add(emissive_material(LANTERN_WARM, 3.0));
    for (pos, size) in [
        (
            Vec3::new(0.0, 0.35, -half.z - 0.4),
            Vec3::new(half.x * 2.0 + 1.6, 0.7, 0.8),
        ),
        (
            Vec3::new(0.0, 0.35, half.z + 0.4),
            Vec3::new(half.x * 2.0 + 1.6, 0.7, 0.8),
        ),
        (
            Vec3::new(-half.x - 0.4, 0.35, 0.0),
            Vec3::new(0.8, 0.7, half.z * 2.0),
        ),
        (
            Vec3::new(half.x + 0.4, 0.35, 0.0),
            Vec3::new(0.8, 0.7, half.z * 2.0),
        ),
    ] {
        commands.spawn((
            SCOPE,
            Mesh3d(meshes.add(Cuboid::from_size(size))),
            MeshMaterial3d(hedge.clone()),
            Transform::from_translation(pos),
        ));
        // A string of warm lights along the top marks the edge of play.
        commands.spawn((
            SCOPE,
            Mesh3d(meshes.add(Cuboid::from_size(if size.x > size.z {
                Vec3::new(size.x, 0.08, 0.12)
            } else {
                Vec3::new(0.12, 0.08, size.z)
            }))),
            MeshMaterial3d(trim.clone()),
            Transform::from_translation(pos + Vec3::Y * 0.39),
        ));
    }
    for (pos, color) in [
        (
            Vec3::new(-half.x - 1.2, 0.0, -half.z - 1.2),
            LinearRgba::rgb(1.0, 0.3, 0.55),
        ),
        (
            Vec3::new(half.x + 1.2, 0.0, -half.z - 1.2),
            LinearRgba::rgb(0.25, 0.75, 1.0),
        ),
        (Vec3::new(0.0, 0.0, -half.z - 1.4), LANTERN_WARM),
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
    commands.spawn((
        SCOPE,
        ParticleEffect::new(effects.motes.clone()),
        Transform::default(),
    ));

    let game = &active.run.game;
    for seat in game.setup.seats() {
        spawn_player(
            &mut commands,
            &mut meshes,
            &mut materials,
            game,
            seat,
            roster.of(seat),
        );
    }
}

fn disc_color(format: &Format, seat: Seat) -> LinearRgba {
    match format {
        Format::FreeForAll | Format::Duel { .. } => seat_color(seat),
        _ => format
            .team(seat)
            .map(|t| TEAM_COLORS[t as usize])
            .unwrap_or(seat_color(seat)),
    }
}

fn spawn_player(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    game: &State,
    seat: Seat,
    character: Character,
) {
    let player = &game.players[seat.index()];
    let folk = spawn_lantern_folk(
        commands,
        meshes,
        materials,
        AppState::Minigame,
        seat,
        character,
        Transform::from_translation(world(player.body.pos))
            .with_scale(Vec3::splat(PLAYER_RADIUS / 0.5)),
    );
    // The net: a pole with a glowing hoop, held out front. It swings when A is pressed.
    let hoop = materials.add(emissive_material(seat_color(seat), 3.0));
    let pole = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.06, 0.04),
        ..default()
    });
    let net = commands
        .spawn((Transform::from_xyz(0.45, 0.7, 0.0), Visibility::default()))
        .with_children(|net| {
            net.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.06, 0.06, 0.9))),
                MeshMaterial3d(pole),
                Transform::from_xyz(0.0, 0.0, 0.45),
            ));
            net.spawn((
                Mesh3d(meshes.add(Torus::new(0.26, 0.32))),
                MeshMaterial3d(hoop),
                Transform::from_xyz(0.0, 0.0, 1.1).with_rotation(Quat::from_rotation_x(PI / 2.0)),
            ));
        })
        .id();
    commands.entity(folk.root).add_child(net);
    // The catch condition is the firefly's centre within this distance of the net's centre,
    // so the ring is drawn at exactly that radius.
    let reach_radius = NET_RADIUS + FIREFLY_RADIUS;
    let reach_material = materials.add(emissive_material(seat_color(seat), REACH_IDLE));
    let reach = commands
        .spawn((
            SCOPE,
            NetReach {
                material: reach_material.clone(),
            },
            Mesh3d(
                meshes.add(
                    Torus::new(reach_radius - 0.05, reach_radius)
                        .mesh()
                        .major_resolution(48),
                ),
            ),
            MeshMaterial3d(reach_material),
            Transform::from_translation(world(player.net_centre(player.facing)) + Vec3::Y * 0.04),
        ))
        .id();
    let disc = commands
        .spawn((
            SCOPE,
            Mesh3d(meshes.add(Cylinder::new(PLAYER_RADIUS, 0.04))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::BLACK,
                emissive: disc_color(&game.format, seat) * 2.0,
                ..default()
            })),
            Transform::from_translation(world(player.body.pos) + Vec3::Y * 0.03),
        ))
        .id();
    commands.entity(folk.root).insert(PlayerView {
        seat,
        net,
        disc,
        reach,
    });
}

/// Spawn views for new fireflies and remove views whose firefly is gone.
fn sync_fireflies(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    effects: Res<Effects>,
    active: Res<ActiveRun<FireflyCatch>>,
    views: Query<(Entity, &FireflyView)>,
    mut mesh: Local<Option<(Handle<Mesh>, Handle<Mesh>)>>,
) {
    let game = &active.run.game;
    for (entity, view) in &views {
        let alive =
            view.generation == active.generation && game.fireflies.iter().any(|f| f.id == view.id);
        if !alive {
            commands.entity(entity).despawn();
            commands.entity(view.spot).despawn();
        }
    }
    let (mesh, spot_mesh) = mesh
        .get_or_insert_with(|| {
            (
                meshes.add(Sphere::new(0.14).mesh().ico(2).expect("small ico")),
                meshes.add(Cylinder::new(FIREFLY_RADIUS, 0.02)),
            )
        })
        .clone();
    for firefly in &game.fireflies {
        if views
            .iter()
            .any(|(_, v)| v.id == firefly.id && v.generation == active.generation)
        {
            continue;
        }
        let glow = glow_color(firefly.kind);
        let material = materials.add(StandardMaterial {
            base_color: Color::BLACK,
            emissive: glow * 8.0,
            ..default()
        });
        let light = commands
            .spawn((
                PointLight {
                    color: Color::from(glow),
                    intensity: 30_000.0,
                    range: 3.0,
                    ..default()
                },
                Transform::default(),
            ))
            .id();
        let trail = commands
            .spawn((
                ParticleEffect::new(effects.trail.clone()),
                particles::colored(hdr(glow, 4.0)),
                Transform::default(),
            ))
            .id();
        let scale = if firefly.kind == FireflyKind::Golden {
            1.5
        } else {
            1.0
        };
        let spot_material = materials.add(emissive_material(glow, SPOT_GLOW));
        let spot = commands
            .spawn((
                SCOPE,
                FireflySpot {
                    material: spot_material.clone(),
                },
                Mesh3d(spot_mesh.clone()),
                MeshMaterial3d(spot_material),
                Transform::from_translation(world(firefly.body.pos) + Vec3::Y * 0.03)
                    .with_scale(Vec3::ZERO),
            ))
            .id();
        commands
            .spawn((
                SCOPE,
                FireflyView {
                    id: firefly.id,
                    material: material.clone(),
                    light,
                    spot,
                    generation: active.generation,
                },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(world(firefly.body.pos) + Vec3::Y * HOVER)
                    .with_scale(Vec3::splat(scale)),
            ))
            .add_children(&[light, trail]);
    }
}

fn glow_color(kind: FireflyKind) -> LinearRgba {
    match kind {
        FireflyKind::Normal => FIREFLY_GLOW,
        FireflyKind::Golden => GOLDEN_GLOW,
    }
}

fn find(game: &State, id: u32) -> Option<&Firefly> {
    game.fireflies.iter().find(|f| f.id == id)
}

fn animate_fireflies(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<FireflyCatch>>,
    mut views: Query<(&FireflyView, &mut Transform)>,
    mut spots: Query<(&FireflySpot, &mut Transform), Without<FireflyView>>,
    mut lights: Query<&mut PointLight>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let alpha = blend(&fixed);
    let now = time.elapsed_secs();
    let game = &active.run.game;
    for (view, mut transform) in &mut views {
        let Some(firefly) = find(game, view.id) else {
            continue;
        };
        let before = find(&active.previous, view.id).unwrap_or(firefly);
        let pos = world(before.body.pos.lerp(firefly.body.pos, alpha));
        let phase = view.id as f32 * 1.7;
        let base_scale = if firefly.kind == FireflyKind::Golden {
            1.5
        } else {
            1.0
        };
        let tick = game.tick as f32 + alpha;
        let fade_in = ((tick - firefly.born as f32) / 30.0).clamp(0.0, 1.0);

        if let Ok((spot, mut spot_transform)) = spots.get_mut(view.spot) {
            spot_transform.translation = pos + Vec3::Y * 0.03;
            let (size, glow) = match firefly.status {
                FireflyStatus::Flying { .. } if firefly.tired => {
                    let pulse = (now * 8.0 + phase).sin() * 0.5 + 0.5;
                    (1.3 + pulse * 0.6, SPOT_TIRED_GLOW * (0.6 + 0.4 * pulse))
                }
                FireflyStatus::Flying { .. } => (1.0, SPOT_GLOW),
                _ => (0.0, 0.0),
            };
            spot_transform.scale = Vec3::new(size, 1.0, size) * base_scale * fade_in;
            if let Some(mut material) = materials.get_mut(&spot.material) {
                material.emissive = glow_color(firefly.kind) * glow;
            }
        }

        let (lift, scale, glow) = match firefly.status {
            FireflyStatus::Flying { .. } => {
                // A hop is a high arc over the chaser's head.
                let hop = if firefly.hop > 0 {
                    let t = 1.0 - (firefly.hop as f32 - alpha).max(0.0) / HOP_TICKS as f32;
                    (t * PI).sin() * 1.6
                } else {
                    0.0
                };
                let bob = (now * 3.0 + phase).sin() * 0.12;
                // Tired fireflies sag and flicker: that's the moment to catch them.
                let (sag, glow) = if firefly.tired {
                    let flicker = 0.35 + 0.35 * (now * 14.0 + phase).sin().abs();
                    (-0.35, flicker)
                } else {
                    (0.0, 1.0)
                };
                (
                    HOVER + bob + hop + sag,
                    base_scale * fade_in,
                    glow * fade_in,
                )
            }
            FireflyStatus::Caught { by, at } => {
                // Zip into the catcher's lantern head and shrink away.
                let t = ((tick - at as f32) / CAUGHT_LINGER_TICKS as f32).clamp(0.0, 1.0);
                let catcher = world(game.players[by.index()].body.pos) + Vec3::Y * 1.3;
                transform.translation = pos.lerp(catcher, t * t) + Vec3::Y * HOVER * (1.0 - t);
                transform.scale = Vec3::splat(base_scale * (1.0 - t));
                set_glow(
                    view,
                    &mut materials,
                    &mut lights,
                    firefly.kind,
                    1.0 + t * 2.0,
                );
                continue;
            }
            FireflyStatus::Leaving { at } => {
                let t = ((tick - at as f32) / LEAVING_LINGER_TICKS as f32).clamp(0.0, 1.0);
                (HOVER + t * t * 5.0, base_scale * (1.0 - t * 0.5), 1.0 - t)
            }
        };
        transform.translation = pos + Vec3::Y * lift;
        transform.scale = Vec3::splat(scale.max(0.001));
        set_glow(view, &mut materials, &mut lights, firefly.kind, glow);
    }
}

fn set_glow(
    view: &FireflyView,
    materials: &mut Assets<StandardMaterial>,
    lights: &mut Query<&mut PointLight>,
    kind: FireflyKind,
    glow: f32,
) {
    if let Some(mut material) = materials.get_mut(&view.material) {
        material.emissive = glow_color(kind) * (8.0 * glow);
    }
    if let Ok(mut light) = lights.get_mut(view.light) {
        light.intensity = 30_000.0 * glow;
    }
}

const SWING_SECS: f32 = 0.25;
const SPOT_GLOW: f32 = 1.2;
const SPOT_TIRED_GLOW: f32 = 6.0;
const REACH_IDLE: f32 = 0.5;
const REACH_READY: f32 = 3.0;
const REACH_SWINGING: f32 = 6.0;

fn animate_players(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<FireflyCatch>>,
    mut players: Query<(&PlayerView, &mut Transform)>,
    mut parts: Query<&mut Transform, Without<PlayerView>>,
    reaches: Query<&NetReach>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let alpha = blend(&fixed);
    let now = time.elapsed_secs();
    let game = &active.run.game;
    let winners: Vec<Seat> = if game.phase == Phase::Finished {
        sim_core::Outcome::from_team_scores(&game.setup, &game.format, &game.team_scores())
            .winners()
            .collect()
    } else {
        Vec::new()
    };
    for (view, mut transform) in &mut players {
        let player = &game.players[view.seat.index()];
        let before = &active.previous.players[view.seat.index()];
        let pos = world(before.body.pos.lerp(player.body.pos, alpha));
        let hop = if winners.contains(&view.seat) {
            (now * 9.0 + view.seat.0 as f32).sin().abs() * 0.4
        } else {
            0.0
        };
        transform.translation = pos + Vec3::Y * hop;
        let mut rotation = Quat::from_rotation_y(player.facing.x.atan2(player.facing.y));
        if player.dash_ticks > 0 {
            rotation *= Quat::from_rotation_x(0.4);
        }
        transform.rotation = rotation;

        // Swing: the net sweeps from the side to straight ahead and down.
        let since_swing = player
            .last_swing
            .map(|tick| (game.tick as f32 + alpha - tick as f32) / TICK_HZ as f32)
            .unwrap_or(f32::MAX);
        if let Ok(mut net) = parts.get_mut(view.net) {
            let t = (since_swing / SWING_SECS).clamp(0.0, 1.0);
            let (yaw, pitch) = if since_swing < SWING_SECS {
                (0.9 * (1.0 - t) - 0.5 * t, 0.6 * (t * PI).sin())
            } else {
                // At rest the net is held up, ready; it droops a little while cooling down.
                let cooling = player.net_cooldown as f32 / NET_COOLDOWN_TICKS as f32;
                (0.5, -0.5 + cooling * 0.3)
            };
            net.rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch);
        }
        if let Ok(mut disc) = parts.get_mut(view.disc) {
            disc.translation = pos + Vec3::Y * 0.03;
        }

        // The reach ring follows the net and lights up when a swing would catch something.
        let facing = world(player.facing);
        if let Ok(mut ring) = parts.get_mut(view.reach) {
            ring.translation = pos + facing * NET_REACH + Vec3::Y * 0.04;
        }
        let net = player.net_centre(player.facing);
        let ready = game
            .flying()
            .any(|f| f.catchable() && net_reaches(net, f.body.pos));
        let glow = if player.swing_ticks > 0 {
            REACH_SWINGING
        } else if ready && player.net_cooldown == 0 {
            REACH_READY * (0.8 + 0.2 * (now * 12.0).sin())
        } else {
            REACH_IDLE
        };
        if let Ok(reach) = reaches.get(view.reach)
            && let Some(mut material) = materials.get_mut(&reach.material)
        {
            material.emissive = seat_color(view.seat) * glow;
        }
    }
}

/// Bursts of sparks and score popups the moment a firefly is caught.
fn catch_effects(
    mut commands: Commands,
    time: Res<Time>,
    effects: Res<Effects>,
    active: Res<ActiveRun<FireflyCatch>>,
    mut shown: Local<(u32, BTreeSet<u32>)>,
) {
    let game = &active.run.game;
    // Catches come in any order, so remember exactly which ones have had their effect. A new
    // run (a fresh resource, or a restart) starts over.
    if active.is_added() || shown.0 != active.generation {
        *shown = (active.generation, BTreeSet::new());
    }
    for firefly in &game.fireflies {
        let FireflyStatus::Caught { by, .. } = firefly.status else {
            continue;
        };
        if !shown.1.insert(firefly.id) {
            continue;
        }
        let at = world(firefly.body.pos) + Vec3::Y * HOVER;
        particles::burst(&mut commands, &effects, SCOPE, at, hdr(seat_color(by), 6.0));
        particles::burst(
            &mut commands,
            &effects,
            SCOPE,
            at,
            hdr(glow_color(firefly.kind), 5.0),
        );
        let points = firefly.kind.points() * game.multiplier(by);
        commands.spawn((
            SCOPE,
            ScorePopup {
                at: at + Vec3::Y * 0.5,
                born: time.elapsed_secs(),
            },
            Text::new(format!("+{points}")),
            TextFont::from_font_size(if points > 1 { 40.0 } else { 30.0 }),
            TextColor(Color::from(seat_color(by) * 1.4)),
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
        ));
    }
}

fn update_popups(
    mut commands: Commands,
    time: Res<Time>,
    camera: Query<(&Camera, &GlobalTransform), With<CameraRig>>,
    mut popups: Query<(Entity, &ScorePopup, &mut Node, &mut TextColor)>,
) {
    let Some((camera, camera_transform)) = camera.iter().next() else {
        return;
    };
    let now = time.elapsed_secs();
    for (entity, popup, mut node, mut color) in &mut popups {
        let age = now - popup.born;
        if age > 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        if let Ok(at) = camera.world_to_viewport(camera_transform, popup.at + Vec3::Y * age) {
            node.left = Val::Px(at.x - 14.0);
            node.top = Val::Px(at.y);
        }
        color.0 = color.0.with_alpha(1.0 - age);
    }
}

const CAMERA_TARGET: Vec3 = Vec3::new(0.0, 0.0, 0.9);
const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 11.5, 9.0);
const FRAMED_HALF_WIDTH: f32 = 10.0;

fn camera_transform(pull_back: f32) -> Transform {
    Transform::from_translation(CAMERA_TARGET + CAMERA_OFFSET * pull_back)
        .looking_at(CAMERA_TARGET, Vec3::Y)
}

fn fit_camera(
    windows: Query<&Window>,
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

fn spawn_hud(mut commands: Commands, active: Res<ActiveRun<FireflyCatch>>) {
    commands
        .spawn((
            SCOPE,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(26.0),
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
                TextFont::from_font_size(52.0),
                TextColor(Color::srgb(1.0, 0.93, 0.8)),
                TextLayout::justify(Justify::Center),
            ));
            top.spawn((
                HudText::SubBanner,
                Text::new(""),
                TextFont::from_font_size(20.0),
                TextColor(Color::srgba(0.85, 0.88, 1.0, 0.9)),
                TextLayout::justify(Justify::Center),
            ));
        });
    let game = &active.run.game;
    commands
        .spawn((
            SCOPE,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(16.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(16.0),
                ..default()
            },
        ))
        .with_children(|row| {
            for seat in game.setup.seats() {
                let border = match game.format {
                    Format::FreeForAll | Format::Duel { .. } => Color::NONE,
                    _ => Color::from(disc_color(&game.format, seat)),
                };
                row.spawn((
                    Node {
                        width: Val::Px(200.0),
                        padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
                        border: UiRect::all(Val::Px(3.0)),
                        ..default()
                    },
                    BorderColor::all(border),
                    BackgroundColor(Color::from(seat_color(seat) * 0.25).with_alpha(0.6)),
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

fn banner_text(active: &ActiveRun<FireflyCatch>) -> (String, String) {
    let run = &active.run;
    let game = &run.game;
    if let Some(banner) = halted_banner(run) {
        return banner;
    }
    match game.phase {
        // The shared countdown (crate::intro) is drawn over this.
        Phase::Intro { .. } => (String::new(), String::new()),
        Phase::Playing { ticks_left } => {
            let secs = (ticks_left as u32).div_ceil(TICK_HZ);
            let title = if secs <= 5 {
                secs.to_string()
            } else if (PLAY_SECS - secs) < 1 {
                tr!("firefly-catch-go")
            } else {
                String::new()
            };
            let clock = format!("0:{secs:02}");
            let subtitle = match game.format {
                Format::FreeForAll | Format::Duel { .. } => clock,
                _ => {
                    let scores = game.team_scores();
                    let side = |team: u8| {
                        names(
                            game.setup
                                .seats()
                                .filter(|s| game.format.team(*s) == Some(team)),
                        )
                    };
                    format!(
                        "{clock}     {} {}  :  {} {}",
                        side(0),
                        scores[0],
                        scores[1],
                        side(1)
                    )
                }
            };
            (title, subtitle)
        }
        Phase::Finished => {
            let outcome = run.outcome().expect("finished games have an outcome");
            let winners: Vec<Seat> = outcome.winners().collect();
            (i18n::winners(&winners), active.continue_prompt())
        }
    }
}

const PLAY_SECS: u32 = minigame_firefly_catch::PLAY_TICKS as u32 / TICK_HZ;

fn update_hud(
    active: Res<ActiveRun<FireflyCatch>>,
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
                    Format::OneVsThree { solo } if solo == seat => tr!("firefly-catch-triple-tag"),
                    _ => String::new(),
                };
                let player = &game.players[seat.index()];
                let status = if active.run.controller_lost(seat) {
                    tr!("controller-lost")
                } else {
                    tr!("points", points = player.score)
                };
                format!("{}{who}{role}\n{status}", seat_label(seat))
            }
        };
    }
}
