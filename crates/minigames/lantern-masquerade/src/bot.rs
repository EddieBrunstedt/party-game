//! Bots read only what anyone watching could know: the crowd, the bells, the scoreboard, the
//! clues the simulation keeps ([`Clue`]) and which folk is their own.
//!
//! A hider heads for a bell its side still needs (the nearest, but Easy bots take theirs in an
//! order of their own) in walks and stops of a stroller's lengths ([`Skill::npc_like`] of the
//! time; the rest are hurried and stand out), and rings it. It strikes a suspect in reach, and
//! goes after anyone it saw strike. The seeker moves its sight onto the likeliest suspect and
//! catches once sure enough, or waits over a bell.
//!
//! Suspicion is built only from public clues, each kept in mind for a while: a ring by a rival
//! side with the folk under the bell (twice if the bot kept track of it between two rings, a
//! chance that fades with the time between them), walks and stops no stroller makes, standing
//! still under a bell someone is pulling, and striking.

use super::*;

struct Skill {
    /// Percent of a hider's walks and stops kept within a stroller's lengths.
    npc_like: u32,
    /// How long a hider's unhurried stops last beyond a stroller's shortest, at most.
    linger: u16,
    /// Ticks added to every unhurried stop.
    dawdle: u16,
    /// Whether a hider heads for the nearest bell it needs, or for its bells in a set order.
    nearest_first: bool,
    /// How long, in ticks, a clue stays in mind...
    memory: u32,
    /// ...how long it takes to register...
    notice: u32,
    /// ...and how well it follows one folk through the crowd between two rings: it keeps
    /// track across a gap of `g` ticks with a chance of `1 - g / track`.
    track: u32,
    /// Suspicion it takes to strike someone in reach...
    strike_at: u32,
    /// ...and to go after them (from within [`HUNT_RANGE`]).
    hunt_at: u32,
    /// Seeker: suspicion it takes to try a catch, less a boldness drawn up to `bold` for each
    /// suspect every [`BOLD_WINDOW`].
    catch_at: u32,
    bold: u32,
    /// Seeker: how far its aim wanders, in floor units.
    aim_error: f32,
}

fn skill(difficulty: Difficulty) -> Skill {
    match difficulty {
        Difficulty::Easy => Skill {
            npc_like: 40,
            linger: 40,
            dawdle: 45,
            nearest_first: false,
            memory: 300,
            notice: 40,
            track: 240,
            strike_at: 70,
            hunt_at: u32::MAX,
            catch_at: 55,
            bold: 40,
            aim_error: 0.3,
        },
        Difficulty::Normal => Skill {
            npc_like: 75,
            linger: 40,
            dawdle: 15,
            nearest_first: true,
            memory: 360,
            notice: 20,
            track: 600,
            strike_at: 70,
            hunt_at: 100,
            catch_at: 55,
            bold: 40,
            aim_error: 0.3,
        },
        Difficulty::Hard => Skill {
            npc_like: 95,
            linger: 20,
            dawdle: 0,
            nearest_first: true,
            memory: 600,
            notice: 8,
            track: 1000,
            strike_at: 70,
            hunt_at: 100,
            catch_at: 55,
            bold: 40,
            aim_error: 0.15,
        },
    }
}

const HUNT_RANGE: f32 = 5.0;
/// Suspicion for standing still under a bell someone is pulling.
const PULLING: i32 = 30;
/// A hurried hider stops for up to this long.
const HURRIED_PAUSE: u16 = 12;
/// The seeker redraws its aim's wander this often.
const AIM_WINDOW: u32 = 30;
/// The seeker makes up its mind about each suspect again this often.
const BOLD_WINDOW: u32 = 120;
/// The seeker waits over one bell this long before picking another.
const PATROL_WINDOW: u32 = 240;

pub(crate) fn input(state: &State, seat: Seat, difficulty: Difficulty, t: u16) -> Input {
    let skill = skill(difficulty);
    if state.seat(seat).seeker {
        seeker(state, seat, &skill, t)
    } else {
        hider(state, seat, &skill, t)
    }
}

