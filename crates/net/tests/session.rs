//! Host and clients going through the lobby over a lossy in-memory network.

use std::time::Duration;

use net::lobby::{LobbyAction, LobbyState, PeerId};
use net::session::{Client, ClientEvent, Host, HostEvent, JoinRefusal};
use net::{
    Clock, Endpoint, EndpointConfig, Faults, Faulty, MemoryAddr, MemoryNetwork, MemoryTransport,
};
use sim_core::{Difficulty, Seat};

type Transport = Faulty<MemoryTransport>;

const FAULTS: Faults = Faults {
    loss_percent: 20,
    duplicate_percent: 5,
    delay: Duration::from_millis(20),
    jitter: Duration::from_millis(20),
};

struct World {
    network: MemoryNetwork,
    clock: Clock,
    host: Host<Transport>,
    host_events: Vec<HostEvent>,
}

impl World {
    fn new() -> Self {
        let network = MemoryNetwork::new();
        let clock = Clock::manual();
        let endpoint = endpoint(&network, &clock, 1, "v1", true);
        let host = Host::new(endpoint, LobbyState::new(4, 12, false));
        Self {
            network,
            clock,
            host,
            host_events: Vec::new(),
        }
    }

    fn client(&self, n: u16, version: &str) -> Client<Transport> {
        Client::connect(
            endpoint(&self.network, &self.clock, n, version, false),
            MemoryAddr(1),
        )
    }

    /// Runs everyone until `done` holds for the clients' collected events.
    fn run(
        &mut self,
        clients: &mut [&mut Client<Transport>],
        events: &mut Vec<Vec<ClientEvent<MemoryAddr>>>,
        done: impl Fn(&Self, &[&mut Client<Transport>], &[Vec<ClientEvent<MemoryAddr>>]) -> bool,
    ) {
        events.resize(clients.len(), Vec::new());
        for _ in 0..2000 {
            self.host_events.extend(self.host.poll());
            for (i, client) in clients.iter_mut().enumerate() {
                events[i].extend(client.poll());
            }
            if done(self, clients, events) {
                return;
            }
            self.clock.advance(Duration::from_millis(5));
        }
        panic!(
            "never got there; clients saw {events:?}, the host saw {:?}",
            self.host_events
        );
    }
}

fn endpoint(
    network: &MemoryNetwork,
    clock: &Clock,
    n: u16,
    version: &str,
    listen: bool,
) -> Endpoint<Transport> {
    let transport = Faulty::new(network.bind(MemoryAddr(n)), FAULTS, n as u64, clock.clone());
    let config = EndpointConfig {
        game_version: version.into(),
        listen,
    };
    Endpoint::new(transport, clock.clone(), config)
}

fn same_lobby(world: &World, client: &Client<Transport>) -> bool {
    client.lobby() == Some(world.host.lobby())
}

#[test]
fn clients_join_pick_leave_and_start() {
    let mut world = World::new();
    let mut a = world.client(2, "v1");
    let mut b = world.client(3, "v1");
    let mut events = Vec::new();
    world.run(&mut [&mut a, &mut b], &mut events, |_, _, events| {
        events
            .iter()
            .all(|e| e.iter().any(|e| matches!(e, ClientEvent::Welcomed(_))))
    });
    let (ida, idb) = (a.me().unwrap(), b.me().unwrap());
    assert_ne!(ida, idb);
    assert!(ida != PeerId::HOST && idb != PeerId::HOST);

    // The host joins first, then each client with one pad; client b also adds a second pad.
    world.host.act(LobbyAction::Join { local: 0 });
    a.act(LobbyAction::Join { local: 0 });
    b.act(LobbyAction::Join { local: 0 });
    b.act(LobbyAction::Join { local: 1 });
    b.act(LobbyAction::Pick {
        local: 1,
        next: true,
    });
    // A client can't change the host's settings, and every machine sees the host's.
    a.act(LobbyAction::CycleBots);
    a.act(LobbyAction::TogglePauseOnControllerLost);
    world.host.act(LobbyAction::TogglePauseOnControllerLost);
    world.run(&mut [&mut a, &mut b], &mut events, |world, _, _| {
        world.host.lobby().seats.iter().flatten().count() == 4
    });
    world.run(&mut [&mut a, &mut b], &mut events, |world, clients, _| {
        clients.iter().all(|c| same_lobby(world, c))
    });
    let lobby = world.host.lobby().clone();
    assert_eq!(lobby.seat_of(PeerId::HOST, 0), Some(Seat(0)));
    assert_eq!(lobby.bots, Difficulty::Normal);
    assert!(lobby.run_config().pause_on_controller_lost);
    let mut characters = lobby.roster();
    characters.sort();
    assert_eq!(
        characters,
        [0, 1, 2, 3],
        "characters stay unique across machines"
    );

    // Client b leaves: both its seats free up, and a sees it.
    b.leave();
    world.run(&mut [&mut a], &mut events, |world, clients, _| {
        world.host_events.contains(&HostEvent::Left(idb)) && same_lobby(world, clients[0])
    });
    assert_eq!(world.host.lobby().seats.iter().flatten().count(), 2);
    assert!(!world.host.lobby().peer_joined(idb));

    // Start: a gets the same lobby and seed, and a latecomer is turned away.
    let start = world.host.start(77);
    assert_eq!(start.clients, vec![(ida, MemoryAddr(2))]);
    let mut late = world.client(4, "v1");
    let mut late_events = Vec::new();
    world.run(
        &mut [&mut a, &mut late],
        &mut late_events,
        |_, _, events| {
            events[0]
                .iter()
                .any(|e| matches!(e, ClientEvent::Started(_)))
                && events[1].contains(&ClientEvent::Refused(JoinRefusal::Started))
        },
    );
    let got = late_events[0].iter().find_map(|e| match e {
        ClientEvent::Started(start) => Some(start.clone()),
        _ => None,
    });
    assert_eq!(got, Some(start));
}

