//! Lantern Run: little lantern gliders race through a night canyon and into a cave whose walls
//! close in. Everyone flies forward by themselves at [`CRUISE`]; the stick steers across the
//! course, with momentum, so a glider drifts until you thrust against it. The camera follows
//! the leader from [`State::window`] behind. In the canyon it pushes stragglers along, so
//! there's time to learn the controls; in the cave, anyone who falls behind it is out. Last
//! glider flying wins.
//!
//! What costs ground is hitting things: an obstacle spins you out and stalls you
//! ([`CRASH_SPEED`]). What gains it: the throttle (hold A to fly faster while the meter lasts;
//! it refills by itself, and fast while grazing obstacles and rivals; flat out, you ram anyone
//! you touch aside), flying in someone's slipstream (lined up behind them), and the grapple (B):
//! hook the glider ahead and hold B, and it tows you while you drag it back, until
//! you slingshot past. The one in front fights back by threading a gap so an obstacle comes
//! between you: the rope catches on it and yanks you in, unless you let go first.
//!
//! Free-for-all and duel: last one flying wins; at [`PLAY_TICKS`] the furthest along does.
//! 2v2: partners can hook each other for a tow that doesn't slow the one ahead, and a team is
//! out when both are. 1v3: the solo flies a wide, heavy barge that starts [`HEAD_START`] ahead
//! and drops mines behind it (A) instead of throttling, and B cuts the ropes hooked on it. It
//! ploughs through obstacles, losing little, and speeds up as the three close in
//! ([`State::barge_cruise`]), so they catch it only by taking turns in each other's slipstreams,
//! throttling and hooking it. The camera follows the three; they win if any of them gets past the
//! barge, and the barge wins if it's still ahead after [`BARGE_TICKS`], or all three are out.

use std::hash::{Hash, Hasher};

use sim_core::hash::hash_f32;
use sim_core::physics::{self, Body, DT, Vec2};
use sim_core::{
    Button, Difficulty, Format, FormatKind, Input, Inputs, MAX_PLAYERS, Minigame, Outcome,
    PlayerSetup, Seat,
};

mod bot;
pub mod course;
#[cfg(test)]
mod tests;

pub use course::{Course, Obstacle, Shape, half_extents};

/// The opening countdown, the same in every minigame.
pub const INTRO_TICKS: u16 = sim_core::COUNTDOWN_TICKS;
pub const PLAY_TICKS: u32 = 100 * 60;
/// After the result, the game runs on this long so the last crash plays out.
pub const DECIDED_TICKS: u16 = 150;

/// Everyone's forward speed when nothing is helping or hindering them.
pub const CRUISE: f32 = 18.0;
pub const MIN_SPEED: f32 = CRUISE * 0.25;
pub const MAX_SPEED: f32 = CRUISE * 1.7;
/// Below cruising speed you speed back up this fast (units a second, a second); above it,
/// you slow down this fast.
pub const SPEED_RECOVER: f32 = 9.0;
pub const SPEED_BLEED: f32 = 6.0;

pub const SHIP_RADIUS: f32 = 0.45;
/// Gliders closer than this along the course can touch.
pub const SHIP_LENGTH: f32 = 1.2;
/// Steering across the course: top speed and thrust.
pub const STEER_SPEED: f32 = 7.0;
pub const STEER_ACCEL: f32 = 18.0;
/// Spinning out after a crash, you drift with only this to slow you.
const STUN_DRAG: f32 = 1.5;
const BUMP_RESTITUTION: f32 = 0.6;
const WALL_RESTITUTION: f32 = 0.5;
/// Hitting a wall costs this share of your speed.
pub const WALL_SCRAPE: f32 = 0.05;

/// Lined up behind someone (within this distance behind and this far off their line), their
/// slipstream raises your speed to cruise times `1 + DRAFT_BONUS`.
pub const DRAFT_LENGTH: f32 = 9.0;
pub const DRAFT_WIDTH: f32 = 1.1;
pub const DRAFT_BONUS: f32 = 0.14;

/// Hitting an obstacle (or a mine) cuts your speed to this share, spins you out for
/// [`CRASH_STUN`], and leaves you passing through obstacles for [`GHOST_TICKS`].
pub const CRASH_SPEED: f32 = 0.5;
pub const CRASH_STUN: u16 = 30;
pub const GHOST_TICKS: u16 = 60;
/// A crash also throws you off across the course this fast.
const CRASH_THROW: f32 = 3.0;

