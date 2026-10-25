//! Every minigame's runner and view. A minigame in `minigame_registry::ALL` must be added here
//! too, or the board could pick a game the client can't show (a test checks this).

use bevy::prelude::*;
use minigame_firefly_catch::FireflyCatch;
use minigame_hedge_maze::HedgeMaze;
use minigame_lantern_ball::LanternBall;
use minigame_lantern_derby::LanternDerby;
use minigame_lantern_echo::LanternEcho;
use minigame_lantern_flail::LanternFlail;
use minigame_lantern_lottery::LanternLottery;
use minigame_lantern_masquerade::LanternMasquerade;
use minigame_lantern_push::LanternPush;
use minigame_lantern_run::LanternRun;
use minigame_lantern_tower::LanternTower;
use minigame_musical_stools::MusicalStools;
use minigame_puzzle_buzzer::PuzzleBuzzer;
use minigame_quick_draw::QuickDraw;
use minigame_sky_lanterns::SkyLanterns;
use minigame_tabletop_rally::TabletopRally;
use minigame_triple_jump::TripleJump;
use sim_core::{Minigame, MinigameEntry};

use crate::runner::MinigamePlugin;
use crate::{
    firefly_catch, hedge_maze, lantern_ball, lantern_derby, lantern_echo, lantern_flail,
    lantern_lottery, lantern_masquerade, lantern_push, lantern_run, lantern_tower, musical_stools,
    puzzle_buzzer, quick_draw, sky_lanterns, tabletop_rally, triple_jump,
};

/// Ids of the minigames this client can show.
pub const SHOWN: &[&str] = &[
    QuickDraw::ID,
    LanternPush::ID,
    FireflyCatch::ID,
    LanternEcho::ID,
    LanternTower::ID,
    SkyLanterns::ID,
    LanternLottery::ID,
    MusicalStools::ID,
    HedgeMaze::ID,
    LanternDerby::ID,
    TabletopRally::ID,
    TripleJump::ID,
    PuzzleBuzzer::ID,
    LanternMasquerade::ID,
    LanternFlail::ID,
    LanternRun::ID,
    LanternBall::ID,
];

/// The registry entries this client can show, in registry order: what the board may pick from.
pub fn shown_entries() -> Vec<MinigameEntry> {
    minigame_registry::ALL
        .iter()
        .filter(|entry| SHOWN.contains(&entry.id))
        .copied()
        .collect()
}

pub struct MinigamesPlugin;

impl Plugin for MinigamesPlugin {
    fn build(&self, app: &mut App) {
        // One (runner, view) pair per minigame.
        app.add_plugins((
            (
                MinigamePlugin::<QuickDraw>::default(),
                quick_draw::QuickDrawViewPlugin,
            ),
            (
                MinigamePlugin::<LanternPush>::default(),
                lantern_push::LanternPushViewPlugin,
            ),
            (
                MinigamePlugin::<FireflyCatch>::default(),
                firefly_catch::FireflyCatchViewPlugin,
            ),
            (
                MinigamePlugin::<LanternEcho>::default(),
                lantern_echo::LanternEchoViewPlugin,
            ),
            (
                MinigamePlugin::<LanternTower>::default(),
                lantern_tower::LanternTowerViewPlugin,
            ),
            (
                MinigamePlugin::<SkyLanterns>::default(),
                sky_lanterns::SkyLanternsViewPlugin,
            ),
            (
                MinigamePlugin::<LanternLottery>::default(),
                lantern_lottery::LanternLotteryViewPlugin,
            ),
            (
                MinigamePlugin::<MusicalStools>::default(),
                musical_stools::MusicalStoolsViewPlugin,
            ),
            (
                MinigamePlugin::<HedgeMaze>::default(),
                hedge_maze::HedgeMazeViewPlugin,
            ),
            (
                MinigamePlugin::<LanternDerby>::default(),
                lantern_derby::LanternDerbyViewPlugin,
            ),
            (
                MinigamePlugin::<TabletopRally>::default(),
                tabletop_rally::TabletopRallyViewPlugin,
            ),
            (
                MinigamePlugin::<TripleJump>::default(),
                triple_jump::TripleJumpViewPlugin,
            ),
            (
                MinigamePlugin::<PuzzleBuzzer>::default(),
                puzzle_buzzer::PuzzleBuzzerViewPlugin,
            ),
            (
                MinigamePlugin::<LanternMasquerade>::default(),
                lantern_masquerade::LanternMasqueradeViewPlugin,
            ),
            (
                MinigamePlugin::<LanternFlail>::default(),
                lantern_flail::LanternFlailViewPlugin,
            ),
        ));
        // Plugin tuples stop at 15.
        app.add_plugins((
            MinigamePlugin::<LanternRun>::default(),
            lantern_run::LanternRunViewPlugin,
            MinigamePlugin::<LanternBall>::default(),
            lantern_ball::LanternBallViewPlugin,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_minigame_has_a_view() {
        for entry in minigame_registry::ALL {
            assert!(SHOWN.contains(&entry.id), "{} has no view", entry.id);
        }
    }
}
