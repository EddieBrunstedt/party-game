mod audio;
mod board_view;
mod characters;
mod dev;
mod firefly_catch;
mod flow;
mod i18n;
mod intro;
mod lantern_push;
mod menus;
mod minigames;
mod palette;
mod particles;
mod perf;
mod quality;
mod quick_draw;
mod runner;
mod scenery;
mod sfx;

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::WindowResolution;
use clap::{Parser, ValueEnum};
use sim_core::{Difficulty, Format, PlayerSetup, RunConfig, Seat, TICK_HZ};

use flow::{AppState, MinigameLaunch, Origin};
use runner::{Controller, Pad, RunSetup, Seats};
use sim_board::{Board, Game, Rules};

/// The display title. This is the only place the game's name appears in code.
pub const GAME_TITLE: &str = "Party Game";

/// Free-play launcher: jump straight into one minigame with humans and bots.
#[derive(Parser, Clone)]
#[command(name = "game")]
struct Cli {
    /// Free play: skip the menus and jump straight into this minigame (e.g. quick-draw).
    #[arg(long)]
    minigame: Option<String>,
    /// Skip the menus and start a board game with these seats.
    #[arg(long)]
    board: bool,
    /// Board game length in rounds (default: the standard rules).
    #[arg(long)]
    rounds: Option<u32>,
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
    /// Press a key at a time after start, as SECS:KEY (repeatable), e.g. `--press 3:Space`.
    #[arg(long = "press", value_parser = dev::parse_key_press)]
    press: Vec<(f32, KeyCode)>,
    /// Press a key every SECS seconds, as SECS:KEY (repeatable).
    #[arg(long = "press-every", value_parser = dev::parse_key_press)]
    press_every: Vec<(f32, KeyCode)>,
    /// Graphics preset; F4 (or Select on a pad) toggles it in-game.
    #[arg(long, value_enum, default_value_t = quality::Quality::Desktop)]
    quality: quality::Quality,
    /// Print average and worst GPU frame time when the game exits.
    #[arg(long)]
    gpu_report: bool,
    /// Log every gamepad button press (diagnoses controller mappings).
    #[arg(long)]
    log_input: bool,
    /// Write every sound effect to this directory as WAVs, then exit.
    #[arg(long, value_name = "DIR")]
    dump_sfx: Option<PathBuf>,
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

/// Where `assets/` is, relative to Bevy's base path. Under `cargo run` that's the crate
/// directory and a shipped build keeps `assets/` beside the executable; a dev binary started
/// directly (say under gamescope) has neither, so it falls back to this crate's own folder.
fn asset_dir() -> String {
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("assets")))
        .is_some_and(|dir| dir.is_dir());
    if std::env::var_os("CARGO_MANIFEST_DIR").is_some() || beside_exe {
        "assets".into()
    } else {
        concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into()
    }
}

fn main() {
    let cli = Cli::parse();
    if let Some(dir) = &cli.dump_sfx {
        audio::dump(dir).expect("couldn't write the sounds");
        println!(
            "Wrote {} sounds to {}",
            audio::Sound::ALL.len() + 1,
            dir.display()
        );
        return;
    }
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
                gamepad: Pad::Nth(i),
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
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: GAME_TITLE.into(),
                    resolution: WindowResolution::new(1280, 800),
                    ..default()
                }),
                ..default()
            })
            .set(AssetPlugin {
                file_path: asset_dir(),
                ..default()
            })
            // Sound goes through bevy_kira_audio (`audio.rs`) instead.
            .disable::<bevy::audio::AudioPlugin>(),
    )
    .insert_resource(Time::<Fixed>::from_hz(TICK_HZ as f64))
    .insert_resource(Seats(seats))
    .init_resource::<characters::Roster>()
    .add_plugins((
        perf::PerfOverlayPlugin {
            report: cli.gpu_report,
        },
        quality::QualityPlugin(cli.quality),
        scenery::SceneryPlugin,
        characters::CharactersPlugin,
        audio::AudioPlugin,
    ));

    app.add_plugins((
        runner::InputPlugin,
        intro::IntroPlugin,
        minigames::MinigamesPlugin,
        board_view::BoardViewPlugin,
        menus::MenusPlugin,
        particles::ParticlesPlugin,
    ));
    match &cli.minigame {
        Some(id) => {
            let Some(entry) = minigame_registry::find(id) else {
                let known: Vec<&str> = minigame_registry::ALL.iter().map(|e| e.id).collect();
                eprintln!("unknown minigame {id:?} (known: {})", known.join(", "));
                std::process::exit(2);
            };
            app.insert_resource(MinigameLaunch {
                id: entry.id.to_string(),
                setup,
                origin: Origin::FreePlay,
            })
            .insert_state(AppState::Minigame);
        }
        None if cli.board => {
            let mut rules = Rules::default();
            if let Some(rounds) = cli.rounds {
                rules.rounds = rounds.max(1);
            }
            let game = Game::new(
                Board::meadow(),
                rules,
                players,
                seed,
                &minigames::shown_entries(),
            );
            app.insert_resource(board_view::BoardSession::new(game))
                .insert_state(AppState::Board);
        }
        // The real start: title, then the join screen binds controllers to seats.
        None => {
            app.insert_resource(Seats([None; 4]))
                .insert_state(AppState::Title);
        }
    }

    app.add_systems(Update, flow::log_transitions);
    if !cli.press.is_empty() || !cli.press_every.is_empty() {
        app.add_plugins(dev::ScriptedKeysPlugin {
            once: cli.press,
            every: cli.press_every,
        });
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
