//! Pause and controller loss are inputs, so they reach every machine on the same tick. Three
//! machines play with scripted input; one client presses Start twice and another client's
//! pad goes missing for a while. Every confirmed tick on every machine must match an offline
//! run fed the same inputs, which halts exactly where the script says.

use std::time::Duration;

use minigame_lantern_push::LanternPush;
use net::check::{OnlineMatch, deal_seats, play_online_with};
use net::rollback::INPUT_DELAY;
use net::{Faults, MemoryAddr, MemoryNetwork};
use sim_core::{Button, Difficulty, Format, Input, PlayerSetup, Run, RunConfig, Seat, checksum};

const LAGGY: Faults = Faults {
    loss_percent: 5,
    duplicate_percent: 2,
    delay: Duration::from_millis(40),
    jitter: Duration::from_millis(30),
};

/// Seat 1 (on machine 1) presses Start on these ticks, for a few ticks each.
const PAUSE_AT: u32 = 230;
const RESUME_AT: u32 = 300;
/// Seat 2's pad (on machine 2) is unplugged for these ticks.
const LOST: std::ops::Range<u32> = 340..420;

fn script(seat: Seat, tick: u32) -> Input {
    let s = seat.index() as u32;
    let mut input = Input::default();
    if seat == Seat(2) && LOST.contains(&tick) {
        // What the game sends for a seat whose pad is gone: nothing but the flag.
        input.set(Button::CONTROLLER_LOST, true);
        return input;
    }
    // Small wiggles, so prediction keeps missing and machines keep rolling back, but nobody
    // walks off the platform.
    input.stick_x = if (tick / 20 + s).is_multiple_of(2) {
        40
    } else {
        -40
    };
    input.stick_y = if (tick / 30 + s).is_multiple_of(2) {
        30
    } else {
        -30
    };
    let pressing = |at: u32| (at..at + 3).contains(&tick);
    if seat == Seat(1) && (pressing(PAUSE_AT) || pressing(RESUME_AT)) {
        input.set(Button::START, true);
    }
    input
}

/// Whether the run should be halted after the tick that took in `tick`'s input.
fn expect_halted(tick: u32, config: RunConfig) -> bool {
    let sent = |at: u32| at + INPUT_DELAY as u32;
    let paused = (sent(PAUSE_AT)..sent(RESUME_AT)).contains(&tick);
    let lost = (sent(LOST.start)..sent(LOST.end)).contains(&tick);
    paused || (lost && config.pause_on_controller_lost)
}

fn play(config: RunConfig) {
    let network = MemoryNetwork::new();
    let addrs: Vec<_> = (1..=3).map(MemoryAddr).collect();
    let transports = addrs.iter().map(|&addr| network.bind(addr)).collect();
    let setup = OnlineMatch {
        seed: 5,
        players: PlayerSetup::first(4),
        format: Format::FreeForAll,
        difficulty: Difficulty::Normal,
        config,
        faults: LAGGY,
    };
    // The pressing seats belong to clients, not the host.
    assert_eq!(deal_seats(&setup.players, 3), [0, 1, 2, 0]);
    let report = play_online_with::<LanternPush, _>(transports, &addrs, setup, |seat, tick, _| {
        script(seat, tick)
    })
    .unwrap();
    assert!(
        report.ticks > LOST.end + 10,
        "the match ended at tick {} before the script did",
        report.ticks
    );
    assert!(
        report.rollbacks.iter().all(|r| *r > 0),
        "{:?}",
        report.rollbacks
    );

    // Offline, the same inputs arrive INPUT_DELAY ticks after they were sent.
    let mut run = Run::<LanternPush>::new(setup.seed, setup.players, setup.format, config);
    let mut halted_ticks = 0;
    for tick in 0..=report.ticks {
        let inputs = Seat::ALL.map(|seat| match tick.checked_sub(INPUT_DELAY as u32) {
            Some(sent) => script(seat, sent),
            None => Input::default(),
        });
        run.step(inputs);
        assert_eq!(
            checksum(&run),
            report.checksums[tick as usize],
            "online and offline differ at tick {tick}"
        );
        assert_eq!(run.halted(), expect_halted(tick, config), "tick {tick}");
        // The 🎮✕ badge reads this, on every machine.
        let lost = (LOST.start + INPUT_DELAY as u32..LOST.end + INPUT_DELAY as u32).contains(&tick);
        assert_eq!(run.controller_lost(Seat(2)), lost, "tick {tick}");
        assert_eq!(
            run.waiting_for_controller(),
            lost && config.pause_on_controller_lost
        );
        halted_ticks += run.halted() as u32;
    }
    let lost_ticks = LOST.end - LOST.start;
    let expected = RESUME_AT - PAUSE_AT + lost_ticks * config.pause_on_controller_lost as u32;
    assert_eq!(halted_ticks, expected);
    assert_eq!(run.tick, report.ticks + 1 - halted_ticks);
}

#[test]
fn a_clients_pause_halts_every_machine_on_the_same_tick() {
    play(RunConfig::default());
}

#[test]
fn a_lost_pad_halts_every_machine_when_the_host_says_so() {
    play(RunConfig {
        pause_on_controller_lost: true,
    });
}
