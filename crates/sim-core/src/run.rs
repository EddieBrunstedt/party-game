use std::hash::{Hash, Hasher};

use crate::{Button, Format, Input, Inputs, MAX_PLAYERS, Minigame, Outcome, PlayerSetup, Seat};

/// Session-wide rules that every peer must agree on, so they live in the simulated state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RunConfig {
    /// When off, an unplugged controller's seat just goes neutral. When on, the game halts
    /// until it is back. Either way the game resumes where it left off.
    pub pause_on_controller_lost: bool,
}

/// One playthrough of a minigame, wrapping its state with what every minigame shares:
/// input edge tracking, pausing and controller-loss handling. This is the unit rollback
/// snapshots and checksums.
pub struct Run<M: Minigame> {
    pub game: M::State,
    pub players: PlayerSetup,
    pub config: RunConfig,
    /// Ticks of game time actually simulated (excludes time spent paused).
    pub tick: u32,
    pub paused: bool,
    previous: [Input; MAX_PLAYERS],
}

impl<M: Minigame> Run<M> {
    pub fn new(seed: u64, players: PlayerSetup, format: Format, config: RunConfig) -> Self {
        Self {
            game: M::new(seed, players, format),
            players,
            config,
            tick: 0,
            paused: false,
            previous: [Input::default(); MAX_PLAYERS],
        }
    }

    pub fn step(&mut self, mut current: [Input; MAX_PLAYERS]) {
        for seat in Seat::ALL {
            if !self.players.is_active(seat) {
                current[seat.index()] = Input::default();
            }
        }
        let inputs = Inputs {
            current,
            previous: self.previous,
        };

        if self
            .players
            .seats()
            .any(|seat| inputs.just_pressed(seat, Button::START))
        {
            self.paused = !self.paused;
        }
        self.previous = current;

        if !self.halted() {
            M::step(&mut self.game, &inputs);
            self.tick += 1;
        }
    }

    /// The inputs most recently fed to [`Run::step`], for presentation (e.g. controller badges).
    pub fn last_inputs(&self) -> &[Input; MAX_PLAYERS] {
        &self.previous
    }

    pub fn controller_lost(&self, seat: Seat) -> bool {
        self.previous[seat.index()].held(Button::CONTROLLER_LOST)
    }

    pub fn waiting_for_controller(&self) -> bool {
        self.config.pause_on_controller_lost
            && self.players.seats().any(|seat| self.controller_lost(seat))
    }

    pub fn halted(&self) -> bool {
        self.paused || self.waiting_for_controller()
    }

    pub fn outcome(&self) -> Option<Outcome> {
        M::outcome(&self.game)
    }
}

// Manual impls: deriving would wrongly require the marker type `M` itself to be Clone/Hash.
impl<M: Minigame> Clone for Run<M> {
    fn clone(&self) -> Self {
        Self {
            game: self.game.clone(),
            players: self.players,
            config: self.config,
            tick: self.tick,
            paused: self.paused,
            previous: self.previous,
        }
    }
}

impl<M: Minigame> Hash for Run<M> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.game.hash(state);
        self.players.hash(state);
        self.config.hash(state);
        self.tick.hash(state);
        self.paused.hash(state);
        self.previous.hash(state);
    }
}
