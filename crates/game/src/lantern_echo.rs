//! Presentation for Lantern Echo. Reads `ActiveRun<LanternEcho>` only; never writes simulation
//! state. Every racing seat gets a lane running away from the camera, its character at the near
//! end and a diamond of four lanterns at the far end, in the seat's colour.

use std::f32::consts::PI;

use bevy::prelude::*;
use minigame_lantern_echo::{
    Dir, LEAD_IN_TICKS, LIT_TICKS, GAP_TICKS, LanternEcho, MAX_LENGTH, Phase, RACE_TICKS,
    STEP_TICKS, STUMBLE_TICKS, State, Status,
};
use sim_core::{Format, Seat, TICK_HZ};

use crate::audio::{Sfx, Sound};
use crate::characters::{Character, Pose, Roster};
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

pub struct LanternEchoViewPlugin;

impl Plugin for LanternEchoViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            (spawn_scene, spawn_hud)
                .after(MinigameSet::Start)
                .run_if(resource_exists::<ActiveRun<LanternEcho>>),
        )
        .add_systems(
            Update,
            (
                animate_lanterns,
                animate_pips,
                animate_tiles,
                animate_runners,
                fit_camera,
                update_hud,
            )
                .run_if(resource_exists::<ActiveRun<LanternEcho>>),
        )
        .add_systems(
            FixedUpdate,
            react
                .in_set(MinigameSet::React)
                .run_if(resource_exists::<ActiveRun<LanternEcho>>),
        );
    }
}

const SCOPE: DespawnOnExit<AppState> = DespawnOnExit(AppState::Minigame);
/// Distance between neighbouring lanes.
const LANE_GAP: f32 = 3.0;
/// Where every lane starts (nearest the camera) and how far it runs.
const LANE_START: f32 = 3.0;
const LANE_LENGTH: f32 = 7.5;
/// The lantern diamond stands past the lane's end, high enough to be seen over the runner.
const DIAMOND: Vec3 = Vec3::new(0.0, 3.3, LANE_START - LANE_LENGTH - 1.3);
const DIAMOND_SPREAD: f32 = 0.8;
const LANTERN_SIZE: f32 = 0.5;
/// The row of pips counting the sequence, above the diamond.
const PIP_Y: f32 = DIAMOND.y + DIAMOND_SPREAD + 0.75;
const PIP_GAP: f32 = 0.3;
const GOAL_WHITE: LinearRgba = LinearRgba::rgb(0.85, 0.9, 1.0);
const SLIP_RED: LinearRgba = LinearRgba::rgb(1.0, 0.08, 0.05);
const FIREFLY_COUNT: usize = 36;
/// Team colours for the panel borders in team formats.
const TEAM_COLORS: [LinearRgba; 2] = [
    LinearRgba::rgb(1.0, 0.78, 0.3),
    LinearRgba::rgb(0.55, 0.45, 1.0),
];

#[derive(Component)]
struct RunnerView {
    seat: Seat,
    head_material: Handle<StandardMaterial>,
    head_light: Entity,
}

#[derive(Component)]
struct EchoLantern {
    seat: Seat,
    dir: Dir,
    material: Handle<StandardMaterial>,
}

/// The light over each lane's diamond, which takes the colour of whatever is lit.
#[derive(Component)]
struct DiamondLight(Seat);

#[derive(Component)]
struct Pip {
    seat: Seat,
    index: usize,
    material: Handle<StandardMaterial>,
}

/// The lane tile a runner stands on after `step` right echoes.
#[derive(Component)]
struct Tile {
    seat: Seat,
    step: u8,
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct CameraRig;

#[derive(Component)]
enum HudText {
    Banner,
    SubBanner,
    SeatPanel(Seat),
}

/// Seats that race, in seat order: everyone but those outside a duel.
fn racers(game: &State) -> Vec<Seat> {
    game.players
        .seats()
        .filter(|s| game.runner(*s).status != Status::Benched)
        .collect()
}

fn lane_x(game: &State, seat: Seat) -> f32 {
    let lanes = racers(game);
    let index = lanes.iter().position(|s| *s == seat).unwrap_or(0) as f32;
    (index - (lanes.len() as f32 - 1.0) / 2.0) * LANE_GAP
}

/// Length of one step in this seat's lane: the whole lane is as many steps as it can take.
fn step_length(game: &State, seat: Seat) -> f32 {
    LANE_LENGTH / game.runner(seat).max_steps as f32
}

fn dir_offset(dir: Dir) -> Vec3 {
    match dir {
        Dir::Up => Vec3::Y,
        Dir::Right => Vec3::X,
        Dir::Down => Vec3::NEG_Y,
        Dir::Left => Vec3::NEG_X,
    }
}

fn team_index(format: &Format, seat: Seat) -> Option<u8> {
    match format {
        Format::FreeForAll | Format::Duel { .. } => None,
        _ => format.team(seat),
    }
}

fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    active: Res<ActiveRun<LanternEcho>>,
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
        night_camera(camera_transform(1.0), (16.0, 40.0)),
        CameraRig,
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

