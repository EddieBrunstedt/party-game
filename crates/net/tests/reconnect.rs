//! Machines dropping out of a started session and coming back, over a lossy in-memory network:
//! the board waits for them and resumes from a snapshot, minigames end (or stop and restart)
//! on the host's word, and a host that vanishes is noticed.

use std::time::Duration;

use net::board::{BoardUpdate, follow};
use net::lobby::{LobbyAction, LobbyState};
use net::session::{Client, ClientEvent, Host, HostEvent, JoinRefusal, Launch};
use net::{
    Clock, Endpoint, EndpointConfig, Faults, Faulty, MemoryAddr, MemoryNetwork, MemoryTransport,
};
use sim_board::{Awaiting, Board, Game, Rules, bot_intent};
use sim_core::registry::play_with_bots;
use sim_core::{Difficulty, Format, MAX_PLAYERS, PlayerSetup, checksum};

type Transport = Faulty<MemoryTransport>;

const FAULTS: Faults = Faults {
    loss_percent: 10,
    duplicate_percent: 5,
    delay: Duration::from_millis(30),
    jitter: Duration::from_millis(30),
};
const STEP: Duration = Duration::from_millis(5);

struct World {
    network: MemoryNetwork,
    clock: Clock,
    host: Host<Transport>,
    host_events: Vec<HostEvent>,
}

/// A client machine and everything it has heard.
struct Machine {
    client: Client<Transport>,
    events: Vec<ClientEvent<MemoryAddr>>,
}

impl Machine {
    fn heard(&self, want: impl Fn(&ClientEvent<MemoryAddr>) -> bool) -> bool {
        self.events.iter().any(want)
    }
}

impl World {
    fn new() -> Self {
        let network = MemoryNetwork::new();
        let clock = Clock::manual();
        let host = Host::new(
            endpoint(&network, &clock, 1, true),
            LobbyState::new(4, 3, false),
        );
        Self {
            network,
            clock,
            host,
            host_events: Vec::new(),
        }
    }

    fn connect(&self, n: u16) -> Machine {
        let endpoint = endpoint(&self.network, &self.clock, n, false);
        Machine {
            client: Client::connect(endpoint, MemoryAddr(1)),
            events: Vec::new(),
        }
    }

    /// One step of everyone: the host, then each machine.
    fn step(&mut self, machines: &mut [&mut Machine]) {
        self.clock.advance(STEP);
        self.host_events.extend(self.host.poll());
        for machine in machines.iter_mut() {
            let events = machine.client.poll();
            machine.events.extend(events);
        }
    }

    /// Steps until `done` holds, for at most `limit` of simulated time.
    fn run_until(
        &mut self,
        machines: &mut [&mut Machine],
        limit: Duration,
        done: impl Fn(&Self, &[&mut Machine]) -> bool,
    ) {
        let mut waited = Duration::ZERO;
        while !done(self, machines) {
            assert!(
                waited < limit,
                "never got there; the host saw {:?}",
                self.host_events
            );
            self.step(machines);
            waited += STEP;
        }
    }

    fn host_heard(&self, event: &HostEvent) -> bool {
        self.host_events.contains(event)
    }

    /// Everyone welcomed; the host takes seat 0, `a` one seat and `b` two (so `b`'s machine
    /// has two controllers to bring back). Then the host starts.
    fn seat_and_start(&mut self, a: &mut Machine, b: &mut Machine) {
        self.run_until(&mut [a, b], Duration::from_secs(5), |_, m| {
            m.iter().all(|m| m.client.me().is_some())
        });
        self.host.act(LobbyAction::Join { local: 0 });
        a.client.act(LobbyAction::Join { local: 0 });
        b.client.act(LobbyAction::Join { local: 0 });
        b.client.act(LobbyAction::Join { local: 1 });
        self.run_until(&mut [a, b], Duration::from_secs(5), |world, _| {
            world.host.lobby().seats.iter().flatten().count() == MAX_PLAYERS
        });
        self.host.start(9);
        self.run_until(&mut [a, b], Duration::from_secs(5), |_, m| {
            m.iter()
                .all(|m| m.heard(|e| matches!(e, ClientEvent::Started(_))))
        });
    }
}

