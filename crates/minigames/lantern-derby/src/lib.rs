//! Lantern Derby: the fairground horse race. Every thrower has a slanted board of glowing holes.
//! The stick aims, holding A charges the throw on a meter that swings up and back down, and letting
//! go rolls a ball up the board: it drops into the hole it stops on, and the hole's value moves the
//! thrower's runner up the race track. First to the finish wins; at the time limit the furthest
//! along does.
//!
//! In team formats a team shares one runner. In 1v3 the solo is the barker: it doesn't throw, but
//! moves a lid over the three's boards and snaps it shut on a hole for a moment. A ball that
//! bounces off the lid moves the barker's own runner by the hole's value, and the barker races the
//! three to its (nearer) finish.
//!
//! Positions are whole milli-units on a board, so the simulation has no floats: x runs across a
//! board from its middle, y up it from the throwing line.

use sim_core::{
    Button, Difficulty, Format, FormatKind, Input, Inputs, MAX_PLAYERS, Minigame, Outcome,
    PlayerSetup, Rng, Seat,
};

/// The opening countdown, the same in every minigame.
pub const INTRO_TICKS: u16 = sim_core::COUNTDOWN_TICKS;
/// Ticks of racing before time runs out.
pub const RACE_TICKS: u32 = 60 * 60;
/// Points to the finish, per member of a side.
pub const GOAL: u16 = 25;
/// The 1v3 trio's finish...
pub const TRIO_GOAL: u16 = 45;
/// ...and the barker's, which the values of the balls it bounces add up to.
pub const BARKER_GOAL: u16 = 27;
/// A board runs from -HALF_WIDTH to HALF_WIDTH across...
pub const HALF_WIDTH: i32 = 500;
/// ...and from the throwing line (0) up to the top rail.
pub const LENGTH: i32 = 1600;
/// Boards stand this far apart, middle to middle.
pub const PITCH: i32 = 1300;
/// A ball's centre stays this far inside the side rails.
pub const RAIL: i32 = 40;

/// A hole in every board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hole {
    pub x: i32,
    pub y: i32,
    /// A ball stopping this close to the middle drops in.
    pub radius: i32,
    pub value: u8,
}

/// Three 1s near the bottom, two 2s in the middle and a small 3 near the top.
pub const HOLES: [Hole; 6] = [
    Hole {
        x: -300,
        y: 500,
        radius: 100,
        value: 1,
    },
    Hole {
        x: 0,
        y: 500,
        radius: 100,
        value: 1,
    },
    Hole {
        x: 300,
        y: 500,
        radius: 100,
        value: 1,
    },
    Hole {
        x: -160,
        y: 925,
        radius: 85,
        value: 2,
    },
    Hole {
        x: 160,
        y: 925,
        radius: 85,
        value: 2,
    },
    Hole {
        x: 0,
        y: 1300,
        radius: 70,
        value: 3,
    },
];
/// A ball stopping up to this much beyond a hole's radius is on its rim: it drops in or bounces
/// out, more likely in the closer it is.
pub const RIM: i32 = 35;
/// A throw stops up to this far from where it was aimed, either way, across and along.
pub const WOBBLE: i32 = 30;
/// How far the aim leans: the ball moves this many milli-units across per 1000 up.
pub const AIM_MAX: i32 = 700;
/// Each tick the aim moves by the stick's lean (-127..=127) divided by this, so a stick resting a
/// little off centre doesn't move it.
pub const AIM_DIVISOR: i32 = 8;
/// How far up the board a throw goes with the meter empty, and full (past the top).
pub const MIN_POWER: i32 = 250;
pub const MAX_POWER: i32 = 1750;
/// The meter fills over this many ticks, then empties over as many, and so on.
pub const CHARGE_TICKS: u16 = 60;
/// After a ball settles, the next can be charged this much later.
pub const RELOAD_TICKS: u16 = 12;
/// A ball takes this long to drop into a hole...
pub const SINK_TICKS: u16 = 14;
/// ...or to roll back to the gutter after a miss.
pub const RETURN_TICKS: u16 = 24;
/// After the result is decided, the game holds this long before it ends.
pub const DECIDED_TICKS: u16 = 90;
/// The barker's lid moves this far per tick with the stick all the way over.
pub const CURSOR_SPEED: i32 = 50;
/// A snaps the lid onto the nearest hole within this distance.
pub const LID_REACH: i32 = 180;
/// The lid stays shut this long...
pub const SHUT_TICKS: u16 = 75;
/// ...then can be snapped again this much later.
pub const LID_COOLDOWN: u16 = 45;

/// Ticks a ball rolls before it stops at `y`: further takes longer.
pub fn roll_ticks(y: i32) -> u16 {
    (18 + y.clamp(0, LENGTH) / 40) as u16
}

/// The power of a throw let go after charging `t` ticks: up over [`CHARGE_TICKS`], back down
/// over as many, and round again.
pub fn power_at(t: u16) -> i32 {
    let phase = t % (2 * CHARGE_TICKS);
    let filled = if phase <= CHARGE_TICKS {
        phase
    } else {
        2 * CHARGE_TICKS - phase
    };
    MIN_POWER + (MAX_POWER - MIN_POWER) * filled as i32 / CHARGE_TICKS as i32
}

