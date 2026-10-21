//! Endpoints talking over memory and over localhost UDP, with and without injected faults.

use std::net::SocketAddr;
use std::time::Duration;

use ggrs::{GgrsRequest, InputStatus, PlayerType, PredictRepeatLast, SessionBuilder, SessionState};
use net::{
    Clock, Endpoint, EndpointConfig, Faults, Faulty, MemoryAddr, MemoryNetwork, NetEvent, Refusal,
    Transport, UdpTransport,
};

const STEP: Duration = Duration::from_millis(5);

fn config(listen: bool) -> EndpointConfig {
    EndpointConfig {
        game_version: "test-1".into(),
        listen,
    }
}

const ROUGH: Faults = Faults {
    loss_percent: 25,
    duplicate_percent: 10,
    delay: Duration::from_millis(30),
    jitter: Duration::from_millis(40),
};

/// The `i`th test message from `from`: its index, then filler of a size that varies from empty
/// to several fragments.
fn message(from: u8, i: u32) -> Vec<u8> {
    let mut bytes = vec![from];
    bytes.extend(i.to_le_bytes());
    let filler = (i as usize * 997) % 5000;
    bytes.extend((0..filler).map(|n| (n as u32 ^ i) as u8));
    bytes
}

/// Connects `client` to `host`, has each send `count` messages to the other, and checks that
/// everything arrives exactly once and in order. `step` advances time between polls.
fn exchange<T: Transport>(
    host: &Endpoint<T>,
    client: &Endpoint<T>,
    host_addr: T::Addr,
    count: u32,
    step: impl Fn(),
) {
    client.connect(host_addr);
    let mut client_addr = None;
    let mut host_got = Vec::new();
    let mut client_got = Vec::new();
    let mut sent = false;

    for _ in 0..20_000 {
        for event in host.poll() {
            match event {
                NetEvent::Connected(addr) => client_addr = Some(addr),
                NetEvent::Message(_, bytes) => host_got.push(bytes),
                NetEvent::Refused(..) | NetEvent::Disconnected(_) | NetEvent::Lost(_) => {
                    panic!("unexpected {event:?}")
                }
            }
        }
        for event in client.poll() {
            match event {
                NetEvent::Connected(_) => {}
                NetEvent::Message(_, bytes) => client_got.push(bytes),
                NetEvent::Refused(..) | NetEvent::Disconnected(_) | NetEvent::Lost(_) => {
                    panic!("unexpected {event:?}")
                }
            }
        }
        if let Some(client_addr) = client_addr.filter(|_| !sent) {
            for i in 0..count {
                client.send(host_addr, &message(1, i)).unwrap();
                host.send(client_addr, &message(2, i)).unwrap();
            }
            sent = true;
        }
        if host_got.len() as u32 == count && client_got.len() as u32 == count {
            break;
        }
        step();
    }

    // Let stragglers (late duplicates, resends) arrive: none may be delivered again.
    for _ in 0..200 {
        step();
        host_got.extend(host.poll().into_iter().filter_map(message_bytes));
        client_got.extend(client.poll().into_iter().filter_map(message_bytes));
    }

    let expected = |from| (0..count).map(|i| message(from, i)).collect::<Vec<_>>();
    assert_eq!(
        host_got.len(),
        count as usize,
        "host got the wrong number of messages"
    );
    assert_eq!(
        client_got.len(),
        count as usize,
        "client got the wrong number of messages"
    );
    assert!(
        host_got == expected(1),
        "host got messages out of order or corrupted"
    );
    assert!(
        client_got == expected(2),
        "client got messages out of order or corrupted"
    );
}

fn message_bytes<A>(event: NetEvent<A>) -> Option<Vec<u8>> {
    match event {
        NetEvent::Message(_, bytes) => Some(bytes),
        _ => None,
    }
}

