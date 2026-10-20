use serde::{Deserialize, Serialize};

use crate::{MAX_PLAYERS, Seat};

/// Bit flags for [`Input::buttons`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Button(u8);

impl Button {
    pub const A: Self = Self(1 << 0);
    pub const B: Self = Self(1 << 1);
    pub const X: Self = Self(1 << 2);
    pub const START: Self = Self(1 << 3);
    /// Not a real button: set by the sending machine while this seat's controller is unplugged.
    pub const CONTROLLER_LOST: Self = Self(1 << 7);
}

/// One seat's input for one tick. This is the only game data sent over the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Input {
    pub buttons: u8,
    /// Stick axes quantized to -127..=127, so the simulation never sees raw device floats.
    pub stick_x: i8,
    pub stick_y: i8,
}

impl Input {
    pub fn held(&self, button: Button) -> bool {
        self.buttons & button.0 != 0
    }

    pub fn set(&mut self, button: Button, held: bool) {
        if held {
            self.buttons |= button.0;
        } else {
            self.buttons &= !button.0;
        }
    }

    pub fn with(mut self, button: Button) -> Self {
        self.set(button, true);
        self
    }

    /// Quantize a device stick value in -1.0..=1.0.
    pub fn quantize_axis(value: f32) -> i8 {
        (value.clamp(-1.0, 1.0) * 127.0).round() as i8
    }
}

/// The inputs a minigame sees for one tick: this tick's held state plus last tick's,
/// so "just pressed" is derived deterministically inside the simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Inputs {
    pub current: [Input; MAX_PLAYERS],
    pub previous: [Input; MAX_PLAYERS],
}

impl Inputs {
    pub fn held(&self, seat: Seat, button: Button) -> bool {
        self.current[seat.index()].held(button)
    }

    pub fn just_pressed(&self, seat: Seat, button: Button) -> bool {
        self.current[seat.index()].held(button) && !self.previous[seat.index()].held(button)
    }
}
