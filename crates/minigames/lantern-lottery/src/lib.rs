//! Lantern Lottery: secret picks with luck. Each round four paper lanterns hang in a diamond
//! (up, right, down, left), each marked with how many fireflies it holds, and a moth hides in
//! one. Everyone points the stick at a lantern and presses A to lock in; nothing on screen says
//! which. Then the picks are shown and the lanterns open one at a time. A lantern's fireflies are
//! shared out among everyone who picked it (rounded down), and a moth's lantern pays nothing.
//! The last round is worth double. How a round is played depends on the format ([`Mode`]).

use sim_core::{
    Button, Difficulty, Format, FormatKind, Input, Inputs, MAX_PLAYERS, Minigame, Outcome,
    PlayerSetup, Rng, Seat,
};

/// The opening countdown, the same in every minigame.
pub const INTRO_TICKS: u16 = sim_core::COUNTDOWN_TICKS;
/// Rounds with three or four players, and in 1v3.
pub const ROUNDS: u8 = 5;
/// Rounds when two sides take turns hiding: three each.
pub const TURNS_ROUNDS: u8 = 6;
/// The most rounds any game has.
pub const MAX_ROUNDS: usize = TURNS_ROUNDS as usize;
/// Each round's lanterns hold four different numbers of fireflies in this range...
pub const MIN_FIREFLIES: u8 = 1;
pub const MAX_FIREFLIES: u8 = 6;
/// ...times this in the last round (the last two when sides take turns, one each).
pub const LAST_ROUND_TIMES: u8 = 2;
/// The solo in 1v3 wins with this many catches over the rounds.
pub const CATCHES_TO_WIN: u32 = 4;
/// A round opens with its lanterns' numbers on show, before anyone can pick.
pub const SHOW_TICKS: u16 = 45;
/// Time to lock in a pick.
pub const PICK_TICKS: u16 = 6 * 60;
/// Once everyone has locked in, picking ends this much later.
pub const ALL_LOCKED_TICKS: u16 = 30;
/// The reveal shows everyone's pick this long before the first lantern opens...
pub const PICKS_SHOWN_TICKS: u16 = 60;
/// ...then opens one lantern after another this far apart...
pub const OPEN_GAP_TICKS: u16 = 45;
/// ...and holds the round's result this long after the last.
pub const ROUND_END_TICKS: u16 = 90;
pub const REVEAL_TICKS: u16 = PICKS_SHOWN_TICKS + 3 * OPEN_GAP_TICKS + ROUND_END_TICKS;
/// After the last round, the result stays up this long.
pub const DECIDED_TICKS: u16 = 60;
/// A stick past this (of 127) along one axis, and further along it than the other, points at
/// that lantern.
pub const AIM: i16 = 64;

pub struct LanternLottery;

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

    /// The input a player gives to point this way.
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

    /// Where a stick points, if it's pushed far enough.
    fn of_stick(input: &Input) -> Option<Dir> {
        let (x, y) = (input.stick_x as i16, input.stick_y as i16);
        if x.abs() >= AIM && x.abs() > y.abs() {
            Some(if x > 0 { Dir::Right } else { Dir::Left })
        } else if y.abs() >= AIM && y.abs() > x.abs() {
            Some(if y > 0 { Dir::Up } else { Dir::Down })
        } else {
            None
        }
    }
}

/// How rounds are played, from the format and the number of sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Three or four players, each for themselves: the moth hides at random, and a crowded
    /// lantern pays little, so the big one isn't always best.
    Split,
    /// Two sides (two players, a duel, 2v2): with only two sides, sharing can't put anyone
    /// ahead, so the sides take turns. Each round one side hides moths (one per member) and the
    /// other picks lanterns.
    TakeTurns,
    /// 1v3: the lanterns hold no fireflies; the solo hides the moth every round and scores a
    /// catch for each of the three it finds.
    SoloHides,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Picker {
    /// False for seats outside a duel.
    pub playing: bool,
    /// Hides a moth this round instead of looking for fireflies.
    pub hiding: bool,
    /// Where the stick last pointed this round. Secret: views must not show it.
    aim: Option<Dir>,
    /// This round's lantern, once locked in. Secret until the reveal.
    pub pick: Option<Dir>,
    /// Picking ran out before this seat locked in and it pointed nowhere, so its lantern was
    /// drawn at random.
    pub drawn: bool,
    /// Fireflies won this round.
    pub gained: u32,
    /// A moth's lantern opened on this seat this round.
    pub caught: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Intro {
        t: u16,
    },
    /// The round's lanterns appear.
    Showing {
        t: u16,
    },
    /// `all_locked` counts ticks since the last seat locked in.
    Picking {
        t: u16,
        all_locked: Option<u16>,
    },
    Reveal {
        t: u16,
    },
    /// The last round is over (or 1v3 was decided early); the result stays up.
    Decided {
        t: u16,
    },
    Finished,
}

/// One finished round, kept for bots that read their rivals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Past {
    pub picks: [Option<Dir>; MAX_PLAYERS],
    pub hiding: [bool; MAX_PLAYERS],
    pub fireflies: [u8; 4],
}

#[derive(Debug, Clone, Hash)]
pub struct State {
    pub seed: u64,
    pub players: PlayerSetup,
    pub format: Format,
    pub mode: Mode,
    pub phase: Phase,
    rng: Rng,
    /// The round being played, from 0.
    pub round: u8,
    /// [`Mode::TakeTurns`]: the side (0 or 1) that hides in even rounds.
    first_hiders: u8,
    /// Fireflies in each lantern this round, by [`Dir::index`]. All 0 in 1v3.
    pub fireflies: [u8; 4],
    /// Which lanterns hold a moth this round. Where hiders pick, it means nothing until the
    /// reveal.
    pub moths: [bool; 4],
    /// The order the lanterns open in: those nobody looked in first.
    pub order: [Dir; 4],
    pub pickers: [Picker; MAX_PLAYERS],
    /// Each seat's fireflies so far; in 1v3 the solo's catches (the three score nothing).
    pub scores: [u32; MAX_PLAYERS],
    pub past: [Past; MAX_ROUNDS],
    /// Set when the game is decided.
    pub outcome: Option<Outcome>,
}

