# Handoff: session 2026-10-22 (next: build a new minigame)

Briefing for the next session. Earlier sessions' notes are in git history (`git log -p docs/HANDOFF.md`).

## Pick up here: build the next minigame

"Continue" means: take the first unticked item in `docs/ROADMAP.md`, which is **Next minigame** (Milestone 3, Minigames). Nothing else is waiting: the tree is clean and every earlier minigame is committed. The developer leaves the choice of minigame to the session.

1. Read the Minigames section of `docs/ROADMAP.md` ("Formats", "Mechanics", "Cost" and "How to add one") and `docs/MINIGAME_IDEAS.md`. Prefer an idea that adds something the current eleven lack. Ideas not yet taken:
   - **Jump-and-duck race:** lanes, A to jump and B to duck at the right time. Cheap, but it would be the second timing game after Lantern Tower.
   - **Button mash race:** simple and cheap, and close to Quick Draw.
   - **Brainy race:** route choices, switches, small puzzles.
   - **Pick the right solution:** a problem with candidate answers (puzzles generated from the seed need no question bank). Bots get a chance to be right that depends on difficulty. Asked on 2026-10-21, the developer leant towards buzzer answering; puzzle kinds and the 1v3 are still open.
   - **Lights out hide and seek:** a natural 1v3. Hedge Maze already used its dark lantern-lit half, so the hiding has to be the new part.
   - **Bigger jobs** (say so in the item, or build the shared piece as its own item first): quiz (needs a question bank), spot the right picture (needs picture content), obstacle course and platform fighter (need platforming physics in `sim_core::physics`).
2. Before writing code, ask the developer about the genuinely open rule questions (AskUserQuestion, 2–4 options each, a recommended one first). Past sessions asked about the core mechanic, the luck, and what the 1v3 solo does; the developer usually takes the recommended option. Then add the item above "Next minigame" and mark the idea taken in `MINIGAME_IDEAS.md`.
3. Build it the way the last ones were built. Tabletop Rally (`crates/minigames/tabletop-rally`, `game/src/tabletop_rally.rs`) and Lantern Derby are the freshest templates. Either commit shows every file a new minigame touches: workspace `Cargo.toml`, registry, tools (three match arms), `game` (`main.rs`, `minigames.rs`, `Cargo.toml`), Fluent strings, and an `intro.rs` test that pins the card's numbers to the constants.
4. Check it: `cargo test -p minigame-<id>` (including `check_all`), `tools bots <id> --runs 4000` for every format and difficulty, `tools net-check <id>`, and screenshots of every format and the instruction card under headless gamescope.
5. Tick the item and fill it in: rules, bots report, decisions, surprises, and "Needs human playtest" with the command. Add a DESIGN.md §3.3 entry, update this handoff, run `cargo test --workspace` and clippy, and commit.

