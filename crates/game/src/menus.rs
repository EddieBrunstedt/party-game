//! The screens around the game: title, join (controllers claim seats and pick characters),
//! the free-play minigame picker, and the results after a board game.
//!
//! Menus read devices directly rather than seats, because before the join screen nobody owns
//! a seat yet: any controller (or the keyboard) can drive the title menu.

use std::collections::BTreeMap;

use bevy::input::gamepad::GamepadButton;
use bevy::prelude::*;
use sim_board::{Board, Game, Rules};
use sim_core::{Difficulty, Format, FormatKind, MAX_PLAYERS, PlayerSetup, Seat};

use crate::board_view::{BoardSession, format_text, minigame_title};
use crate::characters::{Character, Roster};
use crate::flow::{AppState, MinigameLaunch, Origin};
use crate::palette::{MOONLIGHT, NIGHT_SKY, seat_color, seat_label};
use crate::runner::{Controller, Pad, RunSetup, Seats};
use crate::scenery::{night_camera, spawn_fireflies, spawn_lantern_folk, spawn_paper_lantern};
use crate::{GAME_TITLE, minigames, tr};

pub struct MenusPlugin;

impl Plugin for MenusPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DeviceNav>()
            .init_resource::<PlayMode>()
            .add_systems(PreUpdate, read_devices.after(bevy::input::InputSystems))
            .add_systems(OnEnter(AppState::Title), spawn_title)
            .add_systems(Update, title_menu.run_if(in_state(AppState::Title)))
            .add_systems(OnEnter(AppState::Join), spawn_join)
            .add_systems(
                Update,
                (join_lobby, show_lobby)
                    .chain()
                    .run_if(in_state(AppState::Join)),
            )
            .add_systems(OnEnter(AppState::Pick), spawn_pick)
            .add_systems(Update, pick_minigame.run_if(in_state(AppState::Pick)))
            .add_systems(OnEnter(AppState::Results), spawn_results)
            .add_systems(Update, results_screen.run_if(in_state(AppState::Results)));
    }
}

/// What the player chose on the title screen.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayMode {
    #[default]
    Board,
    FreePlay,
}

/// A physical input device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Device {
    Keyboard,
    Pad(Entity),
}

/// One device's menu presses this frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct Nav {
    pub confirm: bool,
    pub back: bool,
    pub start: bool,
    pub extra: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

/// Every connected device's presses this frame.
#[derive(Resource, Default)]
pub struct DeviceNav(pub Vec<(Device, Nav)>);

impl DeviceNav {
    /// Presses from any device, merged.
    pub fn any(&self) -> Nav {
        let mut all = Nav::default();
        for (_, nav) in &self.0 {
            all.confirm |= nav.confirm;
            all.back |= nav.back;
            all.start |= nav.start;
            all.extra |= nav.extra;
            all.up |= nav.up;
            all.down |= nav.down;
            all.left |= nav.left;
            all.right |= nav.right;
        }
        all
    }
}

