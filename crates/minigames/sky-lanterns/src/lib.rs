//! Sky Lanterns: a shooting gallery. Unlit paper lanterns drift up the night sky. Every player
//! moves a sight with the stick, and A fires a spark that lights the frontmost unlit lantern under
//! it: a point for the shooter (small golden lanterns are worth [`GOLDEN_POINTS`]). A spark that
//! hits nothing makes the sight fizzle for a moment. Most points when time runs out wins.
//!
//! In 1v3 only the solo has a sight. The other three each steer a lantern of their own (B
//! dashes) and hide behind the ordinary lanterns, which block sparks. Every spark that lights one
//! of the three scores for the solo, who wins with [`HITS_TO_WIN`] before time runs out.
//!
//! The sky is a rectangle centred on the origin with `x` to the right and `y` up. Lanterns are
//! spawned and removed inside the simulation, each with an id that never repeats.

use std::hash::{Hash, Hasher};

use sim_core::hash::hash_f32;
use sim_core::math;
use sim_core::physics::{Body, DT, Vec2};
use sim_core::{
    Button, Difficulty, Format, FormatKind, Input, Inputs, MAX_PLAYERS, Minigame, Outcome,
    PlayerSetup, Rng, Seat,
};

/// The opening countdown, the same in every minigame.
pub const INTRO_TICKS: u16 = sim_core::COUNTDOWN_TICKS;
pub const PLAY_TICKS: u32 = 40 * 60;
/// After the result is decided, the game holds this long so the last spark plays out.
pub const DECIDED_TICKS: u16 = 60;
/// Half the sky's width and height. Lanterns rise from below the bottom edge (the horizon).
pub const SKY: Vec2 = Vec2::new(8.0, 4.5);

/// A sight's speed with the stick all the way over. Smaller deflections are slower still (the
/// speed goes with the square of the deflection), for fine aim.
pub const SIGHT_SPEED: f32 = 12.0;
/// After a spark, the sight can fire again this much later...
pub const SHOT_COOLDOWN: u16 = 12;
/// ...or this much later when the spark hit nothing.
pub const FIZZLE_TICKS: u16 = 40;

pub const LANTERN_RADIUS: f32 = 0.5;
pub const GOLDEN_RADIUS: f32 = 0.32;
/// How fast lanterns rise, slowest to fastest, in units a second.
pub const RISE_SPEED: (f32, f32) = (0.9, 1.5);
pub const GOLDEN_RISE_SPEED: (f32, f32) = (2.2, 2.8);
/// A lit lantern floats off this fast.
pub const LIT_RISE_SPEED: f32 = 2.4;
/// Lanterns sway from side to side at up to this speed, once every [`SWAY_TICKS`].
pub const SWAY_SPEED: f32 = 0.6;
pub const SWAY_TICKS: u16 = 180;
pub const GOLDEN_PERCENT: u32 = 12;
pub const GOLDEN_POINTS: u32 = 3;
/// Below this many unlit lanterns a new one rises at once; at the most, none do.
pub const MIN_UNLIT: usize = 4;
pub const MAX_UNLIT: usize = 9;
pub const SPAWN_INTERVAL: (u16, u16) = (18, 48);
/// Lanterns in the sky when play starts.
pub const INITIAL_LANTERNS: usize = 5;

/// 1v3: the three's lanterns.
pub const FLYER_RADIUS: f32 = 0.45;
pub const FLYER_SPEED: f32 = 6.0;
pub const FLYER_ACCEL: f32 = 40.0;
pub const FLYER_DRAG: f32 = 4.0;
pub const DASH_SPEED: f32 = 13.0;
pub const DASH_TICKS: u16 = 8;
pub const DASH_COOLDOWN: u16 = 90;
/// A lit flyer flashes this long, and can't be hit again until it's dark.
pub const FLASH_TICKS: u16 = 90;
/// Hits the solo needs to win.
pub const HITS_TO_WIN: u32 = 23;

pub struct SkyLanterns;

/// Who lit a lantern, and when. Sparks on the same tick all count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Lit {
    pub by: [bool; MAX_PLAYERS],
    pub tick: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lantern {
    /// Never reused, so a renderer can match its own entities to lanterns.
    pub id: u32,
    pub pos: Vec2,
    pub radius: f32,
    /// Units a second, while unlit.
    pub rise: f32,
    /// Sideways speed at the height of the sway.
    pub sway: f32,
    /// Front (0) to back (255): a spark lights the frontmost lantern under the sight.
    pub depth: u8,
    pub points: u32,
    /// Ticks in the sky.
    pub age: u16,
    pub lit: Option<Lit>,
}

impl Hash for Lantern {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.pos.hash(state);
        hash_f32(self.radius, state);
        hash_f32(self.rise, state);
        hash_f32(self.sway, state);
        self.depth.hash(state);
        self.points.hash(state);
        self.age.hash(state);
        self.lit.hash(state);
    }
}

