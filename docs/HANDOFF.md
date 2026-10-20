# Handoff: overnight session 2026-10-20

Morning briefing for the developer. Updated at every checkpoint; if the session died, the
"In progress" section says where.

## Done tonight

- **Groundwork** (`0560d90`): team formats (`Format::team`, `Outcome::from_team_scores`), type-erased `MinigameEntry`/`DynRun` for the registry, `sim_core::physics`; `check_all` now covers every supported format.
- **Lantern Push** (`7cf6f06`): sumo with dash and brace on a shrinking platform, bots, interpolated view, all formats.
- **Board simulation** (`b118425`): `sim-board` with the Dusk Meadow board in RON, the §3.1 rules, board bots, the `minigame-registry` crate and `tools board`. Built by a subagent, integrated and checked by me.

## Decisions to review

0. **Firefly Catch fireflies sprint, hop over you and tire** (the subagent added this; plain "slightly slower than the player" fireflies were caught in about 2 s). The 1v3 solo's catches count ×3.
1. **No input crate** (ADR 0007, new): the custom mapper stays for now.
1. **Lantern Push has a B "brace" button** (heavy for 0.75 s, dashes bounce off it). It isn't in DESIGN.md's one-line pitch; it adds a counter to dash spam. Easy to delete if it muddies the game.
2. **1v3 balance in Lantern Push is physical** (the solo is bigger and heavier), tuned so the solo wins 34–56% against bots.
3. **Board rules details** (ROADMAP.md, under Board simulation): passing the Star = stepping onto it; turn-order rolls never tie; 2v2 teams = blue vs red; 12 rounds default.
4. **The Lantern Push platform is a smooth disc with a glowing edge**, not voxel tiles, so the ring-out line is exact; the tiles are decoration that crumbles away.

- **Firefly Catch** (sim `f16273a`, view in the commit after `23a72b5`): fleeing fireflies with stable ids, hanabi particles, all formats; screenshots checked.

## Needs human playtest

- **Firefly Catch:** whether sprint/hop/tire reads, net reach, cooldowns, ×3 solo.
  `cargo run -p game -- --minigame firefly-catch --humans 1` (also `--format 1v3`, `--format 2v2`).

- **Lantern Push:** dash/brace feel, knockback strength, match length with 2 players.
  `cargo run -p game -- --minigame lantern-push --players 4 --humans 1` (also try `--format 1v3`, `--format 2v2`, `--players 2`).

## Blocked or skipped

- **Four lantern-folk characters:** skipped, needs Blender running with its MCP.
- **Polish pass 1:** skipped, needs a human playtest.

## In progress when the session ended

The usage limit ran out mid-session. The last commit ("WIP: board view, ...") is green: tests and clippy pass. Not yet ticked in ROADMAP.md:

- **Board view**: built (`game/src/board_view.rs`). A bot-only 3-round game ran on screen through Lantern Push and back, and the human prompts render. `cargo run -p game` (no flags) now starts a board game; `--rounds 3` shortens it. Not ticked: nobody has played it with a pad. A human-intent test would also help.
- **Game flow**: groundwork only. `AppState` (`game/src/flow.rs`), minigames launch and return through `MinigameLaunch`/`MinigameResult`, and `i18n.rs` (Fluent, with an empty `locales/en/main.ftl`). Missing: the title, join and results screens (the board already switches to `AppState::Results` at game over, which shows nothing yet), moving strings to Fluent, and character pick. Input crate decision: ADR 0007.
- **Steam Deck preset**: not started.
- **GitLab CI**: `.gitlab-ci.yml` is written and pushed (rust:1.99-bookworm; tests and clippy in one job). Check the pipeline result, then tick it.
