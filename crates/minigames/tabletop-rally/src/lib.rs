//! Tabletop Rally: toy cars racing round hand-laid tracks on a giant table, seen from above, in
//! the style of Micro Machines' multiplayer. The stick turns your car towards where it points
//! on screen, A drives and B brakes and reverses. The camera follows whoever is furthest along,
//! and a car that drops off the edge of the screen (or off the table) is out of the round. The
//! last car left takes the round and a point, and the next round starts where it got to, every
//! car side by side. First to [`WIN_POINTS`] wins. A round that drags on closes in: after
//! [`CLOSE_IN_AFTER`] the view shrinks until someone drops out, and at [`ROUND_LIMIT`] the car
//! furthest along takes it. In 2v2 a team takes the round once only its cars are left.
//!
//! In 1v3 the solo is the getaway car: it starts [`HEAD_START`] ahead and the camera follows it.
//! A chaser that touches it takes the round for the three; it takes the round by leaving all
//! three off screen (or by lasting to [`ROUND_LIMIT`]).
//!
//! The table uses `sim_core::physics`' plane: `x` to the right, `y` towards the bottom of the
//! screen (stick up drives up the screen).

pub mod track;

use std::hash::{Hash, Hasher};

use sim_core::hash::hash_f32;
use sim_core::math;
use sim_core::physics::{self, Body, DT, Vec2};
use sim_core::{
    Button, Difficulty, Format, FormatKind, Input, Inputs, MAX_PLAYERS, Minigame, Outcome,
    PlayerSetup, Rng, Seat,
};

pub use track::{PropKind, TABLE_HALF, TRACK_COUNT, Theme, Track, track};

/// The opening countdown, the same in every minigame.
pub const INTRO_TICKS: u16 = sim_core::COUNTDOWN_TICKS;
pub const WIN_POINTS: u32 = 3;

/// Half the screen, in table units: a car whose centre gets further than this (plus its radius)
/// from the camera is off screen.
pub const VIEW_HALF: Vec2 = Vec2::new(8.0, 5.0);
/// How far the view closes in by the end.
pub const VIEW_HALF_MIN: Vec2 = Vec2::new(3.2, 2.0);
/// A round starts closing in this long after the start...
pub const CLOSE_IN_AFTER: u16 = 6 * 60;
/// ...and is fully closed in this long later...
pub const CLOSE_IN_TICKS: u16 = 6 * 60;
/// ...and over at this tick of the round however many cars are left.
pub const ROUND_LIMIT: u16 = CLOSE_IN_AFTER + CLOSE_IN_TICKS + 3 * 60;
/// The camera aims this far ahead of the car it follows, along the track (not in 1v3).
pub const LOOKAHEAD: f32 = 2.5;
/// The fraction of the way to its aim the camera moves each tick.
pub const CAMERA_FOLLOW: f32 = 0.1;

pub const CAR_RADIUS: f32 = 0.34;
pub const MAX_SPEED: f32 = 10.0;
/// 1v3: the getaway car's top speed.
pub const GETAWAY_SPEED: f32 = 10.25;
pub const REVERSE_SPEED: f32 = 3.0;
pub const ACCEL: f32 = 12.0;
pub const BRAKE: f32 = 16.0;
/// Speed lost per second with neither A nor B held.
pub const COAST: f32 = 2.0;
/// How hard the tyres stop a sideways slide, in units a second per second: a car going faster
/// than `sqrt(GRIP × radius)` can't hold a corner of that radius and slides wide.
pub const GRIP: f32 = 18.0;
/// Turning speed in radians a second, standing still and at speed.
pub const TURN_STANDING: f32 = 2.8;
pub const TURN_MOVING: f32 = 4.6;
/// On the tablecloth a car is slower: speed above this bleeds away at `OFF_ROAD_DRAG` units a
/// second per second.
pub const OFF_ROAD_SPEED: f32 = 3.6;
pub const OFF_ROAD_DRAG: f32 = 14.0;
const CAR_RESTITUTION: f32 = 0.5;
const PROP_RESTITUTION: f32 = 0.35;
/// A bump harder than this (momentum exchanged) counts as a hit for the view.
pub const BUMP_IMPULSE: f32 = 1.2;

/// 1v3: the getaway car starts this far ahead of the chasers.
pub const HEAD_START: f32 = 3.0;
/// 1v3: a chaser within this of the getaway car (centre to centre) has caught it.
pub const CATCH_REACH: f32 = 2.0 * CAR_RADIUS + 0.04;

/// The round's result shows this long (the last one's a little longer).
pub const RESULT_TICKS: u16 = 75;
pub const FINAL_RESULT_TICKS: u16 = 120;
/// Cars wait on the grid this long before the next round.
pub const GRID_TICKS: u16 = 60;

pub struct TabletopRally;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Intro {
        t: u16,
    },
    /// Cars wait side by side for the next round.
    Grid {
        t: u16,
    },
    Racing {
        t: u16,
    },
    /// The round is scored; cars roll to a stop.
    Result {
        t: u16,
    },
    Finished,
}

/// Why a car is out of the round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Out {
    Screen,
    Table,
    /// 1v3: the getaway car was caught.
    Caught,
}

/// How a round ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoundEnd {
    LastCar,
    /// Everyone left dropped out on the same tick: the furthest along took it.
    PhotoFinish,
    /// [`ROUND_LIMIT`]: the furthest along took it.
    TimeUp,
    Caught,
    /// 1v3: the getaway car fell off the table.
    OffTable,
    Escaped,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Car {
    pub body: Body,
    /// The way the car points, in radians from +x.
    pub heading: f32,
    /// False for seats outside a duel.
    pub playing: bool,
    pub getaway: bool,
    /// Out of this round, why, and on which tick.
    pub out: Option<(Out, u32)>,
    /// The centreline sample the car is nearest, and laps done (so progress keeps growing).
    pub sample: u16,
    pub lap: i16,
    /// Distance round the track, laps included.
    pub progress: f32,
    pub on_road: bool,
    /// The last tick the car bumped something hard.
    pub last_bump: Option<u32>,
}