impl Lantern {
    pub fn is_golden(&self) -> bool {
        self.points > 1
    }

    /// Where it's heading, in units a second.
    pub fn velocity(&self) -> Vec2 {
        if self.lit.is_some() {
            return Vec2::new(0.0, LIT_RISE_SPEED);
        }
        let turn = (self.age as u32).wrapping_add(self.id.wrapping_mul(37)) % SWAY_TICKS as u32;
        let angle = turn as f32 / SWAY_TICKS as f32 * std::f32::consts::TAU;
        Vec2::new(self.sway * math::sin(angle), self.rise)
    }

    fn contains(&self, point: Vec2) -> bool {
        (point - self.pos).length_squared() <= self.radius * self.radius
    }

    /// Fully in sight above the horizon and not yet off the top.
    pub fn in_sky(&self) -> bool {
        self.pos.y - self.radius > -SKY.y && self.pos.y + self.radius < SKY.y
    }
}

/// What a spark did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shot {
    /// Lit the lantern with this id.
    Lit(u32),
    /// Lit this seat's flyer (1v3).
    Flyer(Seat),
    Miss,
}

#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct Sight {
    pub pos: Vec2,
    /// Ticks until it can fire again.
    pub cooldown: u16,
    /// The last spark and the tick it was fired on.
    pub shot: Option<(u32, Shot)>,
    pub score: u32,
}

/// 1v3: one of the three's lanterns.
#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct Flyer {
    pub seat: Seat,
    pub body: Body,
    /// Ticks left in a dash, and until the next one.
    pub dash: u16,
    pub dash_cooldown: u16,
    /// Ticks left lit after a hit.
    pub flash: u16,
    pub hits: u32,
}

impl Flyer {
    pub fn can_be_hit(&self) -> bool {
        self.flash == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Intro {
        t: u16,
    },
    Playing,
    /// The result is known; the last spark plays out.
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
    pub rng: Rng,
    pub next_id: u32,
    /// Ticks until the next lantern rises.
    pub spawn_in: u16,
    pub lanterns: Vec<Lantern>,
    /// Indexed by seat: whoever shoots this time.
    pub sights: [Option<Sight>; MAX_PLAYERS],
    /// 1v3 only: the three's lanterns, in seat order.
    pub flyers: Vec<Flyer>,
    pub outcome: Option<Outcome>,
}

impl State {
    pub fn is_one_vs_three(&self) -> bool {
        matches!(self.format, Format::OneVsThree { .. })
    }

    pub fn ticks_left(&self) -> u32 {
        PLAY_TICKS.saturating_sub(self.tick)
    }

    pub fn flyer(&self, seat: Seat) -> Option<&Flyer> {
        self.flyers.iter().find(|f| f.seat == seat)
    }

    /// The solo's hits on the three (1v3).
    pub fn solo_hits(&self) -> u32 {
        self.flyers.iter().map(|f| f.hits).sum()
    }

    /// Points per team ([`Format::team`]). In 1v3: the solo's hits, and 0 for the three.
    pub fn team_scores(&self) -> [u32; MAX_PLAYERS] {
        let mut scores = [0; MAX_PLAYERS];
        for seat in self.players.seats() {
            if let (Some(team), Some(sight)) = (self.format.team(seat), self.sights[seat.index()]) {
                scores[team as usize] += sight.score;
            }
        }
        scores
    }

    fn spawn(&mut self, y: Option<f32>) {
        let rng = &mut self.rng;
        let one_vs_three = matches!(self.format, Format::OneVsThree { .. });
        let golden = !one_vs_three && rng.chance(GOLDEN_PERCENT);
        let unit = |rng: &mut Rng| rng.range_u32(0, 1000) as f32 / 1000.0;
        let (radius, (slow, fast), points) = if golden {
            (GOLDEN_RADIUS, GOLDEN_RISE_SPEED, GOLDEN_POINTS)
        } else {
            (LANTERN_RADIUS, RISE_SPEED, 1)
        };
        // With rival sights, a little more often towards the middle, where sights have rivals on
        // both sides (evenly, the sights at the ends won more).
        let across = if one_vs_three {
            unit(rng)
        } else {
            (unit(rng) + unit(rng) + unit(rng) * 6.0) / 8.0
        };
        let x = (across * 2.0 - 1.0) * (SKY.x - 1.0);
        let y = y.unwrap_or(-SKY.y - radius);
        let rise = slow + (fast - slow) * unit(rng);
        let sway = SWAY_SPEED * (unit(rng) * 2.0 - 1.0);
        let depth = rng.range_u32(0, 255) as u8;
        self.lanterns.push(Lantern {
            id: self.next_id,
            pos: Vec2::new(x, y),
            radius,
            rise,
            sway,
            depth,
            points,
            age: 0,
            lit: None,
        });
        self.next_id += 1;
    }

