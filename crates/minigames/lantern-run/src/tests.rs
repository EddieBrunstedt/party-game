use super::*;
use sim_core::{Run, RunConfig};

const NONE: [Input; MAX_PLAYERS] = [Input {
    buttons: 0,
    stick_x: 0,
    stick_y: 0,
}; MAX_PLAYERS];

fn new_run(players: usize, format: Format) -> Run<LanternRun> {
    Run::new(1, PlayerSetup::first(players), format, RunConfig::default())
}

/// A run in play with nothing on the course but `obstacles`.
fn playing(players: usize, format: Format, obstacles: Vec<Obstacle>) -> Run<LanternRun> {
    let mut run = new_run(players, format);
    run.game.course = Course {
        obstacles: obstacles.into(),
    };
    while run.game.phase != Phase::Playing {
        run.step(NONE);
    }
    run
}

fn place(run: &mut Run<LanternRun>, seat: usize, x: f32, y: f32, z: f32) {
    let ship = &mut run.game.ships[seat];
    ship.body.pos = Vec2::new(x, y);
    ship.body.vel = Vec2::ZERO;
    ship.z = z;
    ship.speed = CRUISE;
}

fn inputs(seat: usize, x: i8, y: i8, buttons: &[Button]) -> [Input; MAX_PLAYERS] {
    let mut inputs = NONE;
    inputs[seat].stick_x = x;
    inputs[seat].stick_y = y;
    for b in buttons {
        inputs[seat].set(*b, true);
    }
    inputs
}

/// Somewhere in the cave, where falling behind puts you out.
const CAVE: f32 = course::CAVE_Z + 50.0;

fn rock(x: f32, y: f32, z: f32, radius: f32) -> Obstacle {
    Obstacle {
        z,
        depth: 1.0,
        shape: Shape::Rock {
            center: Vec2::new(x, y),
            radius,
        },
    }
}

#[test]
fn nobody_moves_during_the_intro() {
    let mut run = new_run(2, Format::FreeForAll);
    let start = run.game.ships[0];
    for _ in 0..INTRO_TICKS - 1 {
        run.step(inputs(0, 127, 0, &[Button::A]));
    }
    assert_eq!(run.game.ships[0].z, start.z);
    assert_eq!(run.game.ships[0].body.pos, start.body.pos);
}

#[test]
fn everyone_starts_side_by_side_and_the_barge_ahead() {
    let run = new_run(4, Format::FreeForAll);
    let xs: Vec<f32> = run.game.ships.iter().map(|s| s.body.pos.x).collect();
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
    assert!(run.game.ships.iter().all(|s| s.z == 0.0 && s.flying()));

    let run = new_run(4, Format::OneVsThree { solo: Seat(2) });
    let barge = run.game.ships[2];
    assert!(barge.barge);
    assert_eq!(barge.z, HEAD_START);
    assert_eq!(barge.body.radius, BARGE_RADIUS);
    assert!(
        run.game
            .ships
            .iter()
            .filter(|s| !s.barge)
            .all(|s| s.z == 0.0)
    );
}

#[test]
fn everyone_flies_forward_at_cruising_speed() {
    let mut run = playing(2, Format::FreeForAll, vec![]);
    let z = run.game.ships[0].z;
    for _ in 0..60 {
        run.step(NONE);
    }
    assert!((run.game.ships[0].z - z - CRUISE).abs() < 0.5);
}

#[test]
fn the_stick_steers_across_and_the_glider_drifts_on() {
    let mut run = playing(2, Format::FreeForAll, vec![]);
    place(&mut run, 0, 0.0, 0.0, 10.0);
    for _ in 0..20 {
        run.step(inputs(0, 0, 127, &[]));
    }
    let ship = run.game.ships[0];
    assert!(
        ship.body.pos.y > 1.0,
        "stick up goes up: {:?}",
        ship.body.pos
    );
    assert!(ship.body.vel.y > 4.0);
    // Let go and it keeps sliding for a moment.
    run.step(NONE);
    assert!(run.game.ships[0].body.vel.y > 3.0);
}

#[test]
fn the_walls_keep_gliders_in() {
    let mut run = playing(2, Format::FreeForAll, vec![]);
    place(&mut run, 0, 0.0, 0.0, 10.0);
    for _ in 0..200 {
        run.step(inputs(0, 127, 0, &[]));
    }
    let ship = run.game.ships[0];
    assert!(ship.body.pos.x <= half_extents(ship.z).x - SHIP_RADIUS + 1e-4);
}

