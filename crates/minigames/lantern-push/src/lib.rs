//! Lantern Push: sumo on a round platform that shrinks as the game goes on.
//! A dashes into whoever is in the way and knocks them back; B braces, which makes you heavy
//! enough that a dash bounces off you. Fall off the edge and you're out. The last player (or team)
//! standing wins; if time runs out, everyone still standing shares the win.

use sim_core::physics::{self, Body, Vec2};
use sim_core::{
    Button, Difficulty, Format, FormatKind, Input, Inputs, MAX_PLAYERS, Minigame, Outcome,
    PlayerSetup, Rng, Seat,
};

/// The opening countdown, the same in every minigame.
pub const INTRO_TICKS: u16 = sim_core::COUNTDOWN_TICKS;
/// Ticks of play before time runs out.
pub const PLAY_TICKS: u32 = 75 * 60;
/// The platform starts shrinking after this many ticks of play...
pub const SHRINK_START: u32 = 8 * 60;
/// ...and reaches its smallest size here.
pub const SHRINK_END: u32 = 45 * 60;
pub const START_RADIUS: f32 = 7.0;
pub const MIN_RADIUS: f32 = 2.2;
/// After the result is decided, the game keeps simulating this long so the last fall plays out.
pub const DECIDED_TICKS: u16 = 120;
/// How long a ringed-out fighter falls before they're gone.
pub const FALL_TICKS: u16 = 50;

pub const RADIUS: f32 = 0.5;
/// The solo player in 1v3 is bigger and heavier, so three can't simply bully them off.
pub const SOLO_RADIUS: f32 = 0.58;
pub const SOLO_MASS: f32 = 1.35;
pub const MOVE_SPEED: f32 = 5.0;
pub const ACCEL: f32 = 32.0;
pub const DRAG: f32 = 3.2;
pub const DASH_SPEED: f32 = 12.0;
pub const DASH_TICKS: u16 = 10;
pub const DASH_COOLDOWN: u16 = 66;
/// Extra speed a dash hit adds to its victim, on top of the momentum it hands over.
pub const KNOCKBACK: f32 = 5.5;
pub const STUN_TICKS: u16 = 20;
pub const BRACE_MAX_TICKS: u16 = 45;
pub const BRACE_COOLDOWN: u16 = 90;
/// Bracing multiplies your mass by this.
pub const BRACE_MASS: f32 = 4.0;
const RESTITUTION: f32 = 0.3;

pub struct LanternPush;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Standing,
    /// Went over the edge `t` ticks ago.
    Falling {
        t: u16,
    },
    Out,
}

#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct Fighter {
    pub body: Body,
    /// Unit vector the fighter faces: their last stick direction.
    pub facing: Vec2,
    pub status: Status,
    /// Ticks left in the current dash.
    pub dash: u16,
    pub dash_cooldown: u16,
    /// Ticks spent braced so far (0 = not bracing).
    pub brace: u16,
    pub brace_cooldown: u16,
    /// Ticks left reeling from a hit, unable to steer.
    pub stun: u16,
    /// Play tick of the last time this fighter was knocked back, for hit effects.
    pub last_hit: Option<u32>,
    /// Who hit them last; credited when they fall.
    pub last_hit_by: Option<Seat>,
}

impl Fighter {
    pub fn standing(&self) -> bool {
        self.status == Status::Standing
    }

    pub fn bracing(&self) -> bool {
        self.brace > 0
    }