    fn move_lanterns(&mut self) {
        for lantern in &mut self.lanterns {
            lantern.pos += lantern.velocity() * DT;
            lantern.age = lantern.age.saturating_add(1);
        }
        self.lanterns.retain(|l| l.pos.y - l.radius < SKY.y + 0.5);
        let unlit = self.lanterns.iter().filter(|l| l.lit.is_none()).count();
        self.spawn_in = self.spawn_in.saturating_sub(1);
        if unlit < MIN_UNLIT || (self.spawn_in == 0 && unlit < MAX_UNLIT) {
            self.spawn(None);
            self.spawn_in = self
                .rng
                .range_u32(SPAWN_INTERVAL.0 as u32, SPAWN_INTERVAL.1 as u32)
                as u16;
        }
    }

    fn move_sights(&mut self, inputs: &Inputs) {
        for seat in Seat::ALL {
            let Some(sight) = &mut self.sights[seat.index()] else {
                continue;
            };
            let stick = stick(&inputs.current[seat.index()]);
            sight.pos += stick * (stick.length() * SIGHT_SPEED * DT);
            sight.pos.x = sight.pos.x.clamp(-SKY.x, SKY.x);
            sight.pos.y = sight.pos.y.clamp(-SKY.y, SKY.y);
            sight.cooldown = sight.cooldown.saturating_sub(1);
        }
    }

    fn move_flyers(&mut self, inputs: &Inputs) {
        for flyer in &mut self.flyers {
            let input = &inputs.current[flyer.seat.index()];
            let stick = stick(input);
            flyer.flash = flyer.flash.saturating_sub(1);
            flyer.dash_cooldown = flyer.dash_cooldown.saturating_sub(1);
            if inputs.just_pressed(flyer.seat, Button::B) && flyer.dash_cooldown == 0 {
                let direction = if stick.length() > 0.0 {
                    stick.normalize_or_zero()
                } else {
                    flyer.body.vel.normalize_or_zero()
                };
                if direction != Vec2::ZERO {
                    flyer.body.vel = direction * DASH_SPEED;
                    flyer.dash = DASH_TICKS;
                    flyer.dash_cooldown = DASH_COOLDOWN;
                }
            }
            if flyer.dash > 0 {
                flyer.dash -= 1;
                flyer.body.pos += flyer.body.vel * DT;
            } else {
                flyer.body.steer(stick, FLYER_SPEED, FLYER_ACCEL);
                flyer.body.integrate(FLYER_DRAG);
            }
            flyer.body.bounce_in_rect(SKY, 0.0);
        }
    }

    /// What a spark at `point` would hit: the frontmost unlit lantern, else a flyer that can be
    /// hit (the three's lanterns fly behind the others).
    pub fn target_at(&self, point: Vec2) -> Shot {
        let lantern = self
            .lanterns
            .iter()
            .filter(|l| l.lit.is_none() && l.contains(point))
            .min_by_key(|l| (l.depth, l.id));
        if let Some(lantern) = lantern {
            return Shot::Lit(lantern.id);
        }
        self.flyers
            .iter()
            .find(|f| f.can_be_hit() && (point - f.body.pos).length() <= f.body.radius)
            .map_or(Shot::Miss, |f| Shot::Flyer(f.seat))
    }

    /// Every sight that fires this tick picks its target first, then they all land, so sparks
    /// on the same tick are equal whatever the seat order.
    fn fire(&mut self, inputs: &Inputs) {
        let mut shots = [None; MAX_PLAYERS];
        for seat in Seat::ALL {
            if let Some(sight) = self.sights[seat.index()]
                && sight.cooldown == 0
                && inputs.just_pressed(seat, Button::A)
            {
                shots[seat.index()] = Some(self.target_at(sight.pos));
            }
        }
        let one_vs_three = self.is_one_vs_three();
        for seat in Seat::ALL {
            let Some(shot) = shots[seat.index()] else {
                continue;
            };
            let mut points = 0;
            match shot {
                Shot::Lit(id) => {
                    let lantern = self.lanterns.iter_mut().find(|l| l.id == id).unwrap();
                    let lit = lantern.lit.get_or_insert(Lit {
                        by: [false; MAX_PLAYERS],
                        tick: self.tick,
                    });
                    lit.by[seat.index()] = true;
                    if !one_vs_three {
                        points = lantern.points;
                    }
                }
                Shot::Flyer(target) => {
                    let flyer = self.flyers.iter_mut().find(|f| f.seat == target).unwrap();
                    flyer.hits += 1;
                    flyer.flash = FLASH_TICKS;
                    points = 1;
                }
                Shot::Miss => {}
            }
            let sight = self.sights[seat.index()].as_mut().unwrap();
            sight.score += points;
            sight.shot = Some((self.tick, shot));
            // In 1v3 a spark spent on cover is as good as a miss.
            let wasted = shot == Shot::Miss || (one_vs_three && matches!(shot, Shot::Lit(_)));
            sight.cooldown = if wasted { FIZZLE_TICKS } else { SHOT_COOLDOWN };
        }
    }