fn memory_pair(
    faults: Faults,
    seed: u64,
    clock: &Clock,
) -> [Endpoint<Faulty<net::MemoryTransport>>; 2] {
    let network = MemoryNetwork::new();
    let host = Faulty::new(network.bind(MemoryAddr(1)), faults, seed, clock.clone());
    let client = Faulty::new(network.bind(MemoryAddr(2)), faults, seed + 1, clock.clone());
    [
        Endpoint::new(host, clock.clone(), config(true)),
        Endpoint::new(client, clock.clone(), config(false)),
    ]
}

fn udp_pair(
    faults: Faults,
    seed: u64,
    clock: &Clock,
) -> ([Endpoint<Faulty<UdpTransport>>; 2], SocketAddr) {
    let host = UdpTransport::bind("127.0.0.1:0").unwrap();
    let host_addr = host.local_addr().unwrap();
    let client = UdpTransport::bind("127.0.0.1:0").unwrap();
    let endpoints = [
        Endpoint::new(
            Faulty::new(host, faults, seed, clock.clone()),
            clock.clone(),
            config(true),
        ),
        Endpoint::new(
            Faulty::new(client, faults, seed + 1, clock.clone()),
            clock.clone(),
            config(false),
        ),
    ];
    (endpoints, host_addr)
}

#[test]
fn memory_delivers_everything_in_order() {
    let clock = Clock::manual();
    let [host, client] = memory_pair(Faults::default(), 1, &clock);
    exchange(&host, &client, MemoryAddr(1), 100, || clock.advance(STEP));
}

#[test]
fn memory_survives_loss_duplication_delay_and_reordering() {
    for seed in [1, 2, 3, 4] {
        let clock = Clock::manual();
        let [host, client] = memory_pair(ROUGH, seed * 10, &clock);
        exchange(&host, &client, MemoryAddr(1), 200, || clock.advance(STEP));
    }
}

#[test]
fn memory_survives_heavy_loss() {
    let clock = Clock::manual();
    let faults = Faults {
        loss_percent: 60,
        ..ROUGH
    };
    let [host, client] = memory_pair(faults, 99, &clock);
    exchange(&host, &client, MemoryAddr(1), 40, || clock.advance(STEP));
}

#[test]
fn udp_delivers_everything_in_order() {
    let clock = Clock::manual();
    let ([host, client], host_addr) = udp_pair(Faults::default(), 1, &clock);
    exchange(&host, &client, host_addr, 100, || clock.advance(STEP));
}

#[test]
fn udp_survives_loss_duplication_delay_and_reordering() {
    let clock = Clock::manual();
    let ([host, client], host_addr) = udp_pair(ROUGH, 7, &clock);
    exchange(&host, &client, host_addr, 200, || {
        clock.advance(STEP);
        // Loopback is fast but not instant; give the kernel a moment now and then.
        std::thread::sleep(Duration::from_micros(50));
    });
}

/// Polls `endpoints` until one returns an event `want` accepts.
fn wait_for<T: Transport>(
    endpoints: &[&Endpoint<T>],
    clock: &Clock,
    mut want: impl FnMut(usize, &NetEvent<T::Addr>) -> bool,
) -> bool {
    for _ in 0..1000 {
        for (i, endpoint) in endpoints.iter().enumerate() {
            if endpoint.poll().iter().any(|event| want(i, event)) {
                return true;
            }
        }
        clock.advance(STEP);
    }
    false
}

#[test]
fn a_different_build_is_refused() {
    let clock = Clock::manual();
    let network = MemoryNetwork::new();
    let host = Endpoint::new(network.bind(MemoryAddr(1)), clock.clone(), config(true));
    let other = EndpointConfig {
        game_version: "test-2".into(),
        listen: false,
    };
    let client = Endpoint::new(network.bind(MemoryAddr(2)), clock.clone(), other);
    client.connect(MemoryAddr(1));

    let mut host_connected = false;
    let refused = wait_for(&[&host, &client], &clock, |i, event| {
        host_connected |= i == 0 && matches!(event, NetEvent::Connected(_));
        i == 1
            && matches!(event, NetEvent::Refused(_, Refusal::Version { game, .. }) if game == "test-1")
    });
    assert!(refused, "the client never heard the host's versions");
    assert!(!host_connected);
    assert!(client.send(MemoryAddr(1), b"hi").is_err());
}