    fn effective_mass(&self) -> f32 {
        if self.bracing() {
            self.body.mass * BRACE_MASS
        } else {
            self.body.mass
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Intro {
        t: u16,
    },
    Playing,
    /// The winner is known; physics runs on for a moment so falls finish.
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
    /// Ticks of play so far (the platform's size and the clock derive from this).
    pub tick: u32,
    pub fighters: [Fighter; MAX_PLAYERS],
    /// Play tick at which each seat went over the edge.
    pub fell_at: [Option<u32>; MAX_PLAYERS],
    /// Set when the game is decided.
    pub outcome: Option<Outcome>,
}

/// Platform radius after `tick` ticks of play.
pub fn platform_radius(tick: u32) -> f32 {
    if tick <= SHRINK_START {
        START_RADIUS
    } else if tick >= SHRINK_END {
        MIN_RADIUS
    } else {
        let t = (tick - SHRINK_START) as f32 / (SHRINK_END - SHRINK_START) as f32;
        START_RADIUS + (MIN_RADIUS - START_RADIUS) * t
    }
}

impl State {
    pub fn radius(&self) -> f32 {
        platform_radius(self.tick)
    }

    pub fn ticks_left(&self) -> u32 {
        PLAY_TICKS.saturating_sub(self.tick)
    }

    pub fn fighter(&self, seat: Seat) -> &Fighter {
        &self.fighters[seat.index()]
    }

    /// Seats still on the platform.
    pub fn standing(&self) -> impl Iterator<Item = Seat> + '_ {
        self.players
            .seats()
            .filter(|s| self.fighters[s.index()].standing())
    }

    fn teams_standing(&self) -> usize {
        let mut teams = [false; MAX_PLAYERS];
        for seat in self.standing() {
            if let Some(team) = self.format.team(seat) {
                teams[team as usize] = true;
            }
        }
        teams.iter().filter(|t| **t).count()
    }

    /// Each team's score: standing members count most, then how late its last member fell.
    fn team_scores(&self) -> [u32; MAX_PLAYERS] {
        let mut standing = [0u32; MAX_PLAYERS];
        let mut latest_fall = [0u32; MAX_PLAYERS];
        for seat in self.players.seats() {
            let Some(team) = self.format.team(seat) else {
                continue;
            };
            match self.fell_at[seat.index()] {
                None => standing[team as usize] += 1,
                Some(tick) => latest_fall[team as usize] = latest_fall[team as usize].max(tick + 1),
            }
        }
        std::array::from_fn(|team| standing[team] * 1_000_000 + latest_fall[team])
    }

    fn step_fighters(&mut self, inputs: &Inputs) {
        for seat in self.players.seats() {
            let fighter = &mut self.fighters[seat.index()];
            if !fighter.standing() {
                continue;
            }
            let stick = Vec2::from_stick(&inputs.current[seat.index()]);
            fighter.dash = fighter.dash.saturating_sub(1);
            fighter.dash_cooldown = fighter.dash_cooldown.saturating_sub(1);
            fighter.brace_cooldown = fighter.brace_cooldown.saturating_sub(1);
            fighter.stun = fighter.stun.saturating_sub(1);
            if stick != Vec2::ZERO && fighter.stun == 0 {
                fighter.facing = stick.normalize_or_zero();
            }

            let can_act = fighter.stun == 0 && fighter.dash == 0;
            if fighter.bracing() {
                if inputs.held(seat, Button::B) && fighter.brace < BRACE_MAX_TICKS {
                    fighter.brace += 1;
                } else {
                    fighter.brace = 0;
                    fighter.brace_cooldown = BRACE_COOLDOWN;
                }
            } else if can_act && fighter.brace_cooldown == 0 && inputs.just_pressed(seat, Button::B)
            {
                fighter.brace = 1;
            }

            if can_act
                && !fighter.bracing()
                && fighter.dash_cooldown == 0
                && inputs.just_pressed(seat, Button::A)
            {
                fighter.dash = DASH_TICKS;
                fighter.dash_cooldown = DASH_COOLDOWN;
                fighter.body.vel = fighter.facing * DASH_SPEED;
            }

            if fighter.bracing() {
                fighter.body.steer(Vec2::ZERO, 0.0, ACCEL * 3.0);
            } else if fighter.stun == 0 && fighter.dash == 0 {
                fighter.body.steer(stick, MOVE_SPEED, ACCEL);
            }
            let drag = if fighter.dash > 0 { 0.0 } else { DRAG };
            fighter.body.integrate(drag);
        }
        self.collide();
    }

    fn collide(&mut self) {
        let seats: Vec<Seat> = self.standing().collect();
        let mut bodies: Vec<Body> = seats
            .iter()
            .map(|s| {
                let fighter = &self.fighters[s.index()];
                Body {
                    mass: fighter.effective_mass(),
                    ..fighter.body
                }
            })
            .collect();
        let mut contacts = Vec::new();
        physics::collide_all(&mut bodies, RESTITUTION, |i, j, contact| {
            contacts.push((i, j, contact.normal))
        });
        for (seat, body) in seats.iter().zip(&bodies) {
            let fighter = &mut self.fighters[seat.index()];
            fighter.body.pos = body.pos;
            fighter.body.vel = body.vel;
        }
        for (i, j, normal) in contacts {
            let (a, b) = (seats[i], seats[j]);
            // A dash only counts if it was heading into the other fighter.
            let a_hits = self.fighters[a.index()].dash > 0
                && self.fighters[a.index()].facing.dot(normal) > 0.3;
            let b_hits = self.fighters[b.index()].dash > 0
                && self.fighters[b.index()].facing.dot(-normal) > 0.3;
            if a_hits {
                self.dash_hit(a, b, normal);
            }
            if b_hits {
                self.dash_hit(b, a, -normal);
            }
        }
    }

    /// `attacker` dashed into `victim`; `normal` points from attacker to victim.
    fn dash_hit(&mut self, attacker: Seat, victim: Seat, normal: Vec2) {
        let tick = self.tick;
        self.fighters[attacker.index()].dash = 0;
        if self.fighters[victim.index()].bracing() {
            // A braced fighter is a wall: the attacker rebounds and reels.
            let fighter = &mut self.fighters[attacker.index()];
            fighter.body.vel = -normal * KNOCKBACK;
            fighter.stun = STUN_TICKS;
            fighter.last_hit = Some(tick);
            fighter.last_hit_by = Some(victim);
        } else {
            let fighter = &mut self.fighters[victim.index()];
            fighter.body.vel += normal * (KNOCKBACK / fighter.body.mass);
            fighter.stun = STUN_TICKS;
            fighter.dash = 0;
            fighter.last_hit = Some(tick);
            fighter.last_hit_by = Some(attacker);
        }
    }

    /// Move fallers along; with `eliminate`, anyone past the edge starts falling.
    fn ring_out(&mut self, eliminate: bool) {
        let radius = self.radius();
        for seat in self.players.seats() {
            let fighter = &mut self.fighters[seat.index()];
            match fighter.status {
                Status::Standing if eliminate && fighter.body.pos.length() > radius => {
                    fighter.status = Status::Falling { t: 0 };
                    fighter.dash = 0;
                    fighter.brace = 0;
                    self.fell_at[seat.index()] = Some(self.tick);
                }
                Status::Falling { t } if t + 1 >= FALL_TICKS => fighter.status = Status::Out,
                Status::Falling { t } => {
                    fighter.status = Status::Falling { t: t + 1 };
                    fighter.body.integrate(0.5);
                }
                _ => {}
            }
        }
    }

    fn decide_if_over(&mut self) {
        if self.outcome.is_none() && (self.teams_standing() <= 1 || self.tick >= PLAY_TICKS) {
            self.outcome = Some(Outcome::from_team_scores(
                &self.players,
                &self.format,
                &self.team_scores(),
            ));
            self.phase = Phase::Decided { t: 0 };
        }
    }
}

fn start_position(index: usize, count: usize) -> Vec2 {
    // Spread evenly around the platform, the first seat nearest the camera.
    let angle = std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * index as f32 / count as f32;
    Vec2::from_angle(angle) * (START_RADIUS * 0.55)
}

impl Minigame for LanternPush {
    type State = State;

