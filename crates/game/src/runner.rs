//! Drives any [`Minigame`] at the fixed tick rate: gathers each seat's input (devices or bot),
//! steps the simulation, and when it ends either restarts it (free play) or hands the result
//! back to the board. Online, the same `Run` goes through `ggrs` (`net::rollback`), and the
//! session says when every machine has the end, or when a machine dropped out and the
//! minigame stops until it's back.

use std::marker::PhantomData;
use std::net::SocketAddr;

use bevy::input::gamepad::{Gamepad, GamepadButton, GamepadConnection, GamepadConnectionEvent};
use bevy::prelude::*;
use net::rollback::{Rollback, RollbackEvent};
use net::session::Launch;
use sim_core::{
    Button, Difficulty, Format, Input, Inputs, MAX_PLAYERS, Minigame, Outcome, PlayerSetup, Run,
    RunConfig, Seat,
};

use crate::flow::{AppState, MinigameLaunch, MinigameResult, Origin};
use crate::intro::InstructionCard;
use crate::online::{NetStats, Online, OnlineEvent, RemoteReady};

/// How long a finished game stays on screen before A continues.
const RESTART_DELAY_TICKS: u32 = 90;
/// With no humans in the game, nobody presses A: carry on by itself after this long.
const AUTO_CONTINUE_TICKS: u32 = 240;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Controller {
    Human {
        gamepad: Pad,
        keyboard: bool,
    },
    Bot(Difficulty),
    /// Played on another machine online: a person there, or (`bot`) one of the host's bots.
    Remote {
        bot: bool,
    },
}

/// Which gamepad a human seat reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pad {
    None,
    /// The nth connected gamepad, in connection order (CLI launches, before anyone has joined).
    Nth(usize),
    /// The exact gamepad that joined this seat on the join screen.
    Bound(Entity),
}

/// The gamepad a seat reads, if it's connected.
pub fn find_pad<'a>(pad: Pad, gamepads: &'a Query<(Entity, &Gamepad)>) -> Option<&'a Gamepad> {
    match pad {
        Pad::None => None,
        Pad::Bound(entity) => gamepads.get(entity).ok().map(|(_, pad)| pad),
        Pad::Nth(n) => {
            let mut pads: Vec<_> = gamepads.iter().collect();
            pads.sort_by_key(|(entity, _)| *entity);
            pads.get(n).map(|(_, pad)| *pad)
        }
    }
}

/// Who controls each seat. `None` means the seat is not playing.
#[derive(Resource)]
pub struct Seats(pub [Option<Controller>; MAX_PLAYERS]);

impl Seats {
    pub fn is_bot(&self, seat: Seat) -> bool {
        matches!(
            self.0[seat.index()],
            Some(Controller::Bot(_) | Controller::Remote { bot: true })
        )
    }

    pub fn is_human(&self, seat: Seat) -> bool {
        matches!(self.0[seat.index()], Some(Controller::Human { .. }))
    }
}

/// The rules this session's minigames run by: from the command line, then from the lobby's
/// settings when a game starts (online, the host's lobby, so every machine agrees).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct RunRules(pub RunConfig);

#[derive(Debug, Clone, Copy)]
pub struct RunSetup {
    pub seed: u64,
    pub players: PlayerSetup,
    pub format: Format,
    pub config: RunConfig,
}

/// Exists only while this minigame is on screen.
#[derive(Resource)]
pub struct ActiveRun<M: Minigame> {
    pub run: Run<M>,
    /// The game state one tick before `run.game`. Views blend between the two by
    /// [`Time<Fixed>::overstep_fraction`] so motion is smooth at any frame rate.
    pub previous: M::State,
    setup: RunSetup,
    pub origin: Origin,
    /// Bumped on every restart so views can reset their presentation state.
    pub generation: u32,
    finished_ticks: u32,
    /// Seats whose Start was already down when this run began (e.g. the Start that launched
    /// it). Their Start is ignored until released, so the game doesn't open paused.
    start_held: [bool; MAX_PLAYERS],
    /// Whether the last fixed update stepped the run, so `previous` -> `run.game` is a tick
    /// nobody has reacted to yet. See [`ActiveRun::ticked`].
    stepped: bool,
    net: Net<M>,
    /// Online: the confirmed outcome, kept once the session is gone.
    outcome: Option<Outcome>,
    /// Online on a client: the host decides what comes after the result.
    follows_host: bool,
}

