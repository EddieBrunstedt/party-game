//! The screens around the game: title, join (controllers claim seats and pick characters),
//! the free-play minigame picker, and the results after a board game.
//!
//! Menus read devices directly rather than seats, because before the join screen nobody owns
//! a seat yet: any controller (or the keyboard) can drive the title menu.

use std::collections::BTreeMap;

use bevy::input::ButtonState;
use bevy::input::gamepad::GamepadButton;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use net::lobby::{LobbyAction, LobbyState, PeerId};
use sim_board::{Board, Game, Rules};
use sim_core::{Difficulty, Format, FormatKind, MAX_PLAYERS, PlayerSetup, Seat};

use crate::audio::{Sfx, Sound};
use crate::board_view::{BoardSession, format_text, minigame_title};
use crate::characters::{Character, Pose, Roster};
use crate::flow::{AppState, MinigameLaunch, Origin};
use crate::online::{self, NetSettings, Notice, Online, OnlineEvent, OnlineStart};
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
                (poll_online, join_lobby, show_lobby)
                    .chain()
                    .run_if(in_state(AppState::Join)),
            )
            .add_systems(OnEnter(AppState::Connect), spawn_connect)
            .add_systems(
                Update,
                (poll_online, connect_screen)
                    .chain()
                    .run_if(in_state(AppState::Connect)),
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

const TITLE_ITEMS: [&str; 5] = [
    "title-board",
    "title-free-play",
    "title-host",
    "title-join",
    "title-quit",
];

#[derive(Component)]
struct TitleItem(usize);

fn spawn_title(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    notice: Option<Res<Notice>>,
) {
    commands.init_resource::<TitleMenu>();
    commands.remove_resource::<Lobby>();
    commands.remove_resource::<ConnectScreen>();
    commands.remove_resource::<OnlineStart>();
    close_online(&mut commands);
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
        if let Some(notice) = &notice {
            column.spawn(line(notice.0.clone(), 22.0, CREAM));
        }
    });
    commands.remove_resource::<Notice>();
    let menu = text_column(&mut commands, AppState::Title, -30.0);
    commands.entity(menu).with_children(|menu| {
        for (i, key) in TITLE_ITEMS.iter().enumerate() {
            menu.spawn((TitleItem(i), line(tr!(key), 26.0, CREAM)));
        }
        menu.spawn(line(tr!("title-controls"), 18.0, DIM));
    });
}

#[allow(clippy::too_many_arguments)]
/// Starts hosting a board game, or says why it couldn't.
pub fn host_online(port: u16) -> Result<Online, Notice> {
    Online::host(port, Lobby::new(PlayMode::Board).state).map_err(|e| {
        Notice(tr!(
            "notice-host-failed",
            port = port as u32,
            error = e.to_string()
        ))
    })
}