/// The throttle's meter, in thousandths; everyone starts with [`METER_START`]. Holding A burns
/// [`THROTTLE_BURN`] a tick of it and raises the speed you settle at by `THROTTLE - 1` times
/// [`CRUISE`] (on top of a slipstream), reached at [`THROTTLE_ACCEL`].
pub const METER_MAX: u16 = 1000;
pub const METER_START: u16 = 500;
pub const THROTTLE_BURN: u16 = 5;
pub const THROTTLE: f32 = 1.25;
pub const THROTTLE_ACCEL: f32 = 12.0;
/// Flat out, a glider steers with only this share of its thrust: going fast through obstacles
/// is a risk.
pub const THROTTLE_STEER: f32 = 0.6;
/// The meter refills by itself this much every [`METER_IDLE_TICKS`] while you're not
/// throttling...
pub const METER_IDLE: u16 = 1;
pub const METER_IDLE_TICKS: u32 = 2;
/// ...plus up to this much a tick grazing past an obstacle within [`GRAZE`] of it...
pub const GRAZE: f32 = 0.7;
pub const GRAZE_RATE: f32 = 60.0;
/// ...and this much a tick flying alongside a rival.
pub const RIVAL_GRAZE_RATE: u16 = 2;
pub const RIVAL_GRAZE: f32 = 0.5;
/// A glider throttling faster than this rams rivals it touches aside at [`RAM_PUSH`], spinning
/// them for [`RAM_STUN`].
pub const RAM_SPEED: f32 = CRUISE * 1.15;
pub const RAM_PUSH: f32 = 9.0;
pub const RAM_STUN: u16 = 18;

/// The grapple reaches gliders from [`HOOK_MIN`] to [`HOOK_RANGE`] ahead, within [`HOOK_WIDTH`]
/// of your line.
pub const HOOK_MIN: f32 = 1.5;
pub const HOOK_RANGE: f32 = 14.0;
pub const HOOK_WIDTH: f32 = 4.0;
/// A hooked rope tows the two together this hard along the course (split by their masses)...
pub const TOW: f32 = 12.0;
/// ...and drags each across towards the other at this share of the distance between them a
/// second (a drag the glider's own thrust doesn't cancel, only steering away does).
pub const ROPE_PULL: f32 = 0.8;
/// A hook lets go by itself after this long, or once you're level with the one you hooked.
pub const HOOK_TICKS: u16 = 150;
/// A rope passing closer than this to an obstacle catches on it.
pub const ROPE_CATCH: f32 = 0.3;
/// Before you can fire again: after a hook, and after a miss.
pub const HOOK_COOLDOWN: u16 = 240;
pub const MISS_COOLDOWN: u16 = 30;

/// The camera follows the leader from this far behind; it starts closing in after
/// [`CLOSE_IN_AFTER`] and is [`WINDOW_END`] behind after another [`CLOSE_IN_TICKS`].
pub const WINDOW_START: f32 = 18.0;
pub const WINDOW_END: f32 = 5.0;
pub const CLOSE_IN_AFTER: u32 = 35 * 60;
pub const CLOSE_IN_TICKS: u32 = 35 * 60;
/// In the canyon, stragglers are pushed on to this far ahead of the camera's kill line, so
/// they reach the cave with a little room.
pub const CANYON_CUSHION: f32 = 10.0;

/// 1v3: the barge is wide and heavy, and starts this far ahead. It cruises at cruise times
/// [`BARGE_FAR`] with the three [`BARGE_EASE`] or more behind, speeding up as the nearest closes
/// in, to cruise times [`BARGE_NEAR`] once they're level...
pub const BARGE_RADIUS: f32 = 0.8;
pub const BARGE_MASS: f32 = 3.0;
pub const BARGE_FAR: f32 = 0.94;
pub const BARGE_NEAR: f32 = 1.197;
pub const BARGE_EASE: f32 = 30.0;
pub const HEAD_START: f32 = 24.0;
/// ...ploughs through what it hits, keeping this share of its speed and spinning only for
/// [`BARGE_STUN`]...
pub const BARGE_CRASH_SPEED: f32 = 0.7;
pub const BARGE_STUN: u16 = 12;
/// ...and wins if none of the three has got past it after this long.
pub const BARGE_TICKS: u32 = 75 * 60;
/// ...and B cuts every rope hooked on it, then needs this long before it can again.
pub const CUT_COOLDOWN: u16 = 60;
/// ...and drops a mine behind it every so often.
pub const MINE_RADIUS: f32 = 0.6;
pub const MINE_COOLDOWN: u16 = 150;

