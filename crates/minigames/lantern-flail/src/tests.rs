use super::*;
use sim_core::{Run, RunConfig};

const NONE: [Input; MAX_PLAYERS] = [Input {
    buttons: 0,
    stick_x: 0,
    stick_y: 0,
}; MAX_PLAYERS];

fn new_run(players: usize, format: Format) -> Run<LanternFlail> {
    Run::new(1, PlayerSetup::first(players), format, RunConfig::default())
}

fn to_playing(run: &mut Run<LanternFlail>) {
    while run.game.phase != Phase::Playing {
        run.step(NONE);
    }
}

/// Puts a seat somewhere still, with its lantern resting just beside it, and the barrels away.
fn place(run: &mut Run<LanternFlail>, seat: usize, x: f32, y: f32) {
    let game = &mut run.game;
    game.folk[seat].body.pos = Vec2::new(x, y);
    game.folk[seat].body.vel = Vec2::ZERO;
    if let Some(l) = game.lantern_of[seat] {
        let lantern = &mut game.lanterns[l as usize];
        lantern.body.pos = Vec2::new(x, y + 0.9);
        lantern.body.vel = Vec2::ZERO;
    }
    for barrel in &mut game.barrels {
        barrel.status = BarrelStatus::Gone { t: 0 };
    }
}

fn lantern_mut(run: &mut Run<LanternFlail>, seat: usize) -> &mut Lantern {
    let l = run.game.lantern_of[seat].unwrap() as usize;
    &mut run.game.lanterns[l]
}

fn stick(seat: usize, x: i8, y: i8, buttons: &[Button]) -> [Input; MAX_PLAYERS] {
    let mut inputs = NONE;
    inputs[seat].stick_x = x;
    inputs[seat].stick_y = y;
    for b in buttons {
        inputs[seat].set(*b, true);
    }
    inputs
}

/// Throws seat 0's lantern at seat 1 (standing at the origin) from the left at `speed`.
fn throw_at_seat_1(run: &mut Run<LanternFlail>, speed: f32) {
    place(run, 0, -3.0, 0.0);
    place(run, 1, 0.0, 0.0);
    let lantern = lantern_mut(run, 0);
    lantern.body.pos = Vec2::new(-0.9, 0.0);
    lantern.body.vel = Vec2::new(speed, 0.0);
}

#[test]
fn nobody_moves_during_the_intro() {
    let mut run = new_run(2, Format::FreeForAll);
    let start = run.game.folk[0].body.pos;
    for _ in 0..INTRO_TICKS - 1 {
        run.step(stick(0, 127, 0, &[Button::A]));
    }
    assert_eq!(run.game.folk[0].body.pos, start);
}

#[test]
fn everyone_starts_on_the_raft_with_their_lantern_on_a_slack_rope() {
    for format in [
        Format::FreeForAll,
        Format::OneVsThree { solo: Seat(2) },
        Format::TwoVsTwo {
            team_of: [0, 1, 1, 0],
        },
    ] {
        let run = new_run(4, format);
        for seat in Seat::ALL {
            let folk = run.game.folk(seat);
            assert!(folk.standing());
            assert!(folk.body.pos.length() < RAFT_RADIUS - 1.0);
            let lantern = run.game.lantern(seat).unwrap();
            for slot in 0..2 {
                if let Some(holder) = lantern.holders[slot] {
                    let span = lantern.body.pos.distance(run.game.folk(holder).body.pos);
                    assert!(span < lantern.rope_length(slot), "{format:?}");
                }
            }
        }
    }
}

#[test]
fn teams_share_a_bolas_and_the_giant_swings_a_wrecking_ball() {
    let run = new_run(
        4,
        Format::TwoVsTwo {
            team_of: [0, 1, 1, 0],
        },
    );
    assert_eq!(run.game.lanterns.len(), 2);
    assert_eq!(run.game.lantern_of[0], run.game.lantern_of[3]);
    assert_eq!(run.game.lantern(Seat(1)).unwrap().kind, Kind::Bolas);

    let run = new_run(4, Format::OneVsThree { solo: Seat(1) });
    assert!(run.game.folk[1].giant);
    assert_eq!(run.game.folk[1].body.mass, GIANT_MASS);
    assert_eq!(run.game.lantern(Seat(1)).unwrap().kind, Kind::WreckingBall);
    assert_eq!(run.game.lantern(Seat(0)).unwrap().kind, Kind::Flail);
}

