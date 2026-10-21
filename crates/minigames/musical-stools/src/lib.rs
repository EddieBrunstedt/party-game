//! Musical Stools: musical chairs without anyone sitting out. Glowing stools stand on a turntable
//! in the middle, behind a low fence, one fewer than there are dancers. While the music plays the
//! dancers circle the fence and the turntable spins; A dashes, shoving a rival out of the way.
//! When the music stops the fence drops and the turntable stops, and the first dancer to reach a
//! free stool sits on it. A seat is a point, and whoever sat first that round (everyone who sat on
//! that tick) gets [`FIRST_BONUS`] more. After [`ROUNDS`] rounds the most points wins. Each round
//! the turntable spins faster, the music plays for less time, and the slowdown that warns of the
//! stop gets shorter until it's gone.
//!
//! In 1v3 the solo is the cat: bigger, starting inside the fence, and its dash recharges faster.
//! The cat can't sit and the stools are solid to it. Once the music stops, a runner the cat
//! dashes into is caught and out of the round. The three share [`CAT_STOOLS`] stools and take
//! the round if they fill them within [`CAT_SCRAMBLE_TICKS`] of the stop; the cat takes it once
//! too few runners are left to fill them, or when time runs out. First to
//! [`CAT_ROUNDS_TO_WIN`] rounds wins.
//!
//! The floor uses `sim_core::physics`' plane: `x` to the right, `y` towards the camera.

use std::hash::{Hash, Hasher};

use sim_core::hash::hash_f32;
use sim_core::math;
use sim_core::physics::{self, Body, DT, Vec2};
use sim_core::{
    Button, Difficulty, Format, FormatKind, Input, Inputs, MAX_PLAYERS, Minigame, Outcome,
    PlayerSetup, Rng, Seat,
};

/// The opening countdown, the same in every minigame.
pub const INTRO_TICKS: u16 = sim_core::COUNTDOWN_TICKS;
pub const ROUNDS: u8 = 7;
/// 1v3 is best of five.
pub const CAT_ROUNDS: u8 = 5;
pub const CAT_ROUNDS_TO_WIN: u8 = 3;
pub const CAT_STOOLS: usize = 2;
pub const MAX_STOOLS: usize = MAX_PLAYERS - 1;
/// Extra points for everyone who sat on the round's first sitting tick.
pub const FIRST_BONUS: u32 = 1;

/// The dance floor's outer wall.
pub const FLOOR_RADIUS: f32 = 6.0;
/// The fence around the turntable while the music plays.
pub const FENCE_RADIUS: f32 = 3.0;
/// Stool centres sit this far from the middle.
pub const STOOL_RING: f32 = 1.6;
/// How solid a stool is to the cat.
pub const STOOL_RADIUS: f32 = 0.4;
/// A dancer sits once their centre is this close to a free stool's.
pub const SIT_REACH: f32 = 0.5;

pub const RADIUS: f32 = 0.5;
pub const CAT_RADIUS: f32 = 0.62;
pub const CAT_MASS: f32 = 1.5;
pub const MOVE_SPEED: f32 = 5.0;
pub const ACCEL: f32 = 32.0;
pub const DRAG: f32 = 3.2;
pub const DASH_SPEED: f32 = 12.0;
pub const DASH_TICKS: u16 = 10;
pub const DASH_COOLDOWN: u16 = 60;
/// The cat pounces again sooner.
pub const CAT_DASH_COOLDOWN: u16 = 30;
/// Extra speed a dash hit adds to its victim, on top of the momentum it hands over.
pub const KNOCKBACK: f32 = 5.5;
pub const CAT_KNOCKBACK: f32 = 8.0;
pub const STUN_TICKS: u16 = 20;
/// Seated dancers are immovable; this is their mass in collisions.
const SEATED_MASS: f32 = 1.0e6;
const RESTITUTION: f32 = 0.3;

/// Between rounds the fence rises again (pushing everyone back out) over this long.
pub const REGROUP_TICKS: u16 = 60;
/// The turntable's speed in the first and the last round, in radians a second.
pub const SPIN_SPEED: (f32, f32) = (0.5, 1.1);
/// How long the music plays, shortest to longest, in the first round...
pub const DANCE_TICKS_FIRST: (u16, u16) = (180, 420);
/// ...and in the last.
pub const DANCE_TICKS_LAST: (u16, u16) = (120, 288);
/// The music slows down this long before it stops in the first round, and not at all in the last.
pub const WARN_TICKS_FIRST: u16 = 40;
/// The scramble ends this long after the stop, even with stools still free.
pub const SCRAMBLE_TICKS: u16 = 240;
/// 1v3: the three must fill both stools within this long of the stop.
pub const CAT_SCRAMBLE_TICKS: u16 = 150;
/// The round's result shows this long before the next one (the last one's a little longer).
pub const RESULT_TICKS: u16 = 90;
pub const FINAL_RESULT_TICKS: u16 = 120;

pub struct MusicalStools;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Intro {
        t: u16,
    },
    /// The fence rises again and the turntable waits.
    Regroup {
        t: u16,
    },
    /// The music plays; it stops when `t` reaches the round's `dance_ticks`.
    Dancing {
        t: u16,
    },
    /// The music has stopped: the fence is down and free stools can be taken.
    Scramble {
        t: u16,
    },
    /// The round is scored.
    Result {
        t: u16,
    },
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct Dancer {
    pub body: Body,
    /// Unit vector the dancer faces: their last stick direction.
    pub facing: Vec2,
    /// False for seats outside a duel.
    pub playing: bool,
    pub cat: bool,
    /// Ticks left in the current dash.
    pub dash: u16,
    pub dash_cooldown: u16,
    /// Ticks left reeling from a hit, unable to steer.
    pub stun: u16,
    /// Play tick of the last time this dancer was knocked back, for hit effects.
    pub last_hit: Option<u32>,
    pub last_hit_by: Option<Seat>,
    /// The stool this dancer sits on, this round.
    pub stool: Option<u8>,
    /// Play tick at which they sat, this round.
    pub sat_at: Option<u32>,
    pub points: u32,
    /// Points scored in the last scored round, for the view.
    pub gained: u32,
    /// 1v3: caught by the cat's dash, and out of this round.
    pub caught: bool,
}

impl Dancer {
    pub fn seated(&self) -> bool {
        self.stool.is_some()
    }

    /// Free to move about: playing, not sitting, not caught.
    pub fn moving(&self) -> bool {
        self.playing && !self.seated() && !self.caught
    }
}