pub struct LanternRun;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Flying,
    /// Fell behind the camera `t` ticks ago.
    Out {
        t: u16,
    },
    /// Outside a duel.
    Benched,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hook {
    pub target: Seat,
    /// Ticks since it caught.
    pub t: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ship {
    /// Across the course: `x` to the right, `y` up.
    pub body: Body,
    /// How far along the course, and how fast it's going there.
    pub z: f32,
    pub speed: f32,
    pub status: Status,
    pub barge: bool,
    /// Ticks left spinning out (unable to steer), and passing through obstacles.
    pub stun: u16,
    pub ghost: u16,
    /// Throttling this tick (holding A with something in the meter).
    pub throttle: bool,
    pub meter: u16,
    pub hook: Option<Hook>,
    pub hook_cooldown: u16,
    pub mine_cooldown: u16,
    /// The barge's: ticks before it can cut ropes again, and the play tick it last did.
    pub cut_cooldown: u16,
    pub cut_at: Option<u32>,
    /// Grazing something this tick, and in a slipstream.
    pub grazing: bool,
    pub drafting: bool,
    pub crashes: u16,
    /// Play ticks of the last crash and the last rope that caught on something.
    pub crashed_at: Option<u32>,
    pub snagged_at: Option<u32>,
    pub out_at: Option<u32>,
}

impl Hash for Ship {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.body.hash(state);
        hash_f32(self.z, state);
        hash_f32(self.speed, state);
        self.status.hash(state);
        self.barge.hash(state);
        (self.stun, self.ghost, self.throttle, self.meter).hash(state);
        self.hook.hash(state);
        (self.hook_cooldown, self.mine_cooldown, self.cut_cooldown).hash(state);
        self.cut_at.hash(state);
        (self.grazing, self.drafting, self.crashes).hash(state);
        (self.crashed_at, self.snagged_at, self.out_at).hash(state);
    }
}

impl Ship {
    pub fn flying(&self) -> bool {
        self.status == Status::Flying
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Intro {
        t: u16,
    },
    Playing,
    /// The result is in; the gliders fly on for a moment without anyone steering.
    Decided {
        t: u16,
    },
    Finished,
}

#[derive(Debug, Clone)]
pub struct State {
    pub seed: u64,
    pub players: PlayerSetup,
    pub format: Format,
    pub phase: Phase,
    /// Ticks of play so far.
    pub tick: u32,
    pub ships: [Ship; MAX_PLAYERS],
    pub course: Course,
    /// The barge's mines, in the order dropped.
    pub mines: Vec<Obstacle>,
    /// Where the camera's kill line is along the course: anyone behind it is out.
    pub camera_z: f32,
    pub outcome: Option<Outcome>,
}

impl Hash for State {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.seed.hash(state);
        self.players.hash(state);
        self.format.hash(state);
        self.phase.hash(state);
        self.tick.hash(state);
        self.ships.hash(state);
        self.course.hash(state);
        self.mines.len().hash(state);
        for mine in &self.mines {
            hash_f32(mine.z, state);
            if let Shape::Rock { center, .. } = mine.shape {
                center.hash(state);
            }
        }
        hash_f32(self.camera_z, state);
        self.outcome.hash(state);
    }
}

fn mine(center: Vec2, z: f32) -> Obstacle {
    Obstacle {
        z,
        depth: MINE_RADIUS * 2.0,
        shape: Shape::Rock {
            center,
            radius: MINE_RADIUS,
        },
    }
}

impl State {
    pub fn ship(&self, seat: Seat) -> &Ship {
        &self.ships[seat.index()]
    }