fn read_devices(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<(Entity, &Gamepad)>,
    mut nav: ResMut<DeviceNav>,
    mut sticks: Local<BTreeMap<Entity, Vec2>>,
) {
    nav.0.clear();
    let key = |codes: &[KeyCode]| codes.iter().any(|k| keys.just_pressed(*k));
    nav.0.push((
        Device::Keyboard,
        Nav {
            confirm: key(&[KeyCode::Space, KeyCode::KeyJ]),
            back: key(&[KeyCode::KeyK, KeyCode::Escape, KeyCode::Backspace]),
            start: key(&[KeyCode::Enter]),
            extra: key(&[KeyCode::KeyL]),
            up: key(&[KeyCode::KeyW, KeyCode::ArrowUp]),
            down: key(&[KeyCode::KeyS, KeyCode::ArrowDown]),
            left: key(&[KeyCode::KeyA, KeyCode::ArrowLeft]),
            right: key(&[KeyCode::KeyD, KeyCode::ArrowRight]),
        },
    ));
    for (entity, pad) in &gamepads {
        // A stick flick counts like a d-pad press: on the frame it leaves the centre.
        let stick = pad.left_stick();
        let before = sticks.insert(entity, stick).unwrap_or(Vec2::ZERO);
        let flick = |now: f32, was: f32, sign: f32| now * sign > 0.6 && was * sign <= 0.6;
        nav.0.push((
            Device::Pad(entity),
            Nav {
                confirm: pad.just_pressed(GamepadButton::South),
                back: pad.just_pressed(GamepadButton::East),
                start: pad.just_pressed(GamepadButton::Start),
                extra: pad.just_pressed(GamepadButton::West),
                up: pad.just_pressed(GamepadButton::DPadUp) || flick(stick.y, before.y, 1.0),
                down: pad.just_pressed(GamepadButton::DPadDown) || flick(stick.y, before.y, -1.0),
                left: pad.just_pressed(GamepadButton::DPadLeft) || flick(stick.x, before.x, -1.0),
                right: pad.just_pressed(GamepadButton::DPadRight) || flick(stick.x, before.x, 1.0),
            },
        ));
    }
}

/// Move a selection through `len` items, wrapping.
fn step(index: usize, len: usize, prev: bool, next: bool) -> usize {
    if len == 0 {
        0
    } else if prev {
        (index + len - 1) % len
    } else if next {
        (index + 1) % len
    } else {
        index
    }
}