    fn decide_if_over(&mut self) {
        if self.outcome.is_some() {
            return;
        }
        let outcome = if self.is_one_vs_three() {
            let solo_won = self.solo_hits() >= HITS_TO_WIN;
            if !solo_won && self.tick < PLAY_TICKS {
                return;
            }
            let mut scores = [0; MAX_PLAYERS];
            scores[if solo_won { 0 } else { 1 }] = 1;
            Outcome::from_team_scores(&self.players, &self.format, &scores)
        } else {
            if self.tick < PLAY_TICKS {
                return;
            }
            Outcome::from_team_scores(&self.players, &self.format, &self.team_scores())
        };
        self.outcome = Some(outcome);
        self.phase = Phase::Decided { t: 0 };
    }
}

/// The stick as a direction in the sky (up is up), length 0..=1.
fn stick(input: &Input) -> Vec2 {
    let v = Vec2::from_stick(input);
    Vec2::new(v.x, -v.y)
}

/// Where sights start: each in the middle of an equal share of the sky lanterns rise in, so no
/// seat starts with more sky to itself than the others.
fn start_x(index: usize, count: usize) -> f32 {
    let share = 2.0 * (SKY.x - 1.0) / count as f32;
    (index as f32 - (count as f32 - 1.0) / 2.0) * share
}

impl Minigame for SkyLanterns {
    type State = State;

    const ID: &'static str = "sky-lanterns";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[
        FormatKind::FreeForAll,
        FormatKind::OneVsThree,
        FormatKind::TwoVsTwo,
        FormatKind::Duel,
    ];
    const MAX_TICKS: u32 = INTRO_TICKS as u32 + PLAY_TICKS + DECIDED_TICKS as u32 + 60;

    fn new(seed: u64, players: PlayerSetup, format: Format) -> State {
        let mut sights = [None; MAX_PLAYERS];
        let mut flyers = Vec::new();
        // Seats outside a duel have no team and sit this one out.
        let playing: Vec<Seat> = players
            .seats()
            .filter(|s| format.team(*s).is_some())
            .collect();
        let shooters: Vec<Seat> = match format {
            Format::OneVsThree { solo } => vec![solo],
            _ => playing.clone(),
        };
        for (index, seat) in shooters.iter().enumerate() {
            sights[seat.index()] = Some(Sight {
                pos: Vec2::new(start_x(index, shooters.len()), 0.0),
                cooldown: 0,
                shot: None,
                score: 0,
            });
        }
        if let Format::OneVsThree { solo } = format {
            let three: Vec<Seat> = playing.into_iter().filter(|s| *s != solo).collect();
            for (index, seat) in three.iter().enumerate() {
                flyers.push(Flyer {
                    seat: *seat,
                    body: Body::new(
                        Vec2::new(start_x(index, three.len()), SKY.y * 0.5),
                        FLYER_RADIUS,
                        1.0,
                    ),
                    dash: 0,
                    dash_cooldown: 0,
                    flash: 0,
                    hits: 0,
                });
            }
        }
        let mut state = State {
            seed,
            players,
            format,
            phase: Phase::Intro { t: 0 },
            tick: 0,
            rng: Rng::new(seed),
            next_id: 0,
            spawn_in: 0,
            lanterns: Vec::new(),
            sights,
            flyers,
            outcome: None,
        };
        for index in 0..INITIAL_LANTERNS {
            let y = -SKY.y + 1.0 + index as f32 * (SKY.y * 1.2 / INITIAL_LANTERNS as f32);
            state.spawn(Some(y));
        }
        state.spawn_in = SPAWN_INTERVAL.0;
        state
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
                state.move_lanterns();
                state.move_sights(inputs);
                state.move_flyers(inputs);
                state.fire(inputs);
                state.decide_if_over();
            }
            Phase::Decided { t } => {
                // The sky drifts on; nobody acts.
                state.move_lanterns();
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
        if state.phase != Phase::Playing || !state.players.is_active(seat) {
            return Input::default();
        }
        if let Some(sight) = &state.sights[seat.index()] {
            return aim(state, seat, sight, Skill::of(difficulty));
        }
        if let Some(flyer) = state.flyer(seat) {
            return fly(state, flyer, Skill::of(difficulty));
        }
        Input::default()
    }
}

/// How well a bot plays.
#[derive(Debug, Clone, Copy)]
struct Skill {
    /// A new lantern has to be in the sky this long before the bot goes for it.
    reaction: u16,
    /// How far the stick goes over at most.
    max_stick: f32,
    /// The aim wanders up to this far off the target.
    error: f32,
    /// How many seconds ahead of a moving target the bot aims.
    lead: f32,
    /// 1v3: how far the three's bots push the stick, and whether they dash.
    fly_stick: f32,
    dashes: bool,
}

