# Handoff: session 2026-10-20 (afternoon)

Briefing for the next session. The overnight session's notes are in git history (`git log -p docs/HANDOFF.md`).

## Done this session

- **Firefly Catch readability** (playtest 1: "couldn't catch a single one"): the net stays open for 0.2 s (`SWING_TICKS`) and is bigger, still short of the hop distance (tested). Each player has a ground ring showing exactly where the net catches; each firefly has a ground spot that pulses while it's tired.
- **Shared minigame start** (playtest 2: rules "fly by in under a second"): `crates/game/src/intro.rs`. Every minigame opens with an instruction card that waits for every human's A (bots are ready at once; with no humans it starts after 3 s), then the same 3-2-1 countdown, `sim_core::COUNTDOWN_TICKS`, drawn from `Run::tick`. A minigame supplies only Fluent keys (`{id}-title`, `-goal`, `-how`, `-a`; optional `-b`, `-stick`, `-goal-teams`, `-goal-solo`), and a test checks the required ones. The developer says the ready screen "works great for now".
- **Blender MCP** is set up and connected: the official Blender Lab server in `~/blender_mcp` (venv, no `uv`), add-on v1.0.3 installed in Blender 5.2.2, registered in Claude Code at local scope. See CLAUDE.md.
- Evidence: workspace tests and clippy pass. Screenshots were checked for the card (1v3 with a human pressing A, 2v2 with bots only, Quick Draw card → countdown → play). A scripted board session went Title → Join → Board → 2 × (Minigame → Board) with no panics.

## State of the tree

All of the above is **uncommitted** (`git status`). Review and commit it first, unless the developer says otherwise.

## Decisions

- **Firefly Catch is on hold.** The developer finds it "not very fun" and may cut it. Don't tune or polish it until they decide. It stays in the board's pool for now.
- The countdown is now 3 s in every minigame (it used to be 2 s or 4 s), and the card replaced each minigame's own intro text.

## Next

**Four lantern-folk characters** (ROADMAP.md, first unticked item), now unblocked by the Blender MCP. Then Polish pass 1, which leaves Firefly Catch out, and needs a human playtest.

## Useful to know

- Scripted board session: `--press-every 1.5:Space --press 5:Enter`. Enter starts the game from the join screen; without it the run sits on Join.
- `--humans 0` screenshot runs now spend about 3 s on the instruction card before the countdown, so shift `--screenshot-at` times.
- `/tmp` has a per-user quota. The overnight scratchpad had grown to 13 GB of build output, and filling the quota breaks every shell command. Keep scratch builds small or delete them when done.
- Still open from the overnight session: 12 rounds took about 22 minutes with bots, so the 30–40 minute target may need 15. Board bots hash `usize` lengths, which is fine on x86_64 but would desync with 32-bit peers.