/// Whether the meter is filling after charging `t` ticks.
pub fn rising(t: u16) -> bool {
    t % (2 * CHARGE_TICKS) < CHARGE_TICKS
}

/// Where a throw with this aim and power would stop, before its wobble: up the board by the power
/// and across by the aim, kept inside the rails.
pub fn aimed_at(aim: i32, power: i32) -> (i32, i32) {
    let across = HALF_WIDTH - RAIL;
    ((power * aim / 1000).clamp(-across, across), power)
}

/// The hole whose rim or middle a ball stopping at (x, y) is on, and the distance to its middle.
pub fn hole_at(x: i32, y: i32) -> Option<(u8, i32)> {
    HOLES.iter().enumerate().find_map(|(index, hole)| {
        let distance = distance(x - hole.x, y - hole.y);
        (distance <= hole.radius + RIM).then_some((index as u8, distance))
    })
}

fn distance(dx: i32, dy: i32) -> i32 {
    let squared = (dx as i64 * dx as i64 + dy as i64 * dy as i64) as u64;
    squared.isqrt() as i32
}

/// Where a thrower's ball is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hand {
    /// In the gutter, chargeable once `t` reaches [`RELOAD_TICKS`] (counts on, saturating).
    Ready { t: u16 },
    /// A held for `t` ticks: see [`power_at`].
    Charging { t: u16 },
    /// Rolling up the board to where it stops, which it reaches after [`roll_ticks`].
    Rolling { t: u16, x: i32, y: i32 },
    /// Dropping into `HOLES[hole]`, already counted.
    Sinking { t: u16, hole: u8 },
    /// Rolling back to the gutter from where it stopped.
    Returning { t: u16, x: i32, y: i32 },
}

/// How a ball ended up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shot {
    /// Dropped into `HOLES[hole]`.
    In { hole: u8 },
    /// Caught the rim of `HOLES[hole]` and bounced out.
    Rim { hole: u8 },
    /// Bounced off the barker's lid over `HOLES[hole]`.
    Shut { hole: u8 },
    /// Stopped between holes.
    Short,
    /// Went past the top and hit the rail.
    Wall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Board {
    pub seat: Seat,
    /// Place in the row of boards, left to right.
    pub slot: u8,
    /// Milli-units across per 1000 up; stays where it was left between throws.
    pub aim: i32,
    pub hand: Hand,
    pub throws: u16,
    /// Points this thrower scored (their side's runner counts everyone's).
    pub points: u16,
    /// The last ball's end and the tick it happened on.
    pub last: Option<(u32, Shot)>,
}

/// A side's runner on the race track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Side {
    /// Throwers on this side (the barker throws none).
    pub members: u8,
    pub points: u16,
    /// No goal: no runner.
    pub goal: u16,
}

impl Side {
    pub fn runs(&self) -> bool {
        self.goal > 0
    }

    pub fn home(&self) -> bool {
        self.runs() && self.points >= self.goal
    }

    /// How far along the track the runner is, in ten-thousandths, so runners with different
    /// finishes compare fairly.
    pub fn progress(&self) -> u32 {
        if self.runs() {
            self.points.min(self.goal) as u32 * 10_000 / self.goal as u32
        } else {
            0
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lid {
    Ready,
    /// Shut over `HOLES[hole]` on the board in `slot`.
    Shut {
        slot: u8,
        hole: u8,
        t: u16,
    },
    Cooling {
        t: u16,
    },
}

/// The 1v3 solo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Barker {
    pub seat: Seat,
    /// The lid's position, across the whole row of boards (x from the row's middle).
    pub x: i32,
    pub y: i32,
    pub lid: Lid,
    /// Balls bounced off the lid.
    pub blocks: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Intro {
        t: u16,
    },
    Racing,
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
    /// Ticks of racing so far.
    pub tick: u32,
    pub rng: Rng,
    /// Indexed by seat; `None` for the barker and seats sitting out.
    pub boards: [Option<Board>; MAX_PLAYERS],
    /// Indexed by [`Format::team`].
    pub sides: [Side; MAX_PLAYERS],
    pub barker: Option<Barker>,
    pub outcome: Option<Outcome>,
}

pub struct LanternDerby;

impl State {
    pub fn board(&self, seat: Seat) -> Option<&Board> {
        self.boards[seat.index()].as_ref()
    }

    /// Every board, in seat order.
    pub fn throwers(&self) -> impl Iterator<Item = &Board> + '_ {
        self.boards.iter().flatten()
    }

    pub fn board_count(&self) -> usize {
        self.throwers().count()
    }

    /// The board in `slot`.
    pub fn board_in(&self, slot: u8) -> Option<&Board> {
        self.throwers().find(|b| b.slot == slot)
    }

    /// The middle of the board in `slot`, across the row (0 is the row's middle).
    pub fn slot_x(&self, slot: u8) -> i32 {
        (2 * slot as i32 - (self.board_count() as i32 - 1)) * PITCH / 2
    }

    pub fn side_of(&self, seat: Seat) -> Option<&Side> {
        let team = self.format.team(seat)?;
        Some(&self.sides[team as usize]).filter(|s| s.runs())
    }

    /// Indexes of the sides with a runner, in team order.
    pub fn racing_sides(&self) -> impl Iterator<Item = usize> + '_ {
        (0..MAX_PLAYERS).filter(|t| self.sides[*t].runs())
    }

