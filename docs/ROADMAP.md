# Roadmap

The single source of truth for **what's done and what's next**. Work items top to bottom. Each item is done when its **Done when** line holds. Then tick it, note anything surprising under it, and update `DESIGN.md` if the design changed.

## Minigame completion levels

- **Playable:** rules complete and fun, bots at all difficulties, `check_all` passing, and placeholder visuals that always read clearly (who is who, what just happened, who is winning). New minigames stop here.
- **Polished:** real characters and animations, sound, particles and juice, an instruction card, Fluent strings. This is done in batched polish passes once the shared systems (characters, audio, particles, UI framework) exist, so nothing is built twice.

A fun problem found at the playable stage gets fixed straight away; polish won't rescue a dull game.

## Milestone 1: Vertical slice (couch only, bots fill seats)

- [x] **Workspace and simulation core.** `sim-core`, `sim-check`, `tools`, the `Minigame` trait, `Run<M>` (pause-as-input, controller loss), stable checksums, PCG RNG with SplitMix seeding.
- [x] **Quick Draw.** Simulation, bots, all quality bars, Bevy view (lantern folk placeholders, bloom, fireflies, HUD, camera that fits the window).
- [x] **Playtest Quick Draw (human).** Played with a PS5 controller: "felt good", no changes needed at the playable level.
  - Polish is deferred to the polish pass.
  - Note: a "connected" gamepad that sends no button presses is usually a flaky cable. `--log-input` and the kernel log (`journalctl -k`, look for error -71) will show it.
- [x] **Lantern Push** (to playable). Sumo knockoff on a shrinking platform. Introduces:
  - custom deterministic physics in `sim-core` (circles, knockback, ring-out) with `f32` + `math` wrappers + `hash_f32`
  - render interpolation between ticks in `game`
  - Done when: `check_all` passes, bots at all difficulties produce close matches, and screenshots show a readable arena.
  - Rules: A dashes (knockback + stun on whoever it hits), B braces (×4 mass for up to 0.75 s; a dash into a brace rebounds and stuns the attacker). The platform holds at radius 7 for 8 s, then shrinks to 2.2 by 45 s; 75 s time limit, survivors share the win. Supports FFA, 1v3, 2v2 and duel.
  - Evidence: `check_all` (6 seeds in the unit test, 32 via `tools check`), 12 rule tests. 100 bot runs per difficulty at 4 players: avg 42–46 s, wins spread 21–33 per seat. Over 200 seeds: 1v3 solo wins 42% (Easy), 56% (Normal), 34% (Hard); 2v2 is even; one Hard bot beats three Easy bots 57% of the time.
  - Decision: physics is `sim_core::physics` (`Vec2`, `Body`, `collide`, `collide_all`, `steer`, `bounce_in_rect`), velocities in units/second with a fixed `DT`, because every later minigame needs the same circles. Pairs are resolved in index order, so the result depends only on the slice.
  - Decision: the platform is a disc scaled to the exact simulated radius, with a glowing edge ring, because readability beats voxel purity: decorative voxel tiles crumble off only once fully outside it.
  - Decision: the 1v3 solo is bigger and heavier (radius 0.58, mass 1.35) instead of scoring differently, because in sumo the handicap has to be physical. Tuned with bots; first values (1.6 mass) made the solo win 75%.
  - Decision: `check_all` now runs every supported format (1v3 and 2v2 at 4 players, duel at 2), and `sim-core` gained `Format::team` and `Outcome::from_team_scores`.
  - Interpolation: `ActiveRun` keeps `previous` (the state one tick back), and views blend by `Time<Fixed>::overstep_fraction()`.
  - Surprise: the first knockback values ended games in ~7 s (one hit from the middle was a ring-out), and the next set timed out most games. Bots also need their "won't dash off the edge" check to account for a dash stopping on contact, or they stall on a small platform.
  - Needs human playtest: dash/brace feel, knockback strength, whether the 8 s before shrinking and ~40 s matches drag with 2 players (2-player games mostly end when the platform is nearly minimum size). `cargo run -p game -- --minigame lantern-push --players 4 --humans 1` (add `--format 1v3` or `--format 2v2`).
