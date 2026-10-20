//! The automated quality bars every minigame must pass (DESIGN.md §5):
//! deterministic replay, rollback consistency under `ggrs`, and headless bot runs.

use std::marker::PhantomData;

use ggrs::{GgrsRequest, PredictRepeatLast, SessionBuilder};
use sim_core::{
    Button, Difficulty, Format, Input, MAX_PLAYERS, Minigame, Outcome, PlayerSetup, Rng, Run,
    RunConfig, Seat, checksum,
};

/// Where a recorded run's inputs come from.
#[derive(Debug, Clone, Copy)]
pub enum Driver {
    /// Every seat is a bot of this difficulty.
    Bots(Difficulty),
    /// Random mashing (including occasional pauses and controller losses) from this seed.
    Fuzz { seed: u64 },
}

#[derive(Debug, Clone, Copy)]
pub struct Setup {
    pub seed: u64,
    pub players: PlayerSetup,
    pub format: Format,
    pub config: RunConfig,
}

impl Setup {
    pub fn new(seed: u64, players: usize) -> Self {
        Self {
            seed,
            players: PlayerSetup::first(players),
            format: Format::FreeForAll,
            config: RunConfig::default(),
        }
    }

    pub fn start<M: Minigame>(&self) -> Run<M> {
        Run::new(self.seed, self.players, self.format, self.config)
    }
}

pub struct Recording {
    pub inputs: Vec<[Input; MAX_PLAYERS]>,
    /// Checksum of the state after each tick.
    pub checksums: Vec<u64>,
    pub outcome: Option<Outcome>,
}

pub fn bot_inputs<M: Minigame>(run: &Run<M>, difficulty: Difficulty) -> [Input; MAX_PLAYERS] {
    Seat::ALL.map(|seat| M::bot_input(&run.game, seat, difficulty))
}

fn fuzz_inputs(rng: &mut Rng) -> [Input; MAX_PLAYERS] {
    [(); MAX_PLAYERS].map(|_| {
        let mut input = Input {
            buttons: 0,
            stick_x: rng.range_u32(0, 254) as i8,
            stick_y: rng.range_u32(0, 254) as i8,
        };
        input.set(Button::A, rng.chance(30));
        input.set(Button::B, rng.chance(20));
        input.set(Button::X, rng.chance(10));
        input.set(Button::START, rng.chance(1));
        input.set(Button::CONTROLLER_LOST, rng.chance(1));
        input
    })
}

/// Run until the minigame ends (or `max_ticks` steps), recording inputs and checksums.
pub fn record<M: Minigame>(setup: &Setup, driver: Driver, max_ticks: u32) -> Recording {
    let mut run = setup.start::<M>();
    let mut fuzz = match driver {
        Driver::Fuzz { seed } => Some(Rng::new(seed)),
        Driver::Bots(_) => None,
    };
    let mut recording = Recording {
        inputs: Vec::new(),
        checksums: Vec::new(),
        outcome: None,
    };
    for _ in 0..max_ticks {
        let inputs = match (&mut fuzz, driver) {
            (Some(rng), _) => fuzz_inputs(rng),
            (None, Driver::Bots(difficulty)) => bot_inputs(&run, difficulty),
            (None, Driver::Fuzz { .. }) => unreachable!(),
        };
        run.step(inputs);
        recording.inputs.push(inputs);
        recording.checksums.push(checksum(&run));
        if let Some(outcome) = run.outcome() {
            recording.outcome = Some(outcome);
            break;
        }
    }
    recording
}

/// Bar 1a: replaying the same inputs from the same seed must produce identical state every tick.
pub fn check_replay<M: Minigame>(setup: &Setup, recording: &Recording) -> Result<(), String> {
    let mut run = setup.start::<M>();
    for (tick, (inputs, expected)) in recording
        .inputs
        .iter()
        .zip(&recording.checksums)
        .enumerate()
    {
        run.step(*inputs);
        let actual = checksum(&run);
        if actual != *expected {
            return Err(format!(
                "{}: replay diverged at tick {tick} (expected {expected:016x}, got {actual:016x})",
                M::ID
            ));
        }
    }
    Ok(())
}

struct Config<M>(PhantomData<fn() -> M>);

impl<M: Minigame> ggrs::Config for Config<M> {
    type Input = Input;
    type InputPredictor = PredictRepeatLast;
    type State = Run<M>;
    type Address = usize;
}

