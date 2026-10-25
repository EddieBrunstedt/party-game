//! Lantern Ball: bumper football with a heavy glowing ball. Run into it to shove it, dash (A) to
//! send it flying, or hold B to charge a kick and let go to kick it where you face. Charging
//! shows, and slows you down: a rival who dashes into you knocks you back, stuns you and spoils
//! the kick. A fast ball bowls folk over, and pinball bumpers placed from the seed kick it away.
//! A second ball drops in at [`SECOND_BALL_TICK`].
//!
//! Free-for-all: everyone guards a goal of their own on a pitch with one side each (a square for
//! four, a hexagon for three, a rectangle for two). Each goal let in costs a life; at none your
//! goal is walled up and you're out. The last one left wins, or the most lives at [`PLAY_TICKS`]
//! (a level lead plays on until a goal breaks it, for at most [`SUDDEN_DEATH_TICKS`]).
//! 2v2 and duel: two goals, [`LIVES`] a side, the same way. 1v3: the solo is a giant keeper
//! guarding a wide goal; the three need [`TRIO_GOAL`] goals within [`SOLO_TICKS`], and every goal
//! the keeper puts in their small one takes one back.
//!
//! The floor uses `sim_core::physics`' plane: `x` to the right, `y` towards the camera.

use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::hash::{Hash, Hasher};

use sim_core::hash::hash_f32;
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
pub const PLAY_TICKS: u32 = 75 * 60;
/// 1v3 is shorter: the keeper holds out this long.
pub const SOLO_TICKS: u32 = 60 * 60;
/// Level at the end: play on until a goal breaks the tie, or this long.
pub const SUDDEN_DEATH_TICKS: u32 = 20 * 60;
/// After the result, the game runs on this long so the last goal plays out.
pub const DECIDED_TICKS: u16 = 150;

pub const LIVES: u8 = 5;
/// 1v3: goals the three need.
pub const TRIO_GOAL: u32 = 3;
/// The second ball drops in this far into play.
pub const SECOND_BALL_TICK: u32 = 30 * 60;

pub const FOLK_RADIUS: f32 = 0.5;
pub const MOVE_SPEED: f32 = 6.0;
pub const ACCEL: f32 = 24.0;
const DRAG: f32 = 1.0;
/// Knocked about, you slide: this is all that slows you.
const STUN_DRAG: f32 = 2.5;
/// Nobody goes faster than this, however hard they're hit.
const MAX_FOLK_SPEED: f32 = 16.0;

pub const DASH_SPEED: f32 = 14.0;
pub const DASH_TICKS: u16 = 10;
pub const DASH_COOLDOWN: u16 = 50;
/// A dash into a rival sends them off at this speed and stuns them this long.
pub const DASH_KNOCK: f32 = 11.0;
pub const DASH_STUN: u16 = 30;
/// A dash into a ball sends it off at least this fast.
pub const DASH_BALL_SPEED: f32 = 13.0;

/// Holding B charges a kick to full in this long; you run at this share of your speed meanwhile.
pub const CHARGE_TICKS: u16 = 40;
pub const CHARGE_SLOW: f32 = 0.55;
/// A kick's speed, from a tap to a full charge.
pub const KICK_MIN: f32 = 9.0;
pub const KICK_MAX: f32 = 22.0;
/// A ball this close (between surfaces) can be kicked.
pub const KICK_REACH: f32 = 0.55;
/// A kick adds to the ball's motion, which keeps this share: a tap can't stop a hard shot dead.
pub const KICK_KEEP: f32 = 0.6;

pub const BALL_RADIUS: f32 = 0.85;
pub const BALL_MASS: f32 = 2.0;
const BALL_DRAG: f32 = 0.35;
pub const MAX_BALL_SPEED: f32 = 24.0;
/// Balls off walls and each other, folk off each other and off balls.
const WALL_BOUNCE: f32 = 0.8;
const BALL_BOUNCE: f32 = 0.9;
const FOLK_BOUNCE: f32 = 0.4;
const FOLK_WALL_BOUNCE: f32 = 0.2;
const TOUCH_BOUNCE: f32 = 0.5;
/// A ball this much faster than you bowls you over.
pub const BOWL_SPEED: f32 = 10.0;
pub const BOWL_STUN: u16 = 18;
/// A goal that glances in off a defender counts for the rival who touched it before them, if
/// that was this recently.
pub const SHOT_CREDIT_TICKS: u32 = 120;
/// A ball slower than this for this long hops off by itself, this fast, somewhere from the seed.
pub const RESTLESS_SPEED: f32 = 1.0;
pub const RESTLESS_TICKS: u16 = 120;
pub const RESTLESS_HOP: f32 = 9.0;
/// Goals start widening this far into play, and are this much wider at its end.
pub const WIDEN_FROM: u32 = 30 * 60;
pub const WIDEN: f32 = 0.4;
/// Each goal you score takes this off your own goal's half-width, down to this share of its
/// starting width.
pub const SCORE_SHRINK: f32 = 0.35;
pub const NARROWEST: f32 = 0.7;
/// After a goal the ball rolls on into the net this long, then drops back in over the middle.
pub const SCORED_TICKS: u16 = 45;
pub const DROP_TICKS: u16 = 75;
/// A landing ball shoves folk this near away at this speed.
const LANDING_REACH: f32 = 2.0;
const LANDING_SHOVE: f32 = 7.0;