#[test]
fn a_peer_that_isnt_hosting_refuses() {
    let clock = Clock::manual();
    let network = MemoryNetwork::new();
    let a = Endpoint::new(network.bind(MemoryAddr(1)), clock.clone(), config(false));
    let b = Endpoint::new(network.bind(MemoryAddr(2)), clock.clone(), config(false));
    b.connect(MemoryAddr(1));
    let refused = wait_for(&[&a, &b], &clock, |i, event| {
        i == 1 && *event == NetEvent::Refused(MemoryAddr(1), Refusal::NotListening)
    });
    assert!(refused);
}

#[test]
fn peers_connecting_to_each_other_at_once_both_connect() {
    let clock = Clock::manual();
    let network = MemoryNetwork::new();
    let peers: Vec<_> = (1..=3)
        .map(|n| Endpoint::new(network.bind(MemoryAddr(n)), clock.clone(), config(false)))
        .collect();
    for (i, peer) in peers.iter().enumerate() {
        for n in 1..=3 {
            if n as usize != i + 1 {
                peer.connect(MemoryAddr(n));
            }
        }
    }
    for _ in 0..10 {
        for peer in &peers {
            peer.poll();
        }
        clock.advance(STEP);
    }
    for (i, peer) in peers.iter().enumerate() {
        assert_eq!(
            peer.connected_peers().len(),
            2,
            "peer {} isn't connected to both others",
            i + 1
        );
    }
}

#[test]
fn messages_sent_before_the_handshake_finishes_still_arrive() {
    let clock = Clock::manual();
    let [host, client] = memory_pair(ROUGH, 5, &clock);
    client.connect(MemoryAddr(1));
    client.send(MemoryAddr(1), b"early").unwrap();
    let arrived = wait_for(&[&host, &client], &clock, |i, event| {
        i == 0 && matches!(event, NetEvent::Message(_, bytes) if bytes == b"early")
    });
    assert!(arrived);
}

struct GgrsConfig;

impl ggrs::Config for GgrsConfig {
    type Input = u8;
    type InputPredictor = PredictRepeatLast;
    /// The frame number, so each advance knows which frame its inputs belong to.
    type State = u32;
    type Address = MemoryAddr;
}

fn input(seat: usize, frame: u32) -> u8 {
    (frame as usize * 7 + seat * 100) as u8
}

