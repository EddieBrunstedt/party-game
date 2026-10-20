# 0002: Rollback with the `ggrs` core, driven directly

**Status:** Accepted (2026-10-20)

## Context

ADR 0001 puts minigame simulations in plain Rust. We need rollback for 2–4 peers. The candidates are `bevy_ggrs`, `lightyear`, or `ggrs` used directly.

- `bevy_ggrs` rolls back Bevy entities, which we don't have. It also lags behind Bevy releases: it's on 0.19 while Bevy is on 0.20.
- `lightyear` is a full networking framework with its own replication model. It would compete with our design and ties us to its Bevy release schedule.
- `ggrs` 0.13 doesn't depend on Bevy and is actively maintained. Its API is a small set of requests: save, load, advance.

## Decision

Use `ggrs` directly. One generic adapter in `net` drives any `Minigame`:
- `SaveGameState` → `clone` + hash
- `LoadGameState` → restore
- `AdvanceFrame` → `step`

Defaults: 60 Hz, 2 frames of input delay, at most 8 frames of rollback. Couch play goes through the same path, with every seat local and no rollback.

`ggrs` `SyncTestSession` plus our own two-peer simulation in `tools` provide the per-frame checksum test that every minigame must pass.

## Consequences

- One code path for couch and online play.
- We write the small `NonBlockingSocket` adapter for each transport ourselves (ADR 0004).
- No Bevy coupling in the netcode, so Bevy upgrades don't block on networking crates.
