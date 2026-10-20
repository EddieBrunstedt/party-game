# Handoff: session 2026-10-20 (late)

Briefing for the next session. Earlier sessions' notes are in git history (`git log -p docs/HANDOFF.md`).

## Done this session

- **Milestone 2 was broken down with the developer** (ROADMAP, 9 items). Items 1–4 are ticked, and item 5 is mostly built:
  1. `net` crate: `Transport` (UDP, in-memory, `Faulty` fault injection), `Endpoint` (version handshake, reliable ordered channel, goodbyes, `ggrs` socket).
  2. Rollback driver `net::rollback::Rollback` plus `net::check::play_online` and `tools net-check <minigame>`. The `usize` hashing fix is in `StableHasher`.
  3. Online lobby: shared lobby rules in `net::lobby` (the couch is a host alone), `net::session::{Host, Client}`, connect screen, `--host` / `--join`.
  4. Board protocol: `net::board`, where the host applies everything and clients follow with a checksum per update.
  5. **Online minigames (in progress)**: see ROADMAP item 5's notes for what's built and what's left (verify free play online in two windows, then tick).
- `docs/MINIGAME_IDEAS.md`: the developer's friends' minigame ideas (racing, tower stack, puzzles, hide and seek, quiz, musical chairs), in their own words with fit notes. Linked from DESIGN §3.3 and Milestone 3.

## State of the tree

Everything is committed at the end of the session; nothing is pushed since the developer's push before item 3 (`git log origin/main..main`). Pipeline #12 (the developer's push) passed every test. It rebuilt Bevy from scratch because `Cargo.lock` changed, which takes about 45 minutes on the shared runner; its clippy step was still running.

## How to test online on one PC

- Two windows: `cargo build -p game`, then run `./target/debug/game --host --rounds 2` and `./target/debug/game --join 127.0.0.1:7777` (add `--net-delay 30`, `--net-loss 5` to feel a bad link). Headless, wrap each in `timeout -k 10 <secs> gamescope --backend headless -w 1280 -h 800 -W 1280 -H 800 -- ...` and script players with `--press SECS:KEY` / `--press-every SECS:Space`; start the host about 2 s before the client.
- Logs to compare across machines: `over online at tick N: [...]` (each minigame's confirmed end), `board game over, checksum ...`, and any `out of step` / `minigame out of step` (desyncs).
- Headless checks: `cargo test -p net` (transport, session, board, rollback over lossy networks) and `cargo run -p tools -- net-check <minigame>`.

## Decisions

- Our own reliability layer over UDP, so board messages and `ggrs` share one socket and port (7777).
- Minigames are a full mesh; the board goes through the host. Friends need Tailscale or ZeroTier (explained to the developer).
- The handshake compares `CARGO_PKG_VERSION` plus `net::PROTOCOL_VERSION` (now 4). Bump the protocol version whenever a message changes.
- Bots run only on the host; other machines see them as `Controller::Remote { bot: true }`.

## Gotchas

- `cargo fmt` reflows code, so literal search-and-replace scripts keep missing it. Use the Edit tool, or match whitespace loosely.
- `ggrs` 0.13 doesn't error at the prediction limit: `advance_frame` returns the rollback's requests without a new tick.
- Bevy `Message`s written in `PreUpdate` (`OnlineEvent`) are read by each screen's systems with their own `MessageReader`; one poll per frame serves everyone.

## Next

1. Finish ROADMAP item 5: try free play online in two windows (title → Play online → Host free play; the client joins; the host picks a minigame; A replays; B goes back to the picker), fix what breaks, then tick it with notes.
2. Then item 6 (pause and controller loss online), 7 (disconnects and reconnect: start by ending minigame sessions deliberately, see the `Interrupted(host)` note), 8 (save and resume), 9 (online playtest with a friend).