    spawn_ground(&mut commands, &mut meshes, &mut materials);
    for (pos, color) in [
        (Vec3::new(-7.8, 0.0, -5.0), LinearRgba::rgb(1.0, 0.3, 0.55)),
        (Vec3::new(7.8, 0.0, -5.0), LinearRgba::rgb(0.25, 0.75, 1.0)),
        (Vec3::new(-8.4, 0.0, 1.5), LANTERN_WARM),
        (Vec3::new(8.4, 0.0, 1.5), LANTERN_WARM),
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
        Vec3::new(-9.0, 0.5, -9.0),
        Vec3::new(9.0, 5.0, 3.0),
    );

    let game = &active.run.game;
    for seat in racers(game) {
        spawn_lane(&mut commands, &mut meshes, &mut materials, game, seat);
        spawn_runner(
            &mut commands,
            &mut meshes,
            &mut materials,
            game,
            seat,
            roster.of(seat),
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
    for i in -10..=10 {
        for j in -10..=5 {
            let shade = &shades[(noise(i, j, 5) * 2.999) as usize];
            let y = -0.25 + (noise(i, j, 6) - 0.5) * 0.06;
            commands.spawn((
                SCOPE,
                Mesh3d(tile.clone()),
                MeshMaterial3d(shade.clone()),
                Transform::from_xyz(i as f32, y, j as f32),
            ));
        }
    }
}

fn spawn_lane(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    game: &State,
    seat: Seat,
) {
    let x = lane_x(game, seat);
    let color = seat_color(seat);
    let step = step_length(game, seat);

    // Tiles: the start, then one per step.
    let tile_mesh = meshes.add(Cuboid::new(1.7, 0.12, step * 0.86));
    for k in 0..=game.runner(seat).max_steps {
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.07, 0.07, 0.085),
            perceptual_roughness: 0.7,
            ..default()
        });
        commands.spawn((
            SCOPE,
            Tile {
                seat,
                step: k,
                material: material.clone(),
            },
            Mesh3d(tile_mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_xyz(x, 0.0, LANE_START - k as f32 * step),
        ));
    }