/// 1v3's sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Cat,
    Trio,
}

#[derive(Debug, Clone, Hash)]
pub struct State {
    pub seed: u64,
    pub players: PlayerSetup,
    pub format: Format,
    pub phase: Phase,
    rng: Rng,
    /// Ticks since the intro ended, for effects and bots.
    pub tick: u32,
    pub dancers: [Dancer; MAX_PLAYERS],
    /// The round being played, from 0.
    pub round: u8,
    pub rounds: u8,
    pub stool_count: usize,
    /// Who sits on each stool this round.
    pub stools: [Option<Seat>; MAX_STOOLS],
    pub turntable: Turntable,
    /// How long the music plays this round, and for how long before the stop it slows down.
    pub dance_ticks: u16,
    pub warn_ticks: u16,
    /// The play tick the music stopped this round.
    pub stopped_at: Option<u32>,
    /// 1v3: rounds won by each side, and who took the last scored round.
    pub cat_rounds: u8,
    pub trio_rounds: u8,
    pub round_winner: Option<Side>,
    /// Set once the last round is scored.
    pub outcome: Option<Outcome>,
}

/// The turntable's angle, and its speed this round in radians a tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Turntable {
    pub angle: f32,
    pub speed: f32,
}

impl Hash for Turntable {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_f32(self.angle, state);
        hash_f32(self.speed, state);
    }
}

/// Where stool `index` of `count` stands when the turntable is at angle `spin`.
pub fn stool_position(spin: f32, index: usize, count: usize) -> Vec2 {
    Vec2::from_angle(spin + std::f32::consts::TAU * index as f32 / count as f32) * STOOL_RING
}

impl State {
    pub fn dancer(&self, seat: Seat) -> &Dancer {
        &self.dancers[seat.index()]
    }

    pub fn cat(&self) -> Option<Seat> {
        match self.format {
            Format::OneVsThree { solo } => Some(solo),
            _ => None,
        }
    }

    pub fn stool(&self, index: usize) -> Vec2 {
        stool_position(self.turntable.angle, index, self.stool_count)
    }

