# 0003: Host-authoritative board phase, rollback minigames

**Status:** Accepted (2026-10-20)

## Context

The board phase is turn-based and full of long animations, menus, choices and saves. Minigames are real-time action. Rollback suits the second but adds nothing to the first, and it would make reconnecting and saving harder.

## Decision

- **Board phase:** host-authoritative and event-driven. Clients send intents (roll, pick a path, buy a star, and so on). The host validates them, applies them with a seeded RNG, and broadcasts the results. The host sends a full state snapshot to anyone who reconnects.
- **Minigame phase:** deterministic rollback (ADR 0002). The host hands each minigame its seed, player list and format, and receives its `Outcome`.
- Both phases follow the same pure-state style, so the board rules are also headless-testable.

## Consequences

- **Reconnecting in the board phase is cheap:** the game pauses, then resumes from one snapshot. After ~2 minutes the host can keep waiting, end and save, or choose a bot takeover as a last resort.
- **A peer lost mid-minigame** ends that rollback session. The minigame restarts from the beginning with the same seed when the player is back.
- **Saves happen between turns only**, as a serialized board state.
- **The host leaving ends the session.** There's no host migration.
