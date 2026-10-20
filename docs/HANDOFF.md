# Handoff: session 2026-10-20 (late afternoon)

Briefing for the next session. Earlier sessions' notes are in git history (`git log -p docs/HANDOFF.md`).

## Done this session

- Committed the previous session's work (shared instruction card, Firefly Catch readability) after tests and clippy passed.
- **Four lantern-folk characters** (ROADMAP ticked): `art/lantern_folk.py` builds them in Blender on one rig with `idle`, `hop`, `celebrate` and `slump`, and exports `crates/game/assets/characters/*.glb`. `game/src/characters.rs` loads them, recolours `Body`/`Glow`/`Accent` per seat, moves the head light onto the head bone, and plays whatever `Pose` a view sets on the character's root. Every view now sets poses instead of bouncing the root in code.
- Small fixes: `assets/` is found when the binary runs outside cargo (gamescope); "P1 & P4 take the round".
- Evidence: workspace tests, clippy and fmt pass; screenshots of every scene; Deck GPU time unchanged (Quick Draw 0.55 ms).

## State of the tree

Committed. `.glb` and `.blend` go through Git LFS (`.gitattributes`); `art/*.blend1` is ignored.

## Decisions

- The script is the source of the characters: edit `art/lantern_folk.py` and re-run it, rather than editing the `.blend`. Through the MCP: `exec(compile(open(p).read(), p, "exec"), {"__file__": p, "__name__": "__main__"})`, with `result` set to a dict.
- Firefly Catch is still **on hold** (see ROADMAP); it only got the pose change.

## Next

**Polish pass 1** (ROADMAP). It needs a human playtest to finish, so ask the developer to look at the characters first (do they read at board distance, and do the animations have enough life?), then work through audio, particles and the polished definition for Quick Draw, Lantern Push and the board.

## Useful to know

- To look at the models in Blender, render a pose sheet: hide all but one character mesh, set `LanternFolk.animation_data.action` to a clip, call `frame_set`, and render with EEVEE. The four meshes share the rig and overlap at the origin.
- Scripted board session: `--press-every 1.5:Space --press 5:Enter`. `--humans 0` minigame runs spend about 3 s on the instruction card. A Quick Draw match with seed 3 ends at about 60 s.
- `/tmp` has a per-user quota; keep scratch builds small.
- Still open: 12 bot rounds take about 22 minutes, so the 30–40 minute target may need 15. Board bots hash `usize` lengths, which would desync with 32-bit peers.