    pub fn ticks_left(&self) -> u32 {
        RACE_TICKS.saturating_sub(self.tick)
    }

    /// How far the barker's lid can go: over the row of boards.
    pub fn cursor_bounds(&self) -> (i32, i32) {
        let last = self.board_count().saturating_sub(1) as u8;
        (self.slot_x(0) - HALF_WIDTH, self.slot_x(last) + HALF_WIDTH)
    }

    /// The hole the lid would snap onto from (x, y): the nearest within [`LID_REACH`].
    pub fn hole_near(&self, x: i32, y: i32) -> Option<(u8, u8)> {
        let mut best = None;
        for board in self.throwers() {
            let middle = self.slot_x(board.slot);
            for (index, hole) in HOLES.iter().enumerate() {
                let d = distance(x - middle - hole.x, y - hole.y);
                if d <= LID_REACH && best.is_none_or(|(_, _, nearest)| d < nearest) {
                    best = Some((board.slot, index as u8, d));
                }
            }
        }
        best.map(|(slot, hole, _)| (slot, hole))
    }

    /// Whether the lid is shut over `hole` on the board in `slot`.
    pub fn is_shut(&self, slot: u8, hole: u8) -> bool {
        matches!(
            self.barker.map(|b| b.lid),
            Some(Lid::Shut { slot: s, hole: h, .. }) if s == slot && h == hole
        )
    }

    fn step_barker(&mut self, inputs: &Inputs) {
        let Some(mut barker) = self.barker else {
            return;
        };
        let input = inputs.current[barker.seat.index()];
        // Squared, so small leans make fine moves.
        let lean = |v: i8| v as i32 * (v as i32).abs() * CURSOR_SPEED / (127 * 127);
        let (lo, hi) = self.cursor_bounds();
        barker.x = (barker.x + lean(input.stick_x)).clamp(lo, hi);
        barker.y = (barker.y + lean(input.stick_y)).clamp(0, LENGTH);
        barker.lid = match barker.lid {
            Lid::Ready if inputs.just_pressed(barker.seat, Button::A) => {
                match self.hole_near(barker.x, barker.y) {
                    Some((slot, hole)) => Lid::Shut { slot, hole, t: 0 },
                    None => Lid::Ready,
                }
            }
            Lid::Ready => Lid::Ready,
            Lid::Shut { t, .. } if t + 1 >= SHUT_TICKS => Lid::Cooling { t: 0 },
            Lid::Shut { slot, hole, t } => Lid::Shut {
                slot,
                hole,
                t: t + 1,
            },
            Lid::Cooling { t } if t + 1 >= LID_COOLDOWN => Lid::Ready,
            Lid::Cooling { t } => Lid::Cooling { t: t + 1 },
        };
        self.barker = Some(barker);
    }

    fn step_board(&mut self, seat: Seat, inputs: &Inputs) {
        let Some(mut board) = self.boards[seat.index()] else {
            return;
        };
        let input = inputs.current[seat.index()];
        board.aim = (board.aim + input.stick_x as i32 / AIM_DIVISOR).clamp(-AIM_MAX, AIM_MAX);
        let held = input.held(Button::A);
        board.hand = match board.hand {
            Hand::Ready { t } if t >= RELOAD_TICKS && held => Hand::Charging { t: 0 },
            Hand::Ready { t } => Hand::Ready {
                t: t.saturating_add(1),
            },
            Hand::Charging { t } if held => Hand::Charging { t: t + 1 },
            Hand::Charging { t } => {
                board.throws += 1;
                let (x, y) = aimed_at(board.aim, power_at(t));
                let wobble = |rng: &mut Rng| rng.range_u32(0, 2 * WOBBLE as u32) as i32 - WOBBLE;
                let across = HALF_WIDTH - RAIL;
                let x = (x + wobble(&mut self.rng)).clamp(-across, across);
                let y = y + wobble(&mut self.rng);
                Hand::Rolling { t: 0, x, y }
            }
            Hand::Rolling { t, x, y } if t + 1 >= roll_ticks(y) => {
                let shot = self.settle(board.slot, x, y);
                board.last = Some((self.tick, shot));
                match shot {
                    Shot::In { hole } => {
                        let value = HOLES[hole as usize].value as u16;
                        board.points += value;
                        if let Some(team) = self.format.team(seat) {
                            self.sides[team as usize].points += value;
                        }
                        Hand::Sinking { t: 0, hole }
                    }
                    Shot::Wall => Hand::Returning {
                        t: 0,
                        x,
                        y: y.min(LENGTH),
                    },
                    _ => Hand::Returning { t: 0, x, y },
                }
            }
            Hand::Rolling { t, x, y } => Hand::Rolling { t: t + 1, x, y },
            Hand::Sinking { t, .. } if t + 1 >= SINK_TICKS => Hand::Ready { t: 0 },
            Hand::Sinking { t, hole } => Hand::Sinking { t: t + 1, hole },
            Hand::Returning { t, .. } if t + 1 >= RETURN_TICKS => Hand::Ready { t: 0 },
            Hand::Returning { t, x, y } => Hand::Returning { t: t + 1, x, y },
        };
        self.boards[seat.index()] = Some(board);
    }