Lessons from this session:
- **Ask the developer which minigame if they might have a wish.** This session started on "pick the right solution" (the developer leant towards buzzer answering) and the developer switched to a Micro Machines-style racer; that idea and the buzzer leaning are noted in `MINIGAME_IDEAS.md`.
- **Seed bot rolls by mixing the parts, not XORing them.** `seed ^ window` lets match seeds and windows cancel, so matches share each seat's luck and `tools bots` shows seat bias that isn't real (Tabletop Rally's `bot::mix`). Other minigames' bots XOR seed, seat and counters too; if one shows an odd seat skew, check that first.
- **Draw a new track or layout before tuning it.** Plotting the sampled loop (matplotlib from a throwaway example's dump) showed tight kinks and badly placed props at once; a tiny Python copy of the spline maths made trying control points quick.
- **Mesh quads must be wound counter-clockwise seen from above**, or Bevy culls them and the floor silently vanishes.
- **1v3 balance drifts when the arena or the bots' rolls change.** Reshaping one bend and reseeding the rolls moved the getaway car from 55% to 62% at Normal; rerun the 1v3 report after any track or layout edit.
- Still true: balance 1v3 at every difficulty; throwaway probes as `examples/` (delete before committing); don't run `cargo fmt --all` (lantern-echo isn't rustfmt-clean), use `cargo fmt -p <crate>`.

## Done this session

- **Tabletop Rally** (this commit): see its item in `docs/ROADMAP.md`. The developer chose off-screen knockout, handmade tabletop tracks and the 1v3 getaway car; the stick first pointed the way, and after trying it the developer asked for Micro Machines' car-relative steering (left and right swap when driving down the screen). This session's calls: closing in after 6 s, fixed-deceleration grip, restarting on a straight, a getaway car 1% faster and centred by the camera, and per-difficulty getaway-bot care.

Tuning knobs, if a playtest asks for changes:
- Tabletop Rally: handling in `MAX_SPEED`, `ACCEL`, `BRAKE`, `GRIP`, `TURN_STANDING`/`TURN_MOVING`, `OFF_ROAD_SPEED`; round length in `VIEW_HALF`, `CLOSE_IN_AFTER`, `CLOSE_IN_TICKS`, `LOOKAHEAD`; 1v3 in `GETAWAY_SPEED`, `HEAD_START`, `CATCH_REACH` and the bots' `getaway_care`/`chase`. Bots in `bot::skill` (corner, wobble, slip, lapse). Tracks in `track.rs` (control points, props by `Across`); the view's framing in `FRAME_MARGIN_X`/`_Y` and `PITCH`.
- Lantern Derby: bots in `bot::skill` (picks, aim_error, power_error, pause, reaction). 1v3 balance comes from the ratio of `TRIO_GOAL` to `BARKER_GOAL` (the lid stops about 38% of the value) and the lid's `SHUT_TICKS`, `LID_COOLDOWN`, `LID_REACH` and `CURSOR_SPEED`. Feel: `CHARGE_TICKS`, `AIM_DIVISOR`, `WOBBLE`, `RIM` and `HOLES`.
- Hedge Maze: bots in `bot::skill` (delay, noise, flare_dark, fear, urgency, intercepts). 1v3: `GHOST_SPEED`, `RESPAWN_TICKS`, `GHOST_RACE_TICKS` and `TRIO_NEEDED` (the card's text and an `intro.rs` test pin 2 and 20 s). Generator: `NEWEST_PERCENT`, `MIN_PATH`, `CANDIDATES` and `LOOPS`.

## Waiting on the developer's playtests

None of these block new minigames. Don't build any of the offered changes unless the developer asks.

- Tabletop Rally: see "Needs human playtest" under its item (how the car-relative steering feels, the slide in fast bends, whether the closing-in rectangle reads, match length with four, the inside-lane start, the getaway car's edge). It has no engine sound yet.
- Lantern Derby: see "Needs human playtest" under its item (the meter's speed and readability, the aim's speed, whether the luck feels fair, whether the barker is fun).
- Hedge Maze: see "Needs human playtest" under its item. Open question: equal bots often tie because the maze is symmetric. A tie-break (e.g. total finishing time) would cut that, but ties share the win in every other minigame.
- Musical Stools: see "Needs human playtest" under its item. One rule needs the developer's OK: in 1v3 the cat's **dash catches** a runner (out for the round). With the agreed version (the cat only shoves), the cat won 0–1%; with catching it wins 48–53%. The other levers that were tried are in the item's decisions.
- Sky Lanterns: open question whether a shooting gallery, where the better shooter almost always wins, needs a catch-up twist.
- Lantern Lottery: playtest 1 is done. Still open: never seeing your own pick felt hard to trust. Offered: get the lantern you point at when you press A, a short rumble when the lock-in counts, and "point at a lantern first"; or rumble only; or leave it.
- Lantern Tower: if a seat still seems harder after the orthographic camera, the drop guides (thin lines up from the top floor's edges) are the next thing to try.

## General minigame notes

- Milestone 2 is done apart from the online playtest, which is **deferred** (not a checkbox, so "continue" skips it). Don't start it unless the developer asks.
- Firefly Catch is on hold: don't tune it.
- Tuning for close matches: race every player to the finish in a throwaway test and compare the runner-up with the winner. Check per-seat wins over a few thousand runs: a 2-point seat bias hides in 400 runs. Past causes of seat bias: Lantern Tower's bots all gusting the same tower on the same tick; Sky Lanterns' bots pressing on alternate ticks, and where the sights started.
- Bots that need fresh presses: don't press only on even ticks (it favours one tick parity). Press when the action is possible.
- Seed each bot's per-action choices from `state.seed`, the seat and a counter in the state (throws, drops), so the bot stays a pure function and keeps one plan per action (Lantern Tower, Lantern Derby).
- Views: an orthographic straight-on camera, with a test that pins it, keeps every seat's judgement fair when aiming or timing against something on screen (Lantern Tower, Sky Lanterns, Lantern Derby).
- Possible later follow-ups (not roadmap items yet): a pause menu on the board (back to the title), more save slots.

## How to test saves

- Couch: `./target/debug/game --board --save FILE --rounds 3 --humans 0 --seed 1` saves at each turn start (`saved the game in round N, checksum X`). Then `./target/debug/game --save FILE` and press Space (Continue), Space (join), Enter (start): look for `resumed the saved game in round N, checksum X`.
- Online: `--host --resume --save FILE` and `--join 127.0.0.1:7777`; the client logs `joined the host's board at round N, checksum X`.
- `--resume` without `--host` skips to the couch join screen with the save.
- Never run dev windows without `--save` from the title: they use the real save in `~/.local/share/party-game/board.save`.

## How to test online on one PC

- Two windows: `cargo build -p game`, then `./target/debug/game --host --rounds 2` and `./target/debug/game --join 127.0.0.1:7777` (`--net-delay 30`, `--net-loss 5` for a bad link). Headless: wrap each in `timeout -k 10 <secs> gamescope --backend headless -w 1280 -h 800 -W 1280 -H 800 -- ...`, start the host about 2 s before the client, and script players with `--press SECS:KEY` / `--press-every SECS:Space`.
- Item 7's kill-and-restart recipe and item 6's scripted pad are in `git log -p docs/HANDOFF.md`.

## Gotchas

- Screenshots: `--format duel` with the default `--players 4` is the duel with two benched seats. `--humans 1` holds the instruction card up, which is how to screenshot it.
- Under gamescope the game runs as `./target/debug/game` (a relative path), so `pgrep -f "^$PWD/target/debug/game"` doesn't find it. Use `pgrep -af "target/debug/gam[e]"` (the bracket keeps the pattern from matching your own shell).
- Don't `pkill -f target/debug/game` from the Bash tool: the pattern matches the tool's own shell command.
- The Bash tool runs bash, though the developer's shell is fish.
- bincode isn't self-describing: change anything inside `Game`, `Board` or `Save` and `the_encoding_is_pinned` fails. Bump `SAVE_VERSION`, then re-pin.

## Decisions

- One save slot, written automatically between turns by the couch or the online host; removed at game over.
- A resumed game's seats keep their characters; players pick which seat with left/right.
- Rejoining after a disconnect is by position, not identity (item 7).
