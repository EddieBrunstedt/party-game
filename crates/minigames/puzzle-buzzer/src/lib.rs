//! Puzzle Buzzer: buzzer answering on small puzzles made from the seed. A puzzle appears with four
//! answers placed up, right, down and left; the stick points at one (nothing on screen shows
//! where) and A locks it in. The first right lock-in takes the puzzle. A wrong one is out of this
//! puzzle, and its answer is marked wrong for everyone. Nobody right within [`ANSWER_TICKS`]: the
//! answer is shown and nobody scores. First to [`GOAL`] puzzles wins, or the most after
//! [`MAX_PUZZLES`]. Puzzles get harder as the match goes on, and the kinds come round in a
//! shuffled order (see [`puzzle`]).
//!
//! 2v2 (two halves): each puzzle has two halves side by side, one for each partner. A team takes
//! the puzzle once both partners have locked in their half right, and a wrong half puts the whole
//! team out of it. A right half is a secret until the puzzle ends, so the other team (who has the
//! same halves) learns nothing from it.
//!
//! 1v3 (shared lockout): the three share one buzzer life, so a wrong lock-in from any of them puts
//! all three out of the puzzle; the solo answers alone, and the three's buzzers open a little
//! later ([`TRIO_WAIT`]). The solo needs [`SOLO_GOAL`] puzzles and the three need [`TRIO_GOAL`].

pub mod puzzle;

use sim_core::{
    Button, Difficulty, Format, FormatKind, Input, Inputs, MAX_PLAYERS, Minigame, Outcome,
    PlayerSetup, Rng, Seat,
};

pub use puzzle::{Answer, Board, Dir, Glyph, Kind, Op, Puzzle, Token};

/// The opening countdown, the same in every minigame.
pub const INTRO_TICKS: u16 = sim_core::COUNTDOWN_TICKS;
/// Puzzles to win, for each player (or 2v2 team).
pub const GOAL: u32 = 5;
/// In 1v3, puzzles the solo needs...
pub const SOLO_GOAL: u32 = 3;
/// ...and puzzles the three need between them.
pub const TRIO_GOAL: u32 = 5;
/// In 1v3 the three's buzzers open later than the solo's, by this share (percent) of the
/// puzzle's [`Board::effort`]. Being three, they're likelier to have the quickest answer, and the
/// quicker everyone is, the more that counts; a head start sized to the puzzle helps the solo
/// most in just those games.
pub const TRIO_WAIT: u32 = 55;
/// After this many puzzles the most wins, so a match of timeouts still ends.
pub const MAX_PUZZLES: u8 = 15;
/// Puzzles at each level before the next, harder one.
pub const PUZZLES_PER_LEVEL: u8 = 2;
/// The kind of puzzle is named this long before it appears.
pub const SHOW_TICKS: u16 = 45;
/// Time to answer.
pub const ANSWER_TICKS: u16 = 10 * 60;
/// The right answer, and who got it, stays up this long.
pub const RESULT_TICKS: u16 = 120;
/// After the last puzzle, the result stays up this long.
pub const DECIDED_TICKS: u16 = 60;

pub struct PuzzleBuzzer;

/// How puzzles are played, from the format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Free-for-all and duels: everyone for themselves, one puzzle at a time.
    Single,
    /// 2v2: two puzzles at once, one for each partner.
    Halves,
    /// 1v3: one puzzle, and the three share their buzzer life.
    Shared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Answerer {
    /// False for seats outside a duel.
    pub playing: bool,
    /// Which board this seat answers: 0, or 1 for the second partner in 2v2.
    pub half: u8,
    /// Where the stick last pointed this puzzle. Secret: views must not show it.
    aim: Option<Dir>,
    /// This puzzle's lock-in. A wrong one is out in the open at once (its answer is marked wrong);
    /// a right one ends the puzzle, except a 2v2 half's, which stays secret until it ends.
    pub lock: Option<Dir>,
}

