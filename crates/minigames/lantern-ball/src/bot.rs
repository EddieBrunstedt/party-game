//! Bots for Lantern Ball. Each plays the nearest ball: get behind it on the line to the goal it
//! picks, then dash through it or charge a kick and let go once it's in reach. A ball heading for
//! its own goal sends it back to block. Harder bots aim better, look further ahead, charge
//! harder, guard more often, and dash into rivals who are charging a kick. In 2v2 the partner
//! nearer the ball goes for it and the other covers the goal; the 1v3 keeper stays between the
//! ball and its goal and clears whatever comes close.

use super::*;

pub(crate) struct Skill {
    /// Ticks between fresh decisions (aim noise and rolls are redrawn each window).
    reaction: u32,
    /// Kick and dash direction error in radians, either way.
    aim_noise: f32,
    /// How far ahead it predicts the ball, in seconds.
    lookahead: f32,
    /// The charge it waits for before kicking, as a share of full: drawn in this range.
    charge: (f32, f32),
    /// How square behind the ball it must be to shoot (the cosine of the angle off).
    line_up: f32,
    /// Chance per window of kicking (or dashing, see below) when lined up.
    kick_percent: u32,
    dash_percent: u32,
    /// Chance per window of dashing a rival who is charging a kick by a ball.
    harass_percent: u32,
    /// Chance per window of noticing a ball heading for its goal.
    defend_percent: u32,
    /// Chance per window of drifting about.
    lapse_percent: u32,
    /// Aims away from whoever guards the goal, rather than at its middle.
    places: bool,
    /// How soon a ball must be due in its goal for it to notice, in seconds.
    threat_window: f32,
    /// Free-for-all: a ball further than this from its goal, with a rival nearer it, is left to
    /// them while it guards.
    home: f32,
    /// Dives (dashes) across to block a shot it can't walk to.
    dives: bool,
    /// Chance per window of kicking a ball in reach away from its goal.
    clear_percent: u32,
    /// Chance per window of dashing a rival out of the way of its shot.
    bash_percent: u32,
    /// How many floor units nearer a goal seems for every life its owner has over the fewest.
    leader_bias: f32,
}

pub(crate) fn skill(difficulty: Difficulty) -> Skill {
    match difficulty {
        Difficulty::Easy => Skill {
            reaction: 24,
            aim_noise: 0.4,
            lookahead: 0.25,
            charge: (0.0, 0.5),
            line_up: 0.55,
            kick_percent: 45,
            dash_percent: 15,
            harass_percent: 0,
            defend_percent: 35,
            lapse_percent: 15,
            places: false,
            threat_window: 0.8,
            home: 99.0,
            dives: false,
            clear_percent: 30,
            bash_percent: 10,
            leader_bias: 2.0,
        },
        Difficulty::Normal => Skill {
            reaction: 14,
            aim_noise: 0.2,
            lookahead: 0.5,
            charge: (0.2, 0.8),
            line_up: 0.75,
            kick_percent: 70,
            dash_percent: 30,
            harass_percent: 25,
            defend_percent: 70,
            lapse_percent: 5,
            places: true,
            threat_window: 1.2,
            home: 99.0,
            dives: true,
            clear_percent: 50,
            bash_percent: 30,
            leader_bias: 2.0,
        },
        Difficulty::Hard => Skill {
            reaction: 8,
            aim_noise: 0.07,
            lookahead: 0.7,
            charge: (0.45, 1.0),
            line_up: 0.88,
            kick_percent: 90,
            dash_percent: 45,
            harass_percent: 55,
            defend_percent: 95,
            lapse_percent: 0,
            places: true,
            threat_window: 1.8,
            home: 13.0,
            dives: true,
            clear_percent: 55,
            bash_percent: 50,
            leader_bias: 2.0,
        },
    }
}

