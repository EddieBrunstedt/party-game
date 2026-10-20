//! The board game's rules (DESIGN.md §3.1) as a host-authoritative state machine (ADR 0003).
//!
//! Clients send [`Intent`]s. The host feeds each one to [`Game::apply`], which checks that it's
//! that seat's decision, then resolves everything automatic (movement, landing, turn hand-over,
//! picking the minigame) up to the next decision, and returns [`Event`]s for the view to animate.
//! [`Game::awaiting`] says what has to happen next.

use serde::{Deserialize, Serialize};
use sim_core::{Format, FormatKind, MAX_PLAYERS, MinigameEntry, Outcome, PlayerSetup, Rng, Seat};

use crate::board::{Board, SpaceId, SpaceKind};

/// Dice roll 1 to this, both for turn order and for moving.
pub const DIE_FACES: u8 = 10;

/// The numbers of the ruleset. Every peer must agree on them, so they are part of the state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rules {
    /// Rounds in a game; each round is one turn per player, then a minigame.
    pub rounds: u32,
    pub start_coins: u32,
    pub star_price: u32,
    pub blue_coins: u32,
    pub red_coins: u32,
    /// Paid to every seat that places first in a minigame.
    pub minigame_prize: u32,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            rounds: 12,
            start_coins: 10,
            star_price: 20,
            blue_coins: 3,
            red_coins: 3,
            minigame_prize: 10,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Player {
    pub space: SpaceId,
    pub coins: u32,
    pub stars: u32,
}

/// What a client asks for. Only the seat being waited on may send one, and only the kind
/// [`Game::awaiting`] asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Intent {
    /// Hit the die, for turn order or for moving.
    Roll,
    /// At a fork: take the path at this index into the options of [`Decision::ChoosePath`].
    ChoosePath(u8),
    /// Answer a star offer.
    BuyStar(bool),
}

/// The decision a seat is being asked to make.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Decision {
    /// Roll once at the start of the game to decide turn order.
    TurnOrderRoll,
    /// Roll to move.
    Roll,
    /// Pick where to go from a fork; answer with [`Intent::ChoosePath`].
    ChoosePath { options: Vec<SpaceId> },
    /// Passing the Star with enough coins; answer with [`Intent::BuyStar`].
    BuyStar { price: u32 },
}

/// Everything the host needs to launch the minigame the board picked.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MinigamePlan {
    /// A [`MinigameEntry::id`].
    pub id: String,
    pub players: PlayerSetup,
    pub format: Format,
    pub seed: u64,
}

/// What the game is waiting for.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Awaiting {
    /// An [`Intent`] from this seat, of the kind `decision` asks for.
    Intent { seat: Seat, decision: Decision },
    /// The host to play this minigame and report its outcome with
    /// [`Game::apply_minigame_outcome`].
    Minigame(MinigamePlan),
    /// Nothing: the game is over.
    GameOver,
}

/// Why an intent or outcome was refused. A refused call changes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Rejected {
    GameOver,
    /// Someone else must decide, or the game is waiting for a minigame.
    NotYourTurn,
    /// The seat may act, but not with this kind of intent.
    WrongIntent,
    /// The path index is past the fork's options.
    NoSuchPath,
    /// An outcome arrived while no minigame was being played.
    NoMinigame,
    /// The outcome places inactive seats, misses active ones, or has no winner.
    InvalidOutcome,
}

impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Rejected::GameOver => "the game is over",
            Rejected::NotYourTurn => "not this seat's decision",
            Rejected::WrongIntent => "not the decision being asked for",
            Rejected::NoSuchPath => "no such path at this fork",
            Rejected::NoMinigame => "no minigame is being played",
            Rejected::InvalidOutcome => "the outcome doesn't match the players",
        })
    }
}

impl std::error::Error for Rejected {}