/// Ticks into a reveal at which the lantern `k`th in [`State::order`] opens.
pub fn open_tick(k: usize) -> u16 {
    PICKS_SHOWN_TICKS + k as u16 * OPEN_GAP_TICKS
}

impl State {
    pub fn picker(&self, seat: Seat) -> &Picker {
        &self.pickers[seat.index()]
    }

    /// The solo, who hides the moth, in 1v3.
    pub fn solo(&self) -> Option<Seat> {
        match self.format {
            Format::OneVsThree { solo } => Some(solo),
            _ => None,
        }
    }

    pub fn rounds(&self) -> u8 {
        match self.mode {
            Mode::TakeTurns => TURNS_ROUNDS,
            Mode::Split | Mode::SoloHides => ROUNDS,
        }
    }

    /// Whether this round's fireflies count double.
    pub fn doubled(&self) -> bool {
        let last = match self.mode {
            Mode::Split => 1,
            Mode::TakeTurns => 2,
            Mode::SoloHides => 0,
        };
        self.round + last >= self.rounds()
    }

    /// Seats in the game this round, hiders included.
    pub fn playing(&self) -> impl Iterator<Item = Seat> + '_ {
        self.players.seats().filter(|s| self.picker(*s).playing)
    }

    /// Seats looking for fireflies (or, in 1v3, hiding from the moth) this round.
    pub fn seekers(&self) -> impl Iterator<Item = Seat> + '_ {
        self.playing().filter(|s| !self.picker(*s).hiding)
    }

    pub fn hiders(&self) -> impl Iterator<Item = Seat> + '_ {
        self.playing().filter(|s| self.picker(*s).hiding)
    }

    /// Seekers who picked this lantern (only meaningful once picks are revealed).
    pub fn pickers_of(&self, dir: Dir) -> impl Iterator<Item = Seat> + '_ {
        self.seekers()
            .filter(move |s| self.picker(*s).pick == Some(dir))
    }

    /// Whether this round's picks are out in the open.
    pub fn revealed(&self) -> bool {
        matches!(
            self.phase,
            Phase::Reveal { .. } | Phase::Decided { .. } | Phase::Finished
        )
    }

    /// How many of this round's lanterns have opened, in [`State::order`].
    pub fn opened_count(&self) -> usize {
        match self.phase {
            Phase::Reveal { t } => (0..4).filter(|k| t > open_tick(*k)).count(),
            Phase::Decided { .. } | Phase::Finished => 4,
            _ => 0,
        }
    }

    pub fn is_open(&self, dir: Dir) -> bool {
        self.order[..self.opened_count()].contains(&dir)
    }

    /// Ticks left to pick, while picking.
    pub fn pick_ticks_left(&self) -> Option<u16> {
        match self.phase {
            Phase::Picking { t, .. } => Some(PICK_TICKS - t),
            _ => None,
        }
    }

    /// Catches the solo has in 1v3, and needs.
    pub fn catches(&self) -> Option<(u32, u32)> {
        self.solo()
            .map(|solo| (self.scores[solo.index()], CATCHES_TO_WIN))
    }

    /// Each team's fireflies, indexed by [`Format::team`]; in 1v3, 1 for the side that won.
    pub fn team_scores(&self) -> [u32; MAX_PLAYERS] {
        let mut scores = [0; MAX_PLAYERS];
        if let Some((catches, needed)) = self.catches() {
            scores[0] = (catches >= needed) as u32;
            scores[1] = (catches < needed) as u32;
            return scores;
        }
        for seat in self.players.seats() {
            if let Some(team) = self.format.team(seat) {
                scores[team as usize] += self.scores[seat.index()];
            }
        }
        scores
    }

    /// Which of the two sides (0 or 1) a team is, when sides take turns: the lower team is 0.
    fn side_of(&self, team: u8) -> u8 {
        let lowest = self
            .playing()
            .filter_map(|s| self.format.team(s))
            .min()
            .unwrap_or(0);
        (team != lowest) as u8
    }

    fn start_round(&mut self) {
        let hiding_side = self.first_hiders ^ (self.round % 2);
        let solo = self.solo();
        for i in 0..MAX_PLAYERS {
            let seat = Seat(i as u8);
            let playing = self.pickers[i].playing;
            let hiding = match self.mode {
                Mode::Split => false,
                Mode::SoloHides => Some(seat) == solo,
                Mode::TakeTurns => self
                    .format
                    .team(seat)
                    .is_some_and(|t| self.side_of(t) == hiding_side),
            };
            self.pickers[i] = Picker {
                playing,
                hiding: playing && hiding,
                aim: None,
                pick: None,
                drawn: false,
                gained: 0,
                caught: false,
            };
        }
        self.fireflies = if self.mode == Mode::SoloHides {
            [0; 4]
        } else {
            let times = if self.doubled() { LAST_ROUND_TIMES } else { 1 };
            let mut pool: Vec<u8> = (MIN_FIREFLIES..=MAX_FIREFLIES).collect();
            std::array::from_fn(|_| {
                let i = self.rng.range_u32(0, pool.len() as u32 - 1) as usize;
                pool.remove(i) * times
            })
        };
        self.moths = [false; 4];
        if self.mode == Mode::Split {
            self.moths[self.rng.range_u32(0, 3) as usize] = true;
        }
        self.phase = Phase::Showing { t: 0 };
    }

    /// Reads the sticks and A presses of everyone still picking. True once all have locked in.
    fn read_picks(&mut self, inputs: &Inputs) -> bool {
        let players = self.players;
        for seat in players.seats() {
            let picker = &mut self.pickers[seat.index()];
            if !picker.playing || picker.pick.is_some() {
                continue;
            }
            if let Some(dir) = Dir::of_stick(&inputs.current[seat.index()]) {
                picker.aim = Some(dir);
            }
            if inputs.just_pressed(seat, Button::A) && picker.aim.is_some() {
                picker.pick = picker.aim;
            }
        }
        self.playing().all(|s| self.picker(s).pick.is_some())
    }

    /// Picking is over: fill in late picks, put the hiders' moths in place and set the opening
    /// order.
    fn close_picks(&mut self) {
        for i in 0..MAX_PLAYERS {
            let picker = self.pickers[i];
            if !picker.playing || picker.pick.is_some() {
                continue;
            }
            let pick = match picker.aim {
                Some(dir) => dir,
                None => Dir::ALL[self.rng.range_u32(0, 3) as usize],
            };
            self.pickers[i].pick = Some(pick);
            self.pickers[i].drawn = picker.aim.is_none();
        }
        let hidden: Vec<Dir> = self.hiders().filter_map(|s| self.picker(s).pick).collect();
        for dir in hidden {
            self.moths[dir.index()] = true;
        }
        let (mut unpicked, mut picked): (Vec<Dir>, Vec<Dir>) = Dir::ALL
            .into_iter()
            .partition(|dir| self.pickers_of(*dir).next().is_none());
        shuffle(&mut self.rng, &mut unpicked);
        shuffle(&mut self.rng, &mut picked);
        for (k, dir) in unpicked.into_iter().chain(picked).enumerate() {
            self.order[k] = dir;
        }
        self.past[self.round as usize] = Past {
            picks: self.pickers.map(|p| p.pick.filter(|_| p.playing)),
            hiding: self.pickers.map(|p| p.hiding),
            fireflies: self.fireflies,
        };
        self.phase = Phase::Reveal { t: 0 };
    }

    fn open(&mut self, dir: Dir) {
        let pickers: Vec<Seat> = self.pickers_of(dir).collect();
        if self.moths[dir.index()] {
            for seat in &pickers {
                self.pickers[seat.index()].caught = true;
            }
            if let Some(solo) = self.solo() {
                self.scores[solo.index()] += pickers.len() as u32;
            }
        } else if !pickers.is_empty() {
            let share = self.fireflies[dir.index()] as u32 / pickers.len() as u32;
            for seat in &pickers {
                self.pickers[seat.index()].gained = share;
                self.scores[seat.index()] += share;
            }
        }
    }

    /// Whether the game is decided after `played` rounds.
    fn decided(&self, played: u8) -> bool {
        if played >= self.rounds() {
            return true;
        }
        let Some((catches, needed)) = self.catches() else {
            return false;
        };
        let most_to_come = (self.rounds() - played) as u32 * self.seekers().count() as u32;
        catches >= needed || catches + most_to_come < needed
    }
}