/// Mixes a seed with a part (seat, window) so matches don't share their bots' luck.
fn mix(seed: u64, part: u64) -> u64 {
    let mut rng = Rng::new(seed ^ part.wrapping_mul(0x9e37_79b9_7f4a_7c15));
    ((rng.next_u32() as u64) << 32) | rng.next_u32() as u64
}

fn rotate(v: Vec2, angle: f32) -> Vec2 {
    let (s, c) = (sim_core::math::sin(angle), sim_core::math::cos(angle));
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
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
    charge: f32,
    kicks: bool,
    dashes: bool,
    harasses: bool,
    defends: bool,
    clears: bool,
    bashes: bool,
    lapses: bool,
    /// Which way it drifts in a lapse.
    drift: Vec2,
}

fn plan(state: &State, seat: Seat, skill: &Skill) -> Plan {
    let window = state.tick / skill.reaction;
    let mut rng = Rng::new(mix(mix(state.seed, seat.0 as u64 + 1), window as u64));
    let unit = |rng: &mut Rng| rng.range_u32(0, 1000) as f32 / 1000.0;
    Plan {
        aim_error: (unit(&mut rng) * 2.0 - 1.0) * skill.aim_noise,
        charge: skill.charge.0 + unit(&mut rng) * (skill.charge.1 - skill.charge.0),
        kicks: rng.chance(skill.kick_percent),
        dashes: rng.chance(skill.dash_percent),
        harasses: rng.chance(skill.harass_percent),
        defends: rng.chance(skill.defend_percent),
        clears: rng.chance(skill.clear_percent),
        bashes: rng.chance(skill.bash_percent),
        lapses: rng.chance(skill.lapse_percent),
        drift: Vec2::from_angle(unit(&mut rng) * TAU),
    }
}

/// Where a ball will be in `t` seconds, ignoring walls.
fn predict(ball: &Ball, t: f32) -> Vec2 {
    let travelled = (1.0 - sim_core::math::exp(-BALL_DRAG * t)) / BALL_DRAG;
    ball.body.pos + ball.body.vel * travelled
}

/// How soon a ball crosses into side `side`'s goal, if it's heading in within `window` seconds.
fn threat(state: &State, ball: &Ball, side: usize, window: f32) -> Option<f32> {
    let side = &state.sides[side];
    let goal = side.open_goal()?;
    let into = ball.body.vel.dot(side.normal);
    if into < 1.5 {
        return None;
    }
    let t = (-side.depth(ball.body.pos)).max(0.0) / into;
    let cross = ball.body.pos + ball.body.vel * t;
    (t < window && side.lateral(cross).abs() < goal.half_width + 0.8).then_some(t)
}

/// What the bot does with its stick and buttons this tick.
struct Move {
    direction: Vec2,
    dash: bool,
    /// Hold B (charge) or not.
    charge: bool,
}

pub(crate) fn input(state: &State, seat: Seat, difficulty: Difficulty) -> Input {
    let mut skill = skill(difficulty);
    if state.folk(seat).keeper && difficulty == Difficulty::Easy {
        // An Easy keeper who never dives just watches the three walk it in.
        skill.dives = true;
        skill.threat_window = 1.2;
        skill.defend_percent = 60;
        skill.clear_percent = 45;
    }
    let plan = plan(state, seat, &skill);
    let me = state.folk(seat);
    let own_side = state.owner_of(seat).and_then(|o| state.goal_side(o));

    let mv = choose(state, seat, &skill, &plan, own_side);
    let mut direction = mv.direction;
    if plan.lapses && !me.charging {
        direction = (direction * 0.4 + plan.drift * 0.6).clamp_length(0.7);
    }
    let mut input = stick(direction);
    let ready = me.stun == 0 && me.dash == 0;
    // A fresh press is needed for both, so neither is held while it can't start.
    input.set(Button::A, mv.dash && ready && me.dash_cooldown == 0);
    input.set(Button::B, mv.charge && (me.charging || ready));
    input
}

