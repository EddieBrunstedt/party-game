//! Drives any [`Minigame`] at the fixed tick rate: gathers each seat's input (devices or bot),
//! steps the simulation, and restarts it when a finished game is acknowledged.
//! Couch play only for now; online play will feed the same `Run` through `ggrs`.

use std::marker::PhantomData;

use bevy::input::gamepad::{Gamepad, GamepadButton, GamepadConnection, GamepadConnectionEvent};
use bevy::prelude::*;
use sim_core::{
    Button, Difficulty, Format, Input, MAX_PLAYERS, Minigame, PlayerSetup, Run, RunConfig, Seat,
};

/// How long a finished game stays on screen before A restarts it.
const RESTART_DELAY_TICKS: u32 = 90;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Controller {
    /// `gamepad` is the index into connected gamepads, in connection order.
    Human {
        gamepad: usize,
        keyboard: bool,
    },
    Bot(Difficulty),
}

/// Who controls each seat. `None` means the seat is not playing.
#[derive(Resource)]
pub struct Seats(pub [Option<Controller>; MAX_PLAYERS]);

impl Seats {
    pub fn is_bot(&self, seat: Seat) -> bool {
        matches!(self.0[seat.index()], Some(Controller::Bot(_)))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RunSetup {
    pub seed: u64,
    pub players: PlayerSetup,
    pub format: Format,
    pub config: RunConfig,
}

#[derive(Resource)]
pub struct ActiveRun<M: Minigame> {
    pub run: Run<M>,
    /// The game state one tick before `run.game`. Views blend between the two by
    /// [`Time<Fixed>::overstep_fraction`] so motion is smooth at any frame rate.
    pub previous: M::State,
    setup: RunSetup,
    /// Bumped on every restart so views can reset their presentation state.
    pub generation: u32,
    finished_ticks: u32,
}

impl<M: Minigame> ActiveRun<M> {
    fn new(setup: RunSetup) -> Self {
        let run = Run::<M>::new(setup.seed, setup.players, setup.format, setup.config);
        Self {
            previous: run.game.clone(),
            run,
            setup,
            generation: 0,
            finished_ticks: 0,
        }
    }

    fn restart(&mut self) {
        self.setup.seed = self.setup.seed.wrapping_add(1);
        let generation = self.generation + 1;
        *self = Self::new(self.setup);
        self.generation = generation;
    }

    pub fn can_restart(&self) -> bool {
        self.finished_ticks >= RESTART_DELAY_TICKS
    }
}

/// Button presses seen between fixed ticks, so taps shorter than a tick are never lost.
#[derive(Resource, Default)]
struct PressLatch([u8; MAX_PLAYERS]);

pub struct MinigamePlugin<M> {
    setup: RunSetup,
    _minigame: PhantomData<fn() -> M>,
}

impl<M> MinigamePlugin<M> {
    pub fn new(setup: RunSetup) -> Self {
        Self {
            setup,
            _minigame: PhantomData,
        }
    }
}

impl<M: Minigame> Plugin for MinigamePlugin<M> {
    fn build(&self, app: &mut App) {
        app.insert_resource(ActiveRun::<M>::new(self.setup))
            .init_resource::<PressLatch>()
            .add_systems(Update, (log_gamepad_connections, latch_presses))
            .add_systems(FixedUpdate, step_run::<M>);
    }
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

fn connected_gamepads<'a>(gamepads: &'a Query<(Entity, &Gamepad)>) -> Vec<&'a Gamepad> {
    let mut pads: Vec<_> = gamepads.iter().collect();
    pads.sort_by_key(|(entity, _)| *entity);
    pads.into_iter().map(|(_, pad)| pad).collect()
}

fn latch_presses(
    seats: Res<Seats>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<(Entity, &Gamepad)>,
    mut latch: ResMut<PressLatch>,
) {
    let pads = connected_gamepads(&gamepads);
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
        if let Some(pad) = pads.get(gamepad) {
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
    mut active: ResMut<ActiveRun<M>>,
    seats: Res<Seats>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<(Entity, &Gamepad)>,
    mut latch: ResMut<PressLatch>,
) {
    let pads = connected_gamepads(&gamepads);
    let latched = std::mem::take(&mut latch.0);

    if active.run.outcome().is_some() {
        active.previous = active.run.game.clone();
        active.finished_ticks += 1;
        let human_pressed_a = Seat::ALL.into_iter().any(|seat| {
            !seats.is_bot(seat)
                && latched[seat.index()] & Input::default().with(Button::A).buttons != 0
        });
        if active.can_restart() && human_pressed_a {
            active.restart();
        }
        return;
    }

    let inputs: [Input; MAX_PLAYERS] = std::array::from_fn(|i| match seats.0[i] {
        None => Input::default(),
        Some(Controller::Bot(difficulty)) => {
            M::bot_input(&active.run.game, Seat(i as u8), difficulty)
        }
        Some(Controller::Human { gamepad, keyboard }) => {
            human_input(keyboard, pads.get(gamepad).copied(), &keys, latched[i])
        }
    });
    active.previous = active.run.game.clone();
    active.run.step(inputs);
}
