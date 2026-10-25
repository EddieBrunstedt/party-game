//! Lantern Flail: everyone drags a heavy lantern on a rope round a raft on a night pond. You
//! never hit with your body: run in arcs and the lantern swings after you, and a lantern that
//! strikes a rival knocks them back by how fast it was going. Every hit you take makes the next
//! one knock you further ([`State::damage`], reset when you fall in). Knock someone into the
//! pond and the point is yours; the fallen climb back on after a moment, so nobody sits out.
//!
//! A lunges, whipping the lantern after you. Holding B reels the rope in short, and the lantern
//! spins faster the shorter it gets; let go and it pays out to full length, flinging the lantern
//! outwards. A taut rope sweeps rivals aside too (and hits them if it's moving fast), and the
//! barrels floating about on the raft knock people over when a lantern sends them flying.
//!
//! Free-for-all: a fall scores for whoever hit you last, if that was in the last
//! [`CREDIT_TICKS`]. Most points after [`PLAY_TICKS`] wins; if the lead is level then, it's
//! sudden death until a fall breaks the tie (for at most [`SUDDEN_DEATH_TICKS`]). With two sides (2v2, 1v3, duel) every fall scores for the other side.
//! 2v2: partners hold the two ends of one long rope with a lantern in the middle, and sweep it
//! across the raft together. 1v3: the solo is a giant swinging a wrecking ball on a long chain
//! and needs [`GIANT_GOAL`] falls of the three; they need [`TRIO_GOAL`] of the giant.
//!
//! The floor uses `sim_core::physics`' plane: `x` to the right, `y` towards the camera.

use std::f32::consts::{FRAC_PI_2, TAU};

use sim_core::physics::{self, Body, DT, Vec2};
use sim_core::{
    Button, Difficulty, Format, FormatKind, Input, Inputs, MAX_PLAYERS, Minigame, Outcome,
    PlayerSetup, Rng, Seat,
};

mod bot;
#[cfg(test)]
mod tests;

/// The opening countdown, the same in every minigame.
pub const INTRO_TICKS: u16 = sim_core::COUNTDOWN_TICKS;
pub const PLAY_TICKS: u32 = 60 * 60;
/// Level at the end: sudden death, until the next fall that breaks the tie or this long.
pub const SUDDEN_DEATH_TICKS: u32 = 20 * 60;
/// After the result, the game runs on this long so the last splash plays out.
pub const DECIDED_TICKS: u16 = 150;

pub const RAFT_RADIUS: f32 = 6.5;
pub const FOLK_RADIUS: f32 = 0.45;
pub const MOVE_SPEED: f32 = 5.2;
pub const ACCEL: f32 = 30.0;
pub const DRAG: f32 = 3.0;
/// Reeling from a hit, you slide: this is all that slows you.
pub const STUN_DRAG: f32 = 1.6;
pub const LUNGE_SPEED: f32 = 11.0;
pub const LUNGE_TICKS: u16 = 9;
pub const LUNGE_COOLDOWN: u16 = 60;

/// Lanterns faster than this (relative to whoever they touch) strike; slower ones just nudge.
pub const HIT_MIN: f32 = 4.0;
/// A struck folk flies off at this much of the lantern's speed (times its heft)...
pub const KNOCK: f32 = 0.6;
/// ...and every point of damage adds this share to that: 100 damage knocks twice as far.
pub const DAMAGE_GROWTH: f32 = 0.01;
/// Damage per unit of a hit's speed (times its heft).
pub const DAMAGE_PER_SPEED: f32 = 1.2;
pub const MAX_DAMAGE: u16 = 300;
pub const STUN_BASE: u16 = 10;
pub const STUN_MAX: u16 = 40;
/// The same folk can't be struck twice this soon.
pub const REHIT_TICKS: u32 = 10;
/// A hit this hard (speed times heft) freezes everything for [`HITSTOP_TICKS`].
pub const HITSTOP_POWER: f32 = 10.0;
pub const HITSTOP_TICKS: u16 = 4;
/// A lantern never goes faster than this.
pub const MAX_LANTERN_SPEED: f32 = 20.0;
const LANTERN_DRAG: f32 = 0.5;
/// Lanterns hang from their ropes: gravity swings one back under the hand, pulling it this hard
/// (floor units a second, a second) times the rope's angle from straight down, as a pendulum.
pub const HANG: f32 = 20.0;
/// Turning the stick round whirls the lantern round you, as an arm swings a rope: at
/// [`WHIRL_FULL`] radians a second it pushes the lantern round at [`WHIRL_ACCEL`], until it's
/// going round at [`WHIRL_TOP`]. Anything faster comes from lunges, reeling in and the rope.
pub const WHIRL_FULL: f32 = 9.0;
pub const WHIRL_ACCEL: f32 = 16.0;
pub const WHIRL_TOP: f32 = 13.0;
/// The stick's turning rate is smoothed over about this many ticks.
const WHIRL_SMOOTH: f32 = 8.0;
/// Below this deflection the stick isn't turning anything.
const WHIRL_STICK: f32 = 0.6;
/// How bouncy lanterns are off each other, and a struck folk off a lantern.
const CLANG_RESTITUTION: f32 = 0.8;
const STRIKE_BOUNCE: f32 = 0.3;
const RESTITUTION: f32 = 0.3;