#[test]
fn the_cave_closes_in() {
    assert_eq!(half_extents(0.0), course::CANYON_HALF);
    assert_eq!(half_extents(course::CAVE_Z), course::MOUTH_HALF);
    let narrowest = half_extents(course::CAVE_Z + course::SHRINK_LENGTH + 100.0);
    assert_eq!(narrowest, course::NARROWEST_HALF);
    let middle = half_extents(course::CAVE_Z + course::SHRINK_LENGTH * 0.5);
    assert!(middle.x < course::MOUTH_HALF.x && middle.x > narrowest.x);
}

#[test]
fn every_obstacle_leaves_a_way_through() {
    for seed in 0..20 {
        let course = Course::new(seed);
        assert!(course.obstacles.len() > 80);
        assert!(course.obstacles.windows(2).all(|w| w[0].z < w[1].z));
        for obstacle in course.obstacles.iter() {
            let half = half_extents(obstacle.z);
            let room = half - Vec2::new(SHIP_RADIUS, SHIP_RADIUS);
            let free = (0..=40).any(|i| {
                (0..=20).any(|j| {
                    let p = Vec2::new(
                        -room.x + room.x * 2.0 * i as f32 / 40.0,
                        -room.y + room.y * 2.0 * j as f32 / 20.0,
                    );
                    obstacle.clearance(p) > SHIP_RADIUS + 0.1
                })
            });
            assert!(free, "seed {seed}: no way past {obstacle:?}");
        }
    }
}

#[test]
fn hitting_an_obstacle_spins_you_out() {
    let mut run = playing(2, Format::FreeForAll, vec![rock(0.0, 0.0, 12.0, 1.0)]);
    place(&mut run, 0, 0.0, 0.0, 11.0);
    place(&mut run, 1, 6.0, 0.0, 11.0);
    for _ in 0..6 {
        run.step(NONE);
    }
    let ship = run.game.ships[0];
    assert_eq!(ship.crashes, 1);
    assert!(ship.speed < CRUISE * 0.7);
    assert!(ship.stun > 0 && ship.ghost > 0);
    assert_eq!(run.game.ships[1].crashes, 0);
    // It recovers its speed in time.
    for _ in 0..180 {
        run.step(NONE);
    }
    assert_eq!(run.game.ships[0].speed, CRUISE);
    assert_eq!(run.game.ships[0].crashes, 1);
}

#[test]
fn grazing_an_obstacle_charges_the_boost() {
    let mut run = playing(2, Format::FreeForAll, vec![rock(0.0, 0.0, 12.0, 1.0)]);
    place(&mut run, 0, 1.0 + SHIP_RADIUS + 0.1, 0.0, 11.0);
    place(&mut run, 1, -6.0, 0.0, 11.0);
    for _ in 0..10 {
        run.step(NONE);
    }
    let (grazer, other) = (run.game.ships[0], run.game.ships[1]);
    assert_eq!(grazer.crashes, 0);
    assert!(
        grazer.meter > other.meter + 50,
        "{} vs {}",
        grazer.meter,
        other.meter
    );
}

#[test]
fn a_boost_needs_a_charge_and_speeds_you_up() {
    let mut run = playing(2, Format::FreeForAll, vec![]);
    run.step(inputs(0, 0, 0, &[Button::A]));
    assert_eq!(run.game.ships[0].boost, 0);
    run.step(NONE);
    run.game.ships[0].meter = BOOST_COST + 10;
    run.step(inputs(0, 0, 0, &[Button::A]));
    let ship = run.game.ships[0];
    assert!(ship.boost > 0);
    assert!(ship.meter < 100);
    assert!(ship.speed >= CRUISE * BOOST_SPEED);
}

#[test]
fn a_boost_rams_rivals_aside() {
    let mut run = playing(2, Format::FreeForAll, vec![]);
    place(&mut run, 0, 0.0, 0.0, 10.0);
    place(&mut run, 1, 0.8, 0.0, 10.0);
    run.game.ships[0].boost = 20;
    run.step(NONE);
    let rival = run.game.ships[1];
    assert!(rival.body.vel.x > RAM_PUSH * 0.9);
    assert!(rival.stun > 0);
}

#[test]
fn a_slipstream_speeds_you_up() {
    let mut run = playing(2, Format::FreeForAll, vec![]);
    place(&mut run, 0, 0.0, 0.0, 30.0);
    place(&mut run, 1, 0.2, 0.0, 25.0);
    for _ in 0..30 {
        run.step(NONE);
    }
    assert!(run.game.ships[1].drafting);
    assert!(!run.game.ships[0].drafting);
    assert!(run.game.ships[1].speed > CRUISE + 1.0);
    assert_eq!(run.game.ships[0].speed, CRUISE);
}

