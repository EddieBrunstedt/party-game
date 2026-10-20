# Handoff: session 2026-10-20 (evening)

Briefing for the next session. Earlier sessions' notes are in git history (`git log -p docs/HANDOFF.md`).

## Done this session

- **Milestone 1 (vertical slice) is complete**: every item in ROADMAP is ticked.
- **Four lantern-folk characters**: `art/lantern_folk.py` builds them in Blender; `game/src/characters.rs` plays a `Pose` per character. The developer: "Looks good".
- **Polish pass 1**: synthesized sound for menus, the instruction card and countdown, Quick Draw, Lantern Push, the board and results (`sfx.rs`, `audio.rs`), plus night ambience. Particle bursts on the key events. The developer tested it: "lgtm".
- CI now pulls `crates/game/assets/` from LFS, because a test reads the `.glb` files.

## State of the tree

Committed, but **4 commits are not pushed** (`git log origin/main..main`). CI's LFS change hasn't run on GitLab yet, so check the first pipeline after pushing.

## Decisions

- Sounds are code recipes, with no audio files. `--dump-sfx <dir>` writes them as WAVs; CC0 packs can replace individual sounds later (bevy_kira_audio's `wav` feature is on).
- Minigame sounds and effects come from diffing `ActiveRun::ticked()` in `MinigameSet::React`, so they fire exactly once per event.
- The character script is the source of the models: edit `art/lantern_folk.py` and re-run it, rather than editing the `.blend`.
- Firefly Catch is still **on hold** (the developer may rethink or cut it). It has the shared card and countdown sounds, but no polish.

## Next

ROADMAP has no unticked item: **Milestone 2 (online alpha) needs breaking down first.** Read DESIGN.md and ADRs 0002–0004, then propose items in the roadmap's format: one per line, each with a **Done when**. In scope: the `net` crate (transport trait, UDP backend), a `ggrs` P2P driver for minigames, the host-authoritative board protocol, reconnect and pause flows, save/resume. Agree the breakdown with the developer before building.

Known online gaps to fold in:
- The instruction card's readiness is client-side only; DESIGN §3.4 says it travels as a board-protocol message online.
- Board bots hash `usize` lengths, which would desync with 32-bit peers.

## Useful to know

- Spectrograms check a sound's shape: `ffmpeg -i x.wav -lavfi showspectrumpic=s=320x160:legend=0:fscale=log x.png`.
- Use `ls -v` for screenshot montages: it puts `shot-2.png` before `shot-10.png`, which a plain glob doesn't.
- `--humans 0` minigame runs spend about 3 s on the instruction card. A Quick Draw match with seed 3 ends at about 60 s.
- `/tmp` has a per-user quota; keep scratch builds small.
- Still open from playtesting: 12 bot rounds take about 22 minutes, so the 30–40 minute target may need 15.
