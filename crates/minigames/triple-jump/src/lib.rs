//! Triple Jump: the hop, step and jump from Track & Field. Everyone jumps at once, each in their
//! own lane, seen side on. Mashing A spins up the run-up speed; B before the take-off line plants
//! the foot, and holding it raises the angle until letting go launches the hop. At each touchdown
//! another B press (on time keeps the speed) and another angle launch the step and then the jump.
//! A jump's length grows with the speed and is best at 45°. Three attempts, best mark wins, the
//! second best breaks ties.
//!
//! Run-up speed comes from a heavy flywheel: every press adds a little to it ([`PUSH`]), presses
//! closer than [`MASH_GAP`] count only partly, and it drains by 1/[`DRAG_TICKS`] a tick; the
//! runner's speed follows it smoothly. So the speed at the line is mostly the last second or two
//! of mashing, and a steady mash all the way down the runway beats resting and a late burst.
//!
//! 2v2 (runner and coach): a team shares one athlete. The runner mashes and presses B at take-off
//! and each touchdown; the press starts the angle gauge sweeping, and the coach's A locks it. They
//! swap roles every attempt, over [`TEAM_ATTEMPTS`].
//!
//! 1v3 (the wind): the solo jumps once alone, in calm air, and its mark is the line. Then it's the
//! wind for the three's [`ATTEMPTS`]: it moves a fan between their lanes, mashing A spins up a
//! headwind on that lane (the same kind of flywheel) that slows a runner and shortens jumps in the
//! air, and B is a gust that cuts a jumper's speed. The three win if [`QUALIFIERS`] of them land
//! past the line; otherwise the wind does. The line comes from a jump as good as the players, so
//! the format stays fair however well everyone plays.
//!
//! Distances are whole micrometres along the lane from the take-off line (0), the runway behind it,
//! so the simulation has no floats except a flight's length in ticks.

use sim_core::{
    Button, Difficulty, Format, FormatKind, Input, Inputs, MAX_PLAYERS, Minigame, Outcome,
    PlayerSetup, Seat, math,
};

/// The opening countdown, the same in every minigame.
pub const INTRO_TICKS: u16 = sim_core::COUNTDOWN_TICKS;
/// Attempts each, in free-for-all, duel and (for the three, after the solo's) 1v3...
pub const ATTEMPTS: u8 = 3;
/// ...and a 2v2 team's, so each partner runs twice.
pub const TEAM_ATTEMPTS: u8 = 4;
pub const MAX_ATTEMPTS: usize = TEAM_ATTEMPTS as usize;

/// A millimetre, in the simulation's micrometres.
pub const MM: i32 = 1000;
/// The runway, from the start to the take-off line.
pub const RUNWAY: i32 = 22_000 * MM;
/// The speed (µm a tick) a steady mash at the fastest rate that counts settles at: 10 m/s.
pub const TOP_SPEED: i32 = 166_667;
/// Speed drains by 1/this every tick on the runway: the flywheel's time constant.
pub const DRAG_TICKS: i32 = 90;
/// Presses this many ticks apart (10 a second) count in full; closer ones count only partly.
pub const MASH_GAP: u16 = 6;
/// What a full press adds to the flywheel.
pub const PUSH: i32 = TOP_SPEED * MASH_GAP as i32 / DRAG_TICKS;
/// The runner's speed closes 1/this of the gap to the flywheel's each tick, so a press doesn't
/// jolt it and the speed at take-off doesn't depend on how recently you pressed.
pub const SMOOTH_TICKS: i32 = 8;
/// The hop's, step's and jump's lengths at [`TOP_SPEED`] and [`BEST_ANGLE`].
pub const JUMP_LENGTHS: [i32; 3] = [5_800 * MM, 4_600 * MM, 5_800 * MM];
/// Angles are in tenths of a degree.
pub const BEST_ANGLE: u16 = 450;
/// Holding the gauge this high launches by itself.
pub const MAX_ANGLE: u16 = 900;
/// A jump's length falls off with the square of how far its angle is from the best, to nothing
/// this far off...
pub const ANGLE_SPREAD: i32 = 250;
/// ...but never below this share (per mille).
pub const MIN_ANGLE_SHARE: i32 = 100;
/// The gauge climbs this much a tick while B is held (45° in half a second)...
pub const ANGLE_RATE: u16 = 15;
/// ...and in 2v2, where the coach has to react to the runner, this much (45° in 0.75 s).
pub const COACH_ANGLE_RATE: u16 = 10;
/// A jump at [`TOP_SPEED`] and 45° is in the air this long; faster and steeper is longer (with
/// the square root of the speed, so a slow jump doesn't flash by).
pub const FLIGHT_TICKS: f32 = 42.0;
pub const MIN_FLIGHT_TICKS: u16 = 10;
/// How high that jump goes (for the view).
pub const PEAK: i32 = 1_500 * MM;
/// A touchdown press counts up to this many ticks either side of the landing...
pub const WINDOW: u16 = 9;
/// ...keeping all the speed within this many...
pub const PERFECT: u16 = 2;
/// ...most of it within this many, and less outside (per mille).
pub const GOOD: u16 = 5;
pub const KEEP: [i32; 3] = [1000, 920, 820];
/// An attempt ends after this long, and anyone still on the runway has fouled.
pub const ATTEMPT_TICKS: u16 = 12 * 60;
/// Between attempts: the marks are shown this long...
pub const MARKS_TICKS: u16 = 75;
/// ...then everyone stands at the start this long.
pub const READY_TICKS: u16 = 60;
/// After the result is decided, the game holds this long before it ends.
pub const DECIDED_TICKS: u16 = 90;

/// 1v3: the three need this many of them past the solo's mark.
pub const QUALIFIERS: u8 = 2;
/// The wind's strength (per mille of full) drains by 1/this a tick...
pub const WIND_DRAG_TICKS: i32 = 60;
/// ...and a full press adds this, so mashing at the fastest rate holds it at full.
pub const WIND_PUSH: i32 = 1000 * MASH_GAP as i32 / WIND_DRAG_TICKS;
/// A full headwind drains this share (per mille) of a runner's flywheel each tick, on top of the
/// usual drag...
pub const HEADWIND_RUN: i32 = 2;
/// ...and takes this share (per mille) of a jumper's way forward in the air.
pub const HEADWIND_AIR: i32 = 150;
/// The fan takes this long to move to the next lane, and blows nothing on the way.
pub const FAN_MOVE_TICKS: u16 = 10;
/// The stick has to lean this far to move the fan.
pub const FAN_STICK: i8 = 64;
/// A gust takes this share (per mille) of a jumper's speed...
pub const GUST_CUT: i32 = 80;
/// ...pushes one in the air back this far...
pub const GUST_PUSH: i32 = 400 * MM;
/// ...and the next can come this much later.
pub const GUST_COOLDOWN: u16 = 4 * 60;

/// How much of a jump's length an angle keeps, per mille.
pub fn angle_share(angle: u16) -> i32 {
    let off = angle as i32 - BEST_ANGLE as i32;
    (1000 - 1000 * off * off / (ANGLE_SPREAD * ANGLE_SPREAD)).max(MIN_ANGLE_SHARE)
}

/// How far `jump` (0 hop, 1 step, 2 jump) goes at this speed and angle.
pub fn jump_length(jump: u8, speed: i32, angle: u16) -> i32 {
    (JUMP_LENGTHS[jump as usize] as i64 * speed as i64 / TOP_SPEED as i64
        * angle_share(angle) as i64
        / 1000) as i32
}