/// Bar 1b: `ggrs`' sync test rolls back `check_distance` ticks every frame, resimulates, and
/// fails if the resimulated checksums differ. This catches state that escapes snapshots.
pub fn check_rollback<M: Minigame>(
    setup: &Setup,
    recording: &Recording,
    check_distance: usize,
) -> Result<(), String> {
    let mut session = SessionBuilder::<Config<M>>::new()
        .with_num_players(MAX_PLAYERS)
        .map(|b| b.with_check_distance(check_distance))
        .and_then(|b| b.start_synctest_session())
        .map_err(|e| format!("{}: could not start sync test: {e}", M::ID))?;
    let mut run = setup.start::<M>();

    for inputs in &recording.inputs {
        for (handle, input) in inputs.iter().enumerate() {
            session
                .add_local_input(handle, *input)
                .map_err(|e| e.to_string())?;
        }
        let requests = session
            .advance_frame()
            .map_err(|e| format!("{}: rollback mismatch: {e}", M::ID))?;
        for request in requests {
            match request {
                GgrsRequest::SaveGameState { cell, frame } => {
                    let sum = checksum(&run) as u128;
                    cell.save(frame, Some(run.clone()), Some(sum));
                }
                GgrsRequest::LoadGameState { cell, .. } => {
                    run = cell
                        .load()
                        .expect("ggrs asked to load a state it never saved");
                }
                GgrsRequest::AdvanceFrame { inputs } => {
                    run.step(std::array::from_fn(|i| inputs[i].0));
                }
            }
        }
    }
    Ok(())
}

/// Bar 2: bots alone must finish the minigame without panicking.
pub fn check_bots_finish<M: Minigame>(
    setup: &Setup,
    difficulty: Difficulty,
) -> Result<Recording, String> {
    let recording = record::<M>(setup, Driver::Bots(difficulty), M::MAX_TICKS);
    match recording.outcome {
        Some(_) => Ok(recording),
        None => Err(format!(
            "{}: {difficulty:?} bots did not finish within {} ticks (seed {})",
            M::ID,
            M::MAX_TICKS,
            setup.seed
        )),
    }
}

/// Every automated bar, over a spread of seeds, player counts and drivers.
/// Minigame crates call this from a test; `tools check` calls it from the CLI.
pub fn check_all<M: Minigame>(seeds: u64) -> Result<(), String> {
    for seed in 0..seeds {
        for players in 2..=MAX_PLAYERS {
            let setup = Setup::new(seed, players);
            for difficulty in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard] {
                let recording = check_bots_finish::<M>(&setup, difficulty)?;
                check_replay::<M>(&setup, &recording)?;
            }
            let fuzzed = record::<M>(&setup, Driver::Fuzz { seed }, 2_000);
            check_replay::<M>(&setup, &fuzzed)?;
            check_rollback::<M>(&setup, &fuzzed, 7)?;

            let mut strict = setup;
            strict.config.pause_on_controller_lost = true;
            let fuzzed = record::<M>(&strict, Driver::Fuzz { seed }, 2_000);
            check_rollback::<M>(&strict, &fuzzed, 7)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use sim_core::{FormatKind, Inputs};

    use super::*;

    /// A minigame that cheats determinism by reading state outside its snapshot.
    struct Leaky;

    static OUTSIDE_STATE: AtomicU32 = AtomicU32::new(0);

    impl Minigame for Leaky {
        type State = u32;
        const ID: &'static str = "leaky";
        const SUPPORTED_FORMATS: &'static [FormatKind] = &[FormatKind::FreeForAll];
        const MAX_TICKS: u32 = 100;

        fn new(_seed: u64, _players: PlayerSetup, _format: Format) -> u32 {
            0
        }
        fn step(state: &mut u32, _inputs: &Inputs) {
            *state += OUTSIDE_STATE.fetch_add(1, Ordering::Relaxed);
        }
        fn outcome(_state: &u32) -> Option<Outcome> {
            None
        }
        fn bot_input(_state: &u32, _seat: Seat, _difficulty: Difficulty) -> Input {
            Input::default()
        }
    }

    #[test]
    fn replay_and_rollback_checks_catch_leaked_state() {
        let setup = Setup::new(0, 2);
        let recording = record::<Leaky>(&setup, Driver::Fuzz { seed: 0 }, 50);
        assert!(check_replay::<Leaky>(&setup, &recording).is_err());
        assert!(check_rollback::<Leaky>(&setup, &recording, 7).is_err());
    }
}
