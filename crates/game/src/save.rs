//! Saving board games and picking them up again (`sim_board::save`). The couch and an online
//! host save whenever their game is between turns, a finished game removes its save, and the
//! title's "Continue" (or "Host the saved game") loads it for the players to sit down again.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use sim_board::{Awaiting, LoadError, Save, SavedSeat};
use sim_core::{Difficulty, Seat, checksum};

use crate::board_view::BoardSession;
use crate::characters::{Character, Roster};
use crate::online::{Notice, Online};
use crate::runner::{Controller, RunRules, Seats};
use crate::{minigames, tr};

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, autosave.run_if(resource_exists::<BoardSession>));
    }
}

/// The folder under the user's data directory that holds the save.
const FOLDER: &str = "party-game";
const FILE: &str = "board.save";

/// Where this machine keeps its saved game. `None` doesn't save at all.
#[derive(Resource, Debug, Clone)]
pub struct SaveSlot(pub Option<PathBuf>);

impl SaveSlot {
    /// The user's data folder: `$XDG_DATA_HOME` or `~/.local/share` on Linux (and the Deck),
    /// `%APPDATA%` on Windows, `~/Library/Application Support` on macOS.
    pub fn default_path() -> Option<PathBuf> {
        let env = |name| {
            std::env::var_os(name)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let base = if cfg!(windows) {
            env("APPDATA")?
        } else if cfg!(target_os = "macos") {
            env("HOME")?.join("Library/Application Support")
        } else {
            env("XDG_DATA_HOME").or_else(|| Some(env("HOME")?.join(".local/share")))?
        };
        Some(base.join(FOLDER).join(FILE))
    }

    pub fn exists(&self) -> bool {
        self.0.as_deref().is_some_and(Path::is_file)
    }

    /// Reads the save, or says on the title why it can't be continued.
    pub fn load(&self) -> Result<Save, Notice> {
        let path = self
            .0
            .as_deref()
            .ok_or_else(|| Notice(tr!("notice-no-save")))?;
        let bytes = std::fs::read(path).map_err(|e| {
            warn!("couldn't read {}: {e}", path.display());
            Notice(tr!("notice-save-unreadable", error = e.to_string()))
        })?;
        let save = Save::decode(&bytes).and_then(check_contents).map_err(|e| {
            warn!("refused the save at {}: {e}", path.display());
            Notice(match e {
                LoadError::Version { .. } => tr!("notice-save-version"),
                LoadError::NotASave | LoadError::Damaged(_) => tr!("notice-save-damaged"),
            })
        })?;
        info!(
            "loaded a saved game in round {}, checksum {:016x}",
            save.game.round(),
            checksum(&save.game)
        );
        Ok(save)
    }

    fn write(&self, save: &Save) -> std::io::Result<()> {
        let Some(path) = &self.0 else {
            return Ok(());
        };
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        // Write beside it, then swap it in, so a crash mid-write can't leave half a save.
        let fresh = path.with_extension("save.new");
        std::fs::write(&fresh, save.encode())?;
        std::fs::rename(&fresh, path)
    }

    fn remove(&self) {
        if let Some(path) = self.0.as_deref().filter(|p| p.is_file()) {
            match std::fs::remove_file(path) {
                Ok(()) => info!("the game is over; removed its save"),
                Err(e) => warn!("couldn't remove {}: {e}", path.display()),
            }
        }
    }
}

/// What this build can't show: a minigame it doesn't have, or a character it doesn't know.
/// The save format can be right and the contents still from another version.
fn check_contents(save: Save) -> Result<Save, LoadError> {
    let known_minigames = save
        .game
        .minigame_ids()
        .all(|id| minigames::SHOWN.contains(&id));
    let known_characters = save
        .seats
        .iter()
        .all(|s| (s.character as usize) < Character::ALL.len());
    if known_minigames && known_characters {
        Ok(save)
    } else {
        Err(LoadError::Version {
            found: sim_board::SAVE_VERSION,
        })
    }
}

/// A saved game picked from the title, waiting on the join screen for its players.
#[derive(Resource, Debug, Clone)]
pub struct Resumed(pub Save);

/// Saves the game each time it reaches a point between turns, on the couch and on an online
/// host (clients don't), and removes the save once the game is over.
fn autosave(
    mut session: ResMut<BoardSession>,
    slot: Res<SaveSlot>,
    online: Option<Res<Online>>,
    seats: Res<Seats>,
    roster: Res<Roster>,
    rules: Res<RunRules>,
) {
    if matches!(online.as_deref(), Some(Online::Client(_))) || !session.take_changed() {
        return;
    }
    let game = &session.game;
    if matches!(game.awaiting(), Awaiting::GameOver) {
        slot.remove();
        return;
    }
    if !game.between_turns() || slot.0.is_none() {
        return;
    }
    let human = |seat: Seat| {
        matches!(
            seats.0[seat.index()],
            Some(Controller::Human { .. } | Controller::Remote { bot: false })
        )
    };
    let character = |seat: Seat| {
        let at = Character::ALL.iter().position(|c| *c == roster.of(seat));
        at.expect("every character is in ALL") as u8
    };
    let bots = seats.0.iter().flatten().find_map(|c| match c {
        Controller::Bot(difficulty) => Some(*difficulty),
        _ => None,
    });
    let save = Save {
        game: game.clone(),
        seats: Seat::ALL.map(|seat| SavedSeat {
            character: character(seat),
            human: human(seat),
        }),
        bots: bots.unwrap_or(Difficulty::Normal),
        pause_on_controller_lost: rules.0.pause_on_controller_lost,
    };
    match slot.write(&save) {
        Ok(()) => info!(
            "saved the game in round {}, checksum {:016x}",
            game.round(),
            checksum(game)
        ),
        Err(e) => warn!("couldn't save the game: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use sim_board::{Board, Game, Intent, Rules};

    use super::*;

    fn save() -> Save {
        let mut game = Game::new(
            Board::meadow(),
            Rules::default(),
            4,
            5,
            &minigames::shown_entries(),
        );
        while !game.between_turns() {
            let Awaiting::Intent { seat, .. } = game.awaiting() else {
                unreachable!("turn order comes first");
            };
            game.apply(seat, Intent::Roll).unwrap();
        }
        Save {
            game,
            seats: [0, 1, 2, 3].map(|character| SavedSeat {
                character,
                human: character == 0,
            }),
            bots: Difficulty::Easy,
            pause_on_controller_lost: false,
        }
    }

    #[test]
    fn a_save_file_loads_as_saved_and_bad_ones_are_refused_with_a_message() {
        let dir = std::env::temp_dir().join(format!("save-test-{}", std::process::id()));
        let slot = SaveSlot(Some(dir.join("nested").join(FILE)));
        assert!(!slot.exists());
        assert!(slot.load().is_err());

        let saved = save();
        slot.write(&saved).unwrap();
        assert!(slot.exists());
        assert_eq!(checksum(&slot.load().unwrap().game), checksum(&saved.game));

        let path = slot.0.clone().unwrap();
        std::fs::write(&path, b"not a save").unwrap();
        assert_eq!(slot.load().unwrap_err().0, tr!("notice-save-damaged"));

        let mut unknown = saved.clone();
        unknown.seats[2].character = Character::ALL.len() as u8;
        std::fs::write(&path, unknown.encode()).unwrap();
        assert_eq!(slot.load().unwrap_err().0, tr!("notice-save-version"));

        slot.remove();
        assert!(!slot.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