fn wall_clock_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// The night-meadow backdrop the menus share: a camera, lanterns, fireflies and a line of
/// lantern folk.
fn spawn_backdrop(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    scope: AppState,
    roster: &Roster,
    order: &[Seat],
) {
    commands.insert_resource(ClearColor(NIGHT_SKY));
    commands.insert_resource(GlobalAmbientLight {
        color: MOONLIGHT,
        brightness: 60.0,
        ..default()
    });
    let camera = Transform::from_xyz(0.0, 2.6, 9.0).looking_at(Vec3::new(0.0, 1.3, 0.0), Vec3::Y);
    commands.spawn((DespawnOnExit(scope), night_camera(camera, (10.0, 30.0))));
    commands.spawn((
        DespawnOnExit(scope),
        DirectionalLight {
            color: MOONLIGHT,
            illuminance: 400.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-4.0, 8.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DespawnOnExit(scope),
        Mesh3d(meshes.add(Cuboid::new(30.0, 0.4, 14.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.05, 0.08, 0.06),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.2, -2.0),
    ));
    for (x, color) in [
        (-5.5, LinearRgba::rgb(1.0, 0.3, 0.55)),
        (5.5, LinearRgba::rgb(0.25, 0.75, 1.0)),
    ] {
        spawn_paper_lantern(
            commands,
            meshes,
            materials,
            scope,
            Vec3::new(x, 0.0, -1.5),
            color,
        );
    }
    spawn_fireflies(
        commands,
        meshes,
        materials,
        scope,
        50,
        Vec3::new(-9.0, 0.4, -6.0),
        Vec3::new(9.0, 4.5, 3.0),
    );
    let count = order.len() as f32;
    for (i, seat) in order.iter().enumerate() {
        let x = (i as f32 - (count - 1.0) / 2.0) * 2.0;
        let folk = spawn_lantern_folk(
            commands,
            meshes,
            materials,
            scope,
            *seat,
            roster.of(*seat),
            Transform::from_xyz(x, 0.0, 1.0),
        );
        commands.entity(folk.root).insert(MenuFolk(*seat));
    }
}

/// A character standing in a menu scene, so it can be rebuilt when someone picks another.
#[derive(Component)]
struct MenuFolk(Seat);

/// A centred column of text, `top` pixels from the top (or, if negative, from the bottom).
fn text_column(commands: &mut Commands, scope: AppState, top: f32) -> Entity {
    let (top, bottom) = if top >= 0.0 {
        (Val::Px(top), Val::Auto)
    } else {
        (Val::Auto, Val::Px(-top))
    };
    commands
        .spawn((
            DespawnOnExit(scope),
            Node {
                position_type: PositionType::Absolute,
                top,
                bottom,
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(10.0),
                ..default()
            },
        ))
        .id()
}

fn line(text: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont::from_font_size(size),
        TextColor(color),
        TextLayout::justify(Justify::Center),
    )
}

const CREAM: Color = Color::srgb(1.0, 0.93, 0.8);
const DIM: Color = Color::srgba(0.85, 0.88, 1.0, 0.75);

// ---------------------------------------------------------------------------------------------
// Title

#[derive(Resource, Default)]
struct TitleMenu {
    selected: usize,
}

const TITLE_ITEMS: [&str; 3] = ["title-board", "title-free-play", "title-quit"];

#[derive(Component)]
struct TitleItem(usize);

fn spawn_title(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.init_resource::<TitleMenu>();
    commands.remove_resource::<Lobby>();
    spawn_backdrop(
        &mut commands,
        &mut meshes,
        &mut materials,
        AppState::Title,
        &Roster::default(),
        &Seat::ALL,
    );
    let column = text_column(&mut commands, AppState::Title, 70.0);
    commands.entity(column).with_children(|column| {
        column.spawn(line(GAME_TITLE, 84.0, CREAM));
        column.spawn(line(tr!("title-tagline"), 22.0, DIM));
    });
    let menu = text_column(&mut commands, AppState::Title, -60.0);
    commands.entity(menu).with_children(|menu| {
        for (i, key) in TITLE_ITEMS.iter().enumerate() {
            menu.spawn((TitleItem(i), line(tr!(key), 32.0, CREAM)));
        }
        menu.spawn(line(tr!("title-controls"), 18.0, DIM));
    });
}

fn title_menu(
    nav: Res<DeviceNav>,
    mut menu: ResMut<TitleMenu>,
    mut items: Query<(&TitleItem, &mut Text, &mut TextColor)>,
    mut mode: ResMut<PlayMode>,
    mut next_state: ResMut<NextState<AppState>>,
    mut exit: MessageWriter<AppExit>,
) {
    let any = nav.any();
    menu.selected = step(menu.selected, TITLE_ITEMS.len(), any.up, any.down);
    if any.confirm || any.start {
        match menu.selected {
            0 => {
                *mode = PlayMode::Board;
                next_state.set(AppState::Join);
            }
            1 => {
                *mode = PlayMode::FreePlay;
                next_state.set(AppState::Join);
            }
            _ => {
                exit.write(AppExit::Success);
            }
        }
    }
    for (item, mut text, mut color) in &mut items {
        let label = tr!(TITLE_ITEMS[item.0]);
        let selected = item.0 == menu.selected;
        text.0 = if selected {
            format!("> {label} <")
        } else {
            label
        };
        color.0 = if selected { CREAM } else { DIM };
    }
}

// ---------------------------------------------------------------------------------------------
// Join

/// Who has joined, on what device, as which character; and the session settings.
#[derive(Resource, Clone)]
pub struct Lobby {
    joined: [Option<(Device, Character)>; MAX_PLAYERS],
    bots: Difficulty,
    rounds: u32,
}

const ROUND_CHOICES: [u32; 5] = [6, 9, 12, 15, 20];

impl Default for Lobby {
    fn default() -> Self {
        Self {
            joined: [None; MAX_PLAYERS],
            bots: Difficulty::Normal,
            rounds: Rules::default().rounds,
        }
    }
}

impl Lobby {
    fn seat_of(&self, device: Device) -> Option<Seat> {
        Seat::ALL
            .into_iter()
            .find(|s| matches!(self.joined[s.index()], Some((d, _)) if d == device))
    }

    fn taken(&self, character: Character, except: Seat) -> bool {
        Seat::ALL.into_iter().any(|s| {
            s != except && matches!(self.joined[s.index()], Some((_, c)) if c == character)
        })
    }

    /// The next free character after `from` in `direction` (+1 or -1).
    fn next_character(&self, seat: Seat, from: usize, direction: isize) -> Character {
        let len = Character::ALL.len() as isize;
        for k in 1..=len {
            let index = (from as isize + direction * k).rem_euclid(len) as usize;
            let character = Character::ALL[index];
            if !self.taken(character, seat) {
                return character;
            }
        }
        Character::ALL[from]
    }

    /// Characters per seat: humans keep theirs, bots take whoever is left.
    fn roster(&self) -> Roster {
        let mut roster = [Character::PaperLantern; MAX_PLAYERS];
        let mut free: Vec<Character> = Character::ALL
            .into_iter()
            .filter(|c| !self.joined.iter().flatten().any(|(_, taken)| taken == c))
            .collect();
        for seat in Seat::ALL {
            roster[seat.index()] = match self.joined[seat.index()] {
                Some((_, character)) => character,
                None => free.remove(0),
            };
        }
        Roster(roster)
    }

    fn seats(&self) -> Seats {
        Seats(Seat::ALL.map(|seat| {
            Some(match self.joined[seat.index()] {
                Some((Device::Keyboard, _)) => Controller::Human {
                    gamepad: Pad::None,
                    keyboard: true,
                },
                Some((Device::Pad(entity), _)) => Controller::Human {
                    gamepad: Pad::Bound(entity),
                    keyboard: false,
                },
                None => Controller::Bot(self.bots),
            })
        }))
    }
}

#[derive(Component)]
enum LobbyText {
    Slot(Seat),
    Settings,
}

fn spawn_join(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Option<Res<Lobby>>,
) {
    // Coming back from the free-play picker keeps everyone joined; the title starts afresh.
    let lobby = existing.map(|l| l.clone()).unwrap_or_default();
    spawn_backdrop(
        &mut commands,
        &mut meshes,
        &mut materials,
        AppState::Join,
        &lobby.roster(),
        &Seat::ALL,
    );
    commands.insert_resource(lobby);
    let column = text_column(&mut commands, AppState::Join, 40.0);
    commands.entity(column).with_children(|column| {
        column.spawn(line(tr!("join-title"), 52.0, CREAM));
        column.spawn(line(tr!("join-hint"), 20.0, DIM));
    });
    commands
        .spawn((
            DespawnOnExit(AppState::Join),
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(110.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(16.0),
                ..default()
            },
        ))
        .with_children(|row| {
            for seat in Seat::ALL {
                row.spawn((
                    Node {
                        width: Val::Px(250.0),
                        padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
                        ..default()
                    },
                    BackgroundColor(Color::from(seat_color(seat) * 0.25).with_alpha(0.6)),
                ))
                .with_child((
                    LobbyText::Slot(seat),
                    Text::new(""),
                    TextFont::from_font_size(18.0),
                    TextColor(Color::WHITE),
                ));
            }
        });
    let bottom = text_column(&mut commands, AppState::Join, -24.0);
    commands.entity(bottom).with_children(|bottom| {
        bottom.spawn((LobbyText::Settings, line("", 20.0, DIM)));
    });
}

fn join_lobby(
    nav: Res<DeviceNav>,
    mode: Res<PlayMode>,
    mut lobby: ResMut<Lobby>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let mut begin = false;
    for (device, press) in nav.0.clone() {
        match lobby.seat_of(device) {
            None => {
                if press.confirm {
                    // Take the first free seat, with the first character nobody has.
                    if let Some(seat) = Seat::ALL
                        .into_iter()
                        .find(|s| lobby.joined[s.index()].is_none())
                    {
                        let character = lobby.next_character(seat, Character::ALL.len() - 1, 1);
                        lobby.joined[seat.index()] = Some((device, character));
                    }
                } else if press.back && lobby.joined.iter().all(Option::is_none) {
                    next_state.set(AppState::Title);
                    return;
                }
            }
            Some(seat) => {
                let (_, character) = lobby.joined[seat.index()].expect("joined");
                let index = Character::ALL
                    .iter()
                    .position(|c| *c == character)
                    .unwrap_or(0);
                if press.left || press.right {
                    let direction = if press.left { -1 } else { 1 };
                    let picked = lobby.next_character(seat, index, direction);
                    lobby.joined[seat.index()] = Some((device, picked));
                }
                if press.back {
                    lobby.joined[seat.index()] = None;
                }
                if press.extra {
                    lobby.bots = match lobby.bots {
                        Difficulty::Easy => Difficulty::Normal,
                        Difficulty::Normal => Difficulty::Hard,
                        Difficulty::Hard => Difficulty::Easy,
                    };
                }
                if *mode == PlayMode::Board && (press.up || press.down) {
                    let at = ROUND_CHOICES
                        .iter()
                        .position(|r| *r == lobby.rounds)
                        .unwrap_or(2);
                    let at = step(at, ROUND_CHOICES.len(), press.down, press.up);
                    lobby.rounds = ROUND_CHOICES[at];
                }
                begin |= press.start;
            }
        }
    }
    if !begin {
        return;
    }
    commands.insert_resource(lobby.seats());
    commands.insert_resource(lobby.roster());
    match *mode {
        PlayMode::Board => {
            let rules = Rules {
                rounds: lobby.rounds,
                ..Rules::default()
            };
            let game = Game::new(
                Board::meadow(),
                rules,
                MAX_PLAYERS,
                wall_clock_seed(),
                &minigames::shown_entries(),
            );
            commands.insert_resource(BoardSession::new(game));
            next_state.set(AppState::Board);
        }
        PlayMode::FreePlay => next_state.set(AppState::Pick),
    }
}

fn difficulty_key(difficulty: Difficulty) -> &'static str {
    match difficulty {
        Difficulty::Easy => "difficulty-easy",
        Difficulty::Normal => "difficulty-normal",
        Difficulty::Hard => "difficulty-hard",
    }
}

