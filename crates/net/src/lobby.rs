//! The join screen's rules, shared by couch and online play: on the couch the only machine is
//! the host. Each machine numbers its own controllers (`local`), and a seat is claimed by a
//! machine's controller. The host's copy is the real one; clients send it [`LobbyAction`]s and
//! show what it sends back.

use serde::{Deserialize, Serialize};
use sim_core::{Difficulty, MAX_PLAYERS, Seat};

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
    /// Host only, like [`LobbyAction::Rounds`].
    CycleBots,
    Rounds {
        longer: bool,
    },
}

/// Board lengths the join screen offers.
pub const ROUND_CHOICES: [u32; 5] = [6, 9, 12, 15, 20];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LobbyState {
    pub seats: [Option<Claim>; MAX_PLAYERS],
    /// How well the bots in empty seats play.
    pub bots: Difficulty,
    pub rounds: u32,
    /// Free play (a minigame picker) instead of a board game.
    pub free_play: bool,
    /// How many characters there are to pick from.
    pub characters: u8,
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
            free_play,
            characters,
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
                let Some(seat) = Seat::ALL
                    .into_iter()
                    .find(|s| self.seats[s.index()].is_none())
                else {
                    return false;
                };
                let character = self.next_character(seat, self.characters - 1, true);
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
            LobbyAction::Rounds { longer } if peer == PeerId::HOST && !self.free_play => {
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
            LobbyAction::CycleBots | LobbyAction::Rounds { .. } => false,
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

    /// Characters per seat: players keep theirs, bots take whoever is left, in order.
    pub fn roster(&self) -> [u8; MAX_PLAYERS] {
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
        assert!(lobby.apply(ME, LobbyAction::CycleBots));
        assert_eq!(lobby.bots, Difficulty::Hard);
        assert!(lobby.apply(ME, LobbyAction::Rounds { longer: true }));
        assert_eq!(lobby.rounds, 15);
        assert!(lobby.apply(ME, LobbyAction::Rounds { longer: false }));
        assert!(lobby.apply(ME, LobbyAction::Rounds { longer: false }));
        assert_eq!(lobby.rounds, 9);
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