    /// Seats in the game (benched duel seats aren't).
    pub fn playing(&self) -> impl Iterator<Item = Seat> + '_ {
        self.players
            .seats()
            .filter(|s| self.dancers[s.index()].playing)
    }

    /// Seats that can sit: everyone playing except the cat.
    pub fn sitters(&self) -> impl Iterator<Item = Seat> + '_ {
        self.playing().filter(|s| !self.dancers[s.index()].cat)
    }

    /// The fence's radius, or `None` while it's down.
    pub fn fence(&self) -> Option<f32> {
        match self.phase {
            Phase::Intro { .. } | Phase::Dancing { .. } => Some(FENCE_RADIUS),
            Phase::Regroup { t } => Some(FENCE_RADIUS * (t + 1) as f32 / REGROUP_TICKS as f32),
            Phase::Scramble { .. } | Phase::Result { .. } | Phase::Finished => None,
        }
    }

    pub fn music_playing(&self) -> bool {
        matches!(self.phase, Phase::Dancing { .. })
    }

    /// The music is slowing down: the stop is near.
    pub fn warning(&self) -> bool {
        matches!(self.phase, Phase::Dancing { t } if t + self.warn_ticks >= self.dance_ticks)
    }

    pub fn last_round(&self) -> bool {
        self.round + 1 >= self.rounds
    }

    /// The scramble's time limit this round.
    pub fn scramble_ticks(&self) -> u16 {
        if self.cat().is_some() {
            CAT_SCRAMBLE_TICKS
        } else {
            SCRAMBLE_TICKS
        }
    }

    pub fn free_stools(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.stool_count).filter(|i| self.stools[*i].is_none())
    }

    /// Each team's points (1v3: rounds won by each side).
    fn team_scores(&self) -> [u32; MAX_PLAYERS] {
        let mut scores = [0u32; MAX_PLAYERS];
        if self.cat().is_some() {
            scores[0] = self.cat_rounds as u32;
            scores[1] = self.trio_rounds as u32;
            return scores;
        }
        for seat in self.playing() {
            if let Some(team) = self.format.team(seat) {
                scores[team as usize] += self.dancers[seat.index()].points;
            }
        }
        scores
    }

    /// Draw this round's turntable speed, music length and warning from the seed.
    fn start_round(&mut self) {
        let progress = if self.rounds > 1 {
            self.round as f32 / (self.rounds - 1) as f32
        } else {
            1.0
        };
        let lerp = |a: f32, b: f32| a + (b - a) * progress;
        let speed = lerp(SPIN_SPEED.0, SPIN_SPEED.1) * DT;
        self.turntable.speed = if self.rng.chance(50) { speed } else { -speed };
        let shortest = lerp(DANCE_TICKS_FIRST.0 as f32, DANCE_TICKS_LAST.0 as f32) as u32;
        let longest = lerp(DANCE_TICKS_FIRST.1 as f32, DANCE_TICKS_LAST.1 as f32) as u32;
        self.dance_ticks = self.rng.range_u32(shortest, longest) as u16;
        self.warn_ticks = (WARN_TICKS_FIRST as f32 * (1.0 - progress)) as u16;
        self.stools = [None; MAX_STOOLS];
        self.stopped_at = None;
        for dancer in &mut self.dancers {
            dancer.stool = None;
            dancer.sat_at = None;
            dancer.caught = false;
        }
    }

    fn step_dancers(&mut self, inputs: &Inputs, can_act: bool) {
        for seat in self.playing().collect::<Vec<_>>() {
            let dancer = &mut self.dancers[seat.index()];
            dancer.dash = dancer.dash.saturating_sub(1);
            dancer.dash_cooldown = dancer.dash_cooldown.saturating_sub(1);
            dancer.stun = dancer.stun.saturating_sub(1);
            if dancer.seated() {
                continue;
            }
            let acts = can_act && !dancer.caught;
            let stick = if acts {
                Vec2::from_stick(&inputs.current[seat.index()])
            } else {
                Vec2::ZERO
            };
            if stick != Vec2::ZERO && dancer.stun == 0 {
                dancer.facing = stick.normalize_or_zero();
            }
            let free = dancer.stun == 0 && dancer.dash == 0;
            if acts && free && dancer.dash_cooldown == 0 && inputs.just_pressed(seat, Button::A) {
                dancer.dash = DASH_TICKS;
                dancer.dash_cooldown = if dancer.cat {
                    CAT_DASH_COOLDOWN
                } else {
                    DASH_COOLDOWN
                };
                dancer.body.vel = dancer.facing * DASH_SPEED;
            }
            if dancer.stun == 0 && dancer.dash == 0 {
                dancer.body.steer(stick, MOVE_SPEED, ACCEL);
            }
            let drag = if dancer.dash > 0 { 0.0 } else { DRAG };
            dancer.body.integrate(drag);
        }
        self.collide();
        self.confine();
    }

    fn collide(&mut self) {
        // The caught are out of the way: they drift off where the cat sent them.
        let seats: Vec<Seat> = self
            .playing()
            .filter(|s| !self.dancers[s.index()].caught)
            .collect();
        let mut bodies: Vec<Body> = seats
            .iter()
            .map(|s| {
                let dancer = &self.dancers[s.index()];
                if dancer.seated() {
                    Body {
                        mass: SEATED_MASS,
                        vel: Vec2::ZERO,
                        ..dancer.body
                    }
                } else {
                    dancer.body
                }
            })
            .collect();
        let mut contacts = Vec::new();
        physics::collide_all(&mut bodies, RESTITUTION, |i, j, contact| {
            contacts.push((i, j, contact.normal))
        });
        for (seat, body) in seats.iter().zip(&bodies) {
            let dancer = &mut self.dancers[seat.index()];
            if !dancer.seated() {
                dancer.body.pos = body.pos;
                dancer.body.vel = body.vel;
            }
        }
        for (i, j, normal) in contacts {
            let (a, b) = (seats[i], seats[j]);
            // A dash only counts if it was heading into the other dancer.
            let a_hits = self.dancers[a.index()].dash > 0
                && self.dancers[a.index()].facing.dot(normal) > 0.3;
            let b_hits = self.dancers[b.index()].dash > 0
                && self.dancers[b.index()].facing.dot(-normal) > 0.3;
            if a_hits {
                self.dash_hit(a, b, normal);
            }
            if b_hits {
                self.dash_hit(b, a, -normal);
            }
        }
    }

    /// `attacker` dashed into `victim`; `normal` points from attacker to victim. Seated dancers
    /// and teammates aren't shoved; the dash just stops.
    fn dash_hit(&mut self, attacker: Seat, victim: Seat, normal: Vec2) {
        let tick = self.tick;
        let knockback = if self.dancers[attacker.index()].cat {
            CAT_KNOCKBACK
        } else {
            KNOCKBACK
        };
        self.dancers[attacker.index()].dash = 0;
        if self.dancers[victim.index()].seated() || self.format.same_team(attacker, victim) {
            return;
        }
        let catches =
            self.dancers[attacker.index()].cat && matches!(self.phase, Phase::Scramble { .. });
        let dancer = &mut self.dancers[victim.index()];
        dancer.caught |= catches;
        dancer.body.vel += normal * (knockback / dancer.body.mass);
        dancer.stun = STUN_TICKS;
        dancer.dash = 0;
        dancer.last_hit = Some(tick);
        dancer.last_hit_by = Some(attacker);
    }

    /// Keep everyone on the floor, the dancers outside the fence and the cat inside it, and the
    /// cat off the stools.
    fn confine(&mut self) {
        let fence = self.fence();
        let stools: Vec<Vec2> = (0..self.stool_count).map(|i| self.stool(i)).collect();
        let cat_limit = match self.phase {
            // While the fence rises the cat is drawn back in with it.
            Phase::Regroup { t } => Some(
                FLOOR_RADIUS
                    + (FENCE_RADIUS - FLOOR_RADIUS) * (t + 1) as f32 / REGROUP_TICKS as f32,
            ),
            _ => fence,
        };
        for seat in self.playing().collect::<Vec<_>>() {
            let dancer = &mut self.dancers[seat.index()];
            if dancer.seated() {
                continue;
            }
            let r = dancer.body.radius;
            keep_within(&mut dancer.body, FLOOR_RADIUS - r);
            if dancer.cat {
                if let Some(limit) = cat_limit {
                    keep_within(&mut dancer.body, limit - r);
                }
                for stool in &stools {
                    keep_out_of(&mut dancer.body, *stool, STOOL_RADIUS + r, dancer.facing);
                }
            } else if let Some(fence) = fence {
                keep_out_of(&mut dancer.body, Vec2::ZERO, fence + r, dancer.facing);
            }
        }
    }

    /// Free stools take the nearest dancer in reach.
    fn take_stools(&mut self) {
        for stool in 0..self.stool_count {
            if self.stools[stool].is_some() {
                continue;
            }
            let at = self.stool(stool);
            let mut nearest: Vec<(f32, Seat)> = self
                .sitters()
                .filter(|s| self.dancers[s.index()].moving())
                .map(|s| (self.dancers[s.index()].body.pos.distance(at), s))
                .filter(|(d, _)| *d <= SIT_REACH)
                .collect();
            nearest.sort_by(|a, b| a.0.total_cmp(&b.0));
            let Some(&(best, _)) = nearest.first() else {
                continue;
            };
            // Exactly level dancers get the stool by lot, so no seat wins ties.
            nearest.retain(|(d, _)| *d == best);
            let pick = self.rng.range_u32(0, nearest.len() as u32 - 1) as usize;
            let seat = nearest[pick].1;
            self.stools[stool] = Some(seat);
            let dancer = &mut self.dancers[seat.index()];
            dancer.stool = Some(stool as u8);
            dancer.sat_at = Some(self.tick);
            dancer.body.pos = at;
            dancer.body.vel = Vec2::ZERO;
            dancer.dash = 0;
            dancer.stun = 0;
        }
    }

    fn score_round(&mut self) {
        let first = self.dancers.iter().filter_map(|d| d.sat_at).min();
        for dancer in &mut self.dancers {
            dancer.gained = 0;
            if dancer.cat || !dancer.playing {
                continue;
            }
            if let Some(at) = dancer.sat_at {
                dancer.gained = 1 + if Some(at) == first { FIRST_BONUS } else { 0 };
            }
            dancer.points += dancer.gained;
        }
        if self.cat().is_some() {
            let side = if self.free_stools().next().is_none() {
                self.trio_rounds += 1;
                Side::Trio
            } else {
                self.cat_rounds += 1;
                Side::Cat
            };
            self.round_winner = Some(side);
        }
        let decided = self.cat_rounds >= CAT_ROUNDS_TO_WIN || self.trio_rounds >= CAT_ROUNDS_TO_WIN;
        if self.last_round() || decided {
            self.rounds = self.round + 1;
            self.outcome = Some(Outcome::from_team_scores(
                &self.players,
                &self.format,
                &self.team_scores(),
            ));
        }
    }
}

