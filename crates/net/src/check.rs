//! The online quality bar: several machines play one minigame over a real transport, every
//! seat a bot, and must agree on every confirmed tick's checksum and on the outcome.
//!
//! Peers run round-robin on one thread, a tick each per round, against a manual [`Clock`], so
//! a whole match takes a moment instead of its real length. `ggrs`' own timers use the wall
//! clock, which only matters while synchronizing; that phase gets real time.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use sim_core::{Difficulty, Format, MAX_PLAYERS, Minigame, Outcome, PlayerSetup, Run, RunConfig};

use crate::rollback::{Rollback, RollbackEvent, SeatOwner};
use crate::{Clock, Endpoint, EndpointConfig, Faults, Faulty, Transport};

const TICK: Duration = Duration::from_micros(1_000_000 / sim_core::TICK_HZ as u64);

#[derive(Debug, Clone, Copy)]
pub struct OnlineMatch {
    pub seed: u64,
    pub players: PlayerSetup,
    pub format: Format,
    pub difficulty: Difficulty,
    /// What every link does to datagrams, in both directions.
    pub faults: Faults,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OnlineReport {
    pub outcome: Outcome,
    /// The tick the outcome appeared.
    pub ticks: u32,
    /// Confirmed ticks whose checksums were compared across every peer.
    pub compared: u32,
    /// Rollbacks per peer, and the most ticks one of them replayed.
    pub rollbacks: Vec<u32>,
    pub max_resimulated: u32,
}

/// Which machine sends each seat's input: active seats are dealt out round-robin, so with
/// fewer machines than players some have several local players; the host (machine 0) also
/// fills the empty seats.
pub fn deal_seats(players: &PlayerSetup, peers: usize) -> [usize; MAX_PLAYERS] {
    let mut owners = [0; MAX_PLAYERS];
    for (n, seat) in players.seats().enumerate() {
        owners[seat.index()] = n % peers;
    }
    owners
}

/// Plays `setup` across one machine per transport (2–4 of them, at `addrs`) and checks that
/// they agree.
pub fn play_online<M: Minigame, T: Transport>(
    transports: Vec<T>,
    addrs: &[T::Addr],
    setup: OnlineMatch,
) -> Result<OnlineReport, String> {
    let peers = transports.len();
    assert_eq!(peers, addrs.len());
    let clock = Clock::manual();
    let endpoints: Vec<_> = transports
        .into_iter()
        .enumerate()
        .map(|(i, transport)| {
            let seed = setup.seed.wrapping_mul(31).wrapping_add(i as u64);
            let transport = Faulty::new(transport, setup.faults, seed, clock.clone());
            let config = EndpointConfig {
                game_version: "check".into(),
                listen: true,
            };
            Endpoint::new(transport, clock.clone(), config)
        })
        .collect();

    // Everyone connects to everyone: rollback is a full mesh.
    for (i, endpoint) in endpoints.iter().enumerate() {
        for &addr in &addrs[i + 1..] {
            endpoint.connect(addr);
        }
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while endpoints
        .iter()
        .any(|e| e.connected_peers().len() < peers - 1)
    {
        if Instant::now() > deadline {
            return Err("peers never finished the handshake".into());
        }
        endpoints.iter().for_each(|e| drop(e.poll()));
        clock.advance(TICK);
        std::thread::sleep(Duration::from_millis(1));
    }

    let owners = deal_seats(&setup.players, peers);
    let mut sessions = Vec::new();
    for (me, endpoint) in endpoints.iter().enumerate() {
        let seats = owners.map(|owner| match owner {
            _ if owner == me => SeatOwner::Local,
            _ => SeatOwner::Remote(addrs[owner]),
        });
        let run = Run::<M>::new(
            setup.seed,
            setup.players,
            setup.format,
            RunConfig::default(),
        );
        let session = Rollback::start(run, seats, endpoint).map_err(|e| e.to_string())?;
        sessions.push(session);
    }

    let mut checksums: Vec<BTreeMap<u32, u64>> = vec![BTreeMap::new(); peers];
    let mut rollbacks = vec![0; peers];
    let mut max_resimulated = 0;
    let sync_deadline = Instant::now() + Duration::from_secs(10);
    // Generous: waiting rounds (sync, catching up) don't advance the match.
    for _ in 0..M::MAX_TICKS * 4 {
        let synchronizing = !sessions.iter().all(|s| s.is_running());
        if synchronizing && Instant::now() > sync_deadline {
            return Err("ggrs never synchronized".into());
        }
        for (me, session) in sessions.iter_mut().enumerate() {
            endpoints[me].poll();
            let advance =
                session.update(|seat, run| M::bot_input(&run.game, seat, setup.difficulty));
            if advance.resimulated > 0 {
                rollbacks[me] += 1;
                max_resimulated = max_resimulated.max(advance.resimulated);
            }
            for event in session.events() {
                match event {
                    RollbackEvent::Desync { tick, peer } => {
                        return Err(format!(
                            "{}: ggrs saw a desync at tick {tick} with {peer:?}",
                            M::ID
                        ));
                    }
                    RollbackEvent::Disconnected(peer) => {
                        return Err(format!("{}: peer {me} lost {peer:?}", M::ID));
                    }
                    _ => {}
                }
            }
            checksums[me].extend(session.take_confirmed());
        }
        clock.advance(TICK);
        if synchronizing {
            std::thread::sleep(Duration::from_millis(1));
        }
        if sessions.iter().all(|s| s.confirmed_outcome().is_some()) {
            break;
        }
    }

    let outcomes: Vec<_> = sessions.iter().map(|s| s.confirmed_outcome()).collect();
    let Some((ticks, outcome)) = outcomes[0] else {
        return Err(format!(
            "{}: no confirmed outcome within {} rounds",
            M::ID,
            M::MAX_TICKS * 4
        ));
    };
    if let Some(other) = outcomes.iter().position(|o| *o != outcomes[0]) {
        return Err(format!(
            "{}: peers disagree on the outcome: peer 0 {:?}, peer {other} {:?}",
            M::ID,
            outcomes[0],
            outcomes[other]
        ));
    }

    // Every peer must have confirmed every tick up to the outcome, with the same checksums.
    for (me, sums) in checksums.iter().enumerate() {
        if let Some(missing) = (0..=ticks).find(|tick| !sums.contains_key(tick)) {
            return Err(format!(
                "{}: peer {me} never confirmed tick {missing}",
                M::ID
            ));
        }
    }
    for tick in 0..=ticks {
        let first = checksums[0][&tick];
        if let Some(other) = (1..peers).find(|&p| checksums[p][&tick] != first) {
            return Err(format!(
                "{}: desync at tick {tick}: peer 0 {first:016x}, peer {other} {:016x}",
                M::ID,
                checksums[other][&tick]
            ));
        }
    }

    Ok(OnlineReport {
        outcome,
        ticks,
        compared: ticks + 1,
        rollbacks,
        max_resimulated,
    })
}
