//! Presentation for Lantern Lottery. Reads `ActiveRun<LanternLottery>` only; never writes
//! simulation state. Four paper lanterns float in a diamond (up, right, down, left, as the stick
//! points) over the lantern folk. While picking, the screen shows who has locked in and never
//! where: nothing here reads a pick, or a moth, before the reveal. Then every seat's marker flies
//! to its lantern (seekers below it, hiders above), and the lanterns open in the simulation's
//! order.

use std::f32::consts::PI;

use bevy::camera::ScalingMode;
use bevy::prelude::*;
use minigame_lantern_lottery::{Dir, LanternLottery, Mode, PICK_TICKS, Phase, State, open_tick};
use sim_core::{Format, Seat, TICK_HZ};

use crate::audio::{Sfx, Sound};
use crate::characters::{Pose, Roster};
use crate::flow::AppState;
use crate::i18n::{self, bot_tag, halted_banner, names};
use crate::palette::{LANTERN_WARM, MOONLIGHT, NIGHT_SKY, seat_color, seat_label};
use crate::particles::{self, Effects, hdr};
use crate::runner::{ActiveRun, MinigameSet, Seats};
use crate::scenery::{
    emissive_material, flat_marker_mesh, night_camera, noise, spawn_fireflies, spawn_lantern_folk,
};
use crate::tr;

pub struct LanternLotteryViewPlugin;

impl Plugin for LanternLotteryViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            (spawn_scene, spawn_hud)
                .after(MinigameSet::Start)
                .run_if(resource_exists::<ActiveRun<LanternLottery>>),
        )
        .add_systems(
            Update,
            (
                animate_lanterns,
                animate_moths,
                animate_markers,
                animate_folk,
                place_labels,
                update_hud,
            )
                .run_if(resource_exists::<ActiveRun<LanternLottery>>),
        )
        .add_systems(
            FixedUpdate,
            react
                .in_set(MinigameSet::React)
                .run_if(resource_exists::<ActiveRun<LanternLottery>>),
        );
    }
}

const SCOPE: DespawnOnExit<AppState> = DespawnOnExit(AppState::Minigame);
/// The diamond's centre and how far its lanterns stand from it.
const DIAMOND: Vec2 = Vec2::new(0.0, 0.8);
const SPREAD: Vec2 = Vec2::new(3.4, 2.3);
const LANTERN_RADIUS: f32 = 0.8;
/// Revealed markers sit in a row this far beyond a lantern's edge: seekers below, hiders above.
const ROW_GAP: f32 = 0.45;
const MARKER_GAP: f32 = 0.55;
const MARKER_SCALE: f32 = 1.8;
/// How long a marker takes to fly from its player to its lantern at the reveal.
const FLY_TICKS: f32 = 24.0;
/// The folk stand on the ground below the diamond.
const FOLK_Y: f32 = -5.6;
const FOLK_GAP: f32 = 3.0;
/// Depths: lanterns, then the moths in front of them, then the markers.
const MOTH_Z: f32 = 1.0;
const MARKER_Z: f32 = 2.0;
const FOLK_Z: f32 = 1.5;
const PAPER: LinearRgba = LinearRgba::rgb(1.0, 0.82, 0.6);
const MOTH_GREY: LinearRgba = LinearRgba::rgb(0.45, 0.35, 0.55);
const STAR_COUNT: i32 = 45;
const FIREFLY_COUNT: usize = 16;
/// Team colours for the panel borders in 2v2.
const TEAM_COLORS: [LinearRgba; 2] = [
    LinearRgba::rgb(1.0, 0.78, 0.3),
    LinearRgba::rgb(0.55, 0.45, 1.0),
];

