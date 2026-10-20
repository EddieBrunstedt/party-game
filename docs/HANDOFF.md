# Handoff: session 2026-10-20 (evening)

Briefing for the next session. Earlier sessions' notes are in git history (`git log -p docs/HANDOFF.md`).

## Done this session

- **ROADMAP Milestone 2 item 5 (online minigames) is ticked.** I checked free play online in two windows: host free play, launch, replay with A, B back to the picker, launch again. Every step reached both machines, and both agreed on each match's confirmed end. See the item's notes.
- Fixed client-side text that only made sense on the host: "Rounds" on the free play join screen, the picker's controls hint (now "B: leave"), and the result prompt (now "The host chooses what's next", `ActiveRun::follows_host`). Three new Fluent keys in `locales/en/main.ftl`.
- DESIGN: the title's "Play online" page, online minigames and free play (§4.4), and the instruction card online (an online replay shows the card again, on purpose).
- CI: pipeline #12 finished green, clippy included. The developer then pushed through `ed29235`, and pipeline #13 was running at the end of the session.

## State of the tree

This session's changes are not committed yet (code fixes plus ROADMAP, DESIGN and HANDOFF). `cargo test -p game` and `cargo clippy --workspace --all-targets -- -D warnings` pass.

## How to test online on one PC

- Two windows: `cargo build -p game`, then run `./target/debug/game --host --rounds 2` and `./target/debug/game --join 127.0.0.1:7777` (add `--net-delay 30`, `--net-loss 5` to feel a bad link). Headless, wrap each in `timeout -k 10 <secs> gamescope --backend headless -w 1280 -h 800 -W 1280 -H 800 -- ...` and script players with `--press SECS:KEY` / `--press-every SECS:Space`; start the host about 2 s before the client.
- Free play online has no shortcut flag. From the title the host presses `ArrowDown ArrowDown Space ArrowDown Space` (Play online → Host free play), Space to join, then Enter to start and Space to launch from the picker. Space is A (replay) and K is B (back to the picker).
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

- Free play's picker seeds from the wall clock, so a scripted two-window run gets a different match length each time (Quick Draw took 27–41 s plus the card). Leave wide gaps between scripted presses, or one lands mid-match.

## Next

1. Item 6 (pause and controller loss online), then 7 (disconnects and reconnect: start by ending minigame sessions deliberately, see the `Interrupted(host)` note under item 5), 8 (save and resume), 9 (online playtest with a friend).
