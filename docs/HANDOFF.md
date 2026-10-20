# Handoff: overnight session 2026-10-20

Morning briefing for the developer. Updated at every checkpoint; if the session died, the
"In progress" section says where.

## Done tonight

- **Groundwork** (`0560d90`): team formats (`Format::team`, `Outcome::from_team_scores`), type-erased `MinigameEntry`/`DynRun` for the registry, `sim_core::physics`; `check_all` now covers every supported format.
- **Lantern Push** (`7cf6f06`): sumo with dash and brace on a shrinking platform, bots, interpolated view, all formats.
- **Board simulation** (merge commit after `ad534a6`): `sim-board` with the Dusk Meadow board in RON, the §3.1 rules, board bots, the `minigame-registry` crate and `tools board`. Built by a subagent, integrated and checked by me.

## Decisions to review

1. **Lantern Push has a B "brace" button** (heavy for 0.75 s, dashes bounce off it). It isn't in DESIGN.md's one-line pitch; it adds a counter to dash spam. Easy to delete if it muddies the game.
2. **1v3 balance in Lantern Push is physical** (the solo is bigger and heavier), tuned so the solo wins 34–56% against bots.
3. **Board rules details** (ROADMAP.md, under Board simulation): passing the Star = stepping onto it; turn-order rolls never tie; 2v2 teams = blue vs red; 12 rounds default.
4. **The Lantern Push platform is a smooth disc with a glowing edge**, not voxel tiles, so the ring-out line is exact; the tiles are decoration that crumbles away.

## Needs human playtest

- **Lantern Push:** dash/brace feel, knockback strength, match length with 2 players.
  `cargo run -p game -- --minigame lantern-push --players 4 --humans 1` (also try `--format 1v3`, `--format 2v2`, `--players 2`).

## Blocked or skipped

- **Four lantern-folk characters:** skipped, needs Blender running with its MCP.
- **Polish pass 1:** skipped, needs a human playtest.

## In progress when the session ended

- **Firefly Catch:** simulation being built by a subagent in a worktree; view (with `bevy_hanabi`, git rev `77630ff`, verified working on Bevy 0.20) next.
- Next after Firefly Catch: **Board view**.