#[test]
fn a_rope_drags_the_lantern_after_you() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    place(&mut run, 0, -2.0, 0.0);
    for _ in 0..60 {
        run.step(stick(0, 127, 0, &[]));
    }
    let folk = run.game.folk[0].body.pos;
    let lantern = run.game.lantern(Seat(0)).unwrap();
    assert!(folk.x > 1.0);
    assert!(lantern.body.pos.distance(folk) <= lantern.rope_length(0) + 1e-3);
    assert!(lantern.body.vel.x > 3.0, "it's pulled along");
}

#[test]
fn reeling_in_shortens_the_rope_and_letting_go_pays_it_out() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    let spec = Kind::Flail.spec();
    for _ in 0..60 {
        run.step(stick(0, 0, 0, &[Button::B]));
    }
    assert_eq!(
        run.game.lantern(Seat(0)).unwrap().rope_length(0),
        spec.rope_min
    );
    run.step(NONE);
    let paying = run.game.lantern(Seat(0)).unwrap().rope_length(0);
    assert!(paying > spec.rope_min);
    for _ in 0..30 {
        run.step(NONE);
    }
    assert_eq!(
        run.game.lantern(Seat(0)).unwrap().rope_length(0),
        spec.rope_max
    );
}

#[test]
fn reeling_in_a_swinging_lantern_spins_it_faster() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    place(&mut run, 0, 0.0, 0.0);
    let lantern = lantern_mut(&mut run, 0);
    lantern.body.pos = Vec2::new(2.4, 0.0);
    lantern.body.vel = Vec2::new(0.0, 6.0);
    let mut reeled = run.clone();
    run.step(NONE);
    reeled.step(stick(0, 0, 0, &[Button::B]));
    let free = run.game.lantern(Seat(0)).unwrap().speed();
    let spun = reeled.game.lantern(Seat(0)).unwrap().speed();
    assert!(spun > free + 0.05, "{spun} > {free}");
}

#[test]
fn a_fast_lantern_knocks_a_rival_back_and_stuns_them() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    throw_at_seat_1(&mut run, 12.0);
    run.step(NONE);
    let victim = run.game.folk(Seat(1));
    let hit = victim.last_hit.expect("struck");
    assert_eq!((hit.by, hit.source), (Seat(0), Source::Lantern));
    assert!(victim.stun > 0);
    assert!(victim.damage > 0);
    assert!(victim.body.vel.x > 4.0);
    assert!(
        run.game.freeze > 0,
        "a big hit stops everything for a moment"
    );
}

#[test]
fn a_slow_lantern_only_nudges() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    throw_at_seat_1(&mut run, HIT_MIN * 0.6);
    for _ in 0..3 {
        run.step(NONE);
    }
    let victim = run.game.folk(Seat(1));
    assert_eq!(victim.last_hit, None);
    assert_eq!((victim.stun, victim.damage), (0, 0));
}

#[test]
fn damage_makes_hits_knock_further() {
    let knock = |damage: u16| {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        throw_at_seat_1(&mut run, 9.0);
        run.game.folk[1].damage = damage;
        run.step(NONE);
        run.game.folk[1].body.vel.x
    };
    assert!(knock(100) > knock(0) * 1.8);
}

#[test]
fn your_own_lantern_passes_through_you() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    place(&mut run, 0, 0.0, 0.0);
    place(&mut run, 1, 4.0, 0.0);
    let lantern = lantern_mut(&mut run, 0);
    lantern.body.pos = Vec2::new(-0.9, 0.0);
    lantern.body.vel = Vec2::new(12.0, 0.0);
    run.step(NONE);
    assert_eq!(run.game.folk[0].last_hit, None);
}

#[test]
fn partners_bolas_never_strikes_them() {
    let mut run = new_run(
        4,
        Format::TwoVsTwo {
            team_of: [0, 0, 1, 1],
        },
    );
    to_playing(&mut run);
    place(&mut run, 0, -2.0, 0.0);
    place(&mut run, 1, 2.0, 0.0);
    place(&mut run, 2, -3.0, 4.0);
    place(&mut run, 3, 3.0, 4.0);
    let lantern = lantern_mut(&mut run, 0);
    lantern.body.pos = Vec2::new(-2.0, -0.9);
    lantern.body.vel = Vec2::new(0.0, 12.0);
    run.step(NONE);
    assert_eq!(run.game.folk[0].last_hit, None);
}

