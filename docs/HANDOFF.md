# Handoff: session 2026-10-20 (late)

Briefing for the next session. Earlier sessions' notes are in git history (`git log -p docs/HANDOFF.md`).

## Done this session: ROADMAP Milestone 2 item 6 (pause and controller loss online), ticked

Not committed yet. Clippy is clean across the workspace and `cargo test -p game -p net -p tools` passes. CI pipelines #13 (`ed29235`) and #14 (`0d4ae66`) both passed.

- The pause setting travels with the lobby (host-only, Y / I on the join screen), and every launch uses `runner::RunRules`. `PROTOCOL_VERSION` 5.
- Latched taps survive frames where rollback takes no input.
- `tools fake-pad`: a scripted virtual gamepad (uinput) for windowed runs.
- Two-window runs (setting On and Off) showed a pulled pad and a pause on both machines. Details and evidence are under item 6 in the roadmap. The run script is below.

## How to test online on one PC

- Two windows: `cargo build -p game`, then run `./target/debug/game --host --rounds 2` and `./target/debug/game --join 127.0.0.1:7777` (add `--net-delay 30`, `--net-loss 5` to feel a bad link). Headless, wrap each in `timeout -k 10 <secs> gamescope --backend headless -w 1280 -h 800 -W 1280 -H 800 -- ...` and script players with `--press SECS:KEY` / `--press-every SECS:Space`; start the host about 2 s before the client.
- Free play online has no shortcut flag. From the title the host presses `ArrowDown ArrowDown Space ArrowDown Space` (Play online → Host free play), Space to join, then Enter to start and Space to launch from the picker. Space is A (replay) and K is B (back to the picker).
- Logs to compare across machines: `over online at tick N: [...]` (each minigame's confirmed end), `board game over, checksum ...`, and any `out of step` / `minigame out of step` (desyncs).
- A scripted pad: `./target/debug/tools fake-pad --at 8:A --at 18:Start --at 28:unplug --at 33:plug --until 50` (Linux, the user is in `input`). Every window sees it, so let it join the host before the client's join screen is up, and have the client join with the keyboard. Only joined devices change settings or press Start, so the pad presses Y and Start on the host. Item 6's run: host free play from the title (`--press 3:ArrowDown --press 3.5:ArrowDown --press 4:Space --press 5:ArrowDown --press 5.5:Space`); the pad does A 8, Y 9 (for On), Start 18, A 20 (launch), A 23 (ready), unplug 28, plug 33. The client starts at 10 with `--press 5:Space --press 13:Space --press 26:Enter --press 30:Enter`.
- Headless checks: `cargo test -p net` (transport, session, board, rollback over lossy networks) and `cargo run -p tools -- net-check <minigame>`.

## Decisions

- Our own reliability layer over UDP, so board messages and `ggrs` share one socket and port (7777).
- Minigames are a full mesh; the board goes through the host. Friends need Tailscale or ZeroTier (explained to the developer).
- The handshake compares `CARGO_PKG_VERSION` plus `net::PROTOCOL_VERSION` (now 5). Bump the protocol version whenever a message changes.
- Bots run only on the host; other machines see them as `Controller::Remote { bot: true }`.

## Gotchas

- `cargo fmt` reflows code, so literal search-and-replace scripts keep missing it. Use the Edit tool, or match whitespace loosely.
- `ggrs` 0.13 doesn't error at the prediction limit: `advance_frame` returns the rollback's requests without a new tick.
- Bevy `Message`s written in `PreUpdate` (`OnlineEvent`) are read by each screen's systems with their own `MessageReader`; one poll per frame serves everyone.

- Free play's picker seeds from the wall clock, so a scripted two-window run gets a different match length each time (Quick Draw took 27–41 s plus the card). Leave wide gaps between scripted presses, or one lands mid-match.

## Next

1. Item 7 (disconnects and reconnect: start by ending minigame sessions deliberately, see the `Interrupted(host)` note under item 5), 8 (save and resume), 9 (online playtest with a friend).
