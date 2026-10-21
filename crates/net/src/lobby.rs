//! The join screen's rules, shared by couch and online play: on the couch the only machine is
//! the host. Each machine numbers its own controllers (`local`), and a seat is claimed by a
//! machine's controller. The host's copy is the real one; clients send it [`LobbyAction`]s and
//! show what it sends back.

use serde::{Deserialize, Serialize};
use sim_board::Save;
use sim_core::{Difficulty, MAX_PLAYERS, PlayerSetup, RunConfig, Seat};

/// A machine in the session. The host is always 0; the host numbers the others as they join.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PeerId(pub u8);

impl PeerId {
    pub const HOST: Self = Self(0);
}

/// One controller on one machine sitting in a seat, as a character (an index into the game's
/// character list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Claim {
    pub peer: PeerId,
    pub local: u8,
    pub character: u8,
}

/// What a machine's controller asks of the lobby.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LobbyAction {
    /// Take the first free seat, with the first character nobody has.
    Join {
        local: u8,
    },
    /// The next (or previous) character nobody else has.
    Pick {
        local: u8,
        next: bool,
    },
    Leave {
        local: u8,
    },
    /// Host only, like [`LobbyAction::Rounds`] and [`LobbyAction::TogglePauseOnControllerLost`].
    CycleBots,
    Rounds {
        longer: bool,
    },
    TogglePauseOnControllerLost,
}

/// A saved game being picked up again: every seat keeps its character, and players choose
/// which of the saved seats to take (left/right moves between them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resume {
    pub characters: [u8; MAX_PLAYERS],
    /// Seats people played. Joining offers these first.
    pub humans: [bool; MAX_PLAYERS],
    /// The seats in the game; the others can't be taken.
    pub players: PlayerSetup,
    /// The round the game was saved in.
    pub round: u32,
}

/// Board lengths the join screen offers.
pub const ROUND_CHOICES: [u32; 5] = [6, 9, 12, 15, 20];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LobbyState {
    pub seats: [Option<Claim>; MAX_PLAYERS],
    /// How well the bots in empty seats play.
    pub bots: Difficulty,
    pub rounds: u32,
    /// Minigames halt for everyone while any player's controller is unplugged.
    pub pause_on_controller_lost: bool,
    /// Free play (a minigame picker) instead of a board game.
    pub free_play: bool,
    /// How many characters there are to pick from.
    pub characters: u8,
    /// Picking up a saved game rather than starting one.
    pub resume: Option<Resume>,
}

impl LobbyState {
    pub fn new(characters: u8, rounds: u32, free_play: bool) -> Self {
        assert!(
            characters as usize >= MAX_PLAYERS,
            "every seat needs its own character"
        );
        Self {
            seats: [None; MAX_PLAYERS],
            bots: Difficulty::Normal,
            rounds,
            pause_on_controller_lost: false,
            free_play,
            characters,
            resume: None,
        }
    }

    /// A lobby for picking `save` up again: its seats, characters, length and settings.
    pub fn resuming(characters: u8, save: &Save) -> Self {
        let mut lobby = Self::new(characters, save.game.rules().rounds, false);
        lobby.bots = save.bots;
        lobby.pause_on_controller_lost = save.pause_on_controller_lost;
        lobby.resume = Some(Resume {
            characters: save.seats.map(|s| s.character),
            humans: save.seats.map(|s| s.human),
            players: save.game.players(),
            round: save.game.round(),
        });
        lobby
    }

    /// The rules every machine runs this session's minigames with.
    pub fn run_config(&self) -> RunConfig {
        RunConfig {
            pause_on_controller_lost: self.pause_on_controller_lost,
        }
    }

    pub fn seat_of(&self, peer: PeerId, local: u8) -> Option<Seat> {
        Seat::ALL.into_iter().find(|seat| {
            self.seats[seat.index()].is_some_and(|c| c.peer == peer && c.local == local)
        })
    }

    pub fn anyone_joined(&self) -> bool {
        self.seats.iter().any(Option::is_some)
    }

    pub fn peer_joined(&self, peer: PeerId) -> bool {
        self.seats.iter().flatten().any(|claim| claim.peer == peer)
    }