/// How long a jump at this speed and angle is in the air: longer the faster and steeper.
pub fn flight_ticks(speed: i32, angle: u16) -> u16 {
    let rise = math::sin(angle as f32 / 10.0 * std::f32::consts::PI / 180.0);
    let ticks = FLIGHT_TICKS * (speed as f32 / TOP_SPEED as f32).sqrt() * rise
        / std::f32::consts::FRAC_1_SQRT_2;
    (ticks.round() as u16).max(MIN_FLIGHT_TICKS)
}

/// How high a jump this long in the air goes, for the view.
pub fn peak_height(ticks: u16) -> i32 {
    (PEAK as i64 * ticks as i64 * ticks as i64 / (FLIGHT_TICKS * FLIGHT_TICKS) as i64) as i32
}

/// How a touchdown press this many ticks off the landing is graded.
pub fn grade(off: u16) -> Grade {
    if off <= PERFECT {
        Grade::Perfect
    } else if off <= GOOD {
        Grade::Good
    } else {
        Grade::Off
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Grade {
    Perfect,
    Good,
    Off,
}

impl Grade {
    /// The share of the speed kept, per mille.
    pub fn keep(self) -> i32 {
        KEEP[self as usize]
    }
}

/// The angle gauge: climbing from 0 until it's locked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Gauge {
    pub ticks: u16,
    pub locked: bool,
}

impl Gauge {
    pub fn angle(&self, rate: u16) -> u16 {
        (self.ticks * rate).min(MAX_ANGLE)
    }

    /// Solo, the gauge climbs while B is held and locks when it's let go; with a coach it climbs
    /// by itself until the coach presses A. Either way it locks at the top.
    fn step(mut self, held: bool, coach: Option<bool>, rate: u16) -> Self {
        if self.locked {
            return self;
        }
        let lock = match coach {
            Some(pressed) => pressed,
            None => !held,
        };
        if lock {
            self.locked = true;
        } else {
            self.ticks += 1;
            if self.ticks * rate >= MAX_ANGLE {
                self.locked = true;
            }
        }
        self
    }
}

/// What happens at the next touchdown, decided by a press while still in the air.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Next {
    /// No press yet.
    Open,
    /// Pressed `by` ticks before landing; the gauge started then.
    Early { by: u16, gauge: Gauge },
    /// Pressed too early: the athlete will stumble.
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Motion {
    /// On the runway, mashing.
    Running,
    /// Foot down before `jump` (0 hop, 1 step, 2 jump), the angle gauge climbing. `keep` is the
    /// share of the speed the touchdown kept.
    Planted { jump: u8, gauge: Gauge, keep: i32 },
    /// In the air for `jump`, from `from` to `from + length - drift` over `ticks`.
    Flying {
        jump: u8,
        t: u16,
        ticks: u16,
        from: i32,
        length: i32,
        angle: u16,
        /// How far the wind has pushed it back.
        drift: i32,
        next: Next,
    },
    /// Down after `jump`, waiting `t` ticks so far for the press.
    Landed { jump: u8, t: u16 },
    /// This attempt is over.
    Done,
}

/// An attempt's result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Mark {
    #[default]
    Pending,
    /// Over the line before taking off: no mark.
    Foul,
    /// Missed a touchdown and fell where it landed, in mm.
    Stumble(i32),
    /// All three jumps, landing this far from the line, in mm.
    Landed(i32),
}