fn choose(state: &State, seat: Seat, skill: &Skill, plan: &Plan, own_side: Option<usize>) -> Move {
    let me = state.folk(seat);
    let idle = |to: Vec2| Move {
        direction: route(state, seat, to, 1.0),
        dash: false,
        charge: false,
    };
    let balls: Vec<&Ball> = state.balls.iter().filter(|b| b.rolling()).collect();
    if balls.is_empty() {
        // Wait between our goal and the middle, where the ball drops.
        let home = own_side.map(|s| state.sides[s].center() * 0.35);
        return idle(home.unwrap_or(Vec2::ZERO));
    }

    if let Some(harass) = harass(state, seat, plan) {
        return harass;
    }

    if let Some(side) = own_side
        && plan.clears
        && let Some(clear) = clear(state, seat, &balls, side)
    {
        return clear;
    }

    if me.keeper {
        return keep(state, seat, skill, plan, &balls, own_side);
    }

    // A ball heading for our goal: go back and block it, if it's ours to stop.
    if let Some(side) = own_side
        && plan.defends
        && let Some(ball) = balls
            .iter()
            .filter_map(|b| threat(state, b, side, skill.threat_window).map(|t| (t, *b)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, b)| b)
        && guards(state, seat, side)
    {
        return block(state, seat, skill, plan, ball, side);
    }

    // 2v2: the partner further from the ball covers the goal.
    let nearest = nearest_ball(&balls, me.body.pos);
    // Free-for-all: leave a far ball to a rival nearer it, and mind the goal.
    if let Some(side) = own_side
        && state.format == Format::FreeForAll
        && nearest.body.pos.distance(state.sides[side].center()) > skill.home
    {
        let mine = nearest.body.pos.distance(me.body.pos);
        let rival_nearer = state
            .playing()
            .filter(|s| *s != seat)
            .any(|s| nearest.body.pos.distance(state.folk(s).body.pos) < mine);
        if rival_nearer {
            return cover(state, seat, nearest, side);
        }
    }
    // Teams: whoever's nearest the ball goes for it, and the rest help.
    if let Some(side) = own_side {
        let mine = nearest.body.pos.distance(me.body.pos);
        let helper = state
            .playing()
            .filter(|s| *s != seat && state.format.same_team(seat, *s))
            .any(|s| {
                let bias = if seat < s { 0.5 } else { -0.5 };
                mine > nearest.body.pos.distance(state.folk(s).body.pos) + bias
            });
        if helper && mine > 2.5 {
            // In our half, cover the goal; in theirs, go up beside the attack for the rebound.
            // Two-goal pitches are symmetric, so their goal is opposite ours.
            let goal = state.sides[side].center();
            let ours = nearest.body.pos.distance(goal) < nearest.body.pos.distance(-goal);
            if ours || me.keeper {
                return cover(state, seat, nearest, side);
            }
            if let Some(aim) = target(state, seat, skill, nearest) {
                return support(state, seat, nearest, aim);
            }
        }
    }

    let Some(aim) = target(state, seat, skill, nearest) else {
        return idle(Vec2::ZERO);
    };
    if let Some(bash) = bash(state, seat, plan, nearest, aim) {
        return bash;
    }
    attack(state, seat, skill, plan, nearest, aim, skill.line_up)
}

/// Dash a rival standing in the way of the shot (most often the goal's guard) out of it first.
fn bash(state: &State, seat: Seat, plan: &Plan, ball: &Ball, aim: Vec2) -> Option<Move> {
    let me = state.folk(seat);
    if !plan.bashes || me.dash_cooldown > 0 || me.stun > 0 || me.charging {
        return None;
    }
    let range = DASH_SPEED * DASH_TICKS as f32 * DT + 0.6;
    let victim = state
        .playing()
        .filter(|s| *s != seat && !state.format.same_team(seat, *s))
        .filter(|s| {
            let folk = state.folk(*s);
            let (closest, along) = physics::closest_on_segment(ball.body.pos, aim, folk.body.pos);
            folk.stun == 0
                && along > 0.0
                && closest.distance(folk.body.pos) < ball.body.radius + folk.body.radius + 0.4
                && folk.body.pos.distance(me.body.pos) < range
        })
        .min_by(|a, b| {
            let d = |s: Seat| state.folk(s).body.pos.distance(me.body.pos);
            d(*a).total_cmp(&d(*b)).then(a.cmp(b))
        })?;
    let at = state.folk(victim).body.pos + state.folk(victim).body.vel * 0.1;
    Some(Move {
        direction: route(state, seat, at, 4.0),
        dash: true,
        charge: false,
    })
}

