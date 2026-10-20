//! Every minigame's runner and view. A minigame in `minigame_registry::ALL` must be added here
//! too, or the board could pick a game the client can't show (a test checks this).

use bevy::prelude::*;
use minigame_firefly_catch::FireflyCatch;
use minigame_lantern_push::LanternPush;
use minigame_quick_draw::QuickDraw;
use sim_core::Minigame;

use crate::runner::MinigamePlugin;
use crate::{firefly_catch, lantern_push, quick_draw};

/// Ids of the minigames this client can show.
pub const SHOWN: &[&str] = &[QuickDraw::ID, LanternPush::ID, FireflyCatch::ID];

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
