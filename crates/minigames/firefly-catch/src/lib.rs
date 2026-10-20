//! Firefly Catch: fireflies drift around a meadow and flee anyone who comes near. Run with the
//! stick, swing the net with A to catch every firefly just in front of you, dash with B.
//! Most points when the timer runs out wins.
//!
//! A startled firefly sprints faster than you can run, and when you're about to net it, it hops
//! over your head and lands behind you. Both cost it stamina: chase it long enough and it tires,
//! slows to a crawl and can't hop, and that's when you catch it. Fireflies only stay a few
//! seconds before flying off, and a rare golden one is worth [`GOLDEN_POINTS`].
//!
//! Fireflies are spawned and removed inside the simulation. Each gets an id from a counter that
//! never repeats, so the renderer can match its own entities (and particles) to them, including
//! across rollbacks.

use sim_core::physics::{self, Body, Vec2};
use sim_core::{
    Button, Difficulty, Format, FormatKind, Input, Inputs, MAX_PLAYERS, Minigame, Outcome,
    PlayerSetup, Rng, Seat,
};

/// The meadow is a rectangle centred on the origin with walls on all sides.
pub const ARENA_HALF_EXTENTS: Vec2 = Vec2::new(8.0, 5.0);
/// The opening countdown, the same in every minigame.
pub const INTRO_TICKS: u16 = sim_core::COUNTDOWN_TICKS;
pub const PLAY_TICKS: u16 = 45 * 60;

pub const PLAYER_RADIUS: f32 = 0.45;
pub const PLAYER_MASS: f32 = 1.0;
pub const PLAYER_SPEED: f32 = 5.0;
pub const PLAYER_ACCEL: f32 = 30.0;
pub const PLAYER_DRAG: f32 = 3.0;
pub const BUMP_RESTITUTION: f32 = 0.6;
pub const WALL_RESTITUTION: f32 = 0.3;

/// The net sweeps a circle this far in front of the player's centre...
pub const NET_REACH: f32 = 0.7;
/// ...with this radius. A firefly whose own circle touches it is caught. The far edge stays
/// short of [`HOP_TRIGGER_DISTANCE`], even mid-dash, so only a tired firefly can be netted.
pub const NET_RADIUS: f32 = 0.4;
/// After A is pressed the net stays open this long, catching whatever it touches, so a swing
/// a moment early still lands.
pub const SWING_TICKS: u16 = 12;
pub const NET_COOLDOWN_TICKS: u16 = 45;
pub const DASH_SPEED: f32 = 11.0;
/// A dash overrides steering for this long; drag and bumps still apply.
pub const DASH_TICKS: u16 = 10;
pub const DASH_COOLDOWN_TICKS: u16 = 120;

pub const FIREFLY_RADIUS: f32 = 0.2;
pub const INITIAL_FIREFLIES: usize = 6;
/// Below this many flying fireflies a new one spawns right away; at the maximum none do.
pub const MIN_FLYING: usize = 5;
pub const MAX_FLYING: usize = 8;
pub const SPAWN_INTERVAL_TICKS: (u16, u16) = (45, 120);
/// New fireflies appear at least this far from every player when the meadow allows.
pub const SPAWN_CLEARANCE: f32 = 3.0;
pub const GOLDEN_PERCENT: u32 = 10;
pub const GOLDEN_POINTS: u32 = 3;
/// A caught firefly stays in [`State::fireflies`] this long, so the renderer can play a burst
/// and fly it into the catcher's jar.
pub const CAUGHT_LINGER_TICKS: u16 = 40;
/// Likewise for a firefly whose time ran out and is flying away.
pub const LEAVING_LINGER_TICKS: u16 = 60;
/// In 1v3 every point the solo player scores counts this many times.
pub const SOLO_MULTIPLIER: u32 = 3;

/// A firefly about to be netted (a pursuer this close) hops over them, if it has the stamina.
pub const HOP_TRIGGER_DISTANCE: f32 = 1.5;
pub const HOP_TICKS: u16 = 30;
pub const HOP_DISTANCE: f32 = 3.2;
/// Stamina a hop costs, in ticks of sprinting.
pub const HOP_COST: u16 = 40;
/// A tired firefly sprints (and hops) again once it has this many ticks of stamina back.
pub const RECOVERED_STAMINA: u16 = 45;

/// Wandering fireflies turn away from walls once this close to one.
const WALL_AVOID_MARGIN: f32 = 1.0;
/// Radians the wander heading may turn per tick.
const WANDER_TURN: f32 = 0.12;
/// How far sideways (relative to straight away) a fleeing firefly veers to its dodge side.
const DODGE_WEIGHT: f32 = 0.25;
/// Within this distance of a wall, a fleeing firefly slides along it rather than into it.
const WALL_SLIDE_MARGIN: f32 = 1.0;
/// Chance per tick that a firefly switches the side it prefers to dodge to.
const DODGE_FLIP_PERCENT: u32 = 3;

/// Where players start, by how many are playing: spread evenly so nobody begins with a bigger
/// share of the meadow to themselves. Active seats take these in seat order.
const STARTS_2: [Vec2; 2] = [Vec2::new(-4.0, 0.0), Vec2::new(4.0, 0.0)];
/// An equilateral triangle around the centre.
const STARTS_3: [Vec2; 3] = [
    Vec2::new(-3.46, -2.0),
    Vec2::new(3.46, -2.0),
    Vec2::new(0.0, 4.0),
];
const STARTS_4: [Vec2; 4] = [
    Vec2::new(-4.0, -2.5),
    Vec2::new(4.0, -2.5),
    Vec2::new(-4.0, 2.5),
    Vec2::new(4.0, 2.5),
];

fn start_positions(setup: &PlayerSetup) -> [Vec2; MAX_PLAYERS] {
    let layout: &[Vec2] = match setup.count() {
        0..=2 => &STARTS_2,
        3 => &STARTS_3,
        _ => &STARTS_4,
    };
    // Inactive seats keep a corner; they never move or count for anything.
    let mut starts = STARTS_4;
    for (seat, start) in setup.seats().zip(layout) {
        starts[seat.index()] = *start;
    }
    starts
}

pub struct FireflyCatch;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    /// Players are shown in place and can't move yet.
    Intro {
        t: u16,
    },
    /// The HUD countdown.
    Playing {
        ticks_left: u16,
    },
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FireflyKind {
    Normal,
    /// Rare, faster, jumpier, shorter-lived and worth [`GOLDEN_POINTS`].
    Golden,
}

impl FireflyKind {
    pub fn points(self) -> u32 {
        match self {
            FireflyKind::Normal => 1,
            FireflyKind::Golden => GOLDEN_POINTS,
        }
    }

    fn wander_speed(self) -> f32 {
        match self {
            FireflyKind::Normal => 1.2,
            FireflyKind::Golden => 2.0,
        }
    }

    /// Sprinting, a firefly outruns a player...
    fn flee_speed(self) -> f32 {
        match self {
            FireflyKind::Normal => 6.0,
            FireflyKind::Golden => 6.5,
        }
    }

