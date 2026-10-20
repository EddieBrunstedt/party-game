# Party Game

A 2–4 player Mario Party-style game in Rust + Bevy 0.20, couch and online, built by one developer with Claude.

## Continuing work

"Continue" or "next thing" means: open `docs/ROADMAP.md` and take the first unticked item. An item is finished when its **Done when** line holds. Then tick it, note anything surprising under it, and keep `docs/DESIGN.md` in step with what was built.

Read `docs/DESIGN.md` before designing anything. Read the matching ADR in `docs/adr/` before touching simulation, netcode, transport or physics.

## Invariants

- **Simulation crates stay Bevy-free.** `sim-*` and `minigames/*` are plain Rust, and Bevy only reads their state (ADR 0001). Simulation code is deterministic: a seeded `Rng` inside the state, `sim_core::math` for transcendental functions, ordered collections only, no wall clock.
- **Every minigame passes `sim_check::check_all`,** run as a unit test in its crate. A minigame without that test isn't done.
- **Bots are virtual controllers:** `bot_input` is a pure function of state, seat and difficulty that returns an `Input`.
- **The game's name lives only in `GAME_TITLE`** (`crates/game/src/main.rs`). Crates and identifiers use generic names.

## Gotchas

- Bevy 0.20 and ggrs 0.13 postdate Claude's training data. Check APIs against the source in `~/.cargo/registry/src/*/bevy_*-0.20.0/` before writing code that uses them.
- Check visual work yourself. `cargo run -p game -- --humans 0 --seed 1 --screenshot-dir <dir> --screenshot-at <secs>` (repeatable) saves PNGs and exits; `magick montage` turns several into one contact sheet to view.