#[test]
fn a_fast_taut_rope_sweeps_a_rival_over() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    place(&mut run, 0, 0.0, 0.0);
    place(&mut run, 1, 1.2, -0.5);
    // The rope runs along +x, swinging towards -y fast enough to strike in the middle.
    let lantern = lantern_mut(&mut run, 0);
    lantern.body.pos = Vec2::new(2.4, 0.0);
    lantern.body.vel = Vec2::new(0.0, -14.0);
    run.step(NONE);
    let hit = run.game.folk[1].last_hit.expect("swept");
    assert_eq!(hit.source, Source::Rope);
    assert!(run.game.folk[1].body.vel.y < -1.0);
}

#[test]
fn a_lunge_into_a_rival_shoves_them() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    place(&mut run, 0, -1.4, 0.0);
    place(&mut run, 1, 0.0, 0.0);
    run.game.folk[0].facing = Vec2::new(1.0, 0.0);
    for _ in 0..4 {
        run.step(stick(0, 127, 0, &[Button::A]));
    }
    let hit = run.game.folk[1].last_hit.expect("shoved");
    assert_eq!((hit.by, hit.source), (Seat(0), Source::Lunge));
    assert_eq!(run.game.folk[0].lunge, 0, "the lunge stops on contact");
}

#[test]
fn a_lunge_has_a_cooldown() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    run.step(stick(0, 127, 0, &[Button::A]));
    assert_eq!(run.game.folk[0].lunge, LUNGE_TICKS);
    for _ in 0..LUNGE_TICKS + 2 {
        run.step(NONE);
    }
    run.step(stick(0, 127, 0, &[Button::A]));
    assert_eq!(run.game.folk[0].lunge, 0);
}

#[test]
fn a_barrel_sent_flying_knocks_folk_over_for_its_sender() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    place(&mut run, 0, -4.0, 2.0);
    place(&mut run, 1, 0.0, 0.0);
    let tick = run.game.tick;
    let barrel = &mut run.game.barrels[0];
    barrel.status = BarrelStatus::Floating;
    barrel.body.pos = Vec2::new(-0.8, 0.0);
    barrel.body.vel = Vec2::new(9.0, 0.0);
    barrel.pushed_by = Some((Seat(0), tick));
    run.step(NONE);
    let hit = run.game.folk[1].last_hit.expect("bowled over");
    assert_eq!((hit.by, hit.source), (Seat(0), Source::Barrel));
}

#[test]
fn a_fall_scores_for_the_last_hitter_and_you_climb_back_on() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    place(&mut run, 0, RAFT_RADIUS - 3.5, 1.0);
    place(&mut run, 1, RAFT_RADIUS - 0.6, 0.0);
    let lantern = lantern_mut(&mut run, 0);
    lantern.body.pos = Vec2::new(RAFT_RADIUS - 1.5, 0.0);
    lantern.body.vel = Vec2::new(14.0, 0.0);
    run.game.folk[1].damage = 120;
    for _ in 0..40 {
        run.step(NONE);
    }
    assert!(matches!(run.game.folk[1].status, Status::Falling { .. }));
    assert_eq!(run.game.points[0], 1);
    assert_eq!((run.game.knockoffs[0], run.game.falls[1]), (1, 1));
    for _ in 0..FALL_TICKS + RESPAWN_TICKS {
        run.step(NONE);
    }
    let folk = run.game.folk(Seat(1));
    assert!(folk.standing());
    assert_eq!(folk.damage, 0);
    assert!(folk.safe > 0);
    assert!(folk.body.pos.length() < RAFT_RADIUS - 1.0);
    let lantern = run.game.lantern(Seat(1)).unwrap();
    assert!(!lantern.down);
}

#[test]
fn walking_off_alone_scores_nobody_in_free_for_all() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    place(&mut run, 0, RAFT_RADIUS - 0.3, 0.0);
    for _ in 0..30 {
        run.step(stick(0, 127, 0, &[]));
    }
    assert_eq!(run.game.falls[0], 1);
    assert_eq!(run.game.points, [0; MAX_PLAYERS]);
}

