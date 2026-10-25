use super::*;
use sim_core::{Run, RunConfig};

const NONE: [Input; MAX_PLAYERS] = [Input {
    buttons: 0,
    stick_x: 0,
    stick_y: 0,
}; MAX_PLAYERS];

const TEAMS: Format = Format::TwoVsTwo {
    team_of: [0, 1, 1, 0],
};

fn new_run(players: usize, format: Format) -> Run<LanternBall> {
    Run::new(1, PlayerSetup::first(players), format, RunConfig::default())
}

fn to_playing(run: &mut Run<LanternBall>) {
    while run.game.phase != Phase::Playing {
        run.step(NONE);
    }
}

/// Puts a seat somewhere still, facing `facing`, with the bumpers out of the way.
fn place(run: &mut Run<LanternBall>, seat: usize, x: f32, y: f32) {
    let game = &mut run.game;
    game.folk[seat].body.pos = Vec2::new(x, y);
    game.folk[seat].body.vel = Vec2::ZERO;
    game.bumpers.clear();
}

fn place_ball(run: &mut Run<LanternBall>, x: f32, y: f32, vel: Vec2) {
    let ball = &mut run.game.balls[0];
    ball.body.pos = Vec2::new(x, y);
    ball.body.vel = vel;
    ball.status = BallStatus::Rolling;
}

fn input(seat: usize, x: i8, y: i8, buttons: &[Button]) -> [Input; MAX_PLAYERS] {
    let mut inputs = NONE;
    inputs[seat].stick_x = x;
    inputs[seat].stick_y = y;
    for b in buttons {
        inputs[seat].set(*b, true);
    }
    inputs
}

/// Moves everyone up the pitch, side by side, and takes the ball away.
fn clear_pitch(run: &mut Run<LanternBall>) {
    run.game.balls[0].status = BallStatus::Waiting;
    let seats: Vec<Seat> = run.game.playing().collect();
    for (k, seat) in seats.into_iter().enumerate() {
        place(run, seat.index(), -3.0 + k as f32 * 2.0, -5.0);
    }
}

#[test]
fn nobody_moves_during_the_intro() {
    let mut run = new_run(2, Format::FreeForAll);
    let start = run.game.folk[0].body.pos;
    for _ in 0..INTRO_TICKS - 1 {
        run.step(input(0, 127, 0, &[Button::A]));
    }
    assert_eq!(run.game.folk[0].body.pos, start);
}

#[test]
fn everyone_starts_in_front_of_their_own_goal_the_same_way_from_the_ball() {
    for players in 2..=4 {
        let run = new_run(players, Format::FreeForAll);
        let game = &run.game;
        let distance = game.folk[0].body.pos.length();
        for seat in game.playing() {
            let side = &game.sides[game.goal_side(seat.0).unwrap()];
            let pos = game.folk(seat).body.pos;
            // Just outside the crease.
            let crease = side.crease().unwrap();
            let out = pos.distance(side.center()) - crease - FOLK_RADIUS;
            assert!(out > 0.0 && out < 1.0, "{players} {seat:?}");
            assert!((pos.length() - distance).abs() < 1e-3, "{players} {seat:?}");
            assert!(side.lateral(pos).abs() < 1e-3);
        }
        assert_eq!(
            game.sides.iter().filter(|s| s.goal.is_some()).count(),
            players
        );
    }
}

#[test]
fn the_pitch_is_closed_round_the_middle() {
    for (players, format) in [
        (4, Format::FreeForAll),
        (3, Format::FreeForAll),
        (2, Format::FreeForAll),
        (4, TEAMS),
        (4, Format::OneVsThree { solo: Seat(1) }),
        (
            4,
            Format::Duel {
                a: Seat(0),
                b: Seat(2),
            },
        ),
    ] {
        let game = new_run(players, format).game;
        // Walk out from the middle in every direction: a wall always comes before 12 units.
        for k in 0..64 {
            let dir = Vec2::from_angle(TAU * k as f32 / 64.0);
            let wall = game
                .sides
                .iter()
                .filter(|s| dir.dot(s.normal) > 1e-3)
                .map(|s| s.offset / dir.dot(s.normal))
                .fold(f32::MAX, f32::min);
            assert!(wall > 5.0 && wall < 12.0, "{format:?} {k}: {wall}");
        }
        // Every bumper is clear of the middle and of the walls.
        for bumper in &game.bumpers {
            assert!(bumper.length() > 2.5, "{format:?}");
            for side in &game.sides {
                assert!(side.depth(*bumper) < -1.5, "{format:?} {bumper:?}");
            }
        }
    }
}