- [ ] **Firefly Catch** (to playable). Chase fleeing fireflies. Introduces:
  - spawning and despawning objects inside the simulation with stable IDs
  - GPU particles (`bevy_hanabi` from git main until its 0.20 release)
  - Done when: same bar as Lantern Push.
- [x] **Board simulation (`sim-board`).** Board graph in RON, turn state machine, the §3.1 ruleset, board bots, the minigame format chosen from space colours. A minigame registry lets the board pick and launch any minigame by format.
  - Done when: `tools` can play a full bot-only board game headlessly, minigames included, end to end, deterministically from a seed.
  - Evidence: `cargo run -p tools -- board --seed 1 --verbose` plays 12 rounds with Quick Draw and Lantern Push in FFA, 1v3 and 2v2; the same seed gives the same checksum, and different seeds differ. 25 `sim-board` tests, plus a `minigame-registry` integration test that plays full games with the real minigames.
  - Built by a subagent in a worktree, then merged; ordered ahead of Firefly Catch's view because it didn't depend on it.
  - API: `Game::new(board, rules, players, seed, &registry)`, `apply(seat, Intent) -> Result<Vec<Event>, Rejected>`, `apply_minigame_outcome`, `awaiting()`, `bot_intent`, `headless::play_bots`. Board file: `crates/sim-board/boards/meadow.ron` ("Dusk Meadow", 39 spaces, 2 forks, 5 star spots).
  - Decision: turn-order rolls draw from numbers nobody has rolled yet, so there are no ties and no re-rolls.
  - Decision: space ids equal their index in the RON file, so lookups are plain indexing. Inserting a space means renumbering.
  - Decision: the whole `Board` lives in `Game` (floats hashed with `hash_f32`), so a save or reconnect snapshot is self-contained. The minigame pool is copied into the state as ids and formats.
  - Decision: "passing" the Star means stepping onto it, including the final step. Buying costs no steps, and landing on it still pays its colour.
  - Decision: in 2v2, the players on blue are team 0. In 1v3, the solo is whoever is on the minority colour. Fewer than 4 players is always FFA.
  - Decision: 12 rounds by default (`Rules::rounds`). Needs human playtest: whether that lands in the 30–40 minute target.
  - Decision: board bots seed their randomness from `checksum(game)` plus the seat. They head for the Star 35%/80%/100% of the time on Easy/Normal/Hard, and Easy declines an affordable star 10% of the time.
- [ ] **Board view.** One small board scene (procedural/voxel), a camera that follows the active player, dice roll, path choice at forks, star purchase, coin popups.
  - Done when: a full game is playable on screen with 1 human + 3 bots.
- [ ] **Game flow.** Title, then join screen ("press A to join", controller bound to seat, character pick), then board ↔ minigames, then results. UI strings move to Fluent keys here (all current UI text is hardcoded English). Choose the input crate (leafwing vs bevy_enhanced_input) here.
  - Done when: the game launches into the title screen and a whole session runs without CLI flags.
- [ ] **Four lantern-folk characters.** Modelled in Blender via MCP on one shared rig with shared animations (idle, hop, celebrate, slump), exported as glTF, replacing the code-built placeholders.
  - Done when: all four load in-game from `.glb` and animate.
- [ ] **Polish pass 1.** Bring the three minigames and the board to the **polished** level using the new characters, plus audio (`bevy_kira_audio`, CC0/sfxr-style SFX) and `bevy_hanabi` particles.
  - Done when: every minigame and the board meet the polished definition, and a full game has been playtested by humans.
- [ ] **Steam Deck quality preset.** A "Deck" preset at 1280×800 with the content budgets from DESIGN.md §2, toggleable in-game.
  - Done when: the preset holds ~2–3 ms GPU frame time on the dev RTX 2080 Ti in the busiest scene.
- [ ] **GitLab CI.** `cargo test --workspace` and `cargo clippy --workspace --all-targets -D warnings` on every push.
  - Done when: the pipeline is green on `main`.

## Milestone 2: Online alpha

Not yet broken down. In scope: the `net` crate (transport trait, UDP backend), a `ggrs` P2P driver for minigames, the host-authoritative board protocol, reconnect and pause flows, save/resume. See ADRs 0002–0004.

## Milestone 3: Launch

Not yet broken down. Content targets are in DESIGN.md §6.
