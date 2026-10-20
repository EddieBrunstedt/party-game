mod dev;
mod lantern_push;
mod palette;
mod perf;
mod quick_draw;
mod runner;
mod scenery;

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::WindowResolution;
use clap::{Parser, ValueEnum};
use minigame_lantern_push::LanternPush;
use minigame_quick_draw::QuickDraw;
use sim_core::{Difficulty, Format, PlayerSetup, RunConfig, Seat, TICK_HZ};

use runner::{Controller, MinigamePlugin, RunSetup, Seats};

/// The display title. This is the only place the game's name appears in code.
pub const GAME_TITLE: &str = "Party Game";

/// Free-play launcher: jump straight into one minigame with humans and bots.
#[derive(Parser, Clone)]
#[command(name = "game")]
struct Cli {
    #[arg(long, value_enum, default_value_t = MinigameId::QuickDraw)]
    minigame: MinigameId,
    /// Total seats in play (2-4). Seats beyond `--humans` are bots.
    #[arg(long, default_value_t = 4)]
    players: usize,
    /// Human seats. Seat 1 uses the keyboard and the first gamepad; seat N uses gamepad N.
    #[arg(long, default_value_t = 1)]
    humans: usize,
    #[arg(long, value_enum, default_value_t = BotLevel::Normal)]
    bots: BotLevel,
    /// Team layout. In 1v3, P1 is the solo; in 2v2, P1 and P3 play P2 and P4.
    #[arg(long, value_enum, default_value_t = FormatArg::Ffa)]
    format: FormatArg,
    #[arg(long)]
    seed: Option<u64>,
    /// Halt the minigame while any controller is unplugged (default: that seat just goes idle).
    #[arg(long)]
    pause_on_controller_lost: bool,
    /// Take a screenshot this many seconds after start (repeatable), then exit.
    #[arg(long = "screenshot-at", value_name = "SECONDS")]
    screenshot_at: Vec<f32>,
    #[arg(long, default_value = "screenshots")]
    screenshot_dir: PathBuf,
    /// Log every gamepad button press (diagnoses controller mappings).
    #[arg(long)]
    log_input: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum MinigameId {
    QuickDraw,
    LanternPush,
}

#[derive(Clone, Copy, ValueEnum)]
enum FormatArg {
    Ffa,
    #[value(name = "1v3")]
    OneVsThree,
    #[value(name = "2v2")]
    TwoVsTwo,
    Duel,
}

impl FormatArg {
    fn format(self) -> Format {
        match self {
            FormatArg::Ffa => Format::FreeForAll,
            FormatArg::OneVsThree => Format::OneVsThree { solo: Seat(0) },
            FormatArg::TwoVsTwo => Format::TwoVsTwo {
                team_of: [0, 1, 0, 1],
            },
            FormatArg::Duel => Format::Duel {
                a: Seat(0),
                b: Seat(1),
            },
        }
    }
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

fn main() {
    let cli = Cli::parse();
    let players = cli.players.clamp(2, 4);
    let humans = cli.humans.min(players);
    let seed = cli.seed.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    });

    let mut seats = [None; 4];
    for (i, seat) in seats.iter_mut().enumerate().take(players) {
        *seat = Some(if i < humans {
            Controller::Human {
                gamepad: i,
                keyboard: i == 0,
            }
        } else {
            Controller::Bot(cli.bots.into())
        });
    }
    let setup = RunSetup {
        seed,
        players: PlayerSetup::first(players),
        format: cli.format.format(),
        config: RunConfig {
            pause_on_controller_lost: cli.pause_on_controller_lost,
        },
    };

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: GAME_TITLE.into(),
            resolution: WindowResolution::new(1280, 800),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(Time::<Fixed>::from_hz(TICK_HZ as f64))
    .insert_resource(Seats(seats))
    .add_plugins((perf::PerfOverlayPlugin, scenery::SceneryPlugin));

    match cli.minigame {
        MinigameId::QuickDraw => {
            app.add_plugins((
                MinigamePlugin::<QuickDraw>::new(setup),
                quick_draw::QuickDrawViewPlugin,
            ));
        }
        MinigameId::LanternPush => {
            app.add_plugins((
                MinigamePlugin::<LanternPush>::new(setup),
                lantern_push::LanternPushViewPlugin,
            ));
        }
    }

    if cli.log_input {
        app.add_plugins(dev::InputLogPlugin);
    }
    if !cli.screenshot_at.is_empty() {
        app.add_plugins(dev::ScreenshotPlugin {
            at_seconds: cli.screenshot_at,
            dir: cli.screenshot_dir,
        });
    }

    app.run();
}
