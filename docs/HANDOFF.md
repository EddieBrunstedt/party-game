# Handoff: session 2026-10-20 (evening)

Briefing for the next session. Earlier sessions' notes are in git history (`git log -p docs/HANDOFF.md`).

## Done this session

- **Four lantern-folk characters**, ticked; the developer's verdict was "Looks good". `art/lantern_folk.py` builds them in Blender; `game/src/characters.rs` plays a `Pose` per character.
- **Polish pass 1, first pass** (not ticked: it needs a human playtest). Synthesized sound for menus, the card and countdown, Quick Draw, Lantern Push, the board and results (`sfx.rs`, `audio.rs`), plus night ambience. Particle bursts on the key events in Quick Draw, Lantern Push and the board. Details are in ROADMAP under Polish pass 1.
- CI now pulls `crates/game/assets/` from LFS, because a test reads the `.glb` files. Not yet seen running on GitLab; nothing has been pushed this session.

## State of the tree

Committed.

## Decisions

- Every sound is a recipe in code for now, with no audio files: tweaking is editing numbers, and `--dump-sfx <dir>` writes WAVs to listen to. CC0 packs can replace individual sounds later (bevy_kira_audio's `wav` feature is on).
- Minigame sounds and effects come from diffing `ActiveRun::ticked()` in `MinigameSet::React`, never from per-frame view code, so they fire exactly once per event.
- Firefly Catch is still **on hold**. It only gets the shared card and countdown sounds.

## Next

A **human playtest**: listen to the sounds (`cargo run -p game -- --dump-sfx /tmp/sfx`, or just play), then a full board game with humans (`cargo run -p game`). Fix what they find, then tick Polish pass 1. After that the roadmap moves on to Milestone 2 (online), which isn't broken down yet.

## Useful to know

- Spectrograms are a quick check of a sound's shape: `ffmpeg -i x.wav -lavfi showspectrumpic=s=320x160:legend=0:fscale=log x.png`.
- `ls -v` sorts `shot-2.png` before `shot-10.png` for montages; a plain glob doesn't.
- Rendering pose sheets in Blender, the scripted board session flags and `/tmp` quota notes are in the previous handoff (`git log -p docs/HANDOFF.md`).
- Still open: 12 bot rounds take about 22 minutes, so the 30–40 minute target may need 15. Board bots hash `usize` lengths, which would desync with 32-bit peers.
