//! Bots for Lantern Flail. A swing is pumped the way a person pumps a conker on a string: run
//! the way the lantern is already going round, so your hand leads it and the rope pulls it on.
//! Once it's flying, close in to about a rope's length from the target so the lantern sweeps
//! through them. Bolas partners take a side of the target each and run past it together, so the
//! rope and the lantern between them drag across.

use super::*;

pub(crate) struct Skill {
    /// Ticks between fresh decisions (aim noise and rolls are redrawn each window).
    reaction: u32,
    /// Steering error in radians, either way.
    aim_noise: f32,
    /// How far in from the edge it keeps, and how hard it heads back.
    edge_margin: f32,
    /// Lantern speed it waits for before closing in.
    swing_speed: f32,
    /// How fast it turns the stick round to whirl, in radians a second.
    whirl_rate: f32,
    /// Chance per window of lunging when it lines up.
    lunge_percent: u32,
    /// Reels in to spin faster close up, and when the lantern drags it towards the water.
    reels: bool,
    /// Chance per window of picking the best target (otherwise the nearest).
    picks_percent: u32,
    /// Chance per window of stepping out of the way of a rival lantern coming in fast.
    dodge_percent: u32,
}

pub(crate) fn skill(difficulty: Difficulty) -> Skill {
    match difficulty {
        Difficulty::Easy => Skill {
            reaction: 26,
            aim_noise: 0.6,
            edge_margin: 0.9,
            swing_speed: 6.0,
            whirl_rate: 5.0,
            lunge_percent: 20,
            reels: false,
            picks_percent: 20,
            dodge_percent: 0,
        },
        Difficulty::Normal => Skill {
            reaction: 16,
            aim_noise: 0.3,
            edge_margin: 1.3,
            swing_speed: 8.0,
            whirl_rate: 8.0,
            lunge_percent: 45,
            reels: false,
            picks_percent: 60,
            dodge_percent: 30,
        },
        Difficulty::Hard => Skill {
            reaction: 9,
            aim_noise: 0.12,
            edge_margin: 1.6,
            swing_speed: 10.0,
            whirl_rate: 10.0,
            lunge_percent: 70,
            reels: true,
            picks_percent: 95,
            dodge_percent: 70,
        },
    }
}

/// Mixes a seed with a part (seat, window) so matches don't share their bots' luck.
fn mix(seed: u64, part: u64) -> u64 {
    let mut rng = Rng::new(seed ^ part.wrapping_mul(0x9e37_79b9_7f4a_7c15));
    ((rng.next_u32() as u64) << 32) | rng.next_u32() as u64
}

fn stick(direction: Vec2) -> Input {
    let d = direction.clamp_length(1.0);
    Input {
        buttons: 0,
        stick_x: Input::quantize_axis(d.x),
        stick_y: Input::quantize_axis(-d.y),
    }
}

struct Plan {
    aim_error: f32,
    lunge: bool,
    picks_best: bool,
    dodges: bool,
    /// Which way it spins up when the lantern isn't going round yet.
    spin: f32,
}

fn plan(state: &State, seat: Seat, skill: &Skill) -> Plan {
    let window = state.tick / skill.reaction;
    let mut rng = Rng::new(mix(mix(state.seed, seat.0 as u64 + 1), window as u64));
    Plan {
        aim_error: (rng.range_u32(0, 2000) as f32 / 1000.0 - 1.0) * skill.aim_noise,
        lunge: rng.chance(skill.lunge_percent),
        picks_best: rng.chance(skill.picks_percent),
        dodges: rng.chance(skill.dodge_percent),
        // Each bot keeps one way of spinning up for the whole match.
        spin: if mix(state.seed, seat.0 as u64 + 7) & 1 == 0 {
            1.0
        } else {
            -1.0
        },
    }
}