    /// Applies `action` from one of `peer`'s controllers. Returns whether anything changed;
    /// actions that don't fit (a second join, a client changing settings) change nothing.
    pub fn apply(&mut self, peer: PeerId, action: LobbyAction) -> bool {
        match action {
            LobbyAction::Join { local } => {
                if self.seat_of(peer, local).is_some() {
                    return false;
                }
                let free = |s: &Seat| self.free(*s);
                let seat = match &self.resume {
                    // People's seats first, then the bots'.
                    Some(resume) => (Seat::ALL.into_iter())
                        .filter(free)
                        .find(|s| resume.humans[s.index()])
                        .or_else(|| Seat::ALL.into_iter().find(free)),
                    None => Seat::ALL.into_iter().find(free),
                };
                let Some(seat) = seat else {
                    return false;
                };
                let character = match &self.resume {
                    Some(resume) => resume.characters[seat.index()],
                    None => self.next_character(seat, self.characters - 1, true),
                };
                self.seats[seat.index()] = Some(Claim {
                    peer,
                    local,
                    character,
                });
                true
            }
            LobbyAction::Pick { local, next } => {
                let Some(seat) = self.seat_of(peer, local) else {
                    return false;
                };
                if let Some(resume) = &self.resume {
                    // The character comes with the seat: move to the next free one instead.
                    let n = MAX_PLAYERS as u8;
                    let to = (1..n)
                        .map(|k| Seat(if next { seat.0 + k } else { seat.0 + n - k } % n))
                        .find(|s| self.free(*s));
                    let Some(to) = to else {
                        return false;
                    };
                    let claim = self.seats[seat.index()].take().expect("seat_of found it");
                    self.seats[to.index()] = Some(Claim {
                        character: resume.characters[to.index()],
                        ..claim
                    });
                    return true;
                }
                let from = self.seats[seat.index()]
                    .expect("seat_of found it")
                    .character;
                let picked = self.next_character(seat, from, next);
                self.seats[seat.index()]
                    .as_mut()
                    .expect("still there")
                    .character = picked;
                picked != from
            }
            LobbyAction::Leave { local } => match self.seat_of(peer, local) {
                Some(seat) => {
                    self.seats[seat.index()] = None;
                    true
                }
                None => false,
            },
            LobbyAction::CycleBots if peer == PeerId::HOST => {
                self.bots = match self.bots {
                    Difficulty::Easy => Difficulty::Normal,
                    Difficulty::Normal => Difficulty::Hard,
                    Difficulty::Hard => Difficulty::Easy,
                };
                true
            }
            LobbyAction::Rounds { longer }
                if peer == PeerId::HOST && !self.free_play && self.resume.is_none() =>
            {
                let len = ROUND_CHOICES.len();
                let at = ROUND_CHOICES
                    .iter()
                    .position(|r| *r == self.rounds)
                    .unwrap_or(2);
                let at = if longer {
                    (at + 1) % len
                } else {
                    (at + len - 1) % len
                };
                self.rounds = ROUND_CHOICES[at];
                true
            }
            LobbyAction::TogglePauseOnControllerLost if peer == PeerId::HOST => {
                self.pause_on_controller_lost = !self.pause_on_controller_lost;
                true
            }
            LobbyAction::CycleBots
            | LobbyAction::Rounds { .. }
            | LobbyAction::TogglePauseOnControllerLost => false,
        }
    }

    /// Frees every seat `peer` held, when that machine leaves. Returns whether it held any.
    pub fn remove_peer(&mut self, peer: PeerId) -> bool {
        let mut changed = false;
        for seat in &mut self.seats {
            if seat.is_some_and(|claim| claim.peer == peer) {
                *seat = None;
                changed = true;
            }
        }
        changed
    }

    /// Numbers `peer`'s claims 0, 1, 2... in seat order, for a machine taking that peer's seats
    /// back: its controllers number themselves afresh as they press A.
    pub fn renumber_locals(&mut self, peer: PeerId) {
        let claims = self.seats.iter_mut().flatten();
        let mine = claims.filter(|claim| claim.peer == peer);
        for (local, claim) in mine.enumerate() {
            claim.local = local as u8;
        }
    }

    /// Whether a controller can sit here: nobody has it, and (resuming) the seat is in the game.
    fn free(&self, seat: Seat) -> bool {
        let in_game = self
            .resume
            .as_ref()
            .is_none_or(|r| r.players.is_active(seat));
        self.seats[seat.index()].is_none() && in_game
    }