/// Something that happened, in order, for the view to present. Events carry enough to animate
/// without reading the state (ids, values, new totals).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Event {
    /// Turn-order rolls never tie: each one is drawn from the numbers nobody has rolled yet.
    TurnOrderRolled {
        seat: Seat,
        value: u8,
    },
    /// Highest roll first.
    TurnOrderDecided {
        order: Vec<Seat>,
    },
    RoundStarted {
        round: u32,
    },
    TurnStarted {
        seat: Seat,
    },
    Rolled {
        seat: Seat,
        value: u8,
    },
    /// One step along an arrow. `steps_left` is what remains of the roll after this step.
    Moved {
        seat: Seat,
        from: SpaceId,
        to: SpaceId,
        steps_left: u8,
    },
    /// Movement paused at a fork until the seat picks one of `options`.
    ReachedFork {
        seat: Seat,
        at: SpaceId,
        options: Vec<SpaceId>,
    },
    StarOffered {
        seat: Seat,
        space: SpaceId,
        price: u32,
    },
    /// `stars` is the seat's new total.
    StarBought {
        seat: Seat,
        space: SpaceId,
        stars: u32,
    },
    StarDeclined {
        seat: Seat,
    },
    StarCannotAfford {
        seat: Seat,
        price: u32,
        coins: u32,
    },
    StarMoved {
        from: SpaceId,
        to: SpaceId,
    },
    /// The roll is used up and the seat stops here.
    Landed {
        seat: Seat,
        space: SpaceId,
        kind: SpaceKind,
    },
    /// `delta` is what actually changed (coins stop at 0); `total` is the new balance.
    CoinsChanged {
        seat: Seat,
        delta: i32,
        total: u32,
    },
    TurnEnded {
        seat: Seat,
    },
    MinigameChosen {
        id: String,
        format: Format,
        seed: u64,
    },
    MinigameFinished {
        id: String,
        outcome: Outcome,
    },
    RoundEnded {
        round: u32,
    },
    /// Final placement: most stars, coins break ties; seats still tied share a placement.
    GameOver {
        placements: Outcome,
    },
}

/// Where the game is in its flow.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Phase {
    /// Seats roll for turn order, lowest seat first.
    TurnOrder {
        rolls: [Option<u8>; MAX_PLAYERS],
    },
    /// The current seat must roll.
    Roll,
    /// The current seat stands on a fork with `steps_left` still to walk.
    Fork {
        steps_left: u8,
    },
    /// The current seat was offered the Star, with `steps_left` still to walk afterwards.
    StarOffer {
        steps_left: u8,
    },
    Minigame(MinigamePlan),
    GameOver {
        placements: Outcome,
    },
}

/// A minigame the board may pick: the part of a [`MinigameEntry`] that belongs in saved state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct MinigameOption {
    id: String,
    formats: Vec<FormatKind>,
}

/// One board game. All of it is plain data: clone it to snapshot, serialize it to save or to
/// resync a reconnecting client, hash it ([`sim_core::checksum`]) to compare.
#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct Game {
    board: Board,
    rules: Rules,
    players: PlayerSetup,
    minigames: Vec<MinigameOption>,
    rng: Rng,
    /// 0 until turn order is decided, then 1-based.
    round: u32,
    order: Vec<Seat>,
    /// Index into `order` of the seat whose turn it is.
    turn: u8,
    seats: [Player; MAX_PLAYERS],
    star: SpaceId,
    phase: Phase,
    last_minigame: Option<String>,
}