    // The diamond on a post, its four lanterns and the light they share.
    let centre = DIAMOND + Vec3::X * x;
    let wood = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.055, 0.04),
        perceptual_roughness: 0.85,
        ..default()
    });
    let post_height = centre.y + DIAMOND_SPREAD;
    commands.spawn((
        SCOPE,
        Mesh3d(meshes.add(Cuboid::new(0.16, post_height, 0.16))),
        MeshMaterial3d(wood.clone()),
        Transform::from_xyz(centre.x, post_height / 2.0, centre.z - 0.35),
    ));
    commands.spawn((
        SCOPE,
        Mesh3d(meshes.add(Cuboid::new(DIAMOND_SPREAD * 2.0 + 0.2, 0.12, 0.12))),
        MeshMaterial3d(wood),
        Transform::from_xyz(centre.x, centre.y, centre.z - 0.35),
    ));
    let lantern_mesh = meshes.add(Cuboid::new(LANTERN_SIZE, LANTERN_SIZE, LANTERN_SIZE));
    for dir in Dir::ALL {
        let material = materials.add(emissive_material(color, 0.1));
        commands.spawn((
            SCOPE,
            EchoLantern {
                seat,
                dir,
                material: material.clone(),
            },
            Mesh3d(lantern_mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(centre + dir_offset(dir) * DIAMOND_SPREAD)
                .with_rotation(Quat::from_rotation_z(PI / 4.0)),
        ));
    }
    commands.spawn((
        SCOPE,
        DiamondLight(seat),
        PointLight {
            color: Color::from(color),
            intensity: 0.0,
            range: 6.0,
            ..default()
        },
        Transform::from_translation(centre + Vec3::Z * 0.8),
    ));

    let pip_mesh = meshes.add(Cuboid::new(0.18, 0.18, 0.18));
    for index in 0..MAX_LENGTH {
        let material = materials.add(emissive_material(color, 0.0));
        commands.spawn((
            SCOPE,
            Pip {
                seat,
                index,
                material: material.clone(),
            },
            Mesh3d(pip_mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(centre),
            Visibility::Hidden,
        ));
    }
}

fn spawn_runner(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    game: &State,
    seat: Seat,
    character: Character,
) {
    let folk = spawn_lantern_folk(
        commands,
        meshes,
        materials,
        AppState::Minigame,
        seat,
        character,
        Transform::from_xyz(lane_x(game, seat), 0.06, LANE_START)
            .with_rotation(Quat::from_rotation_y(PI)),
    );
    commands.entity(folk.root).insert(RunnerView {
        seat,
        head_material: folk.head_material,
        head_light: folk.head_light,
    });
}

/// Ticks into the hop up the lane, carried on through the result (the simulation stops
/// stepping runners once the race is decided, mid-hop for the winner).
fn hop_ticks(game: &State, t: u16) -> f32 {
    match game.phase {
        Phase::Decided { t: since } => (t + since) as f32,
        Phase::Finished => STEP_TICKS as f32,
        _ => t as f32,
    }
}

fn blend(fixed: &Time<Fixed>) -> f32 {
    fixed.overstep_fraction().clamp(0.0, 1.0)
}

/// How brightly one lantern glows, and in what colour.
fn lantern_glow(game: &State, seat: Seat, dir: Dir, time: f32) -> (LinearRgba, f32) {
    let color = seat_color(seat);
    let runner = game.runner(seat);
    let sequence = &game.sequences[seat.index()];
    match runner.status {
        Status::Watching { .. } if game.lit(seat) == Some(dir) => (color, 9.0),
        Status::Watching { .. } => (color, 0.08),
        Status::Echoing { entered, idle } => {
            let last = entered.checked_sub(1).map(|i| sequence[i as usize]);
            let flash = 1.0 - idle as f32 / 14.0;
            if last == Some(dir) && flash > 0.0 {
                (color, 0.6 + 7.0 * flash)
            } else {
                // Waiting for the player: a gentle pulse says "your turn".
                (color, 0.5 + 0.2 * (time * 5.0 + seat.0 as f32).sin())
            }
        }
        Status::Stepping { t } => {
            let fade = 1.0 - hop_ticks(game, t) / STEP_TICKS as f32;
            (color, 0.3 + 6.0 * fade.max(0.0))
        }
        Status::Stumbling { t, wrong } if dir == wrong => {
            (SLIP_RED, 6.0 * (1.0 - t as f32 / STUMBLE_TICKS as f32))
        }
        Status::Stumbling { .. } => (color, 0.03),
        Status::Done => (color, 1.5),
        Status::Benched => (color, 0.0),
    }
}

fn animate_lanterns(
    time: Res<Time>,
    active: Res<ActiveRun<LanternEcho>>,
    lanterns: Query<&EchoLantern>,
    mut lights: Query<(&DiamondLight, &mut PointLight)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let game = &active.run.game;
    let t = time.elapsed_secs();
    let mut brightest = [(LinearRgba::BLACK, 0.0f32); 4];
    for lantern in &lanterns {
        let (color, glow) = lantern_glow(game, lantern.seat, lantern.dir, t);
        if let Some(mut material) = materials.get_mut(&lantern.material) {
            material.emissive = color * glow;
        }
        let best = &mut brightest[lantern.seat.index()];
        if glow > best.1 {
            *best = (color, glow);
        }
    }
    for (DiamondLight(seat), mut light) in &mut lights {
        let (color, glow) = brightest[seat.index()];
        light.color = Color::from(color);
        light.intensity = 9_000.0 * glow;
    }
}

/// How many of the sequence's lanterns the pips count, and how brightly.
fn pips_shown(game: &State, seat: Seat) -> (usize, LinearRgba, f32) {
    let runner = game.runner(seat);
    let color = seat_color(seat);
    let length = runner.length() as usize;
    match runner.status {
        Status::Watching { t } => {
            let played = t
                .checked_sub(LEAD_IN_TICKS)
                .map_or(0, |t| (t / (LIT_TICKS + GAP_TICKS)) as usize + 1);
            (played.min(length), LinearRgba::rgb(0.8, 0.8, 0.9), 1.2)
        }
        Status::Echoing { entered, .. } => (entered as usize, color, 4.0),
        Status::Stepping { .. } | Status::Done => (length, color, 4.0),
        Status::Stumbling { .. } => (length, SLIP_RED, 1.5),
        Status::Benched => (0, color, 0.0),
    }
}

fn animate_pips(
    active: Res<ActiveRun<LanternEcho>>,
    mut pips: Query<(&Pip, &mut Transform, &mut Visibility)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let game = &active.run.game;
    for (pip, mut transform, mut visibility) in &mut pips {
        let length = game.runner(pip.seat).length() as usize;
        if pip.index >= length {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = Visibility::Inherited;
        let offset = (pip.index as f32 - (length as f32 - 1.0) / 2.0) * PIP_GAP;
        transform.translation = Vec3::new(lane_x(game, pip.seat) + offset, PIP_Y, DIAMOND.z);
        let (lit, color, glow) = pips_shown(game, pip.seat);
        if let Some(mut material) = materials.get_mut(&pip.material) {
            material.emissive = if pip.index < lit {
                color * glow
            } else {
                LinearRgba::rgb(0.25, 0.25, 0.3) * 0.15
            };
        }
    }
}

fn animate_tiles(
    active: Res<ActiveRun<LanternEcho>>,
    tiles: Query<&Tile>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let game = &active.run.game;
    for tile in &tiles {
        let runner = game.runner(tile.seat);
        let glow = if tile.step == 0 {
            seat_color(tile.seat) * 0.05
        } else if tile.step <= runner.steps {
            seat_color(tile.seat) * 1.2
        } else if tile.step == runner.max_steps {
            // The end of the lane.
            GOAL_WHITE * 0.7
        } else {
            seat_color(tile.seat) * 0.08
        };
        if let Some(mut material) = materials.get_mut(&tile.material) {
            material.emissive = glow;
        }
    }
}

fn animate_runners(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    active: Res<ActiveRun<LanternEcho>>,
    mut runners: Query<(&RunnerView, &mut Transform, &mut Pose)>,
    mut lights: Query<&mut PointLight, Without<DiamondLight>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let alpha = blend(&fixed);
    let now = time.elapsed_secs();
    let game = &active.run.game;
    let winners: Vec<Seat> = game
        .outcome
        .map(|o| o.winners().collect())
        .unwrap_or_default();
    for (view, mut transform, mut pose) in &mut runners {
        let seat = view.seat;
        let runner = game.runner(seat);
        let shown_steps = match runner.status {
            Status::Stepping { t } => {
                let ticks = hop_ticks(game, t) + alpha * (game.phase == Phase::Racing) as u8 as f32;
                runner.steps as f32 - 1.0 + (ticks / STEP_TICKS as f32).min(1.0)
            }
            _ => runner.steps as f32,
        };
        transform.translation.z = LANE_START - shown_steps * step_length(game, seat);

        *pose = if game.outcome.is_some() {
            if winners.contains(&seat) {
                Pose::Celebrate
            } else {
                Pose::Slump
            }
        } else {
            match runner.status {
                Status::Stepping { .. } => Pose::Hop,
                Status::Stumbling { .. } => Pose::Slump,
                Status::Done => Pose::Celebrate,
                _ => Pose::Idle,
            }
        };

        let glow = match runner.status {
            Status::Echoing { .. } => 2.6 + (now * 5.0 + seat.0 as f32).sin() * 0.3,
            Status::Stumbling { .. } => 0.3,
            Status::Stepping { .. } | Status::Done => 3.5,
            _ => 1.6,
        };
        if let Some(mut material) = materials.get_mut(&view.head_material) {
            material.emissive = seat_color(seat) * glow;
        }
        if let Ok(mut light) = lights.get_mut(view.head_light) {
            light.intensity = 22_000.0 * glow;
        }
    }
}

/// Pitches for the four lanterns, a major chord's worth so sequences sound like tunes.
fn tone(dir: Dir) -> f64 {
    match dir {
        Dir::Up => 1.5,
        Dir::Right => 1.26,
        Dir::Down => 0.84,
        Dir::Left => 1.0,
    }
}

/// Sounds and sparks, once per event: lanterns playing and echoed (for people, not bots, so
/// four lanes don't drown each other out), steps, slips, the last seconds and the win.
fn react(
    mut commands: Commands,
    active: Res<ActiveRun<LanternEcho>>,
    seats: Res<Seats>,
    runners: Query<(&RunnerView, &Transform)>,
    effects: Res<Effects>,
    sfx: Sfx,
) {
    let Some((before, now)) = active.ticked() else {
        return;
    };
    let head = |seat: Seat| {
        runners
            .iter()
            .find(|(r, _)| r.seat == seat)
            .map(|(_, t)| t.translation + Vec3::Y * 1.5)
    };
    let mut spark = |pos: Vec3, color: Vec4| {
        particles::burst(&mut commands, &effects, SCOPE, pos, color);
    };
    for seat in racers(now) {
        let human = !seats.is_bot(seat);
        let (was, is) = (before.runner(seat), now.runner(seat));
        if human
            && let Some(dir) = now.lit(seat)
            && before.lit(seat) != Some(dir)
        {
            sfx.play_with(Sound::Count, tone(dir), -6.0);
        }
        // The flick that was just echoed right is the one `entered` pointed at before it.
        let echoed = match (was.status, is.status) {
            (Status::Echoing { entered: a, .. }, Status::Echoing { entered: b, .. }) if b > a => {
                Some(a)
            }
            (Status::Echoing { entered, .. }, Status::Stepping { .. }) => Some(entered),
            _ => None,
        };
        if human && let Some(index) = echoed {
            let dir = now.sequences[seat.index()][index as usize];
            sfx.play_with(Sound::Count, tone(dir), 0.0);
        }
        if is.steps > was.steps {
            sfx.play_varied(Sound::Step, now.tick + seat.0 as u32);
            if let Some(at) = head(seat) {
                spark(at, hdr(seat_color(seat), 4.0));
            }
        }
        if is.misses > was.misses {
            sfx.play_with(Sound::FalseStart, 1.0, -4.0);
            if let Some(at) = head(seat) {
                spark(at, hdr(SLIP_RED, 1.5));
            }
        }
    }
    if now.phase == Phase::Racing {
        let left = RACE_TICKS.saturating_sub(now.tick);
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
                spark(at + Vec3::Y * 0.5, hdr(seat_color(seat), 6.0));
                spark(at + Vec3::Y, hdr(LANTERN_WARM, 6.0));
            }
        }
    }
}