fn endpoint(network: &MemoryNetwork, clock: &Clock, n: u16, listen: bool) -> Endpoint<Transport> {
    let transport = Faulty::new(
        network.bind(MemoryAddr(n)),
        FAULTS,
        n as u64 * 13,
        clock.clone(),
    );
    let config = EndpointConfig {
        game_version: "test".into(),
        listen,
    };
    Endpoint::new(transport, clock.clone(), config)
}

/// A client's copy of the board, kept up to date from its events.
#[derive(Default)]
struct Copy {
    game: Option<Game>,
    seen: usize,
    decided_at: Option<u64>,
}

impl Copy {
    fn follow(&mut self, machine: &Machine) {
        for event in &machine.events[self.seen..] {
            if let ClientEvent::Board(update) = event {
                match (update, self.game.as_mut()) {
                    (BoardUpdate::Snapshot(game), _) => self.game = Some((**game).clone()),
                    (update, Some(game)) => {
                        if let Err(desync) = follow(game, update) {
                            panic!("a client went out of step: {desync}");
                        }
                    }
                    (_, None) => panic!("a board update before the snapshot"),
                }
            }
        }
        self.seen = machine.events.len();
    }

    /// Decides for this machine's own seats, once per state of its copy.
    fn decide(&mut self, machine: &Machine) {
        let me = machine.client.me().unwrap();
        let lobby = machine.client.lobby().unwrap();
        if let Some(game) = &self.game
            && let Awaiting::Intent { seat, .. } = game.awaiting()
            && lobby.seats[seat.index()].is_some_and(|c| c.peer == me)
            && self.decided_at != Some(checksum(game))
        {
            self.decided_at = Some(checksum(game));
            let intent = bot_intent(game, seat, Difficulty::Normal).unwrap();
            machine.client.send_intent(seat, intent);
        }
    }
}

#[test]
fn a_machine_that_drops_out_mid_board_takes_its_seats_back_and_the_game_finishes() {
    let mut world = World::new();
    let mut a = world.connect(2);
    let mut b = Some(world.connect(3));
    world.seat_and_start(&mut a, b.as_mut().unwrap());
    let idb = b.as_ref().unwrap().client.me().unwrap();

    let lobby = world.host.lobby().clone();
    let rules = Rules {
        rounds: lobby.rounds,
        ..Rules::default()
    };
    let mut game = Game::new(
        Board::meadow(),
        rules,
        MAX_PLAYERS,
        9,
        minigame_registry::ALL,
    );
    world.host.share_game(&game);

    let mut copies = [Copy::default(), Copy::default()];
    // The intents clients sent while someone was away, applied once everyone is back.
    let mut held = Vec::new();
    let mut dropped_at_round = None;
    for _ in 0..400_000 {
        let waiting = !world.host.away().is_empty();
        if !waiting {
            for (seat, intent) in held.drain(..) {
                world.host.apply(&mut game, seat, intent).unwrap();
            }
            match game.awaiting() {
                Awaiting::Intent { seat, .. } if lobby.seats[seat.index()].unwrap().peer.0 == 0 => {
                    let intent = bot_intent(&game, seat, Difficulty::Normal).unwrap();
                    world.host.apply(&mut game, seat, intent).unwrap();
                }
                Awaiting::Minigame(plan) => {
                    let entry = minigame_registry::find(&plan.id).unwrap();
                    let bots = [Difficulty::Normal; MAX_PLAYERS];
                    let outcome = play_with_bots(entry, plan.seed, plan.players, plan.format, bots);
                    world
                        .host
                        .apply_outcome(&mut game, outcome.unwrap())
                        .unwrap();
                }
                _ => {}
            }
        }

        // In round 2, b's machine is switched off without a word.
        if game.round() == 2 && dropped_at_round.is_none() {
            drop(b.take());
            copies[1] = Copy::default();
            dropped_at_round = Some(game.round());
        }

        let before = world.host_events.len();
        let mut machines: Vec<&mut Machine> = std::iter::once(&mut a).chain(b.as_mut()).collect();
        world.step(&mut machines);
        let new = world.host_events.split_off(before);
        world.host_events.extend(new.iter().cloned());
        for event in new {
            match event {
                HostEvent::Intent { seat, intent } if waiting => held.push((seat, intent)),
                HostEvent::Intent { seat, intent } => {
                    let _ = world.host.apply(&mut game, seat, intent);
                }
                HostEvent::NeedSnapshot(peer) => panic!("{peer:?} went out of step"),
                HostEvent::Back(peer) => world.host.send_snapshot(peer, &game),
                // Once the host has noticed, b's machine starts again at a new address.
                HostEvent::Away(away) if away == [idb] => {
                    assert!(
                        a.heard(|e| *e == ClientEvent::Away(vec![idb]))
                            || world.host.away() == [idb]
                    );
                    b = Some(world.connect(4));
                }
                _ => {}
            }
        }

        if let Some(rejoined) = b.as_mut() {
            let welcomed = rejoined
                .events
                .iter()
                .any(|e| matches!(e, ClientEvent::Welcomed(_)));
            if welcomed && rejoined.client.rejoining() && rejoined.client.lobby().is_some() {
                // Its controllers take the seats again: both are b's, numbered 0 and 1.
                assert_eq!(rejoined.client.me(), Some(idb));
                let lobby = rejoined.client.lobby().expect("checked above");
                assert_eq!(lobby.seat_of(idb, 0), Some(sim_core::Seat(2)));
                assert_eq!(lobby.seat_of(idb, 1), Some(sim_core::Seat(3)));
                rejoined.client.back();
                rejoined.events.clear();
            }
        }
        let machines: Vec<&Machine> = std::iter::once(&a).chain(b.as_ref()).collect();
        for (copy, machine) in copies.iter_mut().zip(&machines) {
            if machine.client.rejoining() || machine.client.me().is_none() {
                continue;
            }
            copy.follow(machine);
            copy.decide(machine);
        }

        let host_sum = checksum(&game);
        let caught_up = copies
            .iter()
            .all(|c| c.game.as_ref().map(checksum) == Some(host_sum));
        if matches!(game.awaiting(), Awaiting::GameOver) && caught_up && b.is_some() {
            assert!(world.host_heard(&HostEvent::Back(idb)));
            assert!(a.heard(|e| *e == ClientEvent::Away(vec![idb])));
            assert!(a.heard(|e| *e == ClientEvent::Away(vec![])));
            assert!(
                a.client.endpoint().is_connected(MemoryAddr(4)),
                "a meshed up with b's new address"
            );
            assert!(!a.client.endpoint().is_connected(MemoryAddr(3)));
            return;
        }
    }
    panic!(
        "the game never finished: the host is at {:?}",
        game.awaiting()
    );
}

