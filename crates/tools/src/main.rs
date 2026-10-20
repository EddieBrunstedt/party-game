//! Headless developer tools: run the automated quality bars (offline and online), bot-only
//! minigame matches and whole bot-only board games from the CLI.

use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use minigame_firefly_catch::FireflyCatch;
use minigame_lantern_push::LanternPush;
use minigame_quick_draw::QuickDraw;
use net::check::{OnlineMatch, OnlineReport};
use net::{Faults, UdpTransport};
use sim_board::{Board, Event, Game, Rules, Summary};
use sim_core::{
    Difficulty, Format, MAX_PLAYERS, Minigame, MinigameEntry, PlayerSetup, RunConfig, Seat,
};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run every automated quality bar (replay, rollback, bot runs) for a minigame.
    Check {
        /// Minigame id, e.g. quick-draw.
        minigame: String,
        #[arg(long, default_value_t = 32)]
        seeds: u64,
    },
    /// Play bot-only matches across 2-4 peers over localhost UDP with simulated latency and
    /// loss, and check every peer agrees on every confirmed tick and on the outcome.
    NetCheck {
        /// Minigame id, e.g. quick-draw.
        minigame: String,
        #[arg(long, default_value_t = 2)]
        seeds: u64,
        /// One-way delay added to every datagram.
        #[arg(long, default_value_t = 40)]
        delay_ms: u64,
        /// Extra random delay per datagram, up to this much (it also reorders them).
        #[arg(long, default_value_t = 30)]
        jitter_ms: u64,
        #[arg(long, default_value_t = 5)]
        loss_percent: u32,
        #[arg(long, default_value_t = 2)]
        duplicate_percent: u32,
    },
    /// Play bot-only matches and print how they went.
    Bots {
        /// Minigame id, e.g. quick-draw.
        minigame: String,
        #[arg(long, default_value_t = 100)]
        runs: u64,
        #[arg(long, default_value_t = 4)]
        players: usize,
        #[arg(long, value_enum, default_value_t = BotLevel::Normal)]
        difficulty: BotLevel,
    },
    /// Play a whole bot-only board game, minigames included, and print how it went.
    Board {
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[arg(long, default_value_t = 4)]
        players: usize,
        #[arg(long, value_enum, default_value_t = BotLevel::Normal)]
        difficulty: BotLevel,
        /// Defaults to the standard ruleset's length.
        #[arg(long)]
        rounds: Option<u32>,
        /// Also print every round's minigame, format, winners and star purchases.
        #[arg(long)]
        verbose: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum BotLevel {
    Easy,
    Normal,
    Hard,
}

impl From<BotLevel> for Difficulty {
    fn from(level: BotLevel) -> Self {
        match level {
            BotLevel::Easy => Difficulty::Easy,
            BotLevel::Normal => Difficulty::Normal,
            BotLevel::Hard => Difficulty::Hard,
        }
    }
}

fn find(id: &str) -> Result<&'static MinigameEntry, String> {
    minigame_registry::find(id).ok_or_else(|| {
        let known: Vec<&str> = minigame_registry::ALL.iter().map(|e| e.id).collect();
        format!("unknown minigame {id:?} (known: {})", known.join(", "))
    })
}

/// `check_all` needs the concrete minigame type, so every registered minigame is listed here
/// too. A test keeps this in step with the registry.
fn checker(id: &str) -> Option<fn(u64) -> Result<(), String>> {
    match id {
        QuickDraw::ID => Some(sim_check::check_all::<QuickDraw>),
        LanternPush::ID => Some(sim_check::check_all::<LanternPush>),
        FireflyCatch::ID => Some(sim_check::check_all::<FireflyCatch>),
        _ => None,
    }
}

/// Plays one online match across this many peers.
type NetChecker = fn(OnlineMatch, usize) -> Result<OnlineReport, String>;

/// Like [`checker`], for the online check.
fn net_checker(id: &str) -> Option<NetChecker> {
    match id {
        QuickDraw::ID => Some(play_over_udp::<QuickDraw>),
        LanternPush::ID => Some(play_over_udp::<LanternPush>),
        FireflyCatch::ID => Some(play_over_udp::<FireflyCatch>),
        _ => None,
    }
}

