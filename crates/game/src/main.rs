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
mod online;
mod palette;
mod particles;
mod perf;
mod quality;
mod quick_draw;
mod runner;
mod save;
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
    /// Board game length in rounds (default: the standard rules), also when hosting.
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
    /// Also where the join screen's setting starts, when hosting.
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
    /// Skip the title and host an online board game (friends join with `--join`).
    #[arg(long)]
    host: bool,
    /// Skip the title and join the online game hosted at this address, e.g. 127.0.0.1:7777.
    #[arg(long, value_name = "ADDRESS")]
    join: Option<String>,
    /// The port to host on.
    #[arg(long, default_value_t = online::DEFAULT_PORT)]
    port: u16,
    /// Delay every datagram this machine sends by this much (feel a laggy connection locally).
    #[arg(long, value_name = "MS", default_value_t = 0)]
    net_delay: u64,
    /// Lose this percentage of the datagrams this machine sends.
    #[arg(long, value_name = "PERCENT", default_value_t = 0)]
    net_loss: u32,
    /// Hosting: how long to wait for a machine that dropped out before offering to go on
    /// without it.
    #[arg(long, value_name = "SECS", default_value_t = online::WAIT_OPTIONS_SECS)]
    wait_options_after: f32,
    /// The saved board game's file (default: `party-game/board.save` in the user's data folder).
    /// `--board` only saves when this is given.
    #[arg(long, value_name = "FILE")]
    save: Option<PathBuf>,
    /// Skip the title and pick up the saved board game (with `--host`, host it).
    #[arg(long)]
    resume: bool,
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

    let net_settings = online::NetSettings {
        port: cli.port,
        faults: net::Faults {
            loss_percent: cli.net_loss.min(100),
            delay: std::time::Duration::from_millis(cli.net_delay),
            // A little jitter around the delay, as a real connection has.
            jitter: std::time::Duration::from_millis(cli.net_delay / 4),
            duplicate_percent: 0,
        },
        wait_options_secs: cli.wait_options_after,
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
    .insert_resource(runner::RunRules(setup.config))
    .insert_resource(net_settings)
    .insert_resource(save::SaveSlot(match (&cli.save, cli.board) {
        (Some(path), _) => Some(path.clone()),
        // A dev shortcut shouldn't overwrite the real save.
        (None, true) => None,
        (None, false) => save::SaveSlot::default_path(),
    }))
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
        online::OnlinePlugin,
        particles::ParticlesPlugin,
        save::SavePlugin,
    ));
    let resume = cli.resume.then(|| {
        let slot = app.world().resource::<save::SaveSlot>();
        slot.load().unwrap_or_else(|notice| {
            eprintln!("{}", notice.0);
            std::process::exit(1);
        })
    });
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
        None if cli.host => {
            let (mode, rounds) = (menus::PlayMode::Board, cli.rounds);
            match menus::host_online(&net_settings, mode, rounds, setup.config, resume.as_ref()) {
                Ok(online) => {
                    app.insert_resource(Seats([None; 4]))
                        .insert_resource(online)
                        .insert_state(AppState::Join);
                    if let Some(save) = resume {
                        app.insert_resource(save::Resumed(save));
                    }
                }
                Err(notice) => {
                    eprintln!("{}", notice.0);
                    std::process::exit(1);
                }
            }
        }
        None if resume.is_some() => {
            app.insert_resource(Seats([None; 4]))
                .insert_resource(save::Resumed(resume.expect("checked")))
                .insert_resource(menus::PlayMode::Board)
                .insert_state(AppState::Join);
        }
        None if cli.join.is_some() => {
            let address = cli.join.clone().unwrap_or_default();
            app.insert_resource(Seats([None; 4]))
                .insert_resource(menus::ConnectScreen::connecting(address))
                .insert_state(AppState::Connect);
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