/// Where an online minigame's rollback session is.
enum Net<M: Minigame> {
    Couch,
    /// Online, until the instruction card closes.
    NotStarted,
    /// The session owns the real run; `run` is a copy of its newest state, for the views.
    Running(Box<Rollback<M, SocketAddr>>),
    /// Every machine has the confirmed end, so the session is gone.
    Settled,
    /// A machine dropped out before the end: frozen until everyone is back and it restarts.
    Stopped,
}

impl<M: Minigame> ActiveRun<M> {
    fn new(setup: RunSetup, origin: Origin) -> Self {
        let run = Run::<M>::new(setup.seed, setup.players, setup.format, setup.config);
        Self {
            previous: run.game.clone(),
            run,
            setup,
            origin,
            generation: 0,
            finished_ticks: 0,
            start_held: [true; MAX_PLAYERS],
            stepped: false,
            net: Net::Couch,
            outcome: None,
            follows_host: false,
        }
    }

    /// The launch that plays this run again from the start.
    fn launch(&self) -> Launch {
        Launch {
            id: M::ID.to_string(),
            seed: self.setup.seed,
            players: self.setup.players,
            format: self.setup.format,
        }
    }

    /// The state before and after the tick just stepped, if one was; for sounds and effects
    /// that must fire exactly once per event. Read it from [`MinigameSet::React`].
    pub fn ticked(&self) -> Option<(&M::State, &M::State)> {
        self.stepped.then_some((&self.previous, &self.run.game))
    }

    fn restart(&mut self) {
        self.setup.seed = self.setup.seed.wrapping_add(1);
        let generation = self.generation + 1;
        *self = Self::new(self.setup, self.origin);
        self.generation = generation;
    }

    /// Whether the result has been up long enough to move on, and (online) every machine has
    /// it, so leaving won't starve one still confirming the end.
    pub fn can_restart(&self) -> bool {
        self.finished_ticks >= RESTART_DELAY_TICKS && !matches!(self.net, Net::Running(_))
    }

    /// What pressing A does once the game is over, or nothing while it's too soon to press.
    pub fn continue_prompt(&self) -> String {
        match (self.can_restart(), self.origin) {
            (false, _) => String::new(),
            (true, Origin::FreePlay) if self.follows_host => crate::tr!("minigame-waiting-host"),
            (true, Origin::FreePlay) => crate::tr!("minigame-again"),
            (true, Origin::Board) => crate::tr!("minigame-continue"),
        }
    }
}

/// Button presses seen between fixed ticks, so taps shorter than a tick are never lost.
#[derive(Resource, Default)]
pub struct PressLatch(pub [u8; MAX_PLAYERS]);

/// Ordering for systems that run on entering [`AppState::Minigame`]: views spawn their scene
/// after [`MinigameSet::Start`] has created the [`ActiveRun`].
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum MinigameSet {
    Start,
    /// In `FixedUpdate`, right after the run steps: where views react to [`ActiveRun::ticked`].
    React,
}

/// Runs minigame `M` whenever a [`MinigameLaunch`] names it.
pub struct MinigamePlugin<M>(PhantomData<fn() -> M>);

impl<M> Default for MinigamePlugin<M> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

/// Input plumbing shared by every minigame (and the board).
pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PressLatch>()
            .init_resource::<SeatInputs>()
            .add_systems(PreUpdate, read_seat_inputs.after(bevy::input::InputSystems))
            .add_systems(Update, (log_gamepad_connections, latch_presses));
    }
}

/// Every human seat's input this frame and last frame, for turn-based screens (the board,
/// menus) that run per frame rather than per tick. Bot and empty seats read as neutral.
#[derive(Resource, Default)]
pub struct SeatInputs(pub Inputs);

impl SeatInputs {
    pub fn just_pressed(&self, seat: Seat, button: Button) -> bool {
        self.0.just_pressed(seat, button)
    }

    /// The seat's stick, or `None` while it's near the centre.
    pub fn stick(&self, seat: Seat) -> Option<Vec2> {
        let input = self.0.current[seat.index()];
        let v = Vec2::new(input.stick_x as f32, input.stick_y as f32) / 127.0;
        (v.length() > 0.5).then_some(v)
    }

    /// True on the frame the stick leaves the centre.
    pub fn stick_flicked(&self, seat: Seat) -> bool {
        let before = self.0.previous[seat.index()];
        let was_centred = (before.stick_x as f32).hypot(before.stick_y as f32) <= 0.5 * 127.0;
        was_centred && self.stick(seat).is_some()
    }
}