    const ID: &'static str = "lantern-push";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[
        FormatKind::FreeForAll,
        FormatKind::OneVsThree,
        FormatKind::TwoVsTwo,
        FormatKind::Duel,
    ];
    const MAX_TICKS: u32 = INTRO_TICKS as u32 + PLAY_TICKS + DECIDED_TICKS as u32 + 60;

    fn new(seed: u64, players: PlayerSetup, format: Format) -> State {
        let count = players.count();
        let mut fighters = [Fighter {
            body: Body::new(Vec2::ZERO, RADIUS, 1.0),
            facing: Vec2::new(0.0, -1.0),
            status: Status::Out,
            dash: 0,
            dash_cooldown: 0,
            brace: 0,
            brace_cooldown: 0,
            stun: 0,
            last_hit: None,
            last_hit_by: None,
        }; MAX_PLAYERS];
        for (i, seat) in players.seats().enumerate() {
            let fighter = &mut fighters[seat.index()];
            let pos = start_position(i, count);
            let solo = matches!(format, Format::OneVsThree { solo } if solo == seat);
            fighter.body = if solo {
                Body::new(pos, SOLO_RADIUS, SOLO_MASS)
            } else {
                Body::new(pos, RADIUS, 1.0)
            };
            fighter.facing = (-pos).normalize_or_zero();
            // Seats outside a duel sit this one out.
            if format.team(seat).is_some() {
                fighter.status = Status::Standing;
            }
        }
        State {
            seed,
            players,
            format,
            phase: Phase::Intro { t: 0 },
            tick: 0,
            fighters,
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
                state.step_fighters(inputs);
                state.ring_out(true);
                state.decide_if_over();
            }
            Phase::Decided { t } => {
                // Winners stand still and celebrate; only the falls keep going.
                state.step_fighters(&Inputs::default());
                state.ring_out(false);
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
        if state.phase != Phase::Playing || !state.fighter(seat).standing() {
            return Input::default();
        }
        bot::input(state, seat, difficulty)
    }
}

mod bot {
    use super::*;