#[test]
fn a_fourth_machine_is_turned_away() {
    let mut world = World::new();
    let mut clients: Vec<_> = (2..=5).map(|n| world.client(n, "v1")).collect();
    let mut events = Vec::new();
    let mut refs: Vec<_> = clients.iter_mut().collect();
    world.run(&mut refs, &mut events, |_, _, events| {
        let welcomed = events
            .iter()
            .filter(|e| e.iter().any(|e| matches!(e, ClientEvent::Welcomed(_))));
        let full = events
            .iter()
            .filter(|e| e.contains(&ClientEvent::Refused(JoinRefusal::Full)));
        welcomed.count() == 3 && full.count() == 1
    });
}

#[test]
fn a_different_build_is_told_the_hosts_version() {
    let mut world = World::new();
    let mut old = world.client(2, "v0");
    let mut events = Vec::new();
    world.run(&mut [&mut old], &mut events, |_, _, events| {
        events[0].iter().any(|e| {
            matches!(e, ClientEvent::Refused(JoinRefusal::Version { game, .. }) if game == "v1")
        })
    });
}

#[test]
fn clients_hear_when_the_host_closes() {
    let mut world = World::new();
    let mut a = world.client(2, "v1");
    let mut events = Vec::new();
    world.run(&mut [&mut a], &mut events, |_, _, events| {
        events[0]
            .iter()
            .any(|e| matches!(e, ClientEvent::Welcomed(_)))
    });
    let World { host, clock, .. } = world;
    host.close();
    for _ in 0..200 {
        if a.poll().contains(&ClientEvent::HostLeft) {
            return;
        }
        clock.advance(Duration::from_millis(5));
    }
    panic!("the client never heard the host leave");
}

#[test]
fn clients_only_decide_for_their_own_seats() {
    use sim_board::Intent;

    let mut world = World::new();
    let mut a = world.client(2, "v1");
    let mut events = Vec::new();
    world.run(&mut [&mut a], &mut events, |_, _, events| {
        events[0]
            .iter()
            .any(|e| matches!(e, ClientEvent::Welcomed(_)))
    });
    world.host.act(LobbyAction::Join { local: 0 });
    a.act(LobbyAction::Join { local: 0 });
    world.run(&mut [&mut a], &mut events, |world, _, _| {
        world.host.lobby().seats.iter().flatten().count() == 2
    });
    let theirs = world.host.lobby().seat_of(a.me().unwrap(), 0).unwrap();
    let hosts = world.host.lobby().seat_of(PeerId::HOST, 0).unwrap();

    // Only for the sender's own seat.
    world.host.start(1);
    a.send_intent(hosts, Intent::Roll);
    a.send_intent(theirs, Intent::BuyStar(true));
    world.run(&mut [&mut a], &mut events, |world, _, _| {
        world
            .host_events
            .iter()
            .any(|e| matches!(e, HostEvent::Intent { .. }))
    });
    let intents: Vec<_> = world
        .host_events
        .iter()
        .filter(|e| matches!(e, HostEvent::Intent { .. }))
        .collect();
    assert_eq!(
        intents,
        [&HostEvent::Intent {
            seat: theirs,
            intent: Intent::BuyStar(true)
        }]
    );
}

#[test]
fn after_the_start_clients_mesh_up_and_readiness_reaches_everyone() {
    let mut world = World::new();
    let mut a = world.client(2, "v1");
    let mut b = world.client(3, "v1");
    let mut events = Vec::new();
    world.run(&mut [&mut a, &mut b], &mut events, |_, _, events| {
        events
            .iter()
            .all(|e| e.iter().any(|e| matches!(e, ClientEvent::Welcomed(_))))
    });
    a.act(LobbyAction::Join { local: 0 });
    world.run(&mut [&mut a, &mut b], &mut events, |world, _, _| {
        world.host.lobby().anyone_joined()
    });
    let seat = world.host.lobby().seat_of(a.me().unwrap(), 0).unwrap();
    world.host.start(5);
    world.run(&mut [&mut a, &mut b], &mut events, |_, clients, _| {
        clients
            .iter()
            .all(|c| c.endpoint().connected_peers().len() == 2)
    });
    assert_eq!(a.peers().len(), 2, "the host and the other client");

    a.ready(seat);
    // b can't speak for a's seat; the host ignores it.
    b.ready(seat);
    let mut events = Vec::new();
    world.run(&mut [&mut a, &mut b], &mut events, |world, _, events| {
        events[1].contains(&ClientEvent::Ready(seat))
            && world.host_events.contains(&HostEvent::Ready(seat))
    });
    let count = world
        .host_events
        .iter()
        .filter(|e| **e == HostEvent::Ready(seat))
        .count();
    assert_eq!(count, 1);
}