fn read_seat_inputs(
    seats: Res<Seats>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<(Entity, &Gamepad)>,
    mut inputs: ResMut<SeatInputs>,
) {
    let previous = inputs.0.current;
    let current = std::array::from_fn(|i| match seats.0[i] {
        Some(Controller::Human { gamepad, keyboard }) => {
            human_input(keyboard, find_pad(gamepad, &gamepads), &keys, 0)
        }
        _ => Input::default(),
    });
    inputs.0 = Inputs { current, previous };
}

impl<M: Minigame> Plugin for MinigamePlugin<M> {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Minigame),
            start_run::<M>.in_set(MinigameSet::Start),
        )
        .add_systems(OnExit(AppState::Minigame), stop_run::<M>)
        .add_systems(
            FixedUpdate,
            step_run::<M>
                .run_if(resource_exists::<ActiveRun<M>>)
                .before(MinigameSet::React),
        )
        .add_systems(
            Update,
            (
                crate::intro::show_countdown::<M>,
                follow_session::<M>.run_if(resource_exists::<Online>),
            )
                .run_if(resource_exists::<ActiveRun<M>>),
        );
    }
}

fn start_run<M: Minigame>(
    mut commands: Commands,
    launch: Option<Res<MinigameLaunch>>,
    mut latch: ResMut<PressLatch>,
    online: Option<Res<Online>>,
) {
    // Presses made on the menus or the board belong there, not to the new game.
    latch.0 = [0; MAX_PLAYERS];
    if let Some(launch) = launch.filter(|l| l.id == M::ID) {
        let mut active = ActiveRun::<M>::new(launch.setup, launch.origin);
        if let Some(online) = online.as_deref() {
            active.net = Net::NotStarted;
            active.follows_host = !online.is_host();
        }
        commands.insert_resource(active);
    }
}

fn stop_run<M: Minigame>(mut commands: Commands, mut net_stats: ResMut<NetStats>) {
    commands.remove_resource::<ActiveRun<M>>();
    net_stats.clear();
}

/// Online: ends the rollback session once every machine has the end; stops the minigame when
/// a machine drops out before it (the host decides, and tells the others); and once everyone
/// is back, the host plays it again from the start with the same seed.
#[allow(clippy::too_many_arguments)]
fn follow_session<M: Minigame>(
    mut events: MessageReader<OnlineEvent>,
    mut online: ResMut<Online>,
    mut active: ResMut<ActiveRun<M>>,
    mut remote_ready: ResMut<RemoteReady>,
    mut net_stats: ResMut<NetStats>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let stopped = matches!(active.net, Net::Stopped);
    for event in events.read() {
        match event {
            OnlineEvent::MinigameSettled => {
                if let Net::Running(_) = active.net {
                    info!("every machine has the end of {}", M::ID);
                    active.net = Net::Settled;
                }
            }
            OnlineEvent::MinigameStopped if !stopped => stop(&mut active, &mut net_stats),
            OnlineEvent::Away(away) => {
                let Online::Host(host) = online.as_mut() else {
                    continue;
                };
                if !away.is_empty() && active.outcome.is_none() && !stopped {
                    host.stop_minigame();
                    stop(&mut active, &mut net_stats);
                } else if away.is_empty() && stopped {
                    info!("everyone is back: {} starts again", M::ID);
                    host.restart_minigame(active.launch());
                    remote_ready.0 = [false; MAX_PLAYERS];
                    commands.insert_resource(MinigameLaunch {
                        id: M::ID.to_string(),
                        setup: active.setup,
                        origin: active.origin,
                    });
                    next_state.set(AppState::Minigame);
                }
            }
            _ => {}
        }
    }
}

fn stop<M: Minigame>(active: &mut ActiveRun<M>, net_stats: &mut NetStats) {
    warn!("{} stopped: a machine dropped out", M::ID);
    active.net = Net::Stopped;
    active.stepped = false;
    active.previous = active.run.game.clone();
    net_stats.clear();
}

const KEYBOARD_BUTTONS: [(KeyCode, Button); 6] = [
    (KeyCode::Space, Button::A),
    (KeyCode::KeyJ, Button::A),
    (KeyCode::KeyK, Button::B),
    (KeyCode::KeyL, Button::X),
    (KeyCode::Enter, Button::START),
    (KeyCode::Escape, Button::START),
];

const GAMEPAD_BUTTONS: [(GamepadButton, Button); 4] = [
    (GamepadButton::South, Button::A),
    (GamepadButton::East, Button::B),
    (GamepadButton::West, Button::X),
    (GamepadButton::Start, Button::START),
];