/// Keep a body's centre within `limit` of the middle, stopping any outward speed.
fn keep_within(body: &mut Body, limit: f32) {
    let distance = body.pos.length();
    if distance > limit {
        let out = body.pos * (1.0 / distance);
        body.pos = out * limit;
        let speed = body.vel.dot(out);
        if speed > 0.0 {
            body.vel -= out * speed;
        }
    }
}

/// Keep a body's centre at least `limit` from `centre`, stopping any speed towards it. A body
/// right on the centre is pushed out along `fallback`.
fn keep_out_of(body: &mut Body, centre: Vec2, limit: f32, fallback: Vec2) {
    let offset = body.pos - centre;
    let distance = offset.length();
    if distance < limit {
        let out = if distance > 1e-6 {
            offset * (1.0 / distance)
        } else if fallback != Vec2::ZERO {
            fallback
        } else {
            Vec2::new(1.0, 0.0)
        };
        body.pos = centre + out * limit;
        let speed = body.vel.dot(out);
        if speed < 0.0 {
            body.vel -= out * speed;
        }
    }
}

fn start_position(index: usize, count: usize) -> Vec2 {
    // Spread evenly round the fence, the first seat nearest the camera.
    let angle = std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * index as f32 / count as f32;
    Vec2::from_angle(angle) * (FENCE_RADIUS + 1.2)
}

impl Minigame for MusicalStools {
    type State = State;

    const ID: &'static str = "musical-stools";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[
        FormatKind::FreeForAll,
        FormatKind::OneVsThree,
        FormatKind::TwoVsTwo,
        FormatKind::Duel,
    ];
    const MAX_TICKS: u32 = INTRO_TICKS as u32
        + ROUNDS as u32
            * (REGROUP_TICKS + DANCE_TICKS_FIRST.1 + SCRAMBLE_TICKS + FINAL_RESULT_TICKS) as u32
        + 60;

    fn new(seed: u64, players: PlayerSetup, format: Format) -> State {
        let mut dancers = [Dancer {
            body: Body::new(Vec2::ZERO, RADIUS, 1.0),
            facing: Vec2::new(0.0, -1.0),
            playing: false,
            cat: false,
            dash: 0,
            dash_cooldown: 0,
            stun: 0,
            last_hit: None,
            last_hit_by: None,
            stool: None,
            sat_at: None,
            points: 0,
            gained: 0,
            caught: false,
        }; MAX_PLAYERS];
        let cat = match format {
            Format::OneVsThree { solo } => Some(solo),
            _ => None,
        };
        // Seats outside a duel sit this one out.
        let playing: Vec<Seat> = players
            .seats()
            .filter(|s| format.team(*s).is_some())
            .collect();
        let circlers: Vec<Seat> = playing
            .iter()
            .copied()
            .filter(|s| Some(*s) != cat)
            .collect();
        for (i, seat) in circlers.iter().enumerate() {
            let dancer = &mut dancers[seat.index()];
            let pos = start_position(i, circlers.len());
            dancer.body = Body::new(pos, RADIUS, 1.0);
            dancer.facing = (-pos).normalize_or_zero();
            dancer.playing = true;
        }
        if let Some(solo) = cat {
            let dancer = &mut dancers[solo.index()];
            dancer.body = Body::new(Vec2::ZERO, CAT_RADIUS, CAT_MASS);
            dancer.playing = true;
            dancer.cat = true;
        }
        let stool_count = if cat.is_some() {
            CAT_STOOLS
        } else {
            circlers.len().saturating_sub(1).max(1)
        };
        let mut state = State {
            seed,
            players,
            format,
            phase: Phase::Intro { t: 0 },
            rng: Rng::new(seed),
            tick: 0,
            dancers,
            round: 0,
            rounds: if cat.is_some() { CAT_ROUNDS } else { ROUNDS },
            stool_count,
            stools: [None; MAX_STOOLS],
            turntable: Turntable {
                angle: 0.0,
                speed: 0.0,
            },
            dance_ticks: 0,
            warn_ticks: 0,
            stopped_at: None,
            cat_rounds: 0,
            trio_rounds: 0,
            round_winner: None,
            outcome: None,
        };
        state.turntable.angle = state.rng.range_u32(0, 6283) as f32 / 1000.0;
        state.start_round();
        state
    }

    fn step(state: &mut State, inputs: &Inputs) {
        if let Phase::Intro { t } = state.phase {
            state.phase = if t + 1 >= INTRO_TICKS {
                Phase::Dancing { t: 0 }
            } else {
                Phase::Intro { t: t + 1 }
            };
            return;
        }
        if state.phase == Phase::Finished {
            return;
        }
        state.tick += 1;
        match state.phase {
            Phase::Regroup { t } => {
                state.step_dancers(inputs, true);
                state.phase = if t + 1 >= REGROUP_TICKS {
                    Phase::Dancing { t: 0 }
                } else {
                    Phase::Regroup { t: t + 1 }
                };
            }
            Phase::Dancing { t } => {
                state.turntable.angle += state.turntable.speed;
                state.step_dancers(inputs, true);
                state.phase = if t + 1 >= state.dance_ticks {
                    state.stopped_at = Some(state.tick);
                    Phase::Scramble { t: 0 }
                } else {
                    Phase::Dancing { t: t + 1 }
                };
            }
            Phase::Scramble { t } => {
                state.step_dancers(inputs, true);
                state.take_stools();
                let free = state.free_stools().count();
                let able = state
                    .sitters()
                    .filter(|s| state.dancer(*s).moving())
                    .count();
                state.phase = if free == 0 || free > able || t + 1 >= state.scramble_ticks() {
                    state.score_round();
                    Phase::Result { t: 0 }
                } else {
                    Phase::Scramble { t: t + 1 }
                };
            }
            Phase::Result { t } => {
                state.step_dancers(inputs, false);
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
                    Phase::Regroup { t: 0 }
                };
            }
            Phase::Intro { .. } | Phase::Finished => unreachable!(),
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
        let dancer = state.dancer(seat);
        if !dancer.moving() || dancer.stun > 0 {
            return Input::default();
        }
        match state.phase {
            Phase::Regroup { .. } | Phase::Dancing { .. } | Phase::Scramble { .. } => {
                bot::input(state, seat, difficulty)
            }
            _ => Input::default(),
        }
    }
}

mod bot {
    use super::*;

