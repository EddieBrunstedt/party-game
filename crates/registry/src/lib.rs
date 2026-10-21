//! Every minigame in the game, so the board, the game and the tools can find and launch one by
//! id or format without naming its type. A new minigame becomes playable by adding it to [`ALL`].

use minigame_firefly_catch::FireflyCatch;
use minigame_hedge_maze::HedgeMaze;
use minigame_lantern_echo::LanternEcho;
use minigame_lantern_lottery::LanternLottery;
use minigame_lantern_push::LanternPush;
use minigame_lantern_tower::LanternTower;
use minigame_musical_stools::MusicalStools;
use minigame_quick_draw::QuickDraw;
use minigame_sky_lanterns::SkyLanterns;
use sim_core::MinigameEntry;

pub const ALL: &[MinigameEntry] = &[
    MinigameEntry::of::<QuickDraw>(),
    MinigameEntry::of::<LanternPush>(),
    MinigameEntry::of::<FireflyCatch>(),
    MinigameEntry::of::<LanternEcho>(),
    MinigameEntry::of::<LanternTower>(),
    MinigameEntry::of::<SkyLanterns>(),
    MinigameEntry::of::<LanternLottery>(),
    MinigameEntry::of::<MusicalStools>(),
    MinigameEntry::of::<HedgeMaze>(),
];

pub fn find(id: &str) -> Option<&'static MinigameEntry> {
    ALL.iter().find(|entry| entry.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        for (i, entry) in ALL.iter().enumerate() {
            assert!(
                ALL[..i].iter().all(|other| other.id != entry.id),
                "{} is registered twice",
                entry.id
            );
        }
    }

    #[test]
    fn find_looks_up_by_id() {
        assert_eq!(find("quick-draw").map(|e| e.id), Some("quick-draw"));
        assert!(find("no-such-game").is_none());
    }
}
