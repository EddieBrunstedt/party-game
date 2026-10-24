//! Presentation for Puzzle Buzzer. Reads `ActiveRun<PuzzleBuzzer>` only; never writes
//! simulation state. Each board is a puzzle drawn above a diamond of four answer slots (up, right,
//! down, left, as the stick points): one board in the middle with the lantern folk either side,
//! or in 2v2 two boards side by side, one for each partner. Nothing here reads where anyone
//! points, nor (in 2v2) a right half's lock-in before the puzzle ends; a wrong lock-in is shown
//! at once, crossed out, with the marker of whoever locked it in.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::camera::ScalingMode;
use bevy::prelude::*;
use minigame_puzzle_buzzer::puzzle::{COUNT_COLUMNS, COUNT_ROWS, TURNS};
use minigame_puzzle_buzzer::{
    ANSWER_TICKS, Answer, Dir, GOAL, Glyph, Kind, Op, Phase, Puzzle, PuzzleBuzzer, State, Token,
};
use sim_core::{Format, Seat, TICK_HZ};

use crate::audio::{Sfx, Sound};
use crate::characters::{Pose, Roster};
use crate::flow::AppState;
use crate::i18n::{self, bot_tag, halted_banner, names};
use crate::palette::{LANTERN_WARM, MOONLIGHT, NIGHT_SKY, seat_color, seat_label};
use crate::particles::{self, Effects, hdr};
use crate::runner::{ActiveRun, MinigameSet, Seats};
use crate::scenery::{flat_marker_mesh, night_camera, noise, spawn_lantern_folk};
use crate::tr;

pub struct PuzzleBuzzerViewPlugin;

impl Plugin for PuzzleBuzzerViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            (spawn_scene, spawn_hud)
                .after(MinigameSet::Start)
                .run_if(resource_exists::<ActiveRun<PuzzleBuzzer>>),
        )
        .add_systems(
            Update,
            (
                rebuild_puzzles,
                (show_content, show_slots, place_markers, show_timers),
                animate_folk,
                place_labels,
                update_hud,
            )
                .chain()
                .run_if(resource_exists::<ActiveRun<PuzzleBuzzer>>),
        )
        .add_systems(
            FixedUpdate,
            react
                .in_set(MinigameSet::React)
                .run_if(resource_exists::<ActiveRun<PuzzleBuzzer>>),
        );
    }
}

const SCOPE: DespawnOnExit<AppState> = DespawnOnExit(AppState::Minigame);

/// The frame: 16×10 world units, straight on.
const FRAMED_WIDTH: f32 = 16.0;
const FRAMED_HEIGHT: f32 = 10.0;
/// In 2v2 the two boards' centres are this far either side of the middle.
const HALF_X: f32 = 4.0;
/// A board's puzzle panel: its centre height and size.
const PANEL_Y: f32 = 1.5;
const PANEL_SIZE: Vec2 = Vec2::new(7.5, 4.4);
/// The question along the panel's top, the puzzle under it, and the time left along its foot.
const PROMPT_Y: f32 = 3.3;
const CONTENT_Y: f32 = 1.2;
const TIMER_Y: f32 = -0.55;
const TIMER_WIDTH: f32 = 7.0;
/// The answers: the diamond's centre, how far its slots stand from it, and a slot's size.
const DIAMOND_Y: f32 = -2.4;
const SLOT_REACH: Vec2 = Vec2::new(2.45, 1.05);
const SLOT_SIZE: Vec2 = Vec2::new(2.3, 0.85);
/// The lantern folk stand either side of a lone board.
const FOLK_Y: f32 = -3.4;
const FOLK_SIDE: (f32, f32) = (4.6, 7.4);
const FOLK_SCALE: f32 = 0.72;
/// Depths: panels, then the puzzle, then crosses and markers.
const PANEL_Z: f32 = -0.5;
const CONTENT_Z: f32 = 0.2;
const CROSS_Z: f32 = 0.5;
const MARKER_Z: f32 = 0.6;
const MARKER_SCALE: f32 = 1.6;
const STAR_COUNT: i32 = 60;

const CREAM: LinearRgba = LinearRgba::rgb(1.0, 0.9, 0.75);
const WRONG_RED: LinearRgba = LinearRgba::rgb(1.0, 0.12, 0.08);
const RIGHT_GOLD: LinearRgba = LinearRgba::rgb(1.0, 0.75, 0.25);
/// Each token's colour, by [`Token::index`]; also the counting puzzle's lantern colours 1–3.
/// Chosen apart from the seat colours, and every token has its own shape too.
const TOKEN_COLORS: [LinearRgba; 4] = [
    LinearRgba::rgb(1.0, 0.85, 0.35),
    LinearRgba::rgb(0.62, 0.45, 1.0),
    LinearRgba::rgb(1.0, 0.36, 0.3),
    LinearRgba::rgb(0.3, 1.0, 0.8),
];
/// Team colours for the panel borders in 2v2.
const TEAM_COLORS: [LinearRgba; 2] = [
    LinearRgba::rgb(1.0, 0.78, 0.3),
    LinearRgba::rgb(0.55, 0.45, 1.0),
];

/// Meshes and materials the puzzles are built from, made once per scene.
#[derive(Resource)]
struct Kit {
    tokens: [Handle<Mesh>; 4],
    token_materials: [Handle<StandardMaterial>; 4],
    dot: Handle<Mesh>,
    dot_material: Handle<StandardMaterial>,
    paper: Handle<StandardMaterial>,
    rope: Handle<StandardMaterial>,
    wood: Handle<StandardMaterial>,
    prize: Handle<StandardMaterial>,
    bar: Handle<Mesh>,
}

/// Everything drawn for the current puzzle; despawned when the next one comes.
#[derive(Component)]
struct PuzzleContent;