#[test]
fn bumpers_look_the_same_from_every_goal() {
    for seed in 0..20 {
        for players in [3, 4] {
            let game = LanternBall::new(seed, PlayerSetup::first(players), Format::FreeForAll);
            let seen = |owner: u8| {
                let side = &game.sides[game.goal_side(owner).unwrap()];
                let mut view: Vec<(i32, i32)> = game
                    .bumpers
                    .iter()
                    .map(|b| {
                        (
                            (side.lateral(*b) * 100.0).round() as i32,
                            (side.depth(*b) * 100.0).round() as i32,
                        )
                    })
                    .collect();
                view.sort();
                view
            };
            for owner in 1..players as u8 {
                assert_eq!(seen(0), seen(owner), "seed {seed}, {players} players");
            }
        }
        let game = LanternBall::new(seed, PlayerSetup::first(4), TEAMS);
        for b in &game.bumpers {
            assert!(game.bumpers.iter().any(|o| o.distance(-*b) < 1e-4));
        }
    }
}

#[test]
fn a_dash_sends_the_ball_flying() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    clear_pitch(&mut run);
    place(&mut run, 0, -2.0, 0.0);
    place_ball(&mut run, 0.0, 0.0, Vec2::ZERO);
    run.game.folk[0].facing = Vec2::new(1.0, 0.0);
    run.step(input(0, 127, 0, &[Button::A]));
    for _ in 0..12 {
        run.step(input(0, 127, 0, &[Button::A]));
    }
    assert!(run.game.balls[0].body.vel.x > DASH_BALL_SPEED * 0.8);
    assert_eq!(run.game.balls[0].touched, Some(Seat(0)));
}

#[test]
fn walking_into_the_ball_only_shoves_it() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    clear_pitch(&mut run);
    place(&mut run, 0, -2.0, 0.0);
    place_ball(&mut run, 0.0, 0.0, Vec2::ZERO);
    let mut fastest: f32 = 0.0;
    for _ in 0..40 {
        run.step(input(0, 127, 0, &[]));
        fastest = fastest.max(run.game.balls[0].body.vel.length());
    }
    assert!(fastest > 2.0 && fastest < MOVE_SPEED + 1.0, "{fastest}");
}

#[test]
fn a_charged_kick_goes_where_you_face_and_harder_for_longer() {
    let kick = |hold: u32| {
        let mut run = new_run(2, Format::FreeForAll);
        to_playing(&mut run);
        clear_pitch(&mut run);
        place(&mut run, 0, 0.0, 0.0);
        place_ball(
            &mut run,
            0.0,
            -(FOLK_RADIUS + BALL_RADIUS + 0.2),
            Vec2::ZERO,
        );
        // Face up the pitch, then charge without moving.
        run.game.folk[0].facing = Vec2::new(0.0, -1.0);
        for _ in 0..hold {
            run.step(input(0, 0, 0, &[Button::B]));
        }
        run.game.balls[0].body.pos = Vec2::new(0.0, -(FOLK_RADIUS + BALL_RADIUS + 0.2));
        run.game.balls[0].body.vel = Vec2::ZERO;
        run.game.folk[0].body.pos = Vec2::ZERO;
        run.step(NONE);
        run.game.balls[0].body.vel
    };
    let tap = kick(1);
    let full = kick(CHARGE_TICKS as u32 + 20);
    assert!(tap.x.abs() < 1e-3 && full.x.abs() < 1e-3);
    assert!((-tap.y - KICK_MIN).abs() < 0.5, "{tap:?}");
    assert!((-full.y - KICK_MAX).abs() < 0.5, "{full:?}");
}

#[test]
fn charging_slows_you_down_and_a_kick_out_of_reach_whiffs() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    clear_pitch(&mut run);
    place(&mut run, 0, 0.0, 0.0);
    place_ball(&mut run, 4.0, 3.0, Vec2::ZERO);
    run.step(input(0, 127, 0, &[Button::B]));
    for _ in 0..60 {
        run.step(input(0, 127, 0, &[Button::B]));
    }
    let speed = run.game.folk[0].body.vel.length();
    assert!((speed - MOVE_SPEED * CHARGE_SLOW).abs() < 0.1, "{speed}");
    run.step(NONE);
    assert_eq!(run.game.balls[0].body.vel, Vec2::ZERO);
    assert!(!run.game.folk[0].charging);
}