fn show_lobby(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    lobby: Res<Lobby>,
    mode: Res<PlayMode>,
    mut texts: Query<(&LobbyText, &mut Text)>,
    folk: Query<(Entity, &MenuFolk, &Transform)>,
    mut shown: Local<Option<Roster>>,
) {
    for (label, mut text) in &mut texts {
        text.0 = match label {
            LobbyText::Slot(seat) => match lobby.joined[seat.index()] {
                Some((device, character)) => {
                    let how = match device {
                        Device::Keyboard => tr!("join-keyboard"),
                        Device::Pad(_) => tr!("join-gamepad"),
                    };
                    format!("{}  ({how})\n< {} >", seat_label(*seat), character.name())
                }
                None => format!(
                    "{}\n{}",
                    seat_label(*seat),
                    tr!("join-press-a", difficulty = tr!(difficulty_key(lobby.bots)))
                ),
            },
            LobbyText::Settings => {
                let rounds = match *mode {
                    PlayMode::Board => tr!("join-rounds", rounds = lobby.rounds),
                    PlayMode::FreePlay => String::new(),
                };
                let start = if lobby.joined.iter().any(Option::is_some) {
                    tr!("join-start")
                } else {
                    String::new()
                };
                format!(
                    "{}   {rounds}\n{start}",
                    tr!("join-bots", difficulty = tr!(difficulty_key(lobby.bots)))
                )
            }
        };
    }
    // Rebuild a character in the line-up when its pick changes.
    let roster = lobby.roster();
    let before = shown.unwrap_or_default();
    for (entity, menu_folk, transform) in &folk {
        let seat = menu_folk.0;
        if shown.is_some() && before.of(seat) == roster.of(seat) {
            continue;
        }
        let transform = *transform;
        commands.entity(entity).despawn();
        let rebuilt = spawn_lantern_folk(
            &mut commands,
            &mut meshes,
            &mut materials,
            AppState::Join,
            seat,
            roster.of(seat),
            transform,
        );
        commands.entity(rebuilt.root).insert(MenuFolk(seat));
    }
    *shown = Some(roster);
}