    struct Skill {
        /// Ticks between fresh decisions (aim noise and dash rolls are redrawn each window).
        reaction: u32,
        /// Aim error in radians, either way.
        aim_noise: f32,
        /// Distance from the edge at which the bot retreats to the middle.
        edge_margin: f32,
        /// Chance per decision window of taking a good dash.
        dash_percent: u32,
        /// Chance per decision window of bracing against an incoming dash.
        brace_percent: u32,
        /// Checks that a dash won't carry it off the platform.
        careful: bool,
    }

    fn skill(difficulty: Difficulty) -> Skill {
        match difficulty {
            Difficulty::Easy => Skill {
                reaction: 24,
                aim_noise: 0.55,
                edge_margin: 0.7,
                dash_percent: 35,
                brace_percent: 10,
                careful: false,
            },
            Difficulty::Normal => Skill {
                reaction: 14,
                aim_noise: 0.3,
                edge_margin: 1.0,
                dash_percent: 70,
                brace_percent: 45,
                careful: true,
            },
            Difficulty::Hard => Skill {
                reaction: 8,
                aim_noise: 0.12,
                edge_margin: 1.3,
                dash_percent: 95,
                brace_percent: 80,
                careful: true,
            },
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

    pub fn input(state: &State, seat: Seat, difficulty: Difficulty) -> Input {
        let skill = skill(difficulty);
        let me = state.fighter(seat);
        let window = state.tick / skill.reaction;
        let mut rng = Rng::new(
            state.seed
                ^ (window as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
                ^ (seat.0 as u64 + 1).wrapping_mul(0xbf58_476d_1ce4_e5b9),
        );
        let aim_error = (rng.range_u32(0, 2000) as f32 / 1000.0 - 1.0) * skill.aim_noise;
        let wants_dash = rng.chance(skill.dash_percent);
        let wants_brace = rng.chance(skill.brace_percent);

        let radius = state.radius();
        let pos = me.body.pos;
        let from_centre = pos.length();

        // Someone dashing straight at us? Brace if we can.
        let threatened = state.standing().any(|other| {
            let them = state.fighter(other);
            other != seat
                && !state.format.same_team(seat, other)
                && them.dash > 0
                && them.body.pos.distance(pos) < 2.2
                && them.facing.dot((pos - them.body.pos).normalize_or_zero()) > 0.8
        });
        if me.bracing() {
            // Hold the brace while the threat lasts.
            let mut input = Input::default();
            input.set(Button::B, threatened);
            return input;
        }
        if threatened && wants_brace && me.brace_cooldown == 0 && me.stun == 0 {
            return Input::default().with(Button::B);
        }

        // Too close to the edge: get back to the middle first.
        if from_centre > radius - skill.edge_margin.min(radius * 0.4) {
            return stick(-pos.normalize_or_zero());
        }

        // Pick the opponent best to push: close to us and close to the edge.
        let target = state
            .standing()
            .filter(|other| *other != seat && !state.format.same_team(seat, *other))
            .min_by(|a, b| {
                let score = |s: &Seat| {
                    let p = state.fighter(*s).body.pos;
                    p.distance(pos) - (p.length() / radius) * 2.5
                };
                score(a).total_cmp(&score(b)).then(a.cmp(b))
            });
        let Some(target) = target else {
            return stick(-pos * 0.3);
        };
        let target_pos = state.fighter(target).body.pos;
        let outward = target_pos.normalize_or_zero();
        let outward = if outward == Vec2::ZERO {
            (target_pos - pos).normalize_or_zero()
        } else {
            outward
        };
        let to_target = target_pos - pos;
        let distance = to_target.length();
        let toward = to_target.normalize_or_zero();
        let aimed = rotate(toward, aim_error);

        // Lined up behind them (we're between them and the middle) and close: dash.
        let lined_up = toward.dot(outward) > 0.55;
        let in_range = distance < 3.0;
        // A dash stops when it connects, so it only travels about as far as the target.
        let dash_reach = (DASH_SPEED * DASH_TICKS as f32 / 60.0).min(distance) + 0.6;
        let landing = pos + aimed * dash_reach;
        let safe = !skill.careful || landing.length() < radius - 0.3;
        if lined_up && in_range && wants_dash && safe && me.dash_cooldown == 0 && me.stun == 0 {
            let mut input = stick(aimed);
            input.set(Button::A, true);
            return input;
        }

        // Otherwise circle round to the side of the target that faces the middle.
        let behind = target_pos - outward * 1.6;
        stick(rotate((behind - pos).normalize_or_zero(), aim_error * 0.5))
    }

    fn rotate(v: Vec2, angle: f32) -> Vec2 {
        let (s, c) = (sim_core::math::sin(angle), sim_core::math::cos(angle));
        Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
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

    fn new_run(players: usize, format: Format) -> Run<LanternPush> {
        Run::new(1, PlayerSetup::first(players), format, RunConfig::default())
    }

    fn to_playing(run: &mut Run<LanternPush>) {
        while run.game.phase != Phase::Playing {
            run.step(NONE);
        }
    }

    fn place(run: &mut Run<LanternPush>, seat: usize, x: f32, y: f32) {
        run.game.fighters[seat].body.pos = Vec2::new(x, y);
        run.game.fighters[seat].body.vel = Vec2::ZERO;
    }

    fn push_right(seat: usize, buttons: &[Button]) -> [Input; MAX_PLAYERS] {
        let mut inputs = NONE;
        inputs[seat].stick_x = 127;
        for b in buttons {
            inputs[seat].set(*b, true);
        }
        inputs
    }

    #[test]
    fn nobody_moves_during_the_intro() {
        let mut run = new_run(2, Format::FreeForAll);
        let start = run.game.fighters[0].body.pos;
        for _ in 0..INTRO_TICKS - 1 {
            run.step(push_right(0, &[Button::A]));
        }
        assert_eq!(run.game.fighters[0].body.pos, start);
    }

    #[test]
    fn platform_shrinks_on_schedule() {
        assert_eq!(platform_radius(0), START_RADIUS);
        assert_eq!(platform_radius(SHRINK_START), START_RADIUS);
        assert_eq!(platform_radius(SHRINK_END), MIN_RADIUS);
        assert!(platform_radius((SHRINK_START + SHRINK_END) / 2) < START_RADIUS);
    }

    #[test]
    fn walking_off_the_edge_is_a_ring_out() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        place(&mut run, 0, START_RADIUS - 0.2, 0.0);
        for _ in 0..30 {
            run.step(push_right(0, &[]));
        }
        assert!(matches!(
            run.game.fighters[0].status,
            Status::Falling { .. }
        ));
        assert!(run.game.fell_at[0].is_some());
    }

    #[test]
    fn last_one_standing_wins() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        place(&mut run, 0, START_RADIUS - 0.2, 0.0);
        while run.outcome().is_none() {
            run.step(push_right(0, &[]));
        }
        assert_eq!(
            run.outcome().unwrap().placement,
            [Some(2), Some(1), None, None]
        );
    }

    #[test]
    fn a_dash_knocks_the_victim_back_and_stuns_them() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        place(&mut run, 0, -1.5, 0.0);
        place(&mut run, 1, 0.0, 0.0);
        run.game.fighters[0].facing = Vec2::new(1.0, 0.0);
        run.step(push_right(0, &[Button::A]));
        for _ in 0..8 {
            run.step(push_right(0, &[Button::A]));
        }
        let victim = &run.game.fighters[1];
        assert!(victim.stun > 0);
        assert_eq!(victim.last_hit_by, Some(Seat(0)));
        assert!(victim.body.vel.x > KNOCKBACK * 0.8);
    }

    #[test]
    fn dashing_into_a_brace_rebounds() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        place(&mut run, 0, -1.5, 0.0);
        place(&mut run, 1, 0.0, 0.0);
        run.game.fighters[0].facing = Vec2::new(1.0, 0.0);
        let mut inputs = push_right(0, &[Button::A]);
        inputs[1].set(Button::B, true);
        for _ in 0..9 {
            run.step(inputs);
        }
        assert!(run.game.fighters[1].bracing());
        assert_eq!(run.game.fighters[1].stun, 0);
        assert!(run.game.fighters[0].stun > 0);
        assert!(run.game.fighters[0].body.vel.x < 0.0);
    }