/// Rope lengths change at these speeds, in floor units a second: reeling in, paying out.
pub const REEL_SPEED: f32 = 4.0;
pub const PAYOUT_SPEED: f32 = 9.0;
/// A rope stretched to this share of its length is taut, and sweeps rivals aside.
pub const ROPE_TAUT: f32 = 0.92;
pub const ROPE_RADIUS: f32 = 0.08;
/// A taut rope strikes when moving this fast where it touches, with this heft.
pub const ROPE_HIT_MIN: f32 = 4.5;
pub const ROPE_HEFT: f32 = 0.6;
/// A lunge into a rival shoves them with this heft (at [`LUNGE_SPEED`]).
pub const LUNGE_HEFT: f32 = 0.3;

/// A fall scores for whoever hit you last, if they did it this recently (free-for-all).
pub const CREDIT_TICKS: u32 = 180;
/// Sinking into the pond, then waiting to climb back on, then safe from hits for a moment.
pub const FALL_TICKS: u16 = 45;
pub const RESPAWN_TICKS: u16 = 75;
pub const SAFE_TICKS: u16 = 60;

pub const BARRELS: usize = 3;
pub const BARREL_RADIUS: f32 = 0.42;
const BARREL_MASS: f32 = 0.7;
const BARREL_DRAG: f32 = 1.0;
const BARREL_RESTITUTION: f32 = 0.6;
/// A barrel this fast knocks folk over, for whoever last sent it flying.
pub const BARREL_HIT_MIN: f32 = 4.0;
pub const BARREL_HEFT: f32 = 0.8;
/// A barrel that falls in comes back after this long.
pub const BARREL_RESPAWN_TICKS: u16 = 240;

/// 1v3: the giant is big, heavy and a little slow...
pub const GIANT_RADIUS: f32 = 0.62;
pub const GIANT_MASS: f32 = 3.0;
pub const GIANT_SPEED: f32 = 4.6;
/// ...and wins with this many falls of the three, before they dunk it [`TRIO_GOAL`] times.
pub const GIANT_GOAL: u32 = 8;
pub const TRIO_GOAL: u32 = 3;

pub struct LanternFlail;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Your own lantern on a rope.
    Flail,
    /// 2v2: one lantern in the middle of a rope whose ends the two partners hold.
    Bolas,
    /// 1v3: the giant's.
    WreckingBall,
}

/// How each kind of lantern is built.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spec {
    pub radius: f32,
    pub mass: f32,
    /// How hard it knocks folk, for its speed.
    pub heft: f32,
    /// Each holder's rope, paid out and reeled in.
    pub rope_max: f32,
    pub rope_min: f32,
}

impl Kind {
    pub fn spec(self) -> Spec {
        match self {
            Kind::Flail => Spec {
                radius: 0.36,
                mass: 0.55,
                heft: 1.0,
                rope_max: 2.4,
                rope_min: 0.9,
            },
            Kind::Bolas => Spec {
                radius: 0.42,
                mass: 0.7,
                heft: 1.4,
                rope_max: 2.6,
                rope_min: 1.0,
            },
            Kind::WreckingBall => Spec {
                radius: 0.58,
                mass: 1.1,
                heft: 1.5,
                rope_max: 3.2,
                rope_min: 1.3,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Standing,
    /// Went into the pond `t` ticks ago.
    Falling {
        t: u16,
    },
    /// Waiting to climb back on.
    Waiting {
        t: u16,
    },
    /// Outside a duel.
    Benched,
}

/// A hit someone took.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hit {
    pub tick: u32,
    pub by: Seat,
    /// The hit's speed times its heft, in tenths.
    pub power: u16,
    pub source: Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Lantern,
    Rope,
    Barrel,
    Lunge,
}

#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct Folk {
    pub body: Body,
    /// The way it last ran (and lunges): a unit vector.
    pub facing: Vec2,
    pub status: Status,
    pub giant: bool,
    /// Ticks left in the current lunge, and until the next.
    pub lunge: u16,
    pub lunge_cooldown: u16,
    /// Ticks left reeling from a hit, unable to steer.
    pub stun: u16,
    /// Each hit adds to it, and the more it is the further hits knock you. Reset by a fall.
    pub damage: u16,
    /// Ticks left after climbing back on in which nothing strikes you.
    pub safe: u16,
    /// Holding B.
    pub reeling: bool,
    /// How fast the stick is turning, smoothed, in thousandths of a radian a second
    /// (anticlockwise seen from above, as `Vec2::perp`, is positive).
    pub turn: i32,
    pub last_hit: Option<Hit>,
}

impl Folk {
    pub fn standing(&self) -> bool {
        self.status == Status::Standing
    }

    fn speed(&self) -> f32 {
        if self.giant { GIANT_SPEED } else { MOVE_SPEED }
    }

