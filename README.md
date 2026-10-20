# Party Game

A 2–4 player board-and-minigame party game in Rust + Bevy. See [docs/DESIGN.md](docs/DESIGN.md) and [docs/adr/](docs/adr/).

## Running

```sh
# Free-play a minigame: 1 human (keyboard Space/J = A, Enter/Esc = Start, or gamepad 1) vs 3 bots
cargo run -p game

# Options: --players 2..4, --humans N (seat N uses gamepad N), --bots easy|normal|hard,
#          --seed N, --pause-on-controller-lost
cargo run -p game -- --players 4 --humans 2 --bots hard

# Faster incremental builds while iterating
cargo run -p game --features dev
```

F3 toggles the frame-time overlay.

## Checking

```sh
cargo test --workspace                          # unit tests + every minigame's quality bars
cargo run -p tools -- check quick-draw --seeds 64
cargo run -p tools -- bots quick-draw --runs 500 --difficulty hard
```