const CAMERA_TARGET: Vec3 = Vec3::new(0.0, 1.2, -1.0);
const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 7.6, 13.4);
/// Half-width of four lanes and their diamonds (plus margin) that must stay on screen.
const FRAMED_HALF_WIDTH: f32 = 1.5 * LANE_GAP + DIAMOND_SPREAD + 1.0;

fn camera_transform(pull_back: f32) -> Transform {
    Transform::from_translation(CAMERA_TARGET + CAMERA_OFFSET * pull_back)
        .looking_at(CAMERA_TARGET, Vec3::Y)
}

/// Pull the camera back on narrow windows so every lane stays in frame.
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

fn spawn_hud(mut commands: Commands, active: Res<ActiveRun<LanternEcho>>) {
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
            for seat in racers(game) {
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

/// Each team's steps out of what it needs, in team formats: "P1 & P3 7/12   P2 & P4 5/12".
fn team_progress(game: &State) -> Option<String> {
    if !matches!(
        game.format,
        Format::OneVsThree { .. } | Format::TwoVsTwo { .. }
    ) {
        return None;
    }
    let teams = game.team_steps();
    let lines: Vec<String> = (0..2u8)
        .map(|team| {
            let members = names(
                racers(game)
                    .into_iter()
                    .filter(|s| game.format.team(*s) == Some(team)),
            );
            let (steps, needed) = teams[team as usize];
            tr!(
                "lantern-echo-team",
                players = members,
                steps = steps.min(needed),
                needed = needed
            )
        })
        .collect();
    Some(lines.join("     "))
}

fn banner_text(active: &ActiveRun<LanternEcho>) -> (String, String) {
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
                tr!("lantern-echo-go")
            } else {
                String::new()
            };
            let subtitle = match team_progress(game) {
                Some(teams) => format!("{teams}\n{}", clock(game.ticks_left())),
                None => clock(game.ticks_left()),
            };
            (title, subtitle)
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
    active: Res<ActiveRun<LanternEcho>>,
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
                let runner = game.runner(seat);
                let status = if active.run.controller_lost(seat) {
                    tr!("controller-lost")
                } else if let (Some(outcome), true) = (game.outcome, game.phase != Phase::Racing) {
                    match outcome.placement[seat.index()] {
                        Some(place) => tr!("place", place = place as u32),
                        None => String::new(),
                    }
                } else {
                    match runner.status {
                        Status::Watching { .. } => tr!("lantern-echo-watch"),
                        Status::Echoing { entered, .. } => tr!(
                            "lantern-echo-echo",
                            entered = entered as u32,
                            length = runner.length() as u32
                        ),
                        Status::Stepping { .. } => tr!("lantern-echo-right"),
                        Status::Stumbling { .. } => tr!("lantern-echo-slip"),
                        Status::Done => tr!("lantern-echo-done"),
                        Status::Benched => String::new(),
                    }
                };
                format!("{}{who}{role}\n{status}", seat_label(seat))
            }
        };
    }
}
