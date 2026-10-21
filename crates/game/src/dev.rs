//! Development helpers.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use sim_core::{Button, Input, MAX_PLAYERS, Seat};

use crate::runner::{Controller, Seats, find_pad, human_input};

/// Saves screenshots at fixed times after startup, then exits. Lets visuals be checked
/// without a human at the keyboard.
pub struct ScreenshotPlugin {
    pub at_seconds: Vec<f32>,
    pub dir: PathBuf,
}

#[derive(Resource)]
struct ScreenshotSchedule {
    pending: Vec<f32>,
    dir: PathBuf,
    taken: usize,
    exit_at: f32,
}

impl Plugin for ScreenshotPlugin {
    fn build(&self, app: &mut App) {
        let mut pending = self.at_seconds.clone();
        pending.sort_by(f32::total_cmp);
        let exit_at = pending.last().copied().unwrap_or(0.0) + 1.0;
        app.insert_resource(ScreenshotSchedule {
            pending,
            dir: self.dir.clone(),
            taken: 0,
            exit_at,
        })
        .add_systems(Update, take_screenshots);
    }
}

fn take_screenshots(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut schedule: ResMut<ScreenshotSchedule>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = time.elapsed_secs();
    while schedule.pending.first().is_some_and(|at| *at <= now) {
        schedule.pending.remove(0);
        schedule.taken += 1;
        let _ = std::fs::create_dir_all(&schedule.dir);
        let path = schedule.dir.join(format!("shot-{}.png", schedule.taken));
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if schedule.pending.is_empty() && now >= schedule.exit_at {
        exit.write(AppExit::Success);
    }
}

/// Logs every gamepad button press, and each person's input as minigames read it whenever it
/// changes, to diagnose controller mappings.
pub struct InputLogPlugin;

impl Plugin for InputLogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (log_gamepad_presses, log_seat_inputs));
    }
}

fn log_seat_inputs(
    seats: Option<Res<Seats>>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<(Entity, &Gamepad)>,
    mut last: Local<[Input; MAX_PLAYERS]>,
) {
    let Some(seats) = seats else {
        return;
    };
    for seat in Seat::ALL {
        let Some(Controller::Human { gamepad, keyboard }) = seats.0[seat.index()] else {
            continue;
        };
        let input = human_input(keyboard, find_pad(gamepad, &gamepads), &keys, 0);
        if input != last[seat.index()] {
            info!(
                "{seat:?} input: stick ({}, {}), A {}, B {}, left stick {:?}, d-pad {:?}",
                input.stick_x,
                input.stick_y,
                input.held(Button::A),
                input.held(Button::B),
                find_pad(gamepad, &gamepads).map(|pad| pad.left_stick()),
                find_pad(gamepad, &gamepads).map(|pad| pad.dpad()),
            );
            last[seat.index()] = input;
        }
    }
}

fn log_gamepad_presses(gamepads: Query<(Entity, &Gamepad)>) {
    for (entity, gamepad) in &gamepads {
        for button in gamepad.get_just_pressed() {
            info!("gamepad {entity}: pressed {button:?}");
        }
    }
}

/// Presses keyboard keys on a schedule, so a headless run can drive menus and turns the way a
/// player would (`--press 3:Space`, `--press-every 2:Space`).
pub struct ScriptedKeysPlugin {
    pub once: Vec<(f32, KeyCode)>,
    pub every: Vec<(f32, KeyCode)>,
}

#[derive(Resource)]
struct ScriptedKeys {
    once: Vec<(f32, KeyCode)>,
    every: Vec<(f32, KeyCode, f32)>,
    release: Vec<KeyCode>,
}

impl Plugin for ScriptedKeysPlugin {
    fn build(&self, app: &mut App) {
        let mut once = self.once.clone();
        once.sort_by(|a, b| a.0.total_cmp(&b.0));
        app.insert_resource(ScriptedKeys {
            once,
            every: self
                .every
                .iter()
                .map(|(secs, key)| (*secs, *key, *secs))
                .collect(),
            release: Vec::new(),
        })
        // Inside the input set, so every system that reads input after it sees the press.
        .add_systems(
            PreUpdate,
            press_scripted_keys
                .in_set(bevy::input::InputSystems)
                .after(bevy::input::keyboard::keyboard_input_system),
        );
    }
}

/// Parses `SECS:KEY`, e.g. `2.5:Space`. Keys: Space, Enter, Escape, Arrow{Up,Down,Left,Right},
/// or one of the letters J, K, L, I.
pub fn parse_key_press(text: &str) -> Result<(f32, KeyCode), String> {
    let (secs, key) = text
        .split_once(':')
        .ok_or_else(|| format!("expected SECS:KEY, got {text:?}"))?;
    let secs: f32 = secs
        .parse()
        .map_err(|_| format!("bad seconds in {text:?}"))?;
    let key = match key {
        "Space" => KeyCode::Space,
        "Enter" => KeyCode::Enter,
        "Escape" => KeyCode::Escape,
        "ArrowUp" => KeyCode::ArrowUp,
        "ArrowDown" => KeyCode::ArrowDown,
        "ArrowLeft" => KeyCode::ArrowLeft,
        "ArrowRight" => KeyCode::ArrowRight,
        "J" => KeyCode::KeyJ,
        "K" => KeyCode::KeyK,
        "L" => KeyCode::KeyL,
        "I" => KeyCode::KeyI,
        other => return Err(format!("unknown key {other:?}")),
    };
    Ok((secs, key))
}

fn press_scripted_keys(
    time: Res<Time<Real>>,
    mut script: ResMut<ScriptedKeys>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
) {
    let now = time.elapsed_secs();
    // Hold each key for one frame, like a quick tap.
    for key in std::mem::take(&mut script.release) {
        keys.release(key);
    }
    let mut pressed = Vec::new();
    while script.once.first().is_some_and(|(at, _)| *at <= now) {
        pressed.push(script.once.remove(0).1);
    }
    for (period, key, next) in &mut script.every {
        if now >= *next {
            *next += *period;
            pressed.push(*key);
        }
    }
    for key in pressed {
        info!("scripted press: {key:?}");
        keys.press(key);
        script.release.push(key);
    }
}
