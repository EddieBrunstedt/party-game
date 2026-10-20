//! The board graph: spaces on the floor, the arrows between them, and where the Star may sit.
//! Boards are RON files (DESIGN.md §4.6), the source of truth that the Blender scene decorates.

use std::collections::VecDeque;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use sim_core::hash::hash_f32;

/// A space's id, which is also its index in [`Board::spaces`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SpaceId(pub u16);

impl SpaceId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SpaceKind {
    Blue,
    Red,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Space {
    pub id: SpaceId,
    /// Floor position in world units; the view places the space at (x, 0, y).
    pub x: f32,
    pub y: f32,
    pub kind: SpaceKind,
    /// Where a player may step from here. More than one entry is a fork: the player chooses.
    pub next: Vec<SpaceId>,
}

impl Space {
    pub fn is_fork(&self) -> bool {
        self.next.len() > 1
    }
}

impl Hash for Space {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        hash_f32(self.x, state);
        hash_f32(self.y, state);
        self.kind.hash(state);
        self.next.hash(state);
    }
}

/// A validated board. Deserializing one (from a board file or a save) always validates it.
#[derive(Debug, Clone, PartialEq, Hash, Serialize, Deserialize)]
#[serde(try_from = "BoardFile")]
pub struct Board {
    name: String,
    start: SpaceId,
    star_spots: Vec<SpaceId>,
    spaces: Vec<Space>,
}

/// A board file as written, before validation.
#[derive(Deserialize)]
struct BoardFile {
    name: String,
    start: SpaceId,
    star_spots: Vec<SpaceId>,
    spaces: Vec<Space>,
}

impl Board {
    /// Dusk Meadow: a small loop with two forks. See `boards/meadow.ron`.
    pub fn meadow() -> Self {
        Self::from_ron(include_str!("../boards/meadow.ron")).expect("bundled board is valid")
    }

    /// Parse and validate a board file.
    pub fn from_ron(text: &str) -> Result<Self, String> {
        ron::from_str(text).map_err(|e| e.to_string())
    }

    /// Display name, e.g. "Dusk Meadow".
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Where every player begins.
    pub fn start(&self) -> SpaceId {
        self.start
    }

    /// The spaces the Star may sit on.
    pub fn star_spots(&self) -> &[SpaceId] {
        &self.star_spots
    }

    /// Every space, in id order.
    pub fn spaces(&self) -> &[Space] {
        &self.spaces
    }

    pub fn space(&self, id: SpaceId) -> &Space {
        &self.spaces[id.index()]
    }

    /// Fewest steps from `from` to `to` along the arrows, or `None` if `to` can't be reached.
    pub fn distance(&self, from: SpaceId, to: SpaceId) -> Option<u32> {
        let mut steps = vec![None; self.spaces.len()];
        steps[from.index()] = Some(0);
        let mut queue = VecDeque::from([from]);
        while let Some(here) = queue.pop_front() {
            let d = steps[here.index()].expect("queued spaces have a distance");
            if here == to {
                return Some(d);
            }
            for next in &self.space(here).next {
                if steps[next.index()].is_none() {
                    steps[next.index()] = Some(d + 1);
                    queue.push_back(*next);
                }
            }
        }
        None
    }

    /// Every space reachable from `from`, as a per-space flag.
    fn reachable_from(&self, from: SpaceId) -> Vec<bool> {
        let mut seen = vec![false; self.spaces.len()];
        seen[from.index()] = true;
        let mut queue = VecDeque::from([from]);
        while let Some(here) = queue.pop_front() {
            for next in &self.space(here).next {
                if !seen[next.index()] {
                    seen[next.index()] = true;
                    queue.push_back(*next);
                }
            }
        }
        seen
    }
}

impl TryFrom<BoardFile> for Board {
    type Error = String;

