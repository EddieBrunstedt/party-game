//! Headless developer tools: run the automated quality bars and bot-only matches from the CLI.

use clap::{Parser, Subcommand, ValueEnum};
use minigame_quick_draw::QuickDraw;
use sim_check::Setup;
use sim_core::{Difficulty, Minigame, Seat};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run every automated quality bar (replay, rollback, bot runs) for a minigame.
    Check {
        minigame: MinigameId,
        #[arg(long, default_value_t = 32)]
        seeds: u64,
    },
    /// Play bot-only matches and print how they went.
    Bots {
        minigame: MinigameId,
        #[arg(long, default_value_t = 100)]
        runs: u64,
        #[arg(long, default_value_t = 4)]
        players: usize,
        #[arg(long, value_enum, default_value_t = BotLevel::Normal)]
        difficulty: BotLevel,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum MinigameId {
    QuickDraw,
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

fn check<M: Minigame>(seeds: u64) -> Result<(), String> {
    sim_check::check_all::<M>(seeds)?;
    println!("{}: all automated checks passed over {seeds} seeds", M::ID);
    Ok(())
}

fn bots<M: Minigame>(runs: u64, players: usize, difficulty: Difficulty) -> Result<(), String> {
    let mut wins = [0u64; 4];
    let mut total_ticks = 0u64;
    for seed in 0..runs {
        let recording = sim_check::check_bots_finish::<M>(&Setup::new(seed, players), difficulty)?;
        total_ticks += recording.inputs.len() as u64;
        let outcome = recording.outcome.expect("checked above");
        for seat in outcome.winners() {
            wins[seat.index()] += 1;
        }
    }
    let avg_secs = total_ticks as f64 / runs as f64 / sim_core::TICK_HZ as f64;
    println!(
        "{}: {runs} runs, {players} {difficulty:?} bots, avg {avg_secs:.1}s",
        M::ID
    );
    for seat in Seat::ALL.iter().take(players) {
        println!("  P{} won {}", seat.0 + 1, wins[seat.index()]);
    }
    Ok(())
}

fn main() {
    let result = match Cli::parse().command {
        Command::Check { minigame, seeds } => match minigame {
            MinigameId::QuickDraw => check::<QuickDraw>(seeds),
        },
        Command::Bots {
            minigame,
            runs,
            players,
            difficulty,
        } => match minigame {
            MinigameId::QuickDraw => bots::<QuickDraw>(runs, players, difficulty.into()),
        },
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