    /// How far round the fence from the cat runners keep while dancing, in radians.
    const KEEP_AWAY: f32 = 0.9;
    /// The berth runners give the cat in the scramble, in floor units beyond touching.
    const BERTH: f32 = 0.35;
    /// A stool the cat stands this close to is guarded...
    const GUARD_RANGE: f32 = 1.6;
    /// ...so runners hold off this far from it until the cat turns away.
    const HOLD_RADIUS: f32 = 2.0;
    /// How close the cat gets before it pounces.
    const POUNCE_REACH: f32 = 2.6;

    struct Skill {
        /// Ticks before it notices the music has stopped.
        reaction: u16,
        /// Ticks between fresh decisions (noise and dash rolls are redrawn each window).
        window: u32,
        /// How far off its stool it drifts while dancing, in radians either way.
        drift: f32,
        /// Aim error for dashes, in radians either way.
        aim_noise: f32,
        /// Chance per decision window of shoving a rival who has its spot.
        shove_percent: u32,
        /// Chance per decision window of dashing for a stool in the scramble.
        dash_percent: u32,
        /// How much of a runner's movement the cat allows for while its pounce flies.
        cat_lead: f32,
        /// The cat pounces on a runner this close to its stool...
        pounce_reach: f32,
        /// ...once it has noticed the stop, this long after it.
        cat_reaction: u16,
        /// Keeps its dash for the stop once the music slows down.
        saves_dash: bool,
        /// It goes for a guarded stool once the cat faces this far away from it (the cosine of
        /// the angle round the stool between them).
        nerve: f32,
    }

    fn skill(difficulty: Difficulty) -> Skill {
        match difficulty {
            Difficulty::Easy => Skill {
                reaction: 19,
                window: 30,
                drift: 0.45,
                aim_noise: 0.35,
                shove_percent: 20,
                dash_percent: 40,
                cat_lead: 0.5,
                pounce_reach: 2.4,
                cat_reaction: 40,
                saves_dash: false,
                nerve: 0.26,
            },
            Difficulty::Normal => Skill {
                reaction: 15,
                window: 20,
                drift: 0.2,
                aim_noise: 0.18,
                shove_percent: 45,
                dash_percent: 75,
                cat_lead: 0.7,
                pounce_reach: 2.1,
                cat_reaction: 15,
                saves_dash: false,
                nerve: -0.09,
            },
            Difficulty::Hard => Skill {
                reaction: 11,
                window: 12,
                drift: 0.06,
                aim_noise: 0.06,
                shove_percent: 65,
                dash_percent: 95,
                cat_lead: 0.85,
                pounce_reach: 1.4,
                cat_reaction: 11,
                saves_dash: true,
                nerve: -0.34,
            },
        }
    }

    struct Roll {
        drift: f32,
        aim: f32,
        shove: bool,
        dash: bool,
    }