impl Answerer {
    pub fn wrong(&self, board: &Board) -> bool {
        self.lock.is_some_and(|dir| dir != board.right)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Intro {
        t: u16,
    },
    /// The kind of puzzle is named; the puzzle isn't shown yet.
    Showing {
        t: u16,
    },
    Answering {
        t: u16,
    },
    /// The answer is shown.
    Result {
        t: u16,
    },
    /// The match is over; the result stays up.
    Decided {
        t: u16,
    },
    Finished,
}

#[derive(Debug, Clone, Hash)]
pub struct State {
    pub seed: u64,
    pub players: PlayerSetup,
    pub format: Format,
    pub mode: Mode,
    pub phase: Phase,
    rng: Rng,
    /// The puzzle being played, from 0.
    pub index: u8,
    /// The kinds in the order they come round; refilled and reshuffled when used up.
    bag: [Kind; 6],
    drawn: u8,
    /// This puzzle's boards: one, or two in 2v2.
    pub boards: [Board; 2],
    pub answerers: [Answerer; MAX_PLAYERS],
    /// By [`Format::team`]: puzzles taken...
    pub points: [u32; MAX_PLAYERS],
    /// ...whether the team is out of this puzzle...
    pub out: [bool; MAX_PLAYERS],
    /// ...and whether it took this one.
    pub took: [bool; MAX_PLAYERS],
    /// Set when the match is decided.
    pub outcome: Option<Outcome>,
}

impl State {
    pub fn answerer(&self, seat: Seat) -> &Answerer {
        &self.answerers[seat.index()]
    }

    /// The solo in 1v3.
    pub fn solo(&self) -> Option<Seat> {
        match self.format {
            Format::OneVsThree { solo } => Some(solo),
            _ => None,
        }
    }

    pub fn halves(&self) -> usize {
        if self.mode == Mode::Halves { 2 } else { 1 }
    }

    /// The board `seat` answers.
    pub fn board_of(&self, seat: Seat) -> &Board {
        &self.boards[self.answerer(seat).half as usize]
    }

    /// Seats in the game, in seat order.
    pub fn playing(&self) -> impl Iterator<Item = Seat> + '_ {
        self.players.seats().filter(|s| self.answerer(*s).playing)
    }

    pub fn team(&self, seat: Seat) -> usize {
        self.format.team(seat).unwrap_or(0) as usize
    }

    /// The teams in the game, in order.
    pub fn teams(&self) -> Vec<usize> {
        let mut teams: Vec<usize> = self.playing().map(|s| self.team(s)).collect();
        teams.sort();
        teams.dedup();
        teams
    }

    /// Puzzles a team needs to win.
    pub fn goal(&self, team: usize) -> u32 {
        match (self.mode, team) {
            (Mode::Shared, 0) => SOLO_GOAL,
            (Mode::Shared, _) => TRIO_GOAL,
            _ => GOAL,
        }
    }

    /// The puzzles' level: they get harder every [`PUZZLES_PER_LEVEL`].
    pub fn level(&self) -> u8 {
        (self.index / PUZZLES_PER_LEVEL).min(puzzle::LEVELS - 1)
    }

    /// Whether this puzzle's answers are out in the open.
    pub fn revealed(&self) -> bool {
        matches!(
            self.phase,
            Phase::Result { .. } | Phase::Decided { .. } | Phase::Finished
        )
    }

    /// Whether the puzzle is on screen (it isn't while its kind is being named).
    pub fn shown(&self) -> bool {
        matches!(self.phase, Phase::Answering { .. }) || self.revealed()
    }

    /// Ticks left to answer, while answering.
    pub fn ticks_left(&self) -> Option<u16> {
        match self.phase {
            Phase::Answering { t } => Some(ANSWER_TICKS - t),
            _ => None,
        }
    }

    /// Whether `seat` can still lock in.
    pub fn can_answer(&self, seat: Seat) -> bool {
        let answerer = self.answerer(seat);
        matches!(self.phase, Phase::Answering { .. })
            && answerer.playing
            && answerer.lock.is_none()
            && !self.out[self.team(seat)]
    }

    /// Whether `seat` can lock in now: it can answer, and in 1v3, the three's wait is over.
    pub fn buzzer_open(&self, seat: Seat) -> bool {
        self.can_answer(seat) && self.wait_left(seat) == 0
    }