/// Fisher-Yates, from the game's own RNG.
fn shuffle(rng: &mut Rng, dirs: &mut [Dir]) {
    for i in (1..dirs.len()).rev() {
        let j = rng.range_u32(0, i as u32) as usize;
        dirs.swap(i, j);
    }
}

impl Minigame for LanternLottery {
    type State = State;

    const ID: &'static str = "lantern-lottery";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[
        FormatKind::FreeForAll,
        FormatKind::OneVsThree,
        FormatKind::TwoVsTwo,
        FormatKind::Duel,
    ];
    const MAX_TICKS: u32 = INTRO_TICKS as u32
        + TURNS_ROUNDS as u32 * (SHOW_TICKS + PICK_TICKS + REVEAL_TICKS) as u32
        + DECIDED_TICKS as u32
        + 60;

    fn new(seed: u64, players: PlayerSetup, format: Format) -> State {
        let mut pickers = [Picker {
            playing: false,
            hiding: false,
            aim: None,
            pick: None,
            drawn: false,
            gained: 0,
            caught: false,
        }; MAX_PLAYERS];
        for seat in players.seats() {
            // Seats outside a duel sit this one out.
            pickers[seat.index()].playing = format.team(seat).is_some();
        }
        let sides = pickers.iter().filter(|p| p.playing).count();
        let mode = match format {
            Format::OneVsThree { .. } => Mode::SoloHides,
            Format::TwoVsTwo { .. } | Format::Duel { .. } => Mode::TakeTurns,
            Format::FreeForAll if sides <= 2 => Mode::TakeTurns,
            Format::FreeForAll => Mode::Split,
        };
        let mut rng = Rng::new(seed);
        let first_hiders = rng.range_u32(0, 1) as u8;
        State {
            seed,
            players,
            format,
            mode,
            phase: Phase::Intro { t: 0 },
            rng,
            round: 0,
            first_hiders,
            fireflies: [0; 4],
            moths: [false; 4],
            order: Dir::ALL,
            pickers,
            scores: [0; MAX_PLAYERS],
            past: [Past {
                picks: [None; MAX_PLAYERS],
                hiding: [false; MAX_PLAYERS],
                fireflies: [0; 4],
            }; MAX_ROUNDS],
            outcome: None,
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
            Phase::Showing { t } => {
                state.phase = if t + 1 >= SHOW_TICKS {
                    Phase::Picking {
                        t: 0,
                        all_locked: None,
                    }
                } else {
                    Phase::Showing { t: t + 1 }
                };
            }
            Phase::Picking { t, all_locked } => {
                let all = state.read_picks(inputs);
                let all_locked = match all_locked {
                    Some(n) => Some(n + 1),
                    None if all => Some(0),
                    None => None,
                };
                if t + 1 >= PICK_TICKS || all_locked.is_some_and(|n| n >= ALL_LOCKED_TICKS) {
                    state.close_picks();
                } else {
                    state.phase = Phase::Picking {
                        t: t + 1,
                        all_locked,
                    };
                }
            }
            Phase::Reveal { t } => {
                if let Some(k) = (0..4).find(|k| open_tick(*k) == t) {
                    state.open(state.order[k]);
                }
                if t + 1 < REVEAL_TICKS {
                    state.phase = Phase::Reveal { t: t + 1 };
                } else if state.decided(state.round + 1) {
                    state.outcome = Some(Outcome::from_team_scores(
                        &state.players,
                        &state.format,
                        &state.team_scores(),
                    ));
                    state.phase = Phase::Decided { t: 0 };
                } else {
                    state.round += 1;
                    state.start_round();
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
        let Phase::Picking { t, .. } = state.phase else {
            return Input::default();
        };
        let picker = state.picker(seat);
        if !state.players.is_active(seat) || !picker.playing || picker.pick.is_some() {
            return Input::default();
        }
        // One roll per round, so the bot stays a pure function of the state.
        let mut rng = Rng::new(
            state.seed
                ^ (seat.0 as u64 + 1).wrapping_mul(0xd6e8_feb8_6659_fd93)
                ^ (state.round as u64 + 1).wrapping_mul(0xbf58_476d_1ce4_e5b9),
        );
        // Bots take their time, as people do, so locking in early says nothing.
        let (soonest, latest) = match difficulty {
            Difficulty::Easy => (50, 260),
            Difficulty::Normal => (40, 220),
            Difficulty::Hard => (30, 200),
        };
        if (t as u32) < rng.range_u32(soonest, latest) {
            return Input::default();
        }
        let weights = match (state.mode, picker.hiding) {
            (Mode::Split, _) => bots::split(state, difficulty),
            (Mode::TakeTurns, false) => bots::seek(state, difficulty),
            (Mode::TakeTurns, true) => bots::hide(state, difficulty),
            (Mode::SoloHides, true) => bots::solo_hide(state, difficulty),
            (Mode::SoloHides, false) => bots::solo_flee(state, difficulty),
        };
        bots::choose(&mut rng, weights).stick().with(Button::A)
    }
}

/// How bots weigh the lanterns. Every bot picks at random by these weights, so none can be read.
/// Easy goes for the big lanterns; Normal plays the balanced mix (the one nobody can beat by
/// reading it); Hard starts from the balanced mix and leans towards what beats the other
/// players' habits so far.
mod bots {
    use super::*;

    /// How strongly Hard leans from the balanced mix towards what its rivals' habits leave open.
    const LEAN: u32 = 4;
    /// How many rounds of balanced play Hard assumes of a rival before it has seen any, so a
    /// round or two of picks doesn't sway it much.
    const PRIOR_ROUNDS: f64 = 2.0;

    fn power(x: f64, n: u32) -> f64 {
        (0..n).fold(1.0, |acc, _| acc * x)
    }

    pub fn choose(rng: &mut Rng, weights: [f64; 4]) -> Dir {
        let top = weights.iter().fold(0.0f64, |a, b| a.max(*b));
        let ints = weights.map(|w| {
            if top > 0.0 {
                (w / top * 10_000.0) as u32 + 1
            } else {
                1
            }
        });
        let mut roll = rng.range_u32(0, ints.iter().sum::<u32>() - 1);
        for dir in Dir::ALL {
            if roll < ints[dir.index()] {
                return dir;
            }
            roll -= ints[dir.index()];
        }
        Dir::Left
    }

    fn greedy(fireflies: &[u8; 4]) -> [f64; 4] {
        fireflies.map(|f| power(f as f64, 3))
    }

    fn normalized(odds: [f64; 4]) -> [f64; 4] {
        let total: f64 = odds.iter().sum();
        if total > 0.0 {
            odds.map(|o| o / total)
        } else {
            [0.25; 4]
        }
    }

    /// Rank of each lantern by its fireflies (0 = most). Every round's numbers are different.
    fn ranks(fireflies: &[u8; 4]) -> [usize; 4] {
        std::array::from_fn(|i| fireflies.iter().filter(|f| **f > fireflies[i]).count())
    }

    /// The chance that 0, 1, 2 or 3 rivals pick a lantern, given each one's chance of picking it.
    fn sharing(chances: impl Iterator<Item = f64>) -> [f64; 4] {
        let mut sharing = [1.0, 0.0, 0.0, 0.0];
        for p in chances {
            for k in (0..4).rev() {
                let from_below = if k > 0 { sharing[k - 1] * p } else { 0.0 };
                sharing[k] = sharing[k] * (1.0 - p) + from_below;
            }
        }
        sharing
    }

    /// What a lantern of `fireflies` pays on average, shared as `sharing` says (the moth aside:
    /// it's equally likely anywhere).
    fn pays(fireflies: u8, sharing: &[f64; 4]) -> f64 {
        (0..4)
            .map(|k| sharing[k] * (fireflies as u32 / (k as u32 + 1)) as f64)
            .sum()
    }

    /// [`Mode::Split`]'s balanced mix: how often to pick each lantern so that, when all
    /// `seekers` pick this way, every lantern anyone picks pays the same on average. Nobody
    /// gains by straying from it alone, so it can't be read. Found by bisection on what a lantern pays.
    pub fn balanced(fireflies: &[u8; 4], seekers: usize) -> [f64; 4] {
        let rivals = seekers.saturating_sub(1);
        let pays_at = |f: u8, q: f64| pays(f, &sharing((0..rivals).map(|_| q)));
        // How often to pick a lantern of `f` so it pays `level`.
        let odds_for = |f: u8, level: f64| {
            if pays_at(f, 0.0) <= level {
                return 0.0;
            }
            if pays_at(f, 1.0) >= level {
                return 1.0;
            }
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..30 {
                let mid = (lo + hi) / 2.0;
                if pays_at(f, mid) > level {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            (lo + hi) / 2.0
        };
        let (mut lo, mut hi) = (0.0, MAX_FIREFLIES as f64 * LAST_ROUND_TIMES as f64);
        for _ in 0..40 {
            let mid = (lo + hi) / 2.0;
            let total: f64 = fireflies.iter().map(|f| odds_for(*f, mid)).sum();
            if total > 1.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let odds = fireflies.map(|f| odds_for(f, (lo + hi) / 2.0));
        if odds.iter().sum::<f64>() > 0.0 {
            normalized(odds)
        } else {
            normalized(greedy(fireflies))
        }
    }

    /// [`Mode::TakeTurns`]'s balanced mixes, for a seeker and a hider: the hider hides the moth
    /// so that every lantern the seeker might pick pays the same on average, and the seeker
    /// picks each of those lanterns in inverse proportion to its fireflies, so the hider gains
    /// nothing by favouring one. Only the biggest few lanterns are worth looking in.
    pub fn guessing(fireflies: &[u8; 4]) -> ([f64; 4], [f64; 4]) {
        let mut by_size = Dir::ALL.map(|d| d.index());
        by_size.sort_by_key(|i| std::cmp::Reverse(fireflies[*i]));
        let f = |k: usize| fireflies[by_size[k]] as f64;
        // The hidden moth makes every lantern in the top `k` pay `level`.
        for k in 2..=4 {
            let inverse: f64 = (0..k).map(|i| 1.0 / f(i)).sum();
            let level = (k - 1) as f64 / inverse;
            if f(k - 1) >= level && (k == 4 || f(k) <= level) {
                let mut seek = [0.0; 4];
                let mut hide = [0.0; 4];
                for i in 0..k {
                    seek[by_size[i]] = 1.0 / f(i) / inverse;
                    hide[by_size[i]] = 1.0 - level / f(i);
                }
                return (seek, hide);
            }
        }
        ([0.25; 4], [0.25; 4])
    }

    /// How likely `seat` is to pick each lantern this round, if it plays as before: the
    /// balanced mix `now`, tilted by how much more (or less) often its past picks in the same
    /// role went to the biggest lantern, the second biggest and so on than the balanced mix
    /// would have. `then` is the balanced mix for a past round, or `None` for rounds in the other
    /// role. A seat playing the balanced mix isn't tilted, so Hard bots don't chase each other's
    /// noise.
    fn habits(
        state: &State,
        seat: Seat,
        now: &[f64; 4],
        then: impl Fn(&Past) -> Option<[f64; 4]>,
    ) -> [f64; 4] {
        let mut seen = [PRIOR_ROUNDS / 4.0; 4];
        let mut expected = [PRIOR_ROUNDS / 4.0; 4];
        for past in &state.past[..state.round as usize] {
            let (Some(dir), Some(mix)) = (past.picks[seat.index()], then(past)) else {
                continue;
            };
            let rank = ranks(&past.fireflies);
            seen[rank[dir.index()]] += 1.0;
            for (i, odds) in mix.iter().enumerate() {
                expected[rank[i]] += odds;
            }
        }
        let rank = ranks(&state.fireflies);
        normalized(std::array::from_fn(|i| {
            now[i] * seen[rank[i]] / expected[rank[i]]
        }))
    }

    /// Hard's lean: the balanced mix, moved towards lanterns that do better than it would
    /// (`value` against what the balanced mix itself averages).
    fn lean(balanced: &[f64; 4], value: [f64; 4]) -> [f64; 4] {
        let average: f64 = (0..4).map(|i| balanced[i] * value[i]).sum();
        if average <= 0.0 {
            return *balanced;
        }
        std::array::from_fn(|i| (balanced[i] + 0.02) * power(value[i] / average, LEAN))
    }

    /// Free-for-all. Hard plays the balanced mix too: with three or four players, leaving a
    /// greedy rival alone on the big lantern scores well on average but lets them win, and Hard
    /// bots reading each other herded onto the same lanterns.
    pub fn split(state: &State, difficulty: Difficulty) -> [f64; 4] {
        match difficulty {
            Difficulty::Easy => greedy(&state.fireflies),
            Difficulty::Normal | Difficulty::Hard => {
                balanced(&state.fireflies, state.seekers().count())
            }
        }
    }

    /// Two sides, looking for fireflies: Hard keeps away from where the hiders have tended to
    /// hide moths.
    pub fn seek(state: &State, difficulty: Difficulty) -> [f64; 4] {
        let (seek, hide) = guessing(&state.fireflies);
        match difficulty {
            Difficulty::Easy => greedy(&state.fireflies),
            Difficulty::Normal => seek,
            Difficulty::Hard => {
                let mut clear = [1.0; 4];
                for hider in state.hiders() {
                    let odds = habits(state, hider, &hide, |past| {
                        past.hiding[hider.index()].then(|| guessing(&past.fireflies).1)
                    });
                    for i in 0..4 {
                        clear[i] *= 1.0 - odds[i];
                    }
                }
                lean(
                    &seek,
                    std::array::from_fn(|i| state.fireflies[i] as f64 * clear[i]),
                )
            }
        }
    }

    /// Two sides, hiding moths: Easy hides them in the big lanterns; Hard where the seekers
    /// have tended to look.
    pub fn hide(state: &State, difficulty: Difficulty) -> [f64; 4] {
        let (seek, hide) = guessing(&state.fireflies);
        match difficulty {
            Difficulty::Easy => greedy(&state.fireflies),
            Difficulty::Normal => hide,
            Difficulty::Hard => {
                let mut denied = [0.0; 4];
                for seeker in state.seekers() {
                    let odds = habits(state, seeker, &seek, |past| {
                        (!past.hiding[seeker.index()]).then(|| guessing(&past.fireflies).0)
                    });
                    for i in 0..4 {
                        denied[i] += odds[i] * state.fireflies[i] as f64;
                    }
                }
                lean(&hide, denied)
            }
        }
    }

    /// How often `seat` has picked each lantern, plus a little for every lantern, as odds.
    fn favourites(state: &State, seat: Seat) -> [f64; 4] {
        const PRIOR: f64 = 1.5;
        let mut counts = [PRIOR; 4];
        for past in &state.past[..state.round as usize] {
            if let Some(dir) = past.picks[seat.index()] {
                counts[dir.index()] += 1.0;
            }
        }
        normalized(counts)
    }

    /// 1v3, the solo: Hard hides the moth where the three have tended to go.
    pub fn solo_hide(state: &State, difficulty: Difficulty) -> [f64; 4] {
        if difficulty != Difficulty::Hard {
            return [1.0; 4];
        }
        let mut expected = [0.0; 4];
        for seeker in state.seekers() {
            let odds = favourites(state, seeker);
            for i in 0..4 {
                expected[i] += odds[i];
            }
        }
        lean(&[0.25; 4], expected)
    }

    /// 1v3, the three: Hard keeps away from where the solo has tended to hide the moth.
    pub fn solo_flee(state: &State, difficulty: Difficulty) -> [f64; 4] {
        let Some(solo) = state.solo().filter(|_| difficulty == Difficulty::Hard) else {
            return [1.0; 4];
        };
        lean(&[0.25; 4], favourites(state, solo).map(|p| 1.0 - p))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_balanced_mix_makes_every_lantern_picked_pay_the_same() {
            for fireflies in [[1, 3, 4, 6], [2, 3, 5, 6], [12, 2, 8, 10]] {
                for seekers in [3, 4] {
                    let mix = balanced(&fireflies, seekers);
                    assert!((mix.iter().sum::<f64>() - 1.0).abs() < 1e-9);
                    let paid: Vec<f64> = (0..4)
                        .filter(|i| mix[*i] > 0.001)
                        .map(|i| pays(fireflies[i], &sharing((1..seekers).map(|_| mix[i]))))
                        .collect();
                    let spread = paid.iter().fold(0.0f64, |a, b| a.max(*b))
                        - paid.iter().fold(f64::MAX, |a, b| a.min(*b));
                    assert!(
                        paid.len() >= 2 && spread < 0.01,
                        "{fireflies:?} {mix:?} {paid:?}"
                    );
                }
            }
        }

        #[test]
        fn hiding_and_seeking_balance_each_other() {
            let fireflies = [6, 5, 3, 2];
            let (seek, hide) = guessing(&fireflies);
            // The seeker looks in the small lanterns more often, and never in the smallest.
            assert!(seek[2] > seek[1] && seek[1] > seek[0] && seek[3] == 0.0);
            // Wherever the seeker looks, it gets the same on average.
            let paid: Vec<f64> = (0..3)
                .map(|i| fireflies[i] as f64 * (1.0 - hide[i]))
                .collect();
            assert!(paid.iter().all(|p| (p - paid[0]).abs() < 1e-9), "{paid:?}");
            assert!((hide.iter().sum::<f64>() - 1.0).abs() < 1e-9);
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

    fn new_run(players: usize, format: Format) -> Run<LanternLottery> {
        Run::new(7, PlayerSetup::first(players), format, RunConfig::default())
    }

    fn step_until(run: &mut Run<LanternLottery>, done: impl Fn(&State) -> bool) {
        for _ in 0..LanternLottery::MAX_TICKS {
            if done(&run.game) {
                return;
            }
            run.step(NONE);
        }
        panic!("never reached");
    }

    fn picking(game: &State) -> bool {
        matches!(game.phase, Phase::Picking { .. })
    }

    fn revealing(game: &State) -> bool {
        matches!(game.phase, Phase::Reveal { .. })
    }

    /// Lock in a pick for each listed seat: point, press A, let go.
    fn pick(run: &mut Run<LanternLottery>, picks: &[(usize, Dir)]) {
        let mut inputs = NONE;
        for (seat, dir) in picks {
            inputs[*seat] = dir.stick().with(Button::A);
        }
        run.step(inputs);
        run.step(NONE);
    }

    /// Play a round to its end, with these picks.
    fn play_round(run: &mut Run<LanternLottery>, picks: &[(usize, Dir)]) {
        step_until(run, picking);
        pick(run, picks);
        step_until(run, revealing);
        step_until(run, |g| !revealing(g));
    }

    fn moth(game: &State) -> Dir {
        Dir::ALL
            .into_iter()
            .find(|d| game.moths[d.index()])
            .unwrap()
    }

    /// The biggest lantern without a moth.
    fn best(game: &State) -> Dir {
        Dir::ALL
            .into_iter()
            .filter(|d| !game.moths[d.index()])
            .max_by_key(|d| game.fireflies[d.index()])
            .unwrap()
    }

    #[test]
    fn modes_follow_the_format_and_the_number_of_sides() {
        let mode =
            |players, format| LanternLottery::new(1, PlayerSetup::first(players), format).mode;
        assert_eq!(mode(4, Format::FreeForAll), Mode::Split);
        assert_eq!(mode(3, Format::FreeForAll), Mode::Split);
        assert_eq!(mode(2, Format::FreeForAll), Mode::TakeTurns);
        assert_eq!(
            mode(
                4,
                Format::Duel {
                    a: Seat(0),
                    b: Seat(2)
                }
            ),
            Mode::TakeTurns
        );
        let teams = Format::TwoVsTwo {
            team_of: [0, 1, 1, 0],
        };
        assert_eq!(mode(4, teams), Mode::TakeTurns);
        assert_eq!(
            mode(4, Format::OneVsThree { solo: Seat(1) }),
            Mode::SoloHides
        );
    }

    #[test]
    fn a_shared_lantern_splits_its_fireflies() {
        let mut run = new_run(3, Format::FreeForAll);
        step_until(&mut run, picking);
        let big = best(&run.game);
        let fireflies = run.game.fireflies[big.index()] as u32;
        let moth = moth(&run.game);
        let other = Dir::ALL
            .into_iter()
            .find(|d| *d != big && *d != moth)
            .unwrap();
        pick(&mut run, &[(0, big), (1, big), (2, other)]);
        step_until(&mut run, |g| g.opened_count() == 4);
        assert_eq!(run.game.scores[0], fireflies / 2);
        assert_eq!(run.game.scores[1], fireflies / 2);
        assert_eq!(run.game.scores[2], run.game.fireflies[other.index()] as u32);
    }

    #[test]
    fn the_moths_lantern_pays_nothing() {
        let mut run = new_run(3, Format::FreeForAll);
        step_until(&mut run, picking);
        assert_eq!(run.game.moths.iter().filter(|m| **m).count(), 1);
        let moth = moth(&run.game);
        pick(&mut run, &[(0, moth), (1, moth), (2, moth)]);
        step_until(&mut run, |g| g.opened_count() == 4);
        assert_eq!(run.game.scores, [0; MAX_PLAYERS]);
        assert!(run.game.pickers[..3].iter().all(|p| p.caught));
    }

    #[test]
    fn a_needs_the_stick_pointing() {
        let mut run = new_run(2, Format::FreeForAll);
        step_until(&mut run, picking);
        let mut inputs = NONE;
        inputs[0] = Input::default().with(Button::A);
        run.step(inputs);
        assert_eq!(run.game.pickers[0].pick, None);
        // Pointing, letting go, then pressing A picks where the stick last pointed.
        inputs[0] = Dir::Left.stick();
        run.step(inputs);
        run.step(NONE);
        inputs[0] = Input::default().with(Button::A);
        run.step(inputs);
        assert_eq!(run.game.pickers[0].pick, Some(Dir::Left));
        // A pick is final.
        pick(&mut run, &[(0, Dir::Up)]);
        assert_eq!(run.game.pickers[0].pick, Some(Dir::Left));
    }

    #[test]
    fn presses_before_picking_dont_count() {
        let mut run = new_run(2, Format::FreeForAll);
        step_until(&mut run, |g| matches!(g.phase, Phase::Showing { .. }));
        pick(&mut run, &[(0, Dir::Up)]);
        step_until(&mut run, picking);
        assert_eq!(run.game.pickers[0].pick, None);
    }

    #[test]
    fn time_runs_out_on_the_lantern_pointed_at_or_a_random_one() {
        let mut run = new_run(2, Format::FreeForAll);
        step_until(&mut run, picking);
        let mut inputs = NONE;
        inputs[0] = Dir::Down.stick();
        run.step(inputs);
        step_until(&mut run, revealing);
        assert_eq!(run.game.pickers[0].pick, Some(Dir::Down));
        assert!(!run.game.pickers[0].drawn);
        assert!(run.game.pickers[1].pick.is_some());
        assert!(run.game.pickers[1].drawn);
    }

    #[test]
    fn picking_ends_soon_after_everyone_locks_in() {
        let mut run = new_run(2, Format::FreeForAll);
        step_until(&mut run, picking);
        pick(&mut run, &[(0, Dir::Up), (1, Dir::Down)]);
        for _ in 0..ALL_LOCKED_TICKS {
            run.step(NONE);
        }
        assert!(revealing(&run.game));
    }

    #[test]
    fn unpicked_lanterns_open_first() {
        let mut run = new_run(3, Format::FreeForAll);
        step_until(&mut run, picking);
        pick(&mut run, &[(0, Dir::Up), (1, Dir::Right), (2, Dir::Up)]);
        step_until(&mut run, revealing);
        let last_two = &run.game.order[2..];
        assert!(last_two.contains(&Dir::Up) && last_two.contains(&Dir::Right));
    }

    #[test]
    fn lanterns_hold_different_numbers_and_the_last_rounds_double() {
        for (players, doubled) in [(3, 1), (2, 2)] {
            for seed in 0..30 {
                let mut run: Run<LanternLottery> = Run::new(
                    seed,
                    PlayerSetup::first(players),
                    Format::FreeForAll,
                    RunConfig::default(),
                );
                let rounds = run.game.rounds();
                for round in 0..rounds {
                    step_until(&mut run, picking);
                    let fireflies = run.game.fireflies;
                    let times = if round + doubled >= rounds { 2 } else { 1 };
                    assert_eq!(run.game.doubled(), times == 2);
                    for (i, f) in fireflies.iter().enumerate() {
                        assert!((MIN_FIREFLIES * times..=MAX_FIREFLIES * times).contains(f));
                        assert_eq!(f % times, 0);
                        assert!(!fireflies[..i].contains(f));
                    }
                    step_until(&mut run, revealing);
                }
            }
        }
    }

    #[test]
    fn most_fireflies_after_the_last_round_wins() {
        let mut run = new_run(3, Format::FreeForAll);
        for _ in 0..ROUNDS {
            step_until(&mut run, picking);
            // Seat 1 takes the biggest lantern without the moth, alone.
            let (moth, best) = (moth(&run.game), best(&run.game));
            play_round(&mut run, &[(0, moth), (1, best), (2, moth)]);
        }
        step_until(&mut run, |g| g.phase == Phase::Finished);
        assert_eq!(run.game.round, ROUNDS - 1);
        let outcome = run.outcome().unwrap();
        assert_eq!(outcome.placement, [Some(2), Some(1), Some(2), None]);
    }

    #[test]
    fn two_sides_take_turns_hiding() {
        let mut run = new_run(2, Format::FreeForAll);
        step_until(&mut run, picking);
        let hider = (0..2).find(|i| run.game.pickers[*i].hiding).unwrap();
        let seeker = 1 - hider;
        assert_eq!(run.game.moths, [false; 4], "the moth waits for its hider");
        assert_eq!(run.game.rounds(), TURNS_ROUNDS);
        // The seeker finds the moth the hider hid.
        pick(&mut run, &[(hider, Dir::Left), (seeker, Dir::Left)]);
        step_until(&mut run, |g| g.opened_count() == 4);
        assert!(run.game.moths[Dir::Left.index()] && run.game.pickers[seeker].caught);
        assert_eq!(run.game.scores, [0; MAX_PLAYERS]);
        // Next round the roles swap, and a lantern without the moth pays.
        step_until(&mut run, picking);
        assert!(run.game.pickers[seeker].hiding && !run.game.pickers[hider].hiding);
        let fireflies = run.game.fireflies[Dir::Up.index()] as u32;
        pick(&mut run, &[(seeker, Dir::Down), (hider, Dir::Up)]);
        step_until(&mut run, |g| g.opened_count() == 4);
        assert_eq!(run.game.scores[hider], fireflies);
        assert_eq!(run.game.scores[seeker], 0);
    }

    #[test]
    fn in_two_v_two_each_hider_hides_a_moth_and_teams_add_up() {
        let format = Format::TwoVsTwo {
            team_of: [0, 1, 0, 1],
        };
        let mut run = new_run(4, format);
        step_until(&mut run, picking);
        let hiding: Vec<usize> = (0..4).filter(|i| run.game.pickers[*i].hiding).collect();
        assert_eq!(hiding.len(), 2);
        assert!(format.same_team(Seat(hiding[0] as u8), Seat(hiding[1] as u8)));
        let seekers: Vec<usize> = (0..4).filter(|i| !hiding.contains(i)).collect();
        pick(
            &mut run,
            &[
                (hiding[0], Dir::Up),
                (hiding[1], Dir::Right),
                (seekers[0], Dir::Down),
                (seekers[1], Dir::Right),
            ],
        );
        step_until(&mut run, |g| g.opened_count() == 4);
        assert_eq!(run.game.moths, [true, true, false, false]);
        let team = format.team(Seat(seekers[0] as u8)).unwrap() as usize;
        assert_eq!(
            run.game.team_scores()[team],
            run.game.fireflies[Dir::Down.index()] as u32
        );
    }

    #[test]
    fn the_solo_hides_the_moth_and_scores_its_catches() {
        let mut run = new_run(4, Format::OneVsThree { solo: Seat(0) });
        step_until(&mut run, picking);
        assert_eq!(run.game.fireflies, [0; 4]);
        pick(
            &mut run,
            &[
                (0, Dir::Right),
                (1, Dir::Right),
                (2, Dir::Right),
                (3, Dir::Up),
            ],
        );
        step_until(&mut run, revealing);
        assert_eq!(run.game.moths, [false, true, false, false]);
        // The solo's lantern isn't a pick: only the three's lanterns open last.
        assert!(
            run.game.order[..2]
                .iter()
                .all(|d| *d != Dir::Right && *d != Dir::Up)
        );
        step_until(&mut run, |g| g.opened_count() == 4);
        assert_eq!(run.game.catches(), Some((2, CATCHES_TO_WIN)));
        assert!(run.game.pickers[1].caught && !run.game.pickers[3].caught);
    }

    #[test]
    fn one_v_three_ends_once_decided() {
        let mut run = new_run(4, Format::OneVsThree { solo: Seat(2) });
        // Three caught, then two more: four or more catches wins in round two.
        play_round(
            &mut run,
            &[(2, Dir::Up), (0, Dir::Up), (1, Dir::Up), (3, Dir::Up)],
        );
        assert_eq!(run.game.round, 1);
        play_round(
            &mut run,
            &[
                (2, Dir::Down),
                (0, Dir::Down),
                (1, Dir::Left),
                (3, Dir::Left),
            ],
        );
        assert!(matches!(run.game.phase, Phase::Decided { .. }));
        step_until(&mut run, |g| g.phase == Phase::Finished);
        assert_eq!(
            run.outcome().unwrap().winners().collect::<Vec<_>>(),
            vec![Seat(2)]
        );

        // Never catching anyone: once 4 can't be reached, the three win.
        let mut run = new_run(4, Format::OneVsThree { solo: Seat(0) });
        let mut rounds = 0;
        while run.game.outcome.is_none() {
            play_round(
                &mut run,
                &[(0, Dir::Up), (1, Dir::Down), (2, Dir::Down), (3, Dir::Down)],
            );
            rounds += 1;
        }
        // After 4 empty rounds, one round of 3 can't make 4.
        assert_eq!(rounds, 4);
        assert_eq!(
            run.game.outcome.unwrap().winners().collect::<Vec<_>>(),
            vec![Seat(1), Seat(2), Seat(3)]
        );
    }

    #[test]
    fn seats_outside_a_duel_sit_out() {
        let mut run = new_run(
            4,
            Format::Duel {
                a: Seat(1),
                b: Seat(3),
            },
        );
        assert!(!run.game.pickers[0].playing && !run.game.pickers[2].playing);
        step_until(&mut run, picking);
        assert_eq!(run.game.hiders().count(), 1);
        pick(&mut run, &[(0, Dir::Up), (1, Dir::Up), (3, Dir::Down)]);
        assert_eq!(run.game.pickers[0].pick, None);
        for _ in 0..ALL_LOCKED_TICKS {
            run.step(NONE);
        }
        assert!(revealing(&run.game));
    }

    #[test]
    fn bots_take_their_time_to_lock_in() {
        let mut run = new_run(4, Format::FreeForAll);
        step_until(&mut run, picking);
        let inputs = Seat::ALL.map(|s| LanternLottery::bot_input(&run.game, s, Difficulty::Hard));
        assert_eq!(inputs, NONE);
    }

    /// How often seat 0 at `p1` wins (ties included) against `others`.
    fn win_rate(players: usize, p1: Difficulty, others: Difficulty) -> f64 {
        let runs = 300;
        let mut wins = 0;
        for seed in 0..runs {
            let mut run: Run<LanternLottery> = Run::new(
                seed,
                PlayerSetup::first(players),
                Format::FreeForAll,
                RunConfig::default(),
            );
            while run.outcome().is_none() {
                let inputs = Seat::ALL.map(|s| {
                    let level = if s == Seat(0) { p1 } else { others };
                    LanternLottery::bot_input(&run.game, s, level)
                });
                run.step(inputs);
            }
            wins += (run.outcome().unwrap().placement[0] == Some(1)) as u32;
        }
        wins as f64 / runs as f64
    }

    #[test]
    fn hard_bots_beat_easy_ones() {
        let duel = win_rate(2, Difficulty::Hard, Difficulty::Easy);
        assert!(duel > 0.6, "{duel}");
        let four = win_rate(4, Difficulty::Hard, Difficulty::Easy);
        assert!(four > 0.3, "{four}");
    }

    #[test]
    fn passes_all_automated_quality_bars() {
        sim_check::check_all::<LanternLottery>(8).unwrap();
    }
}