fn launch() -> Launch {
    Launch {
        id: "quick-draw".into(),
        seed: 4,
        players: PlayerSetup::first(MAX_PLAYERS),
        format: Format::FreeForAll,
    }
}

#[test]
fn a_minigame_settles_once_every_machine_has_its_end() {
    let mut world = World::new();
    let (mut a, mut b) = (world.connect(2), world.connect(3));
    world.seat_and_start(&mut a, &mut b);
    let settled = |m: &Machine| m.heard(|e| *e == ClientEvent::MinigameSettled);

    // The host and a are done; b is still playing, so nobody may stop yet.
    world.host.finish_minigame();
    a.client.finish_minigame();
    for _ in 0..400 {
        world.step(&mut [&mut a, &mut b]);
    }
    assert!(!world.host_heard(&HostEvent::MinigameSettled));
    assert!(!settled(&a) && !settled(&b));
    b.client.finish_minigame();
    world.run_until(&mut [&mut a, &mut b], Duration::from_secs(2), |world, m| {
        world.host_heard(&HostEvent::MinigameSettled) && m.iter().all(|m| settled(m))
    });

    // The next minigame starts from nothing: a's end alone settles nothing.
    world.host_events.clear();
    a.client.finish_minigame();
    for _ in 0..400 {
        world.step(&mut [&mut a, &mut b]);
    }
    world.host.finish_minigame();
    for _ in 0..400 {
        world.step(&mut [&mut a, &mut b]);
    }
    assert!(!world.host_heard(&HostEvent::MinigameSettled));
}