    fn roll(state: &State, seat: Seat, skill: &Skill) -> Roll {
        let window = state.tick / skill.window;
        let mut rng = Rng::new(
            state.seed
                ^ (window as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
                ^ (seat.0 as u64 + 1).wrapping_mul(0xbf58_476d_1ce4_e5b9),
        );
        let mut signed = || rng.range_u32(0, 2000) as f32 / 1000.0 - 1.0;
        let drift = signed() * skill.drift;
        let aim = signed() * skill.aim_noise;
        Roll {
            drift,
            aim,
            shove: rng.chance(skill.shove_percent),
            dash: rng.chance(skill.dash_percent),
        }
    }

    fn stick(direction: Vec2) -> Input {
        let d = direction.clamp_length(1.0);
        Input {
            buttons: 0,
            stick_x: Input::quantize_axis(d.x),
            stick_y: Input::quantize_axis(-d.y),
        }
    }

    fn dash(direction: Vec2) -> Input {
        stick(direction.normalize_or_zero()).with(Button::A)
    }

    fn rotate(v: Vec2, angle: f32) -> Vec2 {
        let (s, c) = (math::sin(angle), math::cos(angle));
        Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
    }

    /// Head for `target`, arriving with `target_vel` (both in floor units).
    fn seek(pos: Vec2, target: Vec2, target_vel: Vec2) -> Input {
        stick((target - pos) * (3.0 / MOVE_SPEED) + target_vel * (1.0 / MOVE_SPEED))
    }

    pub fn input(state: &State, seat: Seat, difficulty: Difficulty) -> Input {
        let skill = skill(difficulty);
        let roll = roll(state, seat, &skill);
        if state.dancer(seat).cat {
            return cat(state, seat, &skill, &roll);
        }
        match state.phase {
            Phase::Scramble { t } if t >= skill.reaction => scramble(state, seat, &skill, &roll),
            _ => dance(state, seat, &skill, &roll),
        }
    }

    /// The spot just outside the fence level with stool `index`.
    fn spot(state: &State, index: usize) -> Vec2 {
        state.stool(index).normalize_or_zero() * (FENCE_RADIUS + RADIUS + 0.05)
    }

    /// The stool whose spot this seat is best placed for: the nearest one no nearer rival has
    /// already taken as theirs, or simply the nearest if every one is taken.
    fn dance_target(state: &State, seat: Seat) -> usize {
        let nearest_spot = |s: Seat| {
            let pos = state.dancer(s).body.pos;
            (0..state.stool_count)
                .min_by(|a, b| {
                    pos.distance(spot(state, *a))
                        .total_cmp(&pos.distance(spot(state, *b)))
                })
                .unwrap_or(0)
        };
        let pos = state.dancer(seat).body.pos;
        let mut order: Vec<usize> = (0..state.stool_count).collect();
        order.sort_by(|a, b| {
            pos.distance(spot(state, *a))
                .total_cmp(&pos.distance(spot(state, *b)))
        });
        order
            .iter()
            .copied()
            .find(|&i| {
                let mine = pos.distance(spot(state, i));
                !state.sitters().any(|other| {
                    other != seat
                        && nearest_spot(other) == i
                        && state.dancer(other).body.pos.distance(spot(state, i)) < mine
                })
            })
            .unwrap_or(order[0])
    }

    fn dance(state: &State, seat: Seat, skill: &Skill, roll: &Roll) -> Input {
        let me = state.dancer(seat);
        let pos = me.body.pos;
        let target = dance_target(state, seat);
        // Walk with the stool, a little ahead of where it is now.
        let lead = state.turntable.speed * 8.0;
        let mut spot = rotate(spot(state, target), lead + roll.drift);
        if let Some(cat) = state.cat() {
            // Keep away from the cat's side of the fence, or it pounces as the fence drops.
            let cat_pos = state.dancer(cat).body.pos;
            let gap = math::atan2(cat_pos.perp_dot(spot), cat_pos.dot(spot));
            if gap.abs() < KEEP_AWAY {
                let away = if gap < 0.0 { -1.0 } else { 1.0 };
                spot = rotate(spot, away * (KEEP_AWAY - gap.abs()));
            }
        }
        let spot_vel = spot.perp() * (state.turntable.speed / DT);
        if state.music_playing()
            && roll.shove
            && me.dash_cooldown == 0
            && !(skill.saves_dash && state.warning())
        {
            // Shove whoever stands on our spot.
            let rival = state.sitters().find(|&other| {
                other != seat
                    && !state.format.same_team(seat, other)
                    && state.dancer(other).body.pos.distance(spot) < 0.9
                    && state.dancer(other).body.pos.distance(pos) < 1.6
            });
            if let Some(rival) = rival {
                let toward = (state.dancer(rival).body.pos - pos).normalize_or_zero();
                return dash(rotate(toward, roll.aim));
            }
        }
        seek(pos, spot, spot_vel)
    }

    /// How far a seat has to go to reach `at`.
    fn reach(state: &State, seat: Seat, at: Vec2) -> f32 {
        state.dancer(seat).body.pos.distance(at)
    }

    fn scramble(state: &State, seat: Seat, skill: &Skill, roll: &Roll) -> Input {
        let me = state.dancer(seat);
        let pos = me.body.pos;
        let cat = state.cat().map(|c| state.dancer(c).body.pos);
        // The free stool we're furthest ahead of everyone else for (or least behind), counting
        // a stool the cat is nearer to as further.
        let target = state.free_stools().min_by(|&a, &b| {
            let lead = |i: usize| {
                let at = state.stool(i);
                let rival = state
                    .sitters()
                    .filter(|o| *o != seat && state.dancer(*o).moving())
                    .map(|o| reach(state, o, at))
                    .fold(f32::MAX, f32::min);
                let mine = reach(state, seat, at);
                let guarded = cat.is_some_and(|c| c.distance(at) < mine);
                mine - rival + if guarded { 1.5 } else { 0.0 }
            };
            lead(a).total_cmp(&lead(b)).then(a.cmp(&b))
        });
        let Some(target) = target else {
            return Input::default();
        };
        let at = state.stool(target);
        let mut toward = (at - pos).normalize_or_zero();
        if let Some(cat_pos) = cat {
            if cat_pos.distance(at) < GUARD_RANGE {
                return stick(attack(state, seat, skill, at, cat_pos));
            }
            toward = steer_clear(pos, at, cat_pos);
        }
        let distance = pos.distance(at);
        let dash_reach = DASH_SPEED * DASH_TICKS as f32 * DT;
        // A dash can't turn, so runners don't dash with the cat near.
        let cat_near = cat.is_some_and(|c| c.distance(pos) < 3.0);
        if roll.dash
            && !cat_near
            && me.dash_cooldown == 0
            && me.facing.dot(toward) > 0.9
            && distance > dash_reach * 0.6
            && distance < dash_reach * 1.4
        {
            return dash(rotate(toward, roll.aim));
        }
        stick(toward)
    }

    /// The way to `target` that keeps clear of the cat: round it when it stands in the way, and
    /// back off when it gets close, out of easy pouncing range.
    fn steer_clear(pos: Vec2, target: Vec2, cat: Vec2) -> Vec2 {
        let mut toward = (target - pos).normalize_or_zero();
        let to_cat = cat - pos;
        let along = to_cat.dot(toward);
        let aside = toward.perp_dot(to_cat);
        let clear = CAT_RADIUS + RADIUS + BERTH;
        if along > 0.0 && along < pos.distance(target) && aside.abs() < clear {
            let side = if aside > 0.0 { -1.0 } else { 1.0 };
            let beside = cat + toward.perp() * (side * clear);
            toward = (beside - pos).normalize_or_zero();
        }
        let gap = to_cat.length() - (CAT_RADIUS + RADIUS);
        let berth = BERTH * 2.5;
        if gap < berth {
            let push = 1.5 * (1.0 - gap.max(0.0) / berth);
            toward = (toward - to_cat.normalize_or_zero() * push).normalize_or_zero();
        }
        toward
    }

    /// A cat keeping goal on the stool at `at` can face only one way, so runners hold off on
    /// the side opposite a teammate (or the cat) and go in when the cat is turned away.
    fn attack(state: &State, seat: Seat, skill: &Skill, at: Vec2, cat: Vec2) -> Vec2 {
        let pos = state.dancer(seat).body.pos;
        let cat_side = (cat - at).normalize_or_zero();
        if (pos - at).normalize_or_zero().dot(cat_side) < skill.nerve {
            return steer_clear(pos, at, cat);
        }
        let partner = state
            .sitters()
            .filter(|o| *o != seat && state.dancer(*o).moving())
            .map(|o| state.dancer(o).body.pos)
            .filter(|p| p.distance(at) < HOLD_RADIUS + 1.5)
            .min_by(|a, b| a.distance(at).total_cmp(&b.distance(at)));
        let opposite = match partner {
            Some(p) => (at - p).normalize_or_zero(),
            None => -cat_side,
        };
        steer_clear(pos, at + opposite * HOLD_RADIUS, cat)
    }

    /// The cat keeps goal: it guards the free stool nearest to it, staying between the stool and
    /// the nearest runner, and pounces on a runner who comes close.
    fn cat(state: &State, seat: Seat, skill: &Skill, roll: &Roll) -> Input {
        let me = state.dancer(seat);
        let pos = me.body.pos;
        let guarded = state.free_stools().min_by(|&a, &b| {
            pos.distance(state.stool(a))
                .total_cmp(&pos.distance(state.stool(b)))
                .then(a.cmp(&b))
        });
        let Some(guarded) = guarded else {
            return Input::default();
        };
        let at = state.stool(guarded);
        let runner = state
            .sitters()
            .filter(|o| state.dancer(*o).moving())
            .min_by(|a, b| {
                let near = |o: Seat| reach(state, o, at);
                near(*a).total_cmp(&near(*b)).then(a.cmp(b))
            });
        let Some(runner) = runner else {
            return Input::default();
        };
        let them = state.dancer(runner);
        let racing = matches!(state.phase, Phase::Scramble { t } if t >= skill.cat_reaction);
        if racing && me.dash_cooldown == 0 && them.body.pos.distance(at) < skill.pounce_reach {
            let flight = them.body.pos.distance(pos) / DASH_SPEED;
            let ahead = them.body.pos + them.body.vel * (flight * skill.cat_lead) - pos;
            if ahead.length() < POUNCE_REACH {
                return dash(rotate(ahead.normalize_or_zero(), roll.aim));
            }
        }
        // Stand against the stool on the runner's side of it.
        let side = rotate((them.body.pos - at).normalize_or_zero(), roll.drift * 0.5);
        let guard = at + side * (STOOL_RADIUS + CAT_RADIUS + 0.02);
        let guard_vel = if state.music_playing() {
            guard.perp() * (state.turntable.speed / DT)
        } else {
            Vec2::ZERO
        };
        seek(pos, guard, guard_vel)
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

    fn new_run(players: usize, format: Format) -> Run<MusicalStools> {
        Run::new(1, PlayerSetup::first(players), format, RunConfig::default())
    }

    fn run_until(run: &mut Run<MusicalStools>, done: impl Fn(&State) -> bool) {
        for _ in 0..MusicalStools::MAX_TICKS {
            if done(&run.game) {
                return;
            }
            run.step(NONE);
        }
        panic!("never got there: {:?}", run.game.phase);
    }

    fn to_scramble(run: &mut Run<MusicalStools>) {
        run_until(run, |g| matches!(g.phase, Phase::Scramble { .. }));
    }

    fn place(run: &mut Run<MusicalStools>, seat: usize, at: Vec2) {
        run.game.dancers[seat].body.pos = at;
        run.game.dancers[seat].body.vel = Vec2::ZERO;
    }

    #[test]
    fn one_stool_fewer_than_dancers() {
        assert_eq!(new_run(4, Format::FreeForAll).game.stool_count, 3);
        assert_eq!(new_run(3, Format::FreeForAll).game.stool_count, 2);
        assert_eq!(new_run(2, Format::FreeForAll).game.stool_count, 1);
        let duel = new_run(
            4,
            Format::Duel {
                a: Seat(1),
                b: Seat(3),
            },
        );
        assert_eq!(duel.game.stool_count, 1);
        assert!(!duel.game.dancers[0].playing && duel.game.dancers[1].playing);
        let cat = new_run(4, Format::OneVsThree { solo: Seat(2) });
        assert_eq!(cat.game.stool_count, CAT_STOOLS);
        assert!(cat.game.dancers[2].cat);
        assert_eq!(cat.game.dancers[2].body.radius, CAT_RADIUS);
        assert_eq!(cat.game.rounds, CAT_ROUNDS);
    }

    #[test]
    fn nobody_moves_during_the_intro() {
        let mut run = new_run(2, Format::FreeForAll);
        let start = run.game.dancers[0].body.pos;
        let mut push = NONE;
        push[0].stick_x = 127;
        push[0].set(Button::A, true);
        for _ in 0..INTRO_TICKS - 1 {
            run.step(push);
        }
        assert_eq!(run.game.dancers[0].body.pos, start);
    }

    #[test]
    fn the_fence_keeps_dancers_out_while_the_music_plays() {
        let mut run = new_run(2, Format::FreeForAll);
        run_until(&mut run, |g| matches!(g.phase, Phase::Dancing { .. }));
        // Walk straight at the middle.
        let mut inputs = NONE;
        inputs[0].stick_y = 127;
        for _ in 0..60 {
            run.step(inputs);
        }
        assert!(run.game.music_playing());
        let distance = run.game.dancers[0].body.pos.length();
        assert!(distance >= FENCE_RADIUS + RADIUS - 1e-4, "{distance}");
    }

    #[test]
    fn the_turntable_spins_while_the_music_plays_and_stops_with_it() {
        let mut run = new_run(4, Format::FreeForAll);
        run_until(&mut run, |g| matches!(g.phase, Phase::Dancing { .. }));
        let before = run.game.turntable.angle;
        run.step(NONE);
        assert_ne!(run.game.turntable.angle, before);
        to_scramble(&mut run);
        let stopped = run.game.turntable.angle;
        run.step(NONE);
        assert_eq!(run.game.turntable.angle, stopped);
        assert_eq!(run.game.fence(), None);
    }

    #[test]
    fn reaching_a_free_stool_sits_you_and_scores_with_the_bonus() {
        let mut run = new_run(3, Format::FreeForAll);
        to_scramble(&mut run);
        let stool = run.game.stool(0);
        place(&mut run, 0, stool + Vec2::new(0.3, 0.0));
        run.step(NONE);
        assert_eq!(run.game.stools[0], Some(Seat(0)));
        assert_eq!(run.game.dancers[0].body.pos, stool);
        let other = run.game.stool(1);
        place(&mut run, 1, other + Vec2::new(0.0, 0.3));
        run.step(NONE);
        assert_eq!(run.game.stools[1], Some(Seat(1)));
        // Every stool is taken, so the round is scored at once.
        assert!(matches!(run.game.phase, Phase::Result { .. }));
        let gained: Vec<u32> = run.game.dancers[..3].iter().map(|d| d.gained).collect();
        assert_eq!(gained, vec![1 + FIRST_BONUS, 1, 0]);
    }

    #[test]
    fn dancers_sitting_on_the_same_tick_all_get_the_bonus() {
        let mut run = new_run(3, Format::FreeForAll);
        to_scramble(&mut run);
        for seat in 0..2 {
            let stool = run.game.stool(seat);
            place(&mut run, seat, stool);
        }
        run.step(NONE);
        let gained: Vec<u32> = run.game.dancers[..3].iter().map(|d| d.gained).collect();
        assert_eq!(gained, vec![1 + FIRST_BONUS, 1 + FIRST_BONUS, 0]);
    }

    #[test]
    fn the_nearer_dancer_gets_a_contested_stool() {
        let mut run = new_run(3, Format::FreeForAll);
        to_scramble(&mut run);
        let stool = run.game.stool(0);
        place(&mut run, 0, stool + Vec2::new(0.45, 0.0));
        place(&mut run, 1, stool - Vec2::new(0.2, 0.0));
        run.step(NONE);
        assert_eq!(run.game.stools[0], Some(Seat(1)));
    }

    #[test]
    fn a_seated_dancer_cant_be_shoved_off() {
        let mut run = new_run(3, Format::FreeForAll);
        to_scramble(&mut run);
        let stool = run.game.stool(0);
        place(&mut run, 0, stool);
        run.step(NONE);
        place(&mut run, 1, stool + Vec2::new(-1.6, 0.0));
        run.game.dancers[1].facing = Vec2::new(1.0, 0.0);
        let mut inputs = NONE;
        inputs[1].stick_x = 127;
        inputs[1].set(Button::A, true);
        for _ in 0..10 {
            run.step(inputs);
        }
        assert_eq!(run.game.dancers[0].body.pos, stool);
        assert_eq!(run.game.dancers[0].stun, 0);
    }

    #[test]
    fn a_dash_shoves_a_rival() {
        let mut run = new_run(2, Format::FreeForAll);
        run_until(&mut run, |g| matches!(g.phase, Phase::Dancing { .. }));
        place(&mut run, 0, Vec2::new(-1.5, 5.0));
        place(&mut run, 1, Vec2::new(0.0, 5.0));
        run.game.dancers[0].facing = Vec2::new(1.0, 0.0);
        let mut inputs = NONE;
        inputs[0].stick_x = 127;
        inputs[0].set(Button::A, true);
        for _ in 0..8 {
            run.step(inputs);
        }
        let victim = &run.game.dancers[1];
        assert!(victim.stun > 0);
        assert_eq!(victim.last_hit_by, Some(Seat(0)));
        assert!(victim.body.vel.x > KNOCKBACK * 0.8);
    }

    #[test]
    fn teammates_arent_shoved() {
        let mut run = new_run(
            4,
            Format::TwoVsTwo {
                team_of: [0, 0, 1, 1],
            },
        );
        run_until(&mut run, |g| matches!(g.phase, Phase::Dancing { .. }));
        place(&mut run, 0, Vec2::new(-1.5, 5.0));
        place(&mut run, 1, Vec2::new(0.0, 5.0));
        run.game.dancers[0].facing = Vec2::new(1.0, 0.0);
        let mut inputs = NONE;
        inputs[0].stick_x = 127;
        inputs[0].set(Button::A, true);
        for _ in 0..8 {
            run.step(inputs);
        }
        assert_eq!(run.game.dancers[1].stun, 0);
    }

    #[test]
    fn the_fence_rises_and_pushes_the_seated_back_out() {
        let mut run = new_run(2, Format::FreeForAll);
        to_scramble(&mut run);
        let stool = run.game.stool(0);
        place(&mut run, 0, stool);
        run_until(&mut run, |g| matches!(g.phase, Phase::Dancing { .. }));
        assert_eq!(run.game.round, 1);
        assert_eq!(run.game.stools, [None; MAX_STOOLS]);
        assert!(run.game.dancers[0].body.pos.length() >= FENCE_RADIUS + RADIUS - 1e-4);
        assert_eq!(run.game.dancers[0].points, 1 + FIRST_BONUS);
    }

    #[test]
    fn rounds_get_harder() {
        let first = new_run(4, Format::FreeForAll);
        let mut last = new_run(4, Format::FreeForAll);
        last.game.round = ROUNDS - 1;
        last.game.start_round();
        assert_eq!(first.game.warn_ticks, WARN_TICKS_FIRST);
        assert_eq!(last.game.warn_ticks, 0);
        assert!(last.game.turntable.speed.abs() > first.game.turntable.speed.abs() * 2.0);
        assert!(last.game.dance_ticks <= DANCE_TICKS_LAST.1);
    }

    #[test]
    fn the_cat_cant_sit_or_stand_on_a_stool_and_wins_unfilled_rounds() {
        let mut run = new_run(4, Format::OneVsThree { solo: Seat(0) });
        to_scramble(&mut run);
        let stool = run.game.stool(0);
        place(&mut run, 0, stool);
        run.step(NONE);
        assert_eq!(run.game.stools[0], None);
        assert!(run.game.dancers[0].body.pos.distance(stool) >= STOOL_RADIUS + CAT_RADIUS - 1e-4);
        run_until(&mut run, |g| matches!(g.phase, Phase::Result { .. }));
        assert_eq!(run.game.round_winner, Some(Side::Cat));
        assert_eq!(run.game.cat_rounds, 1);
    }

    #[test]
    fn the_trio_wins_a_round_by_filling_both_stools() {
        let mut run = new_run(4, Format::OneVsThree { solo: Seat(3) });
        to_scramble(&mut run);
        for seat in 0..CAT_STOOLS {
            let stool = run.game.stool(seat);
            place(&mut run, seat, stool);
        }
        run.step(NONE);
        assert_eq!(run.game.round_winner, Some(Side::Trio));
        assert_eq!(run.game.trio_rounds, 1);
    }

    #[test]
    fn the_cat_match_ends_once_a_side_has_three() {
        let mut run = new_run(4, Format::OneVsThree { solo: Seat(0) });
        while run.outcome().is_none() {
            run.step(NONE);
        }
        assert_eq!(run.game.cat_rounds, CAT_ROUNDS_TO_WIN);
        assert_eq!(run.game.round, CAT_ROUNDS_TO_WIN - 1);
        assert_eq!(
            run.outcome().unwrap().placement,
            [Some(1), Some(2), Some(2), Some(2)]
        );
    }

    #[test]
    fn most_points_wins_after_the_last_round() {
        let mut run = new_run(3, Format::FreeForAll);
        while run.outcome().is_none() {
            if matches!(run.game.phase, Phase::Scramble { .. }) {
                let stool = run.game.stool(0);
                place(&mut run, 2, stool);
            }
            run.step(NONE);
        }
        assert_eq!(run.game.round, ROUNDS - 1);
        assert_eq!(
            run.game.dancers[2].points,
            ROUNDS as u32 * (1 + FIRST_BONUS)
        );
        assert_eq!(
            run.outcome().unwrap().placement,
            [Some(2), Some(2), Some(1), None]
        );
    }

    #[test]
    fn bots_finish_every_format() {
        use sim_core::registry::play_with_bots;
        let entry = sim_core::MinigameEntry::of::<MusicalStools>();
        for format in [
            Format::FreeForAll,
            Format::OneVsThree { solo: Seat(1) },
            Format::TwoVsTwo {
                team_of: [0, 1, 1, 0],
            },
            Format::Duel {
                a: Seat(0),
                b: Seat(2),
            },
        ] {
            for difficulty in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard] {
                let outcome = play_with_bots(
                    &entry,
                    3,
                    PlayerSetup::first(4),
                    format,
                    [difficulty; MAX_PLAYERS],
                );
                assert!(outcome.is_some(), "{format:?} {difficulty:?}");
            }
        }
    }

    #[test]
    fn passes_all_automated_quality_bars() {
        sim_check::check_all::<MusicalStools>(8).unwrap();
    }
}