pub(crate) fn input(state: &State, seat: Seat, difficulty: Difficulty) -> Input {
    let mut skill = skill(difficulty);
    if state.folk(seat).giant {
        // A big, slow target has to be more careful than its level, or the three dunk it at will.
        let careful = match difficulty {
            Difficulty::Easy => self::skill(Difficulty::Normal),
            _ => self::skill(Difficulty::Hard),
        };
        skill.edge_margin = careful.edge_margin;
        if difficulty == Difficulty::Normal {
            skill.dodge_percent = careful.dodge_percent;
            skill.whirl_rate = careful.whirl_rate;
            skill.swing_speed = careful.swing_speed;
            skill.lunge_percent = careful.lunge_percent;
        }
    }
    let plan = plan(state, seat, &skill);
    let me = state.folk(seat);
    let pos = me.body.pos;
    let Some(lantern) = state.lantern(seat) else {
        return Input::default();
    };

    let Some(target) = pick_target(state, seat, lantern, plan.picks_best) else {
        return stick(-pos * 0.3);
    };
    let (mut direction, mut reel, lunge_at) = if lantern.kind == Kind::Bolas {
        bolas(state, seat, lantern, target)
    } else {
        flail(state, seat, lantern, target, &skill, &plan)
    };

    // A rival lantern coming in fast: step out of its path, towards the middle if we can.
    if plan.dodges
        && let Some(away) = dodge(state, seat)
    {
        direction = away;
    }

    // Too near the edge: head back in, harder the closer it is.
    let edge = RAFT_RADIUS - pos.length();
    if edge < skill.edge_margin {
        let urgency = 1.0 - edge.max(0.0) / skill.edge_margin;
        direction = (direction * (1.0 - urgency) - pos.normalize_or_zero() * (urgency * 2.0))
            .normalize_or_zero();
        // Being dragged out by your own lantern: reel it in.
        let outward = lantern.body.pos.length() > pos.length();
        reel |= skill.reels && outward;
    }

    let aimed = rotate(direction, plan.aim_error);
    let mut input = stick(aimed);
    input.set(Button::B, reel);
    if let Some(towards) = lunge_at {
        let landing = pos + towards * (LUNGE_SPEED * LUNGE_TICKS as f32 * DT);
        let safe = landing.length() < RAFT_RADIUS - skill.edge_margin * 0.5;
        if plan.lunge && safe && me.lunge_cooldown == 0 && me.stun == 0 {
            input = stick(towards);
            input.set(Button::A, true);
        }
    }
    input
}

/// Which way to step to get out of the path of a rival lantern about to hit us, if one is.
fn dodge(state: &State, seat: Seat) -> Option<Vec2> {
    let me = state.folk(seat).body;
    let threat = state
        .lanterns
        .iter()
        .filter(|l| !l.down && !state.friendly(l, seat))
        .filter_map(|l| {
            let to_me = me.pos - l.body.pos;
            let distance = to_me.length();
            let closing = (l.body.vel - me.vel).dot(to_me.normalize_or_zero());
            (distance < 2.5 && closing > 6.0).then_some((distance, l.body.vel))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))?;
    let across = threat.1.normalize_or_zero().perp();
    let inward = -me.pos;
    Some(if across.dot(inward) >= 0.0 {
        across
    } else {
        -across
    })
}

/// The rival to go for: near, near the edge, knocked about already. Easy picks the nearest.
fn pick_target(state: &State, seat: Seat, lantern: &Lantern, best: bool) -> Option<Seat> {
    let from = lantern.body.pos.lerp(state.folk(seat).body.pos, 0.5);
    state
        .standing()
        .filter(|s| *s != seat && !state.allies(seat, *s) && state.folk(*s).safe == 0)
        .min_by(|a, b| {
            let score = |s: Seat| {
                let folk = state.folk(s);
                let near = folk.body.pos.distance(from);
                if best {
                    near - folk.body.pos.length() / RAFT_RADIUS * 2.0 - folk.damage as f32 * 0.01
                } else {
                    near
                }
            };
            score(*a).total_cmp(&score(*b)).then(a.cmp(b))
        })
}

