//! Whole board games with the real minigames: they finish, and replay identically from a seed.

use sim_board::{Board, Game, Rules, play_bots};
use sim_core::Difficulty;

fn play(seed: u64, players: usize) -> sim_board::Summary {
    let rules = Rules {
        rounds: 4,
        ..Rules::default()
    };
    let game = Game::new(
        Board::meadow(),
        rules,
        players,
        seed,
        minigame_registry::ALL,
    );
    play_bots(game, Difficulty::Normal, minigame_registry::ALL).unwrap()
}

#[test]
fn full_games_with_real_minigames_are_deterministic() {
    for (seed, players) in [(1, 4), (2, 3), (3, 2)] {
        let a = play(seed, players);
        let b = play(seed, players);
        assert_eq!(a.game.round(), 4);
        assert_eq!(a.minigames.len(), 4);
        assert_eq!(a.checksum, b.checksum);
        assert_eq!(a.events, b.events);
    }
    assert_ne!(play(1, 4).checksum, play(4, 4).checksum);
}