fn title_menu(
    nav: Res<DeviceNav>,
    mut menu: ResMut<TitleMenu>,
    mut items: Query<(&TitleItem, &mut Text, &mut TextColor)>,
    mut mode: ResMut<PlayMode>,
    settings: Res<NetSettings>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
    mut exit: MessageWriter<AppExit>,
    sfx: Sfx,
) {
    let any = nav.any();
    menu.selected = step(menu.selected, TITLE_ITEMS.len(), any.up, any.down);
    if any.up || any.down {
        sfx.play(Sound::Move);
    }
    if any.confirm || any.start {
        sfx.play(Sound::Confirm);
        match menu.selected {
            0 => {
                *mode = PlayMode::Board;
                next_state.set(AppState::Join);
            }
            1 => {
                *mode = PlayMode::FreePlay;
                next_state.set(AppState::Join);
            }
            2 => {
                *mode = PlayMode::Board;
                match host_online(settings.port) {
                    Ok(online) => {
                        commands.insert_resource(online);
                        next_state.set(AppState::Join);
                    }
                    Err(notice) => {
                        commands.insert_resource(notice);
                        // Re-enter the title to show it.
                        next_state.set(AppState::Title);
                    }
                }
            }
            3 => next_state.set(AppState::Connect),
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

/// Who has joined, on what device, as which character; and the session settings. The rules
/// are `net`'s [`LobbyState`], the same on the couch (this machine is the host, alone) and
/// online (the host's copy is the real one, and a client shows what the host last sent).
#[derive(Resource, Clone)]
pub struct Lobby {
    state: LobbyState,
    /// This machine's controllers, numbered in the order they first pressed A here.
    devices: Vec<Device>,
}

impl Lobby {
    fn new(mode: PlayMode) -> Self {
        let state = LobbyState::new(
            Character::ALL.len() as u8,
            Rules::default().rounds,
            mode == PlayMode::FreePlay,
        );
        Self {
            state,
            devices: Vec::new(),
        }
    }

    fn local_of(&self, device: Device) -> Option<u8> {
        self.devices
            .iter()
            .position(|d| *d == device)
            .map(|i| i as u8)
    }

    fn local_id(&mut self, device: Device) -> u8 {
        self.local_of(device).unwrap_or_else(|| {
            self.devices.push(device);
            (self.devices.len() - 1) as u8
        })
    }

    /// Characters per seat: players keep theirs, bots take whoever is left.
    fn roster(&self) -> Roster {
        Roster(self.state.roster().map(|c| Character::ALL[c as usize]))
    }

    /// Couch seats: every claim is this machine's, read from the device that made it.
    fn seats(&self) -> Seats {
        Seats(Seat::ALL.map(|seat| {
            Some(match self.state.seats[seat.index()] {
                Some(claim) => match self.devices[claim.local as usize] {
                    Device::Keyboard => Controller::Human {
                        gamepad: Pad::None,
                        keyboard: true,
                    },
                    Device::Pad(entity) => Controller::Human {
                        gamepad: Pad::Bound(entity),
                        keyboard: false,
                    },
                },
                None => Controller::Bot(self.state.bots),
            })
        }))
    }
}

#[derive(Component)]
enum LobbyText {
    Slot(Seat),
    Status,
    Settings,
}

fn spawn_join(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mode: Res<PlayMode>,
    existing: Option<Res<Lobby>>,
    online: Option<Res<Online>>,
) {
    // Coming back from the free-play picker keeps everyone joined; the title starts afresh.
    let mut lobby = existing
        .map(|l| l.clone())
        .unwrap_or_else(|| Lobby::new(*mode));
    if let Some(state) = online.as_ref().and_then(|o| o.lobby()) {
        lobby.state = state.clone();
    }
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
        column.spawn((LobbyText::Status, line("", 18.0, DIM)));
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
    mut online: Option<ResMut<Online>>,
    started: Option<Res<OnlineStart>>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
    sfx: Sfx,
) {
    let Some(me) = online.as_ref().map_or(Some(PeerId::HOST), |o| o.me()) else {
        return;
    };
    if started.is_some() {
        return;
    }
    let host = me == PeerId::HOST;
    let mut actions = Vec::new();
    let mut begin = false;
    let mut leave = false;
    for (device, press) in nav.0.clone() {
        let local = lobby.local_of(device);
        match local.filter(|l| lobby.state.seat_of(me, *l).is_some()) {
            None => {
                if press.confirm {
                    actions.push(LobbyAction::Join {
                        local: lobby.local_id(device),
                    });
                    sfx.play(Sound::Join);
                } else if press.back && !lobby.state.peer_joined(me) {
                    leave = true;
                }
            }
            Some(local) => {
                if press.left || press.right {
                    actions.push(LobbyAction::Pick {
                        local,
                        next: press.right,
                    });
                    sfx.play(Sound::Move);
                }
                if press.back {
                    actions.push(LobbyAction::Leave { local });
                    sfx.play(Sound::Back);
                }
                if host && press.extra {
                    actions.push(LobbyAction::CycleBots);
                    sfx.play(Sound::Move);
                }
                if host && *mode == PlayMode::Board && (press.up || press.down) {
                    actions.push(LobbyAction::Rounds { longer: press.up });
                    sfx.play(Sound::Move);
                }
                begin |= press.start;
            }
        }
    }
    for action in actions {
        match online.as_mut() {
            Some(online) => online.act(action),
            None => {
                lobby.state.apply(PeerId::HOST, action);
            }
        }
    }
    if let Some(state) = online.as_ref().and_then(|o| o.lobby()) {
        lobby.state = state.clone();
    }
    if leave {
        sfx.play(Sound::Back);
        close_online(&mut commands);
        next_state.set(AppState::Title);
        return;
    }
    if !begin || !host {
        return;
    }
    sfx.play(Sound::Confirm);
    if let Some(Online::Host(session)) = online.as_deref_mut() {
        // The board protocol (next on the roadmap) takes the session from here.
        let start = session.start(wall_clock_seed());
        info!(
            "online session started with {} other machines",
            start.clients.len()
        );
        commands.insert_resource(OnlineStart(start));
        return;
    }
    commands.insert_resource(lobby.seats());
    commands.insert_resource(lobby.roster());
    match *mode {
        PlayMode::Board => {
            let rules = Rules {
                rounds: lobby.state.rounds,
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

/// Reads the online session every frame on the screens that have one, and acts on what
/// happened: welcomed, refused, started, or the host gone.
fn poll_online(
    mut online: Option<ResMut<Online>>,
    state: Res<State<AppState>>,
    mut connect: Option<ResMut<ConnectScreen>>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
    sfx: Sfx,
) {
    let Some(session) = online.as_deref_mut() else {
        return;
    };
    for event in session.poll() {
        match event {
            OnlineEvent::Welcomed => {
                if *state.get() == AppState::Connect {
                    sfx.play(Sound::Confirm);
                    commands.insert_resource(PlayMode::Board);
                    next_state.set(AppState::Join);
                }
            }
            OnlineEvent::Refused(refusal) => {
                if let Some(connect) = connect.as_deref_mut() {
                    connect.status = ConnectStatus::Failed(online::refusal_text(&refusal));
                }
                sfx.play(Sound::Back);
                commands.remove_resource::<Online>();
                return;
            }
            OnlineEvent::HostLeft => {
                sfx.play(Sound::Back);
                commands.remove_resource::<Online>();
                commands.remove_resource::<OnlineStart>();
                commands.insert_resource(Notice(tr!("notice-host-left")));
                next_state.set(AppState::Title);
                return;
            }
            OnlineEvent::Started(start) => {
                sfx.play(Sound::Confirm);
                info!("the host started the online session");
                commands.insert_resource(OnlineStart(start));
            }
            OnlineEvent::PeerJoined => sfx.play(Sound::Join),
            OnlineEvent::PeerLeft => sfx.play(Sound::Back),
        }
    }
}

/// Leaves the online session, if there is one, saying goodbye.
fn close_online(commands: &mut Commands) {
    commands.queue(|world: &mut World| {
        if let Some(online) = world.remove_resource::<Online>() {
            online.close();
        }
    });
}

fn difficulty_key(difficulty: Difficulty) -> &'static str {
    match difficulty {
        Difficulty::Easy => "difficulty-easy",
        Difficulty::Normal => "difficulty-normal",
        Difficulty::Hard => "difficulty-hard",
    }
}

/// The status line under the join screen's title: where friends can join a host, or which
/// host this client is in.
fn online_status(online: Option<&Online>, port: u16) -> String {
    match online {
        None => String::new(),
        Some(Online::Host(_)) => {
            let mut addresses = vec![tr!("join-hosting-here", port = port as u32)];
            for (kind, ip) in online::reachable_addresses() {
                let key = match kind {
                    "tailscale" => "join-hosting-tailscale",
                    _ => "join-hosting-lan",
                };
                addresses.push(tr!(key, address = ip.to_string(), port = port as u32));
            }
            tr!("join-hosting", addresses = addresses.join(",  "))
        }
        Some(Online::Client(client)) => {
            tr!("join-connected", address = client.host().to_string())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn show_lobby(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    lobby: Res<Lobby>,
    mode: Res<PlayMode>,
    online: Option<Res<Online>>,
    started: Option<Res<OnlineStart>>,
    settings: Res<NetSettings>,
    mut texts: Query<(&LobbyText, &mut Text)>,
    folk: Query<(Entity, &MenuFolk, &Transform)>,
    mut shown: Local<Option<Roster>>,
    mut status: Local<Option<String>>,
) {
    let me = online.as_ref().and_then(|o| o.me()).unwrap_or(PeerId::HOST);
    let host = me == PeerId::HOST;
    let difficulty = tr!(difficulty_key(lobby.state.bots));
    // Finding this machine's addresses asks the OS, so do it once per visit.
    if status.is_none() || lobby.is_added() {
        *status = Some(online_status(online.as_deref(), settings.port));
    }
    for (label, mut text) in &mut texts {
        text.0 = match label {
            LobbyText::Slot(seat) => match lobby.state.seats[seat.index()] {
                Some(claim) => {
                    let how = if claim.peer != me {
                        tr!("join-online")
                    } else {
                        match lobby.devices.get(claim.local as usize) {
                            Some(Device::Keyboard) => tr!("join-keyboard"),
                            _ => tr!("join-gamepad"),
                        }
                    };
                    let character = Character::ALL[claim.character as usize];
                    format!("{}  ({how})\n< {} >", seat_label(*seat), character.name())
                }
                None => format!(
                    "{}\n{}",
                    seat_label(*seat),
                    tr!("join-press-a", difficulty = difficulty.clone())
                ),
            },
            LobbyText::Status => status.clone().unwrap_or_default(),
            LobbyText::Settings => {
                if started.is_some() {
                    tr!("join-starting")
                } else if !host {
                    let settings = tr!(
                        "join-host-settings",
                        difficulty = difficulty.clone(),
                        rounds = lobby.state.rounds
                    );
                    format!("{settings}\n{}", tr!("join-waiting-host"))
                } else {
                    let rounds = match *mode {
                        PlayMode::Board => tr!("join-rounds", rounds = lobby.state.rounds),
                        PlayMode::FreePlay => String::new(),
                    };
                    let start = if lobby.state.peer_joined(me) {
                        tr!("join-start")
                    } else {
                        String::new()
                    };
                    format!(
                        "{}   {rounds}\n{start}",
                        tr!("join-bots", difficulty = difficulty.clone())
                    )
                }
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
// Joining an online game: type the host's address, then wait to be let in.

/// Characters the d-pad can dial in; the keyboard can type names too.
const DIAL: &[u8] = b"0123456789.:";
/// How long to wait for a host that doesn't answer at all.
const CONNECT_TIMEOUT_SECS: f32 = 8.0;

#[derive(Resource)]
pub struct ConnectScreen {
    address: String,
    /// Where the next character goes (or which one the d-pad changes).
    cursor: usize,
    status: ConnectStatus,
    connect_now: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum ConnectStatus {
    Editing,
    Connecting { since: f32, address: String },
    Failed(String),
}

impl ConnectScreen {
    pub fn new(address: String) -> Self {
        Self {
            cursor: address.len(),
            address,
            status: ConnectStatus::Editing,
            connect_now: false,
        }
    }

    /// Connects to `address` as soon as the screen opens (`--join`).
    pub fn connecting(address: String) -> Self {
        Self {
            connect_now: true,
            ..Self::new(address)
        }
    }

    fn insert(&mut self, c: char) {
        self.address.insert(self.cursor, c);
        self.cursor += 1;
    }

    fn delete_before(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.address.remove(self.cursor);
        }
    }

    /// The d-pad's up/down: turn the character at the cursor through [`DIAL`], or start a new
    /// one at the end.
    fn dial(&mut self, up: bool) {
        if self.cursor == self.address.len() {
            self.insert(if up { '0' } else { ':' });
            self.cursor -= 1;
            return;
        }
        let current = self.address.as_bytes()[self.cursor];
        let at = DIAL.iter().position(|c| *c == current).unwrap_or(0);
        let len = DIAL.len();
        let next = if up {
            (at + 1) % len
        } else {
            (at + len - 1) % len
        };
        self.address
            .replace_range(self.cursor..=self.cursor, &(DIAL[next] as char).to_string());
    }

    fn delete_at(&mut self) {
        if self.cursor < self.address.len() {
            self.address.remove(self.cursor);
        } else {
            self.delete_before();
        }
    }
}

#[derive(Component)]
enum ConnectText {
    Field,
    Status,
}

fn spawn_connect(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    screen: Option<Res<ConnectScreen>>,
) {
    if screen.is_none() {
        commands.insert_resource(ConnectScreen::new("127.0.0.1".into()));
    }
    spawn_backdrop(
        &mut commands,
        &mut meshes,
        &mut materials,
        AppState::Connect,
        &Roster::default(),
        &[],
    );
    let column = text_column(&mut commands, AppState::Connect, 60.0);
    commands.entity(column).with_children(|column| {
        column.spawn(line(tr!("connect-title"), 52.0, CREAM));
        column.spawn(line(tr!("connect-hint"), 20.0, DIM));
        column.spawn((ConnectText::Field, line("", 32.0, CREAM)));
        column.spawn(line(tr!("connect-actions"), 18.0, DIM));
        column.spawn((ConnectText::Status, line("", 22.0, CREAM)));
    });
}

#[allow(clippy::too_many_arguments)]
fn connect_screen(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut typed: MessageReader<KeyboardInput>,
    nav: Res<DeviceNav>,
    mut screen: ResMut<ConnectScreen>,
    online: Option<Res<Online>>,
    mut text: Query<(&ConnectText, &mut Text)>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
    sfx: Sfx,
) {
    let now = time.elapsed_secs();
    let pads = nav
        .0
        .iter()
        .filter(|(d, _)| matches!(d, Device::Pad(_)))
        .map(|(_, n)| *n);
    let mut pad = Nav::default();
    for n in pads {
        pad.confirm |= n.confirm;
        pad.back |= n.back;
        pad.extra |= n.extra;
        pad.up |= n.up;
        pad.down |= n.down;
        pad.left |= n.left;
        pad.right |= n.right;
    }
    let connecting = matches!(screen.status, ConnectStatus::Connecting { .. });
    let back = pad.back || keys.just_pressed(KeyCode::Escape);
    let connect =
        pad.confirm || keys.just_pressed(KeyCode::Enter) || std::mem::take(&mut screen.connect_now);

    if back {
        sfx.play(Sound::Back);
        if online.is_some() {
            close_online(&mut commands);
        }
        if connecting {
            screen.status = ConnectStatus::Editing;
        } else {
            next_state.set(AppState::Title);
        }
        return;
    }

    if !connecting {
        let mut edited = false;
        for key in typed.read() {
            if key.state != ButtonState::Pressed {
                continue;
            }
            match (&key.logical_key, &key.text) {
                (Key::Backspace, _) => screen.delete_before(),
                (Key::Delete, _) => screen.delete_at(),
                (Key::ArrowLeft, _) => screen.cursor = screen.cursor.saturating_sub(1),
                (Key::ArrowRight, _) => {
                    screen.cursor = (screen.cursor + 1).min(screen.address.len())
                }
                (_, Some(text)) => {
                    for c in text
                        .chars()
                        .filter(|c| c.is_ascii_alphanumeric() || ".:-".contains(*c))
                    {
                        screen.insert(c);
                    }
                }
                _ => continue,
            }
            edited = true;
        }
        if pad.left {
            screen.cursor = screen.cursor.saturating_sub(1);
        }
        if pad.right {
            screen.cursor = (screen.cursor + 1).min(screen.address.len());
        }
        if pad.up || pad.down {
            screen.dial(pad.up);
        }
        if pad.extra {
            screen.delete_at();
        }
        if edited || pad.left || pad.right || pad.up || pad.down || pad.extra {
            sfx.play(Sound::Move);
            screen.status = ConnectStatus::Editing;
        }
        if connect {
            match online::parse_address(&screen.address).and_then(|addr| {
                Online::join(addr)
                    .map(|online| (addr, online))
                    .map_err(|e| tr!("connect-socket-error", error = e.to_string()))
            }) {
                Ok((addr, online)) => {
                    sfx.play(Sound::Confirm);
                    commands.insert_resource(online);
                    screen.status = ConnectStatus::Connecting {
                        since: now,
                        address: addr.to_string(),
                    };
                }
                Err(message) => {
                    sfx.play(Sound::Back);
                    screen.status = ConnectStatus::Failed(message);
                }
            }
        }
    } else {
        typed.clear();
    }

    if let ConnectStatus::Connecting { since, address } = &screen.status
        && now - since > CONNECT_TIMEOUT_SECS
    {
        let message = tr!("connect-no-answer", address = address.clone());
        sfx.play(Sound::Back);
        close_online(&mut commands);
        screen.status = ConnectStatus::Failed(message);
    }

    let mut field = screen.address.clone();
    if !connecting {
        field.insert(screen.cursor, '|');
    }
    let status = match &screen.status {
        ConnectStatus::Editing => String::new(),
        ConnectStatus::Connecting { address, .. } => {
            tr!("connect-connecting", address = address.clone())
        }
        ConnectStatus::Failed(message) => message.clone(),
    };
    for (part, mut text) in &mut text {
        text.0 = match part {
            ConnectText::Field => field.clone(),
            ConnectText::Status => status.clone(),
        };
    }
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
    sfx: Sfx,
) {
    let entries = minigames::shown_entries();
    let any = nav.any();
    if any.up || any.down || any.left || any.right {
        sfx.play(Sound::Move);
    }
    menu.minigame = step(menu.minigame, entries.len(), any.up, any.down);
    if any.up || any.down {
        menu.format = 0;
    }
    let formats = free_play_formats(entries[menu.minigame].formats);
    menu.format = step(menu.format, formats.len(), any.left, any.right);
    if any.back {
        sfx.play(Sound::Back);
        next_state.set(AppState::Join);
        return;
    }
    if any.confirm || any.start {
        sfx.play(Sound::Confirm);
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
    mut folk: Query<(&MenuFolk, &mut Pose)>,
    mut entered: Local<Option<f32>>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
    sfx: Sfx,
) {
    let now = time.elapsed_secs();
    if session.is_none() {
        *entered = None;
        return;
    }
    if entered.is_none() {
        sfx.play(Sound::Win);
    }
    let since = now - *entered.get_or_insert(now);
    if let Some(session) = &session {
        let standings = session.game.standings();
        for (menu_folk, mut pose) in &mut folk {
            *pose = if standings.placement[menu_folk.0.index()] == Some(1) {
                Pose::Celebrate
            } else {
                Pose::Slump
            };
        }
    }
    let no_humans = Seat::ALL.into_iter().all(|s| !seats.is_human(s));
    let any = nav.any();
    if since > 1.5 && (any.confirm || any.start || (no_humans && since > 10.0)) {
        sfx.play(Sound::Confirm);
        commands.remove_resource::<BoardSession>();
        *entered = None;
        next_state.set(AppState::Title);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn join(lobby: &mut Lobby, device: Device) {
        let local = lobby.local_id(device);
        lobby.state.apply(PeerId::HOST, LobbyAction::Join { local });
    }

    #[test]
    fn devices_keep_their_seats_and_characters() {
        let mut lobby = Lobby::new(PlayMode::Board);
        let pad = Device::Pad(Entity::PLACEHOLDER);
        join(&mut lobby, pad);
        join(&mut lobby, Device::Keyboard);
        let local = lobby.local_of(Device::Keyboard).unwrap();
        lobby
            .state
            .apply(PeerId::HOST, LobbyAction::Pick { local, next: false });
        let roster = lobby.roster();
        assert_eq!(roster.of(Seat(0)), Character::PaperLantern);
        assert_eq!(roster.of(Seat(1)), Character::NeonRing);
        let seats = lobby.seats();
        assert_eq!(
            seats.0[0],
            Some(Controller::Human {
                gamepad: Pad::Bound(Entity::PLACEHOLDER),
                keyboard: false
            })
        );
        assert!(seats.is_human(Seat(1)));
    }

    #[test]
    fn unjoined_seats_become_bots() {
        let mut lobby = Lobby::new(PlayMode::Board);
        lobby.state.bots = Difficulty::Hard;
        join(&mut lobby, Device::Keyboard);
        let seats = lobby.seats();
        assert!(seats.is_human(Seat(0)));
        assert_eq!(seats.0[1], Some(Controller::Bot(Difficulty::Hard)));
    }

    #[test]
    fn the_address_dial_turns_digits_and_appends() {
        let mut screen = ConnectScreen::new("10.0".into());
        screen.dial(true);
        assert_eq!(screen.address, "10.00");
        screen.cursor = 0;
        screen.dial(false);
        assert_eq!(screen.address, "00.00");
        screen.dial(false);
        assert_eq!(
            screen.address, ":0.00",
            "down from 0 wraps to the last character"
        );
        screen.dial(true);
        assert_eq!(screen.address, "00.00");
        screen.delete_at();
        assert_eq!(screen.address, "0.00");
    }
}