    /// The stick's smoothed turning rate, in radians a second.
    pub fn turn_rate(&self) -> f32 {
        self.turn as f32 / 1000.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct Lantern {
    pub body: Body,
    pub kind: Kind,
    /// The side it plays for, by [`Format::team`].
    pub team: u8,
    /// Who holds it: one seat, or both partners with a bolas.
    pub holders: [Option<Seat>; 2],
    /// Each holder's rope length, in thousandths of a floor unit.
    pub rope: [u16; 2],
    /// Nobody holding it is on the raft: it's in the pond.
    pub down: bool,
    /// Play tick it last clanged off another lantern.
    pub last_clang: Option<u32>,
}

impl Lantern {
    pub fn rope_length(&self, slot: usize) -> f32 {
        self.rope[slot] as f32 / 1000.0
    }

    pub fn speed(&self) -> f32 {
        self.body.vel.length()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BarrelStatus {
    Floating,
    /// Went over the edge `t` ticks ago.
    Sinking {
        t: u16,
    },
    /// Back on the raft once `t` reaches [`BARREL_RESPAWN_TICKS`].
    Gone {
        t: u16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct Barrel {
    pub body: Body,
    pub status: BarrelStatus,
    /// Who last sent it flying, and when.
    pub pushed_by: Option<(Seat, u32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Intro {
        t: u16,
    },
    Playing,
    /// The result is in; the raft runs on for a moment without anyone steering.
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
    rng: Rng,
    /// Ticks of play so far.
    pub tick: u32,
    /// Ticks left of a hit-stop, in which nothing moves.
    pub freeze: u16,
    pub folk: [Folk; MAX_PLAYERS],
    pub lanterns: Vec<Lantern>,
    /// Which lantern each seat holds.
    pub lantern_of: [Option<u8>; MAX_PLAYERS],
    pub barrels: [Barrel; BARRELS],
    /// Points by team ([`Format::team`]; in free-for-all that's the seat).
    pub points: [u32; MAX_PLAYERS],
    /// Falls each seat was credited with, and took.
    pub knockoffs: [u32; MAX_PLAYERS],
    pub falls: [u32; MAX_PLAYERS],
    /// Play tick of each seat's last fall.
    pub fell_at: [Option<u32>; MAX_PLAYERS],
    pub outcome: Option<Outcome>,
}

/// Where respawns land: this many spots round the raft.
const SPAWN_SPOTS: usize = 8;
const SPAWN_RING: f32 = RAFT_RADIUS * 0.45;

fn spawn_spot(k: usize) -> Vec2 {
    Vec2::from_angle(TAU * k as f32 / SPAWN_SPOTS as f32 + 0.2) * SPAWN_RING
}

fn rotate(v: Vec2, angle: f32) -> Vec2 {
    let (s, c) = (sim_core::math::sin(angle), sim_core::math::cos(angle));
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

fn millis(length: f32) -> u16 {
    (length * 1000.0).round() as u16
}

impl State {
    pub fn folk(&self, seat: Seat) -> &Folk {
        &self.folk[seat.index()]
    }

    pub fn lantern(&self, seat: Seat) -> Option<&Lantern> {
        self.lantern_of[seat.index()].map(|l| &self.lanterns[l as usize])
    }

    /// Seats playing (benched duel seats aren't).
    pub fn playing(&self) -> impl Iterator<Item = Seat> + '_ {
        self.players
            .seats()
            .filter(|s| self.folk[s.index()].status != Status::Benched)
    }

    pub fn standing(&self) -> impl Iterator<Item = Seat> + '_ {
        self.players
            .seats()
            .filter(|s| self.folk[s.index()].standing())
    }

    pub fn ticks_left(&self) -> u32 {
        PLAY_TICKS.saturating_sub(self.tick)
    }

    /// Time ran out with the lead tied, and the next fall decides.
    pub fn sudden_death(&self) -> bool {
        self.tick >= PLAY_TICKS && self.outcome.is_none()
    }

    /// Whether more than one side shares the lead.
    fn level(&self) -> bool {
        let scores = match self.format {
            Format::FreeForAll => self.points,
            _ => self.team_scores(),
        };
        // In free-for-all every seat is its own team.
        let sides: Vec<u32> = self
            .playing()
            .filter_map(|s| self.format.team(s))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|side| scores[side as usize])
            .collect();
        let best = sides.iter().copied().max().unwrap_or(0);
        sides.iter().filter(|s| **s == best).count() > 1
    }

    pub fn giant(&self) -> Option<Seat> {
        match self.format {
            Format::OneVsThree { solo } => Some(solo),
            _ => None,
        }
    }

    /// Whether `a` and `b` are on the same side (never true in free-for-all).
    pub fn allies(&self, a: Seat, b: Seat) -> bool {
        self.format.same_team(a, b)
    }

    /// The team a lantern can't strike (its own).
    fn friendly(&self, lantern: &Lantern, seat: Seat) -> bool {
        self.format.team(seat) == Some(lantern.team)
    }

    /// The team's points needed to win, in 1v3.
    pub fn goal(&self, team: u8) -> Option<u32> {
        self.giant()
            .map(|_| if team == 0 { GIANT_GOAL } else { TRIO_GOAL })
    }

    fn step_folk(&mut self, inputs: &Inputs) {
        for seat in self.players.seats() {
            let folk = &mut self.folk[seat.index()];
            if !folk.standing() {
                continue;
            }
            let input = &inputs.current[seat.index()];
            let stick = Vec2::from_stick(input);
            folk.lunge = folk.lunge.saturating_sub(1);
            folk.lunge_cooldown = folk.lunge_cooldown.saturating_sub(1);
            folk.stun = folk.stun.saturating_sub(1);
            folk.safe = folk.safe.saturating_sub(1);
            folk.reeling = input.held(Button::B);
            let mut turning = 0.0;
            if stick != Vec2::ZERO && folk.stun == 0 {
                let next = stick.normalize_or_zero();
                if stick.length() >= WHIRL_STICK {
                    let angle =
                        sim_core::math::atan2(folk.facing.perp_dot(next), folk.facing.dot(next));
                    turning = angle / DT;
                }
                folk.facing = next;
            }
            let smoothed = folk.turn_rate() + (turning - folk.turn_rate()) / WHIRL_SMOOTH;
            folk.turn = (smoothed * 1000.0).round() as i32;
            if folk.stun == 0
                && folk.lunge == 0
                && folk.lunge_cooldown == 0
                && inputs.just_pressed(seat, Button::A)
            {
                folk.lunge = LUNGE_TICKS;
                folk.lunge_cooldown = LUNGE_COOLDOWN;
                folk.body.vel = folk.facing * LUNGE_SPEED;
            }
            let drag = if folk.lunge > 0 {
                0.0
            } else if folk.stun > 0 {
                STUN_DRAG
            } else {
                let speed = folk.speed();
                folk.body.steer(stick, speed, ACCEL);
                DRAG
            };
            folk.body.integrate(drag);
        }
    }

    /// Reel ropes in or out, and swing lanterns faster as their ropes get shorter.
    fn reel(&mut self) {
        for l in 0..self.lanterns.len() {
            let mut lantern = self.lanterns[l];
            if lantern.down {
                continue;
            }
            let spec = lantern.kind.spec();
            let holders = lantern.holders.iter().flatten().count();
            for slot in 0..2 {
                let Some(seat) = lantern.holders[slot] else {
                    continue;
                };
                let folk = self.folk[seat.index()];
                if !folk.standing() {
                    continue;
                }
                let before = lantern.rope_length(slot);
                let after = if folk.reeling {
                    (before - REEL_SPEED * DT).max(spec.rope_min)
                } else if before > spec.rope_max {
                    (before - REEL_SPEED * DT).max(spec.rope_max)
                } else {
                    (before + PAYOUT_SPEED * DT).min(spec.rope_max)
                };
                let reach = lantern.body.pos - folk.body.pos;
                // Reeling a taut rope in spins the lantern faster, as a skater's arms do.
                if holders == 1 && after < before && reach.length() >= before * ROPE_TAUT {
                    let out = reach.normalize_or_zero();
                    let relative = lantern.body.vel - folk.body.vel;
                    let across = relative - out * relative.dot(out);
                    lantern.body.vel += across * (before / after - 1.0);
                }
                lantern.rope[slot] = millis(after);
            }
            self.lanterns[l] = lantern;
        }
    }

    fn integrate_things(&mut self) {
        for l in 0..self.lanterns.len() {
            let lantern = &self.lanterns[l];
            if lantern.down {
                continue;
            }
            let mut pull = Vec2::ZERO;
            for slot in 0..2 {
                let Some(seat) = lantern.holders[slot] else {
                    continue;
                };
                let hand = self.folk(seat);
                if !hand.standing() {
                    continue;
                }
                let toward = hand.body.pos - lantern.body.pos;
                let swing = (toward.length() / lantern.rope_length(slot)).min(1.0);
                pull += toward.normalize_or_zero() * (HANG * swing);
                // The whirl pushes it round the hand, the way the stick turns.
                let effort = (hand.turn_rate() / WHIRL_FULL).clamp(-1.0, 1.0);
                let round = (-toward).normalize_or_zero().perp() * effort.signum();
                let going = (lantern.body.vel - hand.body.vel).dot(round);
                if going < WHIRL_TOP {
                    pull += round * (WHIRL_ACCEL * effort.abs());
                }
            }
            self.lanterns[l].body.vel += pull * DT;
        }
        for lantern in self.lanterns.iter_mut().filter(|l| !l.down) {
            lantern.body.vel = lantern.body.vel.clamp_length(MAX_LANTERN_SPEED);
            lantern.body.integrate(LANTERN_DRAG);
        }
        for barrel in &mut self.barrels {
            if barrel.status == BarrelStatus::Floating {
                barrel.body.integrate(BARREL_DRAG);
            }
        }
    }

    fn tighten_ropes(&mut self) {
        for l in 0..self.lanterns.len() {
            let mut lantern = self.lanterns[l];
            if lantern.down {
                continue;
            }
            for slot in 0..2 {
                let Some(seat) = lantern.holders[slot] else {
                    continue;
                };
                let folk = &mut self.folk[seat.index()];
                let length = lantern.rope_length(slot);
                if folk.standing() {
                    physics::tether(&mut folk.body, &mut lantern.body, length);
                }
            }
            self.lanterns[l] = lantern;
        }
    }

    /// Knock `victim` along `dir` with a hit of `speed` and `heft`, for `by`.
    fn strike(&mut self, victim: Seat, by: Seat, dir: Vec2, speed: f32, heft: f32, source: Source) {
        let tick = self.tick;
        let folk = &mut self.folk[victim.index()];
        let power = speed * heft;
        let growth = 1.0 + folk.damage as f32 * DAMAGE_GROWTH;
        folk.body.vel = folk.body.vel * 0.25 + dir * (KNOCK * power * growth / folk.body.mass);
        folk.damage = (folk.damage + (power * DAMAGE_PER_SPEED).round() as u16).min(MAX_DAMAGE);
        folk.stun = (STUN_BASE + (power * 1.5) as u16).min(STUN_MAX);
        folk.lunge = 0;
        folk.last_hit = Some(Hit {
            tick,
            by,
            power: (power * 10.0).round() as u16,
            source,
        });
        if power >= HITSTOP_POWER {
            self.freeze = HITSTOP_TICKS;
        }
    }

    /// Whether `seat` can be struck right now.
    fn open_to_hits(&self, seat: Seat) -> bool {
        let folk = self.folk(seat);
        folk.standing()
            && folk.safe == 0
            && folk
                .last_hit
                .is_none_or(|hit| hit.tick + REHIT_TICKS <= self.tick)
    }

    /// The holder a lantern's hit is credited to: the nearer one still on the raft.
    fn wielder(&self, lantern: &Lantern) -> Option<Seat> {
        lantern
            .holders
            .iter()
            .flatten()
            .filter(|s| self.folk(**s).standing())
            .min_by(|a, b| {
                let d = |s: Seat| self.folk(s).body.pos.distance(lantern.body.pos);
                d(**a).total_cmp(&d(**b)).then(a.cmp(b))
            })
            .copied()
    }

    fn collide(&mut self) {
        self.collide_folk();
        self.collide_barrels();
        self.collide_lanterns();
        self.lanterns_strike();
        self.ropes_sweep();
    }

    fn collide_folk(&mut self) {
        let seats: Vec<Seat> = self.standing().collect();
        for (i, &a) in seats.iter().enumerate() {
            for &b in &seats[i + 1..] {
                let (mut fa, mut fb) = (self.folk[a.index()], self.folk[b.index()]);
                let Some(contact) = physics::collide(&mut fa.body, &mut fb.body, RESTITUTION)
                else {
                    continue;
                };
                self.folk[a.index()] = fa;
                self.folk[b.index()] = fb;
                if self.allies(a, b) {
                    continue;
                }
                // A lunge into a rival shoves them.
                for (attacker, victim, normal) in [(a, b, contact.normal), (b, a, -contact.normal)]
                {
                    let folk = self.folk(attacker);
                    if folk.lunge > 0 && folk.facing.dot(normal) > 0.3 && self.open_to_hits(victim)
                    {
                        self.folk[attacker.index()].lunge = 0;
                        self.strike(
                            victim,
                            attacker,
                            normal,
                            LUNGE_SPEED,
                            LUNGE_HEFT,
                            Source::Lunge,
                        );
                    }
                }
            }
        }
    }

    fn collide_barrels(&mut self) {
        for i in 0..BARRELS {
            if self.barrels[i].status != BarrelStatus::Floating {
                continue;
            }
            for j in i + 1..BARRELS {
                if self.barrels[j].status != BarrelStatus::Floating {
                    continue;
                }
                let (mut a, mut b) = (self.barrels[i], self.barrels[j]);
                if physics::collide(&mut a.body, &mut b.body, BARREL_RESTITUTION).is_some() {
                    // A barrel knocked into another passes on who sent it.
                    if a.body.vel.length() > b.body.vel.length() {
                        b.pushed_by = a.pushed_by.or(b.pushed_by);
                    } else {
                        a.pushed_by = b.pushed_by.or(a.pushed_by);
                    }
                    self.barrels[i] = a;
                    self.barrels[j] = b;
                }
            }
            let seats: Vec<Seat> = self.standing().collect();
            for seat in seats {
                let mut barrel = self.barrels[i];
                let mut folk = self.folk[seat.index()];
                let closing = (barrel.body.vel - folk.body.vel)
                    .dot((folk.body.pos - barrel.body.pos).normalize_or_zero());
                let Some(contact) = physics::collide(&mut barrel.body, &mut folk.body, RESTITUTION)
                else {
                    continue;
                };
                self.barrels[i] = barrel;
                self.folk[seat.index()] = folk;
                if let Some((by, at)) = barrel.pushed_by
                    && closing > BARREL_HIT_MIN
                    && at + CREDIT_TICKS > self.tick
                    && by != seat
                    && !self.allies(by, seat)
                    && self.open_to_hits(seat)
                {
                    self.strike(
                        seat,
                        by,
                        contact.normal,
                        closing,
                        BARREL_HEFT,
                        Source::Barrel,
                    );
                }
            }
        }
    }

    fn collide_lanterns(&mut self) {
        let tick = self.tick;
        for i in 0..self.lanterns.len() {
            if self.lanterns[i].down {
                continue;
            }
            for j in i + 1..self.lanterns.len() {
                if self.lanterns[j].down {
                    continue;
                }
                let (mut a, mut b) = (self.lanterns[i], self.lanterns[j]);
                if let Some(contact) = physics::collide(&mut a.body, &mut b.body, CLANG_RESTITUTION)
                {
                    if contact.impulse > 1.0 {
                        a.last_clang = Some(tick);
                        b.last_clang = Some(tick);
                    }
                    self.lanterns[i] = a;
                    self.lanterns[j] = b;
                }
            }
            let wielder = self.wielder(&self.lanterns[i]);
            for barrel in &mut self.barrels {
                if barrel.status != BarrelStatus::Floating {
                    continue;
                }
                let lantern = &mut self.lanterns[i];
                if let Some(contact) =
                    physics::collide(&mut lantern.body, &mut barrel.body, BARREL_RESTITUTION)
                    && contact.impulse > 0.5
                    && let Some(by) = wielder
                {
                    barrel.pushed_by = Some((by, tick));
                }
            }
        }
    }

    fn lanterns_strike(&mut self) {
        for l in 0..self.lanterns.len() {
            if self.lanterns[l].down {
                continue;
            }
            let Some(by) = self.wielder(&self.lanterns[l]) else {
                continue;
            };
            let seats: Vec<Seat> = self.standing().collect();
            for seat in seats {
                let lantern = self.lanterns[l];
                if self.friendly(&lantern, seat) {
                    continue;
                }
                let mut folk = self.folk[seat.index()];
                let delta = folk.body.pos - lantern.body.pos;
                let reach = lantern.body.radius + folk.body.radius;
                if delta.length_squared() >= reach * reach {
                    continue;
                }
                let normal = delta.normalize_or_zero();
                let normal = if normal == Vec2::ZERO {
                    Vec2::new(1.0, 0.0)
                } else {
                    normal
                };
                let closing = (lantern.body.vel - folk.body.vel).dot(normal);
                if closing > HIT_MIN && self.open_to_hits(seat) {
                    // Knocked mostly away from the lantern, partly along its swing.
                    let swing = lantern.body.vel.normalize_or_zero();
                    let dir = (normal + swing * 0.5).normalize_or_zero();
                    let heft = lantern.kind.spec().heft;
                    let lantern = &mut self.lanterns[l];
                    lantern.body.pos -= normal * (reach - delta.length());
                    lantern.body.vel -= normal * (closing * (1.0 + STRIKE_BOUNCE));
                    self.strike(seat, by, dir, closing, heft, Source::Lantern);
                } else {
                    let mut lantern = lantern;
                    physics::collide(&mut lantern.body, &mut folk.body, RESTITUTION);
                    self.lanterns[l] = lantern;
                    self.folk[seat.index()] = folk;
                }
            }
        }
    }

    /// Taut ropes push rivals out of the way, and strike them if they sweep in fast.
    fn ropes_sweep(&mut self) {
        for l in 0..self.lanterns.len() {
            let lantern = self.lanterns[l];
            if lantern.down {
                continue;
            }
            for slot in 0..2 {
                let Some(holder) = lantern.holders[slot] else {
                    continue;
                };
                let hand = self.folk(holder).body;
                if !self.folk(holder).standing() {
                    continue;
                }
                let span = lantern.body.pos.distance(hand.pos);
                if span < lantern.rope_length(slot) * ROPE_TAUT {
                    continue;
                }
                let seats: Vec<Seat> = self.standing().collect();
                for seat in seats {
                    if self.friendly(&lantern, seat) {
                        continue;
                    }
                    let folk = self.folk(seat).body;
                    let (point, along) =
                        physics::closest_on_segment(hand.pos, lantern.body.pos, folk.pos);
                    // The ends are the hand and the lantern itself, handled elsewhere.
                    if !(0.1..=0.9).contains(&along) {
                        continue;
                    }
                    let delta = folk.pos - point;
                    let reach = folk.radius + ROPE_RADIUS;
                    if delta.length_squared() >= reach * reach {
                        continue;
                    }
                    let normal = delta.normalize_or_zero();
                    if normal == Vec2::ZERO {
                        continue;
                    }
                    let rope_vel = hand.vel.lerp(lantern.body.vel, along);
                    let closing = (rope_vel - folk.vel).dot(normal);
                    let hit = closing > ROPE_HIT_MIN && self.open_to_hits(seat);
                    let folk = &mut self.folk[seat.index()];
                    folk.body.pos += normal * (reach - delta.length());
                    if hit {
                        self.strike(seat, holder, normal, closing, ROPE_HEFT, Source::Rope);
                    } else if closing > 0.0 {
                        folk.body.vel += normal * closing;
                    }
                }
            }
        }
    }

    /// Anyone (or any barrel) past the edge goes in; falls score.
    fn splash(&mut self) {
        let players = self.players;
        for seat in players.seats() {
            let folk = self.folk[seat.index()];
            if folk.standing() && folk.body.pos.length() > RAFT_RADIUS {
                self.fall(seat);
            }
        }
        for b in 0..BARRELS {
            let barrel = &mut self.barrels[b];
            if barrel.status == BarrelStatus::Floating && barrel.body.pos.length() > RAFT_RADIUS {
                barrel.status = BarrelStatus::Sinking { t: 0 };
            }
        }
    }

    fn fall(&mut self, seat: Seat) {
        let tick = self.tick;
        let folk = &mut self.folk[seat.index()];
        folk.status = Status::Falling { t: 0 };
        folk.lunge = 0;
        folk.stun = 0;
        let credit = folk
            .last_hit
            .filter(|hit| hit.tick + CREDIT_TICKS > tick)
            .map(|hit| hit.by);
        self.falls[seat.index()] += 1;
        self.fell_at[seat.index()] = Some(tick);
        if let Some(by) = credit {
            self.knockoffs[by.index()] += 1;
        }
        match self.format {
            Format::FreeForAll => {
                if let Some(by) = credit {
                    self.points[by.index()] += 1;
                }
            }
            // With two sides, every fall scores for the other.
            _ => {
                if let Some(team) = self.format.team(seat) {
                    self.points[1 - team as usize] += 1;
                }
            }
        }
        if let Some(l) = self.lantern_of[seat.index()] {
            let lantern = &self.lanterns[l as usize];
            if !lantern
                .holders
                .iter()
                .flatten()
                .any(|s| self.folk(*s).standing())
            {
                self.lanterns[l as usize].down = true;
            }
        }
    }

    /// Falls play out, the fallen climb back on, barrels come back.
    fn recover(&mut self) {
        let players = self.players;
        for seat in players.seats() {
            let folk = &mut self.folk[seat.index()];
            match folk.status {
                Status::Falling { t } if t + 1 >= FALL_TICKS => {
                    folk.status = Status::Waiting { t: 0 }
                }
                Status::Falling { t } => {
                    folk.status = Status::Falling { t: t + 1 };
                    folk.body.integrate(2.0);
                }
                Status::Waiting { t } if t + 1 >= RESPAWN_TICKS => {
                    if self.phase == Phase::Playing {
                        self.respawn(seat);
                    }
                }
                Status::Waiting { t } => folk.status = Status::Waiting { t: t + 1 },
                Status::Standing | Status::Benched => {}
            }
        }
        for b in 0..BARRELS {
            match self.barrels[b].status {
                BarrelStatus::Floating => {}
                BarrelStatus::Sinking { t } if t + 1 >= FALL_TICKS => {
                    self.barrels[b].status = BarrelStatus::Gone { t: 0 }
                }
                BarrelStatus::Sinking { t } => {
                    let barrel = &mut self.barrels[b];
                    barrel.status = BarrelStatus::Sinking { t: t + 1 };
                    barrel.body.integrate(2.0);
                }
                BarrelStatus::Gone { t } if t + 1 >= BARREL_RESPAWN_TICKS => {
                    let angle = self.rng.range_u32(0, 3599) as f32 / 3600.0 * TAU;
                    let distance = self.rng.range_u32(0, 1000) as f32 / 1000.0 * RAFT_RADIUS * 0.6;
                    let barrel = &mut self.barrels[b];
                    barrel.body = Body::new(
                        Vec2::from_angle(angle) * distance,
                        BARREL_RADIUS,
                        BARREL_MASS,
                    );
                    barrel.status = BarrelStatus::Floating;
                    barrel.pushed_by = None;
                }
                BarrelStatus::Gone { t } => {
                    self.barrels[b].status = BarrelStatus::Gone { t: t + 1 }
                }
            }
        }
    }

    /// Back onto the raft at the spot furthest from rivals (with a bolas, near the partner's
    /// lantern too).
    fn respawn(&mut self, seat: Seat) {
        let lantern = self.lantern_of[seat.index()].map(|l| self.lanterns[l as usize]);
        let held = lantern.filter(|l| !l.down);
        let score = |k: usize| {
            let spot = spawn_spot(k);
            let rivals = self
                .standing()
                .filter(|s| !self.allies(seat, *s) && *s != seat)
                .map(|s| self.folk(s).body.pos.distance(spot))
                .fold(RAFT_RADIUS * 2.0, f32::min);
            let near_lantern = held.map(|l| l.body.pos.distance(spot)).unwrap_or(0.0);
            rivals - near_lantern * 0.7
        };
        let best = (0..SPAWN_SPOTS)
            .max_by(|a, b| score(*a).total_cmp(&score(*b)).then(b.cmp(a)))
            .expect("there are spawn spots");
        let pos = spawn_spot(best);
        let folk = &mut self.folk[seat.index()];
        folk.body.pos = pos;
        folk.body.vel = Vec2::ZERO;
        folk.facing = (-pos).normalize_or_zero();
        folk.status = Status::Standing;
        folk.damage = 0;
        folk.stun = 0;
        folk.lunge = 0;
        folk.safe = SAFE_TICKS;
        folk.last_hit = None;
        let Some(l) = self.lantern_of[seat.index()] else {
            return;
        };
        let lantern = &mut self.lanterns[l as usize];
        let slot = if lantern.holders[0] == Some(seat) {
            0
        } else {
            1
        };
        if lantern.down {
            // It comes back up with you, a little way in towards the middle.
            lantern.down = false;
            lantern.body.pos = pos * (1.0 - 1.2 / pos.length());
            lantern.body.vel = Vec2::ZERO;
            for rope in &mut lantern.rope {
                *rope = millis(lantern.kind.spec().rope_max);
            }
        } else {
            // Catch the partner's rope where it is; it reels back to length.
            let reach = lantern.body.pos.distance(pos);
            lantern.rope[slot] = millis(reach.max(lantern.kind.spec().rope_max));
        }
    }

    /// Scores to rank sides by: points, or in 1v3 the share of each side's goal.
    fn team_scores(&self) -> [u32; MAX_PLAYERS] {
        match self.format {
            Format::OneVsThree { .. } => {
                let mut scores = [0; MAX_PLAYERS];
                scores[0] = self.points[0].min(GIANT_GOAL) * TRIO_GOAL;
                scores[1] = self.points[1].min(TRIO_GOAL) * GIANT_GOAL;
                scores
            }
            _ => self.points,
        }
    }

    fn decide_if_over(&mut self) {
        if self.outcome.is_some() {
            return;
        }
        let goal_reached =
            self.giant().is_some() && (self.points[0] >= GIANT_GOAL || self.points[1] >= TRIO_GOAL);
        let time_up = self.tick >= PLAY_TICKS
            && (!self.level() || self.tick >= PLAY_TICKS + SUDDEN_DEATH_TICKS);
        if goal_reached || time_up {
            self.outcome = Some(match self.format {
                Format::FreeForAll => Outcome::from_scores(&self.players, &self.points),
                _ => Outcome::from_team_scores(&self.players, &self.format, &self.team_scores()),
            });
            self.phase = Phase::Decided { t: 0 };
        }
    }

    fn simulate(&mut self, inputs: &Inputs) {
        if self.freeze > 0 {
            self.freeze -= 1;
            return;
        }
        self.step_folk(inputs);
        self.reel();
        self.integrate_things();
        self.tighten_ropes();
        self.collide();
        self.tighten_ropes();
        self.splash();
        self.recover();
    }
}

/// Where everyone starts: round the raft, partners side by side with their bolas between them.
fn start_positions(players: &PlayerSetup, format: &Format) -> [Vec2; MAX_PLAYERS] {
    let mut positions = [Vec2::ZERO; MAX_PLAYERS];
    let playing: Vec<Seat> = players
        .seats()
        .filter(|s| format.team(*s).is_some())
        .collect();
    let ring = RAFT_RADIUS * 0.55;
    match format {
        Format::TwoVsTwo { .. } => {
            for team in 0..2u8 {
                let side = if team == 0 { -1.0 } else { 1.0 };
                let members = playing.iter().filter(|s| format.team(**s) == Some(team));
                for (k, seat) in members.enumerate() {
                    let angle = FRAC_PI_2 * (1.0 - side) + (k as f32 - 0.5) * 0.9;
                    positions[seat.index()] = Vec2::from_angle(angle) * ring;
                }
            }
        }
        _ => {
            for (i, seat) in playing.iter().enumerate() {
                let angle = FRAC_PI_2 + TAU * i as f32 / playing.len() as f32;
                positions[seat.index()] = Vec2::from_angle(angle) * ring;
            }
        }
    }
    positions
}

impl Minigame for LanternFlail {
    type State = State;

    const ID: &'static str = "lantern-flail";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[
        FormatKind::FreeForAll,
        FormatKind::OneVsThree,
        FormatKind::TwoVsTwo,
        FormatKind::Duel,
    ];
    const MAX_TICKS: u32 =
        INTRO_TICKS as u32 + PLAY_TICKS + SUDDEN_DEATH_TICKS + DECIDED_TICKS as u32 + 60;

    fn new(seed: u64, players: PlayerSetup, format: Format) -> State {
        let positions = start_positions(&players, &format);
        let mut folk = [Folk {
            body: Body::new(Vec2::ZERO, FOLK_RADIUS, 1.0),
            facing: Vec2::new(0.0, -1.0),
            status: Status::Benched,
            giant: false,
            lunge: 0,
            lunge_cooldown: 0,
            stun: 0,
            damage: 0,
            safe: 0,
            reeling: false,
            turn: 0,
            last_hit: None,
        }; MAX_PLAYERS];
        let mut lanterns = Vec::new();
        let mut lantern_of = [None; MAX_PLAYERS];
        for seat in players.seats() {
            let Some(team) = format.team(seat) else {
                continue;
            };
            let pos = positions[seat.index()];
            let giant = matches!(format, Format::OneVsThree { solo } if solo == seat);
            let f = &mut folk[seat.index()];
            f.status = Status::Standing;
            f.giant = giant;
            f.facing = (-pos).normalize_or_zero();
            f.body = if giant {
                Body::new(pos, GIANT_RADIUS, GIANT_MASS)
            } else {
                Body::new(pos, FOLK_RADIUS, 1.0)
            };
            if let Format::TwoVsTwo { .. } = format
                && let Some(l) = lanterns
                    .iter()
                    .position(|l: &Lantern| l.kind == Kind::Bolas && l.team == team)
            {
                lanterns[l].holders[1] = Some(seat);
                let partner = lanterns[l].holders[0].expect("the first partner holds it");
                lanterns[l].body.pos = pos.lerp(positions[partner.index()], 0.5) * 0.8;
                lantern_of[seat.index()] = Some(l as u8);
                continue;
            }
            let kind = match format {
                Format::TwoVsTwo { .. } => Kind::Bolas,
                _ if giant => Kind::WreckingBall,
                _ => Kind::Flail,
            };
            let spec = kind.spec();
            lantern_of[seat.index()] = Some(lanterns.len() as u8);
            lanterns.push(Lantern {
                body: Body::new(pos * (1.0 - 1.2 / pos.length()), spec.radius, spec.mass),
                kind,
                team,
                holders: [Some(seat), None],
                rope: [millis(spec.rope_max); 2],
                down: false,
                last_clang: None,
            });
        }
        // Barrels start between the players, at angles from the seed, so no seat always has one
        // beside it.
        let mut rng = Rng::new(seed);
        let turn = rng.range_u32(0, 3599) as f32 / 3600.0 * TAU;
        let barrels = std::array::from_fn(|b| {
            let angle = turn + TAU * b as f32 / BARRELS as f32;
            Barrel {
                body: Body::new(
                    Vec2::from_angle(angle) * (RAFT_RADIUS * 0.3),
                    BARREL_RADIUS,
                    BARREL_MASS,
                ),
                status: BarrelStatus::Floating,
                pushed_by: None,
            }
        });
        State {
            seed,
            players,
            format,
            phase: Phase::Intro { t: 0 },
            rng,
            tick: 0,
            freeze: 0,
            folk,
            lanterns,
            lantern_of,
            barrels,
            points: [0; MAX_PLAYERS],
            knockoffs: [0; MAX_PLAYERS],
            falls: [0; MAX_PLAYERS],
            fell_at: [None; MAX_PLAYERS],
            outcome: None,
        }
    }

    fn step(state: &mut State, inputs: &Inputs) {
        match state.phase {
            Phase::Intro { t } => {
                state.phase = if t + 1 >= INTRO_TICKS {
                    Phase::Playing
                } else {
                    Phase::Intro { t: t + 1 }
                };
            }
            Phase::Playing => {
                state.tick += 1;
                state.simulate(inputs);
                state.decide_if_over();
            }
            Phase::Decided { t } => {
                state.simulate(&Inputs::default());
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
        if state.phase != Phase::Playing || !state.folk(seat).standing() {
            return Input::default();
        }
        bot::input(state, seat, difficulty)
    }
}