fn nearest_ball<'a>(balls: &[&'a Ball], from: Vec2) -> &'a Ball {
    balls
        .iter()
        .min_by(|a, b| {
            a.body
                .pos
                .distance(from)
                .total_cmp(&b.body.pos.distance(from))
        })
        .copied()
        .expect("there is a ball")
}

/// Whether this seat is the one to go back for a threat to `side`: the teammate nearest the goal.
fn guards(state: &State, seat: Seat, side: usize) -> bool {
    let goal = state.sides[side].center();
    let mine = state.folk(seat).body.pos.distance(goal);
    !state
        .playing()
        .filter(|s| *s != seat && state.format.same_team(seat, *s))
        .any(|s| {
            let theirs = state.folk(s).body.pos.distance(goal);
            theirs < mine || (theirs == mine && s < seat)
        })
}

/// The point to aim at: a rival's open goal, the one that's easiest from the ball.
fn target(state: &State, seat: Seat, skill: &Skill, ball: &Ball) -> Option<Vec2> {
    let mine = state.owner_of(seat);
    let me = state.folk(seat).body.pos;
    let from = ball.body.pos;
    let sides = state.sides.len();
    // Ties go to the next goal round from ours, so no goal is everyone's first choice.
    let own = state
        .owner_of(seat)
        .and_then(|o| state.goal_side(o))
        .unwrap_or(0);
    let fewest = state
        .sides
        .iter()
        .filter_map(|s| s.open_goal())
        .map(|g| state.lives[g.owner as usize])
        .min()
        .unwrap_or(0) as f32;
    let (side, _) = state
        .sides
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.open_goal().map(|g| (i, s, g)))
        .filter(|(_, _, g)| Some(g.owner) != mine)
        .map(|(i, s, g)| {
            // Goals we're already lined up for are cheaper: no running round the ball.
            let line = (s.center() - from).normalize_or_zero();
            let behind = from - line * (ball.body.radius + 0.6);
            // Go for whoever has the most lives: nobody wins by hiding in their goal.
            let ahead = state.lives[g.owner as usize] as f32 - fewest;
            let cost =
                from.distance(s.center()) + me.distance(behind) * 0.7 - ahead * skill.leader_bias;
            (i, cost)
        })
        .min_by(|a, b| {
            let round = |i: usize| (i + sides - own) % sides;
            a.1.total_cmp(&b.1).then(round(a.0).cmp(&round(b.0)))
        })?;
    let side = &state.sides[side];
    let goal = side.goal.expect("a goal side");
    let mut aim = side.center();
    if skill.places {
        // Away from the nearest guard, into the open half of the mouth.
        let guard = state
            .playing()
            .filter(|o| state.owner_of(*o) == Some(goal.owner))
            .map(|o| state.folk(o).body.pos)
            .min_by(|a, b| a.distance(aim).total_cmp(&b.distance(aim)));
        if let Some(guard) = guard {
            let lean = if side.lateral(guard) > 0.0 { -1.0 } else { 1.0 };
            aim += side.tangent() * (lean * goal.half_width * 0.5);
        }
    }
    Some(aim)
}

