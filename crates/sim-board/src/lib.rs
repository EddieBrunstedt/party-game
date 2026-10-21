//! The board phase: the board graph, the §3.1 rules as a host-authoritative state machine,
//! board bots, and a headless driver that plays whole games with minigames included.
//!
//! Plain deterministic Rust like every simulation crate (ADR 0001): the seeded RNG lives in the
//! state, collections are ordered, and there is no wall clock. Minigames come in as a
//! `&[MinigameEntry]` from the `minigame-registry` crate, which this crate doesn't depend on.

pub mod board;
pub mod bot;
pub mod game;
pub mod headless;
pub mod save;
#[cfg(test)]
mod testing;

pub use board::{Board, Space, SpaceId, SpaceKind};
pub use bot::bot_intent;
pub use game::{
    Awaiting, DIE_FACES, Decision, Event, Game, Intent, MinigamePlan, Phase, Player, Rejected,
    Rules,
};
pub use headless::{PlayedMinigame, Summary, play_bots};
pub use save::{LoadError, SAVE_VERSION, Save, SavedSeat};