#[derive(Component)]
struct LanternView {
    dir: Dir,
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct MothView(Dir);

/// A seat's marker, shown at its lantern from the reveal on.
#[derive(Component)]
struct MarkerView(Seat);

#[derive(Component)]
struct FolkView {
    seat: Seat,
    head_material: Handle<StandardMaterial>,
    head_light: Entity,
}

#[derive(Component)]
struct CameraRig;

/// UI text pinned to a lantern: its fireflies on it, and what it paid under it.
#[derive(Component)]
enum LanternLabel {
    Fireflies(Dir),
    Result(Dir),
}

#[derive(Component)]
enum HudText {
    Banner,
    SubBanner,
    SeatPanel(Seat),
}

fn dir_offset(dir: Dir) -> Vec2 {
    match dir {
        Dir::Up => Vec2::Y,
        Dir::Right => Vec2::X,
        Dir::Down => Vec2::NEG_Y,
        Dir::Left => Vec2::NEG_X,
    }
}

fn lantern_pos(dir: Dir) -> Vec2 {
    DIAMOND + dir_offset(dir) * SPREAD
}

/// Seats that play, in seat order: everyone but those outside a duel.
fn playing(game: &State) -> Vec<Seat> {
    game.playing().collect()
}

fn folk_x(game: &State, seat: Seat) -> f32 {
    let seats = playing(game);
    let index = seats.iter().position(|s| *s == seat).unwrap_or(0) as f32;
    (index - (seats.len() as f32 - 1.0) / 2.0) * FOLK_GAP
}

fn team_index(format: &Format, seat: Seat) -> Option<u8> {
    match format {
        Format::TwoVsTwo { .. } => format.team(seat),
        _ => None,
    }
}

/// Ticks since a lantern opened this round, once it has.
fn since_open(game: &State, dir: Dir) -> Option<u16> {
    let k = game.order.iter().position(|d| *d == dir)?;
    match game.phase {
        Phase::Reveal { t } if t > open_tick(k) => Some(t - open_tick(k)),
        Phase::Decided { .. } | Phase::Finished => Some(u16::MAX),
        _ => None,
    }
}

/// Where a seat's marker sits once picks are revealed: in a row under its lantern, or over it
/// for those hiding a moth.
fn marker_slot(game: &State, seat: Seat) -> Option<Vec2> {
    let picker = game.picker(seat);
    let dir = picker.pick?;
    let row: Vec<Seat> = if picker.hiding {
        game.hiders()
            .filter(|s| game.picker(*s).pick == Some(dir))
            .collect()
    } else {
        game.pickers_of(dir).collect()
    };
    let index = row.iter().position(|s| *s == seat)? as f32;
    let x = (index - (row.len() as f32 - 1.0) / 2.0) * MARKER_GAP;
    let side = if picker.hiding { 1.0 } else { -1.0 };
    Some(lantern_pos(dir) + Vec2::new(x, side * (LANTERN_RADIUS + ROW_GAP)))
}

fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    active: Res<ActiveRun<LanternLottery>>,
    roster: Res<Roster>,
) {
    commands.insert_resource(ClearColor(NIGHT_SKY));
    commands.insert_resource(GlobalAmbientLight {
        color: MOONLIGHT,
        brightness: 60.0,
        ..default()
    });
    commands.spawn((
        SCOPE,
        night_camera(camera_transform(), (40.0, 80.0)),
        camera_projection(),
        CameraRig,
    ));
    commands.spawn((
        SCOPE,
        DirectionalLight {
            color: MOONLIGHT,
            illuminance: 500.0,
            ..default()
        },
        Transform::from_xyz(4.0, 6.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let star = meshes.add(Circle::new(0.04));
    let star_material = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: LinearRgba::rgb(0.8, 0.85, 1.0) * 0.9,
        ..default()
    });
    for k in 0..STAR_COUNT {
        let x = (noise(k, 1, 40) * 2.0 - 1.0) * 12.0;
        let y = FOLK_Y + 2.0 + noise(k, 1, 41) * 12.0;
        commands.spawn((
            SCOPE,
            Mesh3d(star.clone()),
            MeshMaterial3d(star_material.clone()),
            Transform::from_xyz(x, y, -8.0).with_scale(Vec3::splat(0.6 + noise(k, 1, 42))),
        ));
    }
    let ground = materials.add(StandardMaterial {
        base_color: Color::srgb(0.03, 0.035, 0.05),
        perceptual_roughness: 0.95,
        ..default()
    });
    commands.spawn((
        SCOPE,
        Mesh3d(meshes.add(Cuboid::new(40.0, 4.0, 1.0))),
        MeshMaterial3d(ground),
        Transform::from_xyz(0.0, FOLK_Y - 2.0, 0.0),
    ));
    spawn_fireflies(
        &mut commands,
        &mut meshes,
        &mut materials,
        AppState::Minigame,
        FIREFLY_COUNT,
        Vec3::new(-10.0, FOLK_Y + 0.3, FOLK_Z - 0.5),
        Vec3::new(10.0, FOLK_Y + 2.0, FOLK_Z + 0.5),
    );

    let body = meshes.add(Sphere::new(LANTERN_RADIUS).mesh().uv(24, 16));
    let cap = meshes.add(Cuboid::new(
        LANTERN_RADIUS * 1.1,
        LANTERN_RADIUS * 0.22,
        LANTERN_RADIUS * 1.1,
    ));
    let cap_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.05, 0.03),
        emissive: LANTERN_WARM * 0.15,
        perceptual_roughness: 0.9,
        ..default()
    });
    // A big forewing and a smaller hindwing on each side.
    let wings = [
        meshes.add(Triangle2d::new(
            Vec2::new(0.0, 0.05),
            Vec2::new(0.6, 0.45),
            Vec2::new(0.5, -0.1),
        )),
        meshes.add(Triangle2d::new(
            Vec2::new(0.0, -0.02),
            Vec2::new(0.38, -0.08),
            Vec2::new(0.24, -0.42),
        )),
    ];
    let moth_body = meshes.add(Capsule2d::new(0.07, 0.4));
    let moth_material = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: MOTH_GREY * 1.5,
        ..default()
    });
    for dir in Dir::ALL {
        let material = materials.add(emissive_material(PAPER, 0.4));
        let at = lantern_pos(dir).extend(0.0);
        commands
            .spawn((
                SCOPE,
                LanternView {
                    dir,
                    material: material.clone(),
                },
                Mesh3d(body.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(at),
            ))
            .with_children(|lantern| {
                for y in [-0.95, 0.95] {
                    lantern.spawn((
                        Mesh3d(cap.clone()),
                        MeshMaterial3d(cap_material.clone()),
                        Transform::from_xyz(0.0, y * LANTERN_RADIUS, 0.0),
                    ));
                }
            });
        commands
            .spawn((
                SCOPE,
                MothView(dir),
                Transform::from_translation(at.with_z(MOTH_Z)),
                Visibility::Hidden,
            ))
            .with_children(|moth| {
                moth.spawn((
                    Mesh3d(moth_body.clone()),
                    MeshMaterial3d(moth_material.clone()),
                    Transform::default(),
                ));
                for side in [1.0, -1.0] {
                    for wing in &wings {
                        moth.spawn((
                            Mesh3d(wing.clone()),
                            MeshMaterial3d(moth_material.clone()),
                            Transform::from_scale(Vec3::new(side, 1.0, 1.0)),
                        ));
                    }
                }
            });
    }

    let game = &active.run.game;
    for seat in playing(game) {
        let folk = spawn_lantern_folk(
            &mut commands,
            &mut meshes,
            &mut materials,
            AppState::Minigame,
            seat,
            roster.of(seat),
            Transform::from_xyz(folk_x(game, seat), FOLK_Y, FOLK_Z).with_scale(Vec3::splat(0.85)),
        );
        commands.entity(folk.root).insert(FolkView {
            seat,
            head_material: folk.head_material,
            head_light: folk.head_light,
        });
        let marker_material = materials.add(StandardMaterial {
            base_color: Color::BLACK,
            emissive: seat_color(seat) * 5.0,
            ..default()
        });
        commands.spawn((
            SCOPE,
            MarkerView(seat),
            Mesh3d(flat_marker_mesh(&mut meshes, seat)),
            MeshMaterial3d(marker_material),
            Transform::from_scale(Vec3::splat(MARKER_SCALE)),
            Visibility::Hidden,
        ));
    }
}

