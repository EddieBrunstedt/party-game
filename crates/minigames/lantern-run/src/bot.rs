//! Bots for Lantern Run. Each tick a bot scores a grid of spots across the course against the
//! next few obstacles (how clear of each it would pass, whether it can get there in time) plus
//! what it wants on the way (a slipstream, a close graze for boost, catching a hooker's rope on
//! something), then steers for the best one, braking so it arrives without sliding past.

use super::*;
use sim_core::Rng;

pub(crate) struct Skill {
    /// Ticks between fresh decisions (aim noise and rolls are redrawn each window).
    reaction: u32,
    /// How far ahead it plans, in seconds.
    lookahead: f32,
    /// How clear of obstacles it means to pass.
    margin: f32,
    /// Error in its aim, either way across.
    aim_noise: f32,
    /// Chance per window of not steering at all.
    lapse_percent: u32,
    /// Lines up behind rivals for their slipstream.
    drafts: bool,
    /// Threads close past obstacles to charge its boost.
    grazes: bool,
    /// Chance per window of firing the grapple at a glider in reach.
    hook_percent: u32,
    /// Lets go of a rope about to pass this close to an obstacle (`None`: holds on regardless).
    snag_care: Option<f32>,
    /// Hooked, it steers so the rope catches on something.
    snags: bool,
    /// Chance per window of opening the throttle (or dropping a mine) when it's worth it.
    boost_percent: u32,
    /// Throttles whatever's coming when the kill line is within this share of the window
    /// behind it.
    danger_share: f32,
    /// How hard it keeps clear of rivals level with it, and of boosting rivals coming up.
    crowd_care: f32,
    /// The barge cuts a rope this many ticks after it catches.
    cut_delay: u16,
}

pub(crate) fn skill(difficulty: Difficulty) -> Skill {
    match difficulty {
        Difficulty::Easy => Skill {
            reaction: 24,
            lookahead: 0.9,
            margin: 0.1,
            aim_noise: 0.7,
            lapse_percent: 12,
            drafts: false,
            grazes: false,
            hook_percent: 15,
            snag_care: None,
            snags: false,
            boost_percent: 25,
            danger_share: 0.0,
            crowd_care: 0.0,
            cut_delay: 50,
        },
        Difficulty::Normal => Skill {
            reaction: 16,
            lookahead: 1.1,
            margin: 0.2,
            aim_noise: 0.5,
            lapse_percent: 6,
            drafts: true,
            grazes: false,
            hook_percent: 35,
            snag_care: Some(0.15),
            snags: true,
            boost_percent: 70,
            danger_share: 0.35,
            crowd_care: 0.8,
            cut_delay: 25,
        },
        Difficulty::Hard => Skill {
            reaction: 8,
            lookahead: 1.7,
            margin: 0.25,
            aim_noise: 0.12,
            lapse_percent: 0,
            drafts: true,
            grazes: true,
            hook_percent: 60,
            snag_care: Some(0.5),
            snags: true,
            boost_percent: 90,
            danger_share: 0.5,
            crowd_care: 2.0,
            cut_delay: 10,
        },
    }
}

/// Mixes a seed with a part (seat, window) so matches don't share their bots' luck.
fn mix(seed: u64, part: u64) -> u64 {
    let mut rng = Rng::new(seed ^ part.wrapping_mul(0x9e37_79b9_7f4a_7c15));
    ((rng.next_u32() as u64) << 32) | rng.next_u32() as u64
}

struct Rolls {
    aim: Vec2,
    lapse: bool,
    hook: bool,
    boost: bool,
}

fn rolls(state: &State, seat: Seat, skill: &Skill) -> Rolls {
    let window = state.tick / skill.reaction;
    let mut rng = Rng::new(mix(mix(state.seed, seat.0 as u64 + 1), window as u64));
    let mut noise = || (rng.range_u32(0, 2000) as f32 / 1000.0 - 1.0) * skill.aim_noise;
    let aim = Vec2::new(noise(), noise());
    Rolls {
        aim,
        lapse: rng.chance(skill.lapse_percent),
        hook: rng.chance(skill.hook_percent),
        boost: rng.chance(skill.boost_percent),
    }
}

