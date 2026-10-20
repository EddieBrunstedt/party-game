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
- [x] **Firefly Catch** (to playable). Chase fleeing fireflies. Introduces:
  - spawning and despawning objects inside the simulation with stable IDs
  - GPU particles (`bevy_hanabi` from git main until its 0.20 release)
  - Done when: same bar as Lantern Push.
  - Rules: run with the stick, A swings the net (catches every firefly just ahead), B dashes. Fireflies spawn and despawn in the simulation with never-reused ids, wander, flee, and **sprint, hop over a chaser and tire** (stamina); tired fireflies are the catch window. Golden ones are worth 3. 45 s. FFA, 1v3 (solo ×3), 2v2 (team sums).
  - Evidence: 23 tests including `check_all` over 16 seeds; `tools check firefly-catch --seeds 32`; 100 bot runs per difficulty at 4 players with wins spread 23–35 per seat; one Hard bot averages 13.5 points against Easy bots' 7.5. Screenshot sheets of FFA and 2v2 checked.
  - Simulation built by a subagent in a worktree. Decision: stamina and the hop were added because fireflies "a bit slower than a player" were always caught in ~2 s in this small meadow; tuning speed, fear radius or net size didn't fix that.
  - Decision: the 1v3 solo's catches count ×3 (bots: solo wins 30–47%; ×4 made it 65–77%).
  - Particles: `bevy_hanabi` git rev `77630ff` (`game/src/particles.rs`): catch bursts in the catcher's colour, firefly trails, drifting motes. Every effect has a fixed capacity (64/48/256).
  - Surprise: effect assets must exist before the first state's `OnEnter`, which runs before `Startup`, so `Effects` is built in the plugin (`FromWorld`).
  - Surprise: once the dev machine's screen went idle, every windowed run hung with no frames (even a standalone Bevy demo). `gamescope --backend headless` renders fine; see CLAUDE.md.
  - Needs human playtest: whether the sprint/hop/tire rhythm reads and feels fair, net reach with a pad, dash/net cooldowns, ×3 solo fairness. `cargo run -p game -- --minigame firefly-catch --humans 1` (also `--format 1v3`, `--format 2v2`).
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
- [x] **Board view.** One small board scene (procedural/voxel), a camera that follows the active player, dice roll, path choice at forks, star purchase, coin popups.
  - Done when: a full game is playable on screen with 1 human + 3 bots.
  - Built: `game/src/board_view.rs`. The view plays `sim-board` events back one at a time with durations, keeping its own "shown" copy (pawn spaces, coins, stars), so nothing is spoiled before its animation. Pawns hop space to space, a die spins over the pawn until you hit A, the camera follows the active pawn and pulls back to the whole board between turns, arrows light the fork options (point the stick, A confirms), a Star offer takes A/B, coin popups show, and minigame announcements show the format.
  - Evidence: unit tests for the human decision path (roll, star yes/no, stick-aimed fork choice). A scripted session (keyboard P1 pressing A every 1.5 s + 3 bots) played 6 rounds on screen, from the title through results, under headless gamescope with no panics. A 12-round bot-only game ran on screen and reached round 12 after about 20 minutes. Screenshot sheets checked; trees that hid pawns from the camera were moved back from the path.
  - Surprise: with bots only, 12 rounds take about 22 minutes. Humans think longer, but 12 rounds may still come in under the 30–40 minute target; consider 15 (the join screen already offers it).
  - Decision: the board picks only from minigames this client can show (`minigames::SHOWN`), and a test fails if the registry has a minigame without a view.
  - Decision: bots wait 0.7 s before each board decision so a human can follow them.
  - Needs human playtest: pacing of event durations (`duration()` in `board_view.rs`), whether stick-aimed fork choice feels natural, camera follow distance, and readability of the dark meadow. `cargo run -p game` → Board game → join with a pad → Start.