impl Game {
    /// A new game for seats `0..player_count`, waiting for seat 0's turn-order roll.
    /// `minigames` is the pool to pick from; it must have at least one free-for-all minigame.
    pub fn new(
        board: Board,
        rules: Rules,
        player_count: usize,
        seed: u64,
        minigames: &[MinigameEntry],
    ) -> Self {
        assert!((2..=MAX_PLAYERS).contains(&player_count), "2-4 players");
        assert!(rules.rounds >= 1, "a game needs at least one round");
        assert!(
            minigames.iter().any(|m| m.supports(FormatKind::FreeForAll)),
            "the minigame pool needs a free-for-all minigame to fall back on"
        );
        let mut rng = Rng::new(seed);
        let spots = board.star_spots();
        let star = spots[rng.range_u32(0, spots.len() as u32 - 1) as usize];
        let player = Player {
            space: board.start(),
            coins: rules.start_coins,
            stars: 0,
        };
        Self {
            rules,
            players: PlayerSetup::first(player_count),
            minigames: minigames
                .iter()
                .map(|m| MinigameOption {
                    id: m.id.to_string(),
                    formats: m.formats.to_vec(),
                })
                .collect(),
            rng,
            round: 0,
            order: Vec::new(),
            turn: 0,
            seats: [player; MAX_PLAYERS],
            star,
            phase: Phase::TurnOrder {
                rolls: [None; MAX_PLAYERS],
            },
            last_minigame: None,
            board,
        }
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    pub fn players(&self) -> PlayerSetup {
        self.players
    }

    pub fn player(&self, seat: Seat) -> &Player {
        &self.seats[seat.index()]
    }

    /// The space the Star sits on.
    pub fn star(&self) -> SpaceId {
        self.star
    }

    /// The current round, 1-based; 0 while turn order is being rolled.
    pub fn round(&self) -> u32 {
        self.round
    }

    /// Seats in turn order; empty until every seat has rolled for it.
    pub fn turn_order(&self) -> &[Seat] {
        &self.order
    }

    /// The seat whose turn it is, if it's someone's turn.
    pub fn current_seat(&self) -> Option<Seat> {
        match self.phase {
            Phase::Roll | Phase::Fork { .. } | Phase::StarOffer { .. } => {
                Some(self.order[self.turn as usize])
            }
            _ => None,
        }
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    #[cfg(test)]
    pub(crate) fn set_star(&mut self, star: SpaceId) {
        self.star = star;
    }

    /// Placement right now: most stars, then most coins. Ties share a placement.
    pub fn standings(&self) -> Outcome {
        let score = |seat: Seat| (self.player(seat).stars, self.player(seat).coins);
        let mut placement = [None; MAX_PLAYERS];
        for seat in self.players.seats() {
            let better = self
                .players
                .seats()
                .filter(|other| score(*other) > score(seat))
                .count();
            placement[seat.index()] = Some(better as u8 + 1);
        }
        Outcome { placement }
    }

    pub fn awaiting(&self) -> Awaiting {
        let ask = |seat, decision| Awaiting::Intent { seat, decision };
        match &self.phase {
            Phase::TurnOrder { rolls } => {
                let seat = self
                    .players
                    .seats()
                    .find(|s| rolls[s.index()].is_none())
                    .expect("turn order is decided once everyone has rolled");
                ask(seat, Decision::TurnOrderRoll)
            }
            Phase::Roll => ask(self.order[self.turn as usize], Decision::Roll),
            Phase::Fork { .. } => {
                let seat = self.order[self.turn as usize];
                let here = self.player(seat).space;
                let options = self.board.space(here).next.clone();
                ask(seat, Decision::ChoosePath { options })
            }
            Phase::StarOffer { .. } => ask(
                self.order[self.turn as usize],
                Decision::BuyStar {
                    price: self.rules.star_price,
                },
            ),
            Phase::Minigame(plan) => Awaiting::Minigame(plan.clone()),
            Phase::GameOver { .. } => Awaiting::GameOver,
        }
    }

    /// Apply `seat`'s intent and resolve everything up to the next decision.
    pub fn apply(&mut self, seat: Seat, intent: Intent) -> Result<Vec<Event>, Rejected> {
        let decision = match self.awaiting() {
            Awaiting::GameOver => return Err(Rejected::GameOver),
            Awaiting::Minigame(_) => return Err(Rejected::NotYourTurn),
            Awaiting::Intent {
                seat: expected,
                decision,
            } if expected == seat => decision,
            Awaiting::Intent { .. } => return Err(Rejected::NotYourTurn),
        };
        let mut events = Vec::new();
        match (decision, intent) {
            (Decision::TurnOrderRoll, Intent::Roll) => self.roll_for_order(seat, &mut events),
            (Decision::Roll, Intent::Roll) => {
                let value = self.rng.range_u32(1, DIE_FACES as u32) as u8;
                events.push(Event::Rolled { seat, value });
                self.walk(seat, value, &mut events);
            }
            (Decision::ChoosePath { options }, Intent::ChoosePath(index)) => {
                let to = *options.get(index as usize).ok_or(Rejected::NoSuchPath)?;
                let Phase::Fork { steps_left } = self.phase else {
                    unreachable!("asked to choose a path outside a fork");
                };
                if !self.step(seat, to, steps_left - 1, &mut events) {
                    self.walk(seat, steps_left - 1, &mut events);
                }
            }
            (Decision::BuyStar { price }, Intent::BuyStar(buy)) => {
                let Phase::StarOffer { steps_left } = self.phase else {
                    unreachable!("asked about the Star outside an offer");
                };
                if buy {
                    let player = &mut self.seats[seat.index()];
                    player.stars += 1;
                    events.push(Event::StarBought {
                        seat,
                        space: self.star,
                        stars: player.stars,
                    });
                    self.change_coins(seat, -(price as i32), &mut events);
                    self.move_star(&mut events);
                } else {
                    events.push(Event::StarDeclined { seat });
                }
                self.walk(seat, steps_left, &mut events);
            }
            _ => return Err(Rejected::WrongIntent),
        }
        Ok(events)
    }

    /// Report how the minigame from [`Awaiting::Minigame`] went. Winners are paid, the round
    /// ends, and the next round starts or the game ends.
    pub fn apply_minigame_outcome(&mut self, outcome: Outcome) -> Result<Vec<Event>, Rejected> {
        let plan = match &self.phase {
            Phase::Minigame(plan) => plan.clone(),
            Phase::GameOver { .. } => return Err(Rejected::GameOver),
            _ => return Err(Rejected::NoMinigame),
        };
        let count = self.players.count() as u8;
        let valid = Seat::ALL.iter().all(|seat| {
            match (
                self.players.is_active(*seat),
                outcome.placement[seat.index()],
            ) {
                (true, Some(place)) => (1..=count).contains(&place),
                (false, None) => true,
                _ => false,
            }
        });
        if !valid || outcome.winners().next().is_none() {
            return Err(Rejected::InvalidOutcome);
        }

        let mut events = vec![Event::MinigameFinished {
            id: plan.id,
            outcome,
        }];
        for seat in outcome.winners() {
            self.change_coins(seat, self.rules.minigame_prize as i32, &mut events);
        }
        events.push(Event::RoundEnded { round: self.round });
        if self.round >= self.rules.rounds {
            let placements = self.standings();
            self.phase = Phase::GameOver { placements };
            events.push(Event::GameOver { placements });
        } else {
            self.start_round(&mut events);
        }
        Ok(events)
    }

    fn roll_for_order(&mut self, seat: Seat, events: &mut Vec<Event>) {
        let Phase::TurnOrder { rolls } = &mut self.phase else {
            unreachable!("turn-order roll outside turn order");
        };
        let free: Vec<u8> = (1..=DIE_FACES)
            .filter(|v| !rolls.contains(&Some(*v)))
            .collect();
        let value = free[self.rng.range_u32(0, free.len() as u32 - 1) as usize];
        rolls[seat.index()] = Some(value);
        events.push(Event::TurnOrderRolled { seat, value });

        let rolls = *rolls;
        if self.players.seats().all(|s| rolls[s.index()].is_some()) {
            let mut order: Vec<Seat> = self.players.seats().collect();
            order.sort_by_key(|s| std::cmp::Reverse(rolls[s.index()]));
            events.push(Event::TurnOrderDecided {
                order: order.clone(),
            });
            self.order = order;
            self.start_round(events);
        }
    }

    fn start_round(&mut self, events: &mut Vec<Event>) {
        self.round += 1;
        self.turn = 0;
        events.push(Event::RoundStarted { round: self.round });
        self.start_turn(events);
    }

    fn start_turn(&mut self, events: &mut Vec<Event>) {
        self.phase = Phase::Roll;
        events.push(Event::TurnStarted {
            seat: self.order[self.turn as usize],
        });
    }

    /// Move `seat` on until the roll is used up (then land and end the turn) or a decision
    /// interrupts (a fork or a star offer).
    fn walk(&mut self, seat: Seat, mut steps_left: u8, events: &mut Vec<Event>) {
        while steps_left > 0 {
            let here = self.player(seat).space;
            let next = &self.board.space(here).next;
            if next.len() > 1 {
                events.push(Event::ReachedFork {
                    seat,
                    at: here,
                    options: next.clone(),
                });
                self.phase = Phase::Fork { steps_left };
                return;
            }
            let to = next[0];
            steps_left -= 1;
            if self.step(seat, to, steps_left, events) {
                return;
            }
        }
        self.land(seat, events);
        self.end_turn(seat, events);
    }

    /// One step onto `to`. Returns true if movement pauses there for a star offer.
    fn step(&mut self, seat: Seat, to: SpaceId, steps_left: u8, events: &mut Vec<Event>) -> bool {
        let player = &mut self.seats[seat.index()];
        let from = player.space;
        player.space = to;
        events.push(Event::Moved {
            seat,
            from,
            to,
            steps_left,
        });
        if to != self.star {
            return false;
        }
        let (coins, price) = (player.coins, self.rules.star_price);
        if coins >= price {
            events.push(Event::StarOffered {
                seat,
                space: to,
                price,
            });
            self.phase = Phase::StarOffer { steps_left };
            true
        } else {
            events.push(Event::StarCannotAfford { seat, price, coins });
            false
        }
    }

    /// The Star jumps to a different random star spot.
    fn move_star(&mut self, events: &mut Vec<Event>) {
        let from = self.star;
        let spots: Vec<SpaceId> = self
            .board
            .star_spots()
            .iter()
            .copied()
            .filter(|s| *s != from)
            .collect();
        self.star = spots[self.rng.range_u32(0, spots.len() as u32 - 1) as usize];
        events.push(Event::StarMoved {
            from,
            to: self.star,
        });
    }

    fn land(&mut self, seat: Seat, events: &mut Vec<Event>) {
        let space = self.player(seat).space;
        let kind = self.board.space(space).kind;
        events.push(Event::Landed { seat, space, kind });
        let delta = match kind {
            SpaceKind::Blue => self.rules.blue_coins as i32,
            SpaceKind::Red => -(self.rules.red_coins as i32),
        };
        self.change_coins(seat, delta, events);
    }

    /// Add `delta` coins, stopping at 0. Emits an event only if the balance actually changed.
    fn change_coins(&mut self, seat: Seat, delta: i32, events: &mut Vec<Event>) {
        let player = &mut self.seats[seat.index()];
        let total = (player.coins as i64 + delta as i64).max(0) as u32;
        let delta = total as i64 - player.coins as i64;
        player.coins = total;
        if delta != 0 {
            events.push(Event::CoinsChanged {
                seat,
                delta: delta as i32,
                total,
            });
        }
    }

    fn end_turn(&mut self, seat: Seat, events: &mut Vec<Event>) {
        events.push(Event::TurnEnded { seat });
        self.turn += 1;
        if (self.turn as usize) < self.order.len() {
            self.start_turn(events);
        } else {
            self.start_minigame(events);
        }
    }

    /// The format from the colours players stand on (DESIGN.md §3.1). With 4 players: all one
    /// colour is a free-for-all, 3 and 1 is 1v3 with the odd one out solo, 2 and 2 is blue vs red.
    /// Fewer players always play free-for-all.
    pub fn format_from_spaces(&self) -> Format {
        if self.players.count() < MAX_PLAYERS {
            return Format::FreeForAll;
        }
        let blue =
            Seat::ALL.map(|s| self.board.space(self.player(s).space).kind == SpaceKind::Blue);
        let odd_one = |colour: bool| {
            Seat::ALL
                .into_iter()
                .find(|s| blue[s.index()] == colour)
                .expect("counted above")
        };
        match blue.iter().filter(|b| **b).count() {
            1 => Format::OneVsThree {
                solo: odd_one(true),
            },
            3 => Format::OneVsThree {
                solo: odd_one(false),
            },
            2 => Format::TwoVsTwo {
                team_of: blue.map(|b| if b { 0 } else { 1 }),
            },
            _ => Format::FreeForAll,
        }
    }

    /// Pick a random minigame that supports the format (falling back to free-for-all if none
    /// does), avoiding the previous one when there's another choice.
    fn start_minigame(&mut self, events: &mut Vec<Event>) {
        let mut format = self.format_from_spaces();
        let supports = |m: &MinigameOption, kind| m.formats.contains(&kind);
        if !self.minigames.iter().any(|m| supports(m, format.kind())) {
            format = Format::FreeForAll;
        }
        let mut candidates: Vec<&MinigameOption> = self
            .minigames
            .iter()
            .filter(|m| supports(m, format.kind()))
            .collect();
        if candidates.len() > 1 {
            candidates.retain(|m| Some(&m.id) != self.last_minigame.as_ref());
        }
        let pick = self.rng.range_u32(0, candidates.len() as u32 - 1) as usize;
        let id = candidates[pick].id.clone();
        let seed = (self.rng.next_u32() as u64) << 32 | self.rng.next_u32() as u64;

        events.push(Event::MinigameChosen {
            id: id.clone(),
            format,
            seed,
        });
        self.last_minigame = Some(id.clone());
        self.phase = Phase::Minigame(MinigamePlan {
            id,
            players: self.players,
            format,
            seed,
        });
    }
}

#[cfg(test)]
mod tests {
    use sim_core::checksum;

    use super::*;
    use crate::testing::{self, BOTH, SOLO_ONLY};

    fn new_game(players: usize, minigames: &[MinigameEntry]) -> Game {
        Game::new(testing::board(), Rules::default(), players, 7, minigames)
    }

    /// A game whose turn order is decided, waiting for the first turn's roll.
    fn started(players: usize, minigames: &[MinigameEntry]) -> Game {
        let mut game = new_game(players, minigames);
        while let Awaiting::Intent {
            seat,
            decision: Decision::TurnOrderRoll,
        } = game.awaiting()
        {
            game.apply(seat, Intent::Roll).unwrap();
        }
        game
    }

    /// Walk the current seat `steps` from `from` (bypassing the die), returning the events.
    fn walk_from(game: &mut Game, from: u16, steps: u8) -> (Seat, Vec<Event>) {
        let seat = game.current_seat().unwrap();
        game.seats[seat.index()].space = SpaceId(from);
        let mut events = Vec::new();
        game.walk(seat, steps, &mut events);
        (seat, events)
    }

    fn place(game: &mut Game, spaces: &[u16]) {
        for (seat, space) in spaces.iter().enumerate() {
            game.seats[seat].space = SpaceId(*space);
        }
    }

    fn outcome(placement: [Option<u8>; MAX_PLAYERS]) -> Outcome {
        Outcome { placement }
    }

    #[test]
    fn turn_order_rolls_never_tie_and_highest_goes_first() {
        let mut game = new_game(4, BOTH);
        let mut rolls = [0; MAX_PLAYERS];
        let mut events = Vec::new();
        for seat in Seat::ALL {
            assert_eq!(
                game.awaiting(),
                Awaiting::Intent {
                    seat,
                    decision: Decision::TurnOrderRoll
                }
            );
            events = game.apply(seat, Intent::Roll).unwrap();
            let Event::TurnOrderRolled { value, .. } = events[0] else {
                panic!("expected a turn-order roll, got {events:?}");
            };
            assert!(!rolls.contains(&value));
            rolls[seat.index()] = value;
        }
        let order = game.turn_order().to_vec();
        assert!(
            order
                .windows(2)
                .all(|w| rolls[w[0].index()] > rolls[w[1].index()])
        );
        assert_eq!(
            events[1..],
            [
                Event::TurnOrderDecided {
                    order: order.clone()
                },
                Event::RoundStarted { round: 1 },
                Event::TurnStarted { seat: order[0] },
            ]
        );
        assert_eq!(game.current_seat(), Some(order[0]));
    }

    #[test]
    fn out_of_turn_and_wrong_intents_change_nothing() {
        let mut game = new_game(2, BOTH);
        let before = checksum(&game);
        assert_eq!(
            game.apply(Seat(1), Intent::Roll),
            Err(Rejected::NotYourTurn)
        );
        assert_eq!(
            game.apply(Seat(3), Intent::Roll),
            Err(Rejected::NotYourTurn)
        );
        assert_eq!(
            game.apply(Seat(0), Intent::BuyStar(true)),
            Err(Rejected::WrongIntent)
        );
        assert_eq!(
            game.apply(Seat(0), Intent::ChoosePath(0)),
            Err(Rejected::WrongIntent)
        );
        assert_eq!(
            game.apply_minigame_outcome(outcome([Some(1), Some(2), None, None])),
            Err(Rejected::NoMinigame)
        );
        assert_eq!(checksum(&game), before);

        let mut game = started(2, BOTH);
        let other = game.turn_order()[1];
        assert_eq!(game.apply(other, Intent::Roll), Err(Rejected::NotYourTurn));
    }

    #[test]
    fn blue_gives_and_red_takes_coins() {
        let mut game = started(4, BOTH);
        game.star = SpaceId(4);
        let (seat, events) = walk_from(&mut game, 0, 1);
        assert_eq!(
            events,
            [
                Event::Moved {
                    seat,
                    from: SpaceId(0),
                    to: SpaceId(1),
                    steps_left: 0
                },
                Event::Landed {
                    seat,
                    space: SpaceId(1),
                    kind: SpaceKind::Red
                },
                Event::CoinsChanged {
                    seat,
                    delta: -3,
                    total: 7
                },
                Event::TurnEnded { seat },
                Event::TurnStarted {
                    seat: game.turn_order()[1]
                },
            ]
        );
        let (seat, events) = walk_from(&mut game, 3, 1);
        assert!(events.contains(&Event::CoinsChanged {
            seat,
            delta: 3,
            total: 13
        }));
    }

    #[test]
    fn coins_never_go_below_zero() {
        let mut game = started(4, BOTH);
        game.star = SpaceId(4);
        let seat = game.current_seat().unwrap();
        game.seats[seat.index()].coins = 2;
        let (_, events) = walk_from(&mut game, 0, 1);
        assert!(events.contains(&Event::CoinsChanged {
            seat,
            delta: -2,
            total: 0
        }));

        let seat = game.current_seat().unwrap();
        game.seats[seat.index()].coins = 0;
        let (_, events) = walk_from(&mut game, 0, 1);
        assert_eq!(game.player(seat).coins, 0);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::CoinsChanged { .. }))
        );
    }

    #[test]
    fn fork_waits_for_a_valid_choice() {
        let mut game = started(4, BOTH);
        game.star = SpaceId(1);
        game.seats.iter_mut().for_each(|p| p.coins = 0);
        let (seat, events) = walk_from(&mut game, 0, 4);
        assert!(events.contains(&Event::StarCannotAfford {
            seat,
            price: 20,
            coins: 0
        }));
        assert_eq!(
            events.last(),
            Some(&Event::ReachedFork {
                seat,
                at: SpaceId(2),
                options: vec![SpaceId(3), SpaceId(4)]
            })
        );
        assert_eq!(
            game.awaiting(),
            Awaiting::Intent {
                seat,
                decision: Decision::ChoosePath {
                    options: vec![SpaceId(3), SpaceId(4)]
                }
            }
        );
        assert_eq!(game.apply(seat, Intent::Roll), Err(Rejected::WrongIntent));
        assert_eq!(
            game.apply(seat, Intent::ChoosePath(2)),
            Err(Rejected::NoSuchPath)
        );

        let events = game.apply(seat, Intent::ChoosePath(1)).unwrap();
        assert_eq!(
            events[..3],
            [
                Event::Moved {
                    seat,
                    from: SpaceId(2),
                    to: SpaceId(4),
                    steps_left: 1
                },
                Event::Moved {
                    seat,
                    from: SpaceId(4),
                    to: SpaceId(5),
                    steps_left: 0
                },
                Event::Landed {
                    seat,
                    space: SpaceId(5),
                    kind: SpaceKind::Blue
                },
            ]
        );
    }

    #[test]
    fn stopping_on_a_fork_needs_no_choice() {
        let mut game = started(4, BOTH);
        game.star = SpaceId(4);
        let (seat, events) = walk_from(&mut game, 1, 1);
        assert_eq!(game.player(seat).space, SpaceId(2));
        assert!(events.contains(&Event::TurnEnded { seat }));
    }

    #[test]
    fn passing_the_star_buys_it_and_moves_it() {
        let mut game = started(4, BOTH);
        game.star = SpaceId(1);
        let seat = game.current_seat().unwrap();
        game.seats[seat.index()].coins = 25;
        let (_, events) = walk_from(&mut game, 0, 2);
        assert_eq!(
            events.last(),
            Some(&Event::StarOffered {
                seat,
                space: SpaceId(1),
                price: 20
            })
        );
        assert_eq!(
            game.awaiting(),
            Awaiting::Intent {
                seat,
                decision: Decision::BuyStar { price: 20 }
            }
        );

        let events = game.apply(seat, Intent::BuyStar(true)).unwrap();
        let moved_to = game.star();
        assert_ne!(moved_to, SpaceId(1));
        assert!(game.board().star_spots().contains(&moved_to));
        assert_eq!(
            events[..5],
            [
                Event::StarBought {
                    seat,
                    space: SpaceId(1),
                    stars: 1
                },
                Event::CoinsChanged {
                    seat,
                    delta: -20,
                    total: 5
                },
                Event::StarMoved {
                    from: SpaceId(1),
                    to: moved_to
                },
                Event::Moved {
                    seat,
                    from: SpaceId(1),
                    to: SpaceId(2),
                    steps_left: 0
                },
                Event::Landed {
                    seat,
                    space: SpaceId(2),
                    kind: SpaceKind::Blue
                },
            ]
        );
        assert_eq!(game.player(seat).stars, 1);
        assert_eq!(game.player(seat).coins, 8);
    }

    #[test]
    fn declining_the_star_leaves_it() {
        let mut game = started(4, BOTH);
        game.star = SpaceId(1);
        let seat = game.current_seat().unwrap();
        game.seats[seat.index()].coins = 20;
        walk_from(&mut game, 0, 2);
        let events = game.apply(seat, Intent::BuyStar(false)).unwrap();
        assert_eq!(events[0], Event::StarDeclined { seat });
        assert_eq!(game.star(), SpaceId(1));
        assert_eq!(game.player(seat).stars, 0);
        assert_eq!(game.player(seat).coins, 23);
    }

    #[test]
    fn format_comes_from_space_colours() {
        // Spaces 0, 2, 3, 5 are blue; 1 and 4 are red.
        let mut game = started(4, BOTH);
        place(&mut game, &[0, 2, 3, 5]);
        assert_eq!(game.format_from_spaces(), Format::FreeForAll);
        place(&mut game, &[1, 4, 1, 4]);
        assert_eq!(game.format_from_spaces(), Format::FreeForAll);
        place(&mut game, &[0, 2, 4, 5]);
        assert_eq!(
            game.format_from_spaces(),
            Format::OneVsThree { solo: Seat(2) }
        );
        place(&mut game, &[1, 0, 4, 1]);
        assert_eq!(
            game.format_from_spaces(),
            Format::OneVsThree { solo: Seat(1) }
        );
        place(&mut game, &[0, 1, 2, 4]);
        assert_eq!(
            game.format_from_spaces(),
            Format::TwoVsTwo {
                team_of: [0, 1, 0, 1]
            }
        );

        let mut game = started(3, BOTH);
        place(&mut game, &[0, 1, 4]);
        assert_eq!(game.format_from_spaces(), Format::FreeForAll);
    }

    #[test]
    fn minigame_supports_the_format_or_falls_back_to_free_for_all() {
        let mut game = started(4, BOTH);
        place(&mut game, &[0, 2, 4, 5]);
        game.start_minigame(&mut Vec::new());
        let Awaiting::Minigame(plan) = game.awaiting() else {
            panic!("expected a minigame");
        };
        assert_eq!(plan.id, "teams");
        assert_eq!(plan.format, Format::OneVsThree { solo: Seat(2) });
        assert_eq!(plan.players, PlayerSetup::first(4));

        let mut game = started(4, SOLO_ONLY);
        place(&mut game, &[0, 2, 4, 5]);
        let mut events = Vec::new();
        game.start_minigame(&mut events);
        let Event::MinigameChosen { id, format, .. } = &events[0] else {
            panic!("expected a minigame, got {events:?}");
        };
        assert_eq!((id.as_str(), *format), ("solo", Format::FreeForAll));
    }

    #[test]
    fn minigames_dont_repeat_when_there_is_a_choice() {
        let mut game = started(4, BOTH);
        let mut last = String::new();
        for _ in 0..10 {
            game.start_minigame(&mut Vec::new());
            let Phase::Minigame(plan) = game.phase() else {
                panic!("expected a minigame");
            };
            assert_ne!(plan.id, last);
            last = plan.id.clone();
        }

        // With only one candidate it simply repeats.
        let mut game = started(4, SOLO_ONLY);
        for _ in 0..2 {
            game.start_minigame(&mut Vec::new());
            assert!(matches!(game.phase(), Phase::Minigame(plan) if plan.id == "solo"));
        }
    }

    #[test]
    fn minigame_winners_get_the_prize_and_the_next_round_starts() {
        let mut game = started(4, BOTH);
        game.start_minigame(&mut Vec::new());
        let before = Seat::ALL.map(|s| game.player(s).coins);
        let result = outcome([Some(1), Some(3), Some(1), Some(4)]);
        let events = game.apply_minigame_outcome(result).unwrap();
        assert!(
            matches!(&events[0], Event::MinigameFinished { outcome, .. } if *outcome == result)
        );
        assert_eq!(
            events[1..],
            [
                Event::CoinsChanged {
                    seat: Seat(0),
                    delta: 10,
                    total: before[0] + 10
                },
                Event::CoinsChanged {
                    seat: Seat(2),
                    delta: 10,
                    total: before[2] + 10
                },
                Event::RoundEnded { round: 1 },
                Event::RoundStarted { round: 2 },
                Event::TurnStarted {
                    seat: game.turn_order()[0]
                },
            ]
        );
        assert_eq!(game.player(Seat(1)).coins, before[1]);
    }

    #[test]
    fn outcomes_must_match_the_players() {
        let mut game = started(3, BOTH);
        game.start_minigame(&mut Vec::new());
        for bad in [
            [Some(1), Some(2), None, None],
            [Some(1), Some(2), Some(3), Some(4)],
            [Some(2), Some(2), Some(3), None],
            [Some(1), Some(2), Some(9), None],
        ] {
            assert_eq!(
                game.apply_minigame_outcome(outcome(bad)),
                Err(Rejected::InvalidOutcome)
            );
        }
        let good = outcome([Some(1), Some(2), Some(3), None]);
        assert!(game.apply_minigame_outcome(good).is_ok());
    }

    #[test]
    fn most_stars_win_and_coins_break_ties() {
        let rules = Rules {
            rounds: 1,
            ..Rules::default()
        };
        let mut game = Game::new(testing::board(), rules, 4, 1, BOTH);
        while let Awaiting::Intent { seat, decision } = game.awaiting() {
            let intent = match decision {
                Decision::ChoosePath { .. } => Intent::ChoosePath(0),
                Decision::BuyStar { .. } => Intent::BuyStar(false),
                _ => Intent::Roll,
            };
            game.apply(seat, intent).unwrap();
        }
        let standings = [(1, 5), (1, 30), (0, 99), (1, 5)];
        for (seat, (stars, coins)) in standings.into_iter().enumerate() {
            game.seats[seat].stars = stars;
            game.seats[seat].coins = coins;
        }
        let events = game
            .apply_minigame_outcome(outcome([Some(1); MAX_PLAYERS]))
            .unwrap();
        let placements = outcome([Some(2), Some(1), Some(4), Some(2)]);
        assert_eq!(events.last(), Some(&Event::GameOver { placements }));
        assert_eq!(game.awaiting(), Awaiting::GameOver);
        assert_eq!(game.apply(Seat(0), Intent::Roll), Err(Rejected::GameOver));
        assert_eq!(
            game.apply_minigame_outcome(placements),
            Err(Rejected::GameOver)
        );
    }

    #[test]
    fn saved_games_resume_identically() {
        let mut game = started(4, BOTH);
        let seat = game.current_seat().unwrap();
        let saved = ron::to_string(&game).unwrap();
        let mut loaded: Game = ron::from_str(&saved).unwrap();
        assert_eq!(checksum(&loaded), checksum(&game));
        assert_eq!(
            loaded.apply(seat, Intent::Roll),
            game.apply(seat, Intent::Roll)
        );
        assert_eq!(checksum(&loaded), checksum(&game));
    }
}
