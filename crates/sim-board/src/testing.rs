//! Test fixtures: a tiny board and fake minigames, so board tests don't depend on real ones.

use sim_core::{
    Difficulty, Format, FormatKind, Input, Inputs, MinigameEntry, Outcome, PlayerSetup, Rng, Seat,
};

use crate::Board;

/// 0 → 1 (red) → 2, a fork to 3 or 4 (red), both rejoining at 5 → back to 0.
pub fn board() -> Board {
    Board::from_ron(
        "(
            name: \"Test Loop\",
            start: 0,
            star_spots: [1, 3, 4, 5],
            spaces: [
                (id: 0, x: 0.0, y: 0.0, kind: Blue, next: [1]),
                (id: 1, x: 2.0, y: 0.0, kind: Red, next: [2]),
                (id: 2, x: 4.0, y: 0.0, kind: Blue, next: [3, 4]),
                (id: 3, x: 6.0, y: 1.0, kind: Blue, next: [5]),
                (id: 4, x: 6.0, y: -1.0, kind: Red, next: [5]),
                (id: 5, x: 8.0, y: 0.0, kind: Blue, next: [0]),
            ],
        )",
    )
    .unwrap()
}

/// Finishes after a few ticks with random (seeded) team scores.
#[derive(Debug, Clone, Hash)]
pub struct FakeState {
    players: PlayerSetup,
    format: Format,
    rng: Rng,
    ticks: u32,
}

fn new(seed: u64, players: PlayerSetup, format: Format) -> FakeState {
    FakeState {
        players,
        format,
        rng: Rng::new(seed),
        ticks: 0,
    }
}

fn step(state: &mut FakeState, _inputs: &Inputs) {
    state.ticks += 1;
}

fn outcome(state: &FakeState) -> Option<Outcome> {
    let mut rng = state.rng;
    let scores = [(); 4].map(|_| rng.range_u32(0, 3));
    (state.ticks >= 10).then(|| Outcome::from_team_scores(&state.players, &state.format, &scores))
}

/// Supports free-for-all and both team formats.
pub struct Teams;

impl sim_core::Minigame for Teams {
    type State = FakeState;
    const ID: &'static str = "teams";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[
        FormatKind::FreeForAll,
        FormatKind::OneVsThree,
        FormatKind::TwoVsTwo,
    ];
    const MAX_TICKS: u32 = 100;

    fn new(seed: u64, players: PlayerSetup, format: Format) -> FakeState {
        new(seed, players, format)
    }
    fn step(state: &mut FakeState, inputs: &Inputs) {
        step(state, inputs)
    }
    fn outcome(state: &FakeState) -> Option<Outcome> {
        outcome(state)
    }
    fn bot_input(_state: &FakeState, _seat: Seat, _difficulty: Difficulty) -> Input {
        Input::default()
    }
}

/// Free-for-all only.
pub struct Solo;

impl sim_core::Minigame for Solo {
    type State = FakeState;
    const ID: &'static str = "solo";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[FormatKind::FreeForAll];
    const MAX_TICKS: u32 = 100;

    fn new(seed: u64, players: PlayerSetup, format: Format) -> FakeState {
        new(seed, players, format)
    }
    fn step(state: &mut FakeState, inputs: &Inputs) {
        step(state, inputs)
    }
    fn outcome(state: &FakeState) -> Option<Outcome> {
        outcome(state)
    }
    fn bot_input(_state: &FakeState, _seat: Seat, _difficulty: Difficulty) -> Input {
        Input::default()
    }
}

pub const BOTH: &[MinigameEntry] = &[MinigameEntry::of::<Teams>(), MinigameEntry::of::<Solo>()];
pub const SOLO_ONLY: &[MinigameEntry] = &[MinigameEntry::of::<Solo>()];