/// One machine per peer, each on its own localhost UDP port.
fn play_over_udp<M: Minigame>(setup: OnlineMatch, peers: usize) -> Result<OnlineReport, String> {
    let transports: Vec<_> = (0..peers)
        .map(|_| UdpTransport::bind("127.0.0.1:0"))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("could not bind a UDP socket: {e}"))?;
    let addrs: Vec<_> = transports.iter().map(|t| t.local_addr().unwrap()).collect();
    net::check::play_online::<M, _>(transports, &addrs, setup)
}

fn net_check(id: &str, seeds: u64, faults: Faults) -> Result<(), String> {
    let entry = find(id)?;
    let play = net_checker(entry.id).ok_or_else(|| format!("{id} has no net checker in tools"))?;
    let mut matches = 0;
    for seed in 0..seeds {
        for setup in checker_setups(entry.id, seed) {
            for peers in 2..=setup.players.count() {
                let online = OnlineMatch {
                    seed,
                    players: setup.players,
                    format: setup.format,
                    difficulty: DIFFICULTIES[seed as usize % DIFFICULTIES.len()],
                    faults,
                };
                let report = play(online, peers)?;
                let rollbacks: u32 = report.rollbacks.iter().sum();
                println!(
                    "  seed {seed}, {} players, {}, {peers} peers: {} ticks agreed, {rollbacks} rollbacks (longest {}), won by {}",
                    setup.players.count(),
                    describe(setup.format, setup.players),
                    report.compared,
                    report.max_resimulated,
                    seat_list(report.outcome.winners(), ", ")
                );
                matches += 1;
            }
        }
    }
    println!(
        "{}: {matches} online matches, every peer agreed on every tick",
        entry.id
    );
    Ok(())
}

const DIFFICULTIES: [Difficulty; 3] = [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard];

/// The seeds, player counts and formats `check_all` covers, for a minigame by id.
fn checker_setups(id: &str, seed: u64) -> Vec<sim_check::Setup> {
    match id {
        QuickDraw::ID => sim_check::setups::<QuickDraw>(seed),
        LanternPush::ID => sim_check::setups::<LanternPush>(seed),
        FireflyCatch::ID => sim_check::setups::<FireflyCatch>(seed),
        _ => Vec::new(),
    }
}

fn check(id: &str, seeds: u64) -> Result<(), String> {
    let entry = find(id)?;
    let check_all = checker(entry.id).ok_or_else(|| format!("{id} has no checker in tools"))?;
    check_all(seeds)?;
    println!(
        "{}: all automated checks passed over {seeds} seeds",
        entry.id
    );
    Ok(())
}

fn bots(id: &str, runs: u64, players: usize, difficulty: Difficulty) -> Result<(), String> {
    let entry = find(id)?;
    let mut wins = [0u64; MAX_PLAYERS];
    let mut total_ticks = 0u64;
    for seed in 0..runs {
        let setup = PlayerSetup::first(players);
        let mut run = (entry.start)(seed, setup, Format::FreeForAll, RunConfig::default());
        let mut ticks = 0;
        let outcome = loop {
            if ticks >= entry.max_ticks {
                return Err(format!(
                    "{}: {difficulty:?} bots did not finish within {} ticks (seed {seed})",
                    entry.id, entry.max_ticks
                ));
            }
            run.step(Seat::ALL.map(|seat| run.bot_input(seat, difficulty)));
            ticks += 1;
            if let Some(outcome) = run.outcome() {
                break outcome;
            }
        };
        total_ticks += ticks as u64;
        for seat in outcome.winners() {
            wins[seat.index()] += 1;
        }
    }
    let avg_secs = total_ticks as f64 / runs as f64 / sim_core::TICK_HZ as f64;
    println!(
        "{}: {runs} runs, {players} {difficulty:?} bots, avg {avg_secs:.1}s",
        entry.id
    );
    for seat in Seat::ALL.iter().take(players) {
        println!("  P{} won {}", seat.0 + 1, wins[seat.index()]);
    }
    Ok(())
}

fn seat_name(seat: Seat) -> String {
    format!("P{}", seat.0 + 1)
}

fn seat_list(seats: impl Iterator<Item = Seat>, separator: &str) -> String {
    seats.map(seat_name).collect::<Vec<_>>().join(separator)
}

