# Handoff: session 2026-10-21 (next: minigames)

Briefing for the next session. Earlier sessions' notes are in git history (`git log -p docs/HANDOFF.md`).

## Done this session

- Committed Musical Stools (`8bf4f10`), which the previous session left uncommitted after its tests and clippy passed.
- **Hedge Maze** built to playable, ticked and committed. A race to a beacon through a dark maze where lit hedges stay lit for everyone, 3 mazes for points; the 1v3 solo is a ghost whose touch sends runners back to their corner. The developer chose all four rule options offered (dark with a shared reveal, 3 mazes for points, a hunting ghost, A as a once-per-maze flare). Rules, tuning, the bots report and surprises are under its item in `docs/ROADMAP.md`; DESIGN.md §3.3 has its entry.

Notes for Hedge Maze, if it needs tuning after a playtest:
- Bot tuning knobs are `bot::skill` in the crate (delay, noise, flare_dark, fear, urgency, intercepts). 1v3 balance knobs are `GHOST_SPEED`, `RESPAWN_TICKS`, `GHOST_RACE_TICKS` and `TRIO_NEEDED` (the card's text and an `intro.rs` test pin 2 and 20 s).
- The generator's knobs are `NEWEST_PERCENT`, `MIN_PATH`, `CANDIDATES` and `LOOPS`; a test checks every corner is equally far from the beacon and the maze is the same turned a quarter.
- An ignored probe test is the quickest way to tune: run bot matches in a loop and print a statistic.
- The view keeps every possible hedge slot as an entity (about 480) and shows the ones that are hedges in this maze and next to a lit cell, so new mazes need no respawning.

## How to test saves

- Couch: `./target/debug/game --board --save FILE --rounds 3 --humans 0 --seed 1` saves at each turn start (`saved the game in round N, checksum X`). Then `./target/debug/game --save FILE` and press Space (Continue), Space (join), Enter (start): look for `resumed the saved game in round N, checksum X`.
- Online: `--host --resume --save FILE` and `--join 127.0.0.1:7777`; the client logs `joined the host's board at round N, checksum X`.
- `--resume` without `--host` skips to the couch join screen with the save.
- Never run dev windows without `--save` from the title: they use the real save in `~/.local/share/party-game/board.save`.

## How to test online on one PC

- Two windows: `cargo build -p game`, then `./target/debug/game --host --rounds 2` and `./target/debug/game --join 127.0.0.1:7777` (`--net-delay 30`, `--net-loss 5` for a bad link). Headless: wrap each in `timeout -k 10 <secs> gamescope --backend headless -w 1280 -h 800 -W 1280 -H 800 -- ...`, start the host about 2 s before the client, and script players with `--press SECS:KEY` / `--press-every SECS:Space`.
- Item 7's kill-and-restart recipe and item 6's scripted pad are in `git log -p docs/HANDOFF.md`.

## Gotchas

- Under gamescope the game runs as `./target/debug/game` (a relative path), so `pgrep -f "^$PWD/target/debug/game"` doesn't find it. Use `pgrep -af "target/debug/gam[e]"` (the bracket keeps the pattern from matching your own shell).
- Don't `pkill -f target/debug/game` from the Bash tool: the pattern matches the tool's own shell command.
- bincode isn't self-describing: change anything inside `Game`, `Board` or `Save` and `the_encoding_is_pinned` fails. Bump `SAVE_VERSION`, then re-pin.

## Decisions

- One save slot, written automatically between turns by the couch or the online host; removed at game over.
- A resumed game's seats keep their characters; players pick which seat with left/right.
- Rejoining after a disconnect is by position, not identity (item 7).

## Waiting on the developer's playtests

- Lantern Lottery: playtest 1 is done (see its item in `docs/ROADMAP.md`). The d-pad bug it found is fixed for every minigame (a pressed d-pad now wins over a stick resting off centre). Still open, for the developer to decide, not a roadmap item: never seeing your own pick felt hard to trust. Offered: you get the lantern you point at when you press A, a short rumble when the lock-in counts, and "point at a lantern first" if A is pressed without pointing; or rumble only; or leave it. Don't build any of these unless the developer asks.
- Sky Lanterns: see "Needs human playtest" under its item. The open question is whether a shooting gallery, where the better shooter wins almost every time, needs a catch-up twist.

- Musical Stools: see "Needs human playtest" under its item. One rule needs the developer's OK: in 1v3 the cat's **dash catches** a runner (out for the round). The agreed version (the cat only shoves) let the three fill both stools in 0.4 s, and the cat won 0–1% of matches; with catching it wins 48–53%. If the developer doesn't like catching, the other levers that were tried (a wider fence, catching on any touch) are in the item's decisions.

- Hedge Maze: see "Needs human playtest" under its item (stick feel in the corridors, whether the shared light makes following too strong, the ghost's speed and 20 s limit). Open, for the developer to decide: equal bots often tie because the maze is symmetric; a tie-break (e.g. total finishing time) would cut that, but ties share the win in every other minigame. Don't build it unless asked.

### Lantern Tower's camera (fixed)

Playtest 1 found the middle towers easier to line up (the perspective camera saw the outer ones from the side). The view now uses an orthographic camera; see the Lantern Tower item in `docs/ROADMAP.md`. Lantern Tower is committed. If the developer's next playtest still finds a seat harder, the drop guides (thin light lines up from the top floor's edges) are the next thing to try.

## Next: add minigames

The developer wants to build minigames for a while, and leaves the choice of which to the session. "Let's add some minigames" or "continue" both mean: take the **Next minigame** item in `docs/ROADMAP.md` (Milestone 3, Minigames). The steps for adding one and what to prefer are written there; the ideas are in `docs/MINIGAME_IDEAS.md`.

- Milestone 2 is done apart from the online playtest, which is **deferred** (not a checkbox, so "continue" skips it). Don't start it unless the developer asks.
- Firefly Catch is on hold: don't tune it.
- Tuning a minigame for close matches: race every player to the finish (ignore the outcome) in a throwaway test and compare the runner-up's finishing time with the winner's; it's finer than counting steps or floors. Check per-seat wins too, over a few thousand runs (`tools bots <id> --runs 4000`): a 2-point seat bias hides in 400 runs. Lantern Tower's bots once had one from all gusting the same tower on the same tick; Sky Lanterns' from bots pressing on alternate ticks, and from where the sights started.
- Bots that need fresh presses: don't press only on even ticks (it favours one tick parity). Press when the action is possible; if the action starts a cooldown, the next tick's release comes for free.
- Every new minigame must also pass `tools net-check <id>`, since the game plays online now.
- Possible later follow-ups (not roadmap items yet): a pause menu on the board (back to the title), more save slots.
