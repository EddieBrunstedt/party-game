use sim_core::{Run, RunConfig};

use super::*;

const NONE: [Input; MAX_PLAYERS] = [Input {
    buttons: 0,
    stick_x: 0,
    stick_y: 0,
}; MAX_PLAYERS];

fn new_run(players: usize, format: Format) -> Run<LanternMasquerade> {
    Run::new(7, PlayerSetup::first(players), format, RunConfig::default())
}

/// A run with the shuffle over, so ringing and striking are open.
fn acting(players: usize, format: Format) -> Run<LanternMasquerade> {
    let mut run = new_run(players, format);
    while !run.game.can_act() {
        run.step(NONE);
    }
    run
}

fn press(seat: Seat, button: Button) -> [Input; MAX_PLAYERS] {
    let mut inputs = NONE;
    inputs[seat.index()].set(button, true);
    inputs
}

/// Hold A long enough to ring a bell.
fn ring(run: &mut Run<LanternMasquerade>, seat: Seat) {
    for _ in 0..RING_TICKS {
        run.step(press(seat, Button::A));
    }
}

/// Stand `seat`'s folk at `pos`, still.
fn place(run: &mut Run<LanternMasquerade>, seat: Seat, pos: Vec2) {
    let f = run.game.folk_of(seat);
    run.game.folk[f].pos = pos;
    run.game.folk[f].moving = false;
}

/// Stand every stroller well away from `pos`, so only the folk we place are near it.
fn clear_around(run: &mut Run<LanternMasquerade>, pos: Vec2) {
    for f in 0..FOLK {
        if run.game.owner[f].is_none() && run.game.folk[f].pos.distance(pos) < 3.0 {
            let away = if pos.x > 0.0 { -7.0 } else { 7.0 };
            run.game.folk[f].pos = Vec2::new(away, run.game.folk[f].pos.y);
        }
    }
}

#[test]
fn passes_check_all() {
    sim_check::check_all::<LanternMasquerade>(4).unwrap();
}

#[test]
fn players_are_dealt_different_folk() {
    let run = new_run(4, Format::FreeForAll);
    let mut dealt: Vec<usize> = Seat::ALL.iter().map(|s| run.game.folk_of(*s)).collect();
    dealt.sort();
    dealt.dedup();
    assert_eq!(dealt.len(), 4);
    for seat in Seat::ALL {
        assert_eq!(run.game.owner[run.game.folk_of(seat)], Some(seat));
        assert_eq!(run.game.revealed(run.game.folk_of(seat)), None);
    }
}

#[test]
fn strollers_seldom_walk_oddly() {
    let mut run = new_run(4, Format::FreeForAll);
    for _ in 0..4000 {
        run.step(NONE);
    }
    assert!(matches!(run.game.phase, Phase::Play { .. }));
    let (mut odd, mut segments) = (0, 0);
    for f in 0..FOLK {
        assert!(inside(run.game.folk[f].pos, 0.0));
        odd += run.game.folk[f].clue.odd as u32;
        segments += run.game.folk[f].segments as u32;
    }
    // Only fidgets look odd (a fidget walk and the stop after it, or a fidget stop).
    assert!(
        odd * 100 <= segments * NPC_FIDGET_PERCENT * 2,
        "{odd} of {segments}"
    );
    assert!(odd > 0);
}

#[test]
fn a_short_walk_stands_out() {
    let mut run = acting(2, Format::FreeForAll);
    let f = run.game.folk_of(Seat(0));
    let mut right = NONE;
    right[0].stick_x = 127;
    for _ in 0..10 {
        run.step(right);
    }
    assert!(run.game.folk[f].moving);
    assert_eq!(run.game.folk[f].dir, 0);
    run.step(NONE);
    assert!(!run.game.folk[f].moving);
    assert_eq!(run.game.folk[f].clue.odd, 1);
}

#[test]
fn a_wobbly_stick_keeps_its_direction() {
    let mut run = acting(2, Format::FreeForAll);
    let f = run.game.folk_of(Seat(0));
    let mut stick = NONE;
    stick[0].stick_x = 127;
    run.step(stick);
    // About 15 degrees off: within the hysteresis, though nearer the next direction.
    stick[0].stick_y = -34;
    run.step(stick);
    assert_eq!(run.game.folk[f].dir, 0);
    // Straight down the screen turns.
    stick[0] = Input {
        buttons: 0,
        stick_x: 0,
        stick_y: -127,
    };
    run.step(stick);
    assert_eq!(run.game.folk[f].dir, DIRECTIONS / 4);
}