// ---------------------------------------------------------------------------------------------
// Free-play picker

#[derive(Resource, Default)]
struct PickMenu {
    minigame: usize,
    format: usize,
}

#[derive(Component)]
struct PickText;

/// Formats a minigame offers in free play (always four seats; bots fill the gaps).
fn free_play_formats(kinds: &[FormatKind]) -> Vec<Format> {
    kinds
        .iter()
        .filter_map(|kind| match kind {
            FormatKind::FreeForAll => Some(Format::FreeForAll),
            FormatKind::OneVsThree => Some(Format::OneVsThree { solo: Seat(0) }),
            FormatKind::TwoVsTwo => Some(Format::TwoVsTwo {
                team_of: [0, 1, 0, 1],
            }),
            FormatKind::Duel => None,
        })
        .collect()
}

fn spawn_pick(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    roster: Res<Roster>,
) {
    commands.init_resource::<PickMenu>();
    spawn_backdrop(
        &mut commands,
        &mut meshes,
        &mut materials,
        AppState::Pick,
        &roster,
        &Seat::ALL,
    );
    let column = text_column(&mut commands, AppState::Pick, 50.0);
    commands.entity(column).with_children(|column| {
        column.spawn(line(tr!("pick-title"), 52.0, CREAM));
        column.spawn((PickText, line("", 26.0, CREAM)));
        column.spawn(line(tr!("pick-hint"), 18.0, DIM));
    });
}