/// An answer slot's panel.
#[derive(Component)]
struct SlotView {
    half: usize,
    dir: Dir,
    material: Handle<StandardMaterial>,
}

/// The red cross over a wrong answer.
#[derive(Component)]
struct CrossView {
    half: usize,
    dir: Dir,
}

#[derive(Component)]
struct MarkerView(Seat);

#[derive(Component)]
struct TimerView;

#[derive(Component)]
struct FolkView(Seat);

#[derive(Component)]
struct CameraRig;

/// UI text pinned to a place on a board.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
enum Label {
    /// The question along the panel's top.
    Prompt(usize),
    /// A sum or a sequence, in the panel's middle.
    Expression(usize),
    /// A number in an answer slot.
    Answer(usize, Dir),
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

fn board_x(game: &State, half: usize) -> f32 {
    if game.halves() == 2 {
        [-HALF_X, HALF_X][half]
    } else {
        0.0
    }
}

fn slot_pos(game: &State, half: usize, dir: Dir) -> Vec2 {
    Vec2::new(board_x(game, half), DIAMOND_Y) + dir_offset(dir) * SLOT_REACH
}

fn content_centre(game: &State, half: usize) -> Vec2 {
    Vec2::new(board_x(game, half), CONTENT_Y)
}

/// Where each seat's folk stands: in 1v3 the solo alone on the left and the three on the right,
/// otherwise the first half of the seats left and the rest right, in seat order left to right.
fn folk_x(game: &State, seat: Seat) -> f32 {
    let seats: Vec<Seat> = game.playing().collect();
    let (left, right): (Vec<Seat>, Vec<Seat>) = match game.solo() {
        Some(solo) => seats.iter().partition(|s| **s == solo),
        None => {
            let split = seats.len().div_ceil(2);
            (seats[..split].to_vec(), seats[split..].to_vec())
        }
    };
    let (near, far) = FOLK_SIDE;
    // Spread evenly from `from` to `to`.
    let place = |row: &[Seat], from: f32, to: f32| {
        let index = row.iter().position(|s| *s == seat)? as f32;
        Some(from + (to - from) * (index + 0.5) / row.len() as f32)
    };
    place(&left, -far, -near)
        .or_else(|| place(&right, near, far))
        .unwrap_or(0.0)
}

fn team_index(format: &Format, seat: Seat) -> Option<u8> {
    match format {
        Format::TwoVsTwo { .. } => format.team(seat),
        _ => None,
    }
}

fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    active: Res<ActiveRun<PuzzleBuzzer>>,
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

    let star = meshes.add(Circle::new(0.035));
    let star_material = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: LinearRgba::rgb(0.8, 0.85, 1.0) * 0.8,
        ..default()
    });
    for k in 0..STAR_COUNT {
        let x = (noise(k, 2, 40) * 2.0 - 1.0) * 8.5;
        let y = (noise(k, 2, 41) * 2.0 - 1.0) * 5.5;
        commands.spawn((
            SCOPE,
            Mesh3d(star.clone()),
            MeshMaterial3d(star_material.clone()),
            Transform::from_xyz(x, y, -2.0).with_scale(Vec3::splat(0.6 + noise(k, 2, 42))),
        ));
    }

    let kit = Kit {
        tokens: Token::ALL.map(|token| meshes.add(token_mesh(token))),
        token_materials: TOKEN_COLORS.map(|color| {
            materials.add(StandardMaterial {
                base_color: Color::BLACK,
                emissive: color * 2.2,
                ..default()
            })
        }),
        dot: meshes.add(Circle::new(1.0)),
        dot_material: materials.add(StandardMaterial {
            base_color: Color::BLACK,
            emissive: LinearRgba::rgb(0.02, 0.02, 0.05),
            unlit: true,
            ..default()
        }),
        paper: materials.add(StandardMaterial {
            base_color: Color::BLACK,
            emissive: CREAM * 1.6,
            ..default()
        }),
        rope: materials.add(StandardMaterial {
            base_color: Color::BLACK,
            emissive: LinearRgba::rgb(0.75, 0.6, 0.42) * 1.1,
            ..default()
        }),
        wood: materials.add(StandardMaterial {
            base_color: Color::BLACK,
            emissive: LinearRgba::rgb(0.5, 0.4, 0.3) * 0.9,
            ..default()
        }),
        prize: materials.add(StandardMaterial {
            base_color: Color::BLACK,
            emissive: LANTERN_WARM * 5.0,
            ..default()
        }),
        bar: meshes.add(Rectangle::new(1.0, 1.0)),
    };

    let game = &active.run.game;
    let panel_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.05, 0.06, 0.1, 1.0),
        emissive: LinearRgba::rgb(0.02, 0.025, 0.05),
        perceptual_roughness: 1.0,
        ..default()
    });
    let panel = meshes.add(Rectangle::new(PANEL_SIZE.x, PANEL_SIZE.y));
    let slot = meshes.add(Rectangle::new(SLOT_SIZE.x, SLOT_SIZE.y));
    let cross_material = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: WRONG_RED * 3.0,
        ..default()
    });
    let timer_material = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: CREAM * 1.2,
        ..default()
    });
    for half in 0..game.halves() {
        let x = board_x(game, half);
        commands.spawn((
            SCOPE,
            Mesh3d(panel.clone()),
            MeshMaterial3d(panel_material.clone()),
            Transform::from_xyz(x, PANEL_Y, PANEL_Z),
        ));
        commands.spawn((
            SCOPE,
            TimerView,
            Mesh3d(kit.bar.clone()),
            MeshMaterial3d(timer_material.clone()),
            Transform::from_xyz(x, TIMER_Y, CONTENT_Z).with_scale(Vec3::new(
                TIMER_WIDTH,
                0.06,
                1.0,
            )),
        ));
        for dir in Dir::ALL {
            let at = slot_pos(game, half, dir);
            let material = materials.add(slot_material(SlotLook::Plain));
            commands.spawn((
                SCOPE,
                SlotView {
                    half,
                    dir,
                    material: material.clone(),
                },
                Mesh3d(slot.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(at.extend(PANEL_Z)),
            ));
            // A cross at the slot's right end, clear of the answer in its middle.
            commands
                .spawn((
                    SCOPE,
                    CrossView { half, dir },
                    Transform::from_translation(
                        (at + Vec2::X * (SLOT_SIZE.x / 2.0 - 0.35)).extend(CROSS_Z),
                    ),
                    Visibility::Hidden,
                ))
                .with_children(|cross| {
                    for angle in [PI / 4.0, -PI / 4.0] {
                        cross.spawn((
                            Mesh3d(kit.bar.clone()),
                            MeshMaterial3d(cross_material.clone()),
                            Transform::from_rotation(Quat::from_rotation_z(angle))
                                .with_scale(Vec3::new(0.5, 0.08, 1.0)),
                        ));
                    }
                });
        }
    }

    for seat in game.playing() {
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
        // Two boards leave no room for the folk; the panels say who's who.
        if game.halves() == 1 {
            let folk = spawn_lantern_folk(
                &mut commands,
                &mut meshes,
                &mut materials,
                AppState::Minigame,
                seat,
                roster.of(seat),
                Transform::from_xyz(folk_x(game, seat), FOLK_Y, 0.0)
                    .with_scale(Vec3::splat(FOLK_SCALE)),
            );
            commands.entity(folk.root).insert(FolkView(seat));
        }
    }
    commands.insert_resource(kit);
}

