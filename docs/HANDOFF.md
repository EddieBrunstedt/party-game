# Handoff: session 2026-10-21 (next: minigames)

Briefing for the next session. Earlier sessions' notes are in git history (`git log -p docs/HANDOFF.md`).

## Done this session

- Lantern Echo (memory race, all four formats) built to playable and ticked; details and the playtest command are under its item in `docs/ROADMAP.md`. `tools bots` gained `--format` and `--p1 <level>` for format and mixed-difficulty reports. The d-pad now feeds a seat's stick.
- Item 7 (network disconnects and reconnect) committed after a full `cargo test --workspace` (`f637e07`).
- Item 8 (save and resume) built, checked and ticked. Details, evidence and gaps are under item 8 in `docs/ROADMAP.md`; `docs/DESIGN.md` §3.4 and §4.4 are updated. `PROTOCOL_VERSION` 7, `SAVE_VERSION` 1.

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

## Next: add minigames

The developer wants to build minigames for a while, and leaves the choice of which to the session. "Let's add some minigames" or "continue" both mean: take the **Next minigame** item in `docs/ROADMAP.md` (Milestone 3, Minigames). The steps for adding one and what to prefer are written there; the ideas are in `docs/MINIGAME_IDEAS.md`.

- Milestone 2 is done apart from the online playtest, which is **deferred** (not a checkbox, so "continue" skips it). Don't start it unless the developer asks.
- Firefly Catch is on hold: don't tune it.
- Every new minigame must also pass `tools net-check <id>`, since the game plays online now.
- Possible later follow-ups (not roadmap items yet): a pause menu on the board (back to the title), more save slots.