#[test]
fn the_grapple_tows_you_and_drags_them_back_until_you_let_go() {
    let mut run = playing(2, Format::FreeForAll, vec![]);
    place(&mut run, 0, 0.0, 0.0, 30.0);
    place(&mut run, 1, 2.0, 0.0, 22.0);
    run.step(inputs(1, 0, 0, &[Button::B]));
    assert_eq!(
        run.game.ships[1].hook,
        Some(Hook {
            target: Seat(0),
            t: 1
        })
    );
    for _ in 0..20 {
        run.step(inputs(1, 0, 0, &[Button::B]));
    }
    let (ahead, behind) = (run.game.ships[0], run.game.ships[1]);
    assert!(behind.speed > CRUISE + 2.0);
    assert!(ahead.speed < CRUISE - 2.0);
    assert!(behind.body.pos.x < 2.0, "the rope pulls you across");
    run.step(NONE);
    assert_eq!(run.game.ships[1].hook, None);
    assert!(run.game.ships[1].hook_cooldown > 0);
}

#[test]
fn a_hook_lets_go_once_you_are_level() {
    let mut run = playing(2, Format::FreeForAll, vec![]);
    place(&mut run, 0, 0.0, 0.0, 30.0);
    place(&mut run, 1, 2.0, 0.0, 27.0);
    run.step(inputs(1, 0, 0, &[Button::B]));
    let mut let_go = false;
    for _ in 0..HOOK_TICKS {
        run.step(inputs(1, 0, 0, &[Button::B]));
        if run.game.ships[1].hook.is_none() {
            let_go = true;
            break;
        }
    }
    assert!(let_go);
    assert!(run.game.ships[1].z > run.game.ships[0].z - SHIP_LENGTH);
}

#[test]
fn nothing_in_reach_is_a_miss() {
    let mut run = playing(2, Format::FreeForAll, vec![]);
    place(&mut run, 0, 0.0, 0.0, 30.0);
    place(&mut run, 1, 0.0, 0.0, 10.0);
    run.step(inputs(1, 0, 0, &[Button::B]));
    assert_eq!(run.game.ships[1].hook, None);
    assert_eq!(run.game.ships[1].hook_cooldown, MISS_COOLDOWN);
}

#[test]
fn you_cant_hook_through_an_obstacle() {
    let mut run = playing(2, Format::FreeForAll, vec![rock(1.0, 0.0, 26.0, 0.8)]);
    place(&mut run, 0, 2.0, 0.0, 30.0);
    place(&mut run, 1, 0.0, 0.0, 22.0);
    assert_eq!(run.game.hook_target(Seat(1)), None);
}

#[test]
fn a_rope_that_catches_on_an_obstacle_yanks_you_into_it() {
    let pillar = Obstacle {
        z: 26.0,
        depth: 1.0,
        shape: Shape::Pillar {
            x: 0.0,
            half_width: 0.5,
        },
    };
    let mut run = playing(2, Format::FreeForAll, vec![pillar]);
    place(&mut run, 0, 2.0, 0.0, 31.0);
    place(&mut run, 1, 2.0, 0.0, 22.0);
    run.step(inputs(1, 0, 0, &[Button::B]));
    assert!(run.game.ships[1].hook.is_some());
    // The one ahead swings across behind the pillar.
    run.game.ships[0].body.pos.x = -2.0;
    run.step(inputs(1, 0, 0, &[Button::B]));
    let ship = run.game.ships[1];
    assert_eq!(ship.hook, None);
    assert_eq!(ship.crashes, 1);
    assert!(ship.snagged_at.is_some());
}

#[test]
fn a_partners_tow_costs_them_nothing() {
    let format = Format::TwoVsTwo {
        team_of: [0, 0, 1, 1],
    };
    let mut run = playing(4, format, vec![]);
    place(&mut run, 0, 0.0, 0.0, 30.0);
    place(&mut run, 1, 1.0, 0.0, 22.0);
    place(&mut run, 2, -6.0, 0.0, 30.0);
    place(&mut run, 3, 6.0, 0.0, 30.0);
    run.step(inputs(1, 0, 0, &[Button::B]));
    for _ in 0..15 {
        run.step(inputs(1, 0, 0, &[Button::B]));
    }
    assert!(run.game.ships[1].speed > CRUISE + 3.0);
    assert_eq!(run.game.ships[0].speed, CRUISE);
}