    /// Characters per seat: players keep theirs, bots take whoever is left, in order. A resumed
    /// game's seats keep the characters they had.
    pub fn roster(&self) -> [u8; MAX_PLAYERS] {
        if let Some(resume) = &self.resume {
            return resume.characters;
        }
        let mut free = (0..self.characters)
            .filter(|c| !self.seats.iter().flatten().any(|s| s.character == *c));
        Seat::ALL.map(|seat| match self.seats[seat.index()] {
            Some(claim) => claim.character,
            None => free.next().expect("at least as many characters as seats"),
        })
    }

    fn taken(&self, character: u8, except: Seat) -> bool {
        Seat::ALL.into_iter().any(|seat| {
            seat != except && self.seats[seat.index()].is_some_and(|c| c.character == character)
        })
    }

    /// The next character after `from` (or before it) that nobody else in the lobby has.
    fn next_character(&self, seat: Seat, from: u8, next: bool) -> u8 {
        let len = self.characters;
        (1..=len)
            .map(|k| {
                if next {
                    (from + k) % len
                } else {
                    (from + len * 2 - k) % len
                }
            })
            .find(|c| !self.taken(*c, seat))
            .unwrap_or(from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: PeerId = PeerId::HOST;
    const THEM: PeerId = PeerId(1);

    fn lobby() -> LobbyState {
        LobbyState::new(4, 12, false)
    }

    #[test]
    fn joining_takes_the_first_free_seat_and_character() {
        let mut lobby = lobby();
        assert!(lobby.apply(ME, LobbyAction::Join { local: 0 }));
        assert!(lobby.apply(THEM, LobbyAction::Join { local: 0 }));
        assert!(
            !lobby.apply(THEM, LobbyAction::Join { local: 0 }),
            "already seated"
        );
        assert_eq!(lobby.seat_of(ME, 0), Some(Seat(0)));
        assert_eq!(lobby.seat_of(THEM, 0), Some(Seat(1)));
        assert_eq!(lobby.seats[0].unwrap().character, 0);
        assert_eq!(lobby.seats[1].unwrap().character, 1);
    }

    #[test]
    fn a_full_lobby_turns_joins_away() {
        let mut lobby = lobby();
        for local in 0..4 {
            assert!(lobby.apply(ME, LobbyAction::Join { local }));
        }
        assert!(!lobby.apply(THEM, LobbyAction::Join { local: 0 }));
    }

    #[test]
    fn picking_skips_characters_someone_else_has() {
        let mut lobby = lobby();
        lobby.apply(ME, LobbyAction::Join { local: 0 });
        lobby.apply(THEM, LobbyAction::Join { local: 0 });
        // Seat 0 has character 0 and seat 1 has 1: moving right skips to 2, left wraps to 3.
        lobby.apply(
            ME,
            LobbyAction::Pick {
                local: 0,
                next: true,
            },
        );
        assert_eq!(lobby.seats[0].unwrap().character, 2);
        lobby.apply(
            ME,
            LobbyAction::Pick {
                local: 0,
                next: false,
            },
        );
        lobby.apply(
            ME,
            LobbyAction::Pick {
                local: 0,
                next: false,
            },
        );
        assert_eq!(lobby.seats[0].unwrap().character, 3);
    }

    #[test]
    fn players_keep_their_characters_and_bots_take_the_rest() {
        let mut lobby = lobby();
        lobby.apply(ME, LobbyAction::Join { local: 0 });
        lobby.apply(
            ME,
            LobbyAction::Pick {
                local: 0,
                next: false,
            },
        );
        lobby.apply(THEM, LobbyAction::Join { local: 3 });
        lobby.apply(THEM, LobbyAction::Leave { local: 3 });
        lobby.apply(THEM, LobbyAction::Join { local: 5 });
        let roster = lobby.roster();
        assert_eq!(roster[0], 3);
        assert_eq!(roster[1], 0);
        let mut sorted = roster;
        sorted.sort();
        assert_eq!(sorted, [0, 1, 2, 3], "every character exactly once");
    }

    #[test]
    fn only_the_host_changes_settings() {
        let mut lobby = lobby();
        assert!(!lobby.apply(THEM, LobbyAction::CycleBots));
        assert!(!lobby.apply(THEM, LobbyAction::Rounds { longer: true }));
        assert!(!lobby.apply(THEM, LobbyAction::TogglePauseOnControllerLost));
        assert!(!lobby.run_config().pause_on_controller_lost);
        assert!(lobby.apply(ME, LobbyAction::TogglePauseOnControllerLost));
        assert!(lobby.run_config().pause_on_controller_lost);
        assert!(lobby.apply(ME, LobbyAction::CycleBots));
        assert_eq!(lobby.bots, Difficulty::Hard);
        assert!(lobby.apply(ME, LobbyAction::Rounds { longer: true }));
        assert_eq!(lobby.rounds, 15);
        assert!(lobby.apply(ME, LobbyAction::Rounds { longer: false }));
        assert!(lobby.apply(ME, LobbyAction::Rounds { longer: false }));
        assert_eq!(lobby.rounds, 9);
    }

    fn resumed(players: usize) -> LobbyState {
        let mut game = sim_board::Game::new(
            sim_board::Board::meadow(),
            sim_board::Rules {
                rounds: 9,
                ..Default::default()
            },
            players,
            3,
            &[sim_core::MinigameEntry::of::<Fake>()],
        );
        while let sim_board::Awaiting::Intent { seat, .. } = game.awaiting() {
            if game.between_turns() {
                break;
            }
            game.apply(seat, sim_board::Intent::Roll).unwrap();
        }
        let save = Save {
            game,
            // People played seats 1 and 3.
            seats: [2, 0, 3, 1].map(|character| sim_board::SavedSeat {
                character,
                human: character % 3 == 0,
            }),
            bots: Difficulty::Hard,
            pause_on_controller_lost: true,
        };
        LobbyState::resuming(4, &save)
    }

    #[test]
    fn resuming_offers_the_saved_seats_with_their_characters() {
        let mut lobby = resumed(4);
        assert_eq!(lobby.rounds, 9);
        assert_eq!(lobby.bots, Difficulty::Hard);
        assert!(lobby.pause_on_controller_lost);
        assert_eq!(lobby.roster(), [2, 0, 3, 1]);
        // People's seats first (1 and 2), then the bots'.
        for local in 0..3 {
            lobby.apply(ME, LobbyAction::Join { local });
        }
        assert_eq!(lobby.seat_of(ME, 0), Some(Seat(1)));
        assert_eq!(lobby.seat_of(ME, 1), Some(Seat(2)));
        assert_eq!(lobby.seat_of(ME, 2), Some(Seat(0)));
        assert_eq!(lobby.seats[2].unwrap().character, 3);
        // Left/right moves to the next free seat, and the character comes with it.
        assert!(lobby.apply(
            ME,
            LobbyAction::Pick {
                local: 1,
                next: true
            }
        ));
        assert_eq!(lobby.seat_of(ME, 1), Some(Seat(3)));
        assert_eq!(lobby.seats[3].unwrap().character, 1);
        assert_eq!(lobby.seats[2], None);
        assert!(!lobby.apply(ME, LobbyAction::Rounds { longer: true }));
        assert_eq!(lobby.rounds, 9);
        assert_eq!(lobby.roster(), [2, 0, 3, 1]);
    }

    #[test]
    fn resuming_a_smaller_game_keeps_its_empty_seats_empty() {
        let mut lobby = resumed(2);
        for local in 0..4 {
            lobby.apply(ME, LobbyAction::Join { local });
        }
        assert_eq!(lobby.seats.iter().flatten().count(), 2);
        assert!(!lobby.apply(
            ME,
            LobbyAction::Pick {
                local: 0,
                next: true
            }
        ));
    }

    /// The smallest minigame a board can be made with.
    struct Fake;

    impl sim_core::Minigame for Fake {
        type State = ();
        const ID: &'static str = "fake";
        const SUPPORTED_FORMATS: &'static [sim_core::FormatKind] =
            &[sim_core::FormatKind::FreeForAll];
        const MAX_TICKS: u32 = 1;
        fn new(_: u64, _: PlayerSetup, _: sim_core::Format) {}
        fn step(_: &mut (), _: &sim_core::Inputs) {}
        fn outcome(_: &()) -> Option<sim_core::Outcome> {
            None
        }
        fn bot_input(_: &(), _: Seat, _: Difficulty) -> sim_core::Input {
            sim_core::Input::default()
        }
    }

    #[test]
    fn a_leaving_machine_frees_all_its_seats() {
        let mut lobby = lobby();
        lobby.apply(THEM, LobbyAction::Join { local: 0 });
        lobby.apply(ME, LobbyAction::Join { local: 0 });
        lobby.apply(THEM, LobbyAction::Join { local: 1 });
        assert!(lobby.remove_peer(THEM));
        assert_eq!(lobby.seat_of(ME, 0), Some(Seat(1)));
        assert_eq!(lobby.seats.iter().flatten().count(), 1);
        assert!(!lobby.remove_peer(THEM));
    }
}
