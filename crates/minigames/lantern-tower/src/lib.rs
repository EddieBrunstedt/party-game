//! Lantern Tower: a stacking race with sabotage. Every team has a tower. A block slides back and
//! forth above its top and A drops it; whatever overhangs the floor below is cut off, so the next
//! block is narrower. A perfect drop cuts nothing, widens the tower back a little and charges a
//! gust; a charged gust is B away from making the tallest rival tower's blocks slide faster. A
//! block that misses knocks the top floor off. First to [`GOAL_FLOORS`] wins; at the time limit
//! the tallest tower does.
//!
//! In team formats the team shares one tower and its members drop in turn. Positions are whole
//! milli-units, so the simulation has no floats: the base is [`BLOCK_WIDTH`] wide, centred on 0.

use sim_core::{
    Button, Difficulty, Format, FormatKind, Input, Inputs, MAX_PLAYERS, Minigame, Outcome,
    PlayerSetup, Rng, Seat,
};

/// The opening countdown, the same in every minigame.
pub const INTRO_TICKS: u16 = sim_core::COUNTDOWN_TICKS;
/// Ticks of building before time runs out.
pub const RACE_TICKS: u32 = 60 * 60;
/// Floors on top of the base to win.
pub const GOAL_FLOORS: u8 = 20;
/// The base's width, and the first block's.
pub const BLOCK_WIDTH: i32 = 1000;
/// A sliding block's centre swings this far either side of the base's centre.
pub const SWING: i32 = 1000;
/// A drop this close to the floor below (edge to edge) is perfect: nothing is cut.
pub const PERFECT: i32 = 40;
/// A perfect drop also widens the tower back by this much (up to [`BLOCK_WIDTH`]).
pub const GROW: i32 = 100;
/// A block that would be left narrower than this tips over: a miss.
pub const SLIVER: i32 = 100;
/// How far a block slides per tick on the bare base, how much faster per floor, and the most.
pub const BASE_SPEED: i32 = 16;
pub const SPEED_PER_FLOOR: i32 = 1;
pub const MAX_SPEED: i32 = 36;
/// A gusted block slides this much faster (numerator over denominator).
pub const GUST_SPEEDUP: (i32, i32) = (3, 2);
/// A dropped block takes this long to land.
pub const FALL_TICKS: u16 = 12;
/// After a block lands, the next one slides in this much later...
pub const SETTLE_TICKS: u16 = 8;
/// ...or this much later after a miss.
pub const KNOCK_TICKS: u16 = 30;
/// Perfect drops it takes to charge a gust. A team holds one gust at most.
pub const GUST_CHARGE: u8 = 3;
/// A gust makes a rival's next this many blocks (the sliding one included) fast...
pub const GUST_BLOCKS: u8 = 2;
/// ...and gusts stack up to this many.
pub const MAX_GUSTED: u8 = 4;
/// After the result is decided, the game holds this long so the last drop plays out.
pub const DECIDED_TICKS: u16 = 90;

/// Floors a tower can hold: the base and the goal.
const FLOORS: usize = GOAL_FLOORS as usize + 1;

pub struct LanternTower;

/// One floor of a tower, from its left edge to its right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Floor {
    pub lo: i32,
    pub hi: i32,
    /// Who placed it (the base: the tower's first member).
    pub seat: Seat,
}