    /// Where a ball stopping at (x, y) on the board in `slot` ends up.
    fn settle(&mut self, slot: u8, x: i32, y: i32) -> Shot {
        if y > LENGTH {
            return Shot::Wall;
        }
        let Some((hole, distance)) = hole_at(x, y) else {
            return Shot::Short;
        };
        if self.is_shut(slot, hole) {
            if let Some(barker) = &mut self.barker {
                barker.blocks += 1;
                // The barker is team 0.
                self.sides[0].points += HOLES[hole as usize].value as u16;
            }
            return Shot::Shut { hole };
        }
        let radius = HOLES[hole as usize].radius;
        let inside = distance <= radius;
        // On the rim: in with a chance that falls from 100% at the hole's edge to 0 at the rim's.
        if inside
            || self
                .rng
                .chance(((radius + RIM - distance) * 100 / RIM) as u32)
        {
            Shot::In { hole }
        } else {
            Shot::Rim { hole }
        }
    }

    fn decide_if_over(&mut self) {
        if self.outcome.is_some() {
            return;
        }
        if !self.sides.iter().any(Side::home) && self.tick < RACE_TICKS {
            return;
        }
        let scores = std::array::from_fn(|t| self.sides[t].progress());
        self.outcome = Some(Outcome::from_team_scores(
            &self.players,
            &self.format,
            &scores,
        ));
        self.phase = Phase::Decided { t: 0 };
    }
}

impl Minigame for LanternDerby {
    type State = State;

    const ID: &'static str = "lantern-derby";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[
        FormatKind::FreeForAll,
        FormatKind::OneVsThree,
        FormatKind::TwoVsTwo,
        FormatKind::Duel,
    ];
    const MAX_TICKS: u32 = INTRO_TICKS as u32 + RACE_TICKS + DECIDED_TICKS as u32 + 60;

    fn new(seed: u64, players: PlayerSetup, format: Format) -> State {
        let solo = match format {
            Format::OneVsThree { solo } => Some(solo),
            _ => None,
        };
        // Seats outside a duel have no team and sit this one out; teammates' boards stand
        // together.
        let mut throwers: Vec<(u8, Seat)> = players
            .seats()
            .filter(|s| Some(*s) != solo)
            .filter_map(|s| Some((format.team(s)?, s)))
            .collect();
        throwers.sort();
        let mut boards = [None; MAX_PLAYERS];
        let mut sides = [Side::default(); MAX_PLAYERS];
        for (slot, (team, seat)) in throwers.into_iter().enumerate() {
            boards[seat.index()] = Some(Board {
                seat,
                slot: slot as u8,
                aim: 0,
                hand: Hand::Ready { t: RELOAD_TICKS },
                throws: 0,
                points: 0,
                last: None,
            });
            sides[team as usize].members += 1;
        }
        for side in &mut sides {
            side.goal = if solo.is_some() {
                TRIO_GOAL
            } else {
                GOAL * side.members as u16
            };
        }
        if solo.is_some() {
            // The barker, team 0.
            sides[0].goal = BARKER_GOAL;
        }
        let mut state = State {
            seed,
            players,
            format,
            phase: Phase::Intro { t: 0 },
            tick: 0,
            rng: Rng::new(seed),
            boards,
            sides,
            barker: None,
            outcome: None,
        };
        state.barker = solo.map(|seat| Barker {
            seat,
            x: 0,
            y: HOLES[5].y,
            lid: Lid::Ready,
            blocks: 0,
        });
        state
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
                state.step_barker(inputs);
                for seat in Seat::ALL {
                    state.step_board(seat, inputs);
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
        if state.phase != Phase::Racing || !state.players.is_active(seat) {
            return Input::default();
        }
        if let Some(board) = state.board(seat) {
            bot::throw(state, board, difficulty)
        } else if state.barker.is_some_and(|b| b.seat == seat) {
            bot::bark(state, difficulty)
        } else {
            Input::default()
        }
    }
}

pub mod bot {
    //! Bots throw at a hole they pick for each ball; the barker bot chases the ball worth most.

    use super::*;

    /// How well a bot plays at a difficulty.
    #[derive(Debug, Clone, Copy)]
    pub struct Skill {
        /// Chances in percent of going for a 1, a 2 and the 3.
        pub picks: [u32; 3],
        /// How far off the hole's middle the bot aims, at most, either way.
        pub aim_error: i32,
        /// How far off the hole's middle the bot's power is, at most, either way.
        pub power_error: i32,
        /// Ticks the bot waits once a ball is ready, before charging.
        pub pause: u16,
        /// Ticks the barker watches a ball roll before chasing it.
        pub reaction: u16,
    }

    pub fn skill(difficulty: Difficulty) -> Skill {
        match difficulty {
            Difficulty::Easy => Skill {
                picks: [50, 35, 15],
                aim_error: 85,
                power_error: 150,
                pause: 24,
                reaction: 20,
            },
            Difficulty::Normal => Skill {
                picks: [25, 45, 30],
                aim_error: 65,
                power_error: 115,
                pause: 18,
                reaction: 16,
            },
            Difficulty::Hard => Skill {
                picks: [5, 35, 60],
                aim_error: 45,
                power_error: 80,
                pause: 10,
                reaction: 8,
            },
        }
    }