#[test]
fn a_minigame_stops_when_a_machine_drops_out_and_restarts_when_it_is_back() {
    let mut world = World::new();
    let (mut a, mut b) = (world.connect(2), world.connect(3));
    world.seat_and_start(&mut a, &mut b);
    let idb = b.client.me().unwrap();

    // b's machine dies mid-minigame, and the host stops it. a reaches the end just then: its
    // report crosses the stop, so it belongs to the abandoned attempt and mustn't count later.
    drop(b);
    world.run_until(&mut [&mut a], Duration::from_secs(7), |world, _| {
        world.host_heard(&HostEvent::Away(vec![idb]))
    });
    world.host.stop_minigame();
    a.client.finish_minigame();
    world.run_until(&mut [&mut a], Duration::from_secs(2), |_, m| {
        m[0].heard(|e| *e == ClientEvent::MinigameStopped)
    });

    // b's machine starts again and sits back down; then everyone plays it again.
    let mut b = world.connect(4);
    world.run_until(&mut [&mut a, &mut b], Duration::from_secs(2), |_, m| {
        m[1].client.rejoining()
    });
    b.client.back();
    world.run_until(&mut [&mut a, &mut b], Duration::from_secs(2), |world, _| {
        world.host_heard(&HostEvent::Back(idb)) && world.host.away().is_empty()
    });
    world.host.restart_minigame(launch());
    world.run_until(&mut [&mut a, &mut b], Duration::from_secs(2), |_, m| {
        m.iter()
            .all(|m| m.heard(|e| *e == ClientEvent::MinigameRestart(launch())))
    });
    assert!(b.heard(|e| matches!(e, ClientEvent::Started(_))));

    // The restarted attempt settles only on fresh ends from everyone.
    world.host.finish_minigame();
    for _ in 0..400 {
        world.step(&mut [&mut a, &mut b]);
    }
    assert!(
        !world.host_heard(&HostEvent::MinigameSettled),
        "a's old end counted"
    );
    a.client.finish_minigame();
    b.client.finish_minigame();
    world.run_until(&mut [&mut a, &mut b], Duration::from_secs(2), |world, _| {
        world.host_heard(&HostEvent::MinigameSettled)
    });
}

#[test]
fn a_restarted_machine_takes_over_a_quiet_one_without_waiting_out_the_timeout() {
    let mut world = World::new();
    let (mut a, mut b) = (world.connect(2), world.connect(3));
    world.seat_and_start(&mut a, &mut b);
    let idb = b.client.me().unwrap();

    // b goes quiet; 2.5 s later (before the endpoint would give up on it) it's back elsewhere.
    for _ in 0..500 {
        world.step(&mut [&mut a]);
    }
    assert!(world.host.away().is_empty());
    let mut b2 = world.connect(4);
    world.run_until(&mut [&mut a, &mut b2], Duration::from_secs(1), |_, m| {
        m[1].client.rejoining()
    });
    assert_eq!(b2.client.me(), Some(idb));
    assert_eq!(world.host.away(), [idb]);
    drop(b);
}

#[test]
fn the_host_can_hand_an_absent_machines_seats_to_bots() {
    let mut world = World::new();
    let (mut a, mut b) = (world.connect(2), world.connect(3));
    world.seat_and_start(&mut a, &mut b);
    let idb = b.client.me().unwrap();
    drop(b);
    world.run_until(&mut [&mut a], Duration::from_secs(7), |world, _| {
        world.host.away() == [idb]
    });

    world.host.hand_to_bots(idb);
    world.run_until(&mut [&mut a], Duration::from_secs(2), |world, m| {
        m[0].heard(|e| *e == ClientEvent::Away(vec![]))
            && m[0].client.lobby() == Some(world.host.lobby())
    });
    assert!(!world.host.lobby().peer_joined(idb), "bots play its seats");

    // Too late to come back now.
    let mut late = world.connect(4);
    world.run_until(&mut [&mut a, &mut late], Duration::from_secs(2), |_, m| {
        m[1].heard(|e| *e == ClientEvent::Refused(JoinRefusal::Started))
    });
}

#[test]
fn clients_notice_a_host_that_vanishes() {
    let mut world = World::new();
    let (mut a, mut b) = (world.connect(2), world.connect(3));
    world.seat_and_start(&mut a, &mut b);
    let World { host, clock, .. } = world;
    drop(host);
    let mut waited = Duration::ZERO;
    while !a.heard(|e| *e == ClientEvent::HostLost) {
        clock.advance(STEP);
        waited += STEP;
        assert!(waited < Duration::from_secs(7), "a never noticed");
        a.events.extend(a.client.poll());
        b.client.poll();
    }
}