/// Spots across the course it considers, in thirds of the half width by quarters of the half
/// height.
const ACROSS: i32 = 4;
const UP: i32 = 2;

/// How bad it would be to aim for `p`. `upcoming` is in order along the course.
fn cost(state: &State, seat: Seat, skill: &Skill, upcoming: &[Obstacle], p: Vec2) -> f32 {
    let me = state.ship(seat);
    let radius = me.body.radius;
    let ahead = me.body.pos + me.body.vel * 0.25;
    let mut cost = ahead.distance(p) * 0.15;
    for (k, obstacle) in upcoming.iter().enumerate() {
        let weight = [1.0, 0.6, 0.3][k.min(2)];
        let clear = obstacle.clearance(p) - radius - skill.margin;
        if clear < 0.0 {
            cost += weight * (10.0 + 20.0 * -clear);
        } else if skill.grazes && k == 0 && clear < 0.3 {
            cost -= 2.0;
        }
        if k == 0 {
            // Whether it can get there before it arrives.
            let time = ((obstacle.z - me.z) / me.speed).max(0.05);
            let short = me.body.pos.distance(p) - STEER_SPEED * 0.6 * time;
            if short > 0.0 {
                cost += short * 4.0;
            }
        }
    }
    let barge = state.barge();
    for other in state.flying().filter(|s| *s != seat) {
        let them = state.ship(other);
        let gap = them.z - me.z;
        let apart = p.distance(them.body.pos);
        if me.barge {
            // Sit in front of the nearest chaser: they have to go round, into its mines.
            if (-14.0..0.0).contains(&gap) {
                cost -= 2.5 * (1.0 - apart / 2.5).max(0.0) * (1.0 + gap / 14.0);
            }
            continue;
        }
        let level = gap.abs() < 2.5 || (them.throttle && (-8.0..0.0).contains(&gap));
        if level && !state.allies(seat, other) {
            cost += skill.crowd_care * (1.0 - apart / 2.0).max(0.0);
        }
        if Some(other) == barge && gap < 4.0 && gap > -SHIP_LENGTH {
            // Close behind the barge: go round it.
            if apart < them.body.radius + radius + 0.3 {
                cost += 3.0;
            }
        } else if skill.drafts
            && gap > SHIP_LENGTH
            && gap < DRAFT_LENGTH * 1.3
            && apart < DRAFT_WIDTH * 0.8
        {
            cost -= 2.0;
        }
    }
    if skill.snags {
        // Hooked: get an obstacle between the rope's thrower and here.
        for hooker in state.flying() {
            let them = state.ship(hooker);
            if them.hook.is_some_and(|h| h.target == seat)
                && state.rope_blocked((them.body.pos, them.z), (p, me.z))
            {
                cost -= 4.0;
            }
        }
    }
    cost
}

/// Where it means to be when the next obstacles arrive.
fn aim(state: &State, seat: Seat, skill: &Skill) -> Vec2 {
    let me = state.ship(seat);
    let horizon = me.z + me.speed * skill.lookahead;
    let mut upcoming: Vec<Obstacle> = state
        .course
        .between(me.z, horizon)
        .iter()
        .chain(state.mines.iter())
        .filter(|o| o.z + o.depth > me.z - 0.2 && o.z < horizon)
        .copied()
        .collect();
    upcoming.sort_by(|a, b| a.z.total_cmp(&b.z));
    upcoming.truncate(3);
    let near = half_extents(me.z);
    let far = half_extents(horizon);
    let half = Vec2::new(near.x.min(far.x), near.y.min(far.y));
    let room = Vec2::new(
        (half.x - me.body.radius - 0.1).max(0.0),
        (half.y - me.body.radius - 0.1).max(0.0),
    );
    let mut best = (f32::MAX, me.body.pos);
    let mut consider = |p: Vec2| {
        let c = cost(state, seat, skill, &upcoming, p);
        if c < best.0 {
            best = (c, p);
        }
    };
    consider(Vec2::new(
        me.body.pos.x.clamp(-room.x, room.x),
        me.body.pos.y.clamp(-room.y, room.y),
    ));
    for i in -ACROSS..=ACROSS {
        for j in -UP..=UP {
            consider(Vec2::new(
                room.x * i as f32 / ACROSS as f32,
                room.y * j as f32 / UP as f32,
            ));
        }
    }
    best.1
}