/// Two real `ggrs` sessions sync through the adapter and see each other's inputs on the right
/// frames, over a lossy, delayed link.
#[test]
fn ggrs_sessions_sync_and_exchange_inputs() {
    const DELAY: u32 = 2;
    let clock = Clock::real();
    let faults = Faults {
        loss_percent: 10,
        duplicate_percent: 5,
        delay: Duration::from_millis(10),
        jitter: Duration::from_millis(10),
    };
    let network = MemoryNetwork::new();
    let addrs = [MemoryAddr(1), MemoryAddr(2)];
    let endpoints: Vec<_> = addrs
        .iter()
        .enumerate()
        .map(|(i, &addr)| {
            let transport = Faulty::new(network.bind(addr), faults, i as u64, clock.clone());
            Endpoint::new(transport, clock.clone(), config(true))
        })
        .collect();
    endpoints[0].connect(addrs[1]);
    while !endpoints.iter().all(|e| e.connected_peers().len() == 1) {
        endpoints.iter().for_each(|e| drop(e.poll()));
        std::thread::sleep(Duration::from_millis(1));
    }

    let mut sessions: Vec<_> = (0..2)
        .map(|me| {
            let mut builder = SessionBuilder::<GgrsConfig>::new()
                .with_num_players(2)
                .unwrap()
                .with_input_delay(DELAY as usize);
            for (seat, &addr) in addrs.iter().enumerate() {
                let player = if seat == me {
                    PlayerType::Local
                } else {
                    PlayerType::Remote(addr)
                };
                builder = builder.add_player(player, seat).unwrap();
            }
            builder
                .start_p2p_session(endpoints[me].ggrs_socket())
                .unwrap()
        })
        .collect();

    let mut states = [0u32; 2];
    let mut confirmed = [0u32; 2];
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while confirmed.iter().any(|&n| n < 120) {
        assert!(
            std::time::Instant::now() < deadline,
            "ggrs stalled: {confirmed:?} confirmed"
        );
        for me in 0..2 {
            let session = &mut sessions[me];
            session.poll_remote_clients();
            endpoints[me].poll();
            if session.current_state() != SessionState::Running {
                continue;
            }
            let frame = session.current_frame() as u32;
            session.add_local_input(me, input(me, frame)).unwrap();
            let Ok(requests) = session.advance_frame() else {
                continue; // too far ahead of the other peer; it catches up
            };
            for request in requests {
                match request {
                    GgrsRequest::SaveGameState { cell, frame } => {
                        cell.save(frame, Some(states[me]), None)
                    }
                    GgrsRequest::LoadGameState { cell, .. } => states[me] = cell.load().unwrap(),
                    GgrsRequest::AdvanceFrame { inputs } => {
                        let at = states[me];
                        let other = 1 - me;
                        let (value, status) = inputs[other];
                        if status == InputStatus::Confirmed && at >= DELAY {
                            assert_eq!(value, input(other, at - DELAY), "peer {me}, frame {at}");
                            confirmed[me] = confirmed[me].max(at);
                        }
                        states[me] += 1;
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn a_goodbye_disconnects_the_other_side() {
    let clock = Clock::manual();
    let [host, client] = memory_pair(Faults::default(), 1, &clock);
    client.connect(MemoryAddr(1));
    assert!(wait_for(&[&host, &client], &clock, |i, event| {
        i == 0 && matches!(event, NetEvent::Connected(_))
    }));
    client.disconnect(MemoryAddr(1));
    assert!(wait_for(&[&host, &client], &clock, |i, event| {
        i == 0 && *event == NetEvent::Disconnected(MemoryAddr(2))
    }));
    assert!(host.connected_peers().is_empty());
    assert!(client.send(MemoryAddr(1), b"gone").is_err());
}

#[test]
fn quiet_peers_stay_connected_and_a_silent_one_is_lost() {
    let clock = Clock::manual();
    let [host, client] = memory_pair(ROUGH, 3, &clock);
    client.connect(MemoryAddr(1));
    assert!(wait_for(&[&host, &client], &clock, |i, event| {
        i == 0 && matches!(event, NetEvent::Connected(_))
    }));

    // Ten seconds with nothing to say: keepalives hold the connection, even over a rough link.
    for _ in 0..2000 {
        for endpoint in [&host, &client] {
            for event in endpoint.poll() {
                assert!(
                    matches!(event, NetEvent::Connected(_)),
                    "unexpected {event:?}"
                );
            }
        }
        clock.advance(STEP);
    }

    // The client stops answering (frozen, or its network went): the host gives up on it.
    let mut waited = Duration::ZERO;
    while !host.poll().contains(&NetEvent::Lost(MemoryAddr(2))) {
        clock.advance(STEP);
        waited += STEP;
        assert!(waited < Duration::from_secs(6), "never lost");
    }
    assert!(waited >= net::PEER_TIMEOUT - Duration::from_millis(500));
    assert!(host.connected_peers().is_empty());

    // When the client speaks again, the host tells it that it was forgotten.
    assert!(wait_for(&[&host, &client], &clock, |i, event| {
        i == 1 && *event == NetEvent::Disconnected(MemoryAddr(1))
    }));
    assert!(client.connected_peers().is_empty());
}
