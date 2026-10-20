//! Type-erased minigames, so the board (and the game) can pick and launch any minigame by
//! format without knowing its concrete type. The list of minigames itself lives in the
//! `minigame-registry` crate, which depends on every minigame.

use std::any::Any;

use crate::{
    Difficulty, Format, FormatKind, Input, MAX_PLAYERS, Minigame, Outcome, PlayerSetup, Run,
    RunConfig, Seat, checksum,
};

/// A running minigame of any type. Implemented by every [`Run<M>`].
pub trait DynRun: Any + Send + Sync {
    fn id(&self) -> &'static str;
    fn step(&mut self, inputs: [Input; MAX_PLAYERS]);
    fn outcome(&self) -> Option<Outcome>;
    fn bot_input(&self, seat: Seat, difficulty: Difficulty) -> Input;
    fn checksum(&self) -> u64;
    /// Game time simulated so far, in ticks.
    fn tick(&self) -> u32;
    fn as_any(&self) -> &dyn Any;
}

impl<M: Minigame> DynRun for Run<M> {
    fn id(&self) -> &'static str {
        M::ID
    }

    fn step(&mut self, inputs: [Input; MAX_PLAYERS]) {
        Run::step(self, inputs);
    }

    fn outcome(&self) -> Option<Outcome> {
        Run::outcome(self)
    }

    fn bot_input(&self, seat: Seat, difficulty: Difficulty) -> Input {
        M::bot_input(&self.game, seat, difficulty)
    }

    fn checksum(&self) -> u64 {
        checksum(self)
    }

    fn tick(&self) -> u32 {
        self.tick
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// One registered minigame: what it is, which formats it supports, and how to start it.
#[derive(Clone, Copy)]
pub struct MinigameEntry {
    pub id: &'static str,
    pub formats: &'static [FormatKind],
    pub max_ticks: u32,
    pub start: fn(u64, PlayerSetup, Format, RunConfig) -> Box<dyn DynRun>,
}

impl MinigameEntry {
    pub const fn of<M: Minigame>() -> Self {
        Self {
            id: M::ID,
            formats: M::SUPPORTED_FORMATS,
            max_ticks: M::MAX_TICKS,
            start: |seed, players, format, config| {
                Box::new(Run::<M>::new(seed, players, format, config))
            },
        }
    }

    pub fn supports(&self, kind: FormatKind) -> bool {
        self.formats.contains(&kind)
    }
}

impl std::fmt::Debug for MinigameEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MinigameEntry")
            .field("id", &self.id)
            .field("formats", &self.formats)
            .finish()
    }
}

/// Play a minigame to the end with every seat a bot. Returns `None` if it runs past its
/// tick cap (a bug the quality bars should have caught).
pub fn play_with_bots(
    entry: &MinigameEntry,
    seed: u64,
    players: PlayerSetup,
    format: Format,
    difficulty: [Difficulty; MAX_PLAYERS],
) -> Option<Outcome> {
    let mut run = (entry.start)(seed, players, format, RunConfig::default());
    for _ in 0..entry.max_ticks {
        let inputs = Seat::ALL.map(|seat| run.bot_input(seat, difficulty[seat.index()]));
        run.step(inputs);
        if let Some(outcome) = run.outcome() {
            return Some(outcome);
        }
    }
    None
}
