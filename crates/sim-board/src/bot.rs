//! Board bots. Like minigame bots they are virtual controllers: a pure function of the state
//! that returns the same [`Intent`] a human would send.

use sim_core::{Difficulty, Rng, Seat, checksum};

use crate::board::SpaceId;
use crate::game::{Awaiting, Decision, Game, Intent};

/// The intent a bot in `seat` sends now, or `None` if the game isn't waiting on that seat.
pub fn bot_intent(game: &Game, seat: Seat, difficulty: Difficulty) -> Option<Intent> {
    let Awaiting::Intent {
        seat: asked,
        decision,
    } = game.awaiting()
    else {
        return None;
    };
    if asked != seat {
        return None;
    }
    // Randomness derived from the state, so the bot stays a pure function.
    let mut rng =
        Rng::new(checksum(game) ^ (seat.0 as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15));
    Some(match decision {
        Decision::TurnOrderRoll | Decision::Roll => Intent::Roll,
        Decision::ChoosePath { options } => {
            let smart_percent = match difficulty {
                Difficulty::Easy => 35,
                Difficulty::Normal => 80,
                Difficulty::Hard => 100,
            };
            let index = if rng.chance(smart_percent) {
                path_towards_star(game, &options)
            } else {
                rng.range_u32(0, options.len() as u32 - 1) as usize
            };
            Intent::ChoosePath(index as u8)
        }
        // Only offered when affordable, and a star is always worth it; Easy sometimes dithers.
        Decision::BuyStar { .. } => {
            Intent::BuyStar(!(difficulty == Difficulty::Easy && rng.chance(10)))
        }
    })
}

/// Index of the option with the fewest steps to the Star (the first one on a tie).
fn path_towards_star(game: &Game, options: &[SpaceId]) -> usize {
    let star = game.star();
    (0..options.len())
        .min_by_key(|i| game.board().distance(options[*i], star).unwrap_or(u32::MAX))
        .expect("a fork has options")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Rules;
    use crate::testing::{self, BOTH};

    /// A 4-player game with the current seat waiting at the test board's fork (2 → 3 or 4).
    fn at_fork(star: u16) -> (Game, Seat) {
        // Bots play until someone reaches the fork with steps to spare; most seeds get there fast.
        for seed in 0.. {
            let mut game = Game::new(testing::board(), Rules::default(), 4, seed, BOTH);
            while let Awaiting::Intent { seat, decision } = game.awaiting() {
                if let Decision::ChoosePath { .. } = decision {
                    game.set_star(SpaceId(star));
                    return (game, seat);
                }
                let intent = bot_intent(&game, seat, Difficulty::Hard).unwrap();
                game.apply(seat, intent).unwrap();
            }
        }
        unreachable!()
    }

    #[test]
    fn bots_only_answer_their_own_decisions() {
        let game = Game::new(testing::board(), Rules::default(), 4, 0, BOTH);
        assert_eq!(
            bot_intent(&game, Seat(0), Difficulty::Easy),
            Some(Intent::Roll)
        );
        assert_eq!(bot_intent(&game, Seat(1), Difficulty::Easy), None);
    }

    #[test]
    fn hard_bots_head_for_the_star() {
        for (star, best) in [(3, 0), (4, 1)] {
            let (game, seat) = at_fork(star);
            assert_eq!(
                bot_intent(&game, seat, Difficulty::Hard),
                Some(Intent::ChoosePath(best))
            );
        }
    }

    #[test]
    fn bots_are_pure() {
        let (game, seat) = at_fork(3);
        for difficulty in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard] {
            assert_eq!(
                bot_intent(&game, seat, difficulty),
                bot_intent(&game.clone(), seat, difficulty)
            );
        }
    }
}