impl Skill {
    fn of(difficulty: Difficulty) -> Self {
        match difficulty {
            Difficulty::Easy => Self {
                reaction: 30,
                max_stick: 0.8,
                error: 0.55,
                lead: 0.0,
                fly_stick: 0.55,
                dashes: false,
            },
            Difficulty::Normal => Self {
                reaction: 20,
                max_stick: 0.9,
                error: 0.3,
                lead: 0.05,
                fly_stick: 0.9,
                dashes: true,
            },
            Difficulty::Hard => Self {
                reaction: 14,
                max_stick: 0.95,
                error: 0.22,
                lead: 0.08,
                fly_stick: 0.95,
                dashes: true,
            },
        }
    }
}

/// A point in the unit disc that changes every `period` ticks, the same for the same state.
fn wander(state: &State, seat: Seat, salt: u64, period: u32) -> Vec2 {
    let mut rng = Rng::new(
        state.seed
            ^ (seat.0 as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ ((state.tick / period + 1) as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9)
            ^ salt,
    );
    let mut unit = || rng.range_u32(0, 2000) as f32 / 1000.0 - 1.0;
    Vec2::new(unit(), unit()).clamp_length(1.0)
}

/// A stick input pointing along `direction` (length 0..=1).
fn stick_input(direction: Vec2) -> Input {
    Input {
        buttons: 0,
        stick_x: Input::quantize_axis(direction.x),
        stick_y: Input::quantize_axis(direction.y),
    }
}

/// A shooter: pick a lantern (or in 1v3 a flyer), move the sight onto it and fire.
fn aim(state: &State, seat: Seat, sight: &Sight, skill: Skill) -> Input {
    let target = if state.is_one_vs_three() {
        // The flyer closest to the sight; one still lit only if there's nobody else.
        state
            .flyers
            .iter()
            .min_by(|a, b| {
                let cost = |f: &Flyer| {
                    let waiting = f.flash.saturating_sub(skill.reaction) as f32 * 0.1;
                    (f.body.pos - sight.pos).length() + waiting
                };
                cost(a).total_cmp(&cost(b))
            })
            .map(|f| (f.body.pos, f.body.vel, f.body.radius, f.can_be_hit()))
    } else {
        state
            .lanterns
            .iter()
            .filter(|l| l.lit.is_none() && l.age >= skill.reaction && l.in_sky())
            .min_by(|a, b| {
                // Leave a lantern to a rival whose sight is much nearer, rather than race them
                // for it.
                let cost = |l: &Lantern| {
                    let mine = (l.pos - sight.pos).length();
                    let near_top = if l.pos.y > SKY.y - 1.2 { 3.0 } else { 0.0 };
                    let contested = state
                        .sights
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *i != seat.index())
                        .flat_map(|(_, s)| s)
                        .any(|s| (l.pos - s.pos).length() < mine * 0.6);
                    let contested = if contested { 2.5 } else { 0.0 };
                    mine / (1.0 + 0.4 * (l.points - 1) as f32) + near_top + contested
                };
                cost(a).total_cmp(&cost(b))
            })
            .map(|l| (l.pos, l.velocity(), l.radius, true))
    };
    let Some((pos, vel, radius, can_hit)) = target else {
        return Input::default();
    };
    let aim_at = pos + vel * skill.lead + wander(state, seat, 0, 24) * (skill.error * radius * 2.0);
    let to = aim_at - sight.pos;
    let distance = to.length();
    // Keep up with the target and close in on it, slowing down near it. The sight's speed goes
    // with the square of the stick.
    let top_speed = SIGHT_SPEED * skill.max_stick * skill.max_stick;
    let wanted = (vel + to * 10.0).clamp_length(top_speed);
    let speed = wanted.length();
    let mut input = stick_input(wanted.normalize_or_zero() * (speed / SIGHT_SPEED).sqrt());
    // A press always fires and starts the cooldown, which releases A again: the next press is a
    // fresh one. (Pressing only on alternate ticks gave the seats firing on odd ticks an edge.)
    if can_hit && sight.cooldown == 0 && distance < 0.12 {
        input.set(Button::A, true);
    }
    input
}

