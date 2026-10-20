//! Play whole bot-only board games without a screen, minigames included: the board's
//! end-to-end determinism check and the engine behind `tools board`.

use sim_core::registry::play_with_bots;
use sim_core::{Difficulty, MAX_PLAYERS, MinigameEntry, Outcome, checksum};

use crate::bot::bot_intent;
use crate::game::{Awaiting, Event, Game, MinigamePlan};

/// Guards against a game that never ends; a real game needs a few hundred decisions.
const MAX_DECISIONS: u32 = 100_000;

#[derive(Debug, Clone)]
pub struct PlayedMinigame {
    pub round: u32,
    pub plan: MinigamePlan,
    pub outcome: Outcome,
}

#[derive(Debug, Clone)]
pub struct Summary {
    /// The finished game: final coins, stars and positions.
    pub game: Game,
    pub minigames: Vec<PlayedMinigame>,
    pub placements: Outcome,
    /// Every event in order, as a view would have received them.
    pub events: Vec<Event>,
    /// [`checksum`] of the finished game.
    pub checksum: u64,
}

/// Play `game` to the end with every seat a bot of `difficulty`, minigames included.
/// `minigames` must contain every minigame the game was created with.
pub fn play_bots(
    mut game: Game,
    difficulty: Difficulty,
    minigames: &[MinigameEntry],
) -> Result<Summary, String> {
    let mut events = Vec::new();
    let mut played = Vec::new();
    for _ in 0..MAX_DECISIONS {
        match game.awaiting() {
            Awaiting::Intent { seat, .. } => {
                let intent = bot_intent(&game, seat, difficulty).expect("asked seat answers");
                let new = game.apply(seat, intent).map_err(|e| {
                    format!("bot intent {intent:?} from P{} refused: {e}", seat.0 + 1)
                })?;
                events.extend(new);
            }
            Awaiting::Minigame(plan) => {
                let entry = minigames
                    .iter()
                    .find(|m| m.id == plan.id)
                    .ok_or_else(|| format!("minigame {} is not in the registry", plan.id))?;
                let outcome = play_with_bots(
                    entry,
                    plan.seed,
                    plan.players,
                    plan.format,
                    [difficulty; MAX_PLAYERS],
                )
                .ok_or_else(|| format!("{} did not finish (seed {})", plan.id, plan.seed))?;
                played.push(PlayedMinigame {
                    round: game.round(),
                    plan,
                    outcome,
                });
                let new = game
                    .apply_minigame_outcome(outcome)
                    .map_err(|e| format!("minigame outcome refused: {e}"))?;
                events.extend(new);
            }
            Awaiting::GameOver => {
                return Ok(Summary {
                    placements: game.standings(),
                    checksum: checksum(&game),
                    game,
                    minigames: played,
                    events,
                });
            }
        }
    }
    Err(format!(
        "board game still running after {MAX_DECISIONS} decisions"
    ))
}

#[cfg(test)]
mod tests {
    use sim_core::FormatKind;

    use super::*;
    use crate::game::Rules;
    use crate::testing::BOTH;
    use crate::{Board, Game};

    fn play(seed: u64, players: usize, difficulty: Difficulty) -> Summary {
        let game = Game::new(Board::meadow(), Rules::default(), players, seed, BOTH);
        play_bots(game, difficulty, BOTH).unwrap()
    }

    #[test]
    fn a_full_game_finishes_with_a_minigame_every_round() {
        let summary = play(1, 4, Difficulty::Normal);
        assert_eq!(summary.game.round(), Rules::default().rounds);
        assert_eq!(summary.minigames.len(), Rules::default().rounds as usize);
        assert_eq!(
            summary.events.last(),
            Some(&Event::GameOver {
                placements: summary.placements
            })
        );
        let stars: u32 = summary
            .game
            .players()
            .seats()
            .map(|s| summary.game.player(s).stars)
            .sum();
        assert!(stars > 0, "nobody bought a star in a whole game");
    }

    #[test]
    fn games_replay_identically_from_a_seed() {
        let a = play(5, 4, Difficulty::Normal);
        let b = play(5, 4, Difficulty::Normal);
        assert_eq!(a.checksum, b.checksum);
        assert_eq!(a.events, b.events);
        let c = play(6, 4, Difficulty::Normal);
        assert_ne!(a.checksum, c.checksum);
        assert_ne!(a.events, c.events);
    }

    #[test]
    fn team_formats_come_up() {
        let kinds: Vec<FormatKind> = (0..4)
            .flat_map(|seed| play(seed, 4, Difficulty::Hard).minigames)
            .map(|m| m.plan.format.kind())
            .collect();
        assert!(kinds.contains(&FormatKind::OneVsThree));
        assert!(kinds.contains(&FormatKind::TwoVsTwo));
    }

    #[test]
    fn every_player_count_and_difficulty_finishes() {
        for players in 2..=MAX_PLAYERS {
            for difficulty in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard] {
                let summary = play(players as u64, players, difficulty);
                assert!(
                    summary
                        .minigames
                        .iter()
                        .all(|m| m.plan.players.count() == players)
                );
            }
        }
    }
}