impl Hash for Car {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.body.hash(state);
        hash_f32(self.heading, state);
        self.playing.hash(state);
        self.getaway.hash(state);
        self.out.hash(state);
        self.sample.hash(state);
        self.lap.hash(state);
        hash_f32(self.progress, state);
        self.on_road.hash(state);
        self.last_bump.hash(state);
    }
}

impl Car {
    pub fn racing(&self) -> bool {
        self.playing && self.out.is_none()
    }

    pub fn forward(&self) -> Vec2 {
        Vec2::from_angle(self.heading)
    }

    /// The fastest it goes on the road.
    pub fn top_speed(&self) -> f32 {
        if self.getaway {
            GETAWAY_SPEED
        } else {
            MAX_SPEED
        }
    }

    /// Speed along the way the car points (negative reversing).
    pub fn speed(&self) -> f32 {
        self.body.vel.dot(self.forward())
    }
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
    pub track: usize,
    pub cars: [Car; MAX_PLAYERS],
    pub camera: Vec2,
    /// The round being played, from 0.
    pub round: u8,
    /// Points by team ([`Format::team`]); in 1v3 rounds won by each side.
    pub points: [u32; MAX_PLAYERS],
    /// The team that took the last scored round, and how.
    pub round_winner: Option<u8>,
    pub round_end: Option<RoundEnd>,
    /// Set once the match is decided.
    pub outcome: Option<Outcome>,
}

/// The first straight at or after `s` with no prop on the road where the cars line up.
fn clear_start(track: &Track, s: f32) -> f32 {
    let mut s = track.straight_from(s);
    for _ in 0..track.samples() {
        let blocked = track.props.iter().any(|p| {
            let mut d = -1.5;
            let mut near = false;
            while d < HEAD_START + 1.5 {
                let at = track.point_at(s + d);
                near |= p.closest(at).distance(at) < track.width / 2.0 + p.radius;
                d += track.spacing;
            }
            near
        });
        if !blocked {
            return s;
        }
        s = track.straight_from(s + track.spacing);
    }
    s
}

/// Turn `angle` into -π..=π.
fn wrap_angle(angle: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let a = angle.rem_euclid(TAU);
    if a > PI { a - TAU } else { a }
}

impl State {
    pub fn car(&self, seat: Seat) -> &Car {
        &self.cars[seat.index()]
    }

