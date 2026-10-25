//! Lantern Masquerade: hide in plain sight. [`FOLK`] identical lantern folk stroll round a night
//! market, and the players are hidden among them, looking exactly alike. Nobody is told which
//! folk is theirs: you find out by moving. The strollers walk the way a crowd does (a while in
//! one of [`DIRECTIONS`] directions, then a stop), and the stick walks you the same way at the
//! same pace, so it's how you move that gives you away.
//!
//! Free-for-all, 2v2 and duel: [`BELLS`] bells hang over the market. Stand still under one and
//! hold A to pull its rope: it swings, and after [`RING_TICKS`] it rings. Everyone hears it and
//! the scoreboard shows whose it was, so whoever stands under that bell is a suspect. B strikes the folk beside you: a struck player is out of the round,
//! and a struck stroller falls over in front of everyone. A round goes to whoever rings every
//! bell (partners share them in 2v2) or is the last one standing; at the time limit, to the most
//! bells. First to [`ROUNDS_TO_WIN`] rounds wins.
//!
//! 1v3: the solo is the seeker, outside the crowd, with a lantern sight; A catches the folk
//! under it. A caught hider is out. Catching a stroller puts the lantern out for
//! [`DARK_TICKS`]. The three ring the bells between them and take the round once all of them
//! have rung; the seeker takes it by catching all three, or when time runs out.
//!
//! For the first [`SHUFFLE_TICKS`] of every round the strollers shuffle on the spot, so the
//! players can find themselves without standing out, and nobody can ring or strike yet. Each
//! round deals the players new folk.
//!
//! The simulation also keeps the clues anyone watching could have picked up ([`Clue`]): who stood
//! under a bell when it rang, who walked as no stroller does, who struck. Bots read only those,
//! the scoreboard and their own folk, never which folk the others are.
//!
//! The floor uses `sim_core::physics`' plane: `x` to the right, `y` towards the camera.

use std::f32::consts::{PI, TAU};

use sim_core::math;
use sim_core::physics::{DT, Vec2};
use sim_core::{
    Button, Difficulty, Format, FormatKind, Input, Inputs, MAX_PLAYERS, Minigame, Outcome,
    PlayerSetup, Rng, Seat,
};

mod bot;

/// The opening countdown, the same in every minigame.
pub const INTRO_TICKS: u16 = sim_core::COUNTDOWN_TICKS;
/// Everyone in the market, players included.
pub const FOLK: usize = 30;
pub const BELLS: usize = 5;
const ALL_BELLS: u8 = (1 << BELLS) - 1;
pub const ROUNDS_TO_WIN: u8 = 2;
/// A match ends after this many rounds even if nobody has won enough.
pub const MAX_ROUNDS: u8 = 5;

/// Folk centres stay within these half extents of the market's middle.
pub const HALF_WIDTH: f32 = 10.0;
pub const HALF_DEPTH: f32 = 6.0;
/// Where the bells hang: the four corners' insides and the middle.
pub const BELL_POSITIONS: [Vec2; BELLS] = [
    Vec2::new(-7.0, -3.6),
    Vec2::new(7.0, -3.6),
    Vec2::new(0.0, 0.0),
    Vec2::new(-7.0, 3.6),
    Vec2::new(7.0, 3.6),
];
/// Everyone walks in one of this many directions...
pub const DIRECTIONS: u8 = 16;
/// ...at this one pace, in floor units a second.
pub const WALK_SPEED: f32 = 1.4;
pub const STEP: f32 = WALK_SPEED * DT;
/// The stick walks once pushed this far.
pub const STICK_WALK: f32 = 0.4;
/// A walker keeps its direction while the stick stays within this many directions' widths of
/// it, so a wobbly thumb doesn't zigzag.
const TURN_HYSTERESIS: f32 = 0.75;
/// Standing still within this of a bell's spot and holding A pulls its rope...
pub const BELL_REACH: f32 = 1.1;
/// ...and it rings after this long.
pub const RING_TICKS: u16 = 60;
/// Folk within this of a ringing bell are suspects.
pub const CLUE_REACH: f32 = 1.5;
/// B strikes the nearest folk within this.
pub const STRIKE_REACH: f32 = 0.9;
pub const STRIKE_COOLDOWN: u16 = 60;
/// A struck stroller lies down this long, then gets up and strolls on.
pub const FALL_TICKS: u16 = 240;
/// The strollers shuffle at the start of every round this long, and nobody can ring or strike.
pub const SHUFFLE_TICKS: u16 = 180;
pub const ROUND_TICKS: u16 = 90 * 60;
pub const SEEKER_ROUND_TICKS: u16 = 75 * 60;
/// The seeker's sight, at full stick, in floor units a second.
pub const SIGHT_SPEED: f32 = 7.0;
/// The seeker's A catches the nearest folk within this of the sight.
pub const CATCH_REACH: f32 = 0.55;
/// The seeker's lantern stays out this long after catching a stroller.
pub const DARK_TICKS: u16 = 180;
/// The round's result, and who everyone was, shows this long (the last one's longer).
pub const RESULT_TICKS: u16 = 180;
pub const FINAL_RESULT_TICKS: u16 = 240;