/// Get behind the ball on the line to `aim`, then shove, dash or kick it along.
fn attack(
    state: &State,
    seat: Seat,
    skill: &Skill,
    plan: &Plan,
    ball: &Ball,
    aim: Vec2,
    line_up: f32,
) -> Move {
    let me = state.folk(seat);
    let pos = me.body.pos;
    let reach_time = (ball.body.pos.distance(pos) / me.speed()).min(skill.lookahead);
    let at = predict(ball, reach_time);
    let line = (aim - at).normalize_or_zero();
    let contact = ball.body.radius + me.body.radius;
    let behind = at - line * (contact + 0.15);
    let to_ball = at - pos;
    let gap = to_ball.length() - contact;
    // Kicks reach the ball where it is now, not where it's going.
    let reach = ball.body.pos.distance(pos) - contact;
    let square = to_ball.normalize_or_zero().dot(line);

    // On the wrong side of it: go round, wide enough not to knock it back.
    if (pos - at).dot(line) > -0.3 {
        let across = line.perp();
        let side = if (pos - at).dot(across) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        let round = at + across * (side * (contact + 0.9)) - line * 0.6;
        return Move {
            direction: (round - pos).normalize_or_zero(),
            dash: false,
            charge: false,
        };
    }

    let shoot = rotate(line, plan.aim_error);
    let lined_up = square > line_up;
    if me.charging {
        // Wait just inside kick reach without touching it, and let go once charged enough (or
        // when it's getting away).
        let leaving = ball.body.vel.dot((ball.body.pos - pos).normalize_or_zero()) > 1.5;
        let release = reach <= KICK_REACH * 0.9
            && square > line_up * 0.8
            && (me.charge_share() >= plan.charge || leaving || reach < 0.05);
        let wait = at - line * (contact + KICK_REACH * 0.5);
        let direction = if release {
            shoot
        } else {
            ((wait - pos) * 2.5).clamp_length(1.0)
        };
        return Move {
            direction,
            dash: false,
            charge: !release && gap < 5.0,
        };
    }
    if lined_up && gap < 2.0 && plan.dashes && me.dash_cooldown == 0 {
        return Move {
            direction: shoot,
            dash: true,
            charge: false,
        };
    }
    let direction = if (behind - pos).length() < 0.5 {
        line
    } else {
        (behind - pos).normalize_or_zero()
    };
    Move {
        direction,
        dash: false,
        charge: lined_up && gap < 2.5 && plan.kicks,
    }
}

/// The nearest spot to `spot` a seat may stand on: outside every crease, unless it's the keeper.
fn allowed(state: &State, seat: Seat, spot: Vec2) -> Vec2 {
    let me = state.folk(seat);
    if me.keeper {
        return spot;
    }
    let mut spot = spot;
    for side in &state.sides {
        let Some(crease) = side.crease() else {
            continue;
        };
        let out = spot - side.center();
        let clear = crease + me.body.radius + 0.05;
        if out.length() < clear {
            let dir = if out.length() > 1e-4 {
                out.normalize_or_zero()
            } else {
                -side.normal
            };
            spot = side.center() + dir * clear;
        }
    }
    spot
}

/// Head for `to` without running through a ball on the way (which would shove it along ahead,
/// often into our own goal): step round it, on our side.
fn route(state: &State, seat: Seat, to: Vec2, speed: f32) -> Vec2 {
    let me = state.folk(seat).body;
    let to = allowed(state, seat, to);
    let path = to - me.pos;
    let mut direction = (path * speed).clamp_length(1.0);
    for ball in state.balls.iter().filter(|b| b.rolling()) {
        let clear = ball.body.radius + me.radius + 0.35;
        let (closest, along) = physics::closest_on_segment(me.pos, to, ball.body.pos);
        if along <= 0.0 || along >= 1.0 || closest.distance(ball.body.pos) >= clear {
            continue;
        }
        let across = path.normalize_or_zero().perp();
        let side = if (me.pos - ball.body.pos).dot(across) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        let round = ball.body.pos + across * (side * (clear + 0.3));
        direction = (round - me.pos).normalize_or_zero();
        break;
    }
    direction
}

