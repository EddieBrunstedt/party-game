# Handoff: overnight session 2026-10-20

Morning briefing for the developer. Updated at every checkpoint; if the session died, the
"In progress" section says where.

## Done tonight

- **Groundwork** (`0560d90`): team formats (`Format::team`, `Outcome::from_team_scores`), type-erased `MinigameEntry`/`DynRun`, `sim_core::physics`; `check_all` now covers every supported format.
- **Lantern Push** (`7cf6f06`): sumo with dash and brace on a shrinking platform, bots, interpolated view, all formats.
- **Board simulation** (`b118425`): `sim-board` with the Dusk Meadow board in RON, the §3.1 rules, board bots, the `minigame-registry` crate and `tools board`. Built by a subagent; I integrated and checked it.
- **Firefly Catch** (sim `f16273a`, view `4b8ce94`): fleeing fireflies with stable ids, bevy_hanabi particles (git `77630ff`), all formats.
- **Board view** (`23a72b5`, tests `88e4784`): playable on screen with humans and bots.
- **Game flow** (`1fee32a`, fixes `d412cf1` and `72cf26a`): title → join (pads bind to seats, character pick) → board ↔ minigames → results; free-play picker; every UI string in Fluent; ADR 0007 on input.
- **GitLab CI** (`.gitlab-ci.yml`, from `23a72b5`): tests and clippy in one job; pipelines green on main (checked with `glab`).
- **Steam Deck preset** (`7a441ee`): F4 / Select toggles it; `--gpu-report` measures it. Numbers are in its roadmap entry.

## Decisions to review

1. **Firefly Catch fireflies sprint, hop over you and tire** (a subagent added this; plain "slightly slower than the player" fireflies were caught in about 2 s). The 1v3 solo's catches count ×3.
2. **No input crate** (ADR 0007, new): our own ~150-line mapper stays until rebinding is needed.
3. **Lantern Push has a B "brace" button** (heavy for 0.75 s, dashes bounce off). Not in DESIGN.md's original pitch; easy to delete.
4. **Board rules details** (ROADMAP.md, under Board simulation): passing the Star = stepping onto it; turn-order rolls never tie; 2v2 teams = blue vs red; 12 rounds by default (the join screen offers 6–20).
5. **Placeholder characters** differ by their light source (tassel, firefly jar, candle, neon halo) until the Blender models exist.
6. **1v3 balance in Lantern Push is physical** (the solo is bigger and heavier), tuned so the solo wins 34–56% against bots.
7. **The Lantern Push platform is a smooth disc with a glowing edge**, not voxel tiles, so the ring-out line is exact; the tiles are decoration that crumbles away.
8. **`clippy.toml` raises `too-many-arguments` to 10**, because Bevy systems take their data as parameters.

## Needs human playtest

- **A whole session:** `cargo run -p game` → Board game → join with pads (A), pick characters (left/right), Start. Check pacing, the stick-aimed fork choice, the camera, and the join flow with several pads. Also check that an unplugged and replugged pad comes back to its seat; this is untested.
- **Lantern Push:** dash/brace feel, knockback strength, match length with 2 players.
  `cargo run -p game -- --minigame lantern-push --humans 1` (also try `--format 1v3`, `--format 2v2`, `--players 2`).
- **Firefly Catch:** whether sprint/hop/tire reads, net reach, cooldowns, ×3 solo.
  `cargo run -p game -- --minigame firefly-catch --humans 1` (also `--format 1v3`, `--format 2v2`).
- **Deck preset look:** press F4 in any scene: are "no MSAA, one shadow cascade" acceptable?

## Blocked or skipped

- **Four lantern-folk characters:** skipped, needs Blender running with its MCP.
- **Polish pass 1:** skipped, needs a human playtest.

## Useful things learned tonight

- When the dev machine's screen goes idle, windowed Bevy runs hang without presenting frames. `gamescope --backend headless -w 1280 -h 800 -W 1280 -H 800 -- ./target/debug/game ...` renders fine (now in CLAUDE.md).
- `--press 3:Space` / `--press-every 1.5:Space` inject key presses, so a headless run can walk the menus and play turns. Screen changes are logged as `screen: A -> B`.
- `--gpu-report` prints GPU frame time (avg/p95/max) and a per-pass breakdown at exit.

## In progress when the session ended

- Measuring the Deck preset's GPU time in every scene, then ticking Board view, Game flow and the Steam Deck preset.
