//! Whole board games across a host and two clients over a lossy in-memory network. Each
//! client's player is a bot deciding on that client's copy of the board and sending its intent,
//! as a human would; the host plays its own player, the bot seat and the minigames (headless,
//! with bots). Every client's copy must match the host's after every update. A saved game
//! picked up online plays on exactly as it would have.

use std::time::Duration;

use net::board::{BoardUpdate, follow};
use net::lobby::{LobbyAction, LobbyState, PeerId};
use net::session::{Client, ClientEvent, Host, HostEvent};
use net::{
    Clock, Endpoint, EndpointConfig, Faults, Faulty, MemoryAddr, MemoryNetwork, MemoryTransport,
};
use sim_board::{Awaiting, Board, Game, Rules, Save, SavedSeat, bot_intent, play_bots};
use sim_core::registry::play_with_bots;
use sim_core::{Difficulty, MAX_PLAYERS, checksum};

type Transport = Faulty<MemoryTransport>;

const FAULTS: Faults = Faults {
    loss_percent: 10,
    duplicate_percent: 5,
    delay: Duration::from_millis(30),
    jitter: Duration::from_millis(30),
};

fn endpoint(network: &MemoryNetwork, clock: &Clock, n: u16, listen: bool) -> Endpoint<Transport> {
    let transport = Faulty::new(
        network.bind(MemoryAddr(n)),
        FAULTS,
        n as u64 * 7,
        clock.clone(),
    );
    let config = EndpointConfig {
        game_version: "test".into(),
        listen,
    };
    Endpoint::new(transport, clock.clone(), config)
}

/// One client: its copy of the board, and the last state it decided on (so it decides once).
struct Follower {
    client: Client<Transport>,
    game: Option<Game>,
    decided_at: Option<u64>,
    updates: usize,
}

/// Plays a game across a host and two clients: a new one, or `resume` picked up again.
/// Returns the finished game's checksum.
fn play(seed: u64, resume: Option<&Save>) -> u64 {
    let network = MemoryNetwork::new();
    let clock = Clock::manual();
    let lobby = match resume {
        Some(save) => LobbyState::resuming(4, save),
        None => LobbyState::new(4, 3, false),
    };
    let mut host = Host::new(endpoint(&network, &clock, 1, true), lobby);
    let mut followers: Vec<_> = (2..=3)
        .map(|n| Follower {
            client: Client::connect(endpoint(&network, &clock, n, false), MemoryAddr(1)),
            game: None,
            decided_at: None,
            updates: 0,
        })
        .collect();
    let tick = |host: &mut Host<Transport>| {
        clock.advance(Duration::from_millis(5));
        host.poll()
    };

    // Lobby: the host and each client take one seat; the fourth is a bot.
    host.act(LobbyAction::Join { local: 0 });
    let mut joined = 0;
    for _ in 0..2000 {
        tick(&mut host);
        for f in &mut followers {
            for event in f.client.poll() {
                if matches!(event, ClientEvent::Welcomed(_)) {
                    f.client.act(LobbyAction::Join { local: 0 });
                    joined += 1;
                }
            }
        }
        if joined == 2 && host.lobby().seats.iter().flatten().count() == 3 {
            break;
        }
    }
    assert_eq!(
        host.lobby().seats.iter().flatten().count(),
        3,
        "everyone sat down"
    );

    let lobby = host.lobby().clone();
    let start = host.start(seed);
    let rules = Rules {
        rounds: lobby.rounds,
        ..Rules::default()
    };
    let mut game = match resume {
        Some(save) => save.game.clone(),
        None => Game::new(
            Board::meadow(),
            rules,
            MAX_PLAYERS,
            start.seed,
            minigame_registry::ALL,
        ),
    };
    host.share_game(&game);
    let first = checksum(&game);

    for _ in 0..200_000 {
        // The host decides for its own seat and the bot, and plays minigames headlessly.
        match game.awaiting() {
            Awaiting::Intent { seat, .. } => {
                let mine = lobby.seats[seat.index()].is_none_or(|c| c.peer == PeerId::HOST);
                if mine {
                    let intent = bot_intent(&game, seat, Difficulty::Normal).unwrap();
                    host.apply(&mut game, seat, intent).unwrap();
                }
            }
            Awaiting::Minigame(plan) => {
                let entry = minigame_registry::find(&plan.id).unwrap();
                let difficulty = [Difficulty::Normal; MAX_PLAYERS];
                let outcome =
                    play_with_bots(entry, plan.seed, plan.players, plan.format, difficulty);
                host.apply_outcome(&mut game, outcome.unwrap()).unwrap();
            }
            Awaiting::GameOver => {}
        }
        for event in tick(&mut host) {
            match event {
                HostEvent::Intent { seat, intent } => {
                    // A late duplicate (sent again before the update arrived) is refused: fine.
                    let _ = host.apply(&mut game, seat, intent);
                }
                HostEvent::NeedSnapshot(peer) => panic!("{peer:?} went out of step"),
                _ => {}
            }
        }

        for f in &mut followers {
            for event in f.client.poll() {
                if let ClientEvent::Board(update) = event {
                    f.updates += 1;
                    match (&update, f.game.as_mut()) {
                        (BoardUpdate::Snapshot(snapshot), None) => {
                            assert_eq!(checksum(&**snapshot), first, "the game as the host has it");
                            f.game = Some((**snapshot).clone());
                        }
                        (BoardUpdate::Snapshot(snapshot), _) => f.game = Some((**snapshot).clone()),
                        (_, Some(copy)) => {
                            if let Err(desync) = follow(copy, &update) {
                                panic!("a client went out of step: {desync}");
                            }
                        }
                        (_, None) => panic!("an update before the snapshot"),
                    }
                }
            }
            // Decide for this client's own seat, once per state of its copy.
            let me = f.client.me().unwrap();
            if let Some(copy) = &f.game
                && let Awaiting::Intent { seat, .. } = copy.awaiting()
                && lobby.seats[seat.index()].is_some_and(|c| c.peer == me)
                && f.decided_at != Some(checksum(copy))
            {
                f.decided_at = Some(checksum(copy));
                f.client
                    .send_intent(seat, bot_intent(copy, seat, Difficulty::Normal).unwrap());
            }
        }

        let host_sum = checksum(&game);
        let caught_up = followers
            .iter()
            .all(|f| f.game.as_ref().map(checksum) == Some(host_sum));
        if matches!(game.awaiting(), Awaiting::GameOver) && caught_up {
            // New: 4 turn-order rolls, 12 turns of one or two decisions, 3 minigames, a
            // snapshot. Resumed in round 2: at least 8 turns and 2 minigames.
            let updates: Vec<_> = followers.iter().map(|f| f.updates).collect();
            assert!(
                updates[0] > 10 && updates[0] == updates[1],
                "updates seen: {updates:?}"
            );
            return host_sum;
        }
    }
    panic!(
        "the game never finished: the host is at {:?}",
        game.awaiting()
    );
}

