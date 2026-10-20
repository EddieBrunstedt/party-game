use std::hash::Hash;

use crate::{Inputs, MAX_PLAYERS};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Seat(pub u8);

impl Seat {
    pub const ALL: [Seat; MAX_PLAYERS] = [Seat(0), Seat(1), Seat(2), Seat(3)];

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Which seats take part. A seat being human or bot is deliberately not visible here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerSetup {
    pub active: [bool; MAX_PLAYERS],
}

impl PlayerSetup {
    pub fn first(count: usize) -> Self {
        assert!((1..=MAX_PLAYERS).contains(&count));
        let mut active = [false; MAX_PLAYERS];
        active[..count].fill(true);
        Self { active }
    }

    pub fn is_active(&self, seat: Seat) -> bool {
        self.active[seat.index()]
    }

    pub fn seats(&self) -> impl Iterator<Item = Seat> + '_ {
        Seat::ALL.into_iter().filter(|s| self.is_active(*s))
    }

    pub fn count(&self) -> usize {
        self.active.iter().filter(|a| **a).count()
    }
}

/// Team layout, decided by the board from the colours of the spaces players stand on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    FreeForAll,
    OneVsThree { solo: Seat },
    TwoVsTwo { team_of: [u8; MAX_PLAYERS] },
    Duel { a: Seat, b: Seat },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
}

/// Final placement per seat: 1 = first. Tied seats share a placement. Inactive seats are `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Outcome {
    pub placement: [Option<u8>; MAX_PLAYERS],
}

impl Outcome {
    /// Competition ranking ("1224") from per-seat scores, higher is better.
    pub fn from_scores(players: &PlayerSetup, scores: &[u32; MAX_PLAYERS]) -> Self {
        let mut placement = [None; MAX_PLAYERS];
        for seat in players.seats() {
            let better = players
                .seats()
                .filter(|other| scores[other.index()] > scores[seat.index()])
                .count();
            placement[seat.index()] = Some(better as u8 + 1);
        }
        Self { placement }
    }

    pub fn winners(&self) -> impl Iterator<Item = Seat> + '_ {
        Seat::ALL
            .into_iter()
            .filter(|s| self.placement[s.index()] == Some(1))
    }
}

/// A minigame's pure, deterministic simulation. Implemented by a marker type; all data lives in
/// [`Minigame::State`], which must be cheap to clone (rollback snapshots it every tick).
pub trait Minigame: 'static {
    type State: Clone + Hash + Send + Sync + 'static;

    /// Stable identifier used by the CLI, saves and free-play menu.
    const ID: &'static str;
    const SUPPORTED_FORMATS: &'static [FormatKind];
    /// Hard cap used by headless runs to catch minigames that never end.
    const MAX_TICKS: u32;

    fn new(seed: u64, players: PlayerSetup, format: Format) -> Self::State;
    fn step(state: &mut Self::State, inputs: &Inputs);
    fn outcome(state: &Self::State) -> Option<Outcome>;
    /// Bots are virtual controllers: they read the state and produce the same kind of input a
    /// human would. Must be a pure function of its arguments.
    fn bot_input(state: &Self::State, seat: Seat, difficulty: Difficulty) -> crate::Input;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatKind {
    FreeForAll,
    OneVsThree,
    TwoVsTwo,
    Duel,
}

impl Format {
    pub fn kind(&self) -> FormatKind {
        match self {
            Format::FreeForAll => FormatKind::FreeForAll,
            Format::OneVsThree { .. } => FormatKind::OneVsThree,
            Format::TwoVsTwo { .. } => FormatKind::TwoVsTwo,
            Format::Duel { .. } => FormatKind::Duel,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ties_share_placement() {
        let players = PlayerSetup::first(4);
        let outcome = Outcome::from_scores(&players, &[3, 1, 3, 0]);
        assert_eq!(outcome.placement, [Some(1), Some(3), Some(1), Some(4)]);
        assert_eq!(
            outcome.winners().collect::<Vec<_>>(),
            vec![Seat(0), Seat(2)]
        );
    }

    #[test]
    fn inactive_seats_have_no_placement() {
        let players = PlayerSetup::first(2);
        let outcome = Outcome::from_scores(&players, &[0, 5, 9, 9]);
        assert_eq!(outcome.placement, [Some(2), Some(1), None, None]);
    }
}