/// One of the three in 1v3: keep a lantern between yourself and the sight, and dash when it
/// closes in.
fn fly(state: &State, flyer: &Flyer, skill: Skill) -> Input {
    let Some(sight) = state.sights.iter().flatten().next() else {
        return Input::default();
    };
    let me = flyer.body.pos;
    let from_sight = me - sight.pos;
    let danger = from_sight.length();
    let away = from_sight.normalize_or_zero();
    // Away from the sight while it's close, and in from the edges.
    let mut want = if danger < 4.0 {
        away * ((4.0 - danger) / 4.0)
    } else {
        Vec2::ZERO
    };
    let edge = |p: f32, half: f32| {
        if p > half - 1.0 {
            -(p - half + 1.0)
        } else if p < -half + 1.0 {
            -half + 1.0 - p
        } else {
            0.0
        }
    };
    want += Vec2::new(edge(me.x, SKY.x), edge(me.y, SKY.y));
    let cover = state
        .lanterns
        .iter()
        .filter(|l| l.lit.is_none() && l.pos.y < SKY.y - 1.5 && l.pos.y > -SKY.y)
        .min_by(|a, b| {
            let cost = |l: &Lantern| (l.pos - me).length() - (l.pos - sight.pos).length() * 0.3;
            cost(a).total_cmp(&cost(b))
        });
    if let Some(cover) = cover {
        let to_cover = cover.pos - me;
        if to_cover.length() < 0.15 {
            // Hidden: stay with it.
            want = cover.velocity() * (1.0 / FLYER_SPEED);
        } else {
            want = want * 0.5 + to_cover.normalize_or_zero();
        }
    } else {
        // Drift about rather than sit still.
        want += wander(state, flyer.seat, 7, 90) * 0.5;
    }
    let mut input = stick_input(want.clamp_length(skill.fly_stick));
    let covered = state.target_at(me).ne(&Shot::Flyer(flyer.seat));
    if skill.dashes && danger < 1.2 && !covered && flyer.can_be_hit() && flyer.dash_cooldown == 0 {
        input = stick_input(away);
        input.set(Button::B, true);
    }
    input
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

    fn playing(players: usize, format: Format) -> Run<SkyLanterns> {
        let mut run: Run<SkyLanterns> =
            Run::new(3, PlayerSetup::first(players), format, RunConfig::default());
        while run.game.phase != Phase::Playing {
            run.step(NONE);
        }
        run
    }

    fn fire(run: &mut Run<SkyLanterns>, seats: &[usize]) {
        let mut inputs = NONE;
        for seat in seats {
            inputs[*seat] = Input::default().with(Button::A);
        }
        run.step(inputs);
    }

    /// Clears the sky and puts one still, unlit lantern at `pos`.
    fn only_lantern(run: &mut Run<SkyLanterns>, pos: Vec2, points: u32, depth: u8) -> u32 {
        let id = run.game.next_id;
        run.game.next_id += 1;
        run.game.lanterns.push(Lantern {
            id,
            pos,
            radius: LANTERN_RADIUS,
            rise: 0.0,
            sway: 0.0,
            depth,
            points,
            age: 0,
            lit: None,
        });
        id
    }

    fn clear_sky(run: &mut Run<SkyLanterns>) {
        run.game.lanterns.clear();
        // Only the fewest new ones rise, from below the horizon, well away from the tests' aims.
        run.game.spawn_in = u16::MAX;
    }

    fn aim_at(run: &mut Run<SkyLanterns>, seat: usize, pos: Vec2) {
        run.game.sights[seat].as_mut().unwrap().pos = pos;
    }

    #[test]
    fn the_stick_moves_the_sight_and_the_sky_holds_it() {
        let mut run = playing(2, Format::FreeForAll);
        let start = run.game.sights[0].unwrap().pos;
        let mut inputs = NONE;
        inputs[0].stick_x = 127;
        inputs[0].stick_y = 127;
        run.step(inputs);
        let moved = run.game.sights[0].unwrap().pos;
        assert!(moved.x > start.x && moved.y > start.y, "up is up");
        assert!((moved - start).length() <= SIGHT_SPEED * DT + 1e-4);
        for _ in 0..200 {
            run.step(inputs);
        }
        assert_eq!(run.game.sights[0].unwrap().pos, SKY);
    }

    #[test]
    fn a_spark_lights_the_frontmost_lantern_under_the_sight() {
        let mut run = playing(2, Format::FreeForAll);
        clear_sky(&mut run);
        let back = only_lantern(&mut run, Vec2::new(1.0, 1.0), 1, 200);
        let front = only_lantern(&mut run, Vec2::new(1.3, 1.0), 1, 10);
        aim_at(&mut run, 0, Vec2::new(1.15, 1.0));
        fire(&mut run, &[0]);
        let lit =
            |run: &Run<SkyLanterns>, id| run.game.lanterns.iter().find(|l| l.id == id).unwrap().lit;
        assert!(lit(&run, front).is_some());
        assert!(lit(&run, back).is_none());
        let sight = run.game.sights[0].unwrap();
        assert_eq!(sight.score, 1);
        assert_eq!(sight.shot.unwrap().1, Shot::Lit(front));
        assert_eq!(sight.cooldown, SHOT_COOLDOWN);
    }

    #[test]
    fn golden_lanterns_are_worth_more() {
        let mut run = playing(2, Format::FreeForAll);
        clear_sky(&mut run);
        only_lantern(&mut run, Vec2::new(0.0, 0.0), GOLDEN_POINTS, 0);
        aim_at(&mut run, 1, Vec2::new(0.0, 0.0));
        fire(&mut run, &[1]);
        assert_eq!(run.game.sights[1].unwrap().score, GOLDEN_POINTS);
    }

    #[test]
    fn a_miss_fizzles_and_holding_a_fires_once() {
        let mut run = playing(2, Format::FreeForAll);
        clear_sky(&mut run);
        aim_at(&mut run, 0, Vec2::new(-5.0, 3.0));
        fire(&mut run, &[0]);
        let sight = run.game.sights[0].unwrap();
        assert_eq!(sight.shot.unwrap().1, Shot::Miss);
        assert_eq!(sight.cooldown, FIZZLE_TICKS);
        only_lantern(&mut run, Vec2::new(-5.0, 3.0), 1, 0);
        // Held through the fizzle: no second spark.
        for _ in 0..FIZZLE_TICKS + 5 {
            fire(&mut run, &[0]);
        }
        assert_eq!(run.game.sights[0].unwrap().score, 0);
        run.step(NONE);
        fire(&mut run, &[0]);
        assert_eq!(run.game.sights[0].unwrap().score, 1);
    }

    #[test]
    fn sparks_on_the_same_tick_all_score() {
        let mut run = playing(4, Format::FreeForAll);
        clear_sky(&mut run);
        let id = only_lantern(&mut run, Vec2::new(2.0, 0.0), 1, 0);
        for seat in 0..4 {
            aim_at(&mut run, seat, Vec2::new(2.0, 0.0));
        }
        fire(&mut run, &[1, 3]);
        let lantern = run.game.lanterns.iter().find(|l| l.id == id).unwrap();
        assert_eq!(lantern.lit.unwrap().by, [false, true, false, true]);
        let scores: Vec<u32> = run.game.sights.iter().map(|s| s.unwrap().score).collect();
        assert_eq!(scores, [0, 1, 0, 1]);
        // Lit lanterns can't be lit again.
        run.step(NONE);
        fire(&mut run, &[0]);
        assert_eq!(run.game.sights[0].unwrap().shot.unwrap().1, Shot::Miss);
    }

    #[test]
    fn lanterns_rise_off_the_top_and_new_ones_come_up() {
        let mut run = playing(2, Format::FreeForAll);
        let first: Vec<u32> = run.game.lanterns.iter().map(|l| l.id).collect();
        for _ in 0..20 * 60 {
            run.step(NONE);
        }
        assert!(run.game.lanterns.iter().all(|l| !first.contains(&l.id)));
        let unlit = run.game.lanterns.iter().filter(|l| l.lit.is_none()).count();
        assert!((MIN_UNLIT..=MAX_UNLIT).contains(&unlit), "{unlit}");
        let ids: Vec<u32> = run.game.lanterns.iter().map(|l| l.id).collect();
        assert!(ids.windows(2).all(|w| w[0] < w[1]), "ids never repeat");
    }

    #[test]
    fn most_points_wins_and_teams_add_up() {
        let format = Format::TwoVsTwo {
            team_of: [0, 1, 1, 0],
        };
        let mut run = playing(4, format);
        run.game.sights[0].as_mut().unwrap().score = 3;
        run.game.sights[1].as_mut().unwrap().score = 4;
        run.game.sights[3].as_mut().unwrap().score = 2;
        while run.outcome().is_none() {
            run.step(NONE);
        }
        assert_eq!(run.game.tick, PLAY_TICKS);
        let outcome = run.outcome().unwrap();
        assert_eq!(outcome.placement, [Some(1), Some(2), Some(2), Some(1)]);
    }

    #[test]
    fn in_1v3_only_the_solo_shoots_and_the_three_fly() {
        let mut run = playing(4, Format::OneVsThree { solo: Seat(2) });
        let sights: Vec<bool> = run.game.sights.iter().map(Option::is_some).collect();
        assert_eq!(sights, [false, false, true, false]);
        let seats: Vec<Seat> = run.game.flyers.iter().map(|f| f.seat).collect();
        assert_eq!(seats, [Seat(0), Seat(1), Seat(3)]);
        let start = run.game.flyers[0].body.pos;
        let mut inputs = NONE;
        inputs[0].stick_x = 127;
        for _ in 0..30 {
            run.step(inputs);
        }
        assert!(run.game.flyers[0].body.pos.x > start.x + 1.0);
        // A dash goes much further.
        let before = run.game.flyers[0].body.pos.x;
        inputs[0] = inputs[0].with(Button::B);
        for _ in 0..DASH_TICKS {
            run.step(inputs);
        }
        let dashed = run.game.flyers[0].body.pos.x - before;
        assert!(
            dashed > FLYER_SPEED * DASH_TICKS as f32 * DT * 2.0,
            "{dashed}"
        );
        assert_eq!(
            run.game.flyers[0].dash_cooldown,
            DASH_COOLDOWN - (DASH_TICKS - 1)
        );
    }

    #[test]
    fn lanterns_shield_the_three_and_a_hit_scores_for_the_solo() {
        let mut run = playing(4, Format::OneVsThree { solo: Seat(0) });
        clear_sky(&mut run);
        let at = Vec2::new(-2.0, 1.0);
        run.game.flyers[0].body.pos = at;
        let cover = only_lantern(&mut run, at, 1, 255);
        aim_at(&mut run, 0, at);
        fire(&mut run, &[0]);
        assert_eq!(
            run.game.sights[0].unwrap().shot.unwrap().1,
            Shot::Lit(cover)
        );
        assert_eq!(
            run.game.sights[0].unwrap().score,
            0,
            "cover isn't worth points"
        );
        assert_eq!(run.game.solo_hits(), 0);
        assert_eq!(
            run.game.sights[0].unwrap().cooldown,
            FIZZLE_TICKS,
            "a spark spent on cover fizzles"
        );
        for _ in 0..FIZZLE_TICKS {
            run.step(NONE);
        }
        let flyer = run.game.flyers[0];
        aim_at(&mut run, 0, flyer.body.pos);
        fire(&mut run, &[0]);
        assert_eq!(
            run.game.sights[0].unwrap().shot.unwrap().1,
            Shot::Flyer(flyer.seat)
        );
        assert_eq!(run.game.solo_hits(), 1);
        assert!(!run.game.flyers[0].can_be_hit());
        // Lit, it can't be hit again until it's dark.
        for _ in 0..SHOT_COOLDOWN {
            run.step(NONE);
        }
        let at = run.game.flyers[0].body.pos;
        aim_at(&mut run, 0, at);
        fire(&mut run, &[0]);
        assert_eq!(run.game.solo_hits(), 1);
    }

    #[test]
    fn the_solo_wins_with_enough_hits() {
        let mut run = playing(4, Format::OneVsThree { solo: Seat(1) });
        run.game.flyers[2].hits = HITS_TO_WIN - 1;
        run.game.flyers[2].flash = 0;
        clear_sky(&mut run);
        let at = run.game.flyers[2].body.pos;
        aim_at(&mut run, 1, at);
        fire(&mut run, &[1]);
        assert!(run.game.outcome.is_some());
        while run.outcome().is_none() {
            run.step(NONE);
        }
        let winners: Vec<Seat> = run.outcome().unwrap().winners().collect();
        assert_eq!(winners, [Seat(1)]);
    }

    #[test]
    fn the_three_win_when_time_runs_out() {
        let mut run = playing(4, Format::OneVsThree { solo: Seat(1) });
        run.game.flyers[0].hits = HITS_TO_WIN - 1;
        while run.outcome().is_none() {
            run.step(NONE);
        }
        let winners: Vec<Seat> = run.outcome().unwrap().winners().collect();
        assert_eq!(winners, [Seat(0), Seat(2), Seat(3)]);
    }

    #[test]
    fn seats_outside_a_duel_sit_out() {
        let run = playing(
            4,
            Format::Duel {
                a: Seat(0),
                b: Seat(3),
            },
        );
        let sights: Vec<bool> = run.game.sights.iter().map(Option::is_some).collect();
        assert_eq!(sights, [true, false, false, true]);
        assert!(run.game.flyers.is_empty());
    }

    #[test]
    fn nothing_fires_during_the_countdown() {
        let mut run: Run<SkyLanterns> = Run::new(
            1,
            PlayerSetup::first(2),
            Format::FreeForAll,
            RunConfig::default(),
        );
        for _ in 0..20 {
            fire(&mut run, &[0, 1]);
            run.step(NONE);
        }
        assert!(matches!(run.game.phase, Phase::Intro { .. }));
        assert!(run.game.sights.iter().flatten().all(|s| s.shot.is_none()));
    }

    #[test]
    fn harder_bots_score_more() {
        let score = |difficulty| {
            let mut total = 0;
            for seed in 0..10 {
                let mut run: Run<SkyLanterns> = Run::new(
                    seed,
                    PlayerSetup::first(1),
                    Format::FreeForAll,
                    RunConfig::default(),
                );
                while run.outcome().is_none() {
                    let inputs =
                        Seat::ALL.map(|s| SkyLanterns::bot_input(&run.game, s, difficulty));
                    run.step(inputs);
                }
                total += run.game.sights[0].unwrap().score;
            }
            total
        };
        let (easy, normal, hard) = (
            score(Difficulty::Easy),
            score(Difficulty::Normal),
            score(Difficulty::Hard),
        );
        assert!(easy < normal && normal < hard, "{easy} {normal} {hard}");
    }

    #[test]
    fn passes_all_automated_quality_bars() {
        sim_check::check_all::<SkyLanterns>(8).unwrap();
    }
}
