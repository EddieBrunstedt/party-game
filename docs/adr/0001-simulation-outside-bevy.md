# 0001: Game simulation lives outside Bevy

**Status:** Accepted (2026-10-20)

## Context

Minigames use rollback netcode, so their simulation must be fully deterministic, cheap to snapshot and restore, and easy to checksum. We also want headless determinism tests and bot runs that Claude can execute without a GPU.

Running gameplay inside Bevy's ECS with `bevy_ggrs` works, but it comes with rules you can't see, and breaking one causes a silent desync:
- Query iteration order isn't guaranteed ([Query docs](https://docs.rs/bevy/latest/bevy/ecs/system/struct.Query.html)), so every system that changes state must sort.
- Every component has to be registered for snapshots. Forgetting one is still an open issue ([bevy_ggrs#123](https://github.com/gschup/bevy_ggrs/issues/123)).
- Entity IDs change on respawn and need remapping.
- Events and `Local<T>` aren't rolled back.
- Change detection, hooks and observers fire again after every restore ([bevy_ggrs#39](https://github.com/gschup/bevy_ggrs/issues/39)).
- Physics plugins hold cached state that isn't rolled back. The `bevy_ggrs` maintainer writes their own physics for this reason ([bevy_ggrs#110](https://github.com/gschup/bevy_ggrs/issues/110)).
- `bevy_ggrs` doesn't support Bevy 0.20 yet.

A separate `bevy_ecs` World per minigame doesn't fix this. A `World` can't be cloned ([bevy#16559](https://github.com/bevyengine/bevy/issues/16559)), and most of the problems above would remain.

## Decision

All game logic is plain Rust in crates that don't depend on Bevy (`sim-core`, `sim-board`, `minigames/*`):
- Each minigame implements a `Minigame` trait whose `State: Clone + Hash`, plus `step(&mut State, &[Input; 4])`.
- Bevy (`game` crate) only reads state to render, interpolate, animate and spawn effects.
- Presentation state (particles, tweens, camera shake) is never simulation state.

This is the pattern [GGPO's developer guide](https://github.com/pond3r/ggpo/blob/master/doc/DeveloperGuide.md) recommends. It's also what [ggrs's own example](https://github.com/gschup/ggrs/tree/main/examples/ex_game) does, and how Photon Quantum works: an isolated simulation decoupled from the engine.

## Consequences

- Snapshot = `clone()`, checksum = hash. A determinism test is a loop. Thousands of headless bot runs per second are possible.
- Bevy upgrades only touch the `game` crate.
- Some duplication between simulation state and the visual entities that mirror it, which is normal for rollback games.
- Bevy-ecosystem gameplay crates (physics, animation) can't be used in the simulation. See ADR 0006.
- Simulation code follows the rules in DESIGN.md §4.2: no HashMap iteration, no wall clock, seeded RNG kept in the state, transcendental math through `libm`.
