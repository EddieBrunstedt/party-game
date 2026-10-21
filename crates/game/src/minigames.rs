//! Every minigame's runner and view. A minigame in `minigame_registry::ALL` must be added here
//! too, or the board could pick a game the client can't show (a test checks this).

use bevy::prelude::*;
use minigame_firefly_catch::FireflyCatch;
use minigame_lantern_echo::LanternEcho;
use minigame_lantern_push::LanternPush;
use minigame_lantern_tower::LanternTower;
use minigame_quick_draw::QuickDraw;
use sim_core::{Minigame, MinigameEntry};

use crate::runner::MinigamePlugin;
use crate::{firefly_catch, lantern_echo, lantern_push, lantern_tower, quick_draw};

/// Ids of the minigames this client can show.
pub const SHOWN: &[&str] = &[
    QuickDraw::ID,
    LanternPush::ID,
    FireflyCatch::ID,
    LanternEcho::ID,
    LanternTower::ID,
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
        app.add_plugins((
            MinigamePlugin::<QuickDraw>::default(),
            quick_draw::QuickDrawViewPlugin,
            MinigamePlugin::<LanternPush>::default(),
            lantern_push::LanternPushViewPlugin,
            MinigamePlugin::<FireflyCatch>::default(),
            firefly_catch::FireflyCatchViewPlugin,
            MinigamePlugin::<LanternEcho>::default(),
            lantern_echo::LanternEchoViewPlugin,
            MinigamePlugin::<LanternTower>::default(),
            lantern_tower::LanternTowerViewPlugin,
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
