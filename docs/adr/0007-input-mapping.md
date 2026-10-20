# 0007: Keep our own small input mapper for the vertical slice

**Status:** Accepted (2026-10-20), made overnight without the developer; worth a second look.

## Context

DESIGN.md §4.7 lists `leafwing-input-manager` and `bevy_enhanced_input` as candidates, to be chosen during the vertical slice. What the game actually needs from input:

- The simulation consumes one quantized `sim_core::Input` per seat per tick (a stick and a few buttons). That struct is also the only thing sent over the network (ADR 0002).
- Joining is device-level: whichever controller presses A takes the next seat and stays bound to it (§3.4).
- Bots are virtual controllers that produce the same `Input` (§3.4).
- Every minigame uses the same stick plus A/B/X/Start, so there are no per-game action sets yet.
- No rebinding UI is planned for the vertical slice.

Both crates map devices to named actions per player. We would immediately flatten those actions back into `Input`, so the mapping layer would add a dependency and a Bevy-version coupling (ADR 0005), and save us nothing.

## Decision

Keep the mapper in `game/src/runner.rs` (about 150 lines). It turns a seat's bound devices into an `Input`, latches presses between ticks, and exposes per-frame `SeatInputs` for turn-based screens. Seats bind a keyboard and/or one gamepad entity; the join screen does the binding.

Revisit when one of these arrives: remappable controls, per-minigame action sets, or a menu system that would rather consume actions than raw `Input`. `bevy_enhanced_input` is the first candidate to evaluate then, because its context-based actions fit menus and minigames swapping in and out.

## Consequences

- No input crate in the dependency tree, so Bevy upgrades don't wait on one.
- Gamepad button mapping is a hand-written table. Unusual controllers may need entries added, and `--log-input` exists to diagnose that.
- Keyboard bindings (Space/J/K/L/Enter, WASD/arrows) are fixed until rebinding exists.