/// The stick that takes it to `target` and stops it there.
fn steer_to(ship: &Ship, target: Vec2) -> Vec2 {
    let to = target - ship.body.pos;
    let distance = to.length();
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
    let speed = (2.0 * thrust * 0.8 * distance)
        .sqrt()
        .min(distance * 5.0)
        .min(top);
    (to.normalize_or_zero() * speed) * (1.0 / top)
}

pub(crate) fn input(state: &State, seat: Seat, difficulty: Difficulty) -> Input {
    let me = state.ship(seat);
    let mut skill = skill(difficulty);
    if me.barge {
        // Three chasers at a level are clumsier together than one barge at it: an Easy barge
        // goes easier on them, and Normal and Hard ones harder.
        match difficulty {
            Difficulty::Easy => {
                skill.cut_delay = 120;
                skill.boost_percent = 5;
                skill.lapse_percent = 25;
            }
            Difficulty::Normal => {
                skill.cut_delay = 20;
                skill.boost_percent = 55;
            }
            Difficulty::Hard => {
                skill.cut_delay = 6;
                skill.boost_percent = 100;
                skill.lookahead = 2.2;
                skill.aim_noise = 0.05;
            }
        }
    }
    let rolls = rolls(state, seat, &skill);
    let mut input = Input::default();
    if me.stun == 0 && !rolls.lapse {
        let target = aim(state, seat, &skill) + rolls.aim;
        let stick = steer_to(me, target).clamp_length(1.0);
        input.stick_x = Input::quantize_axis(stick.x);
        input.stick_y = Input::quantize_axis(stick.y);
    }
    if me.stun > 0 {
        return input;
    }
    let ahead_clear = state
        .obstacles_near(me.z + me.speed * 0.5, me.speed * 0.5)
        .all(|o| o.clearance(me.body.pos) > me.body.radius + 0.6);
    if me.barge {
        let chaser = state.flying().any(|s| {
            let them = state.ship(s);
            let gap = me.z - them.z;
            s != seat && (3.0..30.0).contains(&gap) && them.body.pos.distance(me.body.pos) < 1.8
        });
        if chaser && me.mine_cooldown == 0 && rolls.boost {
            input.set(Button::A, true);
        }
        let hooked = state.flying().any(|s| {
            state
                .ship(s)
                .hook
                .is_some_and(|h| h.target == seat && h.t >= skill.cut_delay)
        });
        if hooked && me.cut_cooldown == 0 {
            input.set(Button::B, true);
        }
        return input;
    }
    // Fly flat out while the way ahead is clear (flat out, it steers sluggishly), and near the
    // kill line whatever's coming; once open, keep it open while that holds.
    let danger = me.z - state.camera_z < state.window() * skill.danger_share;
    let wants = danger || ahead_clear;
    let throttle = if me.throttle {
        wants
    } else {
        wants && rolls.boost && me.meter >= 150
    };
    input.set(Button::A, throttle);
    match me.hook {
        Some(hook) => {
            let them = state.ship(hook.target);
            let soon = 0.2;
            let clearance = state.rope_clearance(
                (me.body.pos + me.body.vel * soon, me.z + me.speed * soon),
                (
                    them.body.pos + them.body.vel * soon,
                    them.z + them.speed * soon,
                ),
            );
            let holds = skill.snag_care.is_none_or(|care| clearance > care);
            input.set(Button::B, holds);
        }
        None => {
            if rolls.hook && state.hook_target(seat).is_some() {
                input.set(Button::B, true);
            }
        }
    }
    input
}