    /// Seats playing (benched duel seats aren't).
    pub fn playing(&self) -> impl Iterator<Item = Seat> + '_ {
        self.players
            .seats()
            .filter(|s| self.ships[s.index()].status != Status::Benched)
    }

    pub fn flying(&self) -> impl Iterator<Item = Seat> + '_ {
        self.players
            .seats()
            .filter(|s| self.ships[s.index()].flying())
    }

    pub fn barge(&self) -> Option<Seat> {
        match self.format {
            Format::OneVsThree { solo } => Some(solo),
            _ => None,
        }
    }

    pub fn allies(&self, a: Seat, b: Seat) -> bool {
        self.format.same_team(a, b)
    }

    /// The barge's cruising speed, from how close the nearest of the three is.
    pub fn barge_cruise(&self) -> f32 {
        let (Some(barge), Some(chase)) = (self.barge(), self.leader_z()) else {
            return CRUISE;
        };
        let near = (1.0 - (self.ship(barge).z - chase) / BARGE_EASE).clamp(0.0, 1.0);
        CRUISE * (BARGE_FAR + (BARGE_NEAR - BARGE_FAR) * near)
    }

    /// How long this match lasts at most: shorter in 1v3.
    pub fn play_ticks(&self) -> u32 {
        if self.barge().is_some() {
            BARGE_TICKS
        } else {
            PLAY_TICKS
        }
    }

    pub fn ticks_left(&self) -> u32 {
        self.play_ticks().saturating_sub(self.tick)
    }

    /// How far behind the leader the camera's kill line is now.
    pub fn window(&self) -> f32 {
        let t = (self.tick.saturating_sub(CLOSE_IN_AFTER) as f32 / CLOSE_IN_TICKS as f32).min(1.0);
        WINDOW_START + (WINDOW_END - WINDOW_START) * t
    }

    /// The furthest along of the gliders still flying (in 1v3, of the three: the camera
    /// follows the chase, with the barge somewhere ahead).
    pub fn leader_z(&self) -> Option<f32> {
        let barge = self.barge();
        self.flying()
            .filter(|s| Some(*s) != barge)
            .map(|s| self.ship(s).z)
            .reduce(f32::max)
    }

    /// Every obstacle (mines included) spanning anything within `reach` of `z`.
    pub fn obstacles_near(&self, z: f32, reach: f32) -> impl Iterator<Item = &Obstacle> + '_ {
        self.course
            .between(z - reach, z + reach)
            .iter()
            .chain(self.mines.iter())
            .filter(move |o| o.spans(z, reach))
    }

    /// The obstacles a rope from `from` to `to` would catch on: anything between them that the
    /// straight line passes through.
    pub fn rope_blocked(&self, from: (Vec2, f32), to: (Vec2, f32)) -> bool {
        self.rope_clearance(from, to) < ROPE_CATCH
    }

    /// How clear of everything between them a straight rope from `from` to `to` passes.
    pub fn rope_clearance(&self, from: (Vec2, f32), to: (Vec2, f32)) -> f32 {
        let (near, far) = if from.1 <= to.1 {
            (from, to)
        } else {
            (to, from)
        };
        let span = far.1 - near.1;
        if span <= 0.0 {
            return f32::MAX;
        }
        self.course
            .between(near.1, far.1)
            .iter()
            .chain(self.mines.iter())
            .filter(|o| o.middle() > near.1 && o.middle() < far.1)
            .map(|o| {
                let along = (o.middle() - near.1) / span;
                o.clearance(near.0.lerp(far.0, along))
            })
            .fold(f32::MAX, f32::min)
    }

    /// Who `seat` would hook if it fired now: the nearest glider ahead within reach, with a
    /// clear line to it. In 1v3 the three hook only the barge.
    pub fn hook_target(&self, seat: Seat) -> Option<Seat> {
        let me = self.ship(seat);
        if !me.flying() || me.barge || me.hook.is_some() || me.hook_cooldown > 0 || me.stun > 0 {
            return None;
        }
        let barge = self.barge();
        self.flying()
            .filter(|s| *s != seat && barge.is_none_or(|b| b == *s))
            .filter_map(|s| {
                let other = self.ship(s);
                let ahead = other.z - me.z;
                let across = other.body.pos.distance(me.body.pos);
                ((HOOK_MIN..=HOOK_RANGE).contains(&ahead)
                    && across <= HOOK_WIDTH
                    && !self.rope_blocked((me.body.pos, me.z), (other.body.pos, other.z)))
                .then_some((s, ahead * ahead + across * across))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
            .map(|(s, _)| s)
    }

    fn crash(&mut self, seat: Seat, away: Vec2) {
        let tick = self.tick;
        let ship = &mut self.ships[seat.index()];
        let (keep, stun) = if ship.barge {
            (BARGE_CRASH_SPEED, BARGE_STUN)
        } else {
            (CRASH_SPEED, CRASH_STUN)
        };
        ship.speed = (ship.speed * keep).max(MIN_SPEED);
        ship.stun = stun;
        ship.ghost = GHOST_TICKS;
        ship.throttle = false;
        ship.body.vel = away * CRASH_THROW;
        ship.crashes += 1;
        ship.crashed_at = Some(tick);
        if ship.hook.take().is_some() {
            ship.hook_cooldown = HOOK_COOLDOWN;
        }
    }

    fn steer(&mut self, inputs: &Inputs) {
        for seat in self.players.seats() {
            let ship = &mut self.ships[seat.index()];
            if !ship.flying() {
                continue;
            }
            let input = &inputs.current[seat.index()];
            // Stick up is up: the floor plane's `y` points the other way.
            let stick = Vec2::from_stick(input);
            let stick = Vec2::new(stick.x, -stick.y);
            ship.stun = ship.stun.saturating_sub(1);
            ship.ghost = ship.ghost.saturating_sub(1);
            ship.hook_cooldown = ship.hook_cooldown.saturating_sub(1);
            ship.mine_cooldown = ship.mine_cooldown.saturating_sub(1);
            ship.cut_cooldown = ship.cut_cooldown.saturating_sub(1);
            ship.throttle = !ship.barge
                && ship.stun == 0
                && input.held(Button::A)
                && ship.meter >= THROTTLE_BURN;
            if ship.throttle {
                ship.meter -= THROTTLE_BURN;
            } else if self.tick.is_multiple_of(METER_IDLE_TICKS) {
                ship.meter = (ship.meter + METER_IDLE).min(METER_MAX);
            }
            if ship.stun == 0 {
                let top = if ship.barge {
                    STEER_SPEED * 0.8
                } else {
                    STEER_SPEED
                };
                let thrust = if ship.throttle {
                    STEER_ACCEL * THROTTLE_STEER
                } else {
                    STEER_ACCEL
                };
                ship.body.steer(stick, top, thrust);
                ship.body.integrate(0.0);
            } else {
                ship.body.integrate(STUN_DRAG);
            }
            if inputs.just_pressed(seat, Button::A)
                && ship.stun == 0
                && ship.barge
                && ship.mine_cooldown == 0
            {
                ship.mine_cooldown = MINE_COOLDOWN;
                let behind = ship.z - BARGE_RADIUS - MINE_RADIUS * 3.0;
                let pos = ship.body.pos;
                self.mines.push(mine(pos, behind));
            }
            let ship = &mut self.ships[seat.index()];
            if ship.barge && ship.cut_cooldown == 0 && inputs.just_pressed(seat, Button::B) {
                ship.cut_cooldown = CUT_COOLDOWN;
                ship.cut_at = Some(self.tick);
                for other in &mut self.ships {
                    if other.hook.is_some_and(|h| h.target == seat) {
                        other.hook = None;
                        other.hook_cooldown = HOOK_COOLDOWN;
                    }
                }
            }
            let ship = &mut self.ships[seat.index()];
            if ship.hook.is_some() && !input.held(Button::B) {
                ship.hook = None;
                ship.hook_cooldown = HOOK_COOLDOWN;
            }
            if inputs.just_pressed(seat, Button::B)
                && !ship.barge
                && ship.hook.is_none()
                && ship.hook_cooldown == 0
                && ship.stun == 0
            {
                let target = self.hook_target(seat);
                let ship = &mut self.ships[seat.index()];
                match target {
                    Some(target) => ship.hook = Some(Hook { target, t: 0 }),
                    None => ship.hook_cooldown = MISS_COOLDOWN,
                }
            }
        }
    }

    /// Slipstreams, throttles and ropes change speeds along the course; then everyone moves on.
    fn fly(&mut self) {
        let seats: Vec<Seat> = self.flying().collect();
        for &seat in &seats {
            let me = *self.ship(seat);
            let drafting = !me.barge
                && seats.iter().any(|&s| {
                    let other = self.ship(s);
                    let behind = other.z - me.z;
                    s != seat
                        && behind > SHIP_LENGTH
                        && behind < DRAFT_LENGTH
                        && other.body.pos.distance(me.body.pos)
                            < DRAFT_WIDTH + other.body.radius - SHIP_RADIUS
                });
            let cruise = if me.barge {
                self.barge_cruise()
            } else if drafting {
                CRUISE * (1.0 + DRAFT_BONUS)
            } else {
                CRUISE
            };
            // The throttle adds to a slipstream rather than multiplying it.
            let (cruise, recover) = if me.throttle {
                (cruise + CRUISE * (THROTTLE - 1.0), THROTTLE_ACCEL)
            } else {
                (cruise, SPEED_RECOVER)
            };
            // While a rope joins two gliders, it sets their speeds.
            let roped = me.hook.is_some()
                || seats
                    .iter()
                    .any(|&s| self.ship(s).hook.is_some_and(|h| h.target == seat));
            let ship = &mut self.ships[seat.index()];
            ship.drafting = drafting;
            if roped {
                continue;
            }
            if ship.speed < cruise {
                ship.speed = (ship.speed + recover * DT).min(cruise);
            } else {
                ship.speed = (ship.speed - SPEED_BLEED * DT).max(cruise);
            }
        }
        for &seat in &seats {
            let Some(hook) = self.ship(seat).hook else {
                continue;
            };
            let (me, them) = (*self.ship(seat), *self.ship(hook.target));
            // A partner's tow costs them nothing.
            let (mine, theirs) = if self.allies(seat, hook.target) {
                (1.0, 0.0)
            } else {
                let total = me.body.mass + them.body.mass;
                (them.body.mass / total, me.body.mass / total)
            };
            let across = them.body.pos - me.body.pos;
            let ship = &mut self.ships[seat.index()];
            ship.speed += TOW * mine * DT;
            ship.body.pos += across * (ROPE_PULL * mine * DT);
            let target = &mut self.ships[hook.target.index()];
            target.speed -= TOW * theirs * DT;
            target.body.pos -= across * (ROPE_PULL * theirs * DT);
        }
        for &seat in &seats {
            let ship = &mut self.ships[seat.index()];
            ship.speed = ship.speed.clamp(MIN_SPEED, MAX_SPEED);
            ship.z += ship.speed * DT;
            let half = half_extents(ship.z);
            let before = ship.body.vel;
            if ship.body.bounce_in_rect(half, WALL_RESTITUTION) {
                // Only a real knock against the wall costs speed, not resting along it.
                if (ship.body.vel - before).length() > 1.0 {
                    ship.speed *= 1.0 - WALL_SCRAPE;
                }
            }
        }
    }

    /// Gliders level with each other bump apart across the course, and a flat-out throttle rams.
    fn bump(&mut self) {
        let seats: Vec<Seat> = self.flying().collect();
        for (i, &a) in seats.iter().enumerate() {
            for &b in &seats[i + 1..] {
                let (mut sa, mut sb) = (self.ships[a.index()], self.ships[b.index()]);
                if (sa.z - sb.z).abs() >= SHIP_LENGTH {
                    continue;
                }
                let Some(contact) = physics::collide(&mut sa.body, &mut sb.body, BUMP_RESTITUTION)
                else {
                    continue;
                };
                if !self.allies(a, b) {
                    let rams = |s: &Ship| s.throttle && s.speed > RAM_SPEED;
                    if rams(&sa) && !rams(&sb) && !sb.barge {
                        sb.body.vel = contact.normal * RAM_PUSH;
                        sb.stun = sb.stun.max(RAM_STUN);
                    } else if rams(&sb) && !rams(&sa) && !sa.barge {
                        sa.body.vel = -contact.normal * RAM_PUSH;
                        sa.stun = sa.stun.max(RAM_STUN);
                    }
                }
                self.ships[a.index()] = sa;
                self.ships[b.index()] = sb;
            }
        }
        // Flying alongside a rival fills the meter.
        for &a in &seats {
            let me = self.ship(a);
            let alongside = seats.iter().any(|&b| {
                let other = self.ship(b);
                b != a
                    && !self.allies(a, b)
                    && (other.z - me.z).abs() < SHIP_LENGTH * 1.5
                    && other.body.pos.distance(me.body.pos)
                        < other.body.radius + me.body.radius + RIVAL_GRAZE
            });
            if alongside {
                let ship = &mut self.ships[a.index()];
                ship.meter = (ship.meter + RIVAL_GRAZE_RATE).min(METER_MAX);
            }
        }
    }

    /// Obstacles and mines crash anyone who hits them and charge anyone who grazes them.
    fn obstacles(&mut self) {
        let seats: Vec<Seat> = self.flying().collect();
        for seat in seats {
            let ship = *self.ship(seat);
            let mut crash = None;
            let mut graze: f32 = 0.0;
            if ship.ghost == 0 {
                for obstacle in self.obstacles_near(ship.z, 0.0) {
                    let clearance = obstacle.clearance(ship.body.pos);
                    if clearance < ship.body.radius {
                        let away = obstacle.away(ship.body.pos);
                        crash = Some((away, ship.body.radius - clearance));
                        break;
                    }
                    let close = 1.0 - (clearance - ship.body.radius) / GRAZE;
                    graze = graze.max(close);
                }
            }
            let ship = &mut self.ships[seat.index()];
            ship.grazing = crash.is_none() && graze > 0.0;
            if let Some((away, depth)) = crash {
                ship.body.pos += away * depth;
                self.crash(seat, away);
            } else if graze > 0.0 {
                ship.meter = (ship.meter + (GRAZE_RATE * graze) as u16).min(METER_MAX);
            }
        }
    }

    /// Ropes let go once you're level, after their time, or when the one ahead goes out; and
    /// a rope that catches on something yanks its thrower into it.
    fn ropes(&mut self) {
        let tick = self.tick;
        let players = self.players;
        for seat in players.seats() {
            let ship = self.ships[seat.index()];
            let Some(hook) = ship.hook else {
                continue;
            };
            let target = *self.ship(hook.target);
            let done = !ship.flying()
                || !target.flying()
                || target.z - ship.z < SHIP_LENGTH * 0.5
                || hook.t >= HOOK_TICKS;
            if done {
                let ship = &mut self.ships[seat.index()];
                ship.hook = None;
                ship.hook_cooldown = HOOK_COOLDOWN;
                continue;
            }
            if self.rope_blocked((ship.body.pos, ship.z), (target.body.pos, target.z)) {
                // Yanked into whatever it caught on.
                let pull = (target.body.pos - ship.body.pos).normalize_or_zero();
                self.crash(seat, pull);
                self.ships[seat.index()].snagged_at = Some(tick);
                continue;
            }
            self.ships[seat.index()].hook = Some(Hook {
                t: hook.t + 1,
                ..hook
            });
        }
    }

    /// The camera follows the leader. In the canyon it pushes stragglers on; in the cave,
    /// anyone it leaves behind is out.
    fn follow(&mut self) {
        if let Some(leader) = self.leader_z() {
            self.camera_z = self.camera_z.max(leader - self.window());
        }
        let tick = self.tick;
        // Never as far as the leader, or the pushed would leapfrog it.
        let cushion = CANYON_CUSHION.min(self.window() - SHIP_LENGTH * 2.0);
        for seat in self.players.seats() {
            let camera_z = self.camera_z;
            let ship = &mut self.ships[seat.index()];
            match ship.status {
                Status::Flying if ship.z < camera_z + cushion && camera_z < course::CAVE_Z => {
                    ship.z = camera_z + cushion;
                }
                Status::Flying if ship.z < camera_z => {
                    ship.status = Status::Out { t: 0 };
                    ship.out_at = Some(tick);
                    ship.hook = None;
                }
                Status::Out { t } => {
                    ship.status = Status::Out {
                        t: t.saturating_add(1),
                    };
                    // Tumbling down out of the race.
                    ship.z += CRUISE * 0.5 * DT;
                    ship.body.vel.y -= 9.0 * DT;
                    ship.body.integrate(0.5);
                }
                _ => {}
            }
        }
        let camera_z = self.camera_z;
        self.mines.retain(|m| m.z + m.depth >= camera_z - 2.0);
    }

    /// Scores to rank by: when each glider went out, and beyond that how far along the
    /// survivors are.
    fn scores(&self) -> [u32; MAX_PLAYERS] {
        let mut scores = [0; MAX_PLAYERS];
        for seat in self.playing() {
            let ship = self.ship(seat);
            scores[seat.index()] = match ship.out_at {
                Some(tick) => tick,
                None => PLAY_TICKS * 2 + (ship.z.max(0.0) * 10.0) as u32,
            };
        }
        scores
    }

    fn decide_if_over(&mut self) {
        if self.outcome.is_some() {
            return;
        }
        let time_up = self.tick >= self.play_ticks();
        let scores = self.scores();
        let outcome = match self.format {
            Format::OneVsThree { solo } => {
                let barge_z = self.ship(solo).z;
                let passed = self.flying().any(|s| s != solo && self.ship(s).z > barge_z);
                let three_out = !self.flying().any(|s| s != solo);
                if passed {
                    Some([0, 1])
                } else if three_out || time_up {
                    Some([1, 0])
                } else {
                    None
                }
                .map(|[barge, three]| {
                    let mut teams = [0; MAX_PLAYERS];
                    teams[0] = barge;
                    teams[1] = three;
                    Outcome::from_team_scores(&self.players, &self.format, &teams)
                })
            }
            Format::TwoVsTwo { .. } => {
                let mut teams = [0; MAX_PLAYERS];
                for seat in self.playing() {
                    let team = self.format.team(seat).expect("playing seats have a team") as usize;
                    teams[team] = teams[team].max(scores[seat.index()]);
                }
                let sides_flying = (0..2u8)
                    .filter(|team| self.flying().any(|s| self.format.team(s) == Some(*team)))
                    .count();
                (sides_flying <= 1 || time_up)
                    .then(|| Outcome::from_team_scores(&self.players, &self.format, &teams))
            }
            _ => (self.flying().count() <= 1 || time_up)
                .then(|| Outcome::from_scores(&self.players, &scores)),
        };
        if let Some(outcome) = outcome {
            self.outcome = Some(outcome);
            self.phase = Phase::Decided { t: 0 };
        }
    }

    fn simulate(&mut self, inputs: &Inputs) {
        self.steer(inputs);
        self.fly();
        self.bump();
        self.obstacles();
        self.ropes();
        self.follow();
    }
}

/// Where everyone starts across the course: side by side, the barge in the middle.
fn start_positions(players: &PlayerSetup, format: &Format) -> [(Vec2, f32); MAX_PLAYERS] {
    let mut positions = [(Vec2::ZERO, 0.0); MAX_PLAYERS];
    let playing: Vec<Seat> = players
        .seats()
        .filter(|s| format.team(*s).is_some())
        .collect();
    let solo = match format {
        Format::OneVsThree { solo } => Some(*solo),
        _ => None,
    };
    let lined: Vec<Seat> = playing
        .iter()
        .copied()
        .filter(|s| Some(*s) != solo)
        .collect();
    let gap = 2.4;
    for (i, seat) in lined.iter().enumerate() {
        let x = (i as f32 - (lined.len() as f32 - 1.0) * 0.5) * gap;
        positions[seat.index()] = (Vec2::new(x, 0.0), 0.0);
    }
    if let Some(solo) = solo {
        positions[solo.index()] = (Vec2::new(0.0, 0.0), HEAD_START);
    }
    positions
}

impl Minigame for LanternRun {
    type State = State;

    const ID: &'static str = "lantern-run";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[
        FormatKind::FreeForAll,
        FormatKind::OneVsThree,
        FormatKind::TwoVsTwo,
        FormatKind::Duel,
    ];
    const MAX_TICKS: u32 = INTRO_TICKS as u32 + PLAY_TICKS + DECIDED_TICKS as u32 + 60;

    fn new(seed: u64, players: PlayerSetup, format: Format) -> State {
        let positions = start_positions(&players, &format);
        let mut ships = [Ship {
            body: Body::new(Vec2::ZERO, SHIP_RADIUS, 1.0),
            z: 0.0,
            speed: CRUISE,
            status: Status::Benched,
            barge: false,
            stun: 0,
            ghost: 0,
            throttle: false,
            meter: METER_START,
            hook: None,
            hook_cooldown: 0,
            mine_cooldown: 0,
            cut_cooldown: 0,
            cut_at: None,
            grazing: false,
            drafting: false,
            crashes: 0,
            crashed_at: None,
            snagged_at: None,
            out_at: None,
        }; MAX_PLAYERS];
        for seat in players.seats() {
            if format.team(seat).is_none() {
                continue;
            }
            let barge = matches!(format, Format::OneVsThree { solo } if solo == seat);
            let (pos, z) = positions[seat.index()];
            let ship = &mut ships[seat.index()];
            ship.status = Status::Flying;
            ship.barge = barge;
            ship.z = z;
            ship.body = if barge {
                Body::new(pos, BARGE_RADIUS, BARGE_MASS)
            } else {
                Body::new(pos, SHIP_RADIUS, 1.0)
            };
        }
        State {
            seed,
            players,
            format,
            phase: Phase::Intro { t: 0 },
            tick: 0,
            ships,
            course: Course::new(seed),
            mines: Vec::new(),
            camera_z: -WINDOW_START,
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
        if state.phase != Phase::Playing || !state.ship(seat).flying() {
            return Input::default();
        }
        bot::input(state, seat, difficulty)
    }
}