#[test]
fn a_dash_into_a_charging_rival_stuns_them_and_spoils_the_kick() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    clear_pitch(&mut run);
    place(&mut run, 0, -2.0, 0.0);
    place(&mut run, 1, 0.0, 0.0);
    run.game.folk[0].facing = Vec2::new(1.0, 0.0);
    let mut inputs = input(0, 127, 0, &[Button::A]);
    inputs[1].set(Button::B, true);
    for _ in 0..12 {
        run.step(inputs);
    }
    let victim = run.game.folk[1];
    assert!(victim.stun > 0 && !victim.charging && victim.charge == 0);
    assert!(victim.body.vel.x > 3.0);
    assert_eq!(victim.knocked.map(|k| k.1), Some(Some(Seat(0))));
    assert_eq!(run.game.folk[0].stun, 0);
}

#[test]
fn a_head_on_clash_knocks_both_back() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    clear_pitch(&mut run);
    place(&mut run, 0, -1.5, 0.0);
    place(&mut run, 1, 1.5, 0.0);
    run.game.folk[0].facing = Vec2::new(1.0, 0.0);
    run.game.folk[1].facing = Vec2::new(-1.0, 0.0);
    let mut inputs = input(0, 127, 0, &[Button::A]);
    inputs[1] = input(1, -127, 0, &[Button::A])[1];
    for _ in 0..10 {
        run.step(inputs);
    }
    let (a, b) = (run.game.folk[0], run.game.folk[1]);
    assert!(a.stun > 0 && b.stun > 0);
    assert!(a.body.vel.x < 0.0 && b.body.vel.x > 0.0);
    assert!((a.body.vel.x + b.body.vel.x).abs() < 1e-3);
}

#[test]
fn teammates_dont_knock_each_other_down() {
    let mut run = new_run(4, TEAMS);
    to_playing(&mut run);
    clear_pitch(&mut run);
    place(&mut run, 0, -1.5, 3.0);
    place(&mut run, 3, 0.0, 3.0);
    run.game.folk[0].facing = Vec2::new(1.0, 0.0);
    for _ in 0..10 {
        run.step(input(0, 127, 0, &[Button::A]));
    }
    assert_eq!(run.game.folk[3].stun, 0);
}

#[test]
fn a_fast_ball_bowls_folk_over() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    clear_pitch(&mut run);
    place(&mut run, 1, 0.0, 0.0);
    place_ball(&mut run, -2.0, 0.0, Vec2::new(18.0, 0.0));
    for _ in 0..10 {
        run.step(NONE);
    }
    let folk = run.game.folk[1];
    assert!(folk.stun > 0 && folk.body.vel.x > 8.0);
    assert_eq!(folk.knocked.map(|k| k.1), Some(None));
}

#[test]
fn walls_and_bumpers_bounce_the_ball() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    clear_pitch(&mut run);
    // Straight up into the top wall.
    place_ball(&mut run, 3.0, 0.0, Vec2::new(0.0, -10.0));
    for _ in 0..60 {
        run.step(NONE);
    }
    assert!(run.game.balls[0].body.vel.y > 0.0);
    assert!(run.game.balls[0].bounced_at.is_some());
    // A slow ball off a bumper comes away fast.
    run.game.bumpers = vec![Vec2::new(0.0, 0.0)];
    place_ball(&mut run, -2.0, 0.0, Vec2::new(4.0, 0.0));
    for _ in 0..30 {
        run.step(NONE);
    }
    assert!(run.game.balls[0].body.vel.x < -BUMPER_KICK * 0.6);
}