/// How a lantern glows: dim paper while closed, bright for fireflies once open, dark for a moth.
fn lantern_glow(game: &State, dir: Dir, time: f32) -> (LinearRgba, f32) {
    let Some(since) = since_open(game, dir) else {
        let breathe = 0.08 * (time * 2.0 + dir.index() as f32).sin();
        return match game.phase {
            Phase::Intro { .. } => (PAPER, 0.25),
            _ => (PAPER, 0.6 + breathe),
        };
    };
    let flare = (-(since.min(600) as f32) / 20.0).exp();
    if game.moths[dir.index()] {
        (MOTH_GREY, 0.2 + 2.0 * flare)
    } else if game.mode == Mode::SoloHides {
        // 1v3: no fireflies, just a lantern without the moth.
        (PAPER, 0.9)
    } else if game.pickers_of(dir).next().is_some() {
        (LANTERN_WARM, 3.0 + 6.0 * flare)
    } else {
        (PAPER, 1.2 + 2.0 * flare)
    }
}

fn animate_lanterns(
    time: Res<Time>,
    active: Res<ActiveRun<LanternLottery>>,
    mut views: Query<(&LanternView, &mut Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let game = &active.run.game;
    let now = time.elapsed_secs();
    for (view, mut transform) in &mut views {
        // A slight sway, for paper in the wind.
        transform.rotation =
            Quat::from_rotation_z(0.06 * (now * 1.1 + view.dir.index() as f32).sin());
        let (color, glow) = lantern_glow(game, view.dir, now);
        if let Some(mut material) = materials.get_mut(&view.material) {
            material.emissive = color * glow;
        }
    }
}

fn animate_moths(
    time: Res<Time>,
    active: Res<ActiveRun<LanternLottery>>,
    mut moths: Query<(&MothView, &mut Transform, &mut Visibility)>,
) {
    let game = &active.run.game;
    let now = time.elapsed_secs();
    for (MothView(dir), mut transform, mut visibility) in &mut moths {
        let Some(since) = since_open(game, *dir).filter(|_| game.moths[dir.index()]) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *visibility = Visibility::Inherited;
        // It bursts out of the lantern, then flutters in front of it.
        let out = (since.min(600) as f32 / 12.0).min(1.0);
        let flap = 0.75 + 0.25 * (now * 18.0).sin();
        transform.scale = Vec3::new(out * flap, out, 1.0) * 1.6;
        transform.translation =
            (lantern_pos(*dir) + Vec2::new(0.0, 0.15 * (now * 3.0).sin())).extend(MOTH_Z);
    }
}

fn animate_markers(
    active: Res<ActiveRun<LanternLottery>>,
    fixed: Res<Time<Fixed>>,
    mut markers: Query<(&MarkerView, &mut Transform, &mut Visibility)>,
) {
    let game = &active.run.game;
    let alpha = fixed.overstep_fraction().clamp(0.0, 1.0);
    let flown = match game.phase {
        Phase::Reveal { t } => ((t as f32 + alpha) / FLY_TICKS).min(1.0),
        Phase::Decided { .. } | Phase::Finished => 1.0,
        _ => 0.0,
    };
    for (MarkerView(seat), mut transform, mut visibility) in &mut markers {
        let slot = marker_slot(game, *seat).filter(|_| game.revealed());
        let Some(slot) = slot else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *visibility = Visibility::Inherited;
        // From over the player's head, on a little arc.
        let from = Vec2::new(folk_x(game, *seat), FOLK_Y + 2.2);
        let ease = flown * flown * (3.0 - 2.0 * flown);
        let arc = (ease * PI).sin() * 0.8;
        transform.translation = (from.lerp(slot, ease) + Vec2::Y * arc).extend(MARKER_Z);
    }
}

fn animate_folk(
    time: Res<Time>,
    active: Res<ActiveRun<LanternLottery>>,
    mut folk: Query<(&FolkView, &mut Pose)>,
    mut lights: Query<&mut PointLight>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let game = &active.run.game;
    let now = time.elapsed_secs();
    let winners: Vec<Seat> = game
        .outcome
        .map(|o| o.winners().collect())
        .unwrap_or_default();
    for (view, mut pose) in &mut folk {
        let seat = view.seat;
        let picker = game.picker(seat);
        let opened = picker
            .pick
            .is_some_and(|dir| since_open(game, dir).is_some());
        *pose = if game.outcome.is_some() && game.opened_count() == 4 {
            if winners.contains(&seat) {
                Pose::Celebrate
            } else {
                Pose::Slump
            }
        } else if opened && picker.caught {
            Pose::Slump
        } else if opened && (picker.gained > 0 || caught_someone(game, seat)) {
            Pose::Hop
        } else {
            Pose::Idle
        };
        // Brighter once locked in, so everyone sees who is still choosing.
        let locked = matches!(game.phase, Phase::Picking { .. }) && picker.pick.is_some();
        let glow = if locked {
            3.2
        } else {
            1.6 + 0.3 * (now * 3.0 + seat.0 as f32).sin()
        };
        if let Some(mut material) = materials.get_mut(&view.head_material) {
            material.emissive = seat_color(seat) * glow;
        }
        if let Ok(mut light) = lights.get_mut(view.head_light) {
            light.intensity = 18_000.0 * glow;
        }
    }
}

/// Whether a hider's moth caught anyone this round (only meaningful once its lantern is open).
fn caught_someone(game: &State, seat: Seat) -> bool {
    let picker = game.picker(seat);
    picker.hiding
        && picker
            .pick
            .is_some_and(|dir| game.pickers_of(dir).next().is_some())
}

/// Sounds and sparks, once per event: locking in, the last seconds to pick, the reveal, each
/// lantern opening and the result. Bots lock in quietly, so a person hears their own.
fn react(
    mut commands: Commands,
    active: Res<ActiveRun<LanternLottery>>,
    seats: Res<Seats>,
    effects: Res<Effects>,
    sfx: Sfx,
) {
    let Some((before, now)) = active.ticked() else {
        return;
    };
    if matches!(now.phase, Phase::Showing { t: 0 }) {
        sfx.play(Sound::TurnStart);
    }
    if let Phase::Picking { .. } = now.phase {
        for seat in now.playing() {
            if before.picker(seat).pick.is_none() && now.picker(seat).pick.is_some() {
                let volume = if seats.is_bot(seat) { -10.0 } else { 0.0 };
                sfx.play_with(Sound::Confirm, 1.0, volume);
            }
        }
        if let Some(left) = now.pick_ticks_left()
            && left <= 3 * TICK_HZ as u16
            && left % TICK_HZ as u16 == 0
        {
            sfx.play(Sound::Tick);
        }
    }
    if !before.revealed() && now.revealed() {
        sfx.play(Sound::Draw);
    }
    if now.opened_count() > before.opened_count()
        && let Phase::Reveal { .. } = now.phase
    {
        let dir = now.order[now.opened_count() - 1];
        let at = lantern_pos(dir).extend(MOTH_Z + 0.5);
        if now.moths[dir.index()] {
            sfx.play(Sound::FalseStart);
            particles::burst(&mut commands, &effects, SCOPE, at, hdr(MOTH_GREY, 2.0));
        } else if now.mode == Mode::SoloHides {
            sfx.play_with(Sound::Ignite, 0.8, -6.0);
        } else if now.pickers_of(dir).next().is_some() {
            sfx.play(Sound::Ignite);
            sfx.play_with(Sound::CoinsUp, 1.0, -3.0);
            particles::burst(&mut commands, &effects, SCOPE, at, hdr(LANTERN_WARM, 6.0));
            particles::burst(&mut commands, &effects, SCOPE, at, hdr(LANTERN_WARM, 3.0));
        } else {
            sfx.play_with(Sound::Ignite, 0.8, -6.0);
            particles::burst(&mut commands, &effects, SCOPE, at, hdr(PAPER, 1.5));
        }
    }
    if before.outcome.is_none() && now.outcome.is_some() {
        sfx.play(Sound::Win);
    }
}

const CAMERA_Y: f32 = 0.1;
/// The diamond, the markers over and under it, and the folk below must stay on screen.
const FRAMED_WIDTH: f32 = 14.0;
const FRAMED_HEIGHT: f32 = 14.0;

fn camera_transform() -> Transform {
    Transform::from_xyz(0.0, CAMERA_Y, 20.0).looking_at(Vec3::new(0.0, CAMERA_Y, 0.0), Vec3::Y)
}

/// Orthographic and straight on, so the four lanterns sit square, like the stick's directions.
fn camera_projection() -> Projection {
    Projection::Orthographic(OrthographicProjection {
        scaling_mode: ScalingMode::AutoMin {
            min_width: FRAMED_WIDTH,
            min_height: FRAMED_HEIGHT,
        },
        ..OrthographicProjection::default_3d()
    })
}

/// Label boxes are this big, centred on where they're pinned.
const LABEL_SIZE: Vec2 = Vec2::new(280.0, 70.0);

fn spawn_hud(mut commands: Commands, active: Res<ActiveRun<LanternLottery>>) {
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

    for dir in Dir::ALL {
        for (label, size) in [
            (LanternLabel::Fireflies(dir), 40.0),
            (LanternLabel::Result(dir), 24.0),
        ] {
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
                    TextColor(Color::srgb(1.0, 0.95, 0.85)),
                    TextLayout::justify(Justify::Center),
                ));
        }
    }

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
            for seat in playing(game) {
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

/// What a lantern's labels say: its fireflies (once the round's are drawn), and once it's open,
/// what it paid.
fn label_text(game: &State, label: &LanternLabel) -> (String, Color) {
    let cream = Color::srgb(1.0, 0.95, 0.85);
    match *label {
        LanternLabel::Fireflies(dir) => {
            let hidden = matches!(game.phase, Phase::Intro { .. }) || game.mode == Mode::SoloHides;
            let open = since_open(game, dir).is_some();
            if hidden || (open && game.moths[dir.index()]) {
                return (String::new(), cream);
            }
            let fireflies = game.fireflies[dir.index()];
            let text = if game.doubled() {
                format!("{fireflies}\nx2")
            } else {
                fireflies.to_string()
            };
            let color = if open { cream.with_alpha(0.45) } else { cream };
            (text, color)
        }
        LanternLabel::Result(dir) => {
            if since_open(game, dir).is_none() {
                return (String::new(), cream);
            }
            let seekers = game.pickers_of(dir).count() as u32;
            if game.moths[dir.index()] {
                let text = if seekers > 0 {
                    tr!("lantern-lottery-moth-caught", count = seekers)
                } else {
                    tr!("lantern-lottery-moth")
                };
                (text, Color::from(MOTH_GREY * 2.2))
            } else if game.mode == Mode::SoloHides {
                let text = if seekers > 0 {
                    tr!("lantern-lottery-safe")
                } else {
                    String::new()
                };
                (text, cream)
            } else if let Some(share) = (game.fireflies[dir.index()] as u32).checked_div(seekers) {
                let text = if seekers > 1 {
                    tr!("lantern-lottery-gain-each", fireflies = share)
                } else {
                    tr!("lantern-lottery-gain", fireflies = share)
                };
                (text, Color::from(LANTERN_WARM * 1.4))
            } else {
                (String::new(), cream)
            }
        }
    }
}

/// Pin each lantern's labels to it on screen, and fill them in.
fn place_labels(
    active: Res<ActiveRun<LanternLottery>>,
    camera: Query<(&Camera, &GlobalTransform), With<CameraRig>>,
    mut labels: Query<(&LanternLabel, &mut Text, &mut TextColor, &ChildOf)>,
    mut boxes: Query<&mut Node>,
) {
    let Some((camera, camera_transform)) = camera.iter().next() else {
        return;
    };
    let game = &active.run.game;
    for (label, mut text, mut color, parent) in &mut labels {
        let (says, tint) = label_text(game, label);
        text.0 = says;
        color.0 = tint;
        let at = match *label {
            LanternLabel::Fireflies(dir) => lantern_pos(dir),
            // Under the seekers' markers.
            LanternLabel::Result(dir) => {
                lantern_pos(dir) - Vec2::Y * (LANTERN_RADIUS + ROW_GAP + 0.55)
            }
        };
        if let (Ok(screen), Ok(mut node)) = (
            camera.world_to_viewport(camera_transform, at.extend(MARKER_Z)),
            boxes.get_mut(parent.parent()),
        ) {
            node.left = Val::Px(screen.x - LABEL_SIZE.x / 2.0);
            node.top = Val::Px(screen.y - LABEL_SIZE.y / 2.0);
        }
    }
}

/// Each team's fireflies in 2v2: "P1 & P3  14     P2 & P4  9".
fn team_line(game: &State) -> Option<String> {
    if !matches!(game.format, Format::TwoVsTwo { .. }) {
        return None;
    }
    let teams = game.team_scores();
    let lines: Vec<String> = (0..2u8)
        .map(|team| {
            let members = names(
                game.playing()
                    .filter(|s| game.format.team(*s) == Some(team)),
            );
            tr!(
                "lantern-lottery-team",
                players = members,
                fireflies = teams[team as usize]
            )
        })
        .collect();
    Some(lines.join("     "))
}

/// Who hides a moth this round, when anyone does: "P1 & P3 hide moths".
fn hiders_line(game: &State) -> Option<String> {
    let hiders: Vec<Seat> = game.hiders().collect();
    (!hiders.is_empty()).then(|| {
        tr!(
            "lantern-lottery-hiders",
            players = names(hiders.iter().copied()),
            count = hiders.len() as u32
        )
    })
}

fn banner_text(active: &ActiveRun<LanternLottery>) -> (String, String) {
    let run = &active.run;
    let game = &run.game;
    if let Some(banner) = halted_banner(run) {
        return banner;
    }
    let round = || {
        let line = tr!(
            "lantern-lottery-round",
            round = game.round as u32 + 1,
            rounds = game.rounds() as u32
        );
        if game.doubled() {
            format!("{line}  {}", tr!("lantern-lottery-double"))
        } else {
            line
        }
    };
    let extra: Vec<String> = [hiders_line(game), team_line(game)]
        .into_iter()
        .flatten()
        .collect();
    match game.phase {
        // The shared countdown (crate::intro) is drawn over this.
        Phase::Intro { .. } => (String::new(), String::new()),
        Phase::Showing { .. } | Phase::Reveal { .. } => (round(), extra.join("\n")),
        Phase::Picking { .. } => {
            let left = game.pick_ticks_left().unwrap_or(PICK_TICKS) as u32;
            let pick = tr!("lantern-lottery-pick", secs = left.div_ceil(TICK_HZ));
            let lines: Vec<String> = std::iter::once(pick).chain(extra).collect();
            (round(), lines.join("\n"))
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

/// A seat's line under its score: picking or locked in, then what its lantern did.
fn seat_status(game: &State, seat: Seat) -> String {
    let picker = game.picker(seat);
    match game.phase {
        Phase::Intro { .. } => String::new(),
        Phase::Showing { .. } if picker.hiding => tr!("lantern-lottery-hiding"),
        Phase::Showing { .. } => String::new(),
        Phase::Picking { .. } if picker.pick.is_some() => tr!("lantern-lottery-locked"),
        Phase::Picking { .. } if picker.hiding => tr!("lantern-lottery-hiding"),
        Phase::Picking { .. } => tr!("lantern-lottery-picking"),
        Phase::Reveal { .. } | Phase::Decided { .. } | Phase::Finished => {
            let opened = picker
                .pick
                .is_some_and(|dir| since_open(game, dir).is_some());
            if !opened {
                String::new()
            } else if picker.hiding {
                if caught_someone(game, seat) {
                    tr!("lantern-lottery-caught-them")
                } else {
                    tr!("lantern-lottery-missed")
                }
            } else if picker.caught {
                tr!("lantern-lottery-caught")
            } else if game.mode == Mode::SoloHides {
                tr!("lantern-lottery-safe")
            } else {
                tr!("lantern-lottery-gain", fireflies = picker.gained)
            }
        }
    }
}

fn update_hud(
    active: Res<ActiveRun<LanternLottery>>,
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
                let solo = game.solo() == Some(seat);
                let role = if solo { tr!("solo-tag") } else { String::new() };
                let score = match game.catches() {
                    Some((catches, needed)) if solo => {
                        tr!(
                            "lantern-lottery-catches",
                            catches = catches,
                            needed = needed
                        )
                    }
                    Some(_) => String::new(),
                    None => tr!(
                        "lantern-lottery-fireflies",
                        fireflies = game.scores[seat.index()]
                    ),
                };
                let status = if active.run.controller_lost(seat) {
                    tr!("controller-lost")
                } else if let (Some(outcome), Phase::Finished) = (game.outcome, game.phase) {
                    match outcome.placement[seat.index()] {
                        Some(place) => tr!("place", place = place as u32),
                        None => String::new(),
                    }
                } else {
                    seat_status(game, seat)
                };
                format!("{}{who}{role}\n{score}\n{status}", seat_label(seat))
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{Button, Input, MAX_PLAYERS, PlayerSetup, Run, RunConfig};

    /// The lanterns stand where the stick points: up is up on screen, right is right.
    #[test]
    fn the_diamond_is_seen_straight_on() {
        assert!(matches!(camera_projection(), Projection::Orthographic(_)));
        let forward = camera_transform().forward();
        assert!(forward.abs_diff_eq(Vec3::NEG_Z, 1e-6), "{forward}");
        assert!(lantern_pos(Dir::Up).y > lantern_pos(Dir::Down).y);
        assert!(lantern_pos(Dir::Right).x > lantern_pos(Dir::Left).x);
    }

    /// Nothing the view shows before the reveal may depend on where anyone points or what they
    /// picked: run two games that differ only in their picks and compare what each would show.
    #[test]
    fn picks_stay_secret_until_the_reveal() {
        for format in [
            Format::FreeForAll,
            Format::OneVsThree { solo: Seat(1) },
            Format::TwoVsTwo {
                team_of: [0, 1, 0, 1],
            },
        ] {
            let new = || {
                Run::<LanternLottery>::new(7, PlayerSetup::first(4), format, RunConfig::default())
            };
            let (mut a, mut b) = (new(), new());
            let mut checked = 0;
            while !a.game.revealed() {
                let inputs = |shift: usize, game: &State| -> [Input; MAX_PLAYERS] {
                    std::array::from_fn(|i| {
                        let stick = Dir::ALL[(i + shift) % 4].stick();
                        // Everyone locks in a moment apart.
                        match game.phase {
                            Phase::Picking { t, .. } if t == 20 * (i as u16 + 1) => {
                                stick.with(Button::A)
                            }
                            _ => stick,
                        }
                    })
                };
                let (to_a, to_b) = (inputs(0, &a.game), inputs(1, &b.game));
                a.step(to_a);
                b.step(to_b);
                if a.game.revealed() {
                    break;
                }
                for dir in Dir::ALL {
                    for label in [LanternLabel::Fireflies(dir), LanternLabel::Result(dir)] {
                        assert_eq!(label_text(&a.game, &label).0, label_text(&b.game, &label).0);
                    }
                    assert_eq!(
                        lantern_glow(&a.game, dir, 0.0),
                        lantern_glow(&b.game, dir, 0.0)
                    );
                }
                for seat in Seat::ALL {
                    assert_eq!(seat_status(&a.game, seat), seat_status(&b.game, seat));
                }
                checked += 1;
            }
            assert!(checked > 100, "{checked}");
            // The picks really did differ.
            assert_ne!(a.game.picker(Seat(0)).pick, b.game.picker(Seat(0)).pick);
        }
    }
}