/// Get between the ball and our goal, then clear it away.
fn block(state: &State, seat: Seat, skill: &Skill, plan: &Plan, ball: &Ball, side: usize) -> Move {
    let me = state.folk(seat);
    let goal = &state.sides[side];
    if skill.dives
        && let Some(t) = threat(state, ball, side, skill.threat_window)
        && (me.body.pos - ball.body.pos).dot(goal.normal) > 0.0
    {
        return intercept(state, seat, ball, side, t);
    }
    let at = predict(ball, skill.lookahead * 0.5);
    let close = at.distance(me.body.pos) < ball.body.radius + me.body.radius + 1.2;
    let goalward = (me.body.pos - at).dot(goal.normal) > 0.0;
    if close && goalward {
        // Between it and the goal: kick it out the way it came.
        let away = at - goal.normal * 6.0;
        return attack(state, seat, skill, plan, ball, away, 0.2);
    }
    let lateral = goal.lateral(at).clamp(
        -goal.goal.map_or(1.0, |g| g.half_width),
        goal.goal.map_or(1.0, |g| g.half_width),
    );
    let spot = goal.center() - goal.normal * 1.4 + goal.tangent() * lateral;
    Move {
        direction: route(state, seat, spot, 1.0),
        dash: false,
        charge: false,
    }
}

/// Up with the attack: off to the side between the ball and the goal it's going for.
fn support(state: &State, seat: Seat, ball: &Ball, aim: Vec2) -> Move {
    let me = state.folk(seat).body.pos;
    let line = (aim - ball.body.pos).normalize_or_zero();
    let across = line.perp();
    let side = if (me - ball.body.pos).dot(across) >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let spot = ball.body.pos.lerp(aim, 0.45) + across * (side * 2.2);
    Move {
        direction: route(state, seat, spot, 0.8),
        dash: false,
        charge: false,
    }
}

/// The partner's job: hang back between the ball and our goal, about halfway, ready for it.
fn cover(state: &State, seat: Seat, ball: &Ball, side: usize) -> Move {
    let goal = &state.sides[side];
    let out = ball.body.pos - goal.center();
    let spot = goal.center() + out.normalize_or_zero() * (out.length() * 0.55).clamp(2.0, 8.0);
    Move {
        direction: route(state, seat, spot, 0.8),
        dash: false,
        charge: false,
    }
}

/// A ball in reach by our goal (or heading for it): a quick kick away, out the way it came.
/// Blocking with your body doesn't stop it: it's twice your weight.
fn clear(state: &State, seat: Seat, balls: &[&Ball], side: usize) -> Option<Move> {
    let me = state.folk(seat);
    let goal = &state.sides[side];
    let ball = balls.iter().find(|b| {
        let reach = b.body.pos.distance(me.body.pos) - b.body.radius - me.body.radius;
        let danger =
            threat(state, b, side, 1.2).is_some() || b.body.pos.distance(goal.center()) < 4.0;
        reach < KICK_REACH * 0.9 && danger
    })?;
    let away = (ball.body.pos - goal.center()).normalize_or_zero();
    // Kick off the line from us, but never back towards the goal.
    let from_me = (ball.body.pos - me.body.pos).normalize_or_zero();
    let mut direction = (away + from_me).normalize_or_zero();
    if direction.dot(goal.normal) > -0.2 {
        direction = (direction - goal.normal).normalize_or_zero();
    }
    Some(Move {
        direction,
        dash: false,
        // A tap: press, then let go next tick.
        charge: !me.charging,
    })
}

/// A shot coming in `t` seconds: get in its way, diving (dashing) across if it's too far to walk.
fn intercept(state: &State, seat: Seat, shot: &Ball, side: usize, t: f32) -> Move {
    let me = state.folk(seat);
    let goal = &state.sides[side];
    let half = goal.goal.map_or(1.0, |g| g.half_width);
    let cross = shot.body.pos + shot.body.vel * t;
    let lateral = goal.lateral(cross).clamp(-half, half);
    let stop = allowed(
        state,
        seat,
        goal.center() - goal.normal * me.body.radius + goal.tangent() * lateral,
    );
    let need = stop - me.body.pos;
    let walk = me.speed() * t;
    let dive = DASH_SPEED * DASH_TICKS as f32 * DT;
    Move {
        direction: (need * 3.0).clamp_length(1.0),
        dash: need.length() > walk && need.length() < dive + walk,
        charge: false,
    }
}