#[test]
fn ringing_a_bell_scores_it_and_marks_who_was_there() {
    let mut run = acting(4, Format::FreeForAll);
    let bell = BELL_POSITIONS[2];
    clear_around(&mut run, bell);
    place(&mut run, Seat(0), bell + Vec2::new(0.5, 0.0));
    place(&mut run, Seat(1), bell + Vec2::new(-1.3, 0.0));
    place(&mut run, Seat(2), bell + Vec2::new(0.0, 2.5));
    ring(&mut run, Seat(0));
    let game = &run.game;
    assert_eq!(game.seat(Seat(0)).bells, 1 << 2);
    assert_eq!(game.bells[2].rung_by, 1);
    assert_eq!(game.folk[game.folk_of(Seat(0))].clue.rang[0], 1);
    assert_eq!(game.folk[game.folk_of(Seat(1))].clue.rang[0], 1);
    assert_eq!(game.folk[game.folk_of(Seat(2))].clue.rang[0], 0);
    // Pressing again doesn't ring it again.
    run.step(NONE);
    ring(&mut run, Seat(0));
    assert_eq!(run.game.seat(Seat(0)).total_bells, 1);
}

#[test]
fn nothing_rings_during_the_shuffle() {
    let mut run = new_run(2, Format::FreeForAll);
    while !matches!(run.game.phase, Phase::Play { .. }) {
        run.step(NONE);
    }
    place(&mut run, Seat(0), BELL_POSITIONS[0]);
    ring(&mut run, Seat(0));
    assert_eq!(run.game.seat(Seat(0)).bells, 0);
}

#[test]
fn partners_share_their_bells() {
    let format = Format::TwoVsTwo {
        team_of: [0, 0, 1, 1],
    };
    let mut run = acting(4, format);
    place(&mut run, Seat(0), BELL_POSITIONS[1]);
    ring(&mut run, Seat(0));
    place(&mut run, Seat(1), BELL_POSITIONS[1]);
    ring(&mut run, Seat(1));
    assert_eq!(
        run.game.seat(Seat(1)).bells,
        0,
        "the partner already rang it"
    );
    assert_eq!(run.game.team_bells(0), 1 << 1);
}

#[test]
fn a_struck_player_is_out_and_shown() {
    let mut run = acting(3, Format::FreeForAll);
    let spot = Vec2::new(2.0, 1.0);
    clear_around(&mut run, spot);
    place(&mut run, Seat(0), spot);
    place(&mut run, Seat(1), spot + Vec2::new(0.6, 0.0));
    run.step(press(Seat(0), Button::B));
    let game = &run.game;
    let victim = game.folk_of(Seat(1));
    assert!(game.seat(Seat(1)).out);
    assert_eq!(game.revealed(victim), Some(Seat(1)));
    assert!(game.folk[game.folk_of(Seat(0))].clue.struck);
    assert_eq!(game.revealed(game.folk_of(Seat(0))), None);
    assert!(matches!(game.phase, Phase::Play { .. }), "two still stand");
}

#[test]
fn a_struck_stroller_falls_and_gets_up() {
    let mut run = acting(2, Format::FreeForAll);
    let spot = Vec2::new(-2.0, 1.0);
    clear_around(&mut run, spot);
    place(&mut run, Seat(0), spot);
    place(&mut run, Seat(1), Vec2::new(6.0, 4.0));
    let stroller = (0..FOLK).find(|f| run.game.owner[*f].is_none()).unwrap();
    run.game.folk[stroller].pos = spot + Vec2::new(0.0, 0.7);
    run.game.folk[stroller].moving = false;
    run.step(press(Seat(0), Button::B));
    assert!(run.game.folk[stroller].cleared);
    assert_eq!(run.game.folk[stroller].down, FALL_TICKS);
    // The cooldown holds the next strike back.
    run.step(NONE);
    run.step(press(Seat(0), Button::B));
    assert_eq!(run.game.seat(Seat(0)).cooldown, STRIKE_COOLDOWN - 2);
    for _ in 0..FALL_TICKS {
        run.step(NONE);
    }
    assert!(run.game.folk[stroller].standing());
}

#[test]
fn the_last_one_standing_takes_the_round() {
    let format = Format::Duel {
        a: Seat(0),
        b: Seat(1),
    };
    let mut run = acting(2, format);
    let spot = Vec2::new(0.0, 3.0);
    clear_around(&mut run, spot);
    place(&mut run, Seat(0), spot);
    place(&mut run, Seat(1), spot + Vec2::new(0.5, 0.0));
    run.step(press(Seat(1), Button::B));
    assert!(matches!(run.game.phase, Phase::Result { .. }));
    assert_eq!(run.game.round_winners, 1 << 1);
    assert_eq!(run.game.ended_by, Some(Ending::LastStanding));
    assert_eq!(run.game.team_rounds[1], 1);
}