/// A random number for this seat from `parts`. Each part is mixed in on its own: XORed together,
/// a seed and a counter could cancel out, and bots in different matches would roll alike.
fn roll(state: &State, seat: Seat, parts: &[u64]) -> u64 {
    let mut key = state.seed ^ (seat.0 as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    for part in parts {
        let mut rng = Rng::new(key ^ part.wrapping_mul(0xbf58_476d_1ce4_e5b9));
        key = ((rng.next_u32() as u64) << 32) | rng.next_u32() as u64;
    }
    key
}

/// How sure the bot in `seat` is that folk `f` is a rival player.
fn suspicion(state: &State, seat: Seat, f: usize, skill: &Skill) -> u32 {
    let team = state.team(seat);
    let folk = &state.folk[f];
    if folk.cleared || !folk.standing() {
        return 0;
    }
    let clue = &folk.clue;
    let age = |at: u32| state.tick.saturating_sub(at);
    let mut score = 0i32;
    // Seen striking (strollers never do), if it hasn't since slipped away into the crowd.
    if folk.struck_at.is_some_and(|at| age(at) <= skill.memory) {
        score += 100;
    }
    // A ring it noticed and remembers, and the one before if it kept track of the folk since.
    let rings = |t: usize| {
        if !(skill.notice..=skill.memory).contains(&age(clue.rang_at[t])) {
            0
        } else if clue.rang_before[t] > 0 && kept_track(state, seat, f, t, skill) {
            2
        } else {
            1
        }
    };
    // One rival ringing two bells with this folk there says more than two rivals ringing one.
    let rivals = (0..MAX_PLAYERS).filter(|t| *t != team as usize).map(rings);
    score += 35 * rivals.max().unwrap_or(0);
    score -= 25 * rings(team as usize);
    if age(clue.odd_at) <= skill.memory {
        score += 8 * clue.odd.min(5) as i32;
    }
    // Standing still under a swinging bell: someone there is pulling its rope.
    let pulled = state
        .bell_in_reach(folk.pos)
        .is_some_and(|b| state.bells[b].pull as u32 >= skill.notice);
    if pulled && !folk.moving {
        score += PULLING;
    }
    score.max(0) as u32
}

/// Whether the bot in `seat` followed folk `f` between team `t`'s last two rings near it.
fn kept_track(state: &State, seat: Seat, f: usize, t: usize, skill: &Skill) -> bool {
    let clue = &state.folk[f].clue;
    let gap = clue.rang_at[t] - clue.rang_before[t];
    let chance = 1000u32.saturating_sub(gap * 1000 / skill.track.max(1));
    let r = roll(
        state,
        seat,
        &[f as u64, t as u64, clue.rang_at[t] as u64, 4],
    );
    (r % 1000) < chance as u64
}

/// The stick pushed fully along direction `dir`.
fn stick_along(dir: u8) -> Input {
    stick_towards(dir_vec(dir))
}

fn stick_towards(v: Vec2) -> Input {
    let v = v.normalize_or_zero();
    Input {
        buttons: 0,
        stick_x: (v.x * 127.0).round() as i8,
        stick_y: (-v.y * 127.0).round() as i8,
    }
}

fn hider(state: &State, seat: Seat, skill: &Skill, t: u16) -> Input {
    let me = state.folk_of(seat);
    let folk = state.folk[me];
    let segment_roll = roll(state, seat, &[state.round as u64, folk.segments as u64, 1]);
    if t < SHUFFLE_TICKS {
        return shuffle(&folk, segment_roll);
    }
    let team = state.team(seat);
    let suspects: Vec<(u32, f32, usize)> = (0..FOLK)
        .filter(|f| *f != me)
        .map(|f| {
            (
                suspicion(state, seat, f, skill),
                state.folk[f].pos.distance(folk.pos),
                f,
            )
        })
        .filter(|(s, _, _)| *s > 0)
        .collect();
    let striking = state.seeker().is_none();
    let in_reach = state.nearest_folk(folk.pos, STRIKE_REACH, Some(me));
    let strike = striking
        && state.seat(seat).cooldown == 0
        && in_reach.is_some_and(|f| suspects.iter().any(|s| s.2 == f && s.0 >= skill.strike_at));
    let needed = ALL_BELLS & !state.team_bells(team);
    // Under a bell its side still needs and standing still: pull the rope until it rings.
    let ring = !folk.moving
        && state
            .bell_in_reach(folk.pos)
            .is_some_and(|b| needed & 1 << b != 0);

    let hunt = suspects
        .iter()
        .filter(|s| striking && s.0 >= skill.hunt_at && s.1 <= HUNT_RANGE)
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.2.cmp(&b.2)));
    let mut input = if ring && hunt.is_none() {
        Input::default()
    } else if let Some(&(_, _, f)) = hunt {
        // Straight at them, step by step: hurried, and it shows.
        stick_towards(state.folk[f].pos - folk.pos)
    } else {
        let goal = if skill.nearest_first {
            (0..BELLS)
                .filter(|b| needed & 1 << b != 0)
                .map(|b| BELL_POSITIONS[b])
                .min_by(|a, b| a.distance(folk.pos).total_cmp(&b.distance(folk.pos)))
        } else {
            // The first bell still needed in an order drawn for the round.
            let mut order: Vec<usize> = (0..BELLS).collect();
            let mut r = roll(state, seat, &[state.round as u64, 6]);
            for i in (1..BELLS).rev() {
                order.swap(i, (r % (i as u64 + 1)) as usize);
                r /= BELLS as u64;
            }
            order
                .into_iter()
                .find(|b| needed & 1 << b != 0)
                .map(|b| BELL_POSITIONS[b])
        }
        .unwrap_or(Vec2::ZERO);
        stroll_to(&folk, goal, skill, segment_roll)
    };
    input.set(Button::A, ring);
    input.set(Button::B, strike);
    input
}

