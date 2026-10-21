//! Lantern Echo: a memory race. Each player has a lane with four lanterns in a diamond (up,
//! right, down, left). The lanterns play a sequence; the player repeats it with the stick and
//! moves one step up the lane. The next sequence is the same plus one more. A wrong flick
//! stumbles you, and the same sequence plays again. First to the end of the lane wins.
//!
//! Teams add their steps up and need [`STAGES`] per member (the 1v3 trio a little less); a seat
//! can go [`TEAM_EXTRA_STEPS`] further than that to carry a teammate. Every seat has its own sequence, so watching a rival's
//! lane gives nothing away.

use sim_core::{
    Difficulty, Format, FormatKind, Input, Inputs, MAX_PLAYERS, Minigame, Outcome, PlayerSetup,
    Rng, Seat,
};

/// The opening countdown, the same in every minigame.
pub const INTRO_TICKS: u16 = sim_core::COUNTDOWN_TICKS;
/// Ticks of racing before time runs out.
pub const RACE_TICKS: u32 = 75 * 60;
/// Steps up the lane to finish alone (and per member in a team).
pub const STAGES: u8 = 6;
/// How many steps past [`STAGES`] a team member can go to make up for a teammate.
pub const TEAM_EXTRA_STEPS: u8 = 3;
/// What the trio in 1v3 needs between them. Less than three times [`STAGES`], because one
/// runner's time is skewed by the odd costly slip, while three runners' combined pace isn't, so
/// at 18 the solo won about 70% of bot races.
pub const TRIO_STEPS: u8 = 17;
/// The first sequence's length; each step adds one.
pub const FIRST_LENGTH: u8 = 3;
/// The longest sequence anyone can reach (a team member on their last step).
pub const MAX_LENGTH: usize = (FIRST_LENGTH + STAGES + TEAM_EXTRA_STEPS - 1) as usize;
/// Before a sequence plays, its lanterns stay dark this long.
pub const LEAD_IN_TICKS: u16 = 24;
/// Each lantern in a sequence stays lit this long...
pub const LIT_TICKS: u16 = 20;
/// ...with this much dark before the next.
pub const GAP_TICKS: u16 = 8;
/// After a right echo, the hop up the lane takes this long before the next sequence plays.
pub const STEP_TICKS: u16 = 30;
/// A wrong flick costs this long before the sequence plays again.
pub const STUMBLE_TICKS: u16 = 45;
/// After the result is decided, the game holds this long so the last hop plays out.
pub const DECIDED_TICKS: u16 = 90;
/// A stick past this (of 127) along one axis, and further along it than the other, is a flick.
pub const FLICK: i16 = 64;
/// The stick must come back inside this on both axes before the next flick counts.
pub const CENTRE: i16 = 40;