#[test]
fn nobody_but_the_keeper_can_enter_a_crease_and_the_ball_can() {
    let mut run = new_run(4, Format::OneVsThree { solo: Seat(0) });
    to_playing(&mut run);
    clear_pitch(&mut run);
    let wide = run.game.sides[run.game.goal_side(0).unwrap()];
    let small = run.game.sides[run.game.goal_side(1).unwrap()];
    // The keeper walks to its line; one of the three tries to walk into the keeper's goal, and
    // another into its own.
    place(&mut run, 0, wide.center().x + 3.0, 0.0);
    place(&mut run, 1, wide.center().x + 5.0, 2.0);
    place(&mut run, 2, small.center().x - 5.0, 0.0);
    for _ in 0..240 {
        let mut inputs = input(0, -127, 0, &[]);
        inputs[1] = input(1, -127, -60, &[])[1];
        inputs[2] = input(2, 127, 0, &[])[2];
        run.step(inputs);
    }
    let game = &run.game;
    let keeper = game.folk[0].body.pos;
    assert!(wide.depth(keeper) > -KEEPER_RADIUS - 0.05, "{keeper:?}");
    for (seat, side) in [(1, wide), (2, small)] {
        let pos = game.folk[seat].body.pos;
        let crease = side.crease().unwrap();
        assert!(
            pos.distance(side.center()) >= crease + FOLK_RADIUS - 1e-3,
            "{seat}"
        );
    }
    // A ball rolls straight through the keeper's crease.
    let start = wide.center() - wide.normal * 3.0 + wide.tangent() * 2.0;
    place(&mut run, 0, 0.0, -5.0);
    place_ball(&mut run, start.x, start.y, wide.normal * 12.0);
    for _ in 0..30 {
        run.step(NONE);
    }
    assert_eq!(run.game.trio_goals, 1);
}

#[test]
fn a_ball_left_alone_hops_off_by_itself() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    clear_pitch(&mut run);
    place_ball(&mut run, 0.0, 0.0, Vec2::ZERO);
    for _ in 0..RESTLESS_TICKS - 1 {
        run.step(NONE);
    }
    assert_eq!(run.game.balls[0].body.vel, Vec2::ZERO);
    run.step(NONE);
    let speed = run.game.balls[0].body.vel.length();
    assert!((speed - RESTLESS_HOP).abs() < 0.2, "{speed}");
}

#[test]
fn goals_widen_after_thirty_seconds_and_scoring_narrows_yours() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    clear_pitch(&mut run);
    let half = |run: &Run<LanternBall>, owner: u8| {
        run.game.sides[run.game.goal_side(owner).unwrap()]
            .goal
            .unwrap()
            .half_width
    };
    run.game.tick = WIDEN_FROM;
    run.step(NONE);
    assert!((half(&run, 0) - DUEL_GOAL_HALF).abs() < 0.01);
    run.game.tick = PLAY_TICKS - 1;
    run.step(NONE);
    assert!((half(&run, 0) - DUEL_GOAL_HALF * (1.0 + WIDEN)).abs() < 0.01);
    run.game.tick = WIDEN_FROM;
    score_on(&mut run, 1, Some(Seat(0)));
    run.step(NONE);
    assert!((half(&run, 0) - (DUEL_GOAL_HALF - SCORE_SHRINK)).abs() < 0.02);
    assert!((half(&run, 1) - DUEL_GOAL_HALF).abs() < 0.02);
    // Never narrower than this.
    run.game.scored[0] = 50;
    run.step(NONE);
    assert!((half(&run, 0) - DUEL_GOAL_HALF * NARROWEST).abs() < 0.01);
}

#[test]
fn the_keepers_goal_never_widens() {
    let mut run = new_run(4, Format::OneVsThree { solo: Seat(0) });
    to_playing(&mut run);
    run.game.tick = SOLO_TICKS - 2;
    run.step(NONE);
    for goal in run.game.sides.iter().filter_map(|s| s.goal) {
        assert_eq!(goal.half_width, goal.base_half_width);
    }
}

#[test]
fn folk_cant_walk_into_a_goal() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    let side = run.game.goal_side(0).unwrap();
    for _ in 0..200 {
        run.step(input(0, -127, 0, &[]));
    }
    let depth = run.game.sides[side].depth(run.game.folk[0].body.pos);
    assert!(depth <= -FOLK_RADIUS + 1e-3, "{depth}");
}