impl Mark {
    /// The distance that counts, in mm.
    pub fn distance(self) -> Option<i32> {
        match self {
            Mark::Stumble(mm) | Mark::Landed(mm) => Some(mm.max(0)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Athlete {
    pub lane: u8,
    pub team: u8,
    /// Who plays it: in 2v2 both partners, taking turns as runner and coach.
    pub members: [Seat; 2],
    pub member_count: u8,
    pub x: i32,
    /// The run-up flywheel, which the speed follows (µm a tick).
    pub drive: i32,
    pub speed: i32,
    /// Ticks on the runway since the last press.
    pub since_press: u16,
    /// Presses this attempt.
    pub presses: u16,
    pub motion: Motion,
    pub marks: [Mark; MAX_ATTEMPTS],
    /// Where this attempt took off.
    pub take_off: Option<i32>,
    /// The last touchdown's grade, the last launch's angle, and the ticks they came on.
    pub last_grade: Option<(u32, Grade)>,
    pub last_angle: Option<(u32, u16)>,
    /// The headwind on it this tick, per mille of full.
    pub headwind: i32,
}

impl Athlete {
    /// Who runs in `attempt`.
    pub fn runner(&self, attempt: u8) -> Seat {
        self.members[attempt as usize % self.member_count as usize]
    }

    /// Who sets the angle in `attempt`, in 2v2.
    pub fn coach(&self, attempt: u8) -> Option<Seat> {
        (self.member_count == 2).then(|| self.members[(attempt as usize + 1) % 2])
    }

    pub fn plays(&self, seat: Seat) -> bool {
        self.members[..self.member_count as usize].contains(&seat)
    }

    /// The best and second-best marks so far, in mm.
    pub fn best(&self) -> (Option<i32>, Option<i32>) {
        let mut marks: Vec<i32> = self.marks.iter().filter_map(|m| m.distance()).collect();
        marks.sort_unstable_by(|a, b| b.cmp(a));
        (marks.first().copied(), marks.get(1).copied())
    }

    fn start(&mut self) {
        self.x = -RUNWAY;
        self.drive = 0;
        self.speed = 0;
        self.since_press = MASH_GAP;
        self.presses = 0;
        self.motion = Motion::Running;
        self.take_off = None;
        self.headwind = 0;
    }
}

/// The 1v3 solo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Wind {
    pub seat: Seat,
    /// The lane the fan is on, or moving to.
    pub lane: u8,
    /// Ticks left moving there from `from`.
    pub travel: u16,
    pub from: u8,
    /// Per mille of full.
    pub strength: i32,
    pub since_press: u16,
    pub presses: u16,
    pub cooldown: u16,
    /// The tick of the last gust.
    pub last_gust: Option<u32>,
}

impl Wind {
    /// How hard the fan blows on `lane`.
    pub fn on(&self, lane: u8) -> i32 {
        if self.travel == 0 && self.lane == lane {
            self.strength
        } else {
            0
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Intro {
        t: u16,
    },
    /// Everyone at the start of the next attempt.
    Ready {
        t: u16,
    },
    Jumping {
        t: u16,
    },
    /// Everyone's done; the marks are shown.
    Marks {
        t: u16,
    },
    /// The winner is known; everything stands still.
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
    /// Ticks since the countdown.
    pub tick: u32,
    pub attempt: u8,
    pub attempts: u8,
    /// Indexed by lane, top of the screen first.
    pub athletes: [Option<Athlete>; MAX_PLAYERS],
    pub wind: Option<Wind>,
    pub outcome: Option<Outcome>,
}

pub struct TripleJump;

impl State {
    pub fn lanes(&self) -> u8 {
        self.athletes.iter().flatten().count() as u8
    }

    pub fn athletes(&self) -> impl Iterator<Item = &Athlete> + '_ {
        self.athletes.iter().flatten()
    }

    /// The athlete `seat` runs or coaches.
    pub fn athlete_of(&self, seat: Seat) -> Option<&Athlete> {
        self.athletes().find(|a| a.plays(seat))
    }

    /// The angle gauge's rate for this format.
    pub fn angle_rate(&self) -> u16 {
        if matches!(self.format, Format::TwoVsTwo { .. }) {
            COACH_ANGLE_RATE
        } else {
            ANGLE_RATE
        }
    }

    /// Whether `athlete` jumps in `attempt`: in 1v3 the solo jumps only the first and the three
    /// only the rest.
    pub fn takes_part(&self, athlete: &Athlete, attempt: u8) -> bool {
        match self.wind {
            Some(wind) => athlete.plays(wind.seat) == (attempt == 0),
            None => true,
        }
    }

    /// 1v3: the solo's mark, which the three have to beat, in mm, once it has jumped.
    pub fn line(&self) -> Option<i32> {
        let wind = self.wind?;
        let solo = self.athlete_of(wind.seat)?;
        match solo.marks[0] {
            Mark::Pending => None,
            mark => Some(mark.distance().unwrap_or(0)),
        }
    }

    /// 1v3: whether one of the three has landed past the solo's mark.
    pub fn qualifies(&self, athlete: &Athlete) -> bool {
        let Some(line) = self.line() else {
            return false;
        };
        self.wind.is_some_and(|w| !athlete.plays(w.seat))
            && athlete
                .marks
                .iter()
                .any(|m| m.distance().is_some_and(|mm| mm > line))
    }

    /// 1v3: how many of the three are past the solo's mark.
    pub fn qualified(&self) -> u8 {
        self.athletes().filter(|a| self.qualifies(a)).count() as u8
    }

    fn step_wind(&mut self, inputs: &Inputs) {
        let Some(mut wind) = self.wind else {
            return;
        };
        if self.attempt == 0 {
            // The solo is jumping.
            return;
        }
        let input = inputs.current[wind.seat.index()];
        wind.since_press = wind.since_press.saturating_add(1);
        if inputs.just_pressed(wind.seat, Button::A) {
            wind.strength += WIND_PUSH * wind.since_press.min(MASH_GAP) as i32 / MASH_GAP as i32;
            wind.presses += 1;
            wind.since_press = 0;
        }
        wind.strength = (wind.strength - wind.strength / WIND_DRAG_TICKS).clamp(0, 1000);
        if wind.travel > 0 {
            wind.travel -= 1;
        } else if input.stick_y >= FAN_STICK && wind.lane > 1 {
            wind.from = wind.lane;
            wind.lane -= 1;
            wind.travel = FAN_MOVE_TICKS;
        } else if input.stick_y <= -FAN_STICK && wind.lane + 1 < self.lanes() {
            wind.from = wind.lane;
            wind.lane += 1;
            wind.travel = FAN_MOVE_TICKS;
        }
        wind.cooldown = wind.cooldown.saturating_sub(1);
        if inputs.just_pressed(wind.seat, Button::B) && wind.cooldown == 0 && wind.travel == 0 {
            wind.cooldown = GUST_COOLDOWN;
            wind.last_gust = Some(self.tick);
            if let Some(athlete) = &mut self.athletes[wind.lane as usize] {
                athlete.speed -= athlete.speed * GUST_CUT / 1000;
                athlete.drive -= athlete.drive * GUST_CUT / 1000;
                if let Motion::Flying { drift, .. } = &mut athlete.motion {
                    *drift += GUST_PUSH;
                }
            }
        }
        self.wind = Some(wind);
    }

    fn step_athlete(&mut self, lane: usize, inputs: &Inputs) {
        let Some(mut a) = self.athletes[lane] else {
            return;
        };
        let runner = a.runner(self.attempt);
        let coach = a
            .coach(self.attempt)
            .map(|c| inputs.just_pressed(c, Button::A));
        let pressed = inputs.just_pressed(runner, Button::B);
        let held = inputs.held(runner, Button::B);
        let rate = self.angle_rate();
        a.headwind = self.wind.map_or(0, |w| w.on(a.lane));
        let tick = self.tick;
        let attempt = self.attempt as usize;
        let launch = |a: &mut Athlete, jump: u8, angle: u16, keep: i32| {
            a.speed = a.speed * keep / 1000;
            a.last_angle = Some((tick, angle));
            let ticks = flight_ticks(a.speed, angle);
            Motion::Flying {
                jump,
                t: 0,
                ticks,
                from: a.x,
                length: jump_length(jump, a.speed, angle),
                angle,
                drift: 0,
                next: Next::Open,
            }
        };
        a.motion = match a.motion {
            Motion::Running if pressed => {
                a.take_off = Some(a.x);
                Motion::Planted {
                    jump: 0,
                    gauge: Gauge::default(),
                    keep: 1000,
                }
            }
            Motion::Running => {
                a.since_press = a.since_press.saturating_add(1);
                if inputs.just_pressed(runner, Button::A) {
                    a.drive += PUSH * a.since_press.min(MASH_GAP) as i32 / MASH_GAP as i32;
                    a.presses += 1;
                    a.since_press = 0;
                }
                a.drive -= a.drive / DRAG_TICKS
                    + (a.drive as i64 * (a.headwind * HEADWIND_RUN) as i64 / 1_000_000) as i32;
                a.drive = a.drive.max(0);
                a.speed += (a.drive - a.speed) / SMOOTH_TICKS;
                a.x += a.speed;
                if a.x >= 0 {
                    a.marks[attempt] = Mark::Foul;
                    Motion::Done
                } else {
                    Motion::Running
                }
            }
            Motion::Planted { jump, gauge, keep } => {
                let gauge = gauge.step(held, coach, rate);
                if gauge.locked {
                    launch(&mut a, jump, gauge.angle(rate), keep)
                } else {
                    Motion::Planted { jump, gauge, keep }
                }
            }
            Motion::Flying {
                jump,
                t,
                ticks,
                from,
                length,
                angle,
                drift,
                next,
            } => {
                let t = t + 1;
                let drift = drift
                    + (length as i64 * (a.headwind * HEADWIND_AIR) as i64
                        / (ticks as i64 * 1_000_000)) as i32;
                let next = match next {
                    Next::Open if pressed && jump < 2 => {
                        let by = ticks.saturating_sub(t);
                        if by <= WINDOW {
                            Next::Early {
                                by,
                                gauge: Gauge::default(),
                            }
                        } else {
                            Next::Locked
                        }
                    }
                    Next::Early { by, gauge } => Next::Early {
                        by,
                        gauge: gauge.step(held, coach, rate),
                    },
                    other => other,
                };
                if t < ticks {
                    a.x = from + (length as i64 * t as i64 / ticks as i64) as i32 - drift;
                    Motion::Flying {
                        jump,
                        t,
                        ticks,
                        from,
                        length,
                        angle,
                        drift,
                        next,
                    }
                } else {
                    a.x = from + length - drift;
                    if jump == 2 {
                        a.marks[attempt] = Mark::Landed(a.x / MM);
                        Motion::Done
                    } else {
                        match next {
                            Next::Locked => {
                                a.marks[attempt] = Mark::Stumble(a.x / MM);
                                Motion::Done
                            }
                            Next::Early { by, gauge } => {
                                let graded = grade(by);
                                a.last_grade = Some((tick, graded));
                                if gauge.locked {
                                    launch(&mut a, jump + 1, gauge.angle(rate), graded.keep())
                                } else {
                                    Motion::Planted {
                                        jump: jump + 1,
                                        gauge,
                                        keep: graded.keep(),
                                    }
                                }
                            }
                            Next::Open => Motion::Landed { jump, t: 0 },
                        }
                    }
                }
            }
            Motion::Landed { jump, t } => {
                let t = t + 1;
                if pressed {
                    let graded = grade(t);
                    a.last_grade = Some((tick, graded));
                    Motion::Planted {
                        jump: jump + 1,
                        gauge: Gauge::default(),
                        keep: graded.keep(),
                    }
                } else if t >= WINDOW {
                    a.marks[attempt] = Mark::Stumble(a.x / MM);
                    Motion::Done
                } else {
                    Motion::Landed { jump, t }
                }
            }
            Motion::Done => Motion::Done,
        };
        self.athletes[lane] = Some(a);
    }

    fn all_done(&self) -> bool {
        self.athletes().all(|a| a.motion == Motion::Done)
    }

    /// Ends the attempt: anyone who never took off has fouled.
    fn close_attempt(&mut self) {
        let attempt = self.attempt as usize;
        for a in self.athletes.iter_mut().flatten() {
            if a.motion != Motion::Done {
                a.marks[attempt] = Mark::Foul;
                a.motion = Motion::Done;
            }
        }
    }

    /// Everyone jumping in this attempt to the start; everyone else stands aside.
    fn line_up(&mut self) {
        for lane in 0..MAX_PLAYERS {
            let Some(mut a) = self.athletes[lane] else {
                continue;
            };
            a.start();
            if !self.takes_part(&a, self.attempt) {
                a.motion = Motion::Done;
            }
            self.athletes[lane] = Some(a);
        }
    }

    /// Whether the match is over after the attempt just shown, and if so its outcome.
    fn decide(&self) -> Option<Outcome> {
        let last = self.attempt + 1 >= self.attempts;
        if self.wind.is_some() {
            if self.attempt == 0 {
                return None;
            }
            let three = self.qualified() >= QUALIFIERS;
            if !three && !last {
                return None;
            }
            let mut scores = [0; MAX_PLAYERS];
            scores[if three { 1 } else { 0 }] = 1;
            return Some(Outcome::from_team_scores(
                &self.players,
                &self.format,
                &scores,
            ));
        }
        if !last {
            return None;
        }
        let mut scores = [0; MAX_PLAYERS];
        for a in self.athletes() {
            let (best, second) = a.best();
            scores[a.team as usize] = (best.unwrap_or(0).min(0xffff) as u32) << 16
                | second.unwrap_or(0).min(0xffff) as u32;
        }
        Some(Outcome::from_team_scores(
            &self.players,
            &self.format,
            &scores,
        ))
    }

    fn next_attempt(&mut self) {
        self.attempt += 1;
        self.line_up();
        if let Some(wind) = &mut self.wind {
            wind.strength = 0;
            wind.presses = 0;
        }
    }
}

impl Minigame for TripleJump {
    type State = State;

    const ID: &'static str = "triple-jump";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[
        FormatKind::FreeForAll,
        FormatKind::OneVsThree,
        FormatKind::TwoVsTwo,
        FormatKind::Duel,
    ];
    const MAX_TICKS: u32 = INTRO_TICKS as u32
        + TEAM_ATTEMPTS as u32 * (ATTEMPT_TICKS + MARKS_TICKS + READY_TICKS) as u32
        + DECIDED_TICKS as u32
        + 60;

    fn new(seed: u64, players: PlayerSetup, format: Format) -> State {
        let solo = match format {
            Format::OneVsThree { solo } => Some(solo),
            _ => None,
        };
        // Seats outside a duel have no team and sit this one out. In 2v2 partners share a lane;
        // otherwise everyone has their own (the 1v3 solo's, team 0, on top).
        let mut jumpers: Vec<(u8, Seat)> = players
            .seats()
            .filter_map(|s| Some((format.team(s)?, s)))
            .collect();
        jumpers.sort();
        let shared = matches!(format, Format::TwoVsTwo { .. });
        let mut athletes: [Option<Athlete>; MAX_PLAYERS] = [None; MAX_PLAYERS];
        let mut lanes = 0;
        for (team, seat) in jumpers {
            if shared
                && let Some(a) = athletes[..lanes]
                    .iter_mut()
                    .flatten()
                    .find(|a| a.team == team)
            {
                a.members[1] = seat;
                a.member_count = 2;
                continue;
            }
            let mut athlete = Athlete {
                lane: lanes as u8,
                team,
                members: [seat; 2],
                member_count: 1,
                x: 0,
                drive: 0,
                speed: 0,
                since_press: 0,
                presses: 0,
                motion: Motion::Running,
                marks: [Mark::Pending; MAX_ATTEMPTS],
                take_off: None,
                last_grade: None,
                last_angle: None,
                headwind: 0,
            };
            athlete.start();
            athletes[lanes] = Some(athlete);
            lanes += 1;
        }
        let mut state = State {
            seed,
            players,
            format,
            phase: Phase::Intro { t: 0 },
            tick: 0,
            attempt: 0,
            attempts: match format {
                Format::TwoVsTwo { .. } => TEAM_ATTEMPTS,
                Format::OneVsThree { .. } => 1 + ATTEMPTS,
                _ => ATTEMPTS,
            },
            athletes,
            wind: solo.map(|seat| Wind {
                seat,
                // The middle of the three's lanes.
                lane: 2,
                travel: 0,
                from: 2,
                strength: 0,
                since_press: MASH_GAP,
                presses: 0,
                cooldown: 0,
                last_gust: None,
            }),
            outcome: None,
        };
        state.line_up();
        state
    }

    fn step(state: &mut State, inputs: &Inputs) {
        if !matches!(state.phase, Phase::Intro { .. } | Phase::Finished) {
            state.tick += 1;
        }
        state.phase = match state.phase {
            Phase::Intro { t } if t + 1 >= INTRO_TICKS => Phase::Jumping { t: 0 },
            Phase::Intro { t } => Phase::Intro { t: t + 1 },
            Phase::Ready { t } if t + 1 >= READY_TICKS => Phase::Jumping { t: 0 },
            Phase::Ready { t } => Phase::Ready { t: t + 1 },
            Phase::Jumping { t } => {
                state.step_wind(inputs);
                for lane in 0..MAX_PLAYERS {
                    state.step_athlete(lane, inputs);
                }
                if state.all_done() || t + 1 >= ATTEMPT_TICKS {
                    state.close_attempt();
                    Phase::Marks { t: 0 }
                } else {
                    Phase::Jumping { t: t + 1 }
                }
            }
            Phase::Marks { t } if t + 1 >= MARKS_TICKS => match state.decide() {
                Some(outcome) => {
                    state.outcome = Some(outcome);
                    Phase::Decided { t: 0 }
                }
                None => {
                    state.next_attempt();
                    Phase::Ready { t: 0 }
                }
            },
            Phase::Marks { t } => Phase::Marks { t: t + 1 },
            Phase::Decided { t } if t + 1 >= DECIDED_TICKS => Phase::Finished,
            Phase::Decided { t } => Phase::Decided { t: t + 1 },
            Phase::Finished => Phase::Finished,
        };
    }

    fn outcome(state: &State) -> Option<Outcome> {
        if state.phase == Phase::Finished {
            state.outcome
        } else {
            None
        }
    }

    fn bot_input(state: &State, seat: Seat, difficulty: Difficulty) -> Input {
        if !matches!(state.phase, Phase::Jumping { .. }) || !state.players.is_active(seat) {
            return Input::default();
        }
        if state.wind.is_some_and(|w| w.seat == seat) && state.attempt > 0 {
            return bot::blow(state, difficulty);
        }
        let Some(athlete) = state.athlete_of(seat) else {
            return Input::default();
        };
        if athlete.runner(state.attempt) == seat {
            bot::run(state, athlete, difficulty)
        } else {
            bot::coach(state, athlete, seat, difficulty)
        }
    }
}

pub mod bot {
    //! Bots mash at a rate set by difficulty, take off a little short of the line, hold the angle
    //! to about 45° and press near each touchdown, all rolled from the seed, the seat and the
    //! attempt so the bot stays a pure function. The wind bot blows on the fastest jumper who
    //! hasn't qualified yet and gusts them close to the line.

    use sim_core::Rng;

    use super::*;

    /// How well a bot plays at a difficulty.
    #[derive(Debug, Clone, Copy)]
    pub struct Skill {
        /// Ticks between presses, rolled per press from this range.
        pub mash: (u16, u16),
        /// How far short of the line a bot takes off, at most (µm).
        pub take_off: i32,
        /// The chance in percent of running over the line.
        pub foul: u32,
        /// How far off 45° a bot holds the angle, at most, either way (tenths of a degree).
        pub angle: u16,
        /// How many ticks off the landing a bot presses, at most, either way...
        pub touch: u16,
        /// ...and the chance in percent of missing the window altogether.
        pub slip: u32,
        /// The wind's chance in percent, each second, of blowing on a lane it picks at random.
        pub distracted: u32,
    }

    pub fn skill(difficulty: Difficulty) -> Skill {
        match difficulty {
            Difficulty::Easy => Skill {
                mash: (8, 10),
                take_off: 800 * MM,
                foul: 10,
                angle: 120,
                touch: 7,
                slip: 8,
                distracted: 35,
            },
            Difficulty::Normal => Skill {
                mash: (7, 8),
                take_off: 400 * MM,
                foul: 5,
                angle: 60,
                touch: 4,
                slip: 4,
                distracted: 15,
            },
            Difficulty::Hard => Skill {
                mash: (6, 7),
                take_off: 150 * MM,
                foul: 2,
                angle: 30,
                touch: 2,
                slip: 1,
                distracted: 0,
            },
        }
    }

    /// A new seed from `seed` and `part`, unrelated to any other pair's.
    fn mix(seed: u64, part: u64) -> u64 {
        let mut rng = Rng::new(seed ^ part.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        ((rng.next_u32() as u64) << 32) | rng.next_u32() as u64
    }

    /// Each part is mixed in on its own: XORed together, parts from different matches could
    /// cancel out and bots would roll alike.
    fn roll(state: &State, parts: &[u64]) -> Rng {
        Rng::new(parts.iter().copied().fold(state.seed, mix))
    }

    /// What a bot means to do in an attempt.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Plan {
        /// Where it takes off (µm from the line); past the line means it runs over.
        pub take_off: i32,
        /// The angles it holds for the hop, step and jump.
        pub angles: [u16; 3],
        /// Ticks after (or, negative, before) each touchdown it presses.
        pub touches: [i16; 2],
    }

    pub fn plan(state: &State, seat: Seat, difficulty: Difficulty) -> Plan {
        let skill = skill(difficulty);
        let mut rng = roll(state, &[1, seat.0 as u64, state.attempt as u64]);
        let take_off = if rng.chance(skill.foul) {
            1
        } else {
            -(rng.range_u32(0, skill.take_off as u32) as i32)
        };
        let angles = [(); 3]
            .map(|_| BEST_ANGLE + rng.range_u32(0, 2 * skill.angle as u32) as u16 - skill.angle);
        let touches = [(); 2].map(|_| {
            if rng.chance(skill.slip) {
                // Too early (locked out) or too late.
                if rng.chance(50) {
                    -(WINDOW as i16 + 3)
                } else {
                    WINDOW as i16 + 1
                }
            } else {
                rng.range_u32(0, 2 * skill.touch as u32) as i16 - skill.touch as i16
            }
        });
        Plan {
            take_off,
            angles,
            touches,
        }
    }

    /// Whether to press A this tick to mash at the bot's pace, `since` ticks after the last.
    fn mash(state: &State, seat: Seat, presses: u16, since: u16, difficulty: Difficulty) -> bool {
        let (lo, hi) = skill(difficulty).mash;
        let mut rng = roll(
            state,
            &[2, seat.0 as u64, state.attempt as u64, presses as u64],
        );
        since + 1 >= rng.range_u32(lo as u32, hi as u32) as u16
    }

    /// Whether holding the gauge one more tick gets closer to `target` than letting go now.
    fn hold(gauge: Gauge, rate: u16, target: u16) -> bool {
        gauge.ticks * rate + rate / 2 < target
    }

    pub fn run(state: &State, athlete: &Athlete, difficulty: Difficulty) -> Input {
        let seat = athlete.runner(state.attempt);
        let plan = plan(state, seat, difficulty);
        let solo = athlete.coach(state.attempt).is_none();
        let rate = state.angle_rate();
        let mut input = Input::default();
        let (a, b) = match athlete.motion {
            Motion::Running => (
                mash(
                    state,
                    seat,
                    athlete.presses,
                    athlete.since_press,
                    difficulty,
                ),
                plan.take_off <= 0 && athlete.x + athlete.speed >= plan.take_off,
            ),
            Motion::Planted { jump, gauge, .. } => {
                (false, solo && hold(gauge, rate, plan.angles[jump as usize]))
            }
            Motion::Flying {
                jump,
                t,
                ticks,
                next,
                ..
            } if jump < 2 => match next {
                // The press lands on the next tick, `ticks - (t + 1)` before touching down.
                Next::Open => {
                    let touch = plan.touches[jump as usize];
                    (false, touch <= 0 && ticks as i16 - (t as i16 + 1) == -touch)
                }
                Next::Early { gauge, .. } => (
                    false,
                    solo && hold(gauge, rate, plan.angles[jump as usize + 1]),
                ),
                Next::Locked => (false, false),
            },
            Motion::Landed { jump, t } => {
                let touch = plan.touches[jump as usize];
                (false, touch > 0 && t as i16 + 1 == touch)
            }
            _ => (false, false),
        };
        input.set(Button::A, a);
        input.set(Button::B, b);
        input
    }

    /// 2v2: lock the gauge with A when it reaches the angle this coach is going for.
    pub fn coach(state: &State, athlete: &Athlete, seat: Seat, difficulty: Difficulty) -> Input {
        let plan = plan(state, seat, difficulty);
        let rate = state.angle_rate();
        let gauge = match athlete.motion {
            Motion::Planted { jump, gauge, .. } => Some((jump, gauge)),
            Motion::Flying {
                jump,
                next: Next::Early { gauge, .. },
                ..
            } => Some((jump + 1, gauge)),
            _ => None,
        };
        let mut input = Input::default();
        if let Some((jump, gauge)) = gauge {
            input.set(
                Button::A,
                !gauge.locked && !hold(gauge, rate, plan.angles[jump as usize]),
            );
        }
        input
    }

    /// How much the wind would gain by blowing on this athlete now: its speed, while it's still
    /// jumping this attempt and hasn't beaten the line.
    fn threat(state: &State, athlete: &Athlete) -> i32 {
        if athlete.motion == Motion::Done || state.qualifies(athlete) {
            0
        } else {
            athlete.speed
        }
    }

    /// The lane the wind bot wants: the biggest threat, unless the one it's on is close.
    pub fn target(state: &State, difficulty: Difficulty) -> u8 {
        let Some(wind) = state.wind else {
            return 0;
        };
        let skill = skill(difficulty);
        let mut rng = roll(state, &[3, state.attempt as u64, state.tick as u64 / 60]);
        // The three's lanes are 1 and on.
        if rng.chance(skill.distracted) {
            return rng.range_u32(1, state.lanes() as u32 - 1) as u8;
        }
        let best = state
            .athletes()
            .filter(|a| a.lane > 0)
            .max_by_key(|a| (threat(state, a), u8::MAX - a.lane))
            .map_or(wind.lane, |a| a.lane);
        let current = state.athletes[wind.lane as usize].map_or(0, |a| threat(state, &a));
        let best_threat = state.athletes[best as usize].map_or(0, |a| threat(state, &a));
        if current * 5 / 4 >= best_threat {
            wind.lane
        } else {
            best
        }
    }

    pub fn blow(state: &State, difficulty: Difficulty) -> Input {
        let Some(wind) = state.wind else {
            return Input::default();
        };
        let target = target(state, difficulty);
        let mut input = Input {
            stick_y: if wind.travel > 0 || target == wind.lane {
                0
            } else if target < wind.lane {
                127
            } else {
                -127
            },
            ..Input::default()
        };
        input.set(
            Button::A,
            mash(state, wind.seat, wind.presses, wind.since_press, difficulty),
        );
        // Gust a runner closing on the line, or a hop in the air.
        let gust = wind.cooldown == 0
            && wind.travel == 0
            && state.athletes[wind.lane as usize].is_some_and(|a| match a.motion {
                Motion::Running => a.x > -6_000 * MM && threat(state, &a) > 0,
                Motion::Flying { jump: 0, .. } => !state.qualifies(&a),
                _ => false,
            });
        input.set(Button::B, gust);
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

    fn new_run(players: usize, format: Format) -> Run<TripleJump> {
        Run::new(7, PlayerSetup::first(players), format, RunConfig::default())
    }

    fn step_until(run: &mut Run<TripleJump>, done: impl Fn(&State) -> bool) {
        for _ in 0..TripleJump::MAX_TICKS {
            if done(&run.game) {
                return;
            }
            run.step(NONE);
        }
        panic!("never reached");
    }

    fn jumping(players: usize, format: Format) -> Run<TripleJump> {
        let mut run = new_run(players, format);
        step_until(&mut run, |g| matches!(g.phase, Phase::Jumping { .. }));
        run
    }

    fn athlete(run: &Run<TripleJump>, lane: usize) -> Athlete {
        run.game.athletes[lane].unwrap()
    }

    fn press(button: Button) -> Input {
        Input::default().with(button)
    }

    /// Mashes `lane`'s runner (seat `seat`) every `gap` ticks for `ticks` ticks.
    fn mash(run: &mut Run<TripleJump>, seat: usize, gap: u32, ticks: u32) {
        for _ in 0..ticks {
            // Counted on the run's own tick, so mashing a tick at a time keeps the pace.
            let mut inputs = NONE;
            if run.tick.is_multiple_of(gap) {
                inputs[seat] = press(Button::A);
            }
            run.step(inputs);
        }
    }

    /// Plays one solo jump: run at full pace to `take_off`, then hold each angle and press each
    /// touchdown `touch` ticks after landing. Returns the mark.
    fn jump(run: &mut Run<TripleJump>, seat: usize, take_off: i32, angle: u16, touch: u16) -> Mark {
        let lane = seat;
        let attempt = run.game.attempt as usize;
        let mut t = 0u32;
        while athlete(run, lane).x + athlete(run, lane).speed < take_off {
            let mut inputs = NONE;
            if t.is_multiple_of(MASH_GAP as u32) {
                inputs[seat] = press(Button::A);
            }
            run.step(inputs);
            t += 1;
        }
        let hold_ticks = angle / ANGLE_RATE;
        for j in 0..3 {
            if j > 0 {
                step_until(run, |g| {
                    matches!(g.athletes[lane].unwrap().motion, Motion::Landed { .. })
                });
                for _ in 1..touch {
                    run.step(NONE);
                }
            }
            let mut inputs = NONE;
            inputs[seat] = press(Button::B);
            for _ in 0..=hold_ticks {
                run.step(inputs);
            }
            run.step(NONE);
            assert!(
                matches!(athlete(run, lane).motion, Motion::Flying { jump, angle: a, .. } if jump == j && a == angle),
                "{:?}",
                athlete(run, lane).motion
            );
        }
        step_until(run, |g| g.athletes[lane].unwrap().motion == Motion::Done);
        athlete(run, lane).marks[attempt]
    }

    #[test]
    fn the_angle_is_best_at_45() {
        assert_eq!(angle_share(BEST_ANGLE), 1000);
        assert!(angle_share(BEST_ANGLE + 30) > 980);
        assert!(angle_share(BEST_ANGLE - 100) < 850);
        assert_eq!(angle_share(0), MIN_ANGLE_SHARE);
        assert_eq!(angle_share(MAX_ANGLE), MIN_ANGLE_SHARE);
        assert_eq!(jump_length(0, TOP_SPEED, BEST_ANGLE), JUMP_LENGTHS[0]);
        assert!((jump_length(0, TOP_SPEED / 2, BEST_ANGLE) - JUMP_LENGTHS[0] / 2).abs() < 100);
        assert_eq!(flight_ticks(TOP_SPEED, BEST_ANGLE), FLIGHT_TICKS as u16);
        assert!(flight_ticks(TOP_SPEED, 700) > flight_ticks(TOP_SPEED, BEST_ANGLE));
        // 45° is a step of the gauge, so holding to it is exact.
        assert_eq!(BEST_ANGLE % ANGLE_RATE, 0);
        assert_eq!(BEST_ANGLE % COACH_ANGLE_RATE, 0);
    }

    #[test]
    fn steady_mashing_beats_a_late_burst() {
        // Ten presses a second all the way...
        let mut steady = jumping(1, Format::FreeForAll);
        while athlete(&steady, 0).x < -1_000 * MM {
            mash(&mut steady, 0, MASH_GAP as u32, 1);
        }
        // ...against five a second, then ten for the last second.
        let mut burst = jumping(1, Format::FreeForAll);
        let mut ticks = 0u32;
        while athlete(&burst, 0).x < -1_000 * MM {
            let fast = athlete(&burst, 0).x > -RUNWAY / 3;
            let gap = if fast { 6 } else { 12 };
            let mut inputs = NONE;
            if ticks.is_multiple_of(gap) {
                inputs[0] = press(Button::A);
            }
            burst.step(inputs);
            ticks += 1;
        }
        let (steady, burst) = (athlete(&steady, 0).speed, athlete(&burst, 0).speed);
        assert!(steady > TOP_SPEED * 85 / 100, "{steady}");
        assert!(burst < steady * 85 / 100, "{burst} {steady}");
    }

    #[test]
    fn mashing_faster_than_the_gap_gains_nothing() {
        let mut fast = jumping(1, Format::FreeForAll);
        mash(&mut fast, 0, 2, 120);
        let mut steady = jumping(1, Format::FreeForAll);
        mash(&mut steady, 0, MASH_GAP as u32, 120);
        let (fast, steady) = (athlete(&fast, 0).speed, athlete(&steady, 0).speed);
        assert!(fast <= steady * 102 / 100, "{fast} {steady}");
    }

    #[test]
    fn a_full_jump_lands_a_mark() {
        let mut run = jumping(2, Format::FreeForAll);
        let mark = jump(&mut run, 0, -100 * MM, BEST_ANGLE, 1);
        let Mark::Landed(mm) = mark else {
            panic!("{mark:?}")
        };
        assert!((12_000..16_000).contains(&mm), "{mm}");
        assert_eq!(athlete(&run, 0).last_grade.unwrap().1, Grade::Perfect);
        // A worse angle lands shorter.
        let mut run = jumping(2, Format::FreeForAll);
        let Mark::Landed(worse) = jump(&mut run, 0, -100 * MM, 300, 1) else {
            panic!()
        };
        assert!(worse < mm - 1_000, "{worse} {mm}");
    }

    #[test]
    fn a_late_touchdown_keeps_less_speed() {
        let mut perfect = jumping(2, Format::FreeForAll);
        let Mark::Landed(perfect) = jump(&mut perfect, 0, -100 * MM, BEST_ANGLE, 1) else {
            panic!()
        };
        let mut late = jumping(2, Format::FreeForAll);
        let Mark::Landed(late_mm) = jump(&mut late, 0, -100 * MM, BEST_ANGLE, 7) else {
            panic!()
        };
        assert_eq!(athlete(&late, 0).last_grade.unwrap().1, Grade::Off);
        assert!(late_mm < perfect - 1_000, "{late_mm} {perfect}");
    }

    #[test]
    fn no_press_at_a_touchdown_is_a_stumble() {
        let mut run = jumping(2, Format::FreeForAll);
        jump_hop(&mut run);
        step_until(&mut run, |g| {
            matches!(g.athletes[0].unwrap().motion, Motion::Landed { .. })
        });
        let landed = athlete(&run, 0).x;
        for _ in 1..WINDOW {
            run.step(NONE);
            assert!(matches!(athlete(&run, 0).motion, Motion::Landed { .. }));
        }
        run.step(NONE);
        assert_eq!(athlete(&run, 0).marks[0], Mark::Stumble(landed / MM));
        assert!(landed > 4_000 * MM, "{landed}");
        // Pressing after that does nothing.
        let mut inputs = NONE;
        inputs[0] = press(Button::B);
        run.step(inputs);
        assert_eq!(athlete(&run, 0).motion, Motion::Done);
    }

    #[test]
    fn a_press_too_early_in_the_air_is_a_stumble() {
        let mut run = jumping(2, Format::FreeForAll);
        jump_hop(&mut run);
        // Press as soon as it's in the air.
        let mut inputs = NONE;
        inputs[0] = press(Button::B);
        run.step(inputs);
        assert!(matches!(
            athlete(&run, 0).motion,
            Motion::Flying {
                next: Next::Locked,
                ..
            }
        ));
        step_until(&mut run, |g| g.athletes[0].unwrap().motion == Motion::Done);
        assert!(matches!(athlete(&run, 0).marks[0], Mark::Stumble(_)));
    }

    /// Runs to the line at full pace and launches the hop at 45°.
    fn jump_hop(run: &mut Run<TripleJump>) {
        while athlete(run, 0).x + athlete(run, 0).speed < -100 * MM {
            mash(run, 0, MASH_GAP as u32, 1);
        }
        let mut inputs = NONE;
        inputs[0] = press(Button::B);
        for _ in 0..=BEST_ANGLE / ANGLE_RATE {
            run.step(inputs);
        }
        run.step(NONE);
        assert!(matches!(
            athlete(run, 0).motion,
            Motion::Flying { jump: 0, .. }
        ));
    }

    #[test]
    fn an_early_press_in_the_window_keeps_its_hold() {
        let mut run = jumping(2, Format::FreeForAll);
        jump_hop(&mut run);
        let Motion::Flying { ticks, .. } = athlete(&run, 0).motion else {
            panic!()
        };
        step_until(
            &mut run,
            |g| matches!(g.athletes[0].unwrap().motion, Motion::Flying { t, .. } if t + 2 == ticks),
        );
        // Pressed two ticks early and held: the gauge has been climbing since.
        let mut inputs = NONE;
        inputs[0] = press(Button::B);
        run.step(inputs);
        run.step(inputs);
        run.step(inputs);
        let Motion::Planted {
            jump: 1,
            gauge,
            keep,
        } = athlete(&run, 0).motion
        else {
            panic!("{:?}", athlete(&run, 0).motion)
        };
        assert_eq!(keep, Grade::Perfect.keep());
        assert_eq!(gauge.ticks, 2);
    }

    #[test]
    fn running_over_the_line_is_a_foul() {
        let mut run = jumping(2, Format::FreeForAll);
        while athlete(&run, 0).motion == Motion::Running {
            mash(&mut run, 0, MASH_GAP as u32, 1);
        }
        assert_eq!(athlete(&run, 0).marks[0], Mark::Foul);
        // B past the line does nothing.
        let mut inputs = NONE;
        inputs[0] = press(Button::B);
        run.step(inputs);
        assert_eq!(athlete(&run, 0).motion, Motion::Done);
    }

    #[test]
    fn taking_off_early_costs_distance() {
        let mut close = jumping(2, Format::FreeForAll);
        let Mark::Landed(close) = jump(&mut close, 0, -100 * MM, BEST_ANGLE, 1) else {
            panic!()
        };
        let mut early = jumping(2, Format::FreeForAll);
        let Mark::Landed(early) = jump(&mut early, 0, -1_100 * MM, BEST_ANGLE, 1) else {
            panic!()
        };
        // A metre less runway is a little less speed too.
        assert!((close - early - 1_000).abs() < 400, "{close} {early}");
    }

    #[test]
    fn best_mark_wins_and_the_second_breaks_ties() {
        let mut run = new_run(2, Format::FreeForAll);
        run.game.phase = Phase::Marks { t: MARKS_TICKS - 1 };
        run.game.attempt = ATTEMPTS - 1;
        let marks = [
            [Mark::Landed(14_000), Mark::Foul, Mark::Landed(12_000)],
            [
                Mark::Landed(14_000),
                Mark::Landed(13_000),
                Mark::Stumble(4_000),
            ],
        ];
        for (lane, marks) in marks.iter().enumerate() {
            run.game.athletes[lane].as_mut().unwrap().marks[..3].copy_from_slice(marks);
        }
        step_until(&mut run, |g| g.phase == Phase::Finished);
        assert_eq!(
            run.outcome().unwrap().winners().collect::<Vec<_>>(),
            vec![Seat(1)]
        );
    }

    #[test]
    fn three_attempts_then_the_result() {
        let mut run = jumping(3, Format::FreeForAll);
        assert_eq!(run.game.attempts, ATTEMPTS);
        step_until(&mut run, |g| g.phase == Phase::Finished);
        // Nobody moved: three fouls each, and everyone shares the win.
        for a in run.game.athletes() {
            assert_eq!(a.marks[..3], [Mark::Foul; 3]);
            assert_eq!(a.marks[3], Mark::Pending);
        }
        assert_eq!(
            run.outcome().unwrap().placement,
            [Some(1), Some(1), Some(1), None]
        );
    }

    #[test]
    fn nothing_moves_during_the_countdown() {
        let mut run = new_run(2, Format::FreeForAll);
        for t in 0..60u32 {
            let mut inputs = NONE;
            if t.is_multiple_of(2) {
                inputs[0] = press(Button::A);
            }
            run.step(inputs);
        }
        assert!(matches!(run.game.phase, Phase::Intro { .. }));
        assert_eq!(athlete(&run, 0).speed, 0);
        assert_eq!(athlete(&run, 0).x, -RUNWAY);
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
        assert_eq!(run.game.lanes(), 2);
        assert!(run.game.athlete_of(Seat(0)).is_none());
        assert_eq!(run.game.athlete_of(Seat(3)).unwrap().lane, 1);
    }

    fn team_run() -> Run<TripleJump> {
        jumping(
            4,
            Format::TwoVsTwo {
                team_of: [0, 1, 1, 0],
            },
        )
    }

    #[test]
    fn partners_share_an_athlete_and_swap_roles() {
        let run = team_run();
        assert_eq!(run.game.lanes(), 2);
        assert_eq!(run.game.attempts, TEAM_ATTEMPTS);
        let a = athlete(&run, 0);
        assert_eq!(a.members, [Seat(0), Seat(3)]);
        assert_eq!((a.runner(0), a.coach(0)), (Seat(0), Some(Seat(3))));
        assert_eq!((a.runner(1), a.coach(1)), (Seat(3), Some(Seat(0))));
        assert_eq!(athlete(&run, 1).members, [Seat(1), Seat(2)]);
    }

    #[test]
    fn the_coach_locks_the_angle() {
        let mut run = team_run();
        // The runner (P1) mashes; the coach's A does nothing to the run.
        for t in 0..200u32 {
            let mut inputs = NONE;
            if t.is_multiple_of(6) {
                inputs[0] = press(Button::A);
            }
            if t % 6 == 3 {
                inputs[3] = press(Button::A);
            }
            run.step(inputs);
        }
        let speed = athlete(&run, 0).speed;
        assert!(speed > TOP_SPEED / 2);
        // The runner's B starts the gauge, which climbs with nobody holding anything...
        let mut inputs = NONE;
        inputs[0] = press(Button::B);
        run.step(inputs);
        for _ in 0..BEST_ANGLE / COACH_ANGLE_RATE {
            run.step(NONE);
        }
        assert!(matches!(athlete(&run, 0).motion, Motion::Planted { .. }));
        // ...until the coach presses A.
        let mut inputs = NONE;
        inputs[3] = press(Button::A);
        run.step(inputs);
        assert!(matches!(
            athlete(&run, 0).motion,
            Motion::Flying {
                angle: BEST_ANGLE,
                ..
            }
        ));
    }

    fn wind_run() -> Run<TripleJump> {
        jumping(4, Format::OneVsThree { solo: Seat(1) })
    }

    /// The solo (P2) has jumped `line` mm and the three's first attempt is under way.
    fn windy_run(line: i32) -> Run<TripleJump> {
        let mut run = wind_run();
        run.game.athletes[0].as_mut().unwrap().marks[0] = Mark::Landed(line);
        run.game.athletes[0].as_mut().unwrap().motion = Motion::Done;
        step_until(&mut run, |g| matches!(g.phase, Phase::Ready { .. }));
        step_until(&mut run, |g| matches!(g.phase, Phase::Jumping { .. }));
        run
    }

    #[test]
    fn the_solo_jumps_alone_first() {
        let mut run = wind_run();
        assert_eq!(run.game.attempts, 1 + ATTEMPTS);
        // The solo's lane is on top and the three stand aside.
        assert_eq!(athlete(&run, 0).members[0], Seat(1));
        assert_eq!(athlete(&run, 0).motion, Motion::Running);
        for lane in 1..4 {
            assert_eq!(athlete(&run, lane).motion, Motion::Done);
        }
        assert_eq!(run.game.line(), None);
        // The wind doesn't blow while the solo jumps: its A mashes the run-up.
        mash(&mut run, 1, 6, 60);
        assert_eq!(run.game.wind.unwrap().strength, 0);
        assert!(athlete(&run, 0).speed > 0);
        // A foul makes the line 0.
        step_until(&mut run, |g| matches!(g.phase, Phase::Ready { .. }));
        assert_eq!(athlete(&run, 0).marks[0], Mark::Foul);
        assert_eq!(run.game.line(), Some(0));
        assert_eq!(athlete(&run, 0).motion, Motion::Done);
        assert_eq!(athlete(&run, 2).motion, Motion::Running);
    }

    #[test]
    fn the_wind_moves_between_the_three_lanes() {
        let mut run = windy_run(10_000);
        assert_eq!(run.game.wind.unwrap().lane, 2);
        let mut inputs = NONE;
        inputs[1].stick_y = 127;
        run.step(inputs);
        assert_eq!(run.game.wind.unwrap().lane, 1);
        for _ in 0..30 {
            run.step(inputs);
        }
        // Never over the solo's own lane.
        assert_eq!(run.game.wind.unwrap().lane, 1);
        inputs[1].stick_y = -127;
        for _ in 0..60 {
            run.step(inputs);
        }
        assert_eq!(run.game.wind.unwrap().lane, 3);
    }

    #[test]
    fn a_headwind_slows_the_runner_it_blows_on() {
        let mut run = windy_run(10_000);
        mash(&mut run, 1, 6, 1);
        for _ in 0..150 {
            let mut inputs = NONE;
            if run.tick.is_multiple_of(6) {
                inputs = [press(Button::A); MAX_PLAYERS];
            }
            run.step(inputs);
        }
        // The fan is on the middle lane of the three (P3).
        let speeds: Vec<i32> = run.game.athletes().skip(1).map(|a| a.speed).collect();
        assert!(run.game.wind.unwrap().strength > 800);
        assert!(speeds[1] < speeds[0] * 95 / 100, "{speeds:?}");
        assert_eq!(speeds[0], speeds[2]);
    }

    #[test]
    fn a_gust_cuts_speed_and_cools_down() {
        let mut run = windy_run(10_000);
        mash(&mut run, 2, 6, 100);
        let before = athlete(&run, 2).speed;
        let mut inputs = NONE;
        inputs[1] = press(Button::B);
        run.step(inputs);
        let after = athlete(&run, 2).speed;
        // The gust's cut, give or take this tick's mashing and drag.
        assert!(after < before * (1000 - GUST_CUT + 15) / 1000, "{before} {after}");
        assert_eq!(run.game.wind.unwrap().cooldown, GUST_COOLDOWN);
        run.step(NONE);
        run.step(inputs);
        assert_eq!(run.game.wind.unwrap().cooldown, GUST_COOLDOWN - 2);
    }

    #[test]
    fn the_three_win_once_two_beat_the_solo() {
        let mut run = windy_run(10_000);
        run.game.phase = Phase::Marks { t: MARKS_TICKS - 1 };
        run.game.athletes[1].as_mut().unwrap().marks[1] = Mark::Landed(10_001);
        run.game.athletes[3].as_mut().unwrap().marks[1] = Mark::Stumble(10_500);
        // Equalling the line isn't enough.
        run.game.athletes[2].as_mut().unwrap().marks[1] = Mark::Landed(10_000);
        assert_eq!(run.game.qualified(), 2);
        step_until(&mut run, |g| g.phase == Phase::Finished);
        assert_eq!(run.game.attempt, 1);
        assert_eq!(
            run.outcome().unwrap().placement,
            [Some(1), Some(2), Some(1), Some(1)]
        );
    }

    #[test]
    fn the_wind_wins_if_fewer_beat_it() {
        let mut run = windy_run(10_000);
        run.game.athletes[1].as_mut().unwrap().marks[1] = Mark::Landed(12_000);
        run.game.athletes[2].as_mut().unwrap().marks[1] = Mark::Landed(10_000);
        step_until(&mut run, |g| g.phase == Phase::Finished);
        assert_eq!(run.game.attempt, ATTEMPTS);
        assert_eq!(
            run.outcome().unwrap().winners().collect::<Vec<_>>(),
            vec![Seat(1)]
        );
    }

    #[test]
    fn harder_bots_jump_further() {
        let average = |difficulty| {
            let mut total = 0;
            for seed in 0..40 {
                let mut run: Run<TripleJump> = Run::new(
                    seed,
                    PlayerSetup::first(1),
                    Format::FreeForAll,
                    RunConfig::default(),
                );
                while run.outcome().is_none() {
                    let inputs = Seat::ALL.map(|s| TripleJump::bot_input(&run.game, s, difficulty));
                    run.step(inputs);
                }
                total += athlete(&run, 0).best().0.unwrap_or(0);
            }
            total / 40
        };
        let (easy, normal, hard) = (
            average(Difficulty::Easy),
            average(Difficulty::Normal),
            average(Difficulty::Hard),
        );
        assert!(hard > normal && normal > easy, "{hard} {normal} {easy}");
    }

    #[test]
    fn passes_all_automated_quality_bars() {
        sim_check::check_all::<TripleJump>(8).unwrap();
    }
}