/// The keeper stays on its line between the ball and the goal, and clears anything near.
fn keep(
    state: &State,
    seat: Seat,
    skill: &Skill,
    plan: &Plan,
    balls: &[&Ball],
    own_side: Option<usize>,
) -> Move {
    let me = state.folk(seat);
    let Some(side) = own_side else {
        return Move {
            direction: Vec2::ZERO,
            dash: false,
            charge: false,
        };
    };
    let goal = &state.sides[side];
    let half = goal.goal.map_or(1.0, |g| g.half_width);
    // The ball that will be nearest the goal soon.
    let ball = balls
        .iter()
        .min_by(|a, b| {
            let d = |ball: &Ball| predict(ball, skill.lookahead).distance(goal.center());
            d(a).total_cmp(&d(b))
        })
        .copied()
        .expect("there is a ball");
    if plan.defends
        && let Some((t, shot)) = balls
            .iter()
            .filter_map(|b| threat(state, b, side, skill.threat_window).map(|t| (t, *b)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
    {
        return intercept(state, seat, shot, side, t);
    }
    let at = predict(ball, skill.lookahead * 0.6);
    let near_goal = at.distance(goal.center()) < 5.0;
    let nearest_rival = state
        .playing()
        .filter(|s| *s != seat)
        .map(|s| state.folk(s).body.pos.distance(at))
        .fold(f32::MAX, f32::min);
    let mine = at.distance(me.body.pos);
    if near_goal && (mine < nearest_rival + 0.5 || mine < 2.2) {
        // Clear it up the pitch, at their goal if it's on.
        let away = state
            .sides
            .iter()
            .find(|s| s.open_goal().is_some_and(|g| g.owner != 0))
            .map(|s| s.center())
            .unwrap_or(-goal.normal * 8.0);
        let line_up = if mine < 1.8 { 0.0 } else { 0.3 };
        return attack(state, seat, skill, plan, ball, away, line_up);
    }
    let toward = (at - goal.center()).normalize_or_zero();
    let mut spot = goal.center() + toward * 1.6;
    let lateral = goal.lateral(spot).clamp(-half * 0.85, half * 0.85);
    spot = goal.center() - goal.normal * 1.2 + goal.tangent() * lateral;
    Move {
        direction: route(state, seat, spot, 1.5),
        dash: false,
        charge: false,
    }
}

/// Dash into a rival who's charging a kick beside a ball, if one is in range.
fn harass(state: &State, seat: Seat, plan: &Plan) -> Option<Move> {
    let me = state.folk(seat);
    if !plan.harasses || me.dash_cooldown > 0 || me.stun > 0 || me.charging {
        return None;
    }
    let range = DASH_SPEED * DASH_TICKS as f32 * DT + 0.6;
    let victim = state
        .playing()
        .filter(|s| *s != seat && !state.format.same_team(seat, *s))
        .filter(|s| {
            let folk = state.folk(*s);
            folk.charging
                && folk.charge > 8
                && folk.stun == 0
                && folk.body.pos.distance(me.body.pos) < range
                && state
                    .balls
                    .iter()
                    .any(|b| b.rolling() && b.body.pos.distance(folk.body.pos) < BALL_RADIUS + 1.6)
        })
        .min_by(|a, b| {
            let d = |s: Seat| state.folk(s).body.pos.distance(me.body.pos);
            d(*a).total_cmp(&d(*b)).then(a.cmp(b))
        })?;
    let at = state.folk(victim).body.pos + state.folk(victim).body.vel * 0.1;
    Some(Move {
        direction: (at - me.body.pos).normalize_or_zero(),
        dash: true,
        charge: false,
    })
}
