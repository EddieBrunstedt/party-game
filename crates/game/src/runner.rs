//! Drives any [`Minigame`] at the fixed tick rate: gathers each seat's input (devices or bot),
//! steps the simulation, and when it ends either restarts it (free play) or hands the result
//! back to the board. Couch play only for now; online play will feed the same `Run` through `ggrs`.

use std::marker::PhantomData;

use bevy::input::gamepad::{Gamepad, GamepadButton, GamepadConnection, GamepadConnectionEvent};
use bevy::prelude::*;
use sim_core::{
    Button, Difficulty, Format, Input, Inputs, MAX_PLAYERS, Minigame, PlayerSetup, Run, RunConfig,
    Seat,
};

use crate::flow::{AppState, MinigameLaunch, MinigameResult, Origin};
use crate::intro::InstructionCard;

/// How long a finished game stays on screen before A continues.
const RESTART_DELAY_TICKS: u32 = 90;
/// With no humans in the game, nobody presses A: carry on by itself after this long.
const AUTO_CONTINUE_TICKS: u32 = 240;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Controller {
    Human { gamepad: Pad, keyboard: bool },
    Bot(Difficulty),
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
        matches!(self.0[seat.index()], Some(Controller::Bot(_)))
    }

    pub fn is_human(&self, seat: Seat) -> bool {
        matches!(self.0[seat.index()], Some(Controller::Human { .. }))
    }
}

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
        }
    }

    fn restart(&mut self) {
        self.setup.seed = self.setup.seed.wrapping_add(1);
        let generation = self.generation + 1;
        *self = Self::new(self.setup, self.origin);
        self.generation = generation;
    }

    pub fn can_restart(&self) -> bool {
        self.finished_ticks >= RESTART_DELAY_TICKS
    }

    /// What pressing A does once the game is over, or nothing while it's too soon to press.
    pub fn continue_prompt(&self) -> String {
        match (self.can_restart(), self.origin) {
            (false, _) => String::new(),
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
            step_run::<M>.run_if(resource_exists::<ActiveRun<M>>),
        )
        .add_systems(
            Update,
            crate::intro::show_countdown::<M>.run_if(resource_exists::<ActiveRun<M>>),
        );
    }
}

fn start_run<M: Minigame>(
    mut commands: Commands,
    launch: Option<Res<MinigameLaunch>>,
    mut latch: ResMut<PressLatch>,
) {
    // Presses made on the menus or the board belong there, not to the new game.
    latch.0 = [0; MAX_PLAYERS];
    if let Some(launch) = launch.filter(|l| l.id == M::ID) {
        commands.insert_resource(ActiveRun::<M>::new(launch.setup, launch.origin));
    }
}

fn stop_run<M: Minigame>(mut commands: Commands) {
    commands.remove_resource::<ActiveRun<M>>();
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
        }
        // A keyboard seat always has a working device; a pad-only seat without its pad is lost.
        None if !keyboard => input.set(Button::CONTROLLER_LOST, true),
        None => {}
    }
    input.stick_x = Input::quantize_axis(stick.x);
    input.stick_y = Input::quantize_axis(stick.y);
    input
}

fn step_run<M: Minigame>(
    mut commands: Commands,
    mut active: ResMut<ActiveRun<M>>,
    seats: Res<Seats>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<(Entity, &Gamepad)>,
    mut latch: ResMut<PressLatch>,
    mut next_state: ResMut<NextState<AppState>>,
    card: Option<Res<InstructionCard>>,
) {
    let latched = std::mem::take(&mut latch.0);
    // Nothing happens until everyone has read the instruction card.
    if card.is_some() {
        return;
    }

    if let Some(outcome) = active.run.outcome() {
        active.previous = active.run.game.clone();
        active.finished_ticks += 1;
        let human_pressed_a = Seat::ALL.into_iter().any(|seat| {
            seats.is_human(seat)
                && latched[seat.index()] & Input::default().with(Button::A).buttons != 0
        });
        let human_pressed_b = Seat::ALL.into_iter().any(|seat| {
            seats.is_human(seat)
                && latched[seat.index()] & Input::default().with(Button::B).buttons != 0
        });
        if active.origin == Origin::FreePlay && active.can_restart() && human_pressed_b {
            next_state.set(AppState::Pick);
            return;
        }
        let no_humans = Seat::ALL.into_iter().all(|seat| !seats.is_human(seat));
        let carry_on = (active.can_restart() && human_pressed_a)
            || (no_humans && active.finished_ticks >= AUTO_CONTINUE_TICKS);
        if carry_on {
            match active.origin {
                Origin::FreePlay => active.restart(),
                Origin::Board => {
                    commands.insert_resource(MinigameResult(outcome));
                    next_state.set(AppState::Board);
                }
            }
        }
        return;
    }

    let mut inputs: [Input; MAX_PLAYERS] = std::array::from_fn(|i| match seats.0[i] {
        None => Input::default(),
        Some(Controller::Bot(difficulty)) => {
            M::bot_input(&active.run.game, Seat(i as u8), difficulty)
        }
        Some(Controller::Human { gamepad, keyboard }) => {
            human_input(keyboard, find_pad(gamepad, &gamepads), &keys, latched[i])
        }
    });
    for (input, held) in inputs.iter_mut().zip(active.start_held.iter_mut()) {
        if *held && input.held(Button::START) {
            input.set(Button::START, false);
        } else {
            *held = false;
        }
    }
    active.previous = active.run.game.clone();
    active.run.step(inputs);
}