/// Rolls ball 0 into `owner`'s goal and steps until it's in.
fn score_on(run: &mut Run<LanternBall>, owner: u8, by: Option<Seat>) {
    let side = run.game.sides[run.game.goal_side(owner).unwrap()];
    let start = side.center() - side.normal * 2.0;
    place_ball(run, start.x, start.y, side.normal * 12.0);
    run.game.balls[0].touched = by;
    for _ in 0..30 {
        run.step(NONE);
        if !run.game.balls[0].rolling() {
            return;
        }
    }
    panic!("the ball didn't go in");
}

#[test]
fn a_goal_costs_its_owner_a_life_and_the_ball_drops_back_in() {
    let mut run = new_run(3, Format::FreeForAll);
    to_playing(&mut run);
    clear_pitch(&mut run);
    score_on(&mut run, 1, Some(Seat(2)));
    assert_eq!(run.game.lives, [LIVES, LIVES - 1, LIVES, LIVES]);
    assert_eq!(run.game.scored[2], 1);
    assert_eq!(
        run.game.last_goal.map(|g| (g.against, g.by)),
        Some((1, Some(Seat(2))))
    );
    for _ in 0..SCORED_TICKS + DROP_TICKS + 2 {
        run.step(NONE);
    }
    assert!(run.game.balls[0].rolling());
    assert!(run.game.balls[0].body.pos.length() < 0.5);
}

#[test]
fn an_own_goal_costs_a_life_but_scores_for_nobody() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    clear_pitch(&mut run);
    score_on(&mut run, 0, Some(Seat(0)));
    assert_eq!(run.game.lives[0], LIVES - 1);
    assert_eq!(run.game.scored, [0; MAX_PLAYERS]);
}

#[test]
fn the_ball_hits_the_post_rather_than_scoring_beside_it() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    clear_pitch(&mut run);
    let side = run.game.sides[run.game.goal_side(1).unwrap()];
    let post = side.posts().unwrap()[1];
    let start = post - side.normal * 3.0 + side.tangent() * 0.6;
    place_ball(&mut run, start.x, start.y, side.normal * 12.0);
    for _ in 0..40 {
        run.step(NONE);
    }
    assert_eq!(run.game.lives[1], LIVES);
    assert!(run.game.balls[0].rolling());
}

#[test]
fn out_of_lives_walls_your_goal_up_and_the_last_one_left_wins() {
    let mut run = new_run(3, Format::FreeForAll);
    to_playing(&mut run);
    clear_pitch(&mut run);
    run.game.lives = [LIVES, 1, 1, 0];
    score_on(&mut run, 1, None);
    assert!(matches!(run.game.folk[1].status, Status::Out { .. }));
    let side = run.game.goal_side(1).unwrap();
    assert!(!run.game.sides[side].goal.unwrap().open);
    assert!(run.game.outcome.is_none());
    // A walled-up goal is just a wall now.
    let wall = run.game.sides[side];
    let start = wall.center() - wall.normal * 2.0;
    for _ in 0..SCORED_TICKS + DROP_TICKS + 2 {
        run.step(NONE);
    }
    place_ball(&mut run, start.x, start.y, wall.normal * 12.0);
    for _ in 0..30 {
        run.step(NONE);
    }
    assert!(run.game.balls[0].rolling());
    score_on(&mut run, 2, None);
    let outcome = run.game.outcome.unwrap();
    assert_eq!(outcome.placement, [Some(1), Some(3), Some(2), None]);
    while run.outcome().is_none() {
        run.step(NONE);
    }
}

#[test]
fn at_time_up_most_lives_wins_and_a_level_lead_plays_on() {
    let mut run = new_run(3, Format::FreeForAll);
    to_playing(&mut run);
    clear_pitch(&mut run);
    run.game.lives = [3, 3, 2, 0];
    run.game.tick = PLAY_TICKS - 1;
    run.step(NONE);
    assert!(run.game.sudden_death());
    score_on(&mut run, 0, None);
    let outcome = run.game.outcome.unwrap();
    assert_eq!(outcome.placement, [Some(2), Some(1), Some(2), None]);
}

#[test]
fn sudden_death_ends_in_a_shared_win() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    clear_pitch(&mut run);
    run.game.balls[0].status = BallStatus::Waiting;
    run.game.tick = PLAY_TICKS + SUDDEN_DEATH_TICKS - 1;
    run.step(NONE);
    assert_eq!(run.game.outcome.unwrap().placement[..2], [Some(1), Some(1)]);
}