/// Walks and stops towards `goal`, planned afresh each tick from the walk's start and the
/// segment's roll, so the plan holds without the bot keeping any state.
fn stroll_to(folk: &Folk, goal: Vec2, skill: &Skill, segment_roll: u64) -> Input {
    let calm = segment_roll % 100 < skill.npc_like as u64;
    let r = segment_roll >> 8;
    if folk.moving {
        let start = folk.pos - dir_vec(folk.dir) * (STEP * folk.segment as f32);
        let ticks = (start.distance(goal) / STEP) as u16;
        let length = if calm {
            ticks.clamp(NPC_WALK.0, NPC_WALK.1)
        } else {
            ticks.max(1)
        };
        // Don't walk off the floor (it would stop us dead against the edge).
        let ahead = folk.pos + dir_vec(folk.dir) * (STEP * 2.0);
        if folk.segment >= length || !inside(ahead, 0.0) {
            Input::default()
        } else {
            stick_along(folk.dir)
        }
    } else {
        let pause = if calm {
            NPC_PAUSE.0 + (r % (skill.linger as u64 + 1)) as u16 + skill.dawdle
        } else {
            (r % (HURRIED_PAUSE as u64 + 1)) as u16
        };
        if folk.segment >= pause && folk.pos.distance(goal) > STEP {
            stick_along(sector_toward(goal - folk.pos))
        } else {
            Input::default()
        }
    }
}

/// Short steps and stops in random directions, like the shuffling crowd.
fn shuffle(folk: &Folk, segment_roll: u64) -> Input {
    let walk =
        SHUFFLE_WALK.0 + (segment_roll % (SHUFFLE_WALK.1 - SHUFFLE_WALK.0 + 1) as u64) as u16;
    let r = segment_roll >> 16;
    let pause = SHUFFLE_PAUSE.0 + (r % (SHUFFLE_PAUSE.1 - SHUFFLE_PAUSE.0 + 1) as u64) as u16;
    if folk.moving {
        return if folk.segment >= walk {
            Input::default()
        } else {
            stick_along(folk.dir)
        };
    }
    if folk.segment < pause {
        return Input::default();
    }
    let mut dir = ((r >> 16) % DIRECTIONS as u64) as u8;
    if !inside(
        folk.pos + dir_vec(dir) * (STEP * SHUFFLE_WALK.1 as f32),
        MARGIN,
    ) {
        dir = sector_toward(-folk.pos);
    }
    stick_along(dir)
}

fn seeker(state: &State, seat: Seat, skill: &Skill, t: u16) -> Input {
    let sight = state.sight;
    let best = (0..FOLK)
        .map(|f| {
            (
                suspicion(state, seat, f, skill),
                state.folk[f].pos.distance(sight.pos),
                f,
            )
        })
        .filter(|(s, _, _)| *s > 0)
        .max_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)).then(b.2.cmp(&a.2)));
    let window = state.tick / AIM_WINDOW;
    let r = roll(state, seat, &[state.round as u64, window as u64, 2]);
    let wander = Vec2::from_angle((r % 6283) as f32 / 1000.0)
        * (skill.aim_error * ((r >> 16) % 101) as f32 / 100.0);
    let aim = match best {
        Some((_, _, f)) => state.folk[f].pos + wander,
        None => {
            // Wait over a bell the three still need.
            let needed = ALL_BELLS & !state.team_bells(1);
            let bells: Vec<usize> = (0..BELLS).filter(|b| needed & 1 << b != 0).collect();
            let pick = roll(
                state,
                seat,
                &[state.round as u64, (state.tick / PATROL_WINDOW) as u64, 3],
            );
            bells
                .get((pick % bells.len().max(1) as u64) as usize)
                .map_or(Vec2::ZERO, |b| BELL_POSITIONS[*b])
        }
    };
    let to = aim - sight.pos;
    let push = (to.length() * 1.5).min(1.0);
    let mut input = stick_towards(to);
    input.stick_x = (input.stick_x as f32 * push).round() as i8;
    input.stick_y = (input.stick_y as f32 * push).round() as i8;
    let sure = best.is_some_and(|(s, _, f)| {
        let window = (state.tick / BOLD_WINDOW) as u64;
        let bold = roll(state, seat, &[state.round as u64, f as u64, window, 5]);
        let bold = (bold % skill.bold.max(1) as u64) as u32;
        s + bold >= skill.catch_at && state.folk[f].pos.distance(sight.pos) < CATCH_REACH * 0.8
    });
    input.set(Button::A, t >= SHUFFLE_TICKS && sight.dark == 0 && sure);
    input
}