/// Where to push the stick, whether to reel, and which way a lunge would go (if it's a good
/// moment).
fn flail(
    state: &State,
    seat: Seat,
    lantern: &Lantern,
    target: Seat,
    skill: &Skill,
    plan: &Plan,
) -> (Vec2, bool, Option<Vec2>) {
    let me = state.folk(seat);
    let pos = me.body.pos;
    let rope = lantern.rope_length(0);
    let goal = state.folk(target).body.pos;

    let out = (lantern.body.pos - pos).normalize_or_zero();
    // Which way the lantern itself goes round (not relative to us: running would flip it).
    let turning = out.perp_dot(lantern.body.vel);
    let spin = if turning.abs() > 1.5 {
        turning.signum()
    } else {
        plan.spin
    };
    let swing = lantern.speed();
    // Whirl: turn the stick round a little further every tick, the way it's going round.
    let whirl = rotate(me.facing, spin * skill.whirl_rate * DT);

    let to_goal = goal - pos;
    let distance = to_goal.length();
    // Stand off about a rope's length, so the lantern's circle passes over them.
    let stand_off = rope * 0.8;
    let approach = if distance > 1e-3 {
        (to_goal * ((distance - stand_off) / distance)).clamp_length(1.0)
    } else {
        Vec2::ZERO
    };
    let ready = swing >= skill.swing_speed;
    let direction = if ready {
        whirl * 0.75 + approach * 0.65
    } else {
        whirl + approach * 0.2
    };
    // Close up and swinging: reel in to whip it round faster.
    let reel = skill.reels && ready && distance < rope * 0.7;
    // A lunge at them while the lantern's flying yanks it into them.
    let lunge = (ready && distance < rope + 0.6).then(|| to_goal.normalize_or_zero());
    (direction.clamp_length(1.0), reel, lunge)
}

/// Bolas partners turn round after sweeping one way this long.
const SWEEP_TICKS: u32 = 80;

/// Bolas partners each take a side of the target and run past it, the rope between them.
fn bolas(state: &State, seat: Seat, lantern: &Lantern, target: Seat) -> (Vec2, bool, Option<Vec2>) {
    let me = state.folk(seat);
    let pos = me.body.pos;
    let goal = state.folk(target).body.pos;
    let partner = lantern
        .holders
        .iter()
        .flatten()
        .find(|s| **s != seat && state.folk(**s).standing())
        .copied();
    let Some(partner) = partner else {
        // Alone with it: swing it like a flail.
        let skill = skill(Difficulty::Normal);
        let plan = plan(state, seat, &skill);
        return flail(state, seat, lantern, target, &skill, &plan);
    };
    let other = state.folk(partner).body.pos;
    // Sweep across the target and back: the line between the partners is the rope, and they
    // run square to it, turning round every so often to drag it back over.
    // Both partners use the same frame: from the lower seat's end to the higher's.
    let (first, second) = if seat < partner {
        (pos, other)
    } else {
        (other, pos)
    };
    let line = (second - first).normalize_or_zero();
    let line = if line == Vec2::ZERO {
        Vec2::new(1.0, 0.0)
    } else {
        line
    };
    let across = line;
    let forward = if (state.tick / SWEEP_TICKS).is_multiple_of(2) {
        line.perp()
    } else {
        -line.perp()
    };
    let sweep = forward;
    let side = if seat < partner { -1.0 } else { 1.0 };
    let spread = lantern.kind.spec().rope_max * 0.85;
    let mut spot = goal + across * (side * spread) + sweep * 2.5;
    // Never run off the raft for it.
    let limit = RAFT_RADIUS - 1.2;
    if spot.length() > limit {
        spot = spot * (limit / spot.length());
    }
    let direction = (spot - pos).clamp_length(1.0);
    let close = goal.distance(lantern.body.pos) < 1.6;
    (direction, false, close.then_some(sweep))
}