#[test]
fn with_two_sides_any_fall_scores_for_the_other() {
    let mut run = new_run(
        2,
        Format::Duel {
            a: Seat(0),
            b: Seat(1),
        },
    );
    to_playing(&mut run);
    place(&mut run, 0, RAFT_RADIUS - 0.3, 0.0);
    for _ in 0..30 {
        run.step(stick(0, 127, 0, &[]));
    }
    assert_eq!(run.game.points[..2], [0, 1]);
}

#[test]
fn the_most_points_win_when_time_runs_out() {
    let mut run = new_run(3, Format::FreeForAll);
    to_playing(&mut run);
    run.game.points = [2, 4, 3, 0];
    while run.outcome().is_none() {
        run.step(NONE);
    }
    assert_eq!(run.game.tick, PLAY_TICKS);
    assert_eq!(
        run.outcome().unwrap().placement,
        [Some(3), Some(1), Some(2), None]
    );
}

#[test]
fn a_level_lead_goes_to_sudden_death_until_a_fall_breaks_it() {
    let mut run = new_run(3, Format::FreeForAll);
    to_playing(&mut run);
    run.game.points = [3, 3, 1, 0];
    run.game.tick = PLAY_TICKS - 1;
    run.step(NONE);
    assert!(run.game.sudden_death());
    assert_eq!(run.game.phase, Phase::Playing);
    run.game.points[1] += 1;
    run.step(NONE);
    assert_eq!(
        run.game.outcome.unwrap().placement,
        [Some(2), Some(1), Some(3), None]
    );
}

#[test]
fn sudden_death_ends_level_after_its_time() {
    let mut run = new_run(
        2,
        Format::Duel {
            a: Seat(0),
            b: Seat(1),
        },
    );
    to_playing(&mut run);
    run.game.tick = PLAY_TICKS - 1;
    while run.outcome().is_none() {
        run.step(NONE);
    }
    assert_eq!(run.game.tick, PLAY_TICKS + SUDDEN_DEATH_TICKS);
    assert_eq!(run.outcome().unwrap().placement[..2], [Some(1), Some(1)]);
}

#[test]
fn the_giant_wins_at_its_goal_and_the_three_at_theirs() {
    let mut run = new_run(4, Format::OneVsThree { solo: Seat(3) });
    to_playing(&mut run);
    run.game.points[0] = GIANT_GOAL;
    run.step(NONE);
    assert!(matches!(run.game.phase, Phase::Decided { .. }));
    assert_eq!(
        run.game.outcome.unwrap().placement,
        [Some(2), Some(2), Some(2), Some(1)]
    );

    let mut run = new_run(4, Format::OneVsThree { solo: Seat(3) });
    to_playing(&mut run);
    run.game.points[1] = TRIO_GOAL;
    run.step(NONE);
    assert_eq!(
        run.game.outcome.unwrap().placement,
        [Some(1), Some(1), Some(1), Some(2)]
    );
}

#[test]
fn at_time_out_1v3_goes_to_the_side_nearer_its_goal() {
    let mut run = new_run(4, Format::OneVsThree { solo: Seat(0) });
    to_playing(&mut run);
    // 3 of the giant's 8 is further than 1 of the three's 3.
    assert_eq!((GIANT_GOAL, TRIO_GOAL), (8, 3));
    run.game.points[0] = 3;
    run.game.points[1] = 1;
    run.game.tick = PLAY_TICKS - 1;
    run.step(NONE);
    assert_eq!(
        run.game.outcome.unwrap().placement,
        [Some(1), Some(2), Some(2), Some(2)]
    );
}

#[test]
fn a_benched_duel_seat_stays_out_of_it() {
    let run = new_run(
        4,
        Format::Duel {
            a: Seat(1),
            b: Seat(3),
        },
    );
    assert_eq!(run.game.folk[0].status, Status::Benched);
    assert_eq!(run.game.lantern_of[0], None);
    assert_eq!(run.game.lanterns.len(), 2);
}

#[test]
fn passes_all_automated_quality_bars() {
    sim_check::check_all::<LanternFlail>(4).unwrap();
}