pub const POST_RADIUS: f32 = 0.22;
/// In front of every open goal is a half-circle out past the posts by this much (the crease)
/// that only the ball may enter, so nobody can stand on their own line as a wall. The 1v3 keeper
/// may: keeping is its job.
pub const CREASE_MARGIN: f32 = 0.3;
pub const BUMPER_RADIUS: f32 = 0.55;
/// A ball off a bumper leaves at least this fast; folk at this.
pub const BUMPER_KICK: f32 = 12.0;
const BUMPER_SHOVE: f32 = 7.0;

/// 1v3: the keeper is big, heavy and a little slow.
pub const KEEPER_RADIUS: f32 = 0.8;
pub const KEEPER_MASS: f32 = 2.5;
pub const KEEPER_SPEED: f32 = 5.4;

pub struct LanternBall;

/// A goal in one side of the pitch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Goal {
    /// Who it belongs to: the seat in free-for-all, the team ([`Format::team`]) otherwise.
    pub owner: u8,
    pub half_width: f32,
    /// What it starts at, before it widens ([`WIDEN_FROM`]).
    pub base_half_width: f32,
    /// Walled up once its owner is out.
    pub open: bool,
}

/// One straight side of the pitch, which is convex: the points `p` with
/// `p.dot(normal) <= offset`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Side {
    /// Points out of the pitch.
    pub normal: Vec2,
    pub offset: f32,
    pub goal: Option<Goal>,
}

impl Side {
    pub fn center(&self) -> Vec2 {
        self.normal * self.offset
    }

    /// Along the side, from its centre.
    pub fn tangent(&self) -> Vec2 {
        self.normal.perp()
    }

    pub fn lateral(&self, p: Vec2) -> f32 {
        (p - self.center()).dot(self.tangent())
    }

    /// How far past the line `p` is (negative inside).
    pub fn depth(&self, p: Vec2) -> f32 {
        p.dot(self.normal) - self.offset
    }

    /// The goal's posts, if it has one.
    pub fn posts(&self) -> Option<[Vec2; 2]> {
        let goal = self.goal?;
        let along = self.tangent() * goal.half_width;
        Some([self.center() - along, self.center() + along])
    }

    pub fn open_goal(&self) -> Option<Goal> {
        self.goal.filter(|g| g.open)
    }

    /// The crease's radius, round the goal's centre, while the goal is open.
    pub fn crease(&self) -> Option<f32> {
        self.open_goal().map(|g| g.half_width + CREASE_MARGIN)
    }
}

impl Hash for Side {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.normal.hash(state);
        hash_f32(self.offset, state);
        if let Some(goal) = self.goal {
            goal.owner.hash(state);
            hash_f32(goal.half_width, state);
            hash_f32(goal.base_half_width, state);
            goal.open.hash(state);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Playing,
    /// Lost their last life `t` ticks ago (free-for-all).
    Out {
        t: u16,
    },
    /// Outside a duel.
    Benched,
}

#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct Folk {
    pub body: Body,
    /// The way it last pushed the stick (and kicks and dashes): a unit vector.
    pub facing: Vec2,
    pub status: Status,
    pub keeper: bool,
    /// Ticks left in the current dash, and until the next.
    pub dash: u16,
    pub dash_cooldown: u16,
    /// Ticks left knocked about, unable to steer.
    pub stun: u16,
    /// Holding B since a fresh press: the kick's charge in ticks.
    pub charging: bool,
    pub charge: u16,
    /// Play tick of the last kick (hit or whiffed), for the view.
    pub kicked_at: Option<u32>,
    /// Play tick it was last knocked down, and by whom (none for a ball or bumper).
    pub knocked: Option<(u32, Option<Seat>)>,
}

impl Folk {
    pub fn playing(&self) -> bool {
        self.status == Status::Playing
    }

    fn speed(&self) -> f32 {
        if self.keeper {
            KEEPER_SPEED
        } else {
            MOVE_SPEED
        }
    }