    /// Ticks until `seat`'s buzzer opens this puzzle: in 1v3, the three wait [`Self::trio_wait`].
    pub fn wait_left(&self, seat: Seat) -> u16 {
        if self.mode != Mode::Shared || self.solo() == Some(seat) {
            return 0;
        }
        match self.phase {
            Phase::Answering { t } => self.trio_wait().saturating_sub(t),
            Phase::Showing { .. } => self.trio_wait(),
            _ => 0,
        }
    }

    /// How long the three wait for their buzzers this puzzle, in 1v3.
    pub fn trio_wait(&self) -> u16 {
        (self.boards[0].effort() * TRIO_WAIT / 100) as u16
    }

    /// Each team's result for [`Outcome::from_team_scores`]: puzzles taken, or in 1v3, 1 for
    /// the side that won.
    pub fn team_scores(&self) -> [u32; MAX_PLAYERS] {
        if self.mode != Mode::Shared {
            return self.points;
        }
        // Whoever got further towards their goal (only short of both at the puzzle limit).
        let solo = self.points[0] * TRIO_GOAL;
        let trio = self.points[1] * SOLO_GOAL;
        let mut scores = [0; MAX_PLAYERS];
        scores[0] = (solo >= trio) as u32;
        scores[1] = (trio >= solo) as u32;
        scores
    }

    fn draw_kind(&mut self) -> Kind {
        if self.drawn as usize >= self.bag.len() {
            let last = self.bag[self.bag.len() - 1];
            puzzle::shuffle(&mut self.rng, &mut self.bag);
            // Never the same kind twice running.
            if self.bag[0] == last {
                self.bag.swap(0, 1 + self.rng.range_u32(0, 4) as usize);
            }
            self.drawn = 0;
        }
        self.drawn += 1;
        self.bag[self.drawn as usize - 1]
    }

    fn start_puzzle(&mut self) {
        let level = self.level();
        for half in 0..self.halves() {
            let kind = self.draw_kind();
            self.boards[half] = Board::new(&mut self.rng, kind, level);
        }
        for answerer in &mut self.answerers {
            answerer.aim = None;
            answerer.lock = None;
        }
        self.out = [false; MAX_PLAYERS];
        self.took = [false; MAX_PLAYERS];
        self.phase = Phase::Showing { t: 0 };
    }

    /// Reads everyone's stick and A. Returns whether the puzzle is over.
    fn read_answers(&mut self, inputs: &Inputs) -> bool {
        // Every lock-in this tick is weighed against the wrong answers as they stood before it,
        // so seat order never matters.
        let wrong_before = self.boards.map(|b| b.wrong);
        let seats: Vec<Seat> = self.playing().collect();
        for seat in seats {
            if !self.can_answer(seat) {
                continue;
            }
            let half = self.answerer(seat).half as usize;
            let open = self.buzzer_open(seat);
            let answerer = &mut self.answerers[seat.index()];
            if let Some(dir) = Dir::of_stick(&inputs.current[seat.index()]) {
                answerer.aim = Some(dir);
            }
            let Some(aim) = answerer.aim else {
                continue;
            };
            // An answer already shown wrong can't be locked in.
            if !inputs.just_pressed(seat, Button::A) || wrong_before[half][aim.index()] || !open {
                continue;
            }
            answerer.lock = Some(aim);
            if aim != self.boards[half].right {
                self.boards[half].wrong[aim.index()] = true;
            }
        }
        // A wrong lock-in puts its team out, even if a partner was right on the same tick.
        for seat in self.playing().collect::<Vec<_>>() {
            if self.answerer(seat).wrong(self.board_of(seat)) {
                let team = self.team(seat);
                self.out[team] = true;
            }
        }
        let mut anyone = false;
        for team in self.teams() {
            let done = !self.out[team]
                && (0..self.halves()).all(|half| {
                    self.playing().any(|s| {
                        self.team(s) == team
                            && self.answerer(s).half as usize == half
                            && self.answerer(s).lock == Some(self.boards[half].right)
                    })
                });
            if done {
                self.took[team] = true;
                self.points[team] += 1;
                anyone = true;
            }
        }
        anyone || self.teams().iter().all(|team| self.out[*team])
    }