    /// ...but only for so long. Tired, it's easy to run down.
    fn tired_speed(self) -> f32 {
        match self {
            FireflyKind::Normal => 3.2,
            FireflyKind::Golden => 3.6,
        }
    }

    /// Ticks of sprinting a rested firefly has in it.
    pub fn max_stamina(self) -> u16 {
        match self {
            FireflyKind::Normal => 120,
            FireflyKind::Golden => 150,
        }
    }

    fn fear_radius(self) -> f32 {
        match self {
            FireflyKind::Normal => 3.0,
            FireflyKind::Golden => 3.5,
        }
    }

    fn accel(self) -> f32 {
        match self {
            FireflyKind::Normal => 30.0,
            FireflyKind::Golden => 36.0,
        }
    }

    fn lifetime_ticks(self) -> (u32, u32) {
        match self {
            FireflyKind::Normal => (480, 840),
            FireflyKind::Golden => (300, 420),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FireflyStatus {
    /// Catchable until [`State::tick`] reaches `leaves_at`.
    Flying { leaves_at: u32 },
    /// Netted on tick `at`; removed [`CAUGHT_LINGER_TICKS`] later. The body stops where it was caught.
    Caught { by: Seat, at: u32 },
    /// Time ran out on tick `at`; it drifts off and is removed [`LEAVING_LINGER_TICKS`] later.
    Leaving { at: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct Firefly {
    /// Unique for the whole match: never reused after removal.
    pub id: u32,
    pub kind: FireflyKind,
    pub body: Body,
    /// Unit vector the firefly drifts along when nobody is near.
    pub wander: Vec2,
    /// Tick it appeared, for a fade-in.
    pub born: u32,
    pub status: FireflyStatus,
    /// Ticks of sprinting left. Fleeing drains it; it refills while the firefly drifts.
    pub stamina: u16,
    /// Out of breath: flees at a crawl until stamina is back to [`RECOVERED_STAMINA`].
    /// The renderer can dim or flicker a tired firefly to show it's catchable.
    pub tired: bool,
    /// Ticks left of a hop over a pursuer's head (0 = not hopping). A hopping firefly is out
    /// of reach of every net; the renderer can lift it along an arc of [`HOP_TICKS`].
    pub hop: u16,
    /// Which way it currently jinks when someone gets close; flips at random.
    dodge_left: bool,
}

impl Firefly {
    pub fn is_flying(&self) -> bool {
        matches!(self.status, FireflyStatus::Flying { .. })
    }

    /// Whether a net could catch it right now.
    pub fn catchable(&self) -> bool {
        self.is_flying() && self.hop == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct Player {
    pub body: Body,
    /// Unit vector of the last non-neutral stick direction; the net swings this way.
    pub facing: Vec2,
    pub net_cooldown: u16,
    /// Tick of the most recent swing, for the swing animation.
    pub last_swing: Option<u32>,
    /// Ticks the net stays open (0 = not swinging); see [`SWING_TICKS`].
    pub swing_ticks: u16,
    pub dash_cooldown: u16,
    /// Ticks of dash left; steering is ignored while this is non-zero.
    pub dash_ticks: u16,
    /// Points scored, with the 1v3 solo multiplier already applied.
    pub score: u32,
    /// Fireflies caught, golden or not.
    pub catches: u32,
}

impl Player {
    fn new(start: Vec2) -> Self {
        Self {
            body: Body::new(start, PLAYER_RADIUS, PLAYER_MASS),
            facing: (-start).normalize_or_zero(),
            net_cooldown: 0,
            last_swing: None,
            swing_ticks: 0,
            dash_cooldown: 0,
            dash_ticks: 0,
            score: 0,
            catches: 0,
        }
    }

    /// Centre of the circle the net sweeps when swung facing `facing`.
    pub fn net_centre(&self, facing: Vec2) -> Vec2 {
        self.body.pos + facing * NET_REACH
    }
}

/// Whether a net centred at `net` reaches a firefly at `pos`.
pub fn net_reaches(net: Vec2, pos: Vec2) -> bool {
    let reach = NET_RADIUS + FIREFLY_RADIUS;
    (pos - net).length_squared() <= reach * reach
}

#[derive(Debug, Clone, Hash)]
pub struct State {
    pub seed: u64,
    pub setup: PlayerSetup,
    pub format: Format,
    rng: Rng,
    pub phase: Phase,
    /// Ticks simulated since the start, intro included.
    pub tick: u32,
    /// Indexed by seat; inactive seats stay at their start and never act.
    pub players: [Player; MAX_PLAYERS],
    /// Ordered by id, oldest first.
    pub fireflies: Vec<Firefly>,
    /// The id the next spawned firefly gets.
    pub next_id: u32,
    /// Ticks until the next scheduled spawn.
    spawn_in: u16,
}

impl State {
    /// Fireflies still in play (not caught or leaving), hopping ones included.
    pub fn flying(&self) -> impl Iterator<Item = &Firefly> + '_ {
        self.fireflies.iter().filter(|f| f.is_flying())
    }

    /// Score per team ([`Format::team`]), the number placement is decided by.
    pub fn team_scores(&self) -> [u32; MAX_PLAYERS] {
        let mut scores = [0; MAX_PLAYERS];
        for seat in self.setup.seats() {
            if let Some(team) = self.format.team(seat) {
                scores[team as usize] += self.players[seat.index()].score;
            }
        }
        scores
    }

    /// How much each catch by `seat` is worth relative to the firefly's base points.
    pub fn multiplier(&self, seat: Seat) -> u32 {
        match self.format {
            Format::OneVsThree { solo } if solo == seat => SOLO_MULTIPLIER,
            _ => 1,
        }
    }

    fn rng_unit(&mut self) -> f32 {
        self.rng.range_u32(0, 10_000) as f32 / 10_000.0
    }

    /// A spawn point that's as far from the players as a few random tries can find.
    fn spawn_point(&mut self) -> Vec2 {
        let margin = WALL_AVOID_MARGIN;
        let mut best = Vec2::ZERO;
        let mut best_clearance = -1.0;
        for _ in 0..8 {
            let x = (self.rng_unit() * 2.0 - 1.0) * (ARENA_HALF_EXTENTS.x - margin);
            let y = (self.rng_unit() * 2.0 - 1.0) * (ARENA_HALF_EXTENTS.y - margin);
            let point = Vec2::new(x, y);
            let clearance = self
                .setup
                .seats()
                .map(|s| self.players[s.index()].body.pos.distance(point))
                .fold(f32::MAX, f32::min);
            if clearance > best_clearance {
                best = point;
                best_clearance = clearance;
            }
            if clearance >= SPAWN_CLEARANCE {
                break;
            }
        }
        best
    }

    fn spawn(&mut self, kind: FireflyKind) {
        let pos = self.spawn_point();
        let wander = Vec2::from_angle(self.rng_unit() * std::f32::consts::TAU);
        let (lo, hi) = kind.lifetime_ticks();
        let leaves_at = self.tick + self.rng.range_u32(lo, hi);
        let id = self.next_id;
        self.next_id += 1;
        self.fireflies.push(Firefly {
            id,
            kind,
            body: Body::new(pos, FIREFLY_RADIUS, 1.0),
            wander,
            born: self.tick,
            status: FireflyStatus::Flying { leaves_at },
            stamina: kind.max_stamina(),
            tired: false,
            hop: 0,
            dodge_left: self.rng.chance(50),
        });
    }

    fn update_spawning(&mut self) {
        self.spawn_in = self.spawn_in.saturating_sub(1);
        let flying = self.flying().count();
        if flying >= MAX_FLYING || (flying >= MIN_FLYING && self.spawn_in > 0) {
            return;
        }
        let golden_alive = self.flying().any(|f| f.kind == FireflyKind::Golden);
        let kind = if !golden_alive && self.rng.chance(GOLDEN_PERCENT) {
            FireflyKind::Golden
        } else {
            FireflyKind::Normal
        };
        self.spawn(kind);
        self.spawn_in = self
            .rng
            .range_u32(SPAWN_INTERVAL_TICKS.0 as u32, SPAWN_INTERVAL_TICKS.1 as u32)
            as u16;
    }

    fn update_players(&mut self, inputs: &Inputs) {
        for seat in self.setup.seats() {
            let player = &mut self.players[seat.index()];
            let stick = Vec2::from_stick(&inputs.current[seat.index()]);
            if stick != Vec2::ZERO {
                player.facing = stick.normalize_or_zero();
            }
            player.net_cooldown = player.net_cooldown.saturating_sub(1);
            player.swing_ticks = player.swing_ticks.saturating_sub(1);
            player.dash_cooldown = player.dash_cooldown.saturating_sub(1);

            if inputs.just_pressed(seat, Button::A) && player.net_cooldown == 0 {
                player.net_cooldown = NET_COOLDOWN_TICKS;
                player.last_swing = Some(self.tick);
                player.swing_ticks = SWING_TICKS;
            }
            if inputs.just_pressed(seat, Button::B) && player.dash_cooldown == 0 {
                player.dash_cooldown = DASH_COOLDOWN_TICKS;
                player.dash_ticks = DASH_TICKS;
                player.body.vel = player.facing * DASH_SPEED;
            }
            if player.dash_ticks > 0 {
                player.dash_ticks -= 1;
            } else {
                player.body.steer(stick, PLAYER_SPEED, PLAYER_ACCEL);
            }
            player.body.integrate(PLAYER_DRAG);
            player
                .body
                .bounce_in_rect(ARENA_HALF_EXTENTS, WALL_RESTITUTION);
        }

        let seats: Vec<Seat> = self.setup.seats().collect();
        let mut bodies: Vec<Body> = seats.iter().map(|s| self.players[s.index()].body).collect();
        physics::collide_all(&mut bodies, BUMP_RESTITUTION, |_, _, _| {});
        for (seat, body) in seats.iter().zip(bodies) {
            let player = &mut self.players[seat.index()];
            player.body = body;
            // A bump can shove someone past a wall; put them back.
            player
                .body
                .bounce_in_rect(ARENA_HALF_EXTENTS, WALL_RESTITUTION);
        }
    }

    /// Every flying firefly touched by an open net this tick is caught. If two nets reach the
    /// same firefly, the closer net wins (lower seat on an exact tie).
    fn resolve_swings(&mut self) {
        let tick = self.tick;
        for i in 0..self.fireflies.len() {
            let firefly = self.fireflies[i];
            if !firefly.catchable() {
                continue;
            }
            let mut catcher: Option<(Seat, f32)> = None;
            for seat in self
                .setup
                .seats()
                .filter(|s| self.players[s.index()].swing_ticks > 0)
            {
                let player = &self.players[seat.index()];
                let net = player.net_centre(player.facing);
                if !net_reaches(net, firefly.body.pos) {
                    continue;
                }
                let dist = net.distance(firefly.body.pos);
                if catcher.is_none_or(|(_, best)| dist < best) {
                    catcher = Some((seat, dist));
                }
            }
            if let Some((seat, _)) = catcher {
                let points = firefly.kind.points() * self.multiplier(seat);
                let player = &mut self.players[seat.index()];
                player.score += points;
                player.catches += 1;
                let firefly = &mut self.fireflies[i];
                firefly.status = FireflyStatus::Caught { by: seat, at: tick };
                firefly.body.vel = Vec2::ZERO;
            }
        }
    }

    fn update_fireflies(&mut self) {
        let tick = self.tick;
        let threats: Vec<Vec2> = self
            .setup
            .seats()
            .map(|s| self.players[s.index()].body.pos)
            .collect();
        for i in 0..self.fireflies.len() {
            let turn = (self.rng_unit() * 2.0 - 1.0) * WANDER_TURN;
            let flip = self.rng.chance(DODGE_FLIP_PERCENT);
            let firefly = &mut self.fireflies[i];
            match firefly.status {
                FireflyStatus::Flying { leaves_at } if tick >= leaves_at => {
                    firefly.status = FireflyStatus::Leaving { at: tick };
                }
                FireflyStatus::Flying { .. } => fly(firefly, &threats, tick, turn, flip),
                FireflyStatus::Leaving { .. } => {
                    // Keeps drifting while the renderer lifts it out of the meadow.
                    firefly.body.integrate(1.0);
                }
                FireflyStatus::Caught { .. } => {}
            }
        }
        self.fireflies.retain(|f| match f.status {
            FireflyStatus::Flying { .. } => true,
            FireflyStatus::Caught { at, .. } => tick - at < CAUGHT_LINGER_TICKS as u32,
            FireflyStatus::Leaving { at } => tick - at < LEAVING_LINGER_TICKS as u32,
        });
    }
}

/// Rotate a vector counter-clockwise by `angle` radians.
fn rotate(v: Vec2, angle: f32) -> Vec2 {
    let r = Vec2::from_angle(angle);
    Vec2::new(v.x * r.x - v.y * r.y, v.x * r.y + v.y * r.x)
}

/// How hard the walls push a point at `pos` back towards the middle (zero away from walls).
fn wall_push(pos: Vec2) -> Vec2 {
    let axis = |p: f32, half: f32| {
        let inner = half - WALL_AVOID_MARGIN;
        if p > inner {
            -(p - inner) / WALL_AVOID_MARGIN
        } else if p < -inner {
            (-inner - p) / WALL_AVOID_MARGIN
        } else {
            0.0
        }
    };
    Vec2::new(
        axis(pos.x, ARENA_HALF_EXTENTS.x),
        axis(pos.y, ARENA_HALF_EXTENTS.y),
    )
}

/// Straight away from the players (closer ones count more), veering a little to the dodge
/// side. Against a wall it runs along it instead, away from its pursuers; cornered, it breaks
/// out along whichever wall leads away from them more, so trapping one takes a good angle or a
/// second player.
fn escape_direction(firefly: &Firefly, threats: &[Vec2]) -> Vec2 {
    let pos = firefly.body.pos;
    let away = threats
        .iter()
        .fold(Vec2::ZERO, |sum, t| {
            let offset = pos - *t;
            sum + offset.normalize_or_zero() * (1.0 / offset.length().max(0.1))
        })
        .normalize_or_zero();
    let side = if firefly.dodge_left { 1.0 } else { -1.0 };
    let direction = (away + away.perp() * (side * DODGE_WEIGHT)).normalize_or_zero();

    let at_wall = |p: f32, d: f32, half: f32| {
        let inner = half - FIREFLY_RADIUS - WALL_SLIDE_MARGIN;
        (p >= inner && d > 0.0) || (p <= -inner && d < 0.0)
    };
    // Along a wall, head whichever way leads away from the pursuers (the dodge side if neither).
    let along = |a: f32| if a.abs() > 0.05 { a.signum() } else { side };
    match (
        at_wall(pos.x, direction.x, ARENA_HALF_EXTENTS.x),
        at_wall(pos.y, direction.y, ARENA_HALF_EXTENTS.y),
    ) {
        (false, false) => direction,
        (true, true) if away.x.abs() < away.y.abs() => Vec2::new(-pos.x.signum(), 0.0),
        (true, true) => Vec2::new(0.0, -pos.y.signum()),
        (true, false) => Vec2::new(0.0, along(away.y)),
        (false, true) => Vec2::new(along(away.x), 0.0),
    }
}

/// One tick of a flying firefly: drift along a slowly turning heading, or flee if anyone is near.
fn fly(firefly: &mut Firefly, threats: &[Vec2], tick: u32, turn: f32, flip_dodge: bool) {
    let kind = firefly.kind;
    let pos = firefly.body.pos;
    if flip_dodge {
        firefly.dodge_left = !firefly.dodge_left;
    }
    if firefly.hop > 0 {
        // Mid-hop it just carries on along its arc.
        firefly.hop -= 1;
        firefly.body.integrate(0.0);
        firefly.body.bounce_in_rect(ARENA_HALF_EXTENTS, 0.0);
        return;
    }
    firefly.wander = rotate(firefly.wander, turn).normalize_or_zero();
    let Some(pursuer) = threats
        .iter()
        .copied()
        .min_by(|a, b| a.distance(pos).total_cmp(&b.distance(pos)))
    else {
        return;
    };
    let nearest = pursuer.distance(pos);

    // About to be netted: flit up over the closest pursuer and land behind them, a little to
    // its dodge side.
    if nearest < HOP_TRIGGER_DISTANCE && !firefly.tired && firefly.stamina >= HOP_COST {
        let side = if firefly.dodge_left { 1.0 } else { -1.0 };
        let over = (pursuer - pos).normalize_or_zero();
        let direction = (over + over.perp() * (side * 0.4)).normalize_or_zero();
        firefly.body.vel = direction * (HOP_DISTANCE / (HOP_TICKS as f32 * physics::DT));
        firefly.hop = HOP_TICKS;
        firefly.stamina -= HOP_COST;
        return;
    }

    let fear = kind.fear_radius();
    let fleeing = nearest < fear;
    // Full speed (and stamina drain) once anyone is within three quarters of the fear radius.
    let urgency = ((fear - nearest) / (fear * 0.25)).clamp(0.0, 1.0);
    let sprinting = urgency >= 1.0 && !firefly.tired;
    if sprinting {
        firefly.stamina = firefly.stamina.saturating_sub(1);
        firefly.tired = firefly.stamina == 0;
    } else if !firefly.tired || urgency < 1.0 || tick.is_multiple_of(2) {
        // Breath comes back at half speed while still being chased.
        firefly.stamina = (firefly.stamina + 1).min(kind.max_stamina());
        if firefly.stamina >= RECOVERED_STAMINA {
            firefly.tired = false;
        }
    }
    let (direction, speed) = if fleeing {
        let top = if firefly.tired {
            kind.tired_speed()
        } else {
            kind.flee_speed()
        };
        let speed = kind.wander_speed() + (top - kind.wander_speed()) * urgency;
        let direction = escape_direction(firefly, threats);
        firefly.wander = direction;
        (direction, speed)
    } else {
        let wall = wall_push(firefly.body.pos);
        let direction = (firefly.wander + wall * 2.0).normalize_or_zero();
        if wall != Vec2::ZERO {
            firefly.wander = direction;
        }
        (direction, kind.wander_speed())
    };
    firefly.body.steer(direction, speed, kind.accel());
    firefly.body.integrate(0.0);
    // No bounce: rebounding off a wall would throw it straight back at its pursuer.
    firefly.body.bounce_in_rect(ARENA_HALF_EXTENTS, 0.0);
}

impl Minigame for FireflyCatch {
    type State = State;

    const ID: &'static str = "firefly-catch";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[
        FormatKind::FreeForAll,
        FormatKind::OneVsThree,
        FormatKind::TwoVsTwo,
    ];
    const MAX_TICKS: u32 = 3_600;

    fn new(seed: u64, setup: PlayerSetup, format: Format) -> State {
        let mut state = State {
            seed,
            setup,
            format,
            rng: Rng::new(seed),
            phase: Phase::Intro { t: 0 },
            tick: 0,
            players: start_positions(&setup).map(Player::new),
            fireflies: Vec::new(),
            next_id: 0,
            spawn_in: 0,
        };
        for _ in 0..INITIAL_FIREFLIES {
            state.spawn(FireflyKind::Normal);
        }
        // The intro shouldn't eat into the first fireflies' time.
        for firefly in &mut state.fireflies {
            if let FireflyStatus::Flying { leaves_at } = &mut firefly.status {
                *leaves_at += INTRO_TICKS as u32;
            }
        }
        state.spawn_in = SPAWN_INTERVAL_TICKS.1;
        state
    }

    fn step(state: &mut State, inputs: &Inputs) {
        match state.phase {
            Phase::Intro { t } => {
                state.update_fireflies();
                state.phase = if t + 1 >= INTRO_TICKS {
                    Phase::Playing {
                        ticks_left: PLAY_TICKS,
                    }
                } else {
                    Phase::Intro { t: t + 1 }
                };
            }
            Phase::Playing { ticks_left } => {
                state.update_players(inputs);
                state.resolve_swings();
                state.update_fireflies();
                state.update_spawning();
                state.phase = if ticks_left <= 1 {
                    Phase::Finished
                } else {
                    Phase::Playing {
                        ticks_left: ticks_left - 1,
                    }
                };
            }
            Phase::Finished => return,
        }
        state.tick += 1;
    }

    fn outcome(state: &State) -> Option<Outcome> {
        if state.phase != Phase::Finished {
            return None;
        }
        Some(match state.format {
            Format::FreeForAll => {
                Outcome::from_scores(&state.setup, &state.players.map(|p| p.score))
            }
            format => Outcome::from_team_scores(&state.setup, &format, &state.team_scores()),
        })
    }

    fn bot_input(state: &State, seat: Seat, difficulty: Difficulty) -> Input {
        if !state.setup.is_active(seat) || !matches!(state.phase, Phase::Playing { .. }) {
            return Input::default();
        }
        bot::input(state, seat, difficulty)
    }
}

mod bot {
    //! A bot picks the firefly most worth chasing, gets between it and the middle of the meadow
    //! so it flees towards a wall, then keeps after it until it tires and swings once it's in
    //! the net. Difficulty only changes how well it judges, aims and reacts, never how fast it
    //! moves.

    use super::*;

    struct Skill {
        /// Ticks between rethinking target choice and aim noise.
        think_ticks: u32,
        /// Max aim error in radians.
        aim_noise: f32,
        /// How much each firefly's apparent worth is randomly scaled (0 = perfect judgement).
        judgement_noise: f32,
        /// Chance per tick of swinging once a firefly is in reach (a reaction delay).
        swing_percent: u32,
        /// Swings when a firefly looks this many times further than the net really reaches.
        reach_misjudge: f32,
        /// Chance per tick of dashing when a dash would close the gap.
        dash_percent: u32,
        /// Leads moving fireflies and circles round them to pin them against walls.
        herds: bool,
        /// Leaves fireflies to closer teammates (and, in free-for-all, much closer rivals).
        team_aware: bool,
        /// Doesn't waste swings on a firefly that's hopping out of reach.
        waits_out_hops: bool,
    }

    impl Skill {
        fn of(difficulty: Difficulty) -> Self {
            match difficulty {
                Difficulty::Easy => Self {
                    think_ticks: 40,
                    aim_noise: 0.3,
                    judgement_noise: 0.5,
                    swing_percent: 35,
                    reach_misjudge: 1.15,
                    dash_percent: 0,
                    herds: false,
                    team_aware: false,
                    waits_out_hops: false,
                },
                Difficulty::Normal => Self {
                    think_ticks: 20,
                    aim_noise: 0.25,
                    judgement_noise: 0.4,
                    swing_percent: 30,
                    reach_misjudge: 1.15,
                    dash_percent: 3,
                    herds: true,
                    team_aware: true,
                    waits_out_hops: true,
                },
                Difficulty::Hard => Self {
                    think_ticks: 10,
                    aim_noise: 0.08,
                    judgement_noise: 0.0,
                    swing_percent: 100,
                    reach_misjudge: 1.0,
                    dash_percent: 25,
                    herds: true,
                    team_aware: true,
                    waits_out_hops: true,
                },
            }
        }
    }

    fn mix(state: &State, seat: Seat, a: u64, b: u64) -> Rng {
        Rng::new(
            state.seed
                ^ (seat.0 as u64 + 1).wrapping_mul(0xbf58_476d_1ce4_e5b9)
                ^ a.wrapping_mul(0x9e37_79b9_7f4a_7c15)
                ^ b.wrapping_mul(0x94d0_49bb_1331_11eb),
        )
    }

    fn unit(rng: &mut Rng) -> f32 {
        rng.range_u32(0, 10_000) as f32 / 10_000.0
    }

    fn choose_target<'a>(state: &'a State, seat: Seat, skill: &Skill) -> Option<&'a Firefly> {
        let me = &state.players[seat.index()];
        let epoch = (state.tick / skill.think_ticks) as u64;
        let mut best: Option<(&Firefly, f32)> = None;
        for firefly in state.flying() {
            let to = firefly.body.pos - me.body.pos;
            let dist = to.length();
            let mut worth = firefly.kind.points() as f32 / (dist + 2.0);
            // Prefer what's already ahead: it keeps the bot from dithering between targets.
            worth *= 1.25 + 0.25 * me.facing.dot(to.normalize_or_zero());
            if let FireflyStatus::Flying { leaves_at } = firefly.status
                && skill.herds
                && leaves_at.saturating_sub(state.tick) < (dist / PLAYER_SPEED * 60.0) as u32 + 20
            {
                worth *= 0.2;
            }
            if skill.team_aware {
                for other in state.setup.seats().filter(|s| *s != seat) {
                    let their = state.players[other.index()]
                        .body
                        .pos
                        .distance(firefly.body.pos);
                    let ally = !matches!(state.format, Format::FreeForAll)
                        && state.format.same_team(seat, other);
                    if (ally && their < dist) || (!ally && their < dist * 0.5) {
                        worth *= 0.3;
                    }
                }
            }
            if skill.judgement_noise > 0.0 {
                let mut rng = mix(state, seat, epoch, firefly.id as u64 + 1);
                worth *= 1.0 + (unit(&mut rng) * 2.0 - 1.0) * skill.judgement_noise;
            }
            if best.is_none_or(|(_, b)| worth > b) {
                best = Some((firefly, worth));
            }
        }
        best.map(|(f, _)| f)
    }

    pub(super) fn input(state: &State, seat: Seat, difficulty: Difficulty) -> Input {
        let skill = Skill::of(difficulty);
        let me = &state.players[seat.index()];
        let epoch = (state.tick / skill.think_ticks) as u64;
        let mut per_epoch = mix(state, seat, epoch, 0);
        let mut per_tick = mix(state, seat, state.tick as u64, u64::MAX);

        let Some(target) = choose_target(state, seat, &skill) else {
            return steer_input(Vec2::ZERO - me.body.pos, false, false);
        };
        let to = target.body.pos - me.body.pos;
        let dist = to.length();
        let aim = if skill.herds && dist > 3.0 {
            // Get between the firefly and the middle of the meadow, so it flees outwards.
            target.body.pos + (-target.body.pos).normalize_or_zero() * 1.5
        } else if skill.herds {
            target.body.pos + target.body.vel * (dist / PLAYER_SPEED * 0.6)
        } else {
            target.body.pos
        };
        let noise = (unit(&mut per_epoch) * 2.0 - 1.0) * skill.aim_noise;
        let mut direction = rotate((aim - me.body.pos).normalize_or_zero(), noise);
        if direction == Vec2::ZERO {
            direction = me.facing;
        }

        // The net swings along this tick's stick direction, so judge reach with that.
        let net = me.net_centre(direction);
        let reach = (NET_RADIUS + FIREFLY_RADIUS) * skill.reach_misjudge;
        let in_reach = state
            .flying()
            .filter(|f| f.hop == 0 || !skill.waits_out_hops)
            .any(|f| net.distance(f.body.pos) <= reach);
        let swing = in_reach && me.net_cooldown == 0 && per_tick.chance(skill.swing_percent);

        let dash = me.dash_cooldown == 0
            && me.dash_ticks == 0
            && (2.0..3.5).contains(&dist)
            && me.facing.dot(direction) > 0.9
            && per_tick.chance(skill.dash_percent);

        steer_input(direction, swing, dash)
    }

    fn steer_input(direction: Vec2, swing: bool, dash: bool) -> Input {
        let direction = direction.normalize_or_zero();
        let mut input = Input {
            buttons: 0,
            stick_x: Input::quantize_axis(direction.x),
            // The stick's up is the floor's −y.
            stick_y: Input::quantize_axis(-direction.y),
        };
        input.set(Button::A, swing);
        input.set(Button::B, dash);
        input
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

    fn new_run(players: usize, format: Format) -> Run<FireflyCatch> {
        Run::new(1, PlayerSetup::first(players), format, RunConfig::default())
    }

    fn to_playing(run: &mut Run<FireflyCatch>) {
        while matches!(run.game.phase, Phase::Intro { .. }) {
            run.step(NONE);
        }
    }

    fn with(seat: usize, input: Input) -> [Input; MAX_PLAYERS] {
        let mut inputs = NONE;
        inputs[seat] = input;
        inputs
    }

    /// Put a firefly of `kind` right where `seat`'s net lands and return its id.
    fn plant_in_net(run: &mut Run<FireflyCatch>, seat: usize, kind: FireflyKind) -> u32 {
        let player = run.game.players[seat];
        let pos = player.net_centre(player.facing);
        plant(run, pos, kind)
    }

    fn plant(run: &mut Run<FireflyCatch>, pos: Vec2, kind: FireflyKind) -> u32 {
        let state = &mut run.game;
        let id = state.next_id;
        state.next_id += 1;
        state.fireflies.push(Firefly {
            id,
            kind,
            body: Body::new(pos, FIREFLY_RADIUS, 1.0),
            wander: Vec2::new(1.0, 0.0),
            born: state.tick,
            status: FireflyStatus::Flying {
                leaves_at: state.tick + 1_000,
            },
            stamina: kind.max_stamina(),
            tired: false,
            hop: 0,
            dodge_left: false,
        });
        id
    }

    fn find(run: &Run<FireflyCatch>, id: u32) -> Option<Firefly> {
        run.game.fireflies.iter().find(|f| f.id == id).copied()
    }

    fn swing() -> Input {
        Input::default().with(Button::A)
    }

    #[test]
    fn nobody_moves_during_the_intro() {
        let mut run = new_run(2, Format::FreeForAll);
        let right = Input {
            stick_x: 127,
            ..Input::default()
        };
        run.step(with(0, right.with(Button::B)));
        assert_eq!(run.game.players[0].body.pos, STARTS_2[0]);
        assert_eq!(run.game.players[0].dash_cooldown, 0);
    }

    #[test]
    fn swinging_catches_the_firefly_in_front() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        let id = plant_in_net(&mut run, 0, FireflyKind::Normal);
        run.step(with(0, swing()));
        let caught = find(&run, id).unwrap();
        assert_eq!(
            caught.status,
            FireflyStatus::Caught {
                by: Seat(0),
                at: run.game.tick - 1
            }
        );
        assert_eq!(run.game.players[0].score, 1);
        assert_eq!(run.game.players[0].catches, 1);
    }

    #[test]
    fn the_net_misses_fireflies_behind_you() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        let player = run.game.players[0];
        let id = plant(
            &mut run,
            player.body.pos - player.facing * 1.0,
            FireflyKind::Normal,
        );
        run.step(with(0, swing()));
        assert!(find(&run, id).unwrap().is_flying());
        assert_eq!(run.game.players[0].score, 0);
    }

    #[test]
    fn one_swing_catches_every_firefly_in_reach() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        let a = plant_in_net(&mut run, 0, FireflyKind::Normal);
        let b = plant_in_net(&mut run, 0, FireflyKind::Golden);
        run.step(with(0, swing()));
        assert!(!find(&run, a).unwrap().is_flying());
        assert!(!find(&run, b).unwrap().is_flying());
        assert_eq!(run.game.players[0].score, 1 + GOLDEN_POINTS);
        assert_eq!(run.game.players[0].catches, 2);
    }

    #[test]
    fn the_net_has_a_cooldown() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        run.step(with(0, swing()));
        for _ in 0..SWING_TICKS {
            run.step(NONE);
        }
        let id = plant_in_net(&mut run, 0, FireflyKind::Normal);
        run.step(with(0, swing()));
        assert!(find(&run, id).unwrap().is_flying());
        for _ in 0..NET_COOLDOWN_TICKS {
            run.step(NONE);
        }
        let id = plant_in_net(&mut run, 0, FireflyKind::Normal);
        run.step(with(0, swing()));
        assert!(!find(&run, id).unwrap().is_flying());
    }