pub struct LanternEcho;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dir {
    Up,
    Right,
    Down,
    Left,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::Up, Dir::Right, Dir::Down, Dir::Left];

    pub fn index(self) -> usize {
        self as usize
    }

    /// The direction this many quarter turns clockwise.
    fn turned(self, quarters: usize) -> Dir {
        Dir::ALL[(self.index() + quarters) % 4]
    }

    /// The input a player gives to flick this way.
    pub fn stick(self) -> Input {
        let (x, y) = match self {
            Dir::Up => (0, 127),
            Dir::Right => (127, 0),
            Dir::Down => (0, -127),
            Dir::Left => (-127, 0),
        };
        Input {
            buttons: 0,
            stick_x: x,
            stick_y: y,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    /// Not racing: a seat outside a duel.
    Benched,
    /// The lanterns play the sequence; `t` ticks since it started.
    Watching { t: u16 },
    /// The player repeats it. `entered` flicks are right so far; `idle` ticks since the last one
    /// (or since the sequence ended).
    Echoing { entered: u8, idle: u16 },
    /// Hopping up the lane after a right echo.
    Stepping { t: u16 },
    /// Flicked `wrong` instead of the next lantern `t` ticks ago.
    Stumbling { t: u16, wrong: Dir },
    /// Went as far as this seat can; waits for the result.
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Runner {
    pub status: Status,
    /// Right echoes so far: how far up the lane.
    pub steps: u8,
    /// The most steps this seat can take.
    pub max_steps: u8,
    /// Wrong flicks so far.
    pub misses: u16,
    /// The stick has been back in the centre since the last flick.
    armed: bool,
}

impl Runner {
    /// The current sequence's length.
    pub fn length(&self) -> u8 {
        FIRST_LENGTH + self.steps.min(self.max_steps - 1)
    }

    /// Steps and the current echo, for ranking runners nobody finished.
    pub fn progress(&self) -> u32 {
        let entered = match self.status {
            Status::Echoing { entered, .. } => entered as u32,
            _ => 0,
        };
        self.steps as u32 * 16 + entered
    }

    /// Reads a flick from this tick's stick, at most once per trip out from the centre.
    fn flick(&mut self, input: &Input) -> Option<Dir> {
        let (x, y) = (input.stick_x as i16, input.stick_y as i16);
        if x.abs() < CENTRE && y.abs() < CENTRE {
            self.armed = true;
            return None;
        }
        if !self.armed {
            return None;
        }
        let dir = if x.abs() >= FLICK && x.abs() > y.abs() {
            if x > 0 { Dir::Right } else { Dir::Left }
        } else if y.abs() >= FLICK && y.abs() > x.abs() {
            if y > 0 { Dir::Up } else { Dir::Down }
        } else {
            return None;
        };
        self.armed = false;
        Some(dir)
    }
}

/// Steps a team of `members` needs to finish: [`STAGES`] each, except a trio, which needs
/// [`TRIO_STEPS`].
pub fn steps_needed(members: u32) -> u32 {
    if members == 3 {
        TRIO_STEPS as u32
    } else {
        members * STAGES as u32
    }
}

/// How long the lanterns take to play a sequence of `length`.
pub fn watch_ticks(length: u8) -> u16 {
    LEAD_IN_TICKS + length as u16 * (LIT_TICKS + GAP_TICKS)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Intro { t: u16 },
    Racing,
    /// The winner is known; everyone stands still while the last hop plays out.
    Decided { t: u16 },
    Finished,
}

#[derive(Debug, Clone, Hash)]
pub struct State {
    pub seed: u64,
    pub players: PlayerSetup,
    pub format: Format,
    pub phase: Phase,
    /// Ticks of racing so far.
    pub tick: u32,
    /// Each seat's whole sequence; step `n` plays its first `FIRST_LENGTH + n`.
    pub sequences: [[Dir; MAX_LENGTH]; MAX_PLAYERS],
    pub runners: [Runner; MAX_PLAYERS],
    /// Set when the race is decided.
    pub outcome: Option<Outcome>,
}

impl State {
    pub fn runner(&self, seat: Seat) -> &Runner {
        &self.runners[seat.index()]
    }

    /// The lantern lit in this seat's lane right now, while its sequence plays.
    pub fn lit(&self, seat: Seat) -> Option<Dir> {
        let runner = self.runner(seat);
        let Status::Watching { t } = runner.status else {
            return None;
        };
        let t = t.checked_sub(LEAD_IN_TICKS)?;
        let index = (t / (LIT_TICKS + GAP_TICKS)) as usize;
        (index < runner.length() as usize && t % (LIT_TICKS + GAP_TICKS) < LIT_TICKS)
            .then(|| self.sequences[seat.index()][index])
    }

    pub fn ticks_left(&self) -> u32 {
        RACE_TICKS.saturating_sub(self.tick)
    }

    /// Members of this seat's team (1 outside team formats).
    pub fn team_size(&self, seat: Seat) -> u32 {
        self.players
            .seats()
            .filter(|other| self.format.same_team(seat, *other))
            .count() as u32
    }

    /// Steps a whole team has taken, and the steps it needs, indexed by [`Format::team`].
    pub fn team_steps(&self) -> [(u32, u32); MAX_PLAYERS] {
        let mut steps = [0; MAX_PLAYERS];
        let mut members = [0; MAX_PLAYERS];
        for seat in self.players.seats() {
            if let Some(team) = self.format.team(seat) {
                steps[team as usize] += self.runner(seat).steps as u32;
                members[team as usize] += 1;
            }
        }
        std::array::from_fn(|team| (steps[team], steps_needed(members[team])))
    }

    /// Each team's share of the way to its finish, so a solo's 3 of 6 steps weighs as much as a
    /// pair's 6 of 12. A finished team's score is capped at the finish, so teams finishing on the
    /// same tick tie.
    fn team_scores(&self) -> [u32; MAX_PLAYERS] {
        let mut progress = [0u32; MAX_PLAYERS];
        let mut members = [0u32; MAX_PLAYERS];
        for seat in self.players.seats() {
            if let Some(team) = self.format.team(seat) {
                progress[team as usize] += self.runner(seat).progress();
                members[team as usize] += 1;
            }
        }
        std::array::from_fn(|team| {
            let finish = steps_needed(members[team]) * 16;
            // Divisible by every finish (6, 12, 17), so equal shares score exactly the same.
            (progress[team].min(finish) * 85_680)
                .checked_div(finish)
                .unwrap_or(0)
        })
    }

    fn step_runners(&mut self, inputs: &Inputs) {
        for seat in self.players.seats() {
            let sequence = self.sequences[seat.index()];
            let runner = &mut self.runners[seat.index()];
            let flick = runner.flick(&inputs.current[seat.index()]);
            runner.status = match runner.status {
                Status::Benched => Status::Benched,
                Status::Watching { t } if t + 1 >= watch_ticks(runner.length()) => {
                    Status::Echoing {
                        entered: 0,
                        idle: 0,
                    }
                }
                Status::Watching { t } => Status::Watching { t: t + 1 },
                Status::Echoing { entered, idle } => match flick {
                    Some(dir) if dir == sequence[entered as usize] => {
                        if entered + 1 >= runner.length() {
                            runner.steps += 1;
                            Status::Stepping { t: 0 }
                        } else {
                            Status::Echoing {
                                entered: entered + 1,
                                idle: 0,
                            }
                        }
                    }
                    Some(wrong) => {
                        runner.misses += 1;
                        Status::Stumbling { t: 0, wrong }
                    }
                    None => Status::Echoing {
                        entered,
                        idle: idle.saturating_add(1),
                    },
                },
                Status::Stepping { t } if t + 1 >= STEP_TICKS => {
                    if runner.steps >= runner.max_steps {
                        Status::Done
                    } else {
                        Status::Watching { t: 0 }
                    }
                }
                Status::Stepping { t } => Status::Stepping { t: t + 1 },
                Status::Stumbling { t, .. } if t + 1 >= STUMBLE_TICKS => Status::Watching { t: 0 },
                Status::Stumbling { t, wrong } => Status::Stumbling { t: t + 1, wrong },
                Status::Done => Status::Done,
            };
        }
    }

    fn decide_if_over(&mut self) {
        let finished = self
            .team_steps()
            .iter()
            .any(|(steps, needed)| *needed > 0 && steps >= needed);
        if self.outcome.is_none() && (finished || self.tick >= RACE_TICKS) {
            self.outcome = Some(Outcome::from_team_scores(
                &self.players,
                &self.format,
                &self.team_scores(),
            ));
            self.phase = Phase::Decided { t: 0 };
        }
    }
}

/// A seat's whole sequence: random, but never the same lantern three times running.
fn sequence(rng: &mut Rng) -> [Dir; MAX_LENGTH] {
    let mut sequence = [Dir::Up; MAX_LENGTH];
    for i in 0..MAX_LENGTH {
        sequence[i] = if i >= 2 && sequence[i - 1] == sequence[i - 2] {
            sequence[i - 1].turned(rng.range_u32(1, 3) as usize)
        } else {
            Dir::ALL[rng.range_u32(0, 3) as usize]
        };
    }
    sequence
}

impl Minigame for LanternEcho {
    type State = State;

    const ID: &'static str = "lantern-echo";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[
        FormatKind::FreeForAll,
        FormatKind::OneVsThree,
        FormatKind::TwoVsTwo,
        FormatKind::Duel,
    ];
    const MAX_TICKS: u32 = INTRO_TICKS as u32 + RACE_TICKS + DECIDED_TICKS as u32 + 60;

    fn new(seed: u64, players: PlayerSetup, format: Format) -> State {
        let mut rng = Rng::new(seed);
        let sequences = [(); MAX_PLAYERS].map(|_| sequence(&mut rng));
        let mut state = State {
            seed,
            players,
            format,
            phase: Phase::Intro { t: 0 },
            tick: 0,
            sequences,
            runners: [Runner {
                status: Status::Benched,
                steps: 0,
                max_steps: STAGES,
                misses: 0,
                armed: false,
            }; MAX_PLAYERS],
            outcome: None,
        };
        for seat in players.seats() {
            // Seats outside a duel sit this one out.
            if format.team(seat).is_some() {
                let team_size = state.team_size(seat);
                let runner = &mut state.runners[seat.index()];
                runner.status = Status::Watching { t: 0 };
                if team_size > 1 {
                    runner.max_steps = STAGES + TEAM_EXTRA_STEPS;
                }
            }
        }
        state
    }

    fn step(state: &mut State, inputs: &Inputs) {
        match state.phase {
            Phase::Intro { t } => {
                // Read the sticks anyway, so one held through the countdown doesn't count.
                for seat in state.players.seats() {
                    state.runners[seat.index()].flick(&inputs.current[seat.index()]);
                }
                state.phase = if t + 1 >= INTRO_TICKS {
                    Phase::Racing
                } else {
                    Phase::Intro { t: t + 1 }
                };
            }
            Phase::Racing => {
                state.tick += 1;
                state.step_runners(inputs);
                state.decide_if_over();
            }
            Phase::Decided { t } => {
                state.phase = if t + 1 >= DECIDED_TICKS {
                    Phase::Finished
                } else {
                    Phase::Decided { t: t + 1 }
                };
            }
            Phase::Finished => {}
        }
    }

    fn outcome(state: &State) -> Option<Outcome> {
        if state.phase == Phase::Finished {
            state.outcome
        } else {
            None
        }
    }

    fn bot_input(state: &State, seat: Seat, difficulty: Difficulty) -> Input {
        if state.phase != Phase::Racing || !state.players.is_active(seat) {
            return Input::default();
        }
        let runner = state.runner(seat);
        // Let go of the stick between flicks, as a person must.
        let Status::Echoing { entered, idle } = runner.status else {
            return Input::default();
        };
        if !runner.armed {
            return Input::default();
        }
        // One roll per flick, so the bot stays a pure function of the state.
        let mut rng = Rng::new(
            state.seed
                ^ (seat.0 as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15)
                ^ (runner.steps as u64 + 1).wrapping_mul(0xbf58_476d_1ce4_e5b9)
                ^ (runner.misses as u64 + 1).wrapping_mul(0x94d0_49bb_1331_11eb)
                ^ (entered as u64 + 1).wrapping_mul(0xd6e8_feb8_6659_fd93),
        );
        // Ticks per flick, extra thinking before the first, and the chance of a wrong flick in
        // thousandths: a base plus some per lantern in the sequence.
        let (pace, think, slip_base, slip_per_lantern) = match difficulty {
            Difficulty::Easy => ((20, 34), 24, 20, 8),
            Difficulty::Normal => ((14, 24), 16, 10, 5),
            Difficulty::Hard => ((10, 16), 8, 4, 2),
        };
        let mut wait = rng.range_u32(pace.0, pace.1);
        if entered == 0 {
            wait += think;
        }
        if (idle as u32) < wait {
            return Input::default();
        }
        let right = state.sequences[seat.index()][entered as usize];
        let slip = slip_base + slip_per_lantern * runner.length() as u32;
        if rng.range_u32(0, 999) < slip {
            right.turned(rng.range_u32(1, 3) as usize).stick()
        } else {
            right.stick()
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

    fn new_run(players: usize, format: Format) -> Run<LanternEcho> {
        Run::new(7, PlayerSetup::first(players), format, RunConfig::default())
    }

    fn step_until(run: &mut Run<LanternEcho>, done: impl Fn(&State) -> bool) {
        for _ in 0..LanternEcho::MAX_TICKS {
            if done(&run.game) {
                return;
            }
            run.step(NONE);
        }
        panic!("never reached");
    }

    fn echoing(seat: usize) -> impl Fn(&State) -> bool {
        move |game| matches!(game.runners[seat].status, Status::Echoing { .. })
    }

    /// Flick one seat's stick for a tick, then let it go.
    fn flick(run: &mut Run<LanternEcho>, seat: usize, dir: Dir) {
        let mut inputs = NONE;
        inputs[seat] = dir.stick();
        run.step(inputs);
        run.step(NONE);
    }

    /// Echo the seat's current sequence correctly (it must be echoing).
    fn echo(run: &mut Run<LanternEcho>, seat: usize) {
        let length = run.game.runners[seat].length() as usize;
        let sequence = run.game.sequences[seat];
        for dir in &sequence[..length] {
            flick(run, seat, *dir);
        }
    }

    fn wrong(dir: Dir) -> Dir {
        dir.turned(2)
    }

    #[test]
    fn a_right_echo_steps_up_and_the_next_sequence_is_longer() {
        let mut run = new_run(2, Format::FreeForAll);
        step_until(&mut run, echoing(0));
        assert_eq!(run.game.runners[0].length(), FIRST_LENGTH);
        echo(&mut run, 0);
        assert_eq!(run.game.runners[0].steps, 1);
        assert!(matches!(run.game.runners[0].status, Status::Stepping { .. }));
        step_until(&mut run, echoing(0));
        assert_eq!(run.game.runners[0].length(), FIRST_LENGTH + 1);
    }

    #[test]
    fn a_wrong_flick_stumbles_and_the_same_sequence_plays_again() {
        let mut run = new_run(2, Format::FreeForAll);
        step_until(&mut run, echoing(0));
        let sequence = run.game.sequences[0];
        flick(&mut run, 0, sequence[0]);
        flick(&mut run, 0, wrong(sequence[1]));
        assert!(matches!(
            run.game.runners[0].status,
            Status::Stumbling { .. }
        ));
        assert_eq!(run.game.runners[0].misses, 1);
        step_until(&mut run, |g| {
            matches!(g.runners[0].status, Status::Watching { .. })
        });
        assert_eq!(run.game.runners[0].steps, 0);
        assert_eq!(run.game.runners[0].length(), FIRST_LENGTH);
    }

    #[test]
    fn holding_the_stick_counts_once() {
        let mut run = new_run(2, Format::FreeForAll);
        step_until(&mut run, echoing(0));
        let first = run.game.sequences[0][0];
        let mut inputs = NONE;
        inputs[0] = first.stick();
        for _ in 0..20 {
            run.step(inputs);
        }
        assert!(matches!(
            run.game.runners[0].status,
            Status::Echoing { entered: 1, .. }
        ));
    }

    #[test]
    fn flicks_while_the_lanterns_play_dont_count() {
        let mut run = new_run(2, Format::FreeForAll);
        step_until(&mut run, |g| g.phase == Phase::Racing);
        for _ in 0..10 {
            flick(&mut run, 0, Dir::Up);
            flick(&mut run, 0, Dir::Left);
        }
        assert!(matches!(
            run.game.runners[0].status,
            Status::Watching { .. }
        ));
        assert_eq!(run.game.runners[0].misses, 0);
    }

    #[test]
    fn lanterns_light_in_sequence_order() {
        let mut run = new_run(1, Format::FreeForAll);
        step_until(&mut run, |g| g.phase == Phase::Racing);
        let mut seen = Vec::new();
        while matches!(run.game.runners[0].status, Status::Watching { .. }) {
            if let Some(dir) = run.game.lit(Seat(0))
                && seen.last() != Some(&dir)
            {
                seen.push(dir);
            }
            run.step(NONE);
        }
        // Repeats light twice with a gap between, so compare runs of distinct lanterns.
        let mut expected = run.game.sequences[0][..FIRST_LENGTH as usize].to_vec();
        expected.dedup();
        assert_eq!(seen, expected);
    }

    #[test]
    fn seats_have_their_own_sequences_without_triples() {
        let game = LanternEcho::new(3, PlayerSetup::first(4), Format::FreeForAll);
        assert_ne!(game.sequences[0], game.sequences[1]);
        for seed in 0..200 {
            let game = LanternEcho::new(seed, PlayerSetup::first(4), Format::FreeForAll);
            for sequence in game.sequences {
                assert!(sequence.windows(3).all(|w| w[0] != w[1] || w[1] != w[2]));
            }
        }
    }

    #[test]
    fn first_to_the_end_wins() {
        let mut run = new_run(3, Format::FreeForAll);
        for _ in 0..STAGES {
            step_until(&mut run, echoing(1));
            echo(&mut run, 1);
        }
        step_until(&mut run, |g| g.phase == Phase::Finished);
        let outcome = run.outcome().unwrap();
        assert_eq!(outcome.winners().collect::<Vec<_>>(), vec![Seat(1)]);
    }

    #[test]
    fn time_up_ranks_by_progress() {
        let mut run = new_run(3, Format::FreeForAll);
        step_until(&mut run, echoing(0));
        echo(&mut run, 0);
        step_until(&mut run, echoing(2));
        let first = run.game.sequences[2][0];
        flick(&mut run, 2, first);
        step_until(&mut run, |g| g.phase == Phase::Finished);
        assert_eq!(run.game.tick, RACE_TICKS);
        let outcome = run.outcome().unwrap();
        assert_eq!(outcome.placement, [Some(1), Some(3), Some(2), None]);
    }

    #[test]
    fn a_team_member_can_carry_their_teammate() {
        let format = Format::TwoVsTwo {
            team_of: [0, 1, 0, 1],
        };
        let mut run = new_run(4, format);
        // Seat 0 alone takes 9 steps; seat 2 needs 3 more for the team's 12.
        for _ in 0..STAGES + TEAM_EXTRA_STEPS {
            step_until(&mut run, echoing(0));
            echo(&mut run, 0);
        }
        step_until(&mut run, |g| g.runners[0].status == Status::Done);
        assert_eq!(run.game.runners[0].length(), MAX_LENGTH as u8);
        for _ in 0..STAGES - TEAM_EXTRA_STEPS {
            assert_eq!(run.game.phase, Phase::Racing);
            step_until(&mut run, echoing(2));
            echo(&mut run, 2);
        }
        step_until(&mut run, |g| g.phase == Phase::Finished);
        let outcome = run.outcome().unwrap();
        assert_eq!(outcome.placement, [Some(1), Some(2), Some(1), Some(2)]);
    }

    #[test]
    fn the_solo_needs_only_their_own_six() {
        let mut run = new_run(4, Format::OneVsThree { solo: Seat(3) });
        assert_eq!(run.game.runners[3].max_steps, STAGES);
        assert_eq!(run.game.runners[0].max_steps, STAGES + TEAM_EXTRA_STEPS);
        for _ in 0..STAGES {
            step_until(&mut run, echoing(3));
            echo(&mut run, 3);
        }
        step_until(&mut run, |g| g.phase == Phase::Finished);
        assert_eq!(
            run.outcome().unwrap().winners().collect::<Vec<_>>(),
            vec![Seat(3)]
        );
    }

    #[test]
    fn teams_rank_by_their_share_of_the_way() {
        let mut game = LanternEcho::new(1, PlayerSetup::first(4), Format::TwoVsTwo {
            team_of: [0, 1, 0, 1],
        });
        // P1 alone is a quarter of team 0's way; team 1's 2 of 12 steps is less.
        game.runners[0].steps = 3;
        game.runners[1].steps = 1;
        game.runners[3].steps = 1;
        let scores = game.team_scores();
        assert!(scores[0] > scores[1]);
        game.runners[3].steps = 2;
        assert_eq!(game.team_scores()[0], game.team_scores()[1]);

        let mut game = LanternEcho::new(1, PlayerSetup::first(4), Format::OneVsThree {
            solo: Seat(0),
        });
        game.runners[0].steps = STAGES;
        assert_eq!(
            game.team_steps()[1],
            (0, TRIO_STEPS as u32),
            "the trio needs {TRIO_STEPS}"
        );
        for seat in 1..4 {
            game.runners[seat].steps = 6;
        }
        // Both past their finish: capped, so equal.
        assert_eq!(game.team_scores()[0], game.team_scores()[1]);
    }

    #[test]
    fn seats_outside_a_duel_sit_out() {
        let run = new_run(4, Format::Duel { a: Seat(1), b: Seat(3) });
        assert_eq!(run.game.runners[0].status, Status::Benched);
        assert_eq!(run.game.runners[2].status, Status::Benched);
        assert!(matches!(run.game.runners[1].status, Status::Watching { .. }));
    }

    #[test]
    fn harder_bots_finish_faster() {
        let finish = |difficulty| {
            let mut total = 0;
            for seed in 0..20 {
                let mut run: Run<LanternEcho> = Run::new(
                    seed,
                    PlayerSetup::first(1),
                    Format::FreeForAll,
                    RunConfig::default(),
                );
                while run.outcome().is_none() {
                    let inputs = Seat::ALL.map(|s| LanternEcho::bot_input(&run.game, s, difficulty));
                    run.step(inputs);
                }
                total += run.game.tick;
            }
            total
        };
        let (easy, normal, hard) = (
            finish(Difficulty::Easy),
            finish(Difficulty::Normal),
            finish(Difficulty::Hard),
        );
        assert!(hard < normal && normal < easy, "{hard} {normal} {easy}");
    }

    #[test]
    fn passes_all_automated_quality_bars() {
        sim_check::check_all::<LanternEcho>(8).unwrap();
    }
}