fn token_mesh(token: Token) -> Mesh {
    match token {
        Token::Circle => Circle::new(0.5).into(),
        Token::Square => Rectangle::new(0.85, 0.85).into(),
        Token::Triangle => Triangle2d::new(
            Vec2::new(0.0, 0.55),
            Vec2::new(-0.55, -0.42),
            Vec2::new(0.55, -0.42),
        )
        .into(),
        Token::Diamond => Rhombus::new(0.8, 1.15).into(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SlotLook {
    Plain,
    Wrong,
    Right,
    /// Shown, but neither: the other answers once the right one is out.
    Faded,
}

fn slot_material(look: SlotLook) -> StandardMaterial {
    let (base, glow) = match look {
        SlotLook::Plain => (
            Color::srgb(0.1, 0.11, 0.17),
            LinearRgba::rgb(0.05, 0.06, 0.1),
        ),
        SlotLook::Wrong => (Color::srgb(0.12, 0.03, 0.03), WRONG_RED * 0.12),
        SlotLook::Right => (Color::srgb(0.2, 0.15, 0.05), RIGHT_GOLD * 0.35),
        SlotLook::Faded => (
            Color::srgb(0.05, 0.05, 0.08),
            LinearRgba::rgb(0.02, 0.02, 0.04),
        ),
    };
    StandardMaterial {
        base_color: base,
        emissive: glow,
        perceptual_roughness: 1.0,
        ..default()
    }
}

/// How an answer slot looks. Before the puzzle ends only wrong lock-ins show; a right one in 2v2
/// stays secret.
fn slot_look(game: &State, half: usize, dir: Dir) -> SlotLook {
    let board = &game.boards[half];
    if !game.shown() {
        SlotLook::Plain
    } else if board.wrong[dir.index()] {
        SlotLook::Wrong
    } else if game.revealed() && board.right == dir {
        SlotLook::Right
    } else if game.revealed() {
        SlotLook::Faded
    } else {
        SlotLook::Plain
    }
}

/// The slot a seat's marker sits in: its lock-in, once it's out in the open (a wrong one at
/// once, any once the puzzle ends).
fn marker_slot(game: &State, seat: Seat) -> Option<(usize, Dir)> {
    let answerer = game.answerer(seat);
    let dir = answerer.lock?;
    let board = game.board_of(seat);
    (game.shown() && (game.revealed() || dir != board.right))
        .then_some((answerer.half as usize, dir))
}

/// Builds the drawing for each board when a new puzzle comes up (or the run restarts).
fn rebuild_puzzles(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    active: Res<ActiveRun<PuzzleBuzzer>>,
    kit: Res<Kit>,
    content: Query<Entity, With<PuzzleContent>>,
    mut built: Local<Option<(u32, u8, bool)>>,
) {
    let game = &active.run.game;
    let started = !matches!(game.phase, Phase::Intro { .. });
    let now = (active.generation, game.index, started);
    if *built == Some(now) {
        return;
    }
    *built = Some(now);
    for entity in &content {
        commands.entity(entity).despawn();
    }
    if !started {
        return;
    }
    for half in 0..game.halves() {
        let mut draw = Draw {
            commands: &mut commands,
            meshes: &mut meshes,
            kit: &kit,
        };
        draw.puzzle(game, half);
        for dir in Dir::ALL {
            let at = slot_pos(game, half, dir).extend(CONTENT_Z);
            match game.boards[half].answers[dir.index()] {
                Answer::Number(_) => {}
                Answer::Token(token) => draw.token(token, at, 0.32),
                Answer::Glyph(glyph) => draw.glyph(glyph, at, 0.34),
            }
        }
    }
}

/// Spawns one puzzle's shapes.
struct Draw<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    meshes: &'a mut Assets<Mesh>,
    kit: &'a Kit,
}

impl Draw<'_, '_, '_> {
    fn spawn(
        &mut self,
        mesh: Handle<Mesh>,
        material: Handle<StandardMaterial>,
        transform: Transform,
    ) {
        self.commands.spawn((
            SCOPE,
            PuzzleContent,
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
            Visibility::Hidden,
        ));
    }

    fn token(&mut self, token: Token, at: Vec3, size: f32) {
        self.spawn(
            self.kit.tokens[token.index()].clone(),
            self.kit.token_materials[token.index()].clone(),
            Transform::from_translation(at).with_scale(Vec3::splat(size)),
        );
    }

    fn bar(
        &mut self,
        material: Handle<StandardMaterial>,
        from: Vec2,
        to: Vec2,
        width: f32,
        z: f32,
    ) {
        let along = to - from;
        self.spawn(
            self.kit.bar.clone(),
            material,
            Transform::from_translation(((from + to) / 2.0).extend(z))
                .with_rotation(Quat::from_rotation_z(along.y.atan2(along.x)))
                .with_scale(Vec3::new(along.length(), width, 1.0)),
        );
    }

    /// A glyph of circumradius `radius`: a paper polygon with dark dots in its marked corners.
    fn glyph(&mut self, glyph: Glyph, at: Vec3, radius: f32) {
        let mesh = self
            .meshes
            .add(RegularPolygon::new(radius, glyph.sides as u32));
        let turn = Quat::from_rotation_z(glyph.turn as f32 * TAU / TURNS as f32);
        self.spawn(
            mesh,
            self.kit.paper.clone(),
            Transform::from_translation(at).with_rotation(turn),
        );
        for corner in 0..glyph.sides {
            if glyph.dots & (1 << corner) == 0 {
                continue;
            }
            // Corner 0 is at the top, the rest anticlockwise, as Bevy's polygon has them.
            let angle = FRAC_PI_2 + corner as f32 * TAU / glyph.sides as f32;
            let offset = turn * (Vec3::new(angle.cos(), angle.sin(), 0.0) * radius * 0.6);
            self.spawn(
                self.kit.dot.clone(),
                self.kit.dot_material.clone(),
                Transform::from_translation(at + offset + Vec3::Z * 0.01)
                    .with_scale(Vec3::splat(radius * 0.17)),
            );
        }
    }

    fn puzzle(&mut self, game: &State, half: usize) {
        let centre = content_centre(game, half);
        let z = CONTENT_Z;
        match game.boards[half].puzzle {
            Puzzle::Count { grid, colour } => {
                let cell = 0.72;
                for (i, c) in grid.iter().enumerate() {
                    if *c == 0 {
                        continue;
                    }
                    let (col, row) = ((i % COUNT_COLUMNS) as f32, (i / COUNT_COLUMNS) as f32);
                    // A little jitter, so they look scattered rather than tabled.
                    let jitter =
                        Vec2::new(noise(i as i32, 0, 7) - 0.5, noise(i as i32, 0, 8) - 0.5)
                            * cell
                            * 0.3;
                    let at = centre
                        + Vec2::new(
                            (col - (COUNT_COLUMNS as f32 - 1.0) / 2.0) * cell,
                            ((COUNT_ROWS as f32 - 1.0) / 2.0 - row) * cell * 0.86,
                        )
                        + jitter;
                    self.token(Token::ALL[*c as usize - 1], at.extend(z), 0.42);
                }
                // The one to count, in the middle of the answers.
                let middle = Vec2::new(board_x(game, half), DIAMOND_Y);
                self.token(Token::ALL[colour as usize - 1], middle.extend(z), 0.55);
            }
            Puzzle::Sum { .. } | Puzzle::Sequence { .. } => {}
            Puzzle::OddOneOut => {
                for dir in Dir::ALL {
                    if let Answer::Glyph(glyph) = game.boards[half].answers[dir.index()] {
                        let at = centre + dir_offset(dir) * Vec2::new(2.2, 1.15);
                        self.glyph(glyph, at.extend(z), 0.62);
                    }
                }
            }
            Puzzle::Ladder {
                tops,
                rungs,
                len,
                prize,
            } => {
                let rope_x = |rope: u8| centre.x + (rope as f32 - 1.5) * 1.6;
                let (top, bottom) = (centre.y + 1.25, centre.y - 1.3);
                for rope in 0..4u8 {
                    let x = rope_x(rope);
                    self.bar(
                        self.kit.rope.clone(),
                        Vec2::new(x, top),
                        Vec2::new(x, bottom),
                        0.06,
                        z,
                    );
                    self.token(tops[rope as usize], Vec3::new(x, top + 0.3, z), 0.4);
                    let (material, size) = if rope == prize {
                        (self.kit.prize.clone(), 0.2)
                    } else {
                        (self.kit.rope.clone(), 0.06)
                    };
                    self.spawn(
                        self.kit.dot.clone(),
                        material,
                        Transform::from_xyz(x, bottom - 0.12, z).with_scale(Vec3::splat(size)),
                    );
                }
                let len = len as usize;
                for (k, rung) in rungs[..len].iter().enumerate() {
                    let y = top - 0.15 - (k as f32 + 0.5) * (top - bottom - 0.3) / len as f32;
                    self.bar(
                        self.kit.rope.clone(),
                        Vec2::new(rope_x(*rung), y),
                        Vec2::new(rope_x(rung + 1), y),
                        0.06,
                        z,
                    );
                }
            }
            Puzzle::Scales { scales, weights } => {
                for (s, [left, right]) in scales.iter().enumerate() {
                    let x = centre.x + (s as f32 - 1.0) * 2.45;
                    let pivot = Vec2::new(x, centre.y + 0.25);
                    let foot = centre.y - 1.25;
                    self.bar(self.kit.wood.clone(), Vec2::new(x, foot), pivot, 0.08, z);
                    self.bar(
                        self.kit.wood.clone(),
                        Vec2::new(x - 0.45, foot),
                        Vec2::new(x + 0.45, foot),
                        0.08,
                        z,
                    );
                    // Tipped down on the heavier side.
                    let tilt = if weights[left.index()] > weights[right.index()] {
                        0.3
                    } else {
                        -0.3
                    };
                    let arm = Vec2::from_angle(tilt) * 0.95;
                    self.bar(self.kit.wood.clone(), pivot - arm, pivot + arm, 0.07, z);
                    for (token, end) in [(left, pivot - arm), (right, pivot + arm)] {
                        self.token(*token, (end + Vec2::Y * 0.3).extend(z), 0.42);
                    }
                }
            }
        }
    }
}

fn show_content(
    active: Res<ActiveRun<PuzzleBuzzer>>,
    mut content: Query<&mut Visibility, With<PuzzleContent>>,
) {
    let shown = active.run.game.shown();
    for mut visibility in &mut content {
        visibility.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

fn show_slots(
    active: Res<ActiveRun<PuzzleBuzzer>>,
    slots: Query<&SlotView>,
    mut crosses: Query<(&CrossView, &mut Visibility)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut looks: Local<Vec<(usize, Dir, SlotLook)>>,
) {
    let game = &active.run.game;
    for slot in &slots {
        let look = slot_look(game, slot.half, slot.dir);
        let known = looks
            .iter_mut()
            .find(|(half, dir, _)| *half == slot.half && *dir == slot.dir);
        let changed = match known {
            Some(entry) if entry.2 == look => false,
            Some(entry) => {
                entry.2 = look;
                true
            }
            None => {
                looks.push((slot.half, slot.dir, look));
                true
            }
        };
        if changed && let Some(mut material) = materials.get_mut(&slot.material) {
            *material = slot_material(look);
        }
    }
    for (cross, mut visibility) in &mut crosses {
        let wrong = slot_look(game, cross.half, cross.dir) == SlotLook::Wrong;
        visibility.set_if_neq(if wrong {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// Markers sit in a row at the left end of the slot their seat locked in.
fn place_markers(
    active: Res<ActiveRun<PuzzleBuzzer>>,
    mut markers: Query<(&MarkerView, &mut Transform, &mut Visibility)>,
) {
    let game = &active.run.game;
    for (MarkerView(seat), mut transform, mut visibility) in &mut markers {
        let Some((half, dir)) = marker_slot(game, *seat) else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let row: Vec<Seat> = game
            .playing()
            .filter(|s| marker_slot(game, *s) == Some((half, dir)))
            .collect();
        let index = row.iter().position(|s| s == seat).unwrap_or(0) as f32;
        let at =
            slot_pos(game, half, dir) + Vec2::new(-SLOT_SIZE.x / 2.0 + 0.25 + index * 0.3, 0.0);
        transform.translation = at.extend(MARKER_Z);
        visibility.set_if_neq(Visibility::Inherited);
    }
}

/// The time left, as a bar along the foot of each puzzle that shrinks to the middle.
fn show_timers(
    active: Res<ActiveRun<PuzzleBuzzer>>,
    mut timers: Query<(&TimerView, &mut Transform, &mut Visibility)>,
) {
    let game = &active.run.game;
    for (_, mut transform, mut visibility) in &mut timers {
        let Some(left) = game.ticks_left() else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(Visibility::Inherited);
        transform.scale.x = TIMER_WIDTH * left as f32 / ANSWER_TICKS as f32;
    }
}

fn animate_folk(active: Res<ActiveRun<PuzzleBuzzer>>, mut folk: Query<(&FolkView, &mut Pose)>) {
    let game = &active.run.game;
    let winners: Vec<Seat> = game
        .outcome
        .map(|o| o.winners().collect())
        .unwrap_or_default();
    for (FolkView(seat), mut pose) in &mut folk {
        let team = game.team(*seat);
        let wrong = game.answerer(*seat).wrong(game.board_of(*seat));
        let next = if game.outcome.is_some() {
            if winners.contains(seat) {
                Pose::Celebrate
            } else {
                Pose::Slump
            }
        } else if game.revealed() && game.took[team] {
            Pose::Hop
        } else if game.shown() && (wrong || game.out[team]) {
            Pose::Slump
        } else {
            Pose::Idle
        };
        pose.set_if_neq(next);
    }
}

/// Sounds and sparks, once per event: a new puzzle, a wrong lock-in, the last seconds, and who
/// took it. A right half in 2v2 locks in quietly, so the other team can't hear it.
fn react(
    mut commands: Commands,
    active: Res<ActiveRun<PuzzleBuzzer>>,
    effects: Res<Effects>,
    sfx: Sfx,
) {
    let Some((before, now)) = active.ticked() else {
        return;
    };
    if matches!(now.phase, Phase::Showing { t: 0 }) {
        sfx.play(Sound::TurnStart);
    }
    if matches!(now.phase, Phase::Answering { t: 0 }) {
        sfx.play_with(Sound::Draw, 1.2, -6.0);
    }
    for half in 0..now.halves() {
        for dir in Dir::ALL {
            if now.boards[half].wrong[dir.index()] && !before.boards[half].wrong[dir.index()] {
                sfx.play(Sound::FalseStart);
                let at = slot_pos(now, half, dir).extend(CROSS_Z);
                particles::burst(&mut commands, &effects, SCOPE, at, hdr(WRONG_RED, 2.0));
            }
        }
    }
    if let Some(left) = now.ticks_left()
        && left <= 3 * TICK_HZ as u16
        && left % TICK_HZ as u16 == 0
    {
        sfx.play(Sound::Tick);
    }
    if !before.revealed() && now.revealed() && now.outcome.is_none() {
        if now.took.iter().any(|t| *t) {
            sfx.play(Sound::RoundWin);
            for half in 0..now.halves() {
                let at = slot_pos(now, half, now.boards[half].right).extend(CROSS_Z);
                particles::burst(&mut commands, &effects, SCOPE, at, hdr(RIGHT_GOLD, 6.0));
            }
        } else {
            sfx.play(Sound::TooSlow);
        }
    }
    if before.outcome.is_none() && now.outcome.is_some() {
        sfx.play(Sound::Win);
    }
}

fn camera_transform() -> Transform {
    Transform::from_xyz(0.0, 0.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y)
}

/// Orthographic and straight on, so the answers sit square, like the stick's directions.
fn camera_projection() -> Projection {
    Projection::Orthographic(OrthographicProjection {
        scaling_mode: ScalingMode::AutoMin {
            min_width: FRAMED_WIDTH,
            min_height: FRAMED_HEIGHT,
        },
        ..OrthographicProjection::default_3d()
    })
}

/// Label boxes are this big, centred on where they're pinned: as wide as a board for a sum or a
/// sequence.
const LABEL_SIZE: Vec2 = Vec2::new(560.0, 110.0);
const WIDE_LABEL_SIZE: Vec2 = Vec2::new(900.0, 110.0);

fn label_size(label: Label) -> Vec2 {
    match label {
        Label::Expression(_) => WIDE_LABEL_SIZE,
        _ => LABEL_SIZE,
    }
}

fn spawn_hud(mut commands: Commands, active: Res<ActiveRun<PuzzleBuzzer>>) {
    commands
        .spawn((
            SCOPE,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(10.0),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(2.0),
                ..default()
            },
        ))
        .with_children(|top| {
            top.spawn((
                HudText::Banner,
                Text::new(""),
                TextFont::from_font_size(34.0),
                TextColor(Color::srgb(1.0, 0.93, 0.8)),
                TextLayout::justify(Justify::Center),
            ));
            top.spawn((
                HudText::SubBanner,
                Text::new(""),
                TextFont::from_font_size(18.0),
                TextColor(Color::srgba(0.85, 0.88, 1.0, 0.85)),
                TextLayout::justify(Justify::Center),
            ));
        });

    let game = &active.run.game;
    let mut labels = Vec::new();
    for half in 0..game.halves() {
        labels.push((Label::Prompt(half), 24.0));
        labels.push((Label::Expression(half), 60.0));
        for dir in Dir::ALL {
            labels.push((Label::Answer(half, dir), 36.0));
        }
    }
    for (label, size) in labels {
        commands
            .spawn((
                SCOPE,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(label_size(label).x),
                    height: Val::Px(label_size(label).y),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
            ))
            .with_child((
                label,
                Text::new(""),
                TextFont::from_font_size(size),
                TextColor(Color::from(CREAM)),
                TextLayout::justify(Justify::Center),
            ));
    }

    commands
        .spawn((
            SCOPE,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(10.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(16.0),
                ..default()
            },
        ))
        .with_children(|row| {
            for seat in panel_order(game) {
                let border = match team_index(&game.format, seat) {
                    Some(team) => Color::from(TEAM_COLORS[team as usize]),
                    None => Color::NONE,
                };
                row.spawn((
                    Node {
                        width: Val::Px(215.0),
                        padding: UiRect::axes(Val::Px(12.0), Val::Px(5.0)),
                        border: UiRect::all(Val::Px(3.0)),
                        ..default()
                    },
                    BorderColor::all(border),
                    BackgroundColor(Color::from(seat_color(seat) * 0.25).with_alpha(0.55)),
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

/// Panels left to right: in 2v2 the left board's two answerers first, so each pair sits under
/// its board; in 1v3 the solo first, as its folk stands on the left.
fn panel_order(game: &State) -> Vec<Seat> {
    let mut seats: Vec<Seat> = game.playing().collect();
    seats.sort_by_key(|s| (game.answerer(*s).half, Some(*s) != game.solo(), s.0));
    seats
}

/// What a sum looks like: "7 + 8 x 2".
fn sum_text(terms: &[i32; 3], ops: &[Op; 2], len: u8) -> String {
    let mut text = terms[0].to_string();
    for i in 1..len as usize {
        // The UI font has no × or −.
        let op = match ops[i - 1] {
            Op::Add => "+",
            Op::Sub => "-",
            Op::Mul => "x",
        };
        text.push_str(&format!(" {op} {}", terms[i]));
    }
    text
}

fn prompt_key(kind: Kind) -> &'static str {
    match kind {
        Kind::Count => "puzzle-buzzer-count",
        Kind::Sum => "puzzle-buzzer-sum",
        Kind::Sequence => "puzzle-buzzer-sequence",
        Kind::OddOneOut => "puzzle-buzzer-odd",
        Kind::Ladder => "puzzle-buzzer-ladder",
        Kind::Scales => "puzzle-buzzer-scales",
    }
}

fn label_text(game: &State, label: Label) -> String {
    if matches!(game.phase, Phase::Intro { .. }) {
        return String::new();
    }
    match label {
        Label::Prompt(half) => {
            let prompt = tr!(prompt_key(game.boards[half].kind));
            if game.halves() == 2 {
                let who = names(
                    game.playing()
                        .filter(|s| game.answerer(*s).half as usize == half),
                );
                format!("{who}\n{prompt}")
            } else {
                prompt
            }
        }
        Label::Expression(half) if game.shown() => match game.boards[half].puzzle {
            Puzzle::Sum { terms, ops, len } => format!("{} = ?", sum_text(&terms, &ops, len)),
            Puzzle::Sequence { terms } => {
                let shown: Vec<String> = terms.iter().map(|t| t.to_string()).collect();
                format!("{}, ?", shown.join(", "))
            }
            _ => String::new(),
        },
        Label::Answer(half, dir) if game.shown() => match game.boards[half].answers[dir.index()] {
            Answer::Number(n) => n.to_string(),
            _ => String::new(),
        },
        Label::Expression(_) | Label::Answer(..) => String::new(),
    }
}

fn label_pos(game: &State, label: Label) -> Vec2 {
    match label {
        Label::Prompt(half) => {
            let lift = if game.halves() == 2 { 0.15 } else { 0.0 };
            Vec2::new(board_x(game, half), PROMPT_Y + lift)
        }
        Label::Expression(half) => content_centre(game, half),
        Label::Answer(half, dir) => slot_pos(game, half, dir),
    }
}

/// Pin each label to its place on screen, and fill it in.
fn place_labels(
    active: Res<ActiveRun<PuzzleBuzzer>>,
    camera: Query<(&Camera, &GlobalTransform), With<CameraRig>>,
    mut labels: Query<(&Label, &mut Text, &mut TextColor, &ChildOf)>,
    mut boxes: Query<&mut Node>,
) {
    let Some((camera, camera_transform)) = camera.iter().next() else {
        return;
    };
    let game = &active.run.game;
    for (label, mut text, mut color, parent) in &mut labels {
        let says = label_text(game, *label);
        if text.0 != says {
            text.0 = says;
        }
        let faded = match *label {
            Label::Answer(half, dir) => matches!(
                slot_look(game, half, dir),
                SlotLook::Wrong | SlotLook::Faded
            ),
            _ => false,
        };
        color.0 = Color::from(CREAM).with_alpha(if faded { 0.35 } else { 1.0 });
        let at = label_pos(game, *label);
        if let (Ok(screen), Ok(mut node)) = (
            camera.world_to_viewport(camera_transform, at.extend(MARKER_Z)),
            boxes.get_mut(parent.parent()),
        ) {
            let size = label_size(*label);
            node.left = Val::Px(screen.x - size.x / 2.0);
            node.top = Val::Px(screen.y - size.y / 2.0);
        }
    }
}

/// Who took the puzzle, or that nobody did.
fn result_line(game: &State) -> String {
    let takers: Vec<Seat> = game
        .playing()
        .filter(|s| game.took[game.team(*s)])
        .collect();
    if !takers.is_empty() {
        tr!(
            "puzzle-buzzer-took",
            players = names(takers.iter().copied())
        )
    } else if game.teams().iter().all(|t| game.out[*t]) {
        tr!("puzzle-buzzer-all-wrong")
    } else {
        tr!("puzzle-buzzer-time")
    }
}

fn banner_text(active: &ActiveRun<PuzzleBuzzer>) -> (String, String) {
    let run = &active.run;
    let game = &run.game;
    if let Some(banner) = halted_banner(run) {
        return banner;
    }
    let puzzle = || tr!("puzzle-buzzer-puzzle", number = game.index as u32 + 1);
    let goal = || match game.solo() {
        Some(_) => tr!("puzzle-buzzer-goal-line-solo"),
        None => tr!("puzzle-buzzer-goal-line", goal = GOAL),
    };
    match game.phase {
        // The shared countdown (crate::intro) is drawn over this.
        Phase::Intro { .. } => (String::new(), String::new()),
        Phase::Showing { .. } => (puzzle(), goal()),
        Phase::Answering { .. } => {
            let secs = game.ticks_left().unwrap_or(0) as u32;
            (
                puzzle(),
                tr!("puzzle-buzzer-left", secs = secs.div_ceil(TICK_HZ)),
            )
        }
        Phase::Result { .. } => (puzzle(), result_line(game)),
        Phase::Decided { .. } | Phase::Finished => {
            let outcome = game.outcome.expect("decided games have an outcome");
            let winners: Vec<Seat> = outcome.winners().collect();
            let prompt = if game.phase == Phase::Finished {
                active.continue_prompt()
            } else {
                result_line(game)
            };
            (i18n::winners(&winners), prompt)
        }
    }
}

/// A seat's line under its score.
fn seat_status(game: &State, seat: Seat) -> String {
    let answerer = game.answerer(seat);
    let team = game.team(seat);
    let wrong = answerer.wrong(game.board_of(seat));
    match game.phase {
        Phase::Intro { .. } | Phase::Showing { .. } => String::new(),
        Phase::Answering { .. } if wrong => tr!("puzzle-buzzer-wrong"),
        Phase::Answering { .. } if game.out[team] => tr!("puzzle-buzzer-out"),
        // Only a 2v2 half can be locked in and still going; it doesn't say whether it's right.
        Phase::Answering { .. } if answerer.lock.is_some() => tr!("puzzle-buzzer-locked"),
        Phase::Answering { .. } if game.wait_left(seat) > 0 => tr!("puzzle-buzzer-wait"),
        Phase::Answering { .. } => tr!("puzzle-buzzer-thinking"),
        Phase::Result { .. } | Phase::Decided { .. } | Phase::Finished => {
            if game.took[team] {
                tr!("puzzle-buzzer-got-it")
            } else if wrong {
                tr!("puzzle-buzzer-wrong")
            } else {
                String::new()
            }
        }
    }
}

fn update_hud(
    active: Res<ActiveRun<PuzzleBuzzer>>,
    seats: Res<Seats>,
    mut texts: Query<(&HudText, &mut Text)>,
) {
    let (title, subtitle) = banner_text(&active);
    let game = &active.run.game;
    for (hud, mut text) in &mut texts {
        let says = match *hud {
            HudText::Banner => title.clone(),
            HudText::SubBanner => subtitle.clone(),
            HudText::SeatPanel(seat) => {
                let who = bot_tag(seats.is_bot(seat));
                let solo = game.solo() == Some(seat);
                let role = if solo { tr!("solo-tag") } else { String::new() };
                let team = game.team(seat);
                let score = tr!(
                    "puzzle-buzzer-points",
                    points = game.points[team],
                    goal = game.goal(team)
                );
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
                format!("{}{who}{role}  {score}\n{status}", seat_label(seat))
            }
        };
        if text.0 != says {
            text.0 = says;
        }
    }
}

/// Everything a board shows, for tests that compare what two games would show.
#[cfg(test)]
fn board_shown(game: &State) -> Vec<String> {
    let mut shown = Vec::new();
    for half in 0..game.halves() {
        shown.push(label_text(game, Label::Prompt(half)));
        shown.push(label_text(game, Label::Expression(half)));
        for dir in Dir::ALL {
            shown.push(label_text(game, Label::Answer(half, dir)));
            shown.push(format!("{:?}", slot_look(game, half, dir)));
        }
    }
    for seat in Seat::ALL {
        shown.push(format!("{:?}", marker_slot(game, seat)));
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;
    use minigame_puzzle_buzzer::puzzle::evaluate;
    use sim_core::{Button, Input, MAX_PLAYERS, PlayerSetup, Run, RunConfig};

    /// The answers stand where the stick points: up is up on screen, right is right.
    #[test]
    fn the_answers_are_seen_straight_on() {
        assert!(matches!(camera_projection(), Projection::Orthographic(_)));
        let forward = camera_transform().forward();
        assert!(forward.abs_diff_eq(Vec3::NEG_Z, 1e-6), "{forward}");
        let run = Run::<PuzzleBuzzer>::new(
            1,
            PlayerSetup::first(4),
            Format::FreeForAll,
            RunConfig::default(),
        );
        let game = &run.game;
        assert!(slot_pos(game, 0, Dir::Up).y > slot_pos(game, 0, Dir::Down).y);
        assert!(slot_pos(game, 0, Dir::Right).x > slot_pos(game, 0, Dir::Left).x);
    }

    /// Both boards, their slots and the folk fit in the frame without overlapping.
    #[test]
    fn everything_fits() {
        let two = Run::<PuzzleBuzzer>::new(
            1,
            PlayerSetup::first(4),
            Format::TwoVsTwo {
                team_of: [0, 1, 0, 1],
            },
            RunConfig::default(),
        );
        let right_edge = board_x(&two.game, 1) + PANEL_SIZE.x / 2.0;
        assert!(right_edge <= FRAMED_WIDTH / 2.0, "{right_edge}");
        assert!(
            board_x(&two.game, 1) - PANEL_SIZE.x / 2.0 > board_x(&two.game, 0) + PANEL_SIZE.x / 2.0
        );
        let game = &two.game;
        let left_slot = slot_pos(game, 0, Dir::Left).x - SLOT_SIZE.x / 2.0;
        assert!(left_slot >= -FRAMED_WIDTH / 2.0, "{left_slot}");
        assert!(slot_pos(game, 0, Dir::Up).y + SLOT_SIZE.y / 2.0 < PANEL_Y - PANEL_SIZE.y / 2.0);
        let one = Run::<PuzzleBuzzer>::new(
            1,
            PlayerSetup::first(4),
            Format::FreeForAll,
            RunConfig::default(),
        );
        for seat in Seat::ALL {
            let x = folk_x(&one.game, seat).abs();
            assert!(
                x > PANEL_SIZE.x / 2.0 && x < FRAMED_WIDTH / 2.0,
                "{seat:?} at {x}"
            );
        }
        let solo = Run::<PuzzleBuzzer>::new(
            1,
            PlayerSetup::first(4),
            Format::OneVsThree { solo: Seat(2) },
            RunConfig::default(),
        );
        assert!(folk_x(&solo.game, Seat(2)) < 0.0);
        for seat in [0, 1, 3] {
            assert!(folk_x(&solo.game, Seat(seat)) > 0.0);
        }
    }

    /// Nothing the view shows may depend on where anyone points, nor on which answer a 2v2 half
    /// locked in right before the puzzle ends (its panel just says it's locked in): run two games
    /// that differ only in that and compare.
    #[test]
    fn aims_and_right_halves_stay_secret() {
        for format in [
            Format::FreeForAll,
            Format::OneVsThree { solo: Seat(1) },
            Format::TwoVsTwo {
                team_of: [0, 1, 0, 1],
            },
        ] {
            let new =
                || Run::<PuzzleBuzzer>::new(7, PlayerSetup::first(4), format, RunConfig::default());
            let (mut a, mut b) = (new(), new());
            let mut checked = 0;
            while !a.game.revealed() {
                let two_v_two = matches!(format, Format::TwoVsTwo { .. });
                let inputs = |shift: usize, game: &State| -> [Input; MAX_PLAYERS] {
                    std::array::from_fn(|i| {
                        let seat = Seat(i as u8);
                        let right = game.board_of(seat).right;
                        // Everyone points somewhere wrong (each run a different wrong)...
                        let stick = Dir::ALL[(right.index() + 1 + shift + i) % 4];
                        let stick = if stick == right {
                            Dir::ALL[(right.index() + 3) % 4]
                        } else {
                            stick
                        }
                        .stick();
                        // ...and in one run of 2v2, P1 locks its half in right.
                        match game.phase {
                            Phase::Answering { t: 30 } if two_v_two && i == 0 && shift == 0 => {
                                right.stick().with(Button::A)
                            }
                            _ => stick,
                        }
                    })
                };
                let (to_a, to_b) = (inputs(0, &a.game), inputs(1, &b.game));
                a.step(to_a);
                b.step(to_b);
                if matches!(a.game.phase, Phase::Answering { t } if t > 200) {
                    break;
                }
                assert_eq!(board_shown(&a.game), board_shown(&b.game));
                // P1's panel may say it locked in, but no other.
                for seat in Seat::ALL.into_iter().skip(1) {
                    assert_eq!(seat_status(&a.game, seat), seat_status(&b.game, seat));
                }
                checked += 1;
            }
            assert!(checked > 200, "{checked}");
        }
    }

    #[test]
    fn sums_read_naturally() {
        assert_eq!(sum_text(&[7, 8, 2], &[Op::Add, Op::Mul], 3), "7 + 8 x 2");
        assert_eq!(sum_text(&[7, 8, 0], &[Op::Add, Op::Add], 2), "7 + 8");
        assert_eq!(evaluate(&[7, 8, 2], &[Op::Add, Op::Mul]), 23);
    }
}