    fn decided(&self) -> bool {
        self.index + 1 >= MAX_PUZZLES
            || self
                .teams()
                .iter()
                .any(|team| self.points[*team] >= self.goal(*team))
    }
}

impl Minigame for PuzzleBuzzer {
    type State = State;

    const ID: &'static str = "puzzle-buzzer";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[
        FormatKind::FreeForAll,
        FormatKind::OneVsThree,
        FormatKind::TwoVsTwo,
        FormatKind::Duel,
    ];
    const MAX_TICKS: u32 = INTRO_TICKS as u32
        + MAX_PUZZLES as u32 * (SHOW_TICKS + ANSWER_TICKS + RESULT_TICKS) as u32
        + DECIDED_TICKS as u32
        + 60;

    fn new(seed: u64, players: PlayerSetup, format: Format) -> State {
        let mode = match format {
            Format::OneVsThree { .. } => Mode::Shared,
            Format::TwoVsTwo { .. } => Mode::Halves,
            Format::FreeForAll | Format::Duel { .. } => Mode::Single,
        };
        let mut answerers = [Answerer {
            playing: false,
            half: 0,
            aim: None,
            lock: None,
        }; MAX_PLAYERS];
        for seat in players.seats() {
            // Seats outside a duel sit this one out.
            answerers[seat.index()].playing = format.team(seat).is_some();
            // In 2v2 each team's first seat takes the left half and its second the right.
            if mode == Mode::Halves {
                let team = format.team(seat);
                let before = players
                    .seats()
                    .filter(|s| *s < seat && format.team(*s) == team)
                    .count();
                answerers[seat.index()].half = before.min(1) as u8;
            }
        }
        let mut rng = Rng::new(seed);
        let mut bag = Kind::ALL;
        puzzle::shuffle(&mut rng, &mut bag);
        let empty = Board::new(&mut Rng::new(0), Kind::Sum, 0);
        State {
            seed,
            players,
            format,
            mode,
            phase: Phase::Intro { t: 0 },
            rng,
            index: 0,
            bag,
            drawn: 0,
            boards: [empty; 2],
            answerers,
            points: [0; MAX_PLAYERS],
            out: [false; MAX_PLAYERS],
            took: [false; MAX_PLAYERS],
            outcome: None,
        }
    }

    fn step(state: &mut State, inputs: &Inputs) {
        match state.phase {
            Phase::Intro { t } => {
                if t + 1 >= INTRO_TICKS {
                    state.start_puzzle();
                } else {
                    state.phase = Phase::Intro { t: t + 1 };
                }
            }
            Phase::Showing { t } => {
                state.phase = if t + 1 >= SHOW_TICKS {
                    Phase::Answering { t: 0 }
                } else {
                    Phase::Showing { t: t + 1 }
                };
            }
            Phase::Answering { t } => {
                let over = state.read_answers(inputs);
                state.phase = if over || t + 1 >= ANSWER_TICKS {
                    Phase::Result { t: 0 }
                } else {
                    Phase::Answering { t: t + 1 }
                };
            }
            Phase::Result { t } => {
                if t + 1 < RESULT_TICKS {
                    state.phase = Phase::Result { t: t + 1 };
                } else if state.decided() {
                    state.outcome = Some(Outcome::from_team_scores(
                        &state.players,
                        &state.format,
                        &state.team_scores(),
                    ));
                    state.phase = Phase::Decided { t: 0 };
                } else {
                    state.index += 1;
                    state.start_puzzle();
                }
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
        let Phase::Answering { t } = state.phase else {
            return Input::default();
        };
        if !state.players.is_active(seat) || !state.buzzer_open(seat) {
            return Input::default();
        }
        let board = state.board_of(seat);
        let plan = bot::plan(state, seat, board, difficulty);
        if (t as u32) < plan.think {
            return Input::default();
        }
        // One that doesn't know guesses among the answers not yet shown wrong.
        let dir = if plan.knows {
            board.right
        } else {
            let open: Vec<Dir> = Dir::ALL
                .into_iter()
                .filter(|d| !board.wrong[d.index()])
                .collect();
            open[plan.guess as usize % open.len()]
        };
        dir.stick().with(Button::A)
    }
}

/// Bots roll once per puzzle whether they know the answer and how long they think, from the
/// puzzle's [`Board::effort`] and their difficulty.
pub mod bot {
    use super::*;

    pub struct Skill {
        /// Chance (percent) of knowing the answer at the easiest level...
        pub knows: u32,
        /// ...less this much per level.
        pub knows_per_level: u32,
        /// Thinking time as a share (percent) of a puzzle's effort...
        pub pace: u32,
        /// ...spread this much (percent) either way, rolled per puzzle.
        pub spread: u32,
        /// Ticks to move the stick and press, on top.
        pub reaction: u32,
    }

    pub fn skill(difficulty: Difficulty) -> Skill {
        match difficulty {
            Difficulty::Easy => Skill {
                knows: 62,
                knows_per_level: 3,
                pace: 145,
                spread: 45,
                reaction: 24,
            },
            Difficulty::Normal => Skill {
                knows: 76,
                knows_per_level: 3,
                pace: 100,
                spread: 45,
                reaction: 18,
            },
            Difficulty::Hard => Skill {
                knows: 88,
                knows_per_level: 3,
                pace: 72,
                spread: 45,
                reaction: 12,
            },
        }
    }

    pub struct Plan {
        pub knows: bool,
        /// Ticks into answering before it locks in.
        pub think: u32,
        /// Which of the answers not shown wrong it guesses, if it doesn't know.
        pub guess: u32,
    }

    /// Mixes one part into a seed. Each part goes in on its own: XORed together, a match's seed
    /// and a puzzle number could cancel and give every match the same rolls.
    fn mix(seed: u64, part: u64) -> u64 {
        let mut rng = Rng::new(seed ^ part.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        ((rng.next_u32() as u64) << 32) | rng.next_u32() as u64
    }

    pub fn plan(state: &State, seat: Seat, board: &Board, difficulty: Difficulty) -> Plan {
        let skill = skill(difficulty);
        let seed = mix(mix(state.seed, seat.0 as u64 + 1), state.index as u64 + 1);
        let mut rng = Rng::new(seed);
        let knows = skill
            .knows
            .saturating_sub(skill.knows_per_level * board.level as u32);
        let share = skill.pace + rng.range_u32(0, 2 * skill.spread) - skill.spread;
        Plan {
            knows: rng.chance(knows),
            think: board.effort() * share / 100 + skill.reaction,
            guess: rng.next_u32(),
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

    fn new_run(players: usize, format: Format) -> Run<PuzzleBuzzer> {
        Run::new(7, PlayerSetup::first(players), format, RunConfig::default())
    }

    fn step_until(run: &mut Run<PuzzleBuzzer>, done: impl Fn(&State) -> bool) {
        for _ in 0..PuzzleBuzzer::MAX_TICKS {
            if done(&run.game) {
                return;
            }
            run.step(NONE);
        }
        panic!("never reached");
    }

    fn answering(players: usize, format: Format) -> Run<PuzzleBuzzer> {
        let mut run = new_run(players, format);
        step_until(&mut run, |g| matches!(g.phase, Phase::Answering { .. }));
        run
    }

    /// The direction of a wrong answer on `seat`'s board, the `k`th from the right one.
    fn wrong_dir(game: &State, seat: Seat, k: usize) -> Dir {
        let right = game.board_of(seat).right;
        Dir::ALL[(right.index() + 1 + k) % 4]
    }

    /// Steps once with each `(seat, dir)` pointing that way and pressing A.
    fn lock(run: &mut Run<PuzzleBuzzer>, locks: &[(usize, Dir)]) {
        let mut inputs = NONE;
        for (seat, dir) in locks {
            inputs[*seat] = dir.stick().with(Button::A);
        }
        run.step(inputs);
    }

    fn right(run: &Run<PuzzleBuzzer>, seat: usize) -> Dir {
        run.game.board_of(Seat(seat as u8)).right
    }

    #[test]
    fn passes_check_all() {
        sim_check::check_all::<PuzzleBuzzer>(4).unwrap();
    }

    #[test]
    fn the_first_right_answer_takes_the_puzzle() {
        let mut run = answering(4, Format::FreeForAll);
        let r = right(&run, 2);
        lock(&mut run, &[(2, r)]);
        assert_eq!(run.game.points, [0, 0, 1, 0]);
        assert!(matches!(run.game.phase, Phase::Result { t: 0 }));
        assert!(run.game.took[2]);
    }

    #[test]
    fn a_wrong_answer_is_out_and_shown_wrong() {
        let mut run = answering(3, Format::FreeForAll);
        let w = wrong_dir(&run.game, Seat(0), 0);
        lock(&mut run, &[(0, w)]);
        assert!(run.game.out[0]);
        assert!(run.game.boards[0].wrong[w.index()]);
        assert!(matches!(run.game.phase, Phase::Answering { .. }));
        // Out: a right answer afterwards doesn't count.
        let r = right(&run, 0);
        run.step(NONE);
        lock(&mut run, &[(0, r)]);
        assert_eq!(run.game.points, [0; 4]);
        // Nobody else can lock in an answer already shown wrong.
        run.step(NONE);
        lock(&mut run, &[(1, w)]);
        assert_eq!(run.game.answerer(Seat(1)).lock, None);
        assert!(!run.game.out[1]);
    }

    #[test]
    fn everyone_wrong_ends_the_puzzle() {
        let mut run = answering(2, Format::FreeForAll);
        let (a, b) = (
            wrong_dir(&run.game, Seat(0), 0),
            wrong_dir(&run.game, Seat(0), 1),
        );
        lock(&mut run, &[(0, a), (1, b)]);
        assert!(matches!(run.game.phase, Phase::Result { .. }));
        assert_eq!(run.game.points, [0; 4]);
    }

    #[test]
    fn right_on_the_same_tick_both_score() {
        let mut run = answering(4, Format::FreeForAll);
        let r = right(&run, 0);
        lock(&mut run, &[(1, r), (3, r)]);
        assert_eq!(run.game.points, [0, 1, 0, 1]);
    }

    #[test]
    fn time_runs_out() {
        let mut run = answering(4, Format::FreeForAll);
        for _ in 0..ANSWER_TICKS - 1 {
            run.step(NONE);
        }
        assert!(matches!(run.game.phase, Phase::Answering { .. }));
        run.step(NONE);
        assert!(matches!(run.game.phase, Phase::Result { .. }));
        assert_eq!(run.game.points, [0; 4]);
    }

    /// The stick can be let go before pressing A: the last place it pointed counts.
    #[test]
    fn the_last_aim_counts() {
        let mut run = answering(2, Format::FreeForAll);
        let r = right(&run, 0);
        let mut inputs = NONE;
        inputs[0] = r.stick();
        run.step(inputs);
        lock(&mut run, &[]);
        assert_eq!(run.game.points, [0; 4]);
        let mut inputs = NONE;
        inputs[0] = Input::default().with(Button::A);
        run.step(inputs);
        assert_eq!(run.game.points, [1, 0, 0, 0]);
    }

    /// A 1v3 run at the first tick the three's buzzers are open.
    fn trio_open(format: Format) -> Run<PuzzleBuzzer> {
        let mut run = answering(4, format);
        let three = Seat::ALL
            .into_iter()
            .find(|s| Some(*s) != run.game.solo())
            .unwrap();
        step_until(&mut run, |g| g.buzzer_open(three));
        run
    }

    /// The three's buzzers open a share of the puzzle's effort after the solo's: an A press
    /// before then does nothing, and the solo can answer from the start.
    #[test]
    fn the_three_wait() {
        let format = Format::OneVsThree { solo: Seat(3) };
        let mut run = answering(4, format);
        let wait = run.game.trio_wait();
        assert_eq!(wait as u32, run.game.boards[0].effort() * TRIO_WAIT / 100);
        assert!(wait > 30, "{wait}");
        assert_eq!(run.game.wait_left(Seat(0)), wait);
        assert_eq!(run.game.wait_left(Seat(3)), 0);
        let r = right(&run, 0);
        lock(&mut run, &[(0, r)]);
        assert_eq!(run.game.answerer(Seat(0)).lock, None);
        assert_eq!(run.game.points, [0; 4]);
        step_until(&mut run, |g| g.buzzer_open(Seat(0)));
        assert!(matches!(run.game.phase, Phase::Answering { t } if t == wait));
        lock(&mut run, &[(0, r)]);
        assert_eq!(run.game.points[1], 1);
        // The solo, meanwhile, could have answered at once.
        let mut run = answering(4, format);
        assert!(run.game.buzzer_open(Seat(3)));
        let r = right(&run, 3);
        lock(&mut run, &[(3, r)]);
        assert_eq!(run.game.points[0], 1);
    }

    #[test]
    fn the_three_share_a_life() {
        let format = Format::OneVsThree { solo: Seat(1) };
        let mut run = trio_open(format);
        let w = wrong_dir(&run.game, Seat(0), 0);
        lock(&mut run, &[(0, w)]);
        let trio = run.game.team(Seat(0));
        assert!(run.game.out[trio]);
        for seat in [0, 2, 3] {
            assert!(!run.game.can_answer(Seat(seat)));
        }
        assert!(run.game.can_answer(Seat(1)));
        let r = right(&run, 1);
        run.step(NONE);
        lock(&mut run, &[(2, r)]);
        assert_eq!(run.game.points[trio], 0);
        lock(&mut run, &[(1, r)]);
        assert_eq!(run.game.points[0], 1);
    }

    #[test]
    fn any_of_the_three_can_take_it() {
        let mut run = trio_open(Format::OneVsThree { solo: Seat(0) });
        let r = right(&run, 3);
        lock(&mut run, &[(3, r)]);
        assert_eq!(run.game.points[1], 1);
        assert_eq!(run.game.points[0], 0);
    }

    #[test]
    fn halves_need_both_partners() {
        let format = Format::TwoVsTwo {
            team_of: [0, 1, 1, 0],
        };
        let mut run = answering(4, format);
        let game = &run.game;
        assert_eq!(
            [0, 1, 2, 3].map(|s| game.answerer(Seat(s)).half),
            [0, 0, 1, 1]
        );
        assert_ne!(game.boards[0], game.boards[1]);
        assert_ne!(game.boards[0].kind, game.boards[1].kind);
        let left = right(&run, 0);
        let right_half = right(&run, 3);
        lock(&mut run, &[(0, left)]);
        assert_eq!(run.game.points, [0; 4]);
        assert!(matches!(run.game.phase, Phase::Answering { .. }));
        // A right half isn't shown: nothing is marked.
        assert_eq!(run.game.boards[0].wrong, [false; 4]);
        // The other team's wrong half puts both of them out.
        let w = wrong_dir(&run.game, Seat(2), 0);
        lock(&mut run, &[(2, w)]);
        assert!(run.game.out[1]);
        assert!(!run.game.can_answer(Seat(1)));
        lock(&mut run, &[(3, right_half)]);
        assert_eq!(run.game.points, [1, 0, 0, 0]);
    }

    #[test]
    fn first_to_the_goal_wins() {
        let mut run = new_run(
            2,
            Format::Duel {
                a: Seat(0),
                b: Seat(1),
            },
        );
        for _ in 0..GOAL {
            step_until(&mut run, |g| matches!(g.phase, Phase::Answering { .. }));
            let r = right(&run, 1);
            lock(&mut run, &[(1, r)]);
        }
        step_until(&mut run, |g| g.phase == Phase::Finished);
        let outcome = PuzzleBuzzer::outcome(&run.game).unwrap();
        assert_eq!(outcome.placement, [Some(2), Some(1), None, None]);
        assert_eq!(run.game.index as u32, GOAL - 1);
    }

    #[test]
    fn the_solo_wins_at_its_own_goal() {
        let mut run = new_run(4, Format::OneVsThree { solo: Seat(2) });
        for _ in 0..SOLO_GOAL {
            step_until(&mut run, |g| matches!(g.phase, Phase::Answering { .. }));
            let r = right(&run, 2);
            lock(&mut run, &[(2, r)]);
        }
        step_until(&mut run, |g| g.phase == Phase::Finished);
        let outcome = PuzzleBuzzer::outcome(&run.game).unwrap();
        assert_eq!(outcome.winners().collect::<Vec<_>>(), vec![Seat(2)]);
    }

    #[test]
    fn the_match_ends_at_the_puzzle_limit() {
        let mut run = new_run(4, Format::FreeForAll);
        step_until(&mut run, |g| g.phase == Phase::Finished);
        assert_eq!(run.game.index + 1, MAX_PUZZLES);
        // Nobody scored, so everyone shares first.
        let outcome = PuzzleBuzzer::outcome(&run.game).unwrap();
        assert_eq!(outcome.placement, [Some(1); 4]);
    }

    /// The kinds come round in turn: every six puzzles (twelve boards in 2v2) see each kind
    /// once, and never the same kind twice running.
    #[test]
    fn the_kinds_come_round() {
        for format in [
            Format::FreeForAll,
            Format::TwoVsTwo {
                team_of: [0, 0, 1, 1],
            },
        ] {
            for seed in 0..20 {
                let mut run = Run::<PuzzleBuzzer>::new(
                    seed,
                    PlayerSetup::first(4),
                    format,
                    RunConfig::default(),
                );
                let mut kinds = Vec::new();
                while run.game.phase != Phase::Finished {
                    if matches!(run.game.phase, Phase::Showing { t: 0 }) {
                        kinds.extend(run.game.boards[..run.game.halves()].iter().map(|b| b.kind));
                    }
                    run.step(NONE);
                }
                for round in kinds.chunks(6).filter(|c| c.len() == 6) {
                    for kind in Kind::ALL {
                        assert!(round.contains(&kind), "{kinds:?}");
                    }
                }
                assert!(kinds.windows(2).all(|w| w[0] != w[1]), "{kinds:?}");
            }
        }
    }

    #[test]
    fn puzzles_get_harder() {
        let mut run = new_run(2, Format::FreeForAll);
        let mut levels = Vec::new();
        while run.game.phase != Phase::Finished {
            if matches!(run.game.phase, Phase::Showing { t: 0 }) {
                levels.push(run.game.boards[0].level);
            }
            run.step(NONE);
        }
        assert_eq!(&levels[..6], &[0, 0, 1, 1, 2, 2]);
        assert_eq!(*levels.last().unwrap(), puzzle::LEVELS - 1);
    }

    /// Better bots know more and answer sooner.
    #[test]
    fn bots_get_better_with_difficulty() {
        let stats = |difficulty| {
            let skill = bot::skill(difficulty);
            (skill.knows, skill.pace)
        };
        let (easy, normal, hard) = (
            stats(Difficulty::Easy),
            stats(Difficulty::Normal),
            stats(Difficulty::Hard),
        );
        assert!(easy.0 < normal.0 && normal.0 < hard.0);
        assert!(easy.1 > normal.1 && normal.1 > hard.1);
        // The slowest Easy bot still answers in time at the hardest puzzles.
        let skill = bot::skill(Difficulty::Easy);
        for kind in Kind::ALL {
            for seed in 0..100 {
                let board = Board::new(&mut Rng::new(seed), kind, puzzle::LEVELS - 1);
                let slowest = board.effort() * (skill.pace + skill.spread) / 100 + skill.reaction;
                assert!(slowest < ANSWER_TICKS as u32, "{kind:?}: {slowest}");
            }
        }
    }

    /// A bot that doesn't know never guesses an answer already shown wrong.
    #[test]
    fn bots_skip_answers_shown_wrong() {
        let mut run = answering(4, Format::FreeForAll);
        let (a, b) = (
            wrong_dir(&run.game, Seat(0), 0),
            wrong_dir(&run.game, Seat(0), 1),
        );
        lock(&mut run, &[(0, a), (1, b)]);
        for _ in 0..ANSWER_TICKS {
            if !matches!(run.game.phase, Phase::Answering { .. }) {
                break;
            }
            for seat in [Seat(2), Seat(3)] {
                let input = PuzzleBuzzer::bot_input(&run.game, seat, Difficulty::Easy);
                if input.held(Button::A) {
                    let dir = Dir::of_stick(&input).unwrap();
                    assert!(dir != a && dir != b);
                }
            }
            let inputs = std::array::from_fn(|i| {
                PuzzleBuzzer::bot_input(&run.game, Seat(i as u8), Difficulty::Easy)
            });
            run.step(inputs);
        }
    }
}