    pub fn track(&self) -> &'static Track {
        track(self.track)
    }

    pub fn getaway(&self) -> Option<Seat> {
        match self.format {
            Format::OneVsThree { solo } => Some(solo),
            _ => None,
        }
    }

    /// Seats in the game (benched duel seats aren't).
    pub fn playing(&self) -> impl Iterator<Item = Seat> + '_ {
        self.players
            .seats()
            .filter(|s| self.cars[s.index()].playing)
    }

    /// Seats still in this round.
    pub fn racing(&self) -> impl Iterator<Item = Seat> + '_ {
        self.playing()
            .filter(|s| self.cars[s.index()].out.is_none())
    }

    /// Half the screen right now: it closes in late in a round.
    pub fn view_half(&self) -> Vec2 {
        let t = match self.phase {
            Phase::Racing { t } => t,
            _ => 0,
        };
        let k = (t.saturating_sub(CLOSE_IN_AFTER) as f32 / CLOSE_IN_TICKS as f32).min(1.0);
        VIEW_HALF.lerp(VIEW_HALF_MIN, k)
    }

    pub fn closing_in(&self) -> bool {
        matches!(self.phase, Phase::Racing { t } if t >= CLOSE_IN_AFTER)
    }

    pub fn on_screen(&self, pos: Vec2) -> bool {
        let half = self.view_half();
        let d = pos - self.camera;
        d.x.abs() <= half.x + CAR_RADIUS && d.y.abs() <= half.y + CAR_RADIUS
    }

    /// The car the camera follows: the getaway car, or whoever is furthest along.
    pub fn leader(&self) -> Option<Seat> {
        if let Some(getaway) = self.getaway() {
            return Some(getaway);
        }
        let mut best: Option<Seat> = None;
        for seat in self.racing() {
            let p = self.cars[seat.index()].progress;
            if best.is_none_or(|b| p > self.cars[b.index()].progress) {
                best = Some(seat);
            }
        }
        best
    }

    /// Where the camera aims: ahead of the car it follows, along the track. In 1v3 it aims at
    /// the getaway car itself, so the chasers behind it stay in view.
    fn camera_aim(&self) -> Option<Vec2> {
        let seat = self.leader()?;
        let car = self.car(seat);
        if car.getaway {
            return Some(car.body.pos);
        }
        let track = self.track();
        let s = car.progress;
        Some(car.body.pos + (track.point_at(s + LOOKAHEAD) - track.point_at(s)))
    }

    /// The teams with a car still in the round.
    fn teams_left(&self) -> Vec<u8> {
        let mut teams: Vec<u8> = self.racing().filter_map(|s| self.format.team(s)).collect();
        teams.sort_unstable();
        teams.dedup();
        teams
    }

    /// Put every car side by side across the road on the first straight at or after `s`,
    /// standing still and facing along it; the getaway car goes [`HEAD_START`] ahead in the
    /// middle. The lanes are drawn from the seed each round, so nobody keeps the inside line.
    fn line_up(&mut self, s: f32) {
        let track = self.track();
        let s = clear_start(track, s);
        let getaway = self.getaway();
        let mut seats: Vec<Seat> = self.playing().filter(|s| Some(*s) != getaway).collect();
        for i in (1..seats.len()).rev() {
            let j = self.rng.range_u32(0, i as u32) as usize;
            seats.swap(i, j);
        }
        let lanes = seats.len();
        let gap = (track.width - 1.0) / lanes.max(2) as f32;
        let place = |car: &mut Car, s: f32, side: f32| {
            let i = track.index(s);
            car.body.pos = track.point_at(s) + track.normal_at(s) * side;
            car.body.vel = Vec2::ZERO;
            car.heading = track.tangent_at(s).angle();
            car.out = None;
            car.sample = i as u16;
            car.lap = (s / track.length).floor() as i16;
            car.progress = s;
            car.on_road = true;
        };
        for (k, seat) in seats.iter().enumerate() {
            let side = (k as f32 - (lanes as f32 - 1.0) / 2.0) * gap;
            place(&mut self.cars[seat.index()], s, side);
        }
        if let Some(getaway) = getaway {
            place(&mut self.cars[getaway.index()], s + HEAD_START, 0.0);
        }
        // Put the camera straight where it belongs: no pan at the start of a round.
        if let Some(aim) = self.camera_aim() {
            self.camera = aim;
        } else {
            self.camera = track.point_at(s + LOOKAHEAD);
        }
    }

    fn drive(&mut self, inputs: &Inputs) {
        for seat in self.racing().collect::<Vec<_>>() {
            let on_road = self.cars[seat.index()].on_road;
            let car = &mut self.cars[seat.index()];
            let input = &inputs.current[seat.index()];
            let forward = car.forward();
            let side = forward.perp();
            let mut along = car.body.vel.dot(forward);
            let mut across = car.body.vel.dot(side);

            let stick = Vec2::from_stick(input);
            if stick != Vec2::ZERO {
                let k = (along.abs() / 4.0).min(1.0);
                let rate = (TURN_STANDING + (TURN_MOVING - TURN_STANDING) * k) * DT;
                let turn = wrap_angle(stick.angle() - car.heading).clamp(-rate, rate);
                car.heading = wrap_angle(car.heading + turn);
            }
            let top = if on_road {
                car.top_speed()
            } else {
                OFF_ROAD_SPEED
            };
            let gas = input.held(Button::A);
            let brake = input.held(Button::B);
            if brake {
                if along > 0.0 {
                    along = (along - BRAKE * DT).max(0.0);
                } else {
                    along = (along - ACCEL * DT).max(-REVERSE_SPEED);
                }
            } else if gas {
                if along < 0.0 {
                    along = (along + BRAKE * DT).min(0.0);
                } else {
                    along = (along + ACCEL * DT).min(top.max(along));
                }
            } else {
                along -= along.signum() * (COAST * DT).min(along.abs());
            }
            if along.abs() > top {
                along -= along.signum() * (OFF_ROAD_DRAG * DT).min(along.abs() - top);
            }
            across -= across.signum() * (GRIP * DT).min(across.abs());
            // The car may have turned this tick: what was straight on is partly sideways from
            // the next tick, and that's the slide the grip takes out.
            car.body.vel = forward * along + side * across;
            car.body.pos += car.body.vel * DT;
        }
    }

    fn collide(&mut self) {
        let tick = self.tick;
        let seats: Vec<Seat> = self.racing().collect();
        let mut bodies: Vec<Body> = seats.iter().map(|s| self.cars[s.index()].body).collect();
        let mut bumps = Vec::new();
        physics::collide_all(&mut bodies, CAR_RESTITUTION, |i, j, contact| {
            if contact.impulse > BUMP_IMPULSE {
                bumps.push(i);
                bumps.push(j);
            }
        });
        for (k, seat) in seats.iter().enumerate() {
            self.cars[seat.index()].body = bodies[k];
        }
        for k in bumps {
            self.cars[seats[k].index()].last_bump = Some(tick);
        }
        let track = self.track();
        for seat in seats {
            let car = &mut self.cars[seat.index()];
            for prop in &track.props {
                let near = prop.closest(car.body.pos);
                let mut solid = Body::new(near, prop.radius, 1.0e6);
                if let Some(contact) = physics::collide(&mut car.body, &mut solid, PROP_RESTITUTION)
                    && contact.impulse > BUMP_IMPULSE
                {
                    car.last_bump = Some(tick);
                }
            }
        }
    }

    /// Follow each car round the track, and note whether it's on the road.
    fn track_progress(&mut self) {
        let track = self.track();
        let n = track.samples();
        for seat in self.racing().collect::<Vec<_>>() {
            let car = &mut self.cars[seat.index()];
            let before = car.sample as usize;
            let i = track.nearest(car.body.pos, before, 8);
            if before > n * 3 / 4 && i < n / 4 {
                car.lap += 1;
            } else if before < n / 4 && i > n * 3 / 4 {
                car.lap -= 1;
            }
            car.sample = i as u16;
            let offset = car.body.pos - track.centre[i];
            let along = offset
                .dot(track.tangent[i])
                .clamp(-track.spacing, track.spacing);
            car.progress = car.lap as f32 * track.length + i as f32 * track.spacing + along;
            let across = offset.dot(track.tangent[i].perp()).abs();
            car.on_road = across <= track.width / 2.0;
        }
    }

    /// Cars past the table's edge fall off.
    fn fall_off(&mut self) {
        let tick = self.tick;
        for car in self.cars.iter_mut().filter(|c| c.racing()) {
            let pos = car.body.pos;
            if pos.x.abs() > TABLE_HALF.x || pos.y.abs() > TABLE_HALF.y {
                car.out = Some((Out::Table, tick));
            }
        }
    }

    /// Cars off the screen are out (the getaway car is always on it).
    fn knock_out(&mut self) {
        let tick = self.tick;
        for seat in self.racing().collect::<Vec<_>>() {
            if !self.on_screen(self.cars[seat.index()].body.pos) && Some(seat) != self.getaway() {
                self.cars[seat.index()].out = Some((Out::Screen, tick));
            }
        }
    }

    /// 1v3: a chaser touching the getaway car catches it.
    fn catch(&mut self) -> bool {
        let Some(getaway) = self.getaway() else {
            return false;
        };
        let target = self.cars[getaway.index()];
        if !target.racing() {
            return false;
        }
        let caught = self
            .racing()
            .filter(|s| *s != getaway)
            .any(|s| self.cars[s.index()].body.pos.distance(target.body.pos) <= CATCH_REACH);
        if caught {
            self.cars[getaway.index()].out = Some((Out::Caught, self.tick));
        }
        caught
    }

    /// The team of the furthest car along among `seats`; an exact tie is drawn from the seed.
    fn furthest(&mut self, seats: &[Seat]) -> Option<u8> {
        let best = seats
            .iter()
            .map(|s| self.cars[s.index()].progress)
            .fold(f32::MIN, f32::max);
        let tied: Vec<Seat> = seats
            .iter()
            .copied()
            .filter(|s| self.cars[s.index()].progress == best)
            .collect();
        let pick = match tied.len() {
            0 => return None,
            1 => tied[0],
            n => tied[self.rng.range_u32(0, n as u32 - 1) as usize],
        };
        self.format.team(pick)
    }

    /// Whether the round is over, and if so who took it and how.
    fn round_result(&mut self, t: u16, racing_before: &[Seat]) -> Option<(u8, RoundEnd)> {
        if let Some(getaway) = self.getaway() {
            let solo = self.format.team(getaway)?;
            let trio = 1 - solo;
            let car = self.cars[getaway.index()];
            match car.out {
                Some((Out::Table, _)) => return Some((trio, RoundEnd::OffTable)),
                Some(_) => return Some((trio, RoundEnd::Caught)),
                None => {}
            }
            if self.racing().all(|s| s == getaway) {
                return Some((solo, RoundEnd::Escaped));
            }
            if t + 1 >= ROUND_LIMIT {
                return Some((solo, RoundEnd::TimeUp));
            }
            return None;
        }
        let teams = self.teams_left();
        match teams.len() {
            0 => {
                let team = self.furthest(racing_before)?;
                Some((team, RoundEnd::PhotoFinish))
            }
            1 => Some((teams[0], RoundEnd::LastCar)),
            _ if t + 1 >= ROUND_LIMIT => {
                let left: Vec<Seat> = self.racing().collect();
                let team = self.furthest(&left)?;
                Some((team, RoundEnd::TimeUp))
            }
            _ => None,
        }
    }

    fn score(&mut self, team: u8, end: RoundEnd) {
        self.points[team as usize] += 1;
        self.round_winner = Some(team);
        self.round_end = Some(end);
        if self.points[team as usize] >= WIN_POINTS {
            self.outcome = Some(Outcome::from_team_scores(
                &self.players,
                &self.format,
                &self.points,
            ));
        }
    }

    /// The camera eases towards its aim.
    fn follow(&mut self) {
        if let Some(aim) = self.camera_aim() {
            self.camera += (aim - self.camera) * CAMERA_FOLLOW;
        }
    }

    /// After the round: cars roll to a stop with nobody steering.
    fn coast(&mut self) {
        for car in self.cars.iter_mut().filter(|c| c.racing()) {
            car.body.vel *= (1.0 - 3.0 * DT).max(0.0);
            car.body.pos += car.body.vel * DT;
        }
        self.collide();
        self.track_progress();
    }
}

