//! Drives any minigame's [`Run`] through a `ggrs` P2P session (ADR 0002): save is a clone,
//! load restores it, advance is [`Run::step`], and the checksum is the run's stable hash.
//!
//! Every seat has a handle, active or not. A machine owns its local players' seats; the host
//! also owns the bots' seats (it computes their inputs like any controller's) and the empty ones.

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::marker::PhantomData;

use ggrs::{
    DesyncDetection, GgrsError, GgrsEvent, GgrsRequest, P2PSession, PlayerType, PredictRepeatLast,
    SessionBuilder, SessionState,
};
use sim_core::{Input, MAX_PLAYERS, Minigame, Outcome, Run, Seat, TICK_HZ, checksum};

use crate::{Endpoint, Transport};

/// Ticks between pressing a button and the simulation seeing it, on every machine alike.
pub const INPUT_DELAY: usize = 2;
/// The furthest a machine simulates ahead of the last inputs it has from everyone.
pub const MAX_PREDICTION: usize = 8;
/// How often (in ticks) peers compare checksums of confirmed ticks.
const DESYNC_INTERVAL: u32 = 30;

/// Who sends a seat's input, as seen from this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeatOwner<A> {
    /// This machine: one of its players, a bot it runs, or an empty seat it fills.
    Local,
    Remote(A),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RollbackEvent<A> {
    Synchronized(A),
    /// Nothing heard from this peer for a while; the session waits for it.
    Interrupted(A),
    Resumed(A),
    /// The peer timed out and its seats now send neutral input.
    Disconnected(A),
    /// This peer computed a different state for a confirmed tick: a determinism bug.
    Desync {
        tick: u32,
        peer: A,
    },
}

/// What one [`Rollback::update`] did. Both can happen at once: late input arrives, the
/// rollback replays the ticks it changed, then the new tick is simulated on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Advance {
    /// A new tick was simulated. Not while synchronizing, nor while this machine is so far
    /// ahead of a peer's input that it has to wait for it.
    pub new_tick: bool,
    /// Ticks replayed because a rollback undid them.
    pub resimulated: u32,
}

struct Config<M, A>(PhantomData<fn() -> (M, A)>);

impl<M: Minigame, A> ggrs::Config for Config<M, A>
where
    A: Copy + Ord + Hash + Debug + Send + Sync + 'static,
{
    type Input = Input;
    type InputPredictor = PredictRepeatLast;
    type State = Run<M>;
    type Address = A;
}

pub struct Rollback<M: Minigame, A>
where
    A: Copy + Ord + Hash + Debug + Send + Sync + 'static,
{
    session: P2PSession<Config<M, A>>,
    run: Run<M>,
    /// The ggrs frame `run` is at: how many ticks of input it has taken in.
    frame: i32,
    local: Vec<Seat>,
    /// Waiting ticks `ggrs` recommended, so a machine running ahead lets the others catch up.
    skip: u32,
    /// Checksum and outcome after each tick that isn't confirmed yet; a rollback rewrites them.
    unconfirmed: BTreeMap<i32, (u64, Option<Outcome>)>,
    confirmed: Vec<(u32, u64)>,
    outcome: Option<(u32, Outcome)>,
    events: Vec<RollbackEvent<A>>,
    /// Each handle's remote address, if it's remote.
    remotes: [Option<A>; MAX_PLAYERS],
}