fn pick_minigame(
    nav: Res<DeviceNav>,
    seats: Res<Seats>,
    mut menu: ResMut<PickMenu>,
    mut text: Query<&mut Text, With<PickText>>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let entries = minigames::shown_entries();
    let any = nav.any();
    menu.minigame = step(menu.minigame, entries.len(), any.up, any.down);
    if any.up || any.down {
        menu.format = 0;
    }
    let formats = free_play_formats(entries[menu.minigame].formats);
    menu.format = step(menu.format, formats.len(), any.left, any.right);
    if any.back {
        next_state.set(AppState::Join);
        return;
    }
    if any.confirm || any.start {
        let players = PlayerSetup::first(seats.0.iter().flatten().count().max(2));
        commands.insert_resource(MinigameLaunch {
            id: entries[menu.minigame].id.to_string(),
            setup: RunSetup {
                seed: wall_clock_seed(),
                players,
                format: formats[menu.format],
                config: default(),
            },
            origin: Origin::FreePlay,
        });
        next_state.set(AppState::Minigame);
        return;
    }
    let mut lines = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        let title = minigame_title(entry.id);
        if i == menu.minigame {
            let format = format_text(&formats[menu.format], Seat::ALL.into_iter());
            lines.push(format!("> {title} <   [ {format} ]"));
        } else {
            lines.push(title);
        }
    }
    for mut text in &mut text {
        text.0 = lines.join("\n");
    }
}

// ---------------------------------------------------------------------------------------------
// Results

