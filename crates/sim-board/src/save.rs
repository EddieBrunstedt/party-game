//! Saved board games: a [`Game`] between turns plus who sat where, as versioned bytes. Couch
//! games and online hosts save; "Continue" loads one and the players take their seats again.
//!
//! The bytes are a magic, [`SAVE_VERSION`] and the bincode encoding of [`Save`]. bincode isn't
//! self-describing, so any change to the saved types must bump the version (a test pins the
//! encoding to catch a forgotten bump). Loading refuses other versions and damaged files with
//! a [`LoadError`] instead of panicking.

use bincode::Options;
use serde::{Deserialize, Serialize};
use sim_core::{Difficulty, MAX_PLAYERS};

use crate::game::Game;

/// Bump on any change to [`Save`] or anything inside it, including [`Game`] and the board.
pub const SAVE_VERSION: u32 = 1;
const MAGIC: [u8; 4] = *b"BSAV";
/// A save is a few kilobytes; never read more than this.
const MAX_SIZE: u64 = 1 << 20;

/// Who sat in a seat when the game was saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SavedSeat {
    /// An index into the game's character list.
    pub character: u8,
    /// A person played it (bots' seats are offered to players last).
    pub human: bool,
}

/// One saved board game.
#[derive(Debug, Clone, PartialEq, Hash, Serialize, Deserialize)]
pub struct Save {
    /// Always [`Game::between_turns`].
    pub game: Game,
    pub seats: [SavedSeat; MAX_PLAYERS],
    pub bots: Difficulty,
    pub pause_on_controller_lost: bool,
}

/// Why a save can't be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// The file isn't a save at all.
    NotASave,
    /// Saved by a version of the game that writes saves differently.
    Version { found: u32 },
    /// The right version, but the contents don't make sense.
    Damaged(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::NotASave => f.write_str("not a saved game"),
            LoadError::Version { found } => {
                write!(
                    f,
                    "save format {found}, but this build reads {SAVE_VERSION}"
                )
            }
            LoadError::Damaged(why) => write!(f, "damaged save: {why}"),
        }
    }
}

impl std::error::Error for LoadError {}

fn options() -> impl Options {
    bincode::DefaultOptions::new().with_limit(MAX_SIZE)
}

impl Save {
    pub fn encode(&self) -> Vec<u8> {
        assert!(self.game.between_turns(), "games are saved between turns");
        let mut bytes = MAGIC.to_vec();
        bytes.extend(SAVE_VERSION.to_le_bytes());
        options()
            .serialize_into(&mut bytes, self)
            .expect("saves always encode");
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, LoadError> {
        let (magic, rest) = bytes.split_at_checked(4).ok_or(LoadError::NotASave)?;
        if magic != MAGIC {
            return Err(LoadError::NotASave);
        }
        let (version, body) = rest.split_at_checked(4).ok_or(LoadError::NotASave)?;
        let found = u32::from_le_bytes(version.try_into().expect("four bytes"));
        if found != SAVE_VERSION {
            return Err(LoadError::Version { found });
        }
        let save: Save = options()
            .allow_trailing_bytes()
            .deserialize(body)
            .map_err(|e| LoadError::Damaged(e.to_string()))?;
        save.game.check().map_err(LoadError::Damaged)?;
        if !save.game.between_turns() {
            return Err(LoadError::Damaged("saved mid-turn".into()));
        }
        let characters = save.seats.map(|s| s.character);
        let unique = (1..MAX_PLAYERS).all(|i| !characters[..i].contains(&characters[i]));
        if !unique {
            return Err(LoadError::Damaged("two seats share a character".into()));
        }
        Ok(save)
    }
}

#[cfg(test)]
mod tests {
    use sim_core::checksum;

    use super::*;
    use crate::bot::bot_intent;
    use crate::game::{Awaiting, Rules};
    use crate::headless::play_bots;
    use crate::testing::{self, BOTH};

    /// A game a few turns in, stopped between turns.
    fn save() -> Save {
        let mut game = Game::new(testing::board(), Rules::default(), 4, 11, BOTH);
        let mut turns = 0;
        while turns < 6 || !game.between_turns() {
            match game.awaiting() {
                Awaiting::Intent { seat, .. } => {
                    let intent = bot_intent(&game, seat, Difficulty::Hard).unwrap();
                    game.apply(seat, intent).unwrap();
                    turns += game.between_turns() as u32;
                }
                Awaiting::Minigame(plan) => {
                    let mut placement = [None; MAX_PLAYERS];
                    for seat in plan.players.seats() {
                        placement[seat.index()] = Some(seat.0 + 1);
                    }
                    game.apply_minigame_outcome(sim_core::Outcome { placement })
                        .unwrap();
                }
                Awaiting::GameOver => unreachable!("a 12-round game lasts longer"),
            }
        }
        Save {
            game,
            seats: [3, 0, 2, 1].map(|character| SavedSeat {
                character,
                human: character % 2 == 0,
            }),
            bots: Difficulty::Hard,
            pause_on_controller_lost: true,
        }
    }

    #[test]
    fn a_loaded_game_is_the_saved_game() {
        let save = save();
        assert!(save.game.round() >= 1);
        let loaded = Save::decode(&save.encode()).unwrap();
        assert_eq!(checksum(&loaded.game), checksum(&save.game));
        assert_eq!(loaded, save);
        let finish = |game| play_bots(game, Difficulty::Normal, BOTH).unwrap().checksum;
        assert_eq!(
            finish(loaded.game),
            finish(save.game),
            "and plays on the same"
        );
    }

    #[test]
    fn the_encoding_is_pinned() {
        // Fails when a saved type changes. Bump SAVE_VERSION, then update the number.
        assert_eq!(
            checksum(&save().encode()),
            5590078080190144120,
            "the save format changed: bump SAVE_VERSION and re-pin"
        );
    }

    #[test]
    fn other_versions_and_other_files_are_refused() {
        let mut bytes = save().encode();
        bytes[4..8].copy_from_slice(&(SAVE_VERSION + 1).to_le_bytes());
        assert_eq!(
            Save::decode(&bytes),
            Err(LoadError::Version {
                found: SAVE_VERSION + 1
            })
        );
        assert_eq!(Save::decode(b"(name: \"x\")"), Err(LoadError::NotASave));
        assert_eq!(Save::decode(b""), Err(LoadError::NotASave));
        assert_eq!(Save::decode(b"BSAV"), Err(LoadError::NotASave));
    }

    #[test]
    fn damaged_saves_are_refused_not_panicked_on() {
        let bytes = save().encode();
        for cut in [9, bytes.len() / 2, bytes.len() - 1] {
            assert!(matches!(
                Save::decode(&bytes[..cut]),
                Err(LoadError::Damaged(_))
            ));
        }
        // Flip every byte in turn: whatever decodes must still be a game that makes sense.
        for at in 8..bytes.len() {
            let mut bad = bytes.clone();
            bad[at] ^= 0x5a;
            if let Ok(save) = Save::decode(&bad) {
                save.game.check().unwrap();
                let _ = save.game.awaiting();
            }
        }
    }
}