    /// The kick's charge, 0 to 1.
    pub fn charge_share(&self) -> f32 {
        (self.charge as f32 / CHARGE_TICKS as f32).min(1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BallStatus {
    /// Waiting to come in (the second ball, before its time).
    Waiting,
    /// Falling in over the middle; lands when `t` reaches [`DROP_TICKS`].
    Dropping {
        t: u16,
    },
    Rolling,
    /// Went into the goal in side `side` `t` ticks ago.
    Scored {
        t: u16,
        side: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct Ball {
    pub body: Body,
    pub status: BallStatus,
    /// Who last touched it.
    pub touched: Option<Seat>,
    /// The last one on another side than [`Ball::touched`] to touch it, and when it passed
    /// from them.
    pub rival: Option<(Seat, u32)>,
    /// Play tick it last hit a wall, post or bumper hard, for the view.
    pub bounced_at: Option<u32>,
    /// Ticks it has sat nearly still; at [`RESTLESS_TICKS`] it hops off by itself.
    pub still: u16,
}

impl Ball {
    pub fn rolling(&self) -> bool {
        self.status == BallStatus::Rolling
    }

    fn touch(&mut self, seat: Seat, tick: u32, format: &Format) {
        if let Some(last) = self.touched
            && format.team(last) != format.team(seat)
        {
            self.rival = Some((last, tick));
        }
        self.touched = Some(seat);
    }
}

/// A goal that went in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GoalEvent {
    pub tick: u32,
    /// The goal's owner ([`Goal::owner`]).
    pub against: u8,
    /// Who it counts for: whoever touched the ball last, unless that was a defender it glanced
    /// off, in which case the rival who touched it before, within [`SHOT_CREDIT_TICKS`].
    pub by: Option<Seat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Intro {
        t: u16,
    },
    Playing,
    /// The result is in; the pitch runs on for a moment without anyone steering.
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
    /// Ticks of play so far.
    pub tick: u32,
    pub folk: [Folk; MAX_PLAYERS],
    pub balls: [Ball; 2],
    pub sides: Vec<Side>,
    pub bumpers: Vec<Vec2>,
    /// Lives by goal owner (the seat in free-for-all, the team otherwise).
    pub lives: [u8; MAX_PLAYERS],
    /// 1v3: the three's goals, less the ones the keeper took back.
    pub trio_goals: u32,
    /// Goals each seat put in (last touch, against someone else).
    pub scored: [u32; MAX_PLAYERS],
    /// Play tick each seat went out (free-for-all).
    pub out_at: [Option<u32>; MAX_PLAYERS],
    pub last_goal: Option<GoalEvent>,
    pub outcome: Option<Outcome>,
    rng: Rng,
}

impl State {
    pub fn folk(&self, seat: Seat) -> &Folk {
        &self.folk[seat.index()]
    }

    /// Seats in play (not benched, not out).
    pub fn playing(&self) -> impl Iterator<Item = Seat> + '_ {
        self.players
            .seats()
            .filter(|s| self.folk[s.index()].playing())
    }

    /// Seats taking part, out or not (benched duel seats aren't).
    pub fn taking_part(&self) -> impl Iterator<Item = Seat> + '_ {
        self.players
            .seats()
            .filter(|s| self.folk[s.index()].status != Status::Benched)
    }

    pub fn keeper(&self) -> Option<Seat> {
        match self.format {
            Format::OneVsThree { solo } => Some(solo),
            _ => None,
        }
    }

    pub fn play_ticks(&self) -> u32 {
        if self.keeper().is_some() {
            SOLO_TICKS
        } else {
            PLAY_TICKS
        }
    }

    pub fn ticks_left(&self) -> u32 {
        self.play_ticks().saturating_sub(self.tick)
    }

    /// Time ran out with the lead level, and the next goal decides.
    pub fn sudden_death(&self) -> bool {
        self.tick >= self.play_ticks() && self.outcome.is_none()
    }

    /// The owner whose goal a seat guards.
    pub fn owner_of(&self, seat: Seat) -> Option<u8> {
        self.format.team(seat)
    }

    /// The side holding `owner`'s goal.
    pub fn goal_side(&self, owner: u8) -> Option<usize> {
        self.sides
            .iter()
            .position(|s| s.goal.is_some_and(|g| g.owner == owner))
    }

    /// Owners still in it: their lives (free-for-all, 2v2, duel).
    fn standing_owners(&self) -> Vec<u8> {
        let mut owners: Vec<u8> = self
            .taking_part()
            .filter(|s| self.folk(*s).playing())
            .filter_map(|s| self.owner_of(s))
            .collect();
        owners.sort();
        owners.dedup();
        owners
    }

    /// Whether more than one side shares the lead.
    fn level(&self) -> bool {
        let lives: Vec<u8> = self
            .standing_owners()
            .into_iter()
            .map(|o| self.lives[o as usize])
            .collect();
        let best = lives.iter().copied().max().unwrap_or(0);
        lives.iter().filter(|l| **l == best).count() > 1
    }

    fn step_folk(&mut self, inputs: &Inputs) {
        let tick = self.tick;
        let players = self.players;
        for seat in players.seats() {
            if !self.folk(seat).playing() {
                continue;
            }
            let input = &inputs.current[seat.index()];
            let stick = Vec2::from_stick(input);
            let folk = &mut self.folk[seat.index()];
            folk.dash = folk.dash.saturating_sub(1);
            folk.dash_cooldown = folk.dash_cooldown.saturating_sub(1);
            folk.stun = folk.stun.saturating_sub(1);
            let free = folk.stun == 0 && folk.dash == 0;
            if free && stick != Vec2::ZERO {
                folk.facing = stick.normalize_or_zero();
            }
            if free && folk.dash_cooldown == 0 && inputs.just_pressed(seat, Button::A) {
                folk.dash = DASH_TICKS;
                folk.dash_cooldown = DASH_COOLDOWN;
                folk.body.vel = folk.facing * DASH_SPEED;
                folk.charging = false;
                folk.charge = 0;
            }
            let mut kick = None;
            if folk.stun == 0 && folk.dash == 0 {
                if inputs.just_pressed(seat, Button::B) {
                    folk.charging = true;
                    folk.charge = 0;
                } else if folk.charging && input.held(Button::B) {
                    folk.charge = (folk.charge + 1).min(CHARGE_TICKS);
                } else if folk.charging {
                    folk.charging = false;
                    kick = Some(KICK_MIN + (KICK_MAX - KICK_MIN) * folk.charge_share());
                    folk.charge = 0;
                    folk.kicked_at = Some(tick);
                }
            }
            let drag = if folk.dash > 0 {
                0.0
            } else if folk.stun > 0 {
                STUN_DRAG
            } else {
                let speed = if folk.charging {
                    folk.speed() * CHARGE_SLOW
                } else {
                    folk.speed()
                };
                folk.body.steer(stick, speed, ACCEL);
                DRAG
            };
            folk.body.integrate(drag);
            if let Some(power) = kick {
                self.kick(seat, power);
            }
        }
    }

    /// Kick the nearest ball in reach where the folk faces.
    fn kick(&mut self, seat: Seat, power: f32) {
        let folk = *self.folk(seat);
        let reach = |ball: &Ball| {
            ball.body.pos.distance(folk.body.pos) - ball.body.radius - folk.body.radius
        };
        let nearest = (0..self.balls.len())
            .filter(|b| self.balls[*b].rolling() && reach(&self.balls[*b]) <= KICK_REACH)
            .min_by(|a, b| reach(&self.balls[*a]).total_cmp(&reach(&self.balls[*b])));
        if let Some(b) = nearest {
            let ball = &mut self.balls[b];
            ball.body.vel =
                (ball.body.vel * KICK_KEEP + folk.facing * power).clamp_length(MAX_BALL_SPEED);
            ball.touch(seat, self.tick, &self.format);
        }
    }

    fn integrate_balls(&mut self) {
        for ball in &mut self.balls {
            match ball.status {
                BallStatus::Rolling => {
                    // Nobody coming for it: a lantern ball won't sit still for long.
                    ball.still = if ball.body.vel.length() < RESTLESS_SPEED {
                        ball.still + 1
                    } else {
                        0
                    };
                    if ball.still >= RESTLESS_TICKS {
                        let angle = self.rng.range_u32(0, 3599) as f32 / 3600.0 * TAU;
                        ball.body.vel = Vec2::from_angle(angle) * RESTLESS_HOP;
                        ball.still = 0;
                    }
                    ball.body.vel = ball.body.vel.clamp_length(MAX_BALL_SPEED);
                    ball.body.integrate(BALL_DRAG);
                }
                BallStatus::Scored { .. } => ball.body.integrate(3.0),
                BallStatus::Waiting | BallStatus::Dropping { .. } => {}
            }
        }
    }

    /// A dash into a rival knocks them back and stuns them.
    fn knock(&mut self, victim: Seat, by: Seat, normal: Vec2, speed: f32) {
        let tick = self.tick;
        let folk = &mut self.folk[victim.index()];
        folk.body.vel = folk.body.vel * 0.2 + normal * (speed / folk.body.mass.sqrt());
        folk.stun = DASH_STUN;
        folk.dash = 0;
        folk.charging = false;
        folk.charge = 0;
        folk.knocked = Some((tick, Some(by)));
    }

    fn collide_folk(&mut self) {
        let seats: Vec<Seat> = self.playing().collect();
        for (i, &a) in seats.iter().enumerate() {
            for &b in &seats[i + 1..] {
                let (mut fa, mut fb) = (self.folk[a.index()], self.folk[b.index()]);
                let Some(contact) = physics::collide(&mut fa.body, &mut fb.body, FOLK_BOUNCE)
                else {
                    continue;
                };
                self.folk[a.index()] = fa;
                self.folk[b.index()] = fb;
                if self.format.same_team(a, b) {
                    continue;
                }
                // Both checked before either lands, so a head-on clash doesn't favour a seat.
                let rams = |attacker: &Folk, victim: &Folk, normal: Vec2| {
                    attacker.dash > 0 && attacker.facing.dot(normal) > 0.3 && victim.stun == 0
                };
                let a_rams = rams(&fa, &fb, contact.normal);
                let b_rams = rams(&fb, &fa, -contact.normal);
                match (a_rams, b_rams) {
                    (true, true) => {
                        self.knock(a, b, -contact.normal, DASH_KNOCK * 0.6);
                        self.knock(b, a, contact.normal, DASH_KNOCK * 0.6);
                    }
                    (true, false) => {
                        self.folk[a.index()].dash = 0;
                        self.folk[a.index()].body.vel *= 0.3;
                        self.knock(b, a, contact.normal, DASH_KNOCK);
                    }
                    (false, true) => {
                        self.folk[b.index()].dash = 0;
                        self.folk[b.index()].body.vel *= 0.3;
                        self.knock(a, b, -contact.normal, DASH_KNOCK);
                    }
                    (false, false) => {}
                }
            }
        }
    }

    fn collide_balls(&mut self) {
        let tick = self.tick;
        let seats: Vec<Seat> = self.playing().collect();
        for b in 0..self.balls.len() {
            if !self.balls[b].rolling() {
                continue;
            }
            for &seat in &seats {
                let mut ball = self.balls[b];
                let mut folk = self.folk[seat.index()];
                let Some(contact) = physics::collide(&mut folk.body, &mut ball.body, TOUCH_BOUNCE)
                else {
                    continue;
                };
                let closing = contact.impulse / folk.body.mass;
                if folk.dash > 0 {
                    // A dash sends it flying.
                    let along = ball.body.vel.dot(contact.normal);
                    if along < DASH_BALL_SPEED {
                        ball.body.vel += contact.normal * (DASH_BALL_SPEED - along);
                    }
                    folk.dash = 0;
                    folk.body.vel *= 0.3;
                } else if closing > BOWL_SPEED && folk.stun == 0 {
                    folk.stun = BOWL_STUN;
                    folk.charging = false;
                    folk.charge = 0;
                    folk.knocked = Some((tick, None));
                }
                if contact.impulse > 0.0 {
                    ball.touch(seat, tick, &self.format);
                }
                self.balls[b] = ball;
                self.folk[seat.index()] = folk;
            }
        }
        let [first, second] = &mut self.balls;
        if first.rolling() && second.rolling() {
            physics::collide(&mut first.body, &mut second.body, BALL_BOUNCE);
        }
    }

    /// Bumpers, posts and walls, for balls and folk.
    fn collide_pitch(&mut self) {
        let tick = self.tick;
        let posts: Vec<Vec2> = self
            .sides
            .iter()
            .filter_map(|s| s.posts())
            .flatten()
            .collect();
        for ball in self.balls.iter_mut().filter(|b| b.rolling()) {
            for &bumper in &self.bumpers {
                if let Some(contact) =
                    physics::bounce_off_circle(&mut ball.body, bumper, BUMPER_RADIUS, WALL_BOUNCE)
                {
                    let along = ball.body.vel.dot(contact.normal);
                    if along < BUMPER_KICK {
                        ball.body.vel += contact.normal * (BUMPER_KICK - along);
                    }
                    ball.bounced_at = Some(tick);
                }
            }
            for &post in &posts {
                if let Some(contact) =
                    physics::bounce_off_circle(&mut ball.body, post, POST_RADIUS, WALL_BOUNCE)
                    && contact.impulse > 4.0
                {
                    ball.bounced_at = Some(tick);
                }
            }
            for side in &self.sides {
                // The ball goes through an open goal's mouth (the posts stop it at the edges).
                if side
                    .open_goal()
                    .is_some_and(|g| side.lateral(ball.body.pos).abs() < g.half_width)
                {
                    continue;
                }
                if let Some(speed) =
                    physics::bounce_off_wall(&mut ball.body, side.normal, side.offset, WALL_BOUNCE)
                    && speed > 3.0
                {
                    ball.bounced_at = Some(tick);
                }
            }
        }
        let seats: Vec<Seat> = self.playing().collect();
        for seat in seats {
            let folk = &mut self.folk[seat.index()];
            for &bumper in &self.bumpers {
                if let Some(contact) =
                    physics::bounce_off_circle(&mut folk.body, bumper, BUMPER_RADIUS, 0.5)
                {
                    let along = folk.body.vel.dot(contact.normal);
                    if along < BUMPER_SHOVE {
                        folk.body.vel += contact.normal * (BUMPER_SHOVE - along);
                    }
                }
            }
            for &post in &posts {
                physics::bounce_off_circle(&mut folk.body, post, POST_RADIUS, FOLK_WALL_BOUNCE);
            }
            // Folk can't go into the goals, nor (but the keeper) into the creases.
            for side in &self.sides {
                if let Some(crease) = side.crease()
                    && !folk.keeper
                {
                    physics::bounce_off_circle(
                        &mut folk.body,
                        side.center(),
                        crease,
                        FOLK_WALL_BOUNCE,
                    );
                }
                physics::bounce_off_wall(
                    &mut folk.body,
                    side.normal,
                    side.offset,
                    FOLK_WALL_BOUNCE,
                );
            }
            folk.body.vel = folk.body.vel.clamp_length(MAX_FOLK_SPEED);
        }
    }

    /// A ball over an open goal's line is a goal against its owner.
    fn score(&mut self) {
        for b in 0..self.balls.len() {
            let ball = self.balls[b];
            if !ball.rolling() {
                continue;
            }
            let Some(side) = self.sides.iter().position(|s| {
                s.open_goal()
                    .is_some_and(|g| s.lateral(ball.body.pos).abs() < g.half_width)
                    && s.depth(ball.body.pos) > 0.0
            }) else {
                continue;
            };
            self.balls[b].status = BallStatus::Scored {
                t: 0,
                side: side as u8,
            };
            let owner = self.sides[side].goal.expect("scored in a goal").owner;
            let defended = ball.touched.and_then(|s| self.owner_of(s)) == Some(owner);
            let rival = ball
                .rival
                .filter(|(s, at)| {
                    self.owner_of(*s) != Some(owner) && at + SHOT_CREDIT_TICKS > self.tick
                })
                .map(|(s, _)| s);
            let by = if defended {
                rival.or(ball.touched)
            } else {
                ball.touched
            };
            self.goal_against(owner, by);
        }
    }

    fn goal_against(&mut self, owner: u8, by: Option<Seat>) {
        // Once the result is in, goals still go in but no longer count.
        if self.outcome.is_some() {
            return;
        }
        let tick = self.tick;
        self.last_goal = Some(GoalEvent {
            tick,
            against: owner,
            by,
        });
        if let Some(by) = by
            && self.owner_of(by) != Some(owner)
        {
            self.scored[by.index()] += 1;
        }
        if self.keeper().is_some() {
            // The keeper's goal counts for the three; theirs takes one back.
            self.trio_goals = if owner == 0 {
                self.trio_goals + 1
            } else {
                self.trio_goals.saturating_sub(1)
            };
            return;
        }
        let lives = &mut self.lives[owner as usize];
        *lives = lives.saturating_sub(1);
        if *lives == 0 && self.format == Format::FreeForAll {
            let seat = Seat(owner);
            self.folk[seat.index()].status = Status::Out { t: 0 };
            self.out_at[seat.index()] = Some(tick);
            if let Some(side) = self.goal_side(owner)
                && let Some(goal) = &mut self.sides[side].goal
            {
                goal.open = false;
            }
        }
    }

    /// Scored balls drop back in, the second ball arrives, the out sink away.
    fn recover(&mut self) {
        let tick = self.tick;
        let mut landed = false;
        for b in 0..self.balls.len() {
            let ball = &mut self.balls[b];
            ball.status = match ball.status {
                BallStatus::Waiting if b == 1 && tick >= SECOND_BALL_TICK => {
                    BallStatus::Dropping { t: 0 }
                }
                BallStatus::Scored { t, .. } if t + 1 >= SCORED_TICKS => {
                    BallStatus::Dropping { t: 0 }
                }
                BallStatus::Scored { t, side } => BallStatus::Scored { t: t + 1, side },
                BallStatus::Dropping { t } if t + 1 >= DROP_TICKS => {
                    ball.body.pos = Vec2::ZERO;
                    ball.body.vel = Vec2::ZERO;
                    ball.touched = None;
                    ball.rival = None;
                    landed = true;
                    BallStatus::Rolling
                }
                BallStatus::Dropping { t } => {
                    ball.body.pos = Vec2::ZERO;
                    ball.body.vel = Vec2::ZERO;
                    BallStatus::Dropping { t: t + 1 }
                }
                other => other,
            };
        }
        if landed {
            self.land();
        }
        for folk in &mut self.folk {
            if let Status::Out { t } = folk.status {
                folk.status = Status::Out {
                    t: t.saturating_add(1),
                };
            }
        }
    }

    /// A ball landing in the middle shoves away whoever is under it.
    fn land(&mut self) {
        let seats: Vec<Seat> = self.playing().collect();
        for seat in seats {
            let folk = &mut self.folk[seat.index()];
            let away = folk.body.pos;
            let distance = away.length();
            if distance < LANDING_REACH {
                let dir = if distance > 1e-4 {
                    away * (1.0 / distance)
                } else {
                    Vec2::new(0.0, 1.0)
                };
                folk.body.vel += dir * LANDING_SHOVE;
            }
        }
    }

    fn decide_if_over(&mut self) {
        if self.outcome.is_some() {
            return;
        }
        let time_up = self.tick >= self.play_ticks();
        let over = match self.format {
            Format::OneVsThree { .. } => time_up || self.trio_goals >= TRIO_GOAL,
            _ => {
                let standing = self.standing_owners();
                let one_left = match self.format {
                    Format::FreeForAll => standing.len() <= 1,
                    _ => standing.iter().any(|o| self.lives[*o as usize] == 0),
                };
                one_left
                    || (time_up
                        && (!self.level() || self.tick >= self.play_ticks() + SUDDEN_DEATH_TICKS))
            }
        };
        if !over {
            return;
        }
        self.outcome = Some(match self.format {
            Format::FreeForAll => {
                // Those still in rank above those out, and the later out above the earlier.
                let mut scores = [0; MAX_PLAYERS];
                for seat in self.taking_part() {
                    scores[seat.index()] = match self.out_at[seat.index()] {
                        Some(tick) => tick,
                        None => 100_000 + self.lives[seat.index()] as u32,
                    };
                }
                Outcome::from_scores(&self.players, &scores)
            }
            Format::OneVsThree { .. } => {
                let trio_won = self.trio_goals >= TRIO_GOAL;
                let scores = [u32::from(!trio_won), u32::from(trio_won), 0, 0];
                Outcome::from_team_scores(&self.players, &self.format, &scores)
            }
            _ => {
                let scores = self.lives.map(u32::from);
                Outcome::from_team_scores(&self.players, &self.format, &scores)
            }
        });
        self.phase = Phase::Decided { t: 0 };
    }

    /// Goals widen from [`WIDEN_FROM`] to the end of play, by [`WIDEN`] of their width, so
    /// standing in front of yours keeps out less and less (not the keeper's: 1v3 is balanced
    /// on its width).
    fn widen_goals(&mut self) {
        if self.keeper().is_some() {
            return;
        }
        let span = self.play_ticks().saturating_sub(WIDEN_FROM).max(1);
        let progress = (self.tick.saturating_sub(WIDEN_FROM) as f32 / span as f32).min(1.0);
        for side in &mut self.sides {
            if let Some(goal) = &mut side.goal {
                // Every goal you score narrows yours back a little.
                let scored: u32 = self
                    .players
                    .seats()
                    .filter(|s| self.format.team(*s) == Some(goal.owner))
                    .map(|s| self.scored[s.index()])
                    .sum();
                let wide = goal.base_half_width * (1.0 + WIDEN * progress);
                goal.half_width =
                    (wide - SCORE_SHRINK * scored as f32).max(goal.base_half_width * NARROWEST);
            }
        }
    }

    fn simulate(&mut self, inputs: &Inputs) {
        self.widen_goals();
        self.step_folk(inputs);
        self.integrate_balls();
        self.collide_folk();
        self.collide_balls();
        self.collide_pitch();
        self.score();
        self.recover();
    }
}

/// A goal on a side, owned by `owner`.
fn goal(owner: u8, half_width: f32) -> Option<Goal> {
    Some(Goal {
        owner,
        half_width,
        base_half_width: half_width,
        open: true,
    })
}

fn side(angle: f32, offset: f32, goal: Option<Goal>) -> Side {
    Side {
        normal: Vec2::from_angle(angle),
        offset,
        goal,
    }
}

/// A rectangle with goals at the left and right ends and its corners cut off `cut` along each wall.
fn rectangle(half: Vec2, cut: f32, left: Option<Goal>, right: Option<Goal>) -> Vec<Side> {
    let corner = (half.x - cut + half.y) * std::f32::consts::FRAC_1_SQRT_2;
    vec![
        side(PI, half.x, left),
        side(0.0, half.x, right),
        side(-FRAC_PI_2, half.y, None),
        side(FRAC_PI_2, half.y, None),
        side(PI * 0.25, corner, None),
        side(PI * 0.75, corner, None),
        side(PI * 1.25, corner, None),
        side(PI * 1.75, corner, None),
    ]
}

/// A regular polygon with a goal in every side (the first at the bottom, towards the camera,
/// then round clockwise as seen on screen), and its corners cut off `cut` along each side.
fn polygon(owners: &[u8], inradius: f32, cut: f32, half_width: f32) -> Vec<Side> {
    let n = owners.len() as f32;
    let corner = inradius / sim_core::math::cos(PI / n) - cut * sim_core::math::sin(PI / n);
    let mut sides = Vec::new();
    for (k, owner) in owners.iter().enumerate() {
        let angle = FRAC_PI_2 + TAU * k as f32 / n;
        sides.push(side(angle, inradius, goal(*owner, half_width)));
        sides.push(side(angle + PI / n, corner, None));
    }
    sides
}

pub const FFA_HALF: f32 = 8.0;
pub const FFA_GOAL_HALF: f32 = 2.9;
pub const HEX_INRADIUS: f32 = 7.5;
pub const FIELD_HALF: Vec2 = Vec2::new(10.0, 6.5);
pub const DUEL_HALF: Vec2 = Vec2::new(9.0, 6.0);
pub const DUEL_GOAL_HALF: f32 = 2.6;
pub const TEAM_GOAL_HALF: f32 = 3.2;
/// 1v3: the keeper's wide goal, and the three's small one.
pub const KEEPER_GOAL_HALF: f32 = 3.5;
pub const TRIO_GOAL_HALF: f32 = 1.4;

fn pitch(players: &PlayerSetup, format: &Format) -> Vec<Side> {
    match *format {
        Format::FreeForAll => {
            let owners: Vec<u8> = players.seats().map(|s| s.0).collect();
            match owners.len() {
                4 => polygon(&owners, FFA_HALF, 2.0, FFA_GOAL_HALF),
                3 => polygon(&owners, HEX_INRADIUS, 6.5, FFA_GOAL_HALF),
                _ => rectangle(
                    DUEL_HALF,
                    1.6,
                    goal(owners[0], DUEL_GOAL_HALF),
                    owners.get(1).and_then(|o| goal(*o, DUEL_GOAL_HALF)),
                ),
            }
        }
        Format::OneVsThree { .. } => rectangle(
            FIELD_HALF,
            1.6,
            goal(0, KEEPER_GOAL_HALF),
            goal(1, TRIO_GOAL_HALF),
        ),
        Format::TwoVsTwo { .. } => rectangle(
            FIELD_HALF,
            1.6,
            goal(0, TEAM_GOAL_HALF),
            goal(1, TEAM_GOAL_HALF),
        ),
        Format::Duel { .. } => rectangle(
            DUEL_HALF,
            1.6,
            goal(0, DUEL_GOAL_HALF),
            goal(1, DUEL_GOAL_HALF),
        ),
    }
}

fn unit(rng: &mut Rng) -> f32 {
    rng.range_u32(0, 1000) as f32 / 1000.0
}

/// Bumpers from the seed, placed with the pitch's symmetry so every goal sees the same layout:
/// a ring of one per side on a polygon, a mirrored four on a rectangle.
fn bumpers(rng: &mut Rng, sides: &[Side], rectangle: bool) -> Vec<Vec2> {
    let goals = sides.iter().filter(|s| s.goal.is_some()).count();
    if rectangle {
        let half_x = sides[1].offset;
        let x = 2.5 + unit(rng) * (half_x - 5.5);
        let y = 1.8 + unit(rng) * 2.0;
        vec![
            Vec2::new(-x, -y),
            Vec2::new(x, -y),
            Vec2::new(-x, y),
            Vec2::new(x, y),
        ]
    } else {
        let radius = 3.0 + unit(rng) * 1.2;
        let turn = unit(rng) * TAU / goals as f32;
        (0..goals)
            .map(|k| Vec2::from_angle(FRAC_PI_2 + turn + TAU * k as f32 / goals as f32) * radius)
            .collect()
    }
}

/// Where each seat starts: in front of its own goal; teams side by side.
fn start_positions(players: &PlayerSetup, format: &Format, sides: &[Side]) -> [Vec2; MAX_PLAYERS] {
    let mut positions = [Vec2::ZERO; MAX_PLAYERS];
    let mut placed = [0usize; MAX_PLAYERS];
    for seat in players.seats() {
        let Some(owner) = format.team(seat) else {
            continue;
        };
        let Some(side) = sides
            .iter()
            .find(|s| s.goal.is_some_and(|g| g.owner == owner))
        else {
            continue;
        };
        let k = placed[owner as usize];
        placed[owner as usize] += 1;
        positions[seat.index()] = match format {
            Format::OneVsThree { solo } if *solo == seat => side.center() - side.normal * 1.5,
            // The three start upfield, spread across it.
            Format::OneVsThree { .. } => Vec2::new(-side.center().x * 0.5, (k as f32 - 1.0) * 2.6),
            Format::TwoVsTwo { .. } => {
                side.center() - side.normal * 4.5 + side.tangent() * (k as f32 * 2.0 - 1.0) * 2.0
            }
            _ => side.center() - side.normal * (side.crease().unwrap_or(0.0) + 1.0),
        };
    }
    positions
}

impl Minigame for LanternBall {
    type State = State;

    const ID: &'static str = "lantern-ball";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[
        FormatKind::FreeForAll,
        FormatKind::OneVsThree,
        FormatKind::TwoVsTwo,
        FormatKind::Duel,
    ];
    const MAX_TICKS: u32 =
        INTRO_TICKS as u32 + PLAY_TICKS + SUDDEN_DEATH_TICKS + DECIDED_TICKS as u32 + 60;

    fn new(seed: u64, players: PlayerSetup, format: Format) -> State {
        let sides = pitch(&players, &format);
        let mut rng = Rng::new(seed);
        let rectangle = format != Format::FreeForAll || players.count() == 2;
        let bumpers = bumpers(&mut rng, &sides, rectangle);
        let positions = start_positions(&players, &format, &sides);
        let mut folk = [Folk {
            body: Body::new(Vec2::ZERO, FOLK_RADIUS, 1.0),
            facing: Vec2::new(0.0, -1.0),
            status: Status::Benched,
            keeper: false,
            dash: 0,
            dash_cooldown: 0,
            stun: 0,
            charging: false,
            charge: 0,
            kicked_at: None,
            knocked: None,
        }; MAX_PLAYERS];
        for seat in players.seats() {
            if format.team(seat).is_none() {
                continue;
            }
            let pos = positions[seat.index()];
            let keeper = matches!(format, Format::OneVsThree { solo } if solo == seat);
            let f = &mut folk[seat.index()];
            f.status = Status::Playing;
            f.keeper = keeper;
            f.facing = (-pos).normalize_or_zero();
            f.body = if keeper {
                Body::new(pos, KEEPER_RADIUS, KEEPER_MASS)
            } else {
                Body::new(pos, FOLK_RADIUS, 1.0)
            };
        }
        let ball = |status| Ball {
            body: Body::new(Vec2::ZERO, BALL_RADIUS, BALL_MASS),
            status,
            touched: None,
            rival: None,
            bounced_at: None,
            still: 0,
        };
        State {
            seed,
            players,
            format,
            phase: Phase::Intro { t: 0 },
            tick: 0,
            folk,
            balls: [ball(BallStatus::Rolling), ball(BallStatus::Waiting)],
            sides,
            bumpers,
            lives: [LIVES; MAX_PLAYERS],
            trio_goals: 0,
            scored: [0; MAX_PLAYERS],
            out_at: [None; MAX_PLAYERS],
            last_goal: None,
            outcome: None,
            rng,
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
        if state.phase != Phase::Playing || !state.folk(seat).playing() {
            return Input::default();
        }
        bot::input(state, seat, difficulty)
    }
}
