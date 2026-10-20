//! Quick Draw: the lantern lights up after a random delay; first to press A wins the round.
//! Pressing before it lights is a false start and sits you out for the round.
//! First to [`POINTS_TO_WIN`] points (or most points after [`MAX_ROUNDS`]) wins.

use sim_core::{
    Button, Difficulty, Format, FormatKind, Input, Inputs, MAX_PLAYERS, Minigame, Outcome,
    PlayerSetup, Rng, Seat,
};

pub const POINTS_TO_WIN: u8 = 3;
pub const MAX_ROUNDS: u8 = 7;
pub const INTRO_TICKS: u16 = 120;
/// The lantern lights somewhere in this window after a round starts (1.5 s to 5 s).
pub const WAIT_TICKS: (u16, u16) = (90, 300);
/// Seats that haven't pressed this long after the lantern lights are too slow.
pub const DRAW_TIMEOUT_TICKS: u16 = 120;
pub const ROUND_OVER_TICKS: u16 = 150;

pub struct QuickDraw;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reaction {
    Waiting,
    FalseStart,
    /// Pressed this many ticks after the lantern lit.
    Fired {
        ticks: u16,
    },
    TooSlow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Intro {
        t: u16,
    },
    /// Lantern dark. `fire_at` is secret to players but visible to the presentation layer.
    Waiting {
        t: u16,
        fire_at: u16,
    },
    /// Lantern lit `t` ticks ago.
    Drawn {
        t: u16,
    },
    RoundOver {
        t: u16,
    },
    Finished,
}

#[derive(Debug, Clone, Hash)]
pub struct State {
    pub seed: u64,
    pub players: PlayerSetup,
    rng: Rng,
    pub phase: Phase,
    /// 1-based once the first round starts.
    pub round: u8,
    pub scores: [u8; MAX_PLAYERS],
    pub reactions: [Reaction; MAX_PLAYERS],
    pub round_winners: [bool; MAX_PLAYERS],
}

impl State {
    fn start_round(&mut self) {
        self.round += 1;
        self.reactions = [Reaction::Waiting; MAX_PLAYERS];
        self.round_winners = [false; MAX_PLAYERS];
        let fire_at = self.rng.range_u32(WAIT_TICKS.0 as u32, WAIT_TICKS.1 as u32) as u16;
        self.phase = Phase::Waiting { t: 0, fire_at };
    }

    fn waiting_seats(&self) -> impl Iterator<Item = Seat> + '_ {
        self.players
            .seats()
            .filter(|s| self.reactions[s.index()] == Reaction::Waiting)
    }

    fn end_round(&mut self) {
        let fastest = self
            .players
            .seats()
            .filter_map(|s| match self.reactions[s.index()] {
                Reaction::Fired { ticks } => Some(ticks),
                _ => None,
            })
            .min();
        if let Some(fastest) = fastest {
            for seat in self.players.seats() {
                if self.reactions[seat.index()] == (Reaction::Fired { ticks: fastest }) {
                    self.scores[seat.index()] += 1;
                    self.round_winners[seat.index()] = true;
                }
            }
        }
        self.phase = Phase::RoundOver { t: 0 };
    }
}

impl Minigame for QuickDraw {
    type State = State;

    const ID: &'static str = "quick-draw";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[FormatKind::FreeForAll, FormatKind::Duel];
    const MAX_TICKS: u32 = 6_000;

    fn new(seed: u64, players: PlayerSetup, _format: Format) -> State {
        State {
            seed,
            players,
            rng: Rng::new(seed),
            phase: Phase::Intro { t: 0 },
            round: 0,
            scores: [0; MAX_PLAYERS],
            reactions: [Reaction::Waiting; MAX_PLAYERS],
            round_winners: [false; MAX_PLAYERS],
        }
    }

    fn step(state: &mut State, inputs: &Inputs) {
        match state.phase {
            Phase::Intro { t } => {
                if t + 1 >= INTRO_TICKS {
                    state.start_round();
                } else {
                    state.phase = Phase::Intro { t: t + 1 };
                }
            }
            Phase::Waiting { t, fire_at } => {
                let jumped: Vec<Seat> = state
                    .waiting_seats()
                    .filter(|s| inputs.just_pressed(*s, Button::A))
                    .collect();
                for seat in jumped {
                    state.reactions[seat.index()] = Reaction::FalseStart;
                }
                if state.waiting_seats().next().is_none() {
                    state.end_round();
                } else if t + 1 >= fire_at {
                    state.phase = Phase::Drawn { t: 0 };
                } else {
                    state.phase = Phase::Waiting { t: t + 1, fire_at };
                }
            }
            Phase::Drawn { t } => {
                let t = t + 1;
                let fired: Vec<Seat> = state
                    .waiting_seats()
                    .filter(|s| inputs.just_pressed(*s, Button::A))
                    .collect();
                for seat in fired {
                    state.reactions[seat.index()] = Reaction::Fired { ticks: t };
                }
                if t >= DRAW_TIMEOUT_TICKS {
                    let slow: Vec<Seat> = state.waiting_seats().collect();
                    for seat in slow {
                        state.reactions[seat.index()] = Reaction::TooSlow;
                    }
                }
                if state.waiting_seats().next().is_none() {
                    state.end_round();
                } else {
                    state.phase = Phase::Drawn { t };
                }
            }
            Phase::RoundOver { t } => {
                if t + 1 < ROUND_OVER_TICKS {
                    state.phase = Phase::RoundOver { t: t + 1 };
                } else if state.scores.iter().any(|s| *s >= POINTS_TO_WIN)
                    || state.round >= MAX_ROUNDS
                {
                    state.phase = Phase::Finished;
                } else {
                    state.start_round();
                }
            }
            Phase::Finished => {}
        }
    }

