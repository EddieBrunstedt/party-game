# Handoff: session 2026-10-20 (night)

Briefing for the next session. Earlier sessions' notes are in git history (`git log -p docs/HANDOFF.md`).

## Done this session: ROADMAP Milestone 2 item 7 (network disconnects and reconnect), ticked

**Not committed yet.** The working tree has all of item 7. Clippy is clean across the workspace (`-D warnings`), `cargo fmt` is applied, and `cargo test -p net -p game -p tools` passes. The full workspace test run was not run this session. Run `cargo test --workspace` first (there is no CI any more; see below), then commit (one commit, message in the style of `git log`).

What changed (details, evidence and known gaps are under item 7 in `docs/ROADMAP.md`; `docs/DESIGN.md` §4.4 is updated):

- `net/src/endpoint.rs`: keepalives (`Frame::Alive`), `NetEvent::Lost` after `PEER_TIMEOUT` (5 s), and a `Bye` in answer to traffic from forgotten peers. `Endpoint::silent_for`.
- `net/src/session.rs`: after the start, lost machines are *away* and keep their seats; a new connection takes an away machine's place (or a machine silent for 2 s); `Back` → `Start` again plus `Peers` for the re-mesh. Minigames end deliberately (`finish_minigame` → `MinigameSettled`), stop (`stop_minigame`) and restart (`restart_minigame`). `hand_to_bots`. `LobbyState::renumber_locals`. `PROTOCOL_VERSION` 6.
- `net/src/rollback.rs`: `ggrs` disconnect timeout 60 s, so the session (not `ggrs`) decides when a peer is gone.
- Game: `runner.rs` (`Net` phase enum in `ActiveRun`, `follow_session::<M>`), `online.rs` (`Waiting` resource, banner, the host's options, `follow_launches`, HostLost notice), `board_view.rs` (board holds while waiting, `held` intents, snapshot on `Back`, `leave_a_minigame_the_host_finished`), `menus.rs` (rejoin mode on the join screen, `ReactToOnline` set, picker holds while waiting), `main.rs` (`--wait-options-after SECS`), new strings in `locales/en/main.ftl`.
- New tests: `net/tests/reconnect.rs` (6 tests), and one in `transport.rs`.

## How to test online on one PC

- Two windows: `cargo build -p game`, then `./target/debug/game --host --rounds 2` and `./target/debug/game --join 127.0.0.1:7777` (`--net-delay 30`, `--net-loss 5` for a bad link). Headless: wrap each in `timeout -k 10 <secs> gamescope --backend headless -w 1280 -h 800 -W 1280 -H 800 -- ...`, start the host about 2 s before the client, and script players with `--press SECS:KEY` / `--press-every SECS:Space`.
- Item 7's kill-and-restart run (the scripts were in the session scratchpad and are gone; this is the recipe). Host: `--host --rounds 2 --press 3:Space --press 9:Enter --press-every 1.3:Space`. Client: `--join 127.0.0.1:7777 --press-every 1:Space` (Space also binds the keyboard when it rejoins). Wait for the client's log line `joined the host's board`, sleep 15, `pkill -KILL -f "^$PWD/target/debug/game --join"`, wait for the host's `waiting for machines`, start a new client; wait for its `Minigame` screen line, sleep 12, kill it again, wait for the host's `stopped: a machine dropped out`, start a third client, and wait for `board game over` on both. Compare `over online at tick N` and the final checksum. To kill the host instead, `pkill -KILL -f "^$PWD/target/debug/game --host"` and look for `Lost the connection to the host.` on the client.
- `--wait-options-after 5` makes the host's options (A keep waiting / X bots / B end) show after 5 s instead of 120.
- Logs to compare across machines: `over online at tick N`, `every machine has the end of <id>`, `board game over, checksum ...`, and any `out of step` / `minigame out of step`.
- Item 6's scripted pad (`tools fake-pad`) still works as described in `git log -p docs/HANDOFF.md`.

## Gotchas found this session

- **In a bash `until` loop helper, add `return 0`.** The loop returns the status of the last command in its body, so `wait_for ... || exit 1` quietly exited after succeeding and left the game windows running. Leftover hosts hold UDP 7777 and make the next run's results nonsense. Before a run, check that no game is running: `pgrep -af "^$PWD/target/debug/gam[e]"`.
- **Don't `pkill -f target/debug/game` from the Bash tool.** The pattern matches the tool's own shell command and kills it. Anchor it (`^/home/.../target/debug/gam[e]`).
- Ordering against a system that's registered more than once (`react_to_online` runs on two screens) panics when Bevy builds the schedule. Put it in a `SystemSet` (`menus::ReactToOnline`) and order against that.
- A PS5 controller was connected to the PC during the last runs; every window sees it (see item 6's notes).
- Headless gamescope occasionally starts a window that never renders a frame (seen once this session, when leftover processes were around). Kill everything and rerun.

## Decisions

- Our own reliability layer over UDP, so board messages and `ggrs` share one socket and port (7777).
- Minigames are a full mesh; the board goes through the host. Friends need Tailscale or ZeroTier.
- The handshake compares `CARGO_PKG_VERSION` plus `net::PROTOCOL_VERSION` (now 6). Bump the protocol version whenever a message changes.
- Bots run only on the host; other machines see them as `Controller::Remote { bot: true }`.
- Rejoining is by position, not identity: the next machine to join a started game takes the lowest away machine's seats. That's fine for friends-only play and needs no saved token.
- If the host already has a minigame's confirmed end when a machine drops out, the result stands. Otherwise the minigame restarts for everyone.
- "End the game" doesn't save yet; item 8 should make it "end and save" (the host's B option in `online::decide_about_the_absent`).

## Next

1. Run `cargo test --workspace` locally, then commit item 7. `.gitlab-ci.yml` was deleted on 2026-10-21 because CI was using up the GitLab compute quota; verification is local until release.
2. Optionally, check the host's options line on screen: run with `--wait-options-after 5` and no `--press-every` on the host (scripted A presses keep resetting the timer), and try X (bots take over) and B (end) in two windows.
3. Item 8 (save and resume: reuse the board snapshot; "end and save" from the waiting options), then 9 (online playtest with a friend).
