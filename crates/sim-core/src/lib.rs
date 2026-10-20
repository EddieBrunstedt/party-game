//! Deterministic simulation building blocks shared by the board and every minigame.
//!
//! Nothing in here (or in any crate depending on it for simulation) may use Bevy, wall-clock
//! time, `HashMap` iteration, global RNG or platform math. See `docs/adr/0001` and `0006`.

pub mod hash;
pub mod input;
pub mod math;
pub mod minigame;
pub mod physics;
pub mod registry;
pub mod rng;
pub mod run;

pub use hash::checksum;
pub use input::{Button, Input, Inputs};
pub use minigame::{Difficulty, Format, FormatKind, Minigame, Outcome, PlayerSetup, Seat};
pub use physics::{Body, Vec2};
pub use registry::{DynRun, MinigameEntry};
pub use rng::Rng;
pub use run::{Run, RunConfig};

/// Every simulation steps at this fixed rate.
pub const TICK_HZ: u32 = 60;

/// Seats are fixed at four; inactive seats simply send [`Input::default`].
pub const MAX_PLAYERS: usize = 4;