/// How long a stroller walks and stops, in ticks. Players moving outside these stand out...
pub const NPC_WALK: (u16, u16) = (40, 160);
pub const NPC_PAUSE: (u16, u16) = (20, 150);
/// ...though strollers fidget too: this percent of their walks and stops are short ones.
pub const NPC_FIDGET_PERCENT: u32 = 8;
pub const FIDGET_WALK: (u16, u16) = (10, 39);
pub const FIDGET_PAUSE: (u16, u16) = (6, 19);
/// Percent of walks that turn straight into another walk, without stopping.
pub const NPC_TURN_PERCENT: u32 = 25;
/// Percent of walks headed for a bell, so standing under one is nothing odd...
pub const NPC_BELL_PERCENT: u32 = 45;
/// ...and strollers linger there: a stop under a bell is this long.
pub const NPC_BELL_PAUSE: (u16, u16) = (60, 300);
/// While shuffling: short steps and short stops.
pub const SHUFFLE_WALK: (u16, u16) = (6, 20);
pub const SHUFFLE_PAUSE: (u16, u16) = (4, 16);
/// Folk start at least this far apart.
const SPAWN_GAP: f32 = 0.8;
/// Strollers plan their walks to stay this far inside the edge.
const MARGIN: f32 = 0.3;

pub struct LanternMasquerade;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Intro {
        t: u16,
    },
    /// A round, `t` ticks in.
    Play {
        t: u16,
    },
    /// The round is over and everyone's shown.
    Result {
        t: u16,
    },
    Finished,
}

/// What ended a round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ending {
    /// A side rang every bell.
    Bells,
    /// One side was left standing (or the last ones struck each other down together).
    LastStanding,
    /// The seeker caught all three.
    Caught,
    /// Time ran out: the most bells win, or in 1v3 the seeker.
    TimeUp,
}

/// What anyone watching could have noticed about one folk this round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Clue {
    /// Bells each team (by [`Format::team`]) rang while this folk stood under them...
    pub rang: [u8; MAX_PLAYERS],
    /// ...the play tick of the last of them, and of the one before (0 for none). Linking two
    /// rings means keeping track of one folk in the crowd between them.
    pub rang_at: [u32; MAX_PLAYERS],
    pub rang_before: [u32; MAX_PLAYERS],
    /// Walks and stops no stroller would make: shorter than [`NPC_WALK`] or [`NPC_PAUSE`].
    pub odd: u8,
    pub odd_at: u32,
    /// It struck someone: strollers never do.
    pub struck: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct Folk {
    pub pos: Vec2,
    /// The way it faces and walks: 0 is +x, counting towards +y, in [`DIRECTIONS`] steps.
    pub dir: u8,
    pub moving: bool,
    /// Ticks into the current walk or stop.
    pub segment: u16,
    /// Walks and stops started this round.
    pub segments: u16,
    /// Strollers: ticks left of the current walk or stop.
    plan: u16,
    /// A struck stroller lies down for this many more ticks.
    pub down: u16,
    /// A player struck or caught: lying down, shown, for the rest of the round.
    pub out: bool,
    /// The play tick it last struck, and was last struck or caught (or lit up by the seeker).
    pub struck_at: Option<u32>,
    pub hit_at: Option<u32>,
    /// Shown to be a stroller this round, by a strike or the seeker's lantern.
    pub cleared: bool,
    pub clue: Clue,
}