    /// What a bot is going for with its next ball: where it aims and how much power it wants.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Plan {
        pub hole: u8,
        pub aim: i32,
        pub power: i32,
    }

    /// One plan per ball, so the bot stays a pure function of the state: drawn from the seed, the
    /// seat and how many balls it has thrown.
    pub fn plan(state: &State, board: &Board, difficulty: Difficulty) -> Plan {
        let skill = skill(difficulty);
        let mut rng = Rng::new(
            state.seed
                ^ (board.seat.0 as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15)
                ^ (board.throws as u64 + 1).wrapping_mul(0xbf58_476d_1ce4_e5b9),
        );
        let roll = rng.range_u32(0, 99);
        let value = if roll < skill.picks[0] {
            1
        } else if roll < skill.picks[0] + skill.picks[1] {
            2
        } else {
            3
        };
        let choices: Vec<u8> = (0..HOLES.len() as u8)
            .filter(|h| HOLES[*h as usize].value == value)
            .collect();
        let mut hole = choices[rng.range_u32(0, choices.len() as u32 - 1) as usize];
        if difficulty != Difficulty::Easy && guarded(state, board.slot, hole) {
            // Another hole worth as much if there is one, else one worth less.
            hole = (0..HOLES.len() as u8)
                .rev()
                .filter(|h| !guarded(state, board.slot, *h))
                .find(|h| HOLES[*h as usize].value <= value)
                .unwrap_or(hole);
        }
        let error = |rng: &mut Rng, most: i32| rng.range_u32(0, 2 * most as u32) as i32 - most;
        let target = HOLES[hole as usize];
        let x = target.x + error(&mut rng, skill.aim_error);
        let power = target.y + error(&mut rng, skill.power_error);
        Plan {
            hole,
            aim: (x * 1000 / target.y).clamp(-AIM_MAX, AIM_MAX),
            power,
        }
    }

    /// Whether the barker's lid is shut over a hole, or hovering near enough to snap onto it.
    fn guarded(state: &State, slot: u8, hole: u8) -> bool {
        let Some(barker) = state.barker else {
            return false;
        };
        state.is_shut(slot, hole) || state.hole_near(barker.x, barker.y) == Some((slot, hole))
    }

    pub fn throw(state: &State, board: &Board, difficulty: Difficulty) -> Input {
        let plan = plan(state, board, difficulty);
        let off = plan.aim - board.aim;
        let mut input = Input {
            stick_x: (off * AIM_DIVISOR).clamp(-127, 127) as i8,
            ..Input::default()
        };
        let charge = match board.hand {
            Hand::Ready { t } => t >= RELOAD_TICKS + skill(difficulty).pause && off == 0,
            // Let go on the tick the meter passes the power on its way up.
            Hand::Charging { t } => !(rising(t) && power_at(t) >= plan.power),
            _ => false,
        };
        input.set(Button::A, charge);
        input
    }

    /// The trio's ball the barker goes after: the one heading for the hole worth most (then the
    /// soonest there), once it has watched it roll for its reaction time.
    pub fn chase(state: &State, difficulty: Difficulty) -> Option<(u8, u8)> {
        let reaction = skill(difficulty).reaction;
        state
            .throwers()
            .filter_map(|board| match board.hand {
                Hand::Rolling { t, x, y } if t >= reaction => {
                    let (hole, _) = hole_at(x, y)?;
                    let left = roll_ticks(y) - t;
                    Some((HOLES[hole as usize].value, left, board.slot, hole))
                }
                _ => None,
            })
            .max_by_key(|(value, left, slot, _)| (*value, u16::MAX - left, u8::MAX - slot))
            .map(|(_, _, slot, hole)| (slot, hole))
    }

    pub fn bark(state: &State, difficulty: Difficulty) -> Input {
        let Some(barker) = state.barker else {
            return Input::default();
        };
        let chased = chase(state, difficulty);
        // Nothing to chase: wait by the board whose ball has charged longest, over the 3 or (Hard)
        // the hole worth most that its aim runs through.
        let (slot, hole) = chased.unwrap_or_else(|| {
            let Some(board) = state.throwers().max_by_key(|b| match b.hand {
                Hand::Charging { t } => t as i32 + 1,
                _ => -(b.slot as i32 - 1).abs(),
            }) else {
                return (0, 5);
            };
            let aimed = (0..HOLES.len() as u8).rev().find(|h| {
                let hole = HOLES[*h as usize];
                (hole.y * board.aim / 1000 - hole.x).abs() <= hole.radius
            });
            match aimed {
                Some(hole) if difficulty == Difficulty::Hard => (board.slot, hole),
                _ => (board.slot, 5),
            }
        });
        let target = HOLES[hole as usize];
        let (tx, ty) = (state.slot_x(slot) + target.x, target.y);
        // The squared lean that moves exactly the way there, as far as the stick goes.
        let lean = |off: i32| {
            let wanted =
                (off.unsigned_abs() as u64 * 127 * 127 / CURSOR_SPEED as u64).min(127 * 127);
            let mut lean = wanted.isqrt();
            if lean * lean < wanted {
                lean += 1;
            }
            (off.signum() * lean as i32) as i8
        };
        let mut input = Input {
            stick_x: lean(tx - barker.x),
            stick_y: lean(ty - barker.y),
            ..Input::default()
        };
        let snap = chased.is_some()
            && barker.lid == Lid::Ready
            && state.hole_near(barker.x, barker.y) == Some((slot, hole));
        input.set(Button::A, snap);
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

    fn new_run(players: usize, format: Format) -> Run<LanternDerby> {
        Run::new(7, PlayerSetup::first(players), format, RunConfig::default())
    }

    fn step_until(run: &mut Run<LanternDerby>, done: impl Fn(&State) -> bool) {
        for _ in 0..LanternDerby::MAX_TICKS {
            if done(&run.game) {
                return;
            }
            run.step(NONE);
        }
        panic!("never reached");
    }

    fn racing(players: usize, format: Format) -> Run<LanternDerby> {
        let mut run = new_run(players, format);
        step_until(&mut run, |g| g.phase == Phase::Racing);
        run
    }

    fn hand(run: &Run<LanternDerby>, seat: usize) -> Hand {
        run.game.boards[seat].unwrap().hand
    }

    /// Throws `seat`'s next ball with exactly this aim and power, and waits for it to stop.
    fn throw(run: &mut Run<LanternDerby>, seat: usize, aim: i32, power: i32) -> Shot {
        step_until(
            run,
            |g| matches!(g.boards[seat].unwrap().hand, Hand::Ready { t } if t >= RELOAD_TICKS),
        );
        run.game.boards[seat].as_mut().unwrap().aim = aim;
        let mut inputs = NONE;
        inputs[seat] = Input::default().with(Button::A);
        run.step(inputs);
        assert!(matches!(hand(run, seat), Hand::Charging { t: 0 }));
        // Charge to the power's tick (the tests use powers on the meter's steps).
        let t = (0..=CHARGE_TICKS)
            .find(|t| power_at(*t) == power)
            .expect("a power the meter reaches");
        run.game.boards[seat].as_mut().unwrap().hand = Hand::Charging { t };
        run.step(NONE);
        let throws = run.game.boards[seat].unwrap().throws;
        step_until(run, |g| {
            !matches!(g.boards[seat].unwrap().hand, Hand::Rolling { .. })
        });
        assert_eq!(run.game.boards[seat].unwrap().throws, throws);
        run.game.boards[seat].unwrap().last.unwrap().1
    }

    /// The aim and power that roll a ball to a hole's middle (give or take the wobble).
    fn at_hole(hole: usize) -> (i32, i32) {
        let hole = HOLES[hole];
        (hole.x * 1000 / hole.y, hole.y)
    }

    #[test]
    fn the_meter_swings_up_and_back_down() {
        assert_eq!(power_at(0), MIN_POWER);
        assert_eq!(power_at(CHARGE_TICKS), MAX_POWER);
        assert_eq!(power_at(2 * CHARGE_TICKS), MIN_POWER);
        assert!(rising(1) && !rising(CHARGE_TICKS + 1) && rising(2 * CHARGE_TICKS));
        const { assert!(MAX_POWER > LENGTH, "a full meter throws past the top") };
        // Every hole's middle is a step of the meter, so tests can throw at it exactly.
        for hole in HOLES {
            assert!((0..CHARGE_TICKS).any(|t| power_at(t) == hole.y));
        }
    }

    #[test]
    fn a_ball_in_a_hole_moves_the_runner_by_its_value() {
        let mut run = racing(2, Format::FreeForAll);
        for (hole, total) in [(5, 3), (3, 5), (1, 6)] {
            let (aim, power) = at_hole(hole);
            assert_eq!(
                throw(&mut run, 0, aim, power),
                Shot::In { hole: hole as u8 }
            );
            assert_eq!(run.game.boards[0].unwrap().points, total);
            assert_eq!(run.game.sides[0].points, total);
        }
        assert_eq!(run.game.sides[1].points, 0);
        assert!(matches!(hand(&run, 0), Hand::Sinking { .. }));
    }

    #[test]
    fn a_ball_between_holes_or_past_the_top_scores_nothing() {
        let mut run = racing(2, Format::FreeForAll);
        // Straight up, between the 1 and the 2s, then the 2s and the 3.
        assert_eq!(throw(&mut run, 0, 0, 750), Shot::Short);
        assert!(matches!(hand(&run, 0), Hand::Returning { .. }));
        assert_eq!(throw(&mut run, 0, 0, 1100), Shot::Short);
        assert_eq!(throw(&mut run, 0, 0, MAX_POWER), Shot::Wall);
        assert_eq!(run.game.sides[0].points, 0);
    }

    #[test]
    fn a_ball_on_the_rim_drops_in_or_bounces_out() {
        let (mut ins, mut outs) = (0, 0);
        for seed in 0..200 {
            let mut game = LanternDerby::new(seed, PlayerSetup::first(2), Format::FreeForAll);
            let hole = HOLES[1];
            match game.settle(0, hole.x + hole.radius + RIM / 2, hole.y) {
                Shot::In { hole: 1 } => ins += 1,
                Shot::Rim { hole: 1 } => outs += 1,
                other => panic!("{other:?}"),
            }
        }
        assert!(ins > 60 && outs > 60, "{ins} {outs}");
        let mut game = LanternDerby::new(0, PlayerSetup::first(2), Format::FreeForAll);
        assert_eq!(game.settle(0, HOLES[1].radius + RIM + 1, 500), Shot::Short);
    }

    #[test]
    fn the_next_ball_comes_once_the_last_has_settled() {
        let mut run = racing(2, Format::FreeForAll);
        let (aim, power) = at_hole(1);
        throw(&mut run, 0, aim, power);
        // Holding A while it sinks charges nothing...
        let mut held = NONE;
        held[0] = Input::default().with(Button::A);
        for _ in 0..SINK_TICKS + RELOAD_TICKS - 1 {
            run.step(held);
            assert!(!matches!(hand(&run, 0), Hand::Charging { .. }));
        }
        // ...until the next ball is ready.
        run.step(held);
        run.step(held);
        assert!(matches!(hand(&run, 0), Hand::Charging { .. }));
    }

    #[test]
    fn the_stick_steers_the_aim_which_stays_between_throws() {
        let mut run = racing(2, Format::FreeForAll);
        let mut inputs = NONE;
        inputs[0].stick_x = 127;
        for _ in 0..10 {
            run.step(inputs);
        }
        assert_eq!(run.game.boards[0].unwrap().aim, 10 * (127 / AIM_DIVISOR));
        // A stick resting a little off centre leaves it be.
        inputs[0].stick_x = 6;
        run.step(inputs);
        assert_eq!(run.game.boards[0].unwrap().aim, 10 * (127 / AIM_DIVISOR));
        inputs[0].stick_x = -127;
        for _ in 0..200 {
            run.step(inputs);
        }
        assert_eq!(run.game.boards[0].unwrap().aim, -AIM_MAX);
        // A ball thrown wide runs along the rail.
        let shot = throw(&mut run, 0, -AIM_MAX, 1300);
        assert_eq!(shot, Shot::Short);
        let Hand::Returning { x, .. } = hand(&run, 0) else {
            panic!()
        };
        assert!(x <= -(HALF_WIDTH - RAIL) + WOBBLE, "{x}");
        assert_eq!(run.game.boards[0].unwrap().aim, -AIM_MAX);
    }

    #[test]
    fn first_to_the_finish_wins() {
        let mut run = racing(3, Format::FreeForAll);
        let (aim, power) = at_hole(5);
        while run.game.sides[1].points < GOAL {
            throw(&mut run, 1, aim, power);
        }
        step_until(&mut run, |g| g.phase == Phase::Finished);
        let outcome = run.outcome().unwrap();
        assert_eq!(outcome.winners().collect::<Vec<_>>(), vec![Seat(1)]);
    }

    #[test]
    fn time_up_ranks_by_points() {
        let mut run = racing(3, Format::FreeForAll);
        throw(&mut run, 0, at_hole(0).0, at_hole(0).1);
        throw(&mut run, 2, at_hole(5).0, at_hole(5).1);
        step_until(&mut run, |g| g.phase == Phase::Finished);
        assert_eq!(run.game.tick, RACE_TICKS);
        assert_eq!(
            run.outcome().unwrap().placement,
            [Some(2), Some(3), Some(1), None]
        );
    }

    #[test]
    fn teammates_share_a_runner() {
        let format = Format::TwoVsTwo {
            team_of: [0, 1, 1, 0],
        };
        let mut run = racing(4, format);
        assert_eq!(run.game.sides[0].goal, 2 * GOAL);
        // Teammates' boards stand together.
        let slots: Vec<u8> = (0..4).map(|s| run.game.boards[s].unwrap().slot).collect();
        assert_eq!(slots, [0, 2, 3, 1]);
        throw(&mut run, 0, at_hole(5).0, at_hole(5).1);
        throw(&mut run, 3, at_hole(3).0, at_hole(3).1);
        assert_eq!(run.game.sides[0].points, 5);
        assert_eq!(run.game.sides[1].points, 0);
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
        assert!(run.game.board(Seat(0)).is_none());
        assert_eq!(run.game.board_count(), 2);
        assert_eq!(run.game.racing_sides().count(), 2);
    }

    #[test]
    fn nothing_moves_during_the_countdown() {
        let mut run = new_run(2, Format::FreeForAll);
        let mut inputs = NONE;
        inputs[0] = Input {
            stick_x: 127,
            ..Input::default().with(Button::A)
        };
        for _ in 0..20 {
            run.step(inputs);
        }
        assert!(matches!(run.game.phase, Phase::Intro { .. }));
        assert_eq!(run.game.boards[0].unwrap().aim, 0);
        assert!(matches!(hand(&run, 0), Hand::Ready { .. }));
    }

    fn solo_run() -> Run<LanternDerby> {
        racing(4, Format::OneVsThree { solo: Seat(1) })
    }

    /// Moves the barker's lid to (x, y) and snaps it with A.
    fn snap(run: &mut Run<LanternDerby>, x: i32, y: i32) {
        let barker = run.game.barker.as_mut().unwrap();
        barker.x = x;
        barker.y = y;
        let mut inputs = NONE;
        inputs[1] = Input::default().with(Button::A);
        run.step(inputs);
        run.step(NONE);
    }

    #[test]
    fn the_barker_throws_nothing_and_the_trio_shares_a_runner() {
        let run = solo_run();
        assert!(run.game.board(Seat(1)).is_none());
        assert_eq!(run.game.board_count(), 3);
        assert_eq!(run.game.sides[1].goal, TRIO_GOAL);
        assert_eq!(run.game.sides[0].members, 0);
        assert_eq!(run.game.sides[0].goal, BARKER_GOAL);
        assert_eq!(run.game.slot_x(0), -PITCH);
    }

    #[test]
    fn a_shut_lid_bounces_the_ball_out() {
        let mut run = solo_run();
        // P4's board is on the right; aim the lid a little off its 3.
        let slot = run.game.boards[3].unwrap().slot;
        let x = run.game.slot_x(slot) + HOLES[5].x + 50;
        snap(&mut run, x, HOLES[5].y - 60);
        assert!(run.game.is_shut(slot, 5));
        let (aim, power) = at_hole(5);
        assert_eq!(throw(&mut run, 3, aim, power), Shot::Shut { hole: 5 });
        assert_eq!(run.game.sides[1].points, 0);
        assert_eq!(run.game.barker.unwrap().blocks, 1);
        // The barker's runner moves by the hole's value.
        assert_eq!(run.game.sides[0].points, 3);
        // Other boards' 3s are open.
        assert_eq!(throw(&mut run, 0, aim, power), Shot::In { hole: 5 });
    }

    #[test]
    fn the_lid_opens_again_and_must_cool_down() {
        let mut run = solo_run();
        // Out of reach of every hole: nothing happens.
        snap(&mut run, 0, LENGTH);
        assert_eq!(run.game.barker.unwrap().lid, Lid::Ready);
        snap(&mut run, 0, HOLES[1].y);
        assert!(run.game.is_shut(1, 1));
        step_until(&mut run, |g| {
            matches!(g.barker.unwrap().lid, Lid::Cooling { .. })
        });
        snap(&mut run, 0, HOLES[1].y);
        assert!(matches!(run.game.barker.unwrap().lid, Lid::Cooling { .. }));
        step_until(&mut run, |g| g.barker.unwrap().lid == Lid::Ready);
    }

    #[test]
    fn the_lid_stays_over_the_boards() {
        let mut run = solo_run();
        let mut inputs = NONE;
        inputs[1] = Input {
            stick_x: 127,
            stick_y: -127,
            ..Input::default()
        };
        for _ in 0..200 {
            run.step(inputs);
        }
        let barker = run.game.barker.unwrap();
        assert_eq!((barker.x, barker.y), (PITCH + HALF_WIDTH, 0));
    }

    #[test]
    fn the_trio_wins_at_its_finish() {
        let mut run = solo_run();
        let (aim, power) = at_hole(5);
        while run.game.sides[1].points < TRIO_GOAL {
            throw(&mut run, 2, aim, power);
        }
        step_until(&mut run, |g| g.phase == Phase::Finished);
        assert_eq!(
            run.outcome().unwrap().placement,
            [Some(1), Some(2), Some(1), Some(1)]
        );
    }

    #[test]
    fn the_barker_wins_at_its_finish() {
        let mut run = solo_run();
        let (aim, power) = at_hole(5);
        while run.game.sides[0].points < BARKER_GOAL {
            let slot = run.game.boards[2].unwrap().slot;
            let x = run.game.slot_x(slot) + HOLES[5].x;
            step_until(&mut run, |g| g.barker.unwrap().lid == Lid::Ready);
            snap(&mut run, x, HOLES[5].y);
            assert_eq!(throw(&mut run, 2, aim, power), Shot::Shut { hole: 5 });
        }
        step_until(&mut run, |g| g.phase == Phase::Finished);
        assert_eq!(
            run.outcome().unwrap().winners().collect::<Vec<_>>(),
            vec![Seat(1)]
        );
    }

    #[test]
    fn at_time_up_the_runner_furthest_along_its_track_wins() {
        let mut run = solo_run();
        // The barker's 3 of its 27 beats the trio's 4 of their 45.
        const { assert!(3 * TRIO_GOAL > 4 * BARKER_GOAL) };
        throw(&mut run, 0, at_hole(5).0, at_hole(5).1);
        throw(&mut run, 0, at_hole(1).0, at_hole(1).1);
        let x = run.game.slot_x(2) + HOLES[5].x;
        snap(&mut run, x, HOLES[5].y);
        throw(&mut run, 3, at_hole(5).0, at_hole(5).1);
        step_until(&mut run, |g| g.phase == Phase::Finished);
        assert_eq!(run.game.tick, RACE_TICKS);
        assert_eq!(
            run.outcome().unwrap().winners().collect::<Vec<_>>(),
            vec![Seat(1)]
        );
    }

    #[test]
    fn harder_bots_score_faster() {
        let finish = |difficulty| {
            let mut total = 0;
            for seed in 0..20 {
                let mut run: Run<LanternDerby> = Run::new(
                    seed,
                    PlayerSetup::first(1),
                    Format::FreeForAll,
                    RunConfig::default(),
                );
                while run.outcome().is_none() {
                    let inputs =
                        Seat::ALL.map(|s| LanternDerby::bot_input(&run.game, s, difficulty));
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
        sim_check::check_all::<LanternDerby>(8).unwrap();
    }
}