fn log_gamepad_connections(mut events: MessageReader<GamepadConnectionEvent>) {
    for event in events.read() {
        match &event.connection {
            GamepadConnection::Connected {
                name,
                vendor_id,
                product_id,
            } => info!(
                "gamepad connected: {name} ({vendor_id:04x?}:{product_id:04x?}) as {}",
                event.gamepad
            ),
            GamepadConnection::Disconnected => info!("gamepad disconnected: {}", event.gamepad),
        }
    }
}

fn latch_presses(
    seats: Res<Seats>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<(Entity, &Gamepad)>,
    mut latch: ResMut<PressLatch>,
) {
    for seat in Seat::ALL {
        let Some(Controller::Human { gamepad, keyboard }) = seats.0[seat.index()] else {
            continue;
        };
        let mut pressed = Input::default();
        if keyboard {
            for (key, button) in KEYBOARD_BUTTONS {
                if keys.just_pressed(key) {
                    pressed.set(button, true);
                }
            }
        }
        if let Some(pad) = find_pad(gamepad, &gamepads) {
            for (pad_button, button) in GAMEPAD_BUTTONS {
                if pad.just_pressed(pad_button) {
                    pressed.set(button, true);
                }
            }
        }
        latch.0[seat.index()] |= pressed.buttons;
    }
}

fn human_input(
    keyboard: bool,
    pad: Option<&Gamepad>,
    keys: &ButtonInput<KeyCode>,
    latched: u8,
) -> Input {
    let mut input = Input {
        buttons: latched,
        ..default()
    };
    let mut stick = Vec2::ZERO;
    if keyboard {
        for (key, button) in KEYBOARD_BUTTONS {
            if keys.pressed(key) {
                input.set(button, true);
            }
        }
        let axis = |neg: [KeyCode; 2], pos: [KeyCode; 2]| {
            (keys.any_pressed(pos) as i8 - keys.any_pressed(neg) as i8) as f32
        };
        stick.x = axis(
            [KeyCode::KeyA, KeyCode::ArrowLeft],
            [KeyCode::KeyD, KeyCode::ArrowRight],
        );
        stick.y = axis(
            [KeyCode::KeyS, KeyCode::ArrowDown],
            [KeyCode::KeyW, KeyCode::ArrowUp],
        );
    }
    match pad {
        Some(pad) => {
            for (pad_button, button) in GAMEPAD_BUTTONS {
                if pad.pressed(pad_button) {
                    input.set(button, true);
                }
            }
            if stick == Vec2::ZERO {
                stick = pad.left_stick();
            }
            if stick == Vec2::ZERO {
                stick = pad.dpad();
            }
        }
        // A keyboard seat always has a working device; a pad-only seat without its pad is lost.
        None if !keyboard => input.set(Button::CONTROLLER_LOST, true),
        None => {}
    }
    input.stick_x = Input::quantize_axis(stick.x);
    input.stick_y = Input::quantize_axis(stick.y);
    input
}