    fn try_from(file: BoardFile) -> Result<Self, String> {
        let count = file.spaces.len();
        let exists = |id: SpaceId| id.index() < count;
        if count == 0 {
            return Err("board has no spaces".into());
        }
        if count > u16::MAX as usize {
            return Err("board has too many spaces".into());
        }
        for (index, space) in file.spaces.iter().enumerate() {
            let id = space.id.0;
            if space.id.index() != index {
                return Err(format!(
                    "space at position {index} has id {id}; ids must count up from 0 in order"
                ));
            }
            if !space.x.is_finite() || !space.y.is_finite() {
                return Err(format!("space {id} has a non-finite position"));
            }
            if space.next.is_empty() {
                return Err(format!("space {id} is a dead end"));
            }
            for (i, next) in space.next.iter().enumerate() {
                if !exists(*next) {
                    return Err(format!("space {id} leads to missing space {}", next.0));
                }
                if *next == space.id {
                    return Err(format!("space {id} leads to itself"));
                }
                if space.next[..i].contains(next) {
                    return Err(format!("space {id} lists {} twice", next.0));
                }
            }
        }
        if !exists(file.start) {
            return Err(format!("start space {} does not exist", file.start.0));
        }
        if file.star_spots.len() < 2 {
            return Err("need at least two star spots, so the Star can move".into());
        }
        for (i, spot) in file.star_spots.iter().enumerate() {
            if !exists(*spot) {
                return Err(format!("star spot {} does not exist", spot.0));
            }
            if file.star_spots[..i].contains(spot) {
                return Err(format!("star spot {} is listed twice", spot.0));
            }
        }

        let board = Board {
            name: file.name,
            start: file.start,
            star_spots: file.star_spots,
            spaces: file.spaces,
        };
        if let Some(lost) = board.reachable_from(board.start).iter().position(|r| !r) {
            return Err(format!("space {lost} cannot be reached from the start"));
        }
        // Nobody may end up somewhere the Star can never be reached from.
        for space in &board.spaces {
            let reachable = board.reachable_from(space.id);
            if let Some(spot) = board.star_spots.iter().find(|s| !reachable[s.index()]) {
                return Err(format!(
                    "star spot {} cannot be reached from space {}",
                    spot.0, space.id.0
                ));
            }
        }
        Ok(board)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(spaces: &str) -> Result<Board, String> {
        Board::from_ron(&format!(
            "(name: \"Test\", start: 0, star_spots: [1, 2], spaces: [{spaces}])"
        ))
    }

    const LOOP: &str = "(id: 0, x: 0.0, y: 0.0, kind: Blue, next: [1]),
        (id: 1, x: 2.0, y: 0.0, kind: Red, next: [2]),
        (id: 2, x: 2.0, y: 2.0, kind: Blue, next: [0]),";

    #[test]
    fn meadow_loads_and_has_the_intended_shape() {
        let board = Board::meadow();
        assert_eq!(board.name(), "Dusk Meadow");
        let spaces = board.spaces();
        assert!((32..=44).contains(&spaces.len()));
        assert_eq!(spaces.iter().filter(|s| s.is_fork()).count(), 2);
        assert!((4..=6).contains(&board.star_spots().len()));
        let red = spaces.iter().filter(|s| s.kind == SpaceKind::Red).count();
        let red_percent = red * 100 / spaces.len();
        assert!((20..=30).contains(&red_percent), "{red_percent}% red");
        for spot in board.star_spots() {
            assert_ne!(*spot, board.start());
        }
        // Neighbours sit about two units apart, so the view can space its tiles evenly.
        for space in spaces {
            for next in &space.next {
                let to = board.space(*next);
                let d = ((to.x - space.x).powi(2) + (to.y - space.y).powi(2)).sqrt();
                assert!(
                    (1.5..=3.0).contains(&d),
                    "{} -> {}: {d}",
                    space.id.0,
                    to.id.0
                );
            }
        }
    }

    #[test]
    fn distance_follows_the_arrows() {
        let board = board(LOOP).unwrap();
        assert_eq!(board.distance(SpaceId(0), SpaceId(2)), Some(2));
        assert_eq!(board.distance(SpaceId(2), SpaceId(1)), Some(2));
        assert_eq!(board.distance(SpaceId(1), SpaceId(1)), Some(0));
    }

    #[test]
    fn broken_boards_are_rejected() {
        assert!(board(LOOP).is_ok());
        let dead_end = LOOP.replace("next: [0]", "next: []");
        assert!(board(&dead_end).unwrap_err().contains("dead end"));
        let missing = LOOP.replace("next: [0]", "next: [7]");
        assert!(board(&missing).unwrap_err().contains("missing space 7"));
        let misnumbered = LOOP.replace("id: 2", "id: 5");
        assert!(
            board(&misnumbered)
                .unwrap_err()
                .contains("ids must count up")
        );
        let unreachable = format!("{LOOP} (id: 3, x: 4.0, y: 0.0, kind: Blue, next: [0]),");
        assert!(
            board(&unreachable)
                .unwrap_err()
                .contains("cannot be reached")
        );
        let trapped = format!(
            "{} (id: 3, x: 4.0, y: 0.0, kind: Blue, next: [4]),
            (id: 4, x: 4.0, y: 2.0, kind: Blue, next: [3]),",
            LOOP.replace("next: [1]", "next: [1, 3]")
        );
        assert!(
            board(&trapped)
                .unwrap_err()
                .contains("cannot be reached from space 3")
        );
        let bad_star = Board::from_ron(&format!(
            "(name: \"Test\", start: 0, star_spots: [1, 9], spaces: [{LOOP}])"
        ));
        assert!(bad_star.unwrap_err().contains("star spot 9"));
    }
}