    #[test]
    fn the_net_stays_open_for_a_moment() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        run.step(with(0, swing()));
        for _ in 0..SWING_TICKS / 2 {
            run.step(NONE);
        }
        let late = plant_in_net(&mut run, 0, FireflyKind::Normal);
        run.step(NONE);
        assert!(!find(&run, late).unwrap().is_flying());
        for _ in 0..SWING_TICKS {
            run.step(NONE);
        }
        let too_late = plant_in_net(&mut run, 0, FireflyKind::Normal);
        run.step(NONE);
        assert!(find(&run, too_late).unwrap().is_flying());
    }

    /// The chase is the game: a rested firefly hops away before any net, even a dashing
    /// player's, can touch it.
    #[test]
    fn nets_fall_short_of_the_hop_distance() {
        let far_edge = NET_REACH + NET_RADIUS + FIREFLY_RADIUS;
        assert!(far_edge + DASH_SPEED * physics::DT < HOP_TRIGGER_DISTANCE);
    }

    #[test]
    fn holding_a_swings_only_once() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        for _ in 0..NET_COOLDOWN_TICKS + 5 {
            run.step(with(0, swing()));
        }
        let id = plant_in_net(&mut run, 0, FireflyKind::Normal);
        run.step(with(0, swing()));
        assert!(find(&run, id).unwrap().is_flying());
    }

    #[test]
    fn dashing_is_fast_and_has_a_cooldown() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        run.step(with(0, Input::default().with(Button::B)));
        let player = run.game.players[0];
        assert!(player.body.vel.length() > PLAYER_SPEED);
        assert_eq!(player.dash_cooldown, DASH_COOLDOWN_TICKS);
        run.step(NONE);
        run.step(with(0, Input::default().with(Button::B)));
        assert_eq!(run.game.players[0].dash_cooldown, DASH_COOLDOWN_TICKS - 2);
    }

    #[test]
    fn players_bump_each_other() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        run.game.players[1].body.pos = run.game.players[0].body.pos + Vec2::new(0.5, 0.0);
        run.step(NONE);
        let gap = run.game.players[0]
            .body
            .pos
            .distance(run.game.players[1].body.pos);
        assert!(gap >= 2.0 * PLAYER_RADIUS - 1e-4);
    }

    #[test]
    fn caught_fireflies_linger_then_disappear() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        let id = plant_in_net(&mut run, 0, FireflyKind::Normal);
        run.step(with(0, swing()));
        for _ in 1..CAUGHT_LINGER_TICKS {
            run.step(NONE);
        }
        assert!(find(&run, id).is_some());
        run.step(NONE);
        assert!(find(&run, id).is_none());
    }

    #[test]
    fn fireflies_leave_when_their_time_is_up() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        let id = plant(&mut run, Vec2::ZERO, FireflyKind::Normal);
        let leaves_at = run.game.tick + 5;
        run.game.fireflies.last_mut().unwrap().status = FireflyStatus::Flying { leaves_at };
        for _ in 0..6 {
            run.step(NONE);
        }
        assert_eq!(
            find(&run, id).unwrap().status,
            FireflyStatus::Leaving { at: leaves_at }
        );
        // A leaving firefly can't be caught any more.
        let pos = find(&run, id).unwrap().body.pos;
        let facing = run.game.players[0].facing;
        run.game.players[0].body.pos = pos - facing * NET_REACH;
        run.step(with(0, swing()));
        assert_eq!(run.game.players[0].score, 0);
        for _ in 0..LEAVING_LINGER_TICKS {
            run.step(NONE);
        }
        assert!(find(&run, id).is_none());
    }

    #[test]
    fn fireflies_flee_players() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        let player = run.game.players[0].body.pos;
        let id = plant(&mut run, player + Vec2::new(2.0, 0.0), FireflyKind::Normal);
        for _ in 0..20 {
            run.step(NONE);
        }
        assert!(find(&run, id).unwrap().body.pos.distance(player) > 2.0);
    }

    #[test]
    fn a_cornered_firefly_hops_out_of_reach() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        let player = run.game.players[0];
        let id = plant(
            &mut run,
            player.body.pos + player.facing * 1.3,
            FireflyKind::Normal,
        );
        run.step(NONE);
        let hopping = find(&run, id).unwrap();
        assert_eq!(hopping.hop, HOP_TICKS);
        assert_eq!(
            hopping.stamina,
            FireflyKind::Normal.max_stamina() - HOP_COST
        );
        assert!(!hopping.catchable());
        // Even a net right on top of it misses while it's in the air.
        run.game.players[0].body.pos = hopping.body.pos - player.facing * NET_REACH;
        run.game.players[0].body.vel = Vec2::ZERO;
        run.step(with(0, swing()));
        assert!(find(&run, id).unwrap().is_flying());
        assert_eq!(run.game.players[0].score, 0);
    }

    #[test]
    fn tired_fireflies_cannot_hop() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        let player = run.game.players[0];
        let id = plant(
            &mut run,
            player.body.pos + player.facing * 1.3,
            FireflyKind::Normal,
        );
        let firefly = run.game.fireflies.last_mut().unwrap();
        firefly.tired = true;
        firefly.stamina = 0;
        run.step(NONE);
        let firefly = find(&run, id).unwrap();
        assert_eq!(firefly.hop, 0);
        assert!(firefly.tired);
        assert!(firefly.body.vel.length() <= FireflyKind::Normal.tired_speed() + 1e-4);
    }

    #[test]
    fn sprinting_tires_a_firefly_out_and_rest_recovers_it() {
        let kind = FireflyKind::Normal;
        let mut firefly = Firefly {
            id: 0,
            kind,
            body: Body::new(Vec2::new(-4.0, 0.0), FIREFLY_RADIUS, 1.0),
            wander: Vec2::new(1.0, 0.0),
            born: 0,
            status: FireflyStatus::Flying { leaves_at: 10_000 },
            stamina: kind.max_stamina(),
            tired: false,
            hop: 0,
            dodge_left: false,
        };
        // A pursuer that stays 2 units behind, inside the sprint zone but out of hop range.
        for tick in 0..kind.max_stamina() as u32 {
            assert!(!firefly.tired, "tired after only {tick} ticks");
            let threat = firefly.body.pos - Vec2::new(2.0, 0.0);
            fly(&mut firefly, &[threat], tick, 0.0, false);
            firefly.body.pos.x = -4.0;
        }
        assert!(firefly.tired);
        assert_eq!(firefly.stamina, 0);
        // Left alone, it gets its breath back.
        for tick in 0..RECOVERED_STAMINA as u32 {
            fly(&mut firefly, &[Vec2::new(-100.0, 0.0)], tick, 0.0, false);
        }
        assert!(!firefly.tired);
    }

    #[test]
    fn ids_are_ordered_and_never_reused() {
        let mut run = new_run(4, Format::FreeForAll);
        let mut removed = std::collections::BTreeSet::new();
        let mut previous: Vec<u32> = Vec::new();
        while run.outcome().is_none() {
            let inputs = Seat::ALL.map(|s| FireflyCatch::bot_input(&run.game, s, Difficulty::Hard));
            run.step(inputs);
            let ids: Vec<u32> = run.game.fireflies.iter().map(|f| f.id).collect();
            assert!(ids.windows(2).all(|w| w[0] < w[1]), "ids out of order");
            assert!(ids.iter().all(|id| *id < run.game.next_id));
            for id in &previous {
                if !ids.contains(id) {
                    removed.insert(*id);
                }
            }
            assert!(
                ids.iter().all(|id| !removed.contains(id)),
                "an id came back"
            );
            previous = ids;
        }
        assert!(
            removed.len() > 20,
            "expected plenty of fireflies to come and go"
        );
    }

    #[test]
    fn spawning_keeps_the_meadow_stocked() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        run.game.fireflies.clear();
        for _ in 0..(MIN_FLYING as u32 + 1) {
            run.step(NONE);
        }
        assert!(run.game.flying().count() >= MIN_FLYING);
        for _ in 0..600 {
            run.step(NONE);
            assert!(run.game.flying().count() <= MAX_FLYING);
        }
    }

    #[test]
    fn the_timer_ends_the_game() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        assert_eq!(
            run.game.phase,
            Phase::Playing {
                ticks_left: PLAY_TICKS
            }
        );
        for _ in 1..PLAY_TICKS {
            run.step(NONE);
        }
        assert!(run.outcome().is_none());
        run.step(NONE);
        assert_eq!(run.game.phase, Phase::Finished);
        // Nobody scored: everyone ties for first.
        assert_eq!(
            run.outcome().unwrap().placement,
            [Some(1), Some(1), None, None]
        );
    }

    fn finish(run: &mut Run<FireflyCatch>) -> Outcome {
        run.game.phase = Phase::Playing { ticks_left: 1 };
        run.step(NONE);
        run.outcome().unwrap()
    }

    #[test]
    fn free_for_all_places_by_points() {
        let mut run = new_run(3, Format::FreeForAll);
        to_playing(&mut run);
        plant_in_net(&mut run, 0, FireflyKind::Golden);
        plant_in_net(&mut run, 1, FireflyKind::Normal);
        plant_in_net(&mut run, 2, FireflyKind::Normal);
        let mut inputs = NONE;
        for input in &mut inputs[..3] {
            *input = swing();
        }
        run.step(inputs);
        let scores = run.game.players.map(|p| p.score);
        assert_eq!(scores, [GOLDEN_POINTS, 1, 1, 0]);
        assert_eq!(
            finish(&mut run).placement,
            [Some(1), Some(2), Some(2), None]
        );
    }

    #[test]
    fn the_solo_scores_triple_in_one_vs_three() {
        let mut run = new_run(4, Format::OneVsThree { solo: Seat(2) });
        to_playing(&mut run);
        plant_in_net(&mut run, 2, FireflyKind::Normal);
        plant_in_net(&mut run, 0, FireflyKind::Normal);
        plant_in_net(&mut run, 1, FireflyKind::Normal);
        run.step([swing(); MAX_PLAYERS]);
        assert_eq!(run.game.players[2].score, SOLO_MULTIPLIER);
        assert_eq!(run.game.team_scores()[..2], [SOLO_MULTIPLIER, 2]);
        assert_eq!(
            finish(&mut run).placement,
            [Some(2), Some(2), Some(1), Some(2)]
        );
    }

    #[test]
    fn two_vs_two_sums_each_team() {
        let mut run = new_run(
            4,
            Format::TwoVsTwo {
                team_of: [0, 1, 1, 0],
            },
        );
        to_playing(&mut run);
        plant_in_net(&mut run, 0, FireflyKind::Normal);
        plant_in_net(&mut run, 3, FireflyKind::Normal);
        plant_in_net(&mut run, 1, FireflyKind::Normal);
        run.step([swing(); MAX_PLAYERS]);
        assert_eq!(run.game.team_scores()[..2], [2, 1]);
        assert_eq!(
            finish(&mut run).placement,
            [Some(1), Some(2), Some(2), Some(1)]
        );
    }

    #[test]
    fn the_closer_net_wins_a_shared_firefly() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        // Both nets reach the firefly; seat 1's is centred closer to it.
        run.game.players[0].body.pos = Vec2::new(-NET_REACH - 0.4, 0.0);
        run.game.players[0].facing = Vec2::new(1.0, 0.0);
        run.game.players[1].body.pos = Vec2::new(NET_REACH + 0.2, 0.0);
        run.game.players[1].facing = Vec2::new(-1.0, 0.0);
        let id = plant(&mut run, Vec2::new(0.0, 0.0), FireflyKind::Normal);
        run.step([swing(); MAX_PLAYERS]);
        assert!(matches!(
            find(&run, id).unwrap().status,
            FireflyStatus::Caught { by: Seat(1), .. }
        ));
    }

    fn mean_score(seed: u64, difficulties: [Difficulty; MAX_PLAYERS]) -> [u32; MAX_PLAYERS] {
        let mut run = Run::<FireflyCatch>::new(
            seed,
            PlayerSetup::first(4),
            Format::FreeForAll,
            RunConfig::default(),
        );
        while run.outcome().is_none() {
            let inputs =
                Seat::ALL.map(|s| FireflyCatch::bot_input(&run.game, s, difficulties[s.index()]));
            run.step(inputs);
        }
        run.game.players.map(|p| p.score)
    }

    #[test]
    fn hard_bots_beat_easy_bots() {
        use Difficulty::{Easy, Hard};
        let (mut hard, mut easy) = (0, 0);
        for seed in 0..20 {
            // Rotate the hard bot through every seat so start positions don't matter.
            let mut difficulties = [Easy; MAX_PLAYERS];
            difficulties[seed as usize % MAX_PLAYERS] = Hard;
            let scores = mean_score(seed, difficulties);
            for (score, difficulty) in scores.iter().zip(difficulties) {
                match difficulty {
                    Hard => hard += score,
                    _ => easy += score,
                }
            }
        }
        let (hard, easy) = (hard as f32 / 20.0, easy as f32 / 60.0);
        println!("one hard bot vs three easy: hard averaged {hard}, easy {easy}");
        assert!(hard > easy * 1.3, "hard averaged {hard}, easy {easy}");
    }

    /// Tuning aid, not a rule: `cargo test -p minigame-firefly-catch --release -- --ignored
    /// --nocapture` prints how bot matches go per difficulty, player count and format.
    #[test]
    #[ignore]
    fn print_bot_report() {
        use Difficulty::{Easy, Hard, Normal};
        const SEEDS: u64 = 60;
        let formats = [
            (2, Format::FreeForAll),
            (3, Format::FreeForAll),
            (4, Format::FreeForAll),
            (
                4,
                Format::TwoVsTwo {
                    team_of: [0, 1, 1, 0],
                },
            ),
        ];
        for difficulty in [Easy, Normal, Hard] {
            for (players, format) in formats {
                let (mut total, mut winner, mut zeros) = (0u32, 0u32, 0u32);
                let mut wins = [0u32; MAX_PLAYERS];
                for seed in 0..SEEDS {
                    let mut run = Run::<FireflyCatch>::new(
                        seed,
                        PlayerSetup::first(players),
                        format,
                        RunConfig::default(),
                    );
                    while run.outcome().is_none() {
                        let inputs =
                            Seat::ALL.map(|s| FireflyCatch::bot_input(&run.game, s, difficulty));
                        run.step(inputs);
                    }
                    let scores = run.game.players.map(|p| p.score);
                    total += scores[..players].iter().sum::<u32>();
                    winner += scores[..players].iter().max().unwrap();
                    zeros += scores[..players].iter().filter(|s| **s == 0).count() as u32;
                    for seat in run.outcome().unwrap().winners() {
                        wins[seat.index()] += 1;
                    }
                }
                println!(
                    "{difficulty:?} {players}p {:?}: mean {:.1}, best {:.1}, zeros {zeros}, wins {:?}",
                    format.kind(),
                    total as f32 / (SEEDS as f32 * players as f32),
                    winner as f32 / SEEDS as f32,
                    &wins[..players]
                );
            }
            // 1v3 with the solo rotating through every seat.
            let mut solo_wins = 0;
            for seed in 0..SEEDS {
                let solo = Seat((seed % 4) as u8);
                let mut run = Run::<FireflyCatch>::new(
                    seed,
                    PlayerSetup::first(4),
                    Format::OneVsThree { solo },
                    RunConfig::default(),
                );
                while run.outcome().is_none() {
                    let inputs =
                        Seat::ALL.map(|s| FireflyCatch::bot_input(&run.game, s, difficulty));
                    run.step(inputs);
                }
                if run.outcome().unwrap().placement[solo.index()] == Some(1) {
                    solo_wins += 1;
                }
            }
            println!("{difficulty:?} 1v3: solo won or tied {solo_wins}/{SEEDS}");
        }
    }

    /// One bot, one firefly, no spawning: how many ticks does a catch take? This isolates the
    /// chase itself from crowding and competition.
    fn chase_ticks(seed: u64, difficulty: Difficulty) -> u32 {
        let mut state = FireflyCatch::new(seed, PlayerSetup::first(1), Format::FreeForAll);
        state.phase = Phase::Playing { ticks_left: 10_000 };
        state.fireflies.truncate(1);
        state.fireflies[0].status = FireflyStatus::Flying {
            leaves_at: u32::MAX,
        };
        let mut previous = [Input::default(); MAX_PLAYERS];
        for t in 0..3_000 {
            let mut current = [Input::default(); MAX_PLAYERS];
            current[0] = FireflyCatch::bot_input(&state, Seat(0), difficulty);
            let inputs = Inputs { current, previous };
            previous = current;
            state.update_players(&inputs);
            state.resolve_swings();
            if !state.fireflies[0].is_flying() {
                return t;
            }
            state.update_fireflies();
            state.tick += 1;
        }
        3_000
    }

    /// Tuning aid like [`print_bot_report`].
    #[test]
    #[ignore]
    fn print_chase_times() {
        for difficulty in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard] {
            let mut times: Vec<u32> = (0..40).map(|s| chase_ticks(s, difficulty)).collect();
            let mean = times.iter().sum::<u32>() as f32 / times.len() as f32 / 60.0;
            times.sort();
            println!(
                "{difficulty:?}: mean {mean:.2}s median {:.2}s max {:.2}s",
                times[20] as f32 / 60.0,
                times[39] as f32 / 60.0
            );
        }
    }

    #[test]
    fn passes_all_automated_quality_bars() {
        sim_check::check_all::<FireflyCatch>(16).unwrap();
    }
}