fn spawn_results(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    roster: Res<Roster>,
    session: Option<Res<BoardSession>>,
) {
    let Some(session) = session else {
        // Nothing to show (e.g. reached by mistake): go back to the title.
        commands.insert_resource(NextState::Pending(AppState::Title));
        return;
    };
    let game = &session.game;
    let standings = game.standings();
    let mut order: Vec<Seat> = game.players().seats().collect();
    order.sort_by_key(|s| (standings.placement[s.index()], s.0));
    spawn_backdrop(
        &mut commands,
        &mut meshes,
        &mut materials,
        AppState::Results,
        &roster,
        &order,
    );
    let column = text_column(&mut commands, AppState::Results, 40.0);
    commands.entity(column).with_children(|column| {
        let winners: Vec<String> = standings.winners().map(seat_label).collect();
        column.spawn(line(
            tr!(
                "results-title",
                winners = winners.join(" & "),
                count = winners.len() as u32
            ),
            56.0,
            CREAM,
        ));
        for seat in &order {
            let player = game.player(*seat);
            column.spawn(line(
                tr!(
                    "results-line",
                    place = standings.placement[seat.index()].unwrap_or(0) as u32,
                    player = seat_label(*seat),
                    character = roster.of(*seat).name(),
                    stars = player.stars,
                    coins = player.coins
                ),
                24.0,
                Color::from(seat_color(*seat) * 1.2),
            ));
        }
    });
    let bottom = text_column(&mut commands, AppState::Results, -24.0);
    commands.entity(bottom).with_children(|bottom| {
        bottom.spawn(line(tr!("results-continue"), 20.0, DIM));
    });
}

fn results_screen(
    time: Res<Time>,
    nav: Res<DeviceNav>,
    seats: Res<Seats>,
    session: Option<Res<BoardSession>>,
    mut folk: Query<(&MenuFolk, &mut Transform)>,
    mut entered: Local<Option<f32>>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let now = time.elapsed_secs();
    let since = now - *entered.get_or_insert(now);
    if let Some(session) = &session {
        let standings = session.game.standings();
        for (menu_folk, mut transform) in &mut folk {
            if standings.placement[menu_folk.0.index()] == Some(1) {
                transform.translation.y = (now * 8.0).sin().abs() * 0.5;
            }
        }
    }
    let no_humans = Seat::ALL.into_iter().all(|s| !seats.is_human(s));
    let any = nav.any();
    if since > 1.5 && (any.confirm || any.start || (no_humans && since > 10.0)) {
        commands.remove_resource::<BoardSession>();
        *entered = None;
        next_state.set(AppState::Title);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humans_keep_their_characters_and_bots_take_the_rest() {
        let mut lobby = Lobby::default();
        lobby.joined[0] = Some((Device::Keyboard, Character::NeonRing));
        lobby.joined[2] = Some((Device::Pad(Entity::PLACEHOLDER), Character::PaperLantern));
        let roster = lobby.roster();
        assert_eq!(roster.of(Seat(0)), Character::NeonRing);
        assert_eq!(roster.of(Seat(2)), Character::PaperLantern);
        let mut all: Vec<_> = roster.0.to_vec();
        all.sort_by_key(|c| Character::ALL.iter().position(|x| x == c));
        assert_eq!(all, Character::ALL.to_vec(), "every character exactly once");
    }

    #[test]
    fn picking_skips_characters_someone_else_has() {
        let mut lobby = Lobby::default();
        lobby.joined[0] = Some((Device::Keyboard, Character::PaperLantern));
        lobby.joined[1] = Some((Device::Pad(Entity::PLACEHOLDER), Character::FireflyJar));
        // Seat 0 moving right from Paper Lantern skips seat 1's Firefly Jar.
        assert_eq!(lobby.next_character(Seat(0), 0, 1), Character::Candle);
        assert_eq!(lobby.next_character(Seat(0), 0, -1), Character::NeonRing);
    }

    #[test]
    fn unjoined_seats_become_bots() {
        let mut lobby = Lobby {
            bots: Difficulty::Hard,
            ..Lobby::default()
        };
        lobby.joined[1] = Some((Device::Keyboard, Character::Candle));
        let seats = lobby.seats();
        assert!(seats.is_human(Seat(1)));
        assert_eq!(seats.0[0], Some(Controller::Bot(Difficulty::Hard)));
    }
}