#[test]
fn striking_each_other_at_once_shares_the_round() {
    let mut run = acting(2, Format::FreeForAll);
    let spot = Vec2::new(0.0, 3.0);
    clear_around(&mut run, spot);
    place(&mut run, Seat(0), spot);
    place(&mut run, Seat(1), spot + Vec2::new(0.5, 0.0));
    let mut both = press(Seat(0), Button::B);
    both[1].set(Button::B, true);
    run.step(both);
    assert_eq!(run.game.round_winners, 0b11);
}

#[test]
fn every_bell_takes_the_round_and_two_rounds_the_match() {
    let mut run = acting(4, Format::FreeForAll);
    for round in 0..ROUNDS_TO_WIN {
        run.game.seats[2].bells = ALL_BELLS & !1;
        place(&mut run, Seat(2), BELL_POSITIONS[0]);
        ring(&mut run, Seat(2));
        assert!(matches!(run.game.phase, Phase::Result { .. }));
        assert_eq!(run.game.round_winners, 1 << 2);
        assert_eq!(run.game.ended_by, Some(Ending::Bells));
        assert_eq!(run.game.team_rounds[2], round + 1);
        while !run.game.can_act() && run.outcome().is_none() {
            run.step(NONE);
        }
    }
    let outcome = run.outcome().expect("two rounds win the match");
    assert_eq!(outcome.placement[2], Some(1));
    assert!(outcome.placement[0].unwrap() > 1);
}

#[test]
fn time_up_goes_to_the_most_bells() {
    let mut run = acting(3, Format::FreeForAll);
    run.game.seats[1].bells = 0b11;
    run.game.seats[2].bells = 0b100;
    while matches!(run.game.phase, Phase::Play { .. }) {
        run.step(NONE);
    }
    assert_eq!(run.game.round_winners, 1 << 1);
    assert_eq!(run.game.ended_by, Some(Ending::TimeUp));
}

fn one_vs_three() -> Run<LanternMasquerade> {
    acting(4, Format::OneVsThree { solo: Seat(0) })
}

#[test]
fn the_seeker_catching_a_stroller_goes_dark() {
    let mut run = one_vs_three();
    let stroller = (0..FOLK).find(|f| run.game.owner[*f].is_none()).unwrap();
    let spot = Vec2::new(3.0, -1.0);
    clear_around(&mut run, spot);
    run.game.folk[stroller].pos = spot;
    run.game.folk[stroller].moving = false;
    run.game.sight.pos = spot;
    run.step(press(Seat(0), Button::A));
    assert!(run.game.folk[stroller].cleared);
    assert!(run.game.folk[stroller].standing());
    assert_eq!(run.game.sight.dark, DARK_TICKS);
    // While dark, nothing is caught.
    let hider = run.game.folk_of(Seat(1));
    run.game.folk[hider].pos = spot;
    run.step(NONE);
    run.step(press(Seat(0), Button::A));
    assert!(!run.game.seat(Seat(1)).out);
}

#[test]
fn the_seeker_takes_the_round_by_catching_all_three() {
    let mut run = one_vs_three();
    let spot = Vec2::new(-3.0, 2.0);
    clear_around(&mut run, spot);
    for seat in [Seat(1), Seat(2), Seat(3)] {
        place(&mut run, seat, spot);
        run.game.sight.pos = spot;
        run.step(press(Seat(0), Button::A));
        assert!(run.game.seat(seat).out);
        run.step(NONE);
    }
    assert!(matches!(run.game.phase, Phase::Result { .. }));
    assert_eq!(run.game.round_winners, 1);
    assert_eq!(run.game.ended_by, Some(Ending::Caught));
}

#[test]
fn the_three_take_the_round_with_every_bell() {
    let mut run = one_vs_three();
    run.game.seats[1].bells = 0b00011;
    run.game.seats[3].bells = 0b01100;
    place(&mut run, Seat(2), BELL_POSITIONS[4]);
    ring(&mut run, Seat(2));
    assert!(matches!(run.game.phase, Phase::Result { .. }));
    assert_eq!(run.game.round_winners, 0b1110);
}

#[test]
fn the_three_cannot_strike() {
    let mut run = one_vs_three();
    let spot = Vec2::new(0.0, -3.5);
    place(&mut run, Seat(1), spot);
    place(&mut run, Seat(2), spot + Vec2::new(0.4, 0.0));
    run.step(press(Seat(1), Button::B));
    assert!(!run.game.seat(Seat(2)).out);
}

#[test]
fn letting_go_of_the_rope_starts_the_pull_again() {
    let mut run = acting(2, Format::FreeForAll);
    place(&mut run, Seat(0), BELL_POSITIONS[3]);
    for _ in 0..RING_TICKS - 1 {
        run.step(press(Seat(0), Button::A));
    }
    assert_eq!(run.game.bells[3].pull, RING_TICKS - 1);
    run.step(NONE);
    assert_eq!(run.game.bells[3].pull, 0);
    ring(&mut run, Seat(0));
    assert_eq!(run.game.seat(Seat(0)).bells, 1 << 3);
}