impl Folk {
    /// On its feet and in the round.
    pub fn standing(&self) -> bool {
        self.down == 0 && !self.out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Player {
    /// False for seats outside a duel.
    pub playing: bool,
    pub seeker: bool,
    /// Which folk this player is this round. Secret: see [`State::folk_of`].
    folk: u8,
    pub out: bool,
    /// Bells this player rang this round, as bits by bell.
    pub bells: u8,
    /// Bells rung over the whole match (the tiebreak).
    pub total_bells: u32,
    pub cooldown: u16,
    /// Ticks spent pulling a bell's rope, standing still with A held.
    pub pull: u16,
}

impl Player {
    pub fn hiding(&self) -> bool {
        self.playing && !self.seeker
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Bell {
    pub rung_at: Option<u32>,
    /// Seats that rang it this round, as bits.
    pub rung_by: u8,
    /// Someone has pulled its rope this long (the longest pull, if several): it swings.
    pub pull: u16,
}

/// The 1v3 seeker's lantern sight.
#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct Sight {
    pub pos: Vec2,
    /// Ticks left with the lantern out.
    pub dark: u16,
    /// The play tick it last caught a hider, and last caught a stroller.
    pub caught_at: Option<u32>,
    pub missed_at: Option<u32>,
}

#[derive(Debug, Clone, Hash)]
pub struct State {
    pub seed: u64,
    pub players: PlayerSetup,
    pub format: Format,
    pub phase: Phase,
    rng: Rng,
    /// Ticks since the intro ended, for clues, effects and bots.
    pub tick: u32,
    /// The round being played, from 0.
    pub round: u8,
    pub folk: [Folk; FOLK],
    /// Which seat each folk is. Secret, like [`Player::folk`].
    owner: [Option<Seat>; FOLK],
    pub seats: [Player; MAX_PLAYERS],
    pub bells: [Bell; BELLS],
    pub sight: Sight,
    /// Rounds won, by team.
    pub team_rounds: [u8; MAX_PLAYERS],
    /// Seats on the side that took the last round, as bits, and how.
    pub round_winners: u8,
    pub ended_by: Option<Ending>,
    /// Set once the last round is scored.
    pub outcome: Option<Outcome>,
}

/// The unit vector of direction `dir`.
pub fn dir_vec(dir: u8) -> Vec2 {
    Vec2::from_angle(dir as f32 * TAU / DIRECTIONS as f32)
}

/// The direction nearest to the angle `angle`.
fn sector(angle: f32) -> u8 {
    let width = TAU / DIRECTIONS as f32;
    ((angle / width).round() as i32).rem_euclid(DIRECTIONS as i32) as u8
}

/// The direction nearest to `v`'s.
pub fn sector_toward(v: Vec2) -> u8 {
    sector(math::atan2(v.y, v.x))
}

fn inside(p: Vec2, margin: f32) -> bool {
    p.x.abs() <= HALF_WIDTH - margin && p.y.abs() <= HALF_DEPTH - margin
}

fn clamp_to_floor(p: Vec2) -> Vec2 {
    Vec2::new(
        p.x.clamp(-HALF_WIDTH, HALF_WIDTH),
        p.y.clamp(-HALF_DEPTH, HALF_DEPTH),
    )
}

/// Bit `seat` set for each seat in `seats`.
fn mask(seats: impl Iterator<Item = Seat>) -> u8 {
    seats.fold(0, |m, s| m | 1 << s.0)
}

impl State {
    pub fn seat(&self, seat: Seat) -> &Player {
        &self.seats[seat.index()]
    }

    /// Which folk `seat` is this round. This is the game's secret: views must not show it while
    /// the round is on (they use [`State::revealed`]), and a bot may only ask about its own seat.
    pub fn folk_of(&self, seat: Seat) -> usize {
        self.seats[seat.index()].folk as usize
    }

    /// Who folk `f` is, if everyone may know: once that player is out, or the round is over.
    pub fn revealed(&self, f: usize) -> Option<Seat> {
        let over = matches!(self.phase, Phase::Result { .. } | Phase::Finished);
        self.owner[f].filter(|_| over || self.folk[f].out)
    }

    pub fn seeker(&self) -> Option<Seat> {
        match self.format {
            Format::OneVsThree { solo } => Some(solo),
            _ => None,
        }
    }

    /// Seats in the game (benched duel seats aren't).
    pub fn playing(&self) -> impl Iterator<Item = Seat> + '_ {
        self.players
            .seats()
            .filter(|s| self.seats[s.index()].playing)
    }

    /// Seats hiding in the crowd: everyone playing but the seeker.
    pub fn hiders(&self) -> impl Iterator<Item = Seat> + '_ {
        self.playing().filter(|s| !self.seats[s.index()].seeker)
    }

    /// Hiders still in the round.
    pub fn standing(&self) -> impl Iterator<Item = Seat> + '_ {
        self.hiders().filter(|s| !self.seats[s.index()].out)
    }