#[test]
fn falling_behind_the_camera_puts_you_out_and_the_last_one_flying_wins() {
    let mut run = playing(3, Format::FreeForAll, vec![]);
    place(&mut run, 0, -3.0, 0.0, CAVE);
    place(&mut run, 1, 0.0, 0.0, CAVE - WINDOW_START - 1.0);
    place(&mut run, 2, 3.0, 0.0, CAVE - 1.0);
    run.step(NONE);
    assert!(matches!(run.game.ships[1].status, Status::Out { .. }));
    assert!(run.game.ships[0].flying() && run.game.ships[2].flying());
    run.game.ships[2].z = run.game.camera_z - 1.0;
    while run.outcome().is_none() {
        run.step(NONE);
    }
    assert_eq!(
        run.outcome().unwrap().placement,
        [Some(1), Some(3), Some(2), None]
    );
}

#[test]
fn in_the_canyon_the_camera_pushes_stragglers_on() {
    let mut run = playing(2, Format::FreeForAll, vec![]);
    place(&mut run, 0, -3.0, 0.0, 100.0);
    place(&mut run, 1, 3.0, 0.0, 50.0);
    run.step(NONE);
    let straggler = run.game.ships[1];
    assert!(straggler.flying());
    assert_eq!(straggler.z, run.game.camera_z + CANYON_CUSHION);
}

#[test]
fn at_time_up_the_furthest_along_wins() {
    let mut run = playing(2, Format::FreeForAll, vec![]);
    run.game.tick = PLAY_TICKS - 1;
    place(&mut run, 0, -3.0, 0.0, 40.0);
    place(&mut run, 1, 3.0, 0.0, 41.0);
    while run.outcome().is_none() {
        run.step(NONE);
    }
    assert_eq!(
        run.outcome().unwrap().placement,
        [Some(2), Some(1), None, None]
    );
}

#[test]
fn a_team_is_out_when_both_partners_are() {
    let format = Format::TwoVsTwo {
        team_of: [0, 1, 0, 1],
    };
    let mut run = playing(4, format, vec![]);
    for seat in 0..4 {
        place(&mut run, seat, seat as f32 * 2.0 - 3.0, 0.0, CAVE);
    }
    run.game.ships[0].z = CAVE - 30.0;
    run.step(NONE);
    assert!(run.game.outcome.is_none(), "a partner still flies");
    run.game.ships[2].z = CAVE - 30.0;
    while run.outcome().is_none() {
        run.step(NONE);
    }
    assert_eq!(
        run.outcome().unwrap().placement,
        [Some(2), Some(1), Some(2), Some(1)]
    );
}

#[test]
fn the_barge_drops_mines_behind_it() {
    let mut run = playing(4, Format::OneVsThree { solo: Seat(0) }, vec![]);
    run.step(inputs(0, 0, 0, &[Button::A]));
    assert_eq!(run.game.mines.len(), 1);
    let mine = run.game.mines[0];
    assert!(mine.z < run.game.ships[0].z);
    run.step(NONE);
    run.step(inputs(0, 0, 0, &[Button::A]));
    assert_eq!(run.game.mines.len(), 1, "it has a cooldown");
    assert_eq!(run.game.ships[0].boost, 0, "the barge doesn't boost");
}

#[test]
fn a_mine_crashes_whoever_hits_it() {
    let mut run = playing(4, Format::OneVsThree { solo: Seat(0) }, vec![]);
    place(&mut run, 0, 0.0, 0.0, 40.0);
    place(&mut run, 1, 0.0, 0.0, 30.0);
    run.game.mines.push(mine(Vec2::ZERO, 31.0));
    for _ in 0..6 {
        run.step(NONE);
    }
    assert_eq!(run.game.ships[1].crashes, 1);
}

#[test]
fn the_three_win_by_getting_past_the_barge() {
    let mut run = playing(4, Format::OneVsThree { solo: Seat(0) }, vec![]);
    let barge_z = run.game.ships[0].z;
    run.game.ships[2].z = barge_z + 0.1;
    while run.outcome().is_none() {
        run.step(NONE);
    }
    assert_eq!(
        run.outcome().unwrap().placement,
        [Some(2), Some(1), Some(1), Some(1)]
    );
}

#[test]
fn the_barge_wins_if_still_ahead_at_the_end() {
    let mut run = playing(4, Format::OneVsThree { solo: Seat(3) }, vec![]);
    run.game.tick = BARGE_TICKS - 1;
    while run.outcome().is_none() {
        run.step(NONE);
    }
    assert_eq!(
        run.outcome().unwrap().placement,
        [Some(2), Some(2), Some(2), Some(1)]
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
    assert_eq!(run.game.ships[0].status, Status::Benched);
    assert_eq!(run.game.ships[2].status, Status::Benched);
    assert_eq!(run.game.playing().count(), 2);
}

#[test]
fn passes_all_automated_quality_bars() {
    sim_check::check_all::<LanternRun>(3).unwrap();
}
