//! Every minigame in the game, so the board, the game and the tools can find and launch one by
//! id or format without naming its type. A new minigame becomes playable by adding it to [`ALL`].

use minigame_quick_draw::QuickDraw;
use sim_core::MinigameEntry;

pub const ALL: &[MinigameEntry] = &[MinigameEntry::of::<QuickDraw>()];

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