    #[test]
    fn brace_runs_out_then_cools_down() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        let mut hold = NONE;
        hold[0].set(Button::B, true);
        for _ in 0..BRACE_MAX_TICKS + 1 {
            run.step(hold);
        }
        assert!(!run.game.fighters[0].bracing());
        assert_eq!(run.game.fighters[0].brace_cooldown, BRACE_COOLDOWN);
    }

    #[test]
    fn dash_has_a_cooldown() {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        run.step(push_right(0, &[Button::A]));
        assert_eq!(run.game.fighters[0].dash, DASH_TICKS);
        for _ in 0..DASH_TICKS + 2 {
            run.step(NONE);
        }
        run.step(push_right(0, &[Button::A]));
        assert_eq!(run.game.fighters[0].dash, 0);
    }

    #[test]
    fn team_wins_when_the_other_side_is_gone() {
        let mut run = new_run(
            4,
            Format::TwoVsTwo {
                team_of: [0, 1, 0, 1],
            },
        );
        to_playing(&mut run);
        place(&mut run, 1, START_RADIUS + 1.0, 0.0);
        place(&mut run, 3, -START_RADIUS - 1.0, 0.0);
        while run.outcome().is_none() {
            run.step(NONE);
        }
        assert_eq!(
            run.outcome().unwrap().placement,
            [Some(1), Some(2), Some(1), Some(2)]
        );
    }

    #[test]
    fn survivors_share_the_win_at_time_out() {
        let mut run = new_run(3, Format::FreeForAll);
        to_playing(&mut run);
        place(&mut run, 0, -0.6, 0.0);
        place(&mut run, 1, 0.6, 0.0);
        place(&mut run, 2, START_RADIUS + 1.0, 0.0);
        while run.outcome().is_none() {
            run.step(NONE);
        }
        assert_eq!(run.game.tick, PLAY_TICKS);
        assert_eq!(
            run.outcome().unwrap().placement,
            [Some(1), Some(1), Some(3), None]
        );
    }

    #[test]
    fn the_solo_is_heavier() {
        let run = new_run(4, Format::OneVsThree { solo: Seat(2) });
        assert_eq!(run.game.fighters[2].body.mass, SOLO_MASS);
        assert_eq!(run.game.fighters[0].body.mass, 1.0);
    }

    #[test]
    fn passes_all_automated_quality_bars() {
        sim_check::check_all::<LanternPush>(6).unwrap();
    }
}
