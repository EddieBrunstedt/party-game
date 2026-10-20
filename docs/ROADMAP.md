# Roadmap

The single source of truth for **what's done and what's next**. Work items top to bottom. Each item is done when its **Done when** line holds. Then tick it, note anything surprising under it, and update `DESIGN.md` if the design changed.

## Milestone 1: Vertical slice (couch only, bots fill seats)

- [x] **Workspace and simulation core.** `sim-core`, `sim-check`, `tools`, the `Minigame` trait, `Run<M>` (pause-as-input, controller loss), stable checksums, PCG RNG with SplitMix seeding.
- [x] **Quick Draw.** Simulation, bots, all quality bars, Bevy view (lantern folk placeholders, bloom, fireflies, HUD, camera that fits the window).
- [ ] **Playtest Quick Draw (human).** The developer plays with real gamepads and reports on feel, timing and readability.
  - Done when: feedback is collected and any changes from it are made.
- [ ] **Lantern Push.** Sumo knockoff on a shrinking platform. Introduces:
  - custom deterministic physics in `sim-core` (circles, knockback, ring-out) with `f32` + `math` wrappers + `hash_f32`
  - render interpolation between ticks in `game`
  - Done when: `check_all` passes, bots at all difficulties produce close matches, and screenshots show a readable arena.
- [ ] **Firefly Catch.** Chase fleeing fireflies. Introduces:
  - spawning and despawning objects inside the simulation with stable IDs
  - GPU particles (`bevy_hanabi` from git main until its 0.20 release)
  - Done when: same bar as Lantern Push.
- [ ] **Board simulation (`sim-board`).** Board graph in RON, turn state machine, the §3.1 ruleset, board bots, the minigame format chosen from space colours. A minigame registry lets the board pick and launch any minigame by format.
  - Done when: `tools` can play a full bot-only board game headlessly, minigames included, end to end, deterministically from a seed.
- [ ] **Board view.** One small board scene (procedural/voxel), a camera that follows the active player, dice roll, path choice at forks, star purchase, coin popups.
  - Done when: a full game is playable on screen with 1 human + 3 bots.
- [ ] **Game flow.** Title, then join screen ("press A to join", controller bound to seat, character pick), then board ↔ minigames, then results. UI strings move to Fluent keys here (all current UI text is hardcoded English). Choose the input crate (leafwing vs bevy_enhanced_input) here.
  - Done when: the game launches into the title screen and a whole session runs without CLI flags.
- [ ] **Four lantern-folk characters.** Modelled in Blender via MCP on one shared rig with shared animations (idle, hop, celebrate, slump), exported as glTF, replacing the code-built placeholders.
  - Done when: all four load in-game from `.glb` and animate.
- [ ] **Steam Deck quality preset.** A "Deck" preset at 1280×800 with the content budgets from DESIGN.md §2, toggleable in-game.
  - Done when: the preset holds ~2–3 ms GPU frame time on the dev RTX 2080 Ti in the busiest scene.
- [ ] **GitLab CI.** `cargo test --workspace` and `cargo clippy --workspace --all-targets -D warnings` on every push.
  - Done when: the pipeline is green on `main`.

## Milestone 2: Online alpha

Not yet broken down. In scope: the `net` crate (transport trait, UDP backend), a `ggrs` P2P driver for minigames, the host-authoritative board protocol, reconnect and pause flows, save/resume. See ADRs 0002–0004.

## Milestone 3: Launch

Not yet broken down. Content targets are in DESIGN.md §6.