impl Floor {
    pub fn width(&self) -> i32 {
        self.hi - self.lo
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Block {
    /// Sliding above the top, its left edge at `lo`.
    Sliding { lo: i32, right: bool },
    /// Dropped at `lo`; lands when `t` reaches [`FALL_TICKS`].
    Falling { lo: i32, t: u16 },
    /// The next block slides in when `t` reaches `wait`.
    Resting { t: u16, wait: u16 },
}

/// What the last block did when it landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Landing {
    Perfect,
    /// Placed, and the overhang from `lo` to `hi` was cut off.
    Cut {
        lo: i32,
        hi: i32,
    },
    /// Missed: the block (`lo` to `hi`) tumbled and knocked the top floor off, unless the tower
    /// was bare.
    Miss {
        lo: i32,
        hi: i32,
        knocked: Option<Floor>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tower {
    /// Who builds it, in turn order; the first `member_count`. No members: no tower.
    pub members: [Seat; MAX_PLAYERS],
    pub member_count: u8,
    /// Index into `members` of whoever drops the current block.
    pub turn: u8,
    /// `floors[0]` is the base; the top is `floors[height]`.
    pub floors: [Floor; FLOORS],
    pub height: u8,
    pub block: Block,
    /// Blocks dropped so far (the next one slides in from the left when even).
    pub drops: u16,
    /// Perfect drops towards the next gust; at [`GUST_CHARGE`] it's ready to blow.
    pub charge: u8,
    /// How many more blocks (the sliding one included) slide fast.
    pub gusted: u8,
    /// The last landing and the tick it happened on.
    pub landing: Option<(u32, Landing)>,
    /// The tick a gust last hit this tower.
    pub gust_hit: Option<u32>,
    pub perfects: u16,
    pub misses: u16,
}

impl Tower {
    pub fn is_built(&self) -> bool {
        self.member_count > 0
    }

    pub fn members(&self) -> &[Seat] {
        &self.members[..self.member_count as usize]
    }

    /// Whoever drops the current block.
    pub fn builder(&self) -> Seat {
        self.members[self.turn as usize]
    }

    pub fn top(&self) -> Floor {
        self.floors[self.height as usize]
    }

    /// How far the current block slides per tick.
    pub fn speed(&self) -> i32 {
        let speed = (BASE_SPEED + SPEED_PER_FLOOR * self.height as i32).min(MAX_SPEED);
        if self.gusted > 0 {
            speed * GUST_SPEEDUP.0 / GUST_SPEEDUP.1
        } else {
            speed
        }
    }

    /// Where a block's left edge turns back: its centre is [`SWING`] either side of 0.
    fn swing(&self) -> (i32, i32) {
        let half = self.top().width() / 2;
        (-SWING - half, SWING - half)
    }

    /// A new block, sliding in from alternating sides.
    fn next_block(&self) -> Block {
        let (left, right) = self.swing();
        if self.drops.is_multiple_of(2) {
            Block::Sliding {
                lo: left,
                right: true,
            }
        } else {
            Block::Sliding {
                lo: right,
                right: false,
            }
        }
    }

    /// Ranks towers: height first, then how wide the top is.
    fn score(&self) -> u32 {
        self.height as u32 * 10_000 + self.top().width() as u32
    }

    fn slide(&mut self) {
        let Block::Sliding { lo, right } = self.block else {
            return;
        };
        let (min, max) = self.swing();
        let speed = self.speed();
        let (lo, right) = if right {
            let lo = lo + speed;
            if lo > max {
                (2 * max - lo, false)
            } else {
                (lo, true)
            }
        } else {
            let lo = lo - speed;
            if lo < min {
                (2 * min - lo, true)
            } else {
                (lo, false)
            }
        };
        self.block = Block::Sliding { lo, right };
    }

    fn land(&mut self, lo: i32, tick: u32) {
        let top = self.top();
        let hi = lo + top.width();
        let builder = self.builder();
        let (landing, wait) = if (lo - top.lo).abs() <= PERFECT {
            self.height += 1;
            let width = (top.width() + GROW).min(BLOCK_WIDTH);
            let lo = top.lo - (width - top.width()) / 2;
            self.floors[self.height as usize] = Floor {
                lo,
                hi: lo + width,
                seat: builder,
            };
            self.perfects += 1;
            self.charge = (self.charge + 1).min(GUST_CHARGE);
            (Landing::Perfect, SETTLE_TICKS)
        } else if hi.min(top.hi) - lo.max(top.lo) >= SLIVER {
            self.height += 1;
            self.floors[self.height as usize] = Floor {
                lo: lo.max(top.lo),
                hi: hi.min(top.hi),
                seat: builder,
            };
            let cut = if lo < top.lo {
                Landing::Cut { lo, hi: top.lo }
            } else {
                Landing::Cut { lo: top.hi, hi }
            };
            (cut, SETTLE_TICKS)
        } else {
            let knocked = (self.height > 0).then(|| {
                self.height -= 1;
                top
            });
            self.misses += 1;
            (Landing::Miss { lo, hi, knocked }, KNOCK_TICKS)
        };
        self.landing = Some((tick, landing));
        self.turn = (self.turn + 1) % self.member_count;
        self.block = Block::Resting { t: 0, wait };
    }

    fn step(&mut self, inputs: &Inputs, tick: u32) {
        self.block = match self.block {
            Block::Sliding { lo, .. } if inputs.just_pressed(self.builder(), Button::A) => {
                self.drops += 1;
                self.gusted = self.gusted.saturating_sub(1);
                Block::Falling { lo, t: 0 }
            }
            Block::Sliding { .. } => {
                self.slide();
                self.block
            }
            Block::Falling { lo, t } if t + 1 >= FALL_TICKS => {
                self.land(lo, tick);
                self.block
            }
            Block::Falling { lo, t } => Block::Falling { lo, t: t + 1 },
            Block::Resting { t, wait } if t + 1 >= wait => self.next_block(),
            Block::Resting { t, wait } => Block::Resting { t: t + 1, wait },
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Intro {
        t: u16,
    },
    Racing,
    /// The winner is known; everyone stands still while the last drop plays out.
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
    pub phase: Phase,
    /// Ticks of racing so far.
    pub tick: u32,
    /// Indexed by [`Format::team`].
    pub towers: [Tower; MAX_PLAYERS],
    /// Set when the race is decided.
    pub outcome: Option<Outcome>,
}

impl State {
    /// The tower this seat builds, if it plays.
    pub fn tower_of(&self, seat: Seat) -> Option<&Tower> {
        let team = self.format.team(seat)?;
        Some(&self.towers[team as usize]).filter(|t| t.is_built())
    }

    /// Indexes of the towers being built, in team order.
    pub fn built(&self) -> impl Iterator<Item = usize> + '_ {
        (0..MAX_PLAYERS).filter(|t| self.towers[*t].is_built())
    }

    pub fn ticks_left(&self) -> u32 {
        RACE_TICKS.saturating_sub(self.tick)
    }

    /// Where `team`'s gust goes: the tallest rival towers (all of them when tied).
    pub fn gust_targets(&self, team: usize) -> impl Iterator<Item = usize> + '_ {
        let rivals = move || self.built().filter(move |t| *t != team);
        let tallest = rivals().map(|t| self.towers[t].height).max();
        rivals().filter(move |t| Some(self.towers[*t].height) == tallest)
    }

    /// B from any member blows the team's charged gust.
    fn blow_gusts(&mut self, inputs: &Inputs) {
        for seat in self.players.seats() {
            let Some(team) = self.format.team(seat) else {
                continue;
            };
            let team = team as usize;
            if !inputs.just_pressed(seat, Button::B) || self.towers[team].charge < GUST_CHARGE {
                continue;
            }
            self.towers[team].charge = 0;
            let targets: Vec<usize> = self.gust_targets(team).collect();
            for target in targets {
                let tower = &mut self.towers[target];
                tower.gusted = (tower.gusted + GUST_BLOCKS).min(MAX_GUSTED);
                tower.gust_hit = Some(self.tick);
            }
        }
    }

    fn decide_if_over(&mut self) {
        let topped = self.towers.iter().any(|t| t.height >= GOAL_FLOORS);
        if self.outcome.is_none() && (topped || self.tick >= RACE_TICKS) {
            let scores = std::array::from_fn(|t| self.towers[t].score());
            self.outcome = Some(Outcome::from_team_scores(
                &self.players,
                &self.format,
                &scores,
            ));
            self.phase = Phase::Decided { t: 0 };
        }
    }
}

impl Minigame for LanternTower {
    type State = State;

    const ID: &'static str = "lantern-tower";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[
        FormatKind::FreeForAll,
        FormatKind::OneVsThree,
        FormatKind::TwoVsTwo,
        FormatKind::Duel,
    ];
    const MAX_TICKS: u32 = INTRO_TICKS as u32 + RACE_TICKS + DECIDED_TICKS as u32 + 60;

    fn new(seed: u64, players: PlayerSetup, format: Format) -> State {
        let empty = Tower {
            members: [Seat(0); MAX_PLAYERS],
            member_count: 0,
            turn: 0,
            floors: [Floor {
                lo: -BLOCK_WIDTH / 2,
                hi: BLOCK_WIDTH / 2,
                seat: Seat(0),
            }; FLOORS],
            height: 0,
            block: Block::Resting { t: 0, wait: 1 },
            drops: 0,
            charge: 0,
            gusted: 0,
            landing: None,
            gust_hit: None,
            perfects: 0,
            misses: 0,
        };
        let mut towers = [empty; MAX_PLAYERS];
        // Seats outside a duel have no team and sit this one out.
        for seat in players.seats() {
            if let Some(team) = format.team(seat) {
                let tower = &mut towers[team as usize];
                if tower.member_count == 0 {
                    tower.floors[0].seat = seat;
                }
                tower.members[tower.member_count as usize] = seat;
                tower.member_count += 1;
            }
        }
        for tower in &mut towers {
            tower.block = tower.next_block();
        }
        State {
            seed,
            players,
            format,
            phase: Phase::Intro { t: 0 },
            tick: 0,
            towers,
            outcome: None,
        }
    }

    fn step(state: &mut State, inputs: &Inputs) {
        match state.phase {
            Phase::Intro { t } => {
                state.phase = if t + 1 >= INTRO_TICKS {
                    Phase::Racing
                } else {
                    Phase::Intro { t: t + 1 }
                };
            }
            Phase::Racing => {
                state.tick += 1;
                state.blow_gusts(inputs);
                for tower in &mut state.towers {
                    if tower.is_built() {
                        tower.step(inputs, state.tick);
                    }
                }
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
        let mut input = Input::default();
        if state.phase != Phase::Racing || !state.players.is_active(seat) {
            return input;
        }
        let Some(tower) = state.tower_of(seat) else {
            return input;
        };
        // Only the builder plays, so teammates don't spend two gusts at once.
        if tower.builder() != seat {
            return input;
        }
        if tower.charge >= GUST_CHARGE && gust_now(state, seat, difficulty) {
            input.set(Button::B, true);
        }
        let Block::Sliding { lo, right } = tower.block else {
            return input;
        };
        // One aim per block, so the bot stays a pure function of the state: off the floor below
        // by a triangular error that grows with the block's speed.
        let mut rng = Rng::new(
            state.seed
                ^ (seat.0 as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15)
                ^ (tower.drops as u64 + 1).wrapping_mul(0xbf58_476d_1ce4_e5b9),
        );
        // How early or late the bot presses, in tenths of a tick either way.
        let timing = match difficulty {
            Difficulty::Easy => 90,
            Difficulty::Normal => 70,
            Difficulty::Hard => 40,
        };
        let spread = timing * tower.speed() / 10;
        let error = rng.range_u32(0, spread as u32) as i32 + rng.range_u32(0, spread as u32) as i32
            - spread;
        let aim = tower.top().lo + error;
        // Drop on the tick the block passes the aim.
        let step = if right { tower.speed() } else { -tower.speed() };
        let was = lo - step;
        if (was - aim).signum() != (lo - aim).signum() || lo == aim {
            input.set(Button::A, true);
        }
        input
    }
}

/// Easy bots blow a gust as soon as it's charged; the others wait until it hits a block that has
/// just started to slide and isn't gusted already.
fn gust_now(state: &State, seat: Seat, difficulty: Difficulty) -> bool {
    if difficulty == Difficulty::Easy {
        return true;
    }
    let Some(team) = state.format.team(seat) else {
        return false;
    };
    // Of tied rivals, watch the next one round from this team, so rivals' bots don't all watch
    // the same tower and hit it with three gusts at once (which stack only to [`MAX_GUSTED`]).
    let team = team as usize;
    let Some(target) = state
        .gust_targets(team)
        .min_by_key(|t| (t + MAX_PLAYERS - team) % MAX_PLAYERS)
    else {
        return false;
    };
    let target = &state.towers[target];
    let (left, right) = target.swing();
    match target.block {
        Block::Sliding { lo, right: true } => target.gusted == 0 && lo - left < 4 * target.speed(),
        Block::Sliding { lo, right: false } => {
            target.gusted == 0 && right - lo < 4 * target.speed()
        }
        _ => false,
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

    fn new_run(players: usize, format: Format) -> Run<LanternTower> {
        Run::new(7, PlayerSetup::first(players), format, RunConfig::default())
    }

    fn step_until(run: &mut Run<LanternTower>, done: impl Fn(&State) -> bool) {
        for _ in 0..LanternTower::MAX_TICKS {
            if done(&run.game) {
                return;
            }
            run.step(NONE);
        }
        panic!("never reached");
    }

    fn press(run: &mut Run<LanternTower>, seat: usize, button: Button) {
        let mut inputs = NONE;
        inputs[seat] = Input::default().with(button);
        run.step(inputs);
        run.step(NONE);
    }

    /// Waits for `team`'s block to slide until its left edge is `offset` from the top's, drops
    /// it with its builder, and waits for it to land.
    fn drop_at(run: &mut Run<LanternTower>, team: usize, offset: i32) {
        step_until(run, |g| {
            let tower = &g.towers[team];
            match tower.block {
                Block::Sliding { lo, right } => {
                    let aim = tower.top().lo + offset;
                    if right { lo >= aim } else { lo <= aim }
                }
                _ => false,
            }
        });
        // Put the block exactly there, so tests needn't depend on the speed.
        let tower = &mut run.game.towers[team];
        let Block::Sliding { right, .. } = tower.block else {
            unreachable!()
        };
        tower.block = Block::Sliding {
            lo: tower.top().lo + offset,
            right,
        };
        let builder = tower.builder().index();
        let mut inputs = NONE;
        inputs[builder] = Input::default().with(Button::A);
        // Pressing drops before the block moves on.
        run.step(inputs);
        step_until(run, |g| {
            matches!(g.towers[team].block, Block::Resting { .. })
        });
    }

    fn racing(players: usize, format: Format) -> Run<LanternTower> {
        let mut run = new_run(players, format);
        step_until(&mut run, |g| g.phase == Phase::Racing);
        run
    }

    #[test]
    fn an_overhang_is_cut_off() {
        let mut run = racing(2, Format::FreeForAll);
        drop_at(&mut run, 0, 200);
        let tower = &run.game.towers[0];
        assert_eq!(tower.height, 1);
        assert_eq!(tower.top().width(), BLOCK_WIDTH - 200);
        assert_eq!(tower.top().lo, tower.floors[0].lo + 200);
        assert_eq!(tower.top().hi, tower.floors[0].hi);
        assert!(matches!(
            tower.landing,
            Some((_, Landing::Cut { lo, hi })) if hi - lo == 200
        ));
        assert_eq!(tower.charge, 0);
    }

    #[test]
    fn a_perfect_drop_cuts_nothing_and_charges_a_gust() {
        let mut run = racing(2, Format::FreeForAll);
        drop_at(&mut run, 0, -PERFECT);
        let tower = &run.game.towers[0];
        assert_eq!(tower.height, 1);
        assert_eq!(tower.top().width(), BLOCK_WIDTH);
        assert_eq!(tower.top().lo, tower.floors[0].lo);
        assert_eq!(tower.charge, 1);
        assert_eq!(tower.perfects, 1);
        for _ in 0..GUST_CHARGE {
            drop_at(&mut run, 0, 0);
        }
        assert_eq!(run.game.towers[0].charge, GUST_CHARGE);
    }

    #[test]
    fn a_perfect_drop_widens_the_tower_back() {
        let mut run = racing(2, Format::FreeForAll);
        drop_at(&mut run, 0, 300);
        let narrow = run.game.towers[0].top();
        drop_at(&mut run, 0, PERFECT);
        let top = run.game.towers[0].top();
        assert_eq!(top.width(), narrow.width() + GROW);
        assert_eq!(top.lo, narrow.lo - GROW / 2, "it grows on both sides");
        for _ in 0..10 {
            drop_at(&mut run, 0, 0);
        }
        assert_eq!(run.game.towers[0].top().width(), BLOCK_WIDTH);
    }

    #[test]
    fn a_miss_knocks_the_top_floor_off() {
        let mut run = racing(2, Format::FreeForAll);
        drop_at(&mut run, 0, 300);
        drop_at(&mut run, 0, 0);
        assert_eq!(run.game.towers[0].height, 2);
        let narrow = run.game.towers[0].top();
        // Leaves less than a sliver: a miss.
        drop_at(&mut run, 0, narrow.width() - SLIVER + 1);
        let tower = &run.game.towers[0];
        assert_eq!(tower.height, 1);
        assert_eq!(tower.misses, 1);
        assert!(matches!(
            tower.landing,
            Some((_, Landing::Miss { knocked: Some(floor), .. })) if floor == narrow
        ));
        // The next block is as wide as the new top.
        step_until(&mut run, |g| {
            matches!(g.towers[0].block, Block::Sliding { .. })
        });
        assert_eq!(run.game.towers[0].top().width(), BLOCK_WIDTH - 300);
    }

    #[test]
    fn a_miss_on_the_bare_base_knocks_nothing() {
        let mut run = racing(2, Format::FreeForAll);
        drop_at(&mut run, 0, -BLOCK_WIDTH);
        let tower = &run.game.towers[0];
        assert_eq!(tower.height, 0);
        assert!(matches!(
            tower.landing,
            Some((_, Landing::Miss { knocked: None, .. }))
        ));
    }

    #[test]
    fn blocks_bounce_between_the_ends_of_the_swing() {
        let mut run = racing(1, Format::FreeForAll);
        let mut turned = 0;
        let mut was = run.game.towers[0].block;
        for _ in 0..600 {
            run.step(NONE);
            let Block::Sliding { lo, right } = run.game.towers[0].block else {
                panic!("nobody dropped it");
            };
            let centre = lo + BLOCK_WIDTH / 2;
            assert!(centre.abs() <= SWING, "{centre}");
            if matches!(was, Block::Sliding { right: r, .. } if r != right) {
                turned += 1;
            }
            was = run.game.towers[0].block;
        }
        assert!(turned >= 4, "{turned}");
    }

    #[test]
    fn blocks_speed_up_as_the_tower_grows() {
        let mut run = racing(2, Format::FreeForAll);
        let first = run.game.towers[0].speed();
        for _ in 0..5 {
            drop_at(&mut run, 0, 0);
        }
        assert_eq!(run.game.towers[0].speed(), first + 5 * SPEED_PER_FLOOR);
    }

    #[test]
    fn a_gust_speeds_up_the_tallest_rivals_next_two_blocks() {
        let mut run = racing(4, Format::FreeForAll);
        // Not charged yet: B does nothing.
        press(&mut run, 0, Button::B);
        assert!(run.game.towers.iter().all(|t| t.gusted == 0));
        for _ in 0..GUST_CHARGE {
            drop_at(&mut run, 0, 0);
        }
        drop_at(&mut run, 2, 100);
        drop_at(&mut run, 3, 0);
        let calm = run.game.towers[2].speed();
        press(&mut run, 0, Button::B);
        assert_eq!(run.game.towers[0].charge, 0);
        assert_eq!(run.game.towers[0].gusted, 0, "not your own tower");
        assert_eq!(run.game.towers[1].gusted, 0, "not a shorter tower");
        for tied in [2, 3] {
            assert_eq!(run.game.towers[tied].gusted, GUST_BLOCKS);
            assert_eq!(
                run.game.towers[tied].speed(),
                calm * GUST_SPEEDUP.0 / GUST_SPEEDUP.1
            );
        }
        drop_at(&mut run, 2, 0);
        drop_at(&mut run, 2, 0);
        assert_eq!(run.game.towers[2].gusted, 0);
    }

    #[test]
    fn team_members_take_turns() {
        let format = Format::TwoVsTwo {
            team_of: [0, 1, 0, 1],
        };
        let mut run = racing(4, format);
        let tower = &run.game.towers[0];
        assert_eq!(tower.members(), &[Seat(0), Seat(2)]);
        assert_eq!(tower.builder(), Seat(0));
        // It isn't P3's turn: their A does nothing.
        step_until(&mut run, |g| {
            matches!(g.towers[0].block, Block::Sliding { .. })
        });
        press(&mut run, 2, Button::A);
        assert!(matches!(run.game.towers[0].block, Block::Sliding { .. }));
        drop_at(&mut run, 0, 0);
        assert_eq!(run.game.towers[0].builder(), Seat(2));
        assert_eq!(run.game.towers[0].top().seat, Seat(0));
        drop_at(&mut run, 0, 0);
        assert_eq!(run.game.towers[0].top().seat, Seat(2));
        assert_eq!(run.game.towers[0].builder(), Seat(0));
        // Either member blows the team's gust.
        drop_at(&mut run, 0, 0);
        press(&mut run, 2, Button::B);
        assert_eq!(run.game.towers[1].gusted, GUST_BLOCKS);
    }

    #[test]
    fn the_trio_shares_one_tower() {
        let run = racing(4, Format::OneVsThree { solo: Seat(1) });
        assert_eq!(run.game.towers[0].members(), &[Seat(1)]);
        assert_eq!(run.game.towers[1].members(), &[Seat(0), Seat(2), Seat(3)]);
        assert_eq!(run.game.built().count(), 2);
    }

    #[test]
    fn first_to_the_goal_wins() {
        let mut run = racing(3, Format::FreeForAll);
        for _ in 0..GOAL_FLOORS {
            drop_at(&mut run, 1, 0);
        }
        step_until(&mut run, |g| g.phase == Phase::Finished);
        let outcome = run.outcome().unwrap();
        assert_eq!(outcome.winners().collect::<Vec<_>>(), vec![Seat(1)]);
    }

    #[test]
    fn time_up_ranks_by_height_then_width() {
        let mut run = racing(3, Format::FreeForAll);
        drop_at(&mut run, 0, 0);
        drop_at(&mut run, 1, 100);
        drop_at(&mut run, 2, 0);
        drop_at(&mut run, 2, 0);
        // Nobody drops again; the blocks slide until time runs out.
        step_until(&mut run, |g| g.phase == Phase::Finished);
        assert_eq!(run.game.tick, RACE_TICKS);
        let outcome = run.outcome().unwrap();
        assert_eq!(outcome.placement, [Some(2), Some(3), Some(1), None]);
    }

    #[test]
    fn seats_outside_a_duel_sit_out() {
        let run = new_run(
            4,
            Format::Duel {
                a: Seat(1),
                b: Seat(3),
            },
        );
        assert_eq!(run.game.built().count(), 2);
        assert!(run.game.tower_of(Seat(0)).is_none());
        assert_eq!(run.game.tower_of(Seat(3)).unwrap().members(), &[Seat(3)]);
    }

    #[test]
    fn nothing_drops_during_the_countdown() {
        let mut run = new_run(2, Format::FreeForAll);
        for _ in 0..20 {
            press(&mut run, 0, Button::A);
        }
        assert!(matches!(run.game.phase, Phase::Intro { .. }));
        assert_eq!(run.game.towers[0].drops, 0);
    }

    #[test]
    fn harder_bots_build_faster() {
        let finish = |difficulty| {
            let mut total = 0;
            for seed in 0..20 {
                let mut run: Run<LanternTower> = Run::new(
                    seed,
                    PlayerSetup::first(1),
                    Format::FreeForAll,
                    RunConfig::default(),
                );
                while run.outcome().is_none() {
                    let inputs =
                        Seat::ALL.map(|s| LanternTower::bot_input(&run.game, s, difficulty));
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
        sim_check::check_all::<LanternTower>(8).unwrap();
    }
}
