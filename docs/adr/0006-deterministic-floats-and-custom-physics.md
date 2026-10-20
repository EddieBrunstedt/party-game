# 0006: Floats with `libm`, and custom minigame physics

**Status:** Accepted (2026-10-20)

## Context

Rollback needs bit-identical simulation on every peer. All targets (Windows, Linux, Steam Deck) are x86_64. On x86_64, basic IEEE float operations are deterministic, and Rust doesn't silently fuse operations into FMA. Transcendental functions (sin, cos, pow, …) differ between platform math libraries. Full physics engines carry cached solver state and have open determinism issues (e.g. [avian#1069](https://github.com/avianphysics/avian/issues/1069)).

## Decision

- Use `f32` in the simulation. All transcendental math goes through a wrapper in `sim-core` backed by `libm`. Never build with `target-cpu=native`.
- Minigames use **simple custom physics** in `sim-core`: circles, boxes, heightfields, gravity, knockback.
- A minigame that really needs rigid-body physics may use `avian3d` with `enhanced-determinism`, behind the same `Minigame` trait. It must pass the same determinism test.
- Keep simulation and rendering features separate in Cargo, so determinism features (e.g. glam `libm`) don't spread into Bevy through feature unification ([bevy#26087](https://github.com/bevyengine/bevy/issues/26087)).

## Consequences

- Small simulation state, which means cheap snapshots and simple reasoning.
- If a non-x86 platform is ever added (macOS ARM), check determinism across architectures again. The math wrapper allows switching to fixed-point (`fixed` crate) without touching minigame code.
