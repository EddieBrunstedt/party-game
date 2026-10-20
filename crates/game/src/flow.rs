//! The screens the game moves between, and the hand-offs between them.

use bevy::prelude::*;
use sim_core::Outcome;

use crate::runner::RunSetup;

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    #[default]
    Title,
    #[allow(dead_code)] // The join screen comes with the Game flow item.
    Join,
    Board,
    Minigame,
    Results,
}

/// Where a minigame was started from, which decides what happens when it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Straight from the menu or CLI: A restarts it with a fresh seed.
    FreePlay,
    /// From the board: the result goes back to the board.
    Board,
}

/// The minigame to run on entering [`AppState::Minigame`].
#[derive(Resource, Debug, Clone)]
pub struct MinigameLaunch {
    pub id: String,
    pub setup: RunSetup,
    pub origin: Origin,
}

/// A board minigame's result, left for the board to collect.
#[derive(Resource, Debug, Clone, Copy)]
pub struct MinigameResult(pub Outcome);
