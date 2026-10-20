//! The playable lantern folk. One shared body; each character's personality is its light
//! source (DESIGN.md §3.5). Placeholders until the Blender models land.

use bevy::prelude::*;
use sim_core::{MAX_PLAYERS, Seat};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Character {
    PaperLantern,
    FireflyJar,
    Candle,
    NeonRing,
}

impl Character {
    pub const ALL: [Character; 4] = [
        Character::PaperLantern,
        Character::FireflyJar,
        Character::Candle,
        Character::NeonRing,
    ];

    /// Fluent key of the character's name.
    pub fn name_key(self) -> &'static str {
        match self {
            Character::PaperLantern => "character-paper-lantern",
            Character::FireflyJar => "character-firefly-jar",
            Character::Candle => "character-candle",
            Character::NeonRing => "character-neon-ring",
        }
    }

    pub fn name(self) -> String {
        crate::tr!(self.name_key())
    }
}

/// Who plays which character, by seat. Bots get whoever is left.
#[derive(Resource, Debug, Clone, Copy)]
pub struct Roster(pub [Character; MAX_PLAYERS]);

impl Default for Roster {
    fn default() -> Self {
        Self(Character::ALL)
    }
}

impl Roster {
    pub fn of(&self, seat: Seat) -> Character {
        self.0[seat.index()]
    }
}