- [x] **Game flow.** Title, then join screen ("press A to join", controller bound to seat, character pick), then board ↔ minigames, then results. UI strings move to Fluent keys here (all current UI text is hardcoded English). Choose the input crate (leafwing vs bevy_enhanced_input) here.
  - Done when: the game launches into the title screen and a whole session runs without CLI flags.
  - Built: `flow.rs` (`AppState`: Title, Join, Pick, Board, Minigame, Results) and `menus.rs`. Title (board game / free play / quit) → join (each controller pressing A takes the next seat and stays bound to that exact gamepad; left/right picks a character, each one once; B leaves; X sets bot difficulty; up/down sets rounds; Start begins; bots fill the rest) → board ↔ minigames → results → title. Free play: join → picker (minigame + team layout) → minigame; B after a result returns to the picker.
  - Characters: four placeholder lantern folk, each with its own light source (paper-lantern tassel, firefly jar, candle flame, neon halo), until the Blender models exist.
  - Fluent: every UI string is in `crates/game/locales/en/main.ftl`, used through `tr!`. Tests check that every key parses and resolves, and that every minigame has a name.
  - Input crate: neither, for now; see ADR 0007.
  - Evidence: `cargo run -p game` with no flags opens the title. A headless run driven only by scripted key presses (`--press`, `--press-every`, which inject presses through the normal input path) went Title → Join → Board → 6 × (Minigame → Board) → Results → Title. The free-play path ran too (picker → Lantern Push 1v3 → B back to the picker). Lobby rules have unit tests.
  - Review: a review subagent found that a Start held from the menus opened the next minigame paused, and that Firefly Catch lost catch effects when fireflies were caught out of id order. Both are fixed; a held Start is now ignored until released.
  - Decision: board sessions always have 4 seats, with bots filling the empty ones, because the §3.1 format rules assume four.
  - Decision: menus read devices directly (any pad or the keyboard can drive the title), and seats read only their bound device.
  - Needs human playtest: the join flow with real pads (several controllers joining, character pick, leaving), and whether a reconnected pad comes back to its seat. Bevy may give a reconnected pad a new entity, in which case the seat stays "lost"; this is untested. `cargo run -p game`.
- [ ] **Four lantern-folk characters.** Modelled in Blender via MCP on one shared rig with shared animations (idle, hop, celebrate, slump), exported as glTF, replacing the code-built placeholders.
  - Done when: all four load in-game from `.glb` and animate.
- [ ] **Polish pass 1.** Bring the three minigames and the board to the **polished** level using the new characters, plus audio (`bevy_kira_audio`, CC0/sfxr-style SFX) and `bevy_hanabi` particles.
  - Done when: every minigame and the board meet the polished definition, and a full game has been playtested by humans.
- [x] **Steam Deck quality preset.** A "Deck" preset at 1280×800 with the content budgets from DESIGN.md §2, toggleable in-game.
  - Done when: the preset holds ~2–3 ms GPU frame time on the dev RTX 2080 Ti in the busiest scene.
  - Built: `game/src/quality.rs`. Deck = 1280×800 window, one shadow cascade at 1024² (Desktop: two at 2048²), no point-light shadows (Desktop keeps the Quick Draw lantern's), no MSAA (Desktop 4×), bloom chain capped at 256 (Desktop 512). F4 or a pad's Select toggles it; `--quality deck` starts in it. The overlay (F3) shows GPU time; `--gpu-report` prints avg/p95/max and per-pass times at exit (`game/src/perf.rs`, Bevy's `RenderDiagnosticsPlugin` timestamp queries).
  - Evidence (RTX 2080 Ti, 1280×800, headless gamescope, 1021 frames after a 4 s warm-up, average GPU ms Desktop → Deck): title 0.51 → 0.46, board 0.59 → 0.52, Quick Draw 1.20 → 0.54 (the busiest scene; half its Desktop time is the lantern's cube shadow map), Lantern Push 0.62 → 0.53, Firefly Catch 0.59 → 0.50. p95 is within 0.02 ms of the average everywhere. Screenshots: Deck looks the same apart from the missing lantern shadows.
  - Caveat: bevy_hanabi's compute passes don't record timestamp spans, so particle simulation isn't in these numbers (it's capped at a few hundred particles). The current scenes are placeholders; re-measure after Polish pass 1 brings real models, particles and audio.
  - Decision: content budgets from DESIGN.md §2 are enforced by convention (≤2 shadow-casting lights per scene; lanterns are emissive plus unshadowed point lights) and checked by `--gpu-report`, not by code limits.
- [x] **GitLab CI.** `cargo test --workspace` and `cargo clippy --workspace --all-targets -D warnings` on every push.
  - Done when: the pipeline is green on `main`.
  - `.gitlab-ci.yml`: one `check` job on `rust:1.99-bookworm` (bevy_hanabi needs Rust 1.97+) running `cargo test --workspace --locked`, then clippy with `-D warnings`. Cargo registry and `target/` are cached by `Cargo.lock`; LFS smudging is skipped (no assets needed to build).
  - Evidence: pipelines #2–#4 green on main; #4 (`1fee32a`) ran every test and clippy in about 5.5 minutes (checked with `glab`).
  - Decision: one job instead of separate test and clippy jobs, because both would compile Bevy from scratch and the cache only helps the second run.

## Milestone 2: Online alpha

Not yet broken down. In scope: the `net` crate (transport trait, UDP backend), a `ggrs` P2P driver for minigames, the host-authoritative board protocol, reconnect and pause flows, save/resume. See ADRs 0002–0004.

## Milestone 3: Launch

Not yet broken down. Content targets are in DESIGN.md §6.
