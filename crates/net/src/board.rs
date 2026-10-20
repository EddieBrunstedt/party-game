//! The board phase online (ADR 0003): the host owns the real [`Game`] and is the only machine
//! that applies anything, its own players' and bots' intents and those clients send for their
//! seats. Each accepted intent goes to every client with the game's checksum after it; a client
//! applies it to its copy and checks the sum. A copy that doesn't match asks for a snapshot.

use serde::{Deserialize, Serialize};
use sim_board::{Event, Game, Intent, Rejected};
use sim_core::{Outcome, Seat, checksum};

/// What the host tells clients about the board.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BoardUpdate {
    /// The whole game: at the start, and for a client whose copy went wrong (or reconnects).
    Snapshot(Box<Game>),
    /// The host applied `intent` for `seat`; `checksum` is the game's after it.
    Applied {
        seat: Seat,
        intent: Intent,
        checksum: u64,
    },
    /// A minigame finished with this outcome.
    Outcome { outcome: Outcome, checksum: u64 },
}

/// A client's copy no longer matches the host's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Desync {
    /// The copy refused what the host had accepted.
    Refused(Rejected),
    /// The copy got another checksum than the host.
    Checksum { host: u64, ours: u64 },
}

impl std::fmt::Display for Desync {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Desync::Refused(why) => write!(f, "our copy refused the host's move: {why}"),
            Desync::Checksum { host, ours } => {
                write!(f, "checksum {ours:016x} where the host has {host:016x}")
            }
        }
    }
}

/// Applies an update (other than a snapshot, which replaces the game) to a client's copy and
/// returns the events it produced, as the host's own `apply` did.
pub fn follow(game: &mut Game, update: &BoardUpdate) -> Result<Vec<Event>, Desync> {
    let (result, host) = match update {
        BoardUpdate::Snapshot(snapshot) => {
            *game = (**snapshot).clone();
            return Ok(Vec::new());
        }
        BoardUpdate::Applied {
            seat,
            intent,
            checksum,
        } => (game.apply(*seat, *intent), *checksum),
        BoardUpdate::Outcome { outcome, checksum } => {
            (game.apply_minigame_outcome(*outcome), *checksum)
        }
    };
    let events = result.map_err(Desync::Refused)?;
    let ours = checksum(game);
    if ours != host {
        return Err(Desync::Checksum { host, ours });
    }
    Ok(events)
}
