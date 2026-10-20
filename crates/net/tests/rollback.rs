//! Every minigame played across several machines over a lossy in-memory network: all of them
//! must agree on every confirmed tick and on the outcome. `tools net-check` runs the same
//! check over localhost UDP for every format and peer count.

use std::time::Duration;

use minigame_firefly_catch::FireflyCatch;
use minigame_lantern_push::LanternPush;
use minigame_quick_draw::QuickDraw;
use net::check::{OnlineMatch, deal_seats, play_online};
use net::{Faults, MemoryAddr, MemoryNetwork};
use sim_core::{Difficulty, Format, Minigame, PlayerSetup, Seat};

const LAGGY: Faults = Faults {
    loss_percent: 5,
    duplicate_percent: 2,
    delay: Duration::from_millis(40),
    jitter: Duration::from_millis(30),
};

fn play<M: Minigame>(players: usize, format: Format, peers: u16) {
    let network = MemoryNetwork::new();
    let addrs: Vec<_> = (1..=peers).map(MemoryAddr).collect();
    let transports = addrs.iter().map(|&addr| network.bind(addr)).collect();
    let setup = OnlineMatch {
        seed: 3,
        players: PlayerSetup::first(players),
        format,
        difficulty: Difficulty::Hard,
        faults: LAGGY,
    };
    let report = play_online::<M, _>(transports, &addrs, setup).unwrap();
    assert!(report.compared > 100);
    assert!(
        report.rollbacks.iter().all(|&n| n > 0),
        "the lag should have caused rollbacks on every machine: {:?}",
        report.rollbacks
    );
}

#[test]
fn quick_draw_agrees_online() {
    play::<QuickDraw>(4, Format::FreeForAll, 4);
    let duel = Format::Duel {
        a: Seat(0),
        b: Seat(1),
    };
    play::<QuickDraw>(2, duel, 2);
}

#[test]
fn lantern_push_agrees_online() {
    let teams = Format::TwoVsTwo {
        team_of: [0, 1, 0, 1],
    };
    play::<LanternPush>(4, teams, 4);
    play::<LanternPush>(4, Format::OneVsThree { solo: Seat(2) }, 2);
}

#[test]
fn firefly_catch_agrees_online() {
    play::<FireflyCatch>(4, Format::FreeForAll, 3);
    play::<FireflyCatch>(2, Format::FreeForAll, 2);
}

#[test]
fn seats_are_dealt_round_robin_and_the_host_fills_empty_ones() {
    assert_eq!(deal_seats(&PlayerSetup::first(4), 2), [0, 1, 0, 1]);
    assert_eq!(deal_seats(&PlayerSetup::first(4), 3), [0, 1, 2, 0]);
    assert_eq!(deal_seats(&PlayerSetup::first(2), 2), [0, 1, 0, 0]);
}