fn describe(format: Format, players: PlayerSetup) -> String {
    match format {
        Format::FreeForAll => "free-for-all".into(),
        Format::OneVsThree { solo } => format!("1v3, {} solo", seat_name(solo)),
        Format::TwoVsTwo { .. } => {
            let team = |t| {
                seat_list(
                    players.seats().filter(move |s| format.team(*s) == Some(t)),
                    "+",
                )
            };
            format!("2v2, {} vs {}", team(0), team(1))
        }
        Format::Duel { a, b } => format!("duel, {} vs {}", seat_name(a), seat_name(b)),
    }
}

fn board(
    seed: u64,
    players: usize,
    difficulty: Difficulty,
    rounds: Option<u32>,
    verbose: bool,
) -> Result<(), String> {
    if !(2..=MAX_PLAYERS).contains(&players) {
        return Err(format!("a board game needs 2-{MAX_PLAYERS} players"));
    }
    let mut rules = Rules::default();
    match rounds {
        Some(0) => return Err("a board game needs at least one round".into()),
        Some(rounds) => rules.rounds = rounds,
        None => {}
    }
    let all = minigame_registry::ALL;
    let game = Game::new(Board::meadow(), rules, players, seed, all);
    let board_name = game.board().name().to_string();
    let summary = sim_board::play_bots(game, difficulty, all)?;

    println!(
        "{board_name}: {players} {difficulty:?} bots, {} rounds, seed {seed}",
        rules.rounds
    );
    if verbose {
        print_rounds(&summary);
    }
    let game = &summary.game;
    let mut standings: Vec<Seat> = game.players().seats().collect();
    standings.sort_by_key(|s| summary.placements.placement[s.index()]);
    println!("Final standings:");
    for seat in standings {
        let player = game.player(seat);
        println!(
            "  {}. {}  {} stars  {} coins",
            summary.placements.placement[seat.index()].unwrap_or(0),
            seat_name(seat),
            player.stars,
            player.coins
        );
    }
    println!(
        "Checksum {:016x} ({} events)",
        summary.checksum,
        summary.events.len()
    );
    Ok(())
}

/// One line per round: who bought stars, then the minigame, its format and its winners.
fn print_rounds(summary: &Summary) {
    let players = summary.game.players();
    let mut stars_bought = Vec::new();
    let mut minigames = summary.minigames.iter();
    for event in &summary.events {
        match event {
            Event::StarBought { seat, .. } => stars_bought.push(*seat),
            Event::RoundEnded { round } => {
                let played = minigames.next().expect("a minigame every round");
                let stars = if stars_bought.is_empty() {
                    String::new()
                } else {
                    format!("  (stars: {})", seat_list(stars_bought.drain(..), ", "))
                };
                println!(
                    "  round {round:>2}: {} ({}) won by {}{stars}",
                    played.plan.id,
                    describe(played.plan.format, players),
                    seat_list(played.outcome.winners(), ", ")
                );
            }
            _ => {}
        }
    }
}

fn main() {
    let result = match Cli::parse().command {
        Command::Check { minigame, seeds } => check(&minigame, seeds),
        Command::NetCheck {
            minigame,
            seeds,
            delay_ms,
            jitter_ms,
            loss_percent,
            duplicate_percent,
        } => net_check(
            &minigame,
            seeds,
            Faults {
                loss_percent,
                duplicate_percent,
                delay: Duration::from_millis(delay_ms),
                jitter: Duration::from_millis(jitter_ms),
            },
        ),
        Command::Bots {
            minigame,
            runs,
            players,
            difficulty,
        } => bots(&minigame, runs, players, difficulty.into()),
        Command::Board {
            seed,
            players,
            difficulty,
            rounds,
            verbose,
        } => board(seed, players, difficulty.into(), rounds, verbose),
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_minigame_can_be_checked() {
        for entry in minigame_registry::ALL {
            assert!(checker(entry.id).is_some(), "add {} to `checker`", entry.id);
            assert!(
                net_checker(entry.id).is_some(),
                "add {} to `net_checker`",
                entry.id
            );
            assert!(
                !checker_setups(entry.id, 0).is_empty(),
                "add {} to `checker_setups`",
                entry.id
            );
        }
    }
}