#[allow(clippy::too_many_arguments)]
fn step_run<M: Minigame>(
    mut commands: Commands,
    mut active: ResMut<ActiveRun<M>>,
    seats: Res<Seats>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<(Entity, &Gamepad)>,
    mut latch: ResMut<PressLatch>,
    mut next_state: ResMut<NextState<AppState>>,
    card: Option<Res<InstructionCard>>,
    mut online: Option<ResMut<Online>>,
    mut net_stats: ResMut<NetStats>,
) {
    let latched = std::mem::take(&mut latch.0);
    active.stepped = false;
    // Nothing happens until everyone has read the instruction card, nor while stopped.
    if card.is_some() || matches!(active.net, Net::Stopped) {
        return;
    }
    // Online, the run goes through rollback from here on.
    if let (Some(online), Net::NotStarted) = (online.as_deref(), &active.net) {
        match online.seat_owners() {
            Some(owners) => match Rollback::start(active.run.clone(), owners, online.endpoint()) {
                Ok(rollback) => active.net = Net::Running(Box::new(rollback)),
                Err(err) => error!("couldn't start the online minigame: {err}"),
            },
            None => error!("couldn't start the online minigame: a machine's address is missing"),
        }
    }

    // This machine's seats: its players, and (on the host or the couch) bots and empty seats.
    let mut start_held = active.start_held;
    let mut input_for = |seat: Seat, run: &Run<M>| {
        let i = seat.index();
        let mut input = match seats.0[i] {
            None | Some(Controller::Remote { .. }) => Input::default(),
            Some(Controller::Bot(difficulty)) => M::bot_input(&run.game, seat, difficulty),
            Some(Controller::Human { gamepad, keyboard }) => {
                human_input(keyboard, find_pad(gamepad, &gamepads), &keys, latched[i])
            }
        };
        if start_held[i] && input.held(Button::START) {
            input.set(Button::START, false);
        } else {
            start_held[i] = false;
        }
        input
    };

    let first_time = active.finished_ticks == 0;
    let outcome = if let Net::Running(rollback) = &mut active.net {
        // Keep exchanging inputs even after the end, so slower machines can confirm it too.
        let advance = rollback.update(&mut input_for);
        if !advance.new_tick && rollback.confirmed_outcome().is_none() {
            // No tick took this machine's input (still synchronizing, or waiting for slower
            // machines): keep the taps for the next one, or a quick Start would never pause.
            for (kept, taps) in latch.0.iter_mut().zip(latched) {
                *kept |= taps;
            }
        }
        net_stats.record(advance.resimulated, rollback.pings());
        for event in rollback.events() {
            match event {
                RollbackEvent::Desync { tick, peer } => {
                    error!("minigame out of step with {peer} at tick {tick}")
                }
                RollbackEvent::Synchronized(_) => {}
                other => warn!("online minigame: {other:?}"),
            }
        }
        let outcome = rollback.confirmed_outcome().map(|(tick, outcome)| {
            if first_time {
                // Every machine logs this line; they must agree.
                info!(
                    "{} over online at tick {tick}: {:?}",
                    M::ID,
                    outcome.placement
                );
                if let Some(online) = online.as_deref_mut() {
                    online.finish_minigame();
                }
            }
            outcome
        });
        // What was on screen becomes `previous`, and the newest prediction is shown. After a
        // rollback the views' before/after diff covers the correction, so nothing fires twice.
        let shown = (advance.new_tick || advance.resimulated > 0).then(|| rollback.run().clone());
        match shown {
            Some(shown) => {
                active.previous = std::mem::replace(&mut active.run, shown).game;
                active.stepped = true;
            }
            None => active.previous = active.run.game.clone(),
        }
        active.outcome = active.outcome.or(outcome);
        outcome
    } else if let Net::Settled = active.net {
        active.outcome
    } else {
        active.run.outcome()
    };

    let online = online.as_deref();
    if let Some(outcome) = outcome {
        if !matches!(active.net, Net::Running(_)) {
            active.previous = active.run.game.clone();
        }
        active.start_held = start_held;
        active.finished_ticks += 1;
        let human_pressed = |button: Button| {
            Seat::ALL.into_iter().any(|seat| {
                seats.is_human(seat)
                    && latched[seat.index()] & Input::default().with(button).buttons != 0
            })
        };
        let (pressed_a, pressed_b) = (human_pressed(Button::A), human_pressed(Button::B));
        // Online free play follows the host: only its players choose to replay or pick again.
        let host = online.is_none_or(Online::is_host);
        if active.origin == Origin::FreePlay && active.can_restart() && pressed_b && host {
            if let Some(Online::Host(session)) = online {
                session.back_to_picker();
            }
            next_state.set(AppState::Pick);
            return;
        }
        let no_humans = Seat::ALL.into_iter().all(|seat| !seats.is_human(seat));
        let carry_on = active.can_restart()
            && (pressed_a || (no_humans && active.finished_ticks >= AUTO_CONTINUE_TICKS));
        if carry_on {
            match (active.origin, online) {
                (Origin::FreePlay, None) => active.restart(),
                (Origin::FreePlay, Some(Online::Host(session))) => {
                    let mut setup = active.setup;
                    setup.seed = setup.seed.wrapping_add(1);
                    session.launch(Launch {
                        id: M::ID.to_string(),
                        seed: setup.seed,
                        players: setup.players,
                        format: setup.format,
                    });
                    commands.insert_resource(MinigameLaunch {
                        id: M::ID.to_string(),
                        setup,
                        origin: Origin::FreePlay,
                    });
                    next_state.set(AppState::Minigame);
                }
                (Origin::FreePlay, Some(Online::Client(_))) => {}
                // Every machine goes back; only the host's board applies the outcome.
                (Origin::Board, _) => {
                    commands.insert_resource(MinigameResult(outcome));
                    next_state.set(AppState::Board);
                }
            }
        }
        return;
    }
    if !matches!(active.net, Net::Couch) {
        active.start_held = start_held;
        return;
    }

    let inputs = Seat::ALL.map(|seat| input_for(seat, &active.run));
    active.start_held = start_held;
    active.previous = active.run.game.clone();
    active.run.step(inputs);
    active.stepped = true;
}