    fn outcome(state: &State) -> Option<Outcome> {
        (state.phase == Phase::Finished)
            .then(|| Outcome::from_scores(&state.players, &state.scores.map(u32::from)))
    }

    fn bot_input(state: &State, seat: Seat, difficulty: Difficulty) -> Input {
        if !state.players.is_active(seat) || state.reactions[seat.index()] != Reaction::Waiting {
            return Input::default();
        }
        // Per-round, per-seat personality derived from the state, so the bot stays a pure function.
        let mut rng = Rng::new(
            state.seed
                ^ (state.round as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
                ^ (seat.0 as u64 + 1).wrapping_mul(0xbf58_476d_1ce4_e5b9),
        );
        let (fastest, slowest, false_start_percent) = match difficulty {
            Difficulty::Easy => (24, 40, 12),
            Difficulty::Normal => (17, 28, 5),
            Difficulty::Hard => (13, 20, 1),
        };
        let reaction = rng.range_u32(fastest, slowest) as u16;
        let jumps_early = rng.chance(false_start_percent);
        let jump_at = rng.range_u32(30, WAIT_TICKS.1 as u32) as u16;

        let press = match state.phase {
            Phase::Waiting { t, .. } => jumps_early && t >= jump_at,
            // `step` counts this press as `t + 1` ticks after lighting.
            Phase::Drawn { t } => t + 1 >= reaction,
            _ => false,
        };
        if press {
            Input::default().with(Button::A)
        } else {
            Input::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{Run, RunConfig};

    const NONE: [Input; MAX_PLAYERS] = [Input {
        buttons: 0,
        stick_x: 0,
        stick_y: 0,
    }; MAX_PLAYERS];

    fn press(seats: &[usize]) -> [Input; MAX_PLAYERS] {
        let mut inputs = NONE;
        for &s in seats {
            inputs[s] = Input::default().with(Button::A);
        }
        inputs
    }

    fn new_run(players: usize) -> Run<QuickDraw> {
        Run::new(
            1,
            PlayerSetup::first(players),
            Format::FreeForAll,
            RunConfig::default(),
        )
    }

    fn step_until(run: &mut Run<QuickDraw>, done: impl Fn(&Phase) -> bool) {
        for _ in 0..QuickDraw::MAX_TICKS {
            if done(&run.game.phase) {
                return;
            }
            run.step(NONE);
        }
        panic!("phase never reached");
    }

    fn to_waiting(run: &mut Run<QuickDraw>) {
        step_until(run, |p| matches!(p, Phase::Waiting { .. }));
    }

    fn to_drawn(run: &mut Run<QuickDraw>) {
        step_until(run, |p| matches!(p, Phase::Drawn { .. }));
    }

    #[test]
    fn pressing_before_the_light_is_a_false_start() {
        let mut run = new_run(2);
        to_waiting(&mut run);
        run.step(press(&[0]));
        assert_eq!(run.game.reactions[0], Reaction::FalseStart);
        assert_eq!(run.game.reactions[1], Reaction::Waiting);
    }

    #[test]
    fn false_starter_cannot_score() {
        let mut run = new_run(2);
        to_waiting(&mut run);
        run.step(press(&[0]));
        run.step(NONE);
        to_drawn(&mut run);
        run.step(press(&[0, 1]));
        assert_eq!(run.game.phase, Phase::RoundOver { t: 0 });
        assert_eq!(run.game.scores, [0, 1, 0, 0]);
    }

    #[test]
    fn fastest_press_wins_the_round() {
        let mut run = new_run(3);
        to_drawn(&mut run);
        run.step(NONE);
        run.step(press(&[2]));
        run.step(press(&[0, 2]));
        run.step(press(&[1]));
        assert_eq!(run.game.reactions[2], Reaction::Fired { ticks: 2 });
        assert_eq!(run.game.scores, [0, 0, 1, 0]);
    }

    #[test]
    fn simultaneous_presses_both_score() {
        let mut run = new_run(2);
        to_drawn(&mut run);
        run.step(press(&[0, 1]));
        assert_eq!(run.game.scores, [1, 1, 0, 0]);
    }

    #[test]
    fn everyone_false_starting_voids_the_round() {
        let mut run = new_run(2);
        to_waiting(&mut run);
        run.step(press(&[0, 1]));
        assert_eq!(run.game.phase, Phase::RoundOver { t: 0 });
        assert_eq!(run.game.scores, [0; MAX_PLAYERS]);
    }

    #[test]
    fn nobody_pressing_times_out() {
        let mut run = new_run(2);
        to_drawn(&mut run);
        for _ in 0..DRAW_TIMEOUT_TICKS {
            run.step(NONE);
        }
        assert_eq!(run.game.reactions[0], Reaction::TooSlow);
        assert!(matches!(run.game.phase, Phase::RoundOver { .. }));
    }

    #[test]
    fn first_to_three_wins() {
        let mut run = new_run(2);
        for _ in 0..POINTS_TO_WIN {
            to_drawn(&mut run);
            run.step(press(&[1]));
            run.step(NONE);
        }
        step_until(&mut run, |p| *p == Phase::Finished);
        let outcome = run.outcome().unwrap();
        assert_eq!(outcome.placement, [Some(2), Some(1), None, None]);
    }

    #[test]
    fn passes_all_automated_quality_bars() {
        sim_check::check_all::<QuickDraw>(8).unwrap();
    }
}