impl<M: Minigame, A> Rollback<M, A>
where
    A: Copy + Ord + Hash + Debug + Send + Sync + 'static,
{
    /// Starts a session from `run` (the same seed, players and format on every machine) over
    /// `endpoint`, which must already be connected to every remote owner in `seats`.
    pub fn start<T: Transport<Addr = A>>(
        run: Run<M>,
        seats: [SeatOwner<A>; MAX_PLAYERS],
        endpoint: &Endpoint<T>,
    ) -> Result<Self, GgrsError> {
        let mut builder = SessionBuilder::<Config<M, A>>::new()
            .with_num_players(MAX_PLAYERS)?
            .with_fps(TICK_HZ as usize)?
            .with_input_delay(INPUT_DELAY)
            .with_max_prediction_window(MAX_PREDICTION)
            .with_desync_detection_mode(DesyncDetection::On {
                interval: DESYNC_INTERVAL,
            });
        for (handle, owner) in seats.iter().enumerate() {
            let player = match *owner {
                SeatOwner::Local => PlayerType::Local,
                SeatOwner::Remote(addr) => PlayerType::Remote(addr),
            };
            builder = builder.add_player(player, handle)?;
        }
        let local = Seat::ALL
            .into_iter()
            .filter(|seat| seats[seat.index()] == SeatOwner::Local)
            .collect();
        Ok(Self {
            session: builder.start_p2p_session(endpoint.ggrs_socket())?,
            run,
            frame: 0,
            local,
            skip: 0,
            unconfirmed: BTreeMap::new(),
            confirmed: Vec::new(),
            outcome: None,
            events: Vec::new(),
            remotes: seats.map(|owner| match owner {
                SeatOwner::Local => None,
                SeatOwner::Remote(addr) => Some(addr),
            }),
        })
    }

    /// The newest simulated state, which may include predicted input from remote seats.
    pub fn run(&self) -> &Run<M> {
        &self.run
    }

    /// Ticks simulated in the newest state. Local input given to the next [`Rollback::update`]
    /// is for this tick, and takes effect [`INPUT_DELAY`] ticks later on every machine.
    pub fn tick(&self) -> u32 {
        self.frame as u32
    }

    /// The seats this machine sends input for.
    pub fn local_seats(&self) -> &[Seat] {
        &self.local
    }

    pub fn is_running(&self) -> bool {
        self.session.current_state() == SessionState::Running
    }

    /// Call once per tick: exchanges packets and, once synchronized, simulates the next tick
    /// with `local_input` for each of this machine's seats (given the newest state, which is
    /// what bots read).
    pub fn update(&mut self, mut local_input: impl FnMut(Seat, &Run<M>) -> Input) -> Advance {
        self.session.poll_remote_clients();
        self.collect_events();
        if !self.is_running() {
            return Advance::default();
        }
        if self.skip > 0 {
            self.skip -= 1;
            return Advance::default();
        }
        for &seat in &self.local {
            let input = local_input(seat, &self.run);
            self.session
                .add_local_input(seat.index(), input)
                .expect("every local seat has a handle, and the session is running");
        }
        let requests = match self.session.advance_frame() {
            Ok(requests) => requests,
            Err(GgrsError::PredictionThreshold | GgrsError::NotSynchronized) => {
                return Advance::default();
            }
            Err(err) => panic!("ggrs refused to advance: {err}"),
        };

        let start = self.frame;
        let mut stepped = 0;
        for request in requests {
            match request {
                GgrsRequest::SaveGameState { cell, frame } => {
                    debug_assert_eq!(frame, self.frame);
                    let sum = checksum(&self.run) as u128;
                    cell.save(frame, Some(self.run.clone()), Some(sum));
                }
                GgrsRequest::LoadGameState { cell, frame } => {
                    self.run = cell.load().expect("ggrs only loads frames it saved");
                    self.frame = frame;
                }
                GgrsRequest::AdvanceFrame { inputs } => {
                    self.run.step(std::array::from_fn(|i| inputs[i].0));
                    let entry = (checksum(&self.run), self.run.outcome());
                    self.unconfirmed.insert(self.frame, entry);
                    self.frame += 1;
                    stepped += 1;
                }
            }
        }
        self.confirm();
        let new_tick = self.frame > start;
        debug_assert!(
            self.frame <= start + 1,
            "ggrs simulates at most one new tick per update"
        );
        Advance {
            new_tick,
            resimulated: stepped - new_tick as u32,
        }
    }

    /// Ticks no rollback can change any more, with the run's checksum after each, oldest first.
    /// Every machine reports the same pairs; comparing them is the determinism check.
    pub fn take_confirmed(&mut self) -> Vec<(u32, u64)> {
        std::mem::take(&mut self.confirmed)
    }

    /// The outcome and the tick it first appeared, once that tick is confirmed. Identical on
    /// every machine, unlike `run().outcome()`, which a rollback can still take back.
    pub fn confirmed_outcome(&self) -> Option<(u32, Outcome)> {
        self.outcome
    }

    /// Round-trip time to each remote machine, in milliseconds, once `ggrs` has measured it.
    pub fn pings(&self) -> Vec<(A, u128)> {
        let mut pings: Vec<(A, u128)> = Vec::new();
        for handle in self.session.remote_player_handles() {
            let Ok(stats) = self.session.network_stats(handle) else {
                continue;
            };
            let addr = self.session_addr(handle);
            if let Some(addr) = addr.filter(|a| !pings.iter().any(|(p, _)| p == a)) {
                pings.push((addr, stats.ping));
            }
        }
        pings
    }

    fn session_addr(&self, handle: usize) -> Option<A> {
        self.remotes.get(handle).copied().flatten()
    }

    pub fn events(&mut self) -> Vec<RollbackEvent<A>> {
        std::mem::take(&mut self.events)
    }

    fn confirm(&mut self) {
        let confirmed = self.session.confirmed_frame();
        while let Some(entry) = self.unconfirmed.first_entry() {
            if *entry.key() > confirmed {
                break;
            }
            let (frame, (sum, outcome)) = entry.remove_entry();
            let tick = frame as u32;
            self.confirmed.push((tick, sum));
            if let (None, Some(outcome)) = (self.outcome, outcome) {
                self.outcome = Some((tick, outcome));
            }
        }
    }

    fn collect_events(&mut self) {
        for event in self.session.events() {
            let event = match event {
                GgrsEvent::Synchronizing { .. } => continue,
                GgrsEvent::Synchronized { addr } => RollbackEvent::Synchronized(addr),
                GgrsEvent::NetworkInterrupted { addr, .. } => RollbackEvent::Interrupted(addr),
                GgrsEvent::NetworkResumed { addr } => RollbackEvent::Resumed(addr),
                GgrsEvent::Disconnected { addr } => RollbackEvent::Disconnected(addr),
                GgrsEvent::WaitRecommendation { skip_frames } => {
                    self.skip += skip_frames;
                    continue;
                }
                GgrsEvent::DesyncDetected { frame, addr, .. } => RollbackEvent::Desync {
                    tick: frame as u32,
                    peer: addr,
                },
            };
            self.events.push(event);
        }
    }
}