impl Minigame for TabletopRally {
    type State = State;

    const ID: &'static str = "tabletop-rally";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[
        FormatKind::FreeForAll,
        FormatKind::OneVsThree,
        FormatKind::TwoVsTwo,
        FormatKind::Duel,
    ];
    // At most (teams × (WIN_POINTS − 1) + 1) rounds, each at most the round limit.
    const MAX_TICKS: u32 = INTRO_TICKS as u32
        + (MAX_PLAYERS as u32 * (WIN_POINTS - 1) + 1)
            * (GRID_TICKS + ROUND_LIMIT + FINAL_RESULT_TICKS) as u32
        + 60;

    fn new(seed: u64, players: PlayerSetup, format: Format) -> State {
        let car = Car {
            body: Body::new(Vec2::ZERO, CAR_RADIUS, 1.0),
            heading: 0.0,
            playing: false,
            getaway: false,
            out: None,
            sample: 0,
            lap: 0,
            progress: 0.0,
            on_road: true,
            last_bump: None,
        };
        let mut rng = Rng::new(seed);
        let track = rng.range_u32(0, TRACK_COUNT as u32 - 1) as usize;
        let mut state = State {
            seed,
            players,
            format,
            phase: Phase::Intro { t: 0 },
            rng,
            tick: 0,
            track,
            cars: [car; MAX_PLAYERS],
            camera: Vec2::ZERO,
            round: 0,
            points: [0; MAX_PLAYERS],
            round_winner: None,
            round_end: None,
            outcome: None,
        };
        // Seats outside a duel sit this one out.
        let getaway = state.getaway();
        for seat in players.seats() {
            let car = &mut state.cars[seat.index()];
            car.playing = format.team(seat).is_some();
            car.getaway = getaway == Some(seat);
        }
        state.line_up(0.0);
        state
    }

    fn step(state: &mut State, inputs: &Inputs) {
        if let Phase::Intro { t } = state.phase {
            state.phase = if t + 1 >= INTRO_TICKS {
                Phase::Racing { t: 0 }
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
            Phase::Grid { t } => {
                state.phase = if t + 1 >= GRID_TICKS {
                    Phase::Racing { t: 0 }
                } else {
                    Phase::Grid { t: t + 1 }
                };
            }
            Phase::Racing { t } => {
                let racing_before: Vec<Seat> = state.racing().collect();
                state.drive(inputs);
                state.catch();
                state.collide();
                state.track_progress();
                state.fall_off();
                state.follow();
                state.knock_out();
                state.phase = match state.round_result(t, &racing_before) {
                    Some((team, end)) => {
                        state.score(team, end);
                        Phase::Result { t: 0 }
                    }
                    None => Phase::Racing { t: t + 1 },
                };
            }
            Phase::Result { t } => {
                state.coast();
                state.follow();
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
                    // The next round starts where the round's winner got to.
                    let winner = state.leader_of(state.round_winner);
                    let s = winner.map_or(0.0, |w| state.cars[w.index()].progress);
                    state.round += 1;
                    state.round_winner = None;
                    state.round_end = None;
                    state.line_up(s);
                    Phase::Grid { t: 0 }
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
        match state.phase {
            Phase::Racing { .. } if state.car(seat).racing() => bot::input(state, seat, difficulty),
            _ => Input::default(),
        }
    }
}

impl State {
    /// Where the next round starts from: the furthest car of the winning team still on the
    /// table (in 1v3, the getaway car's spot, caught or not; the three line up behind it).
    fn leader_of(&self, team: Option<u8>) -> Option<Seat> {
        if let Some(getaway) = self.getaway() {
            return Some(getaway);
        }
        let team = team?;
        let mut best: Option<Seat> = None;
        for seat in self.playing() {
            let car = self.car(seat);
            if self.format.team(seat) != Some(team) || matches!(car.out, Some((Out::Table, _))) {
                continue;
            }
            if best.is_none_or(|b| car.progress > self.car(b).progress) {
                best = Some(seat);
            }
        }
        best.or_else(|| {
            self.playing()
                .max_by(|a, b| self.car(*a).progress.total_cmp(&self.car(*b).progress))
        })
    }
}

mod bot {
    use super::*;

    struct Skill {
        /// How far ahead along the road it steers for, at full speed.
        look: f32,
        /// The share of each corner's safe speed it carries in (over 1 brakes too late).
        corner: f32,
        /// Up to this much wrong in the direction it steers, in radians.
        wobble: f32,
        /// Ticks per decision window: the line, wobble and lapses are redrawn each window.
        window: u32,
        /// Percent of windows in which it lifts off and drifts wide.
        lapse: u32,
        /// Percent of windows in which it brakes far too late for whatever corner comes.
        slip: u32,
        /// How far from the middle of the road its line may sit, as a share of half the width.
        spread: f32,
        /// 1v3: the getaway car's slips and lapses, in percent of the usual.
        getaway_care: u32,
        /// 1v3: chasers go straight for the getaway car within this.
        chase: f32,
    }

    fn skill(difficulty: Difficulty) -> Skill {
        match difficulty {
            Difficulty::Easy => Skill {
                look: 2.4,
                corner: 1.25,
                wobble: 0.3,
                window: 24,
                lapse: 10,
                slip: 20,
                spread: 0.5,
                getaway_care: 35,
                chase: 4.0,
            },
            Difficulty::Normal => Skill {
                look: 2.9,
                corner: 1.1,
                wobble: 0.16,
                window: 30,
                lapse: 5,
                slip: 12,
                spread: 0.45,
                getaway_care: 80,
                chase: 5.0,
            },
            Difficulty::Hard => Skill {
                look: 3.3,
                corner: 1.0,
                wobble: 0.07,
                window: 36,
                lapse: 2,
                slip: 6,
                spread: 0.4,
                getaway_care: 100,
                chase: 5.0,
            },
        }
    }

    /// The sideways acceleration the bots believe the tyres hold, for their corner speeds.
    const CORNER_GRIP: f32 = GRIP * 0.9;
    /// How hard the bots plan to slow for a corner, in units a second per second.
    const PLAN_BRAKE: f32 = 12.0;
    /// How far ahead the bots look for corners.
    const PLAN_AHEAD: f32 = 14.0;
    /// How much faster than safe a slipping bot takes the next corner.
    const SLIP: f32 = 0.8;
    /// The furthest a bot moves its aim sideways for one obstacle.
    const MAX_SWERVE: f32 = 2.5;
    /// The getaway car swerves from a chaser within this.
    const CHASE_RANGE: f32 = 5.0;
    /// ...and flat out, whatever the corner, within this.
    const RAM_RANGE: f32 = 1.5;

    struct Roll {
        line: f32,
        wobble: f32,
        lapse: bool,
        slip: bool,
    }

    /// A new seed from `seed` and `part`, unrelated to any other pair's.
    fn mix(seed: u64, part: u64) -> u64 {
        let mut rng = Rng::new(seed ^ part.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        ((rng.next_u32() as u64) << 32) | rng.next_u32() as u64
    }

    fn roll(state: &State, seat: Seat, skill: &Skill) -> Roll {
        let care = if state.car(seat).getaway {
            skill.getaway_care
        } else {
            100
        };
        let window = state.tick / skill.window;
        // Each part is mixed in on its own: XORed together, a seed and a window could cancel
        // out, and bots in different matches would roll alike.
        let key = [seat.0 as u64, state.round as u64, window as u64]
            .into_iter()
            .fold(state.seed, mix);
        let mut rng = Rng::new(key);
        let unit = |rng: &mut Rng| rng.range_u32(0, 2000) as f32 / 1000.0 - 1.0;
        Roll {
            line: unit(&mut rng) * skill.spread,
            wobble: unit(&mut rng) * skill.wobble,
            lapse: rng.chance(skill.lapse * care / 100),
            slip: rng.chance(skill.slip * care / 100),
        }
    }

    /// The fastest the car can be going at `s` and still make every corner within
    /// [`PLAN_AHEAD`] by braking at [`PLAN_BRAKE`].
    fn safe_speed(track: &Track, s: f32, corner: f32, top: f32) -> f32 {
        let mut limit = top;
        let mut d = 0.0;
        while d < PLAN_AHEAD {
            let k = track.curvature[track.index(s + d)].abs().max(1e-4);
            let v = (CORNER_GRIP / k).sqrt() * corner;
            limit = limit.min((v * v + 2.0 * PLAN_BRAKE * d).sqrt());
            d += track.spacing * 2.0;
        }
        limit
    }

    /// Nudge `target` sideways so the way from `from` doesn't run into a prop or another car.
    fn avoid(state: &State, seat: Seat, from: Vec2, target: Vec2) -> Vec2 {
        let way = target - from;
        let len = way.length();
        if len < 1e-3 {
            return target;
        }
        let dir = way * (1.0 / len);
        let mut shifted = target;
        let mut check = |centre: Vec2, radius: f32| {
            let rel = centre - from;
            let t = rel.dot(dir);
            if t <= 0.0 || t >= len + radius {
                return;
            }
            let off = rel.perp_dot(dir);
            let clear = radius + CAR_RADIUS + 0.25;
            if off.abs() < clear {
                // Pass on the side the obstacle isn't: `off` > 0 means it's to the right.
                let side = if off >= 0.0 { 1.0 } else { -1.0 };
                let push = ((clear - off.abs()) * len / t.max(0.5)).min(MAX_SWERVE);
                shifted += dir.perp() * (side * push);
            }
        };
        for prop in &state.track().props {
            let near = prop.closest(from + dir * (len * 0.5));
            check(near, prop.radius);
        }
        for other in state.racing() {
            if other != seat && !state.format.same_team(seat, other) {
                let car = state.car(other);
                check(car.body.pos, CAR_RADIUS);
            }
        }
        shifted
    }

    fn stick_towards(direction: Vec2) -> (i8, i8) {
        let d = direction.normalize_or_zero();
        (Input::quantize_axis(d.x), Input::quantize_axis(-d.y))
    }

    fn rotate(v: Vec2, angle: f32) -> Vec2 {
        let (s, c) = (math::sin(angle), math::cos(angle));
        Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
    }

    pub fn input(state: &State, seat: Seat, difficulty: Difficulty) -> Input {
        let skill = skill(difficulty);
        let roll = roll(state, seat, &skill);
        let car = state.car(seat);
        let track = state.track();
        let pos = car.body.pos;
        let speed = car.speed();
        let s = car.progress;

        let half = track.width / 2.0;
        let mut line = roll.line * half;
        // The getaway car keeps to the side away from the nearest chaser behind it.
        if car.getaway {
            let normal = track.normal_at(s);
            let nearest = state
                .racing()
                .filter(|o| *o != seat)
                .map(|o| state.car(o).body.pos)
                .min_by(|a, b| a.distance(pos).total_cmp(&b.distance(pos)));
            if let Some(chaser) = nearest
                && chaser.distance(pos) < CHASE_RANGE
            {
                let side = if (chaser - pos).dot(normal) > 0.0 {
                    -1.0
                } else {
                    1.0
                };
                line = side * half * 0.5;
            }
        }
        let look = 1.0 + skill.look * (speed.max(0.0) / MAX_SPEED);
        let aim = track.point_at(s + look) + track.normal_at(s + look) * line;
        let mut target = avoid(state, seat, pos, aim);
        let corner = if roll.slip {
            skill.corner + SLIP
        } else {
            skill.corner
        };
        let mut wanted = safe_speed(track, s, corner, car.top_speed());

        if let Some(getaway) = state.getaway()
            && getaway != seat
        {
            let prey = state.car(getaway);
            let gap = prey.body.pos.distance(pos);
            if gap < skill.chase {
                let lead = gap / MAX_SPEED.max(1.0);
                target = prey.body.pos + prey.body.vel * lead;
                if gap < RAM_RANGE {
                    wanted = MAX_SPEED;
                }
            }
        }

        let mut direction = rotate(target - pos, roll.wobble);
        let mut gas = speed < wanted;
        let mut brake = speed > wanted + 0.8;
        if roll.lapse && speed > 3.0 {
            gas = false;
            brake = false;
            direction = rotate(direction, roll.wobble * 2.0);
        }
        // Wedged against something ahead: back off, still pointing where it wants to go.
        let facing = car.forward();
        let blocked = track.props.iter().any(|p| {
            let near = p.closest(pos);
            near.distance(pos) < p.radius + CAR_RADIUS + 0.08 && (near - pos).dot(facing) > 0.0
        });
        if blocked && speed.abs() < 1.0 && facing.dot(direction.normalize_or_zero()) > 0.3 {
            gas = false;
            brake = true;
            direction = rotate(direction, 0.6);
        }
        let (stick_x, stick_y) = stick_towards(direction);
        let mut input = Input {
            buttons: 0,
            stick_x,
            stick_y,
        };
        input.set(Button::A, gas);
        input.set(Button::B, brake);
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

    fn new_run(players: usize, format: Format) -> Run<TabletopRally> {
        Run::new(1, PlayerSetup::first(players), format, RunConfig::default())
    }

    fn run_until(run: &mut Run<TabletopRally>, done: impl Fn(&State) -> bool) {
        for _ in 0..TabletopRally::MAX_TICKS {
            if done(&run.game) {
                return;
            }
            run.step(NONE);
        }
        panic!("never got there: {:?}", run.game.phase);
    }

    fn to_racing(run: &mut Run<TabletopRally>) {
        run_until(run, |g| matches!(g.phase, Phase::Racing { .. }));
    }

    fn drive(stick: Vec2) -> Input {
        let mut input = Input {
            buttons: 0,
            stick_x: Input::quantize_axis(stick.x),
            stick_y: Input::quantize_axis(-stick.y),
        };
        input.set(Button::A, true);
        input
    }

    #[test]
    fn cars_line_up_side_by_side_on_the_road() {
        let run = new_run(4, Format::FreeForAll);
        let game = &run.game;
        let track = game.track();
        for seat in Seat::ALL {
            let car = game.car(seat);
            assert!(car.playing && car.on_road);
            assert!(car.progress.abs() < track.spacing);
            assert!(game.on_screen(car.body.pos));
        }
        for a in Seat::ALL {
            for b in Seat::ALL.into_iter().filter(|b| *b > a) {
                let d = game.car(a).body.pos.distance(game.car(b).body.pos);
                assert!(d > 2.0 * CAR_RADIUS, "{a:?} {b:?} {d}");
            }
        }
        let duel = new_run(
            4,
            Format::Duel {
                a: Seat(1),
                b: Seat(3),
            },
        );
        assert!(!duel.game.cars[0].playing && duel.game.cars[1].playing);
    }

    #[test]
    fn nobody_moves_during_the_intro() {
        let mut run = new_run(2, Format::FreeForAll);
        let start = run.game.cars[0].body.pos;
        let mut inputs = NONE;
        inputs[0] = drive(Vec2::new(1.0, 0.0));
        for _ in 0..INTRO_TICKS - 1 {
            run.step(inputs);
        }
        assert_eq!(run.game.cars[0].body.pos, start);
    }

    #[test]
    fn a_drives_and_the_stick_turns_the_car_towards_it() {
        let mut run = new_run(2, Format::FreeForAll);
        to_racing(&mut run);
        let game = &mut run.game;
        let start = game.cars[0];
        let ahead = start.forward();
        let mut inputs = Inputs::default();
        inputs.current[0] = drive(ahead);
        for _ in 0..30 {
            game.drive(&inputs);
        }
        let moved = game.cars[0].body.pos - start.body.pos;
        assert!(moved.dot(ahead) > 1.0, "{moved:?}");
        assert!(game.cars[0].speed() > 4.0);
        // Point the stick a quarter turn round: the car swings round to face it.
        let left = ahead.perp();
        inputs.current[0] = drive(left);
        for _ in 0..30 {
            game.drive(&inputs);
        }
        assert!(game.cars[0].forward().dot(left) > 0.95);
        // B brakes to a stop, then reverses.
        inputs.current[0] = Input::default().with(Button::B);
        for _ in 0..90 {
            game.drive(&inputs);
        }
        assert!(game.cars[0].speed() < -1.0);
    }

    #[test]
    fn the_tablecloth_is_slow() {
        let mut run = new_run(2, Format::FreeForAll);
        to_racing(&mut run);
        let game = &mut run.game;
        game.cars[0].body.vel = game.cars[0].forward() * MAX_SPEED;
        game.cars[0].on_road = false;
        let mut inputs = Inputs::default();
        inputs.current[0] = drive(game.cars[0].forward());
        for _ in 0..60 {
            game.drive(&inputs);
        }
        assert!(game.cars[0].speed() < OFF_ROAD_SPEED + 0.3);
        game.cars[0].on_road = true;
        for _ in 0..60 {
            game.drive(&inputs);
        }
        assert!(game.cars[0].speed() > MAX_SPEED - 0.3);
    }

    #[test]
    fn the_camera_follows_the_leader_and_the_last_car_on_screen_scores() {
        let mut run = new_run(3, Format::FreeForAll);
        to_racing(&mut run);
        // Seat 0 races off along the road; the others sit still.
        let track = track(run.game.track);
        for _ in 0..600 {
            let car = run.game.cars[0];
            let aim = track.point_at(car.progress + 3.0) - car.body.pos;
            let mut inputs = NONE;
            inputs[0] = drive(aim);
            run.step(inputs);
            if !matches!(run.game.phase, Phase::Racing { .. }) {
                break;
            }
        }
        assert!(matches!(run.game.phase, Phase::Result { .. }));
        assert_eq!(run.game.leader(), Some(Seat(0)));
        assert_eq!(run.game.cars[1].out.map(|o| o.0), Some(Out::Screen));
        assert_eq!(run.game.cars[2].out.map(|o| o.0), Some(Out::Screen));
        assert_eq!(run.game.round_winner, Some(0));
        assert_eq!(run.game.round_end, Some(RoundEnd::LastCar));
        assert_eq!(run.game.points[0], 1);
        // The next round starts where the winner rolled to, everyone side by side again.
        let reached = run.game.cars[0].progress;
        run_until(&mut run, |g| matches!(g.phase, Phase::Grid { .. }));
        assert_eq!(run.game.round, 1);
        let start = run.game.cars[0].progress;
        assert!(start >= reached, "{start} {reached}");
        for seat in 0..3 {
            let car = run.game.cars[seat];
            assert!(car.racing());
            assert_eq!(car.progress, start);
            assert!(run.game.on_screen(car.body.pos));
        }
    }

    #[test]
    fn falling_off_the_table_is_out() {
        let mut run = new_run(2, Format::FreeForAll);
        to_racing(&mut run);
        run.game.cars[1].body.pos = Vec2::new(TABLE_HALF.x + 0.1, 0.0);
        run.step(NONE);
        assert_eq!(run.game.cars[1].out.map(|o| o.0), Some(Out::Table));
        assert_eq!(run.game.round_winner, Some(0));
    }

    #[test]
    fn everyone_out_on_one_tick_goes_to_the_furthest() {
        let mut run = new_run(2, Format::FreeForAll);
        to_racing(&mut run);
        run.game.cars[0].body.pos = Vec2::new(TABLE_HALF.x + 0.1, 0.0);
        run.game.cars[1].body.pos = Vec2::new(TABLE_HALF.x + 0.1, 3.0);
        run.step(NONE);
        assert_eq!(run.game.round_end, Some(RoundEnd::PhotoFinish));
        assert!(run.game.round_winner.is_some());
    }

    #[test]
    fn a_long_round_closes_in_and_ends() {
        let mut run = new_run(2, Format::FreeForAll);
        to_racing(&mut run);
        run.game.phase = Phase::Racing { t: CLOSE_IN_AFTER };
        assert!(run.game.closing_in());
        assert_eq!(run.game.view_half(), VIEW_HALF);
        run.game.phase = Phase::Racing {
            t: CLOSE_IN_AFTER + CLOSE_IN_TICKS,
        };
        assert!(run.game.view_half().distance(VIEW_HALF_MIN) < 1e-5);
        run.game.phase = Phase::Racing { t: ROUND_LIMIT - 1 };
        run.step(NONE);
        assert_eq!(run.game.round_end, Some(RoundEnd::TimeUp));
    }

    #[test]
    fn a_team_takes_the_round_once_only_its_cars_are_left() {
        let mut run = new_run(
            4,
            Format::TwoVsTwo {
                team_of: [0, 1, 0, 1],
            },
        );
        to_racing(&mut run);
        run.game.cars[1].body.pos = Vec2::new(TABLE_HALF.x + 1.0, 0.0);
        run.step(NONE);
        assert!(matches!(run.game.phase, Phase::Racing { .. }));
        run.game.cars[3].body.pos = Vec2::new(TABLE_HALF.x + 1.0, 0.0);
        run.step(NONE);
        assert_eq!(run.game.round_winner, Some(0));
        assert_eq!(run.game.points, [1, 0, 0, 0]);
    }

    #[test]
    fn the_getaway_car_leads_and_the_camera_stays_on_it() {
        let mut run = new_run(4, Format::OneVsThree { solo: Seat(2) });
        let game = &run.game;
        assert!(game.cars[2].getaway);
        assert!((game.cars[2].progress - HEAD_START).abs() < 0.01);
        assert_eq!(game.leader(), Some(Seat(2)));
        to_racing(&mut run);
        // A chaser racing ahead doesn't take the camera.
        run.game.cars[0].progress = 50.0;
        assert_eq!(run.game.leader(), Some(Seat(2)));
    }

    #[test]
    fn a_chaser_touching_the_getaway_car_takes_the_round() {
        let mut run = new_run(4, Format::OneVsThree { solo: Seat(0) });
        to_racing(&mut run);
        run.game.cars[3].body.pos = run.game.cars[0].body.pos + Vec2::new(0.6, 0.0);
        run.step(NONE);
        assert_eq!(run.game.cars[0].out.map(|o| o.0), Some(Out::Caught));
        assert_eq!(run.game.round_end, Some(RoundEnd::Caught));
        assert_eq!(run.game.points, [0, 1, 0, 0]);
    }

    #[test]
    fn leaving_every_chaser_off_screen_takes_the_round_for_the_getaway() {
        let mut run = new_run(4, Format::OneVsThree { solo: Seat(1) });
        to_racing(&mut run);
        for seat in [0, 2, 3] {
            run.game.cars[seat].body.pos = run.game.camera + Vec2::new(VIEW_HALF.x + 1.0, 0.0);
        }
        run.step(NONE);
        assert_eq!(run.game.round_end, Some(RoundEnd::Escaped));
        assert_eq!(run.game.points, [1, 0, 0, 0]);
    }

    #[test]
    fn first_to_three_wins() {
        let mut run = new_run(3, Format::FreeForAll);
        to_racing(&mut run);
        while run.outcome().is_none() {
            if matches!(run.game.phase, Phase::Racing { .. }) {
                run.game.cars[1].body.pos = Vec2::new(TABLE_HALF.x + 1.0, 0.0);
                run.game.cars[2].body.pos = Vec2::new(TABLE_HALF.x + 1.0, 0.0);
            }
            run.step(NONE);
        }
        assert_eq!(run.game.points[0], WIN_POINTS);
        assert_eq!(run.game.round, WIN_POINTS as u8 - 1);
        assert_eq!(
            run.outcome().unwrap().placement,
            [Some(1), Some(2), Some(2), None]
        );
    }

    #[test]
    fn bots_finish_every_format() {
        use sim_core::registry::play_with_bots;
        let entry = sim_core::MinigameEntry::of::<TabletopRally>();
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
        sim_check::check_all::<TabletopRally>(8).unwrap();
    }
}