#[test]
fn a_board_game_plays_to_the_end_across_three_machines() {
    for seed in [1, 2] {
        play(seed, None);
    }
}

/// A 3-round game saved at the start of round 2, as the couch or a host would have saved it.
fn saved_in_round_two(seed: u64) -> Save {
    let rules = Rules {
        rounds: 3,
        ..Rules::default()
    };
    let mut game = Game::new(Board::meadow(), rules, 4, seed, minigame_registry::ALL);
    while game.round() < 2 || !game.between_turns() {
        match game.awaiting() {
            Awaiting::Intent { seat, .. } => {
                let intent = bot_intent(&game, seat, Difficulty::Normal).unwrap();
                game.apply(seat, intent).unwrap();
            }
            Awaiting::Minigame(plan) => {
                let entry = minigame_registry::find(&plan.id).unwrap();
                let difficulty = [Difficulty::Normal; MAX_PLAYERS];
                let outcome =
                    play_with_bots(entry, plan.seed, plan.players, plan.format, difficulty);
                game.apply_minigame_outcome(outcome.unwrap()).unwrap();
            }
            Awaiting::GameOver => unreachable!("saved before the end"),
        }
    }
    let save = Save {
        game,
        seats: [0, 1, 2, 3].map(|character| SavedSeat {
            character,
            human: character < 3,
        }),
        bots: Difficulty::Normal,
        pause_on_controller_lost: false,
    };
    // Through the bytes, as from a file.
    Save::decode(&save.encode()).unwrap()
}

#[test]
fn a_saved_game_picked_up_online_plays_on_as_it_would_have() {
    for seed in [3, 4] {
        let save = saved_in_round_two(seed);
        let offline = play_bots(
            save.game.clone(),
            Difficulty::Normal,
            minigame_registry::ALL,
        );
        assert_eq!(play(seed, Some(&save)), offline.unwrap().checksum);
    }
}