    pub fn team(&self, seat: Seat) -> u8 {
        self.format.team(seat).unwrap_or(seat.0)
    }

    /// The bells `team` has rung this round, as bits.
    pub fn team_bells(&self, team: u8) -> u8 {
        self.hiders()
            .filter(|s| self.team(*s) == team)
            .fold(0, |m, s| m | self.seats[s.index()].bells)
    }

    /// The round's time limit.
    pub fn round_ticks(&self) -> u16 {
        if self.seeker().is_some() {
            SEEKER_ROUND_TICKS
        } else {
            ROUND_TICKS
        }
    }

    /// Ringing and striking are open: the round is on and the shuffle is over.
    pub fn can_act(&self) -> bool {
        matches!(self.phase, Phase::Play { t } if t >= SHUFFLE_TICKS)
    }

    /// The bell whose spot `pos` is within reach of.
    pub fn bell_in_reach(&self, pos: Vec2) -> Option<usize> {
        (0..BELLS).find(|b| BELL_POSITIONS[*b].distance(pos) <= BELL_REACH)
    }

    /// The nearest standing folk other than `except` within `reach` of `pos`.
    pub fn nearest_folk(&self, pos: Vec2, reach: f32, except: Option<usize>) -> Option<usize> {
        (0..FOLK)
            .filter(|f| Some(*f) != except && self.folk[*f].standing())
            .map(|f| (self.folk[f].pos.distance(pos), f))
            .filter(|(d, _)| *d <= reach)
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, f)| f)
    }

    pub fn last_round(&self) -> bool {
        self.round + 1 >= MAX_ROUNDS
    }

    /// Deal the players new folk and scatter the crowd.
    fn start_round(&mut self) {
        let mut order: Vec<usize> = (0..FOLK).collect();
        for i in (1..FOLK).rev() {
            let j = self.rng.range_u32(0, i as u32) as usize;
            order.swap(i, j);
        }
        self.owner = [None; FOLK];
        for (k, seat) in self.hiders().collect::<Vec<_>>().into_iter().enumerate() {
            self.owner[order[k]] = Some(seat);
            self.seats[seat.index()].folk = order[k] as u8;
        }
        let mut placed: Vec<Vec2> = Vec::new();
        for f in 0..FOLK {
            let mut pos = Vec2::ZERO;
            for _ in 0..40 {
                pos = Vec2::new(
                    self.random_between(-HALF_WIDTH + 0.6, HALF_WIDTH - 0.6),
                    self.random_between(-HALF_DEPTH + 0.6, HALF_DEPTH - 0.6),
                );
                if placed.iter().all(|p| p.distance(pos) >= SPAWN_GAP) {
                    break;
                }
            }
            placed.push(pos);
            let dir = self.rng.range_u32(0, DIRECTIONS as u32 - 1) as u8;
            let plan = self.rng.range_u32(0, SHUFFLE_PAUSE.1 as u32) as u16;
            self.folk[f] = Folk {
                pos,
                dir,
                moving: false,
                segment: 0,
                segments: 0,
                plan,
                down: 0,
                out: false,
                struck_at: None,
                hit_at: None,
                cleared: false,
                clue: Clue::default(),
            };
        }
        for player in &mut self.seats {
            player.out = false;
            player.bells = 0;
            player.cooldown = 0;
            player.pull = 0;
        }
        self.bells = [Bell::default(); BELLS];
        self.sight = Sight {
            pos: Vec2::ZERO,
            dark: 0,
            caught_at: None,
            missed_at: None,
        };
        self.round_winners = 0;
        self.ended_by = None;
    }

    fn random_between(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.rng.range_u32(0, 10_000) as f32 / 10_000.0
    }

    /// Start a walk in `dir`, or stop (`moving` false), noting a walk or stop no stroller makes.
    fn set_motion(&mut self, f: usize, moving: bool, dir: u8, t: u16) {
        let tick = self.tick;
        let folk = &mut self.folk[f];
        if folk.moving == moving && (!moving || folk.dir == dir) {
            return;
        }
        // Shuffling steps are short for everyone, so only later ones count.
        if t.saturating_sub(folk.segment) >= SHUFFLE_TICKS {
            let odd = if folk.moving {
                folk.segment < NPC_WALK.0
            } else {
                folk.segment > 0 && folk.segment < NPC_PAUSE.0
            };
            if odd {
                folk.clue.odd = folk.clue.odd.saturating_add(1);
                folk.clue.odd_at = tick;
            }
        }
        folk.moving = moving;
        if moving {
            folk.dir = dir;
        }
        folk.segment = 0;
        folk.segments += 1;
    }

    /// A player's stick: walk in the nearest direction, keeping the current one through small
    /// wobbles, or stop.
    fn steer(&mut self, f: usize, input: &Input, t: u16) {
        let stick = Vec2::from_stick(input);
        let folk = self.folk[f];
        if stick.length() < STICK_WALK {
            self.set_motion(f, false, folk.dir, t);
            return;
        }
        let angle = math::atan2(stick.y, stick.x);
        if folk.moving {
            let width = TAU / DIRECTIONS as f32;
            let mut off = (angle - folk.dir as f32 * width).rem_euclid(TAU);
            if off > PI {
                off = TAU - off;
            }
            if off <= TURN_HYSTERESIS * width {
                return;
            }
        }
        self.set_motion(f, true, sector(angle), t);
    }

    /// A stroller whose walk or stop is over picks the next.
    fn plan_stroller(&mut self, f: usize, t: u16) {
        let shuffle = t < SHUFFLE_TICKS;
        let folk = self.folk[f];
        let at_bell = self.bell_in_reach(folk.pos).is_some();
        let walk = !folk.moving || (!shuffle && !at_bell && self.rng.chance(NPC_TURN_PERCENT));
        if walk {
            let avoid = folk.moving.then_some(folk.dir);
            let (dir, ticks) = self.choose_walk(folk.pos, avoid, shuffle);
            self.set_motion(f, true, dir, t);
            self.folk[f].plan = ticks;
        } else {
            let (lo, hi) = if shuffle {
                SHUFFLE_PAUSE
            } else if at_bell {
                NPC_BELL_PAUSE
            } else if self.rng.chance(NPC_FIDGET_PERCENT) {
                FIDGET_PAUSE
            } else {
                NPC_PAUSE
            };
            self.set_motion(f, false, folk.dir, t);
            self.folk[f].plan = self.rng.range_u32(lo as u32, hi as u32) as u16;
        }
    }

    /// A walk from `from` that stays on the floor: sometimes to a bell, mostly anywhere.
    fn choose_walk(&mut self, from: Vec2, avoid: Option<u8>, shuffle: bool) -> (u8, u16) {
        let (lo, hi) = if shuffle {
            SHUFFLE_WALK
        } else if self.rng.chance(NPC_FIDGET_PERCENT) {
            FIDGET_WALK
        } else {
            NPC_WALK
        };
        let fits = |dir: u8, ticks: u16| {
            Some(dir) != avoid && inside(from + dir_vec(dir) * (STEP * ticks as f32), MARGIN)
        };
        if !shuffle && self.rng.chance(NPC_BELL_PERCENT) {
            let bell = BELL_POSITIONS[self.rng.range_u32(0, BELLS as u32 - 1) as usize];
            let dir = sector_toward(bell - from);
            let ticks = (bell.distance(from) / STEP) as u16;
            if (NPC_WALK.0..=NPC_WALK.1 + 80).contains(&ticks) && fits(dir, ticks) {
                return (dir, ticks);
            }
        }
        for _ in 0..12 {
            let dir = self.rng.range_u32(0, DIRECTIONS as u32 - 1) as u8;
            let ticks = self.rng.range_u32(lo as u32, hi as u32) as u16;
            if fits(dir, ticks) {
                return (dir, ticks);
            }
        }
        // Hemmed in by the edge: the shortest walk towards the middle.
        let dir = sector_toward(-from);
        let dir = if Some(dir) == avoid {
            (dir + 1) % DIRECTIONS
        } else {
            dir
        };
        (dir, lo)
    }

    fn ring(&mut self, seat: Seat, bell: usize) {
        let tick = self.tick;
        let team = self.team(seat) as usize;
        let player = &mut self.seats[seat.index()];
        player.bells |= 1 << bell;
        player.total_bells += 1;
        self.bells[bell].rung_at = Some(tick);
        self.bells[bell].rung_by |= 1 << seat.0;
        let at = BELL_POSITIONS[bell];
        for folk in &mut self.folk {
            if folk.standing() && folk.pos.distance(at) <= CLUE_REACH {
                let clue = &mut folk.clue;
                clue.rang[team] = clue.rang[team].saturating_add(1);
                clue.rang_before[team] = clue.rang_at[team];
                clue.rang_at[team] = tick;
            }
        }
    }

    /// Folk `f` was struck or caught: a player is out, a stroller falls (or, caught by the
    /// seeker, is simply lit up) and is shown to be one.
    fn hit(&mut self, f: usize, falls: bool) {
        let tick = self.tick;
        let owner = self.owner[f];
        let folk = &mut self.folk[f];
        folk.hit_at = Some(tick);
        if let Some(seat) = owner {
            folk.out = true;
            folk.moving = false;
            self.seats[seat.index()].out = true;
        } else {
            folk.cleared = true;
            if falls {
                folk.down = FALL_TICKS;
                folk.moving = false;
            }
        }
    }

    fn step_play(&mut self, inputs: &Inputs, t: u16) {
        let can_act = t >= SHUFFLE_TICKS;
        for seat in self.hiders().collect::<Vec<_>>() {
            let player = &mut self.seats[seat.index()];
            player.cooldown = player.cooldown.saturating_sub(1);
            let (out, f) = (player.out, player.folk as usize);
            if !out {
                self.steer(f, &inputs.current[seat.index()], t);
            }
        }
        for f in 0..FOLK {
            if self.owner[f].is_some() {
                continue;
            }
            let folk = &mut self.folk[f];
            if folk.down > 0 {
                folk.down -= 1;
                if folk.down == 0 {
                    folk.segment = 0;
                    folk.segments += 1;
                    folk.plan = self.rng.range_u32(NPC_PAUSE.0 as u32, NPC_PAUSE.1 as u32) as u16;
                }
                continue;
            }
            if folk.plan == 0 {
                self.plan_stroller(f, t);
            }
        }
        for folk in &mut self.folk {
            if !folk.standing() {
                continue;
            }
            if folk.moving {
                folk.pos = clamp_to_floor(folk.pos + dir_vec(folk.dir) * STEP);
            }
            folk.segment = folk.segment.saturating_add(1);
            folk.plan = folk.plan.saturating_sub(1);
        }

        if let Some(seeker) = self.seeker() {
            self.step_sight(seeker, inputs, can_act);
        }
        if !can_act {
            return;
        }
        for bell in &mut self.bells {
            bell.pull = 0;
        }
        for seat in self.standing().collect::<Vec<_>>() {
            let folk = self.folk[self.folk_of(seat)];
            let pulling = self.bell_in_reach(folk.pos).filter(|b| {
                !folk.moving
                    && inputs.held(seat, Button::A)
                    && self.team_bells(self.team(seat)) & 1 << b == 0
            });
            let player = &mut self.seats[seat.index()];
            let Some(bell) = pulling else {
                player.pull = 0;
                continue;
            };
            player.pull += 1;
            let pull = player.pull;
            self.bells[bell].pull = self.bells[bell].pull.max(pull);
            if pull >= RING_TICKS {
                self.seats[seat.index()].pull = 0;
                self.ring(seat, bell);
            }
        }
        if self.seeker().is_none() {
            // Strikes land together, so two players striking each other both go down.
            let mut hits = Vec::new();
            for seat in self.standing().collect::<Vec<_>>() {
                if self.seats[seat.index()].cooldown > 0 || !inputs.just_pressed(seat, Button::B) {
                    continue;
                }
                let me = self.folk_of(seat);
                self.seats[seat.index()].cooldown = STRIKE_COOLDOWN;
                let folk = &mut self.folk[me];
                folk.struck_at = Some(self.tick);
                folk.clue.struck = true;
                let pos = folk.pos;
                if let Some(target) = self.nearest_folk(pos, STRIKE_REACH, Some(me)) {
                    hits.push(target);
                }
            }
            for target in hits {
                self.hit(target, true);
            }
        }
    }

    fn step_sight(&mut self, seeker: Seat, inputs: &Inputs, can_act: bool) {
        let stick = Vec2::from_stick(&inputs.current[seeker.index()]);
        self.sight.pos = clamp_to_floor(self.sight.pos + stick * (SIGHT_SPEED * DT));
        self.sight.dark = self.sight.dark.saturating_sub(1);
        if !can_act || self.sight.dark > 0 || !inputs.just_pressed(seeker, Button::A) {
            return;
        }
        let Some(f) = self.nearest_folk(self.sight.pos, CATCH_REACH, None) else {
            return;
        };
        if self.owner[f].is_some() {
            self.sight.caught_at = Some(self.tick);
        } else {
            self.sight.missed_at = Some(self.tick);
            self.sight.dark = DARK_TICKS;
        }
        self.hit(f, false);
    }

    /// Seats on the side that takes the round, if it's over. `standing_before` is the hiders
    /// standing when the tick began, who share a round nobody is left standing in.
    fn round_over(&self, t: u16, standing_before: u8) -> Option<(u8, Ending)> {
        let time_up = t + 1 >= self.round_ticks();
        let on_team = |teams: u8| mask(self.hiders().filter(|s| teams & 1 << self.team(*s) != 0));
        let teams_of = |seats: u8| {
            self.hiders()
                .filter(|s| seats & 1 << s.0 != 0)
                .fold(0u8, |m, s| m | 1 << self.team(s))
        };
        if let Some(seeker) = self.seeker() {
            let trio = mask(self.hiders());
            // 1v3 puts the three on team 1.
            return if self.team_bells(1) == ALL_BELLS {
                Some((trio, Ending::Bells))
            } else if self.standing().next().is_none() {
                Some((1 << seeker.0, Ending::Caught))
            } else if time_up {
                Some((1 << seeker.0, Ending::TimeUp))
            } else {
                None
            };
        }
        let full = self
            .hiders()
            .filter(|s| self.team_bells(self.team(*s)) == ALL_BELLS)
            .fold(0u8, |m, s| m | 1 << self.team(s));
        if full != 0 {
            return Some((on_team(full), Ending::Bells));
        }
        let standing = teams_of(mask(self.standing()));
        if teams_of(mask(self.hiders())).count_ones() >= 2 {
            match standing.count_ones() {
                0 => return Some((on_team(teams_of(standing_before)), Ending::LastStanding)),
                1 => return Some((on_team(standing), Ending::LastStanding)),
                _ => {}
            }
        }
        if time_up {
            let most = (0..MAX_PLAYERS as u8)
                .filter(|team| standing & 1 << team != 0)
                .map(|team| self.team_bells(team).count_ones())
                .max()
                .unwrap_or(0);
            let best = (0..MAX_PLAYERS as u8)
                .filter(|team| {
                    standing & 1 << team != 0 && self.team_bells(*team).count_ones() == most
                })
                .fold(0u8, |m, team| m | 1 << team);
            return Some((on_team(best), Ending::TimeUp));
        }
        None
    }

    fn end_round(&mut self, winners: u8, ending: Ending) {
        self.round_winners = winners;
        self.ended_by = Some(ending);
        let mut teams = 0u8;
        for seat in self.playing() {
            if winners & 1 << seat.0 != 0 {
                teams |= 1 << self.team(seat);
            }
        }
        for team in 0..MAX_PLAYERS {
            if teams & 1 << team != 0 {
                self.team_rounds[team] += 1;
            }
        }
        let decided = self.team_rounds.iter().any(|r| *r >= ROUNDS_TO_WIN);
        if decided || self.last_round() {
            self.outcome = Some(Outcome::from_team_scores(
                &self.players,
                &self.format,
                &self.team_scores(),
            ));
        }
    }

    /// Rounds won, then (outside 1v3) bells rung over the match.
    fn team_scores(&self) -> [u32; MAX_PLAYERS] {
        let mut scores = self.team_rounds.map(|r| r as u32 * 100);
        if self.seeker().is_none() {
            for seat in self.hiders() {
                scores[self.team(seat) as usize] += self.seat(seat).total_bells;
            }
        }
        scores
    }
}