#[test]
fn teams_share_lives_and_lose_together() {
    let mut run = new_run(4, TEAMS);
    to_playing(&mut run);
    clear_pitch(&mut run);
    run.game.lives[1] = 1;
    score_on(&mut run, 1, Some(Seat(0)));
    let outcome = run.game.outcome.unwrap();
    assert_eq!(outcome.placement, [Some(1), Some(2), Some(2), Some(1)]);
    // Nobody is "out" in a team game: the pitch just stops.
    assert!(run.game.folk.iter().all(|f| f.playing()));
}

#[test]
fn the_keeper_is_big_and_guards_the_wide_goal() {
    let run = new_run(4, Format::OneVsThree { solo: Seat(2) });
    let game = &run.game;
    let keeper = game.folk[2];
    assert!(keeper.keeper && keeper.body.radius == KEEPER_RADIUS);
    let wide = &game.sides[game.goal_side(0).unwrap()];
    let small = &game.sides[game.goal_side(1).unwrap()];
    assert_eq!(wide.goal.unwrap().half_width, KEEPER_GOAL_HALF);
    assert_eq!(small.goal.unwrap().half_width, TRIO_GOAL_HALF);
    assert!(keeper.body.pos.distance(wide.center()) < 2.0);
    // The three start nearer the ball than the keeper.
    for seat in [0, 1, 3] {
        assert!(game.folk[seat].body.pos.length() < keeper.body.pos.length());
    }
}

#[test]
fn the_three_need_three_goals_and_the_keepers_take_one_back() {
    let mut run = new_run(4, Format::OneVsThree { solo: Seat(0) });
    to_playing(&mut run);
    clear_pitch(&mut run);
    score_on(&mut run, 0, Some(Seat(1)));
    assert_eq!(run.game.trio_goals, 1);
    run.game.balls[0].status = BallStatus::Rolling;
    score_on(&mut run, 1, Some(Seat(0)));
    assert_eq!(run.game.trio_goals, 0);
    // With none to take back, the keeper's goal banks nothing.
    run.game.balls[0].status = BallStatus::Rolling;
    score_on(&mut run, 1, Some(Seat(0)));
    assert_eq!(run.game.trio_goals, 0);
    for _ in 0..TRIO_GOAL {
        run.game.balls[0].status = BallStatus::Rolling;
        score_on(&mut run, 0, Some(Seat(2)));
    }
    let outcome = run.game.outcome.unwrap();
    assert_eq!(outcome.placement, [Some(2), Some(1), Some(1), Some(1)]);
}

#[test]
fn the_keeper_wins_by_holding_out() {
    let mut run = new_run(4, Format::OneVsThree { solo: Seat(3) });
    to_playing(&mut run);
    run.game.trio_goals = TRIO_GOAL - 1;
    run.game.tick = SOLO_TICKS - 1;
    run.step(NONE);
    let outcome = run.game.outcome.unwrap();
    assert_eq!(outcome.placement, [Some(2), Some(2), Some(2), Some(1)]);
}

#[test]
fn the_second_ball_drops_in_at_thirty_seconds_and_shoves_folk_aside() {
    let mut run = new_run(2, Format::FreeForAll);
    to_playing(&mut run);
    run.game.balls[0].status = BallStatus::Waiting;
    run.game.tick = SECOND_BALL_TICK - 1;
    place(&mut run, 0, 0.3, 0.0);
    run.step(NONE);
    assert!(matches!(
        run.game.balls[1].status,
        BallStatus::Dropping { .. }
    ));
    for _ in 0..DROP_TICKS {
        run.step(NONE);
    }
    assert!(run.game.balls[1].rolling());
    run.step(NONE);
    let folk = run.game.folk[0];
    assert!(folk.body.pos.x > 0.3 + 0.5, "{folk:?}");
}

#[test]
fn benched_duel_seats_take_no_part() {
    let format = Format::Duel {
        a: Seat(1),
        b: Seat(3),
    };
    let run = new_run(4, format);
    assert_eq!(run.game.folk[0].status, Status::Benched);
    assert_eq!(run.game.folk[2].status, Status::Benched);
    assert_eq!(
        run.game.playing().collect::<Vec<_>>(),
        vec![Seat(1), Seat(3)]
    );
}

#[test]
fn passes_every_quality_bar() {
    sim_check::check_all::<LanternBall>(4).unwrap();
}