impl Minigame for LanternMasquerade {
    type State = State;

    const ID: &'static str = "lantern-masquerade";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[
        FormatKind::FreeForAll,
        FormatKind::OneVsThree,
        FormatKind::TwoVsTwo,
        FormatKind::Duel,
    ];
    const MAX_TICKS: u32 =
        INTRO_TICKS as u32 + MAX_ROUNDS as u32 * (ROUND_TICKS + FINAL_RESULT_TICKS) as u32 + 60;

    fn new(seed: u64, players: PlayerSetup, format: Format) -> State {
        let mut seats = [Player {
            playing: false,
            seeker: false,
            folk: 0,
            out: false,
            bells: 0,
            total_bells: 0,
            cooldown: 0,
            pull: 0,
        }; MAX_PLAYERS];
        for seat in players.seats() {
            let player = &mut seats[seat.index()];
            // Seats outside a duel sit this one out.
            player.playing = format.team(seat).is_some();
            player.seeker = matches!(format, Format::OneVsThree { solo } if solo == seat);
        }
        let mut state = State {
            seed,
            players,
            format,
            phase: Phase::Intro { t: 0 },
            rng: Rng::new(seed),
            tick: 0,
            round: 0,
            folk: [Folk {
                pos: Vec2::ZERO,
                dir: 0,
                moving: false,
                segment: 0,
                segments: 0,
                plan: 0,
                down: 0,
                out: false,
                struck_at: None,
                hit_at: None,
                cleared: false,
                clue: Clue::default(),
            }; FOLK],
            owner: [None; FOLK],
            seats,
            bells: [Bell::default(); BELLS],
            sight: Sight {
                pos: Vec2::ZERO,
                dark: 0,
                caught_at: None,
                missed_at: None,
            },
            team_rounds: [0; MAX_PLAYERS],
            round_winners: 0,
            ended_by: None,
            outcome: None,
        };
        state.start_round();
        state
    }

    fn step(state: &mut State, inputs: &Inputs) {
        match state.phase {
            Phase::Intro { t } => {
                state.phase = if t + 1 >= INTRO_TICKS {
                    Phase::Play { t: 0 }
                } else {
                    Phase::Intro { t: t + 1 }
                };
            }
            Phase::Play { t } => {
                state.tick += 1;
                let standing_before = mask(state.standing());
                state.step_play(inputs, t);
                state.phase = match state.round_over(t, standing_before) {
                    Some((winners, ending)) => {
                        state.end_round(winners, ending);
                        Phase::Result { t: 0 }
                    }
                    None => Phase::Play { t: t + 1 },
                };
            }
            Phase::Result { t } => {
                state.tick += 1;
                let length = if state.outcome.is_some() {
                    FINAL_RESULT_TICKS
                } else {
                    RESULT_TICKS
                };
                state.phase = if t + 1 < length {
                    Phase::Result { t: t + 1 }
                } else if state.outcome.is_some() {
                    Phase::Finished
                } else {
                    state.round += 1;
                    state.start_round();
                    Phase::Play { t: 0 }
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
        let player = state.seat(seat);
        let Phase::Play { t } = state.phase else {
            return Input::default();
        };
        if !player.playing || player.out {
            return Input::default();
        }
        bot::input(state, seat, difficulty, t)
    }
}

#[cfg(test)]
mod tests;
