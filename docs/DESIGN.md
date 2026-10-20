# Party Game: Design

Working codename for internal use only: `lantern`. The display title lives in one constant, and crates use generic names. Nothing else in the codebase refers to the codename.

## 1. Vision

A Mario Party-style board-and-minigame party game for **2–4 players**, made in **Rust + Bevy** by one developer working with Claude.

- **Couch and online**, including mixed sessions where one machine has several local players and other players are remote.
- **Look:** a toy-like miniature world at dusk or night, built from chunky "voxel-ish" shapes. It gets its beauty from lighting: lanterns, neon, fireflies, emissive materials, HDR bloom, fog and GPU particles.
- **Goal:** a Steam release, planned so that each milestone is a satisfying stopping point on its own.

## 2. Platforms and performance

- Windows, Linux and Steam Deck. macOS only if it's cheap. No web or consoles.
- **Performance floor: 60 fps on Steam Deck at 1280×800.** We don't have a Deck yet, so we enforce this with budgets:
  - an in-game frame-time overlay
  - a "Deck" quality preset at 1280×800
  - a target of about 2–3 ms GPU frame time on the dev RTX 2080 Ti in that preset (roughly 8× the Deck's GPU)
  - content limits: ≤2 shadow-casting lights per scene; lanterns are emissive plus *unshadowed* point lights with a capped count; capped particle counts and draw calls
- Test on a real Deck before launch.

## 3. Game rules

### 3.1 Vertical-slice ruleset (close to Mario Party)

Placeholder names "Coins" and "Stars" will be reskinned later.

1. **Turn order:** each player rolls once at the start.
2. **Each turn:** roll 1–10, move along the board graph, choose a path at forks.
3. **Blue space:** +3 coins. **Red space:** −3 coins.
4. **The Star** sits on one space. Passing it lets you buy it for 20 coins, and then it moves to a new space.
5. **After every round, a minigame.** The format comes from the colour of the space each player is standing on:
   - all four on the same colour → free-for-all
   - three and one → 1v3
   - two and two → 2v2

   Winners get +10 coins.
6. **Winner:** most stars, with coins as the tiebreak.

The number of turns is configurable. The default is tuned to a **~30–40 minute** game.

### 3.2 After the vertical slice

Items, shops, event spaces, bonus stars, last-5-turns events, **duels (1v1, needed at launch)** and **battles**. In a battle every player pays into a pot, and placement in a free-for-all minigame decides the split.

### 3.3 Minigames

- **Formats:** free-for-all, 1v3, 2v2 and duel at launch; battle later. Each minigame declares which formats it supports.
- **One shared screen, always.** There's no split-screen. Online players see the same framing as the couch.
- **Inputs:** a stick plus 2–3 buttons, so every minigame plays on any controller.
- **Vertical-slice minigames:**
  - **Lantern Push:** sumo knockoff on a shrinking platform. Proves physics, knockback and rollback. A dashes into rivals, B braces (heavy for a moment, so a dash bounces off). The platform shrinks from 8 s to 45 s; last player or team standing wins, and at the 75 s limit survivors share the win. In 1v3 the solo is bigger and heavier.
  - **Firefly Catch:** chase fleeing fireflies. Proves spawning, simple AI-driven objects, particles and scoring.
  - **Quick Draw:** press when the lantern lights. Proves reflex/timing games and is the first minigame used to build the pipeline.
- **Free-play mode:** play any minigame directly from a menu. The same entry point is available from the CLI for development, with N bots.

### 3.4 Players, bots and controllers

- **One controller per player.** The game is designed for gamepads; keyboard input is for development only.
- **Joining:** on the join screen, each controller that presses A takes the next seat and stays bound to it. Online, each machine joins its local controllers the same way.
- **Bots** fill empty seats, set by the host, with 2–3 difficulty levels. A bot is a **virtual controller**: a pure function `bot_input(state, seat, difficulty) -> Input` whose output is sent exactly like a human's. The simulation never knows a seat is a bot.
- **Controller disconnects** are local events and never break the network session. The setting "Pause when a controller disconnects" defaults to **Off**:
  - **Off:** that seat's input goes neutral and a "🎮✕ P2" badge appears.
  - **On:** the minigame pauses for everyone until the controller is back, then resumes. It never restarts.

  The board phase simply waits for the player whose turn it is.
- **Telling players apart:** each player has a colour **plus** a number and shape (P1 ●, P2 ▲, P3 ■, P4 ◆).

### 3.5 Characters and customization

- **Fixed roster:** 4 characters in the vertical slice, 6–8 at launch.
- **One shared body plan:**
  - the same rig and skeleton, so animations are made once
  - the same proportions: chunky, rounded, stubby limbs
  - the same hitbox in the simulation
- **Concept: "lantern folk".** One species, and each character's personality is its *light source*: a paper lantern, a jar of fireflies, a candle flame, a neon ring, and so on. An emissive accent in the player colour makes everyone easy to spot.
- **Customization (not early):** attachable cosmetics on standard sockets (head, back, hand) and a personal glow colour. Unlocks come later. Cosmetics never change the hitbox and never affect the simulation.

## 4. Architecture

### 4.1 Workspace

```
crates/
  sim-core/     # no Bevy. Math wrapper (libm), portable hasher, seeded RNG, Minigame trait, Run wrapper, input types
  sim-check/    # no Bevy. The automated quality bars: replay, ggrs sync test, bot runs
  sim-board/    # (planned) no Bevy. Board graph (RON), turn state machine, rules, board bots
  minigames/*   # no Bevy. One crate per minigame: State: Clone + Hash, step(), bot_input()
  net/          # (planned) transport layer (UDP now, Steam later), ggrs driver, board-phase protocol
  game/         # Bevy 0.20: rendering, interpolation, UI, audio, input, menus
  tools/        # headless CLI: `check` (all quality bars), `bots` (bot-only matches)
```

### 4.2 Simulation outside Bevy ([ADR 0001](adr/0001-simulation-outside-bevy.md))

All game logic is plain Rust in the `sim-*` and `minigames/*` crates. Bevy only **reads** that state to render, animate and spawn effects.

```rust
pub trait Minigame {
    type State: Clone + Hash + Send + Sync;
    const ID: &'static str;
    const SUPPORTED_FORMATS: &'static [FormatKind];
    const MAX_TICKS: u32;                                    // headless runs fail past this
    fn new(seed: u64, players: PlayerSetup, format: Format) -> Self::State;
    fn step(state: &mut Self::State, inputs: &Inputs);      // fixed 60 Hz
    fn outcome(state: &Self::State) -> Option<Outcome>;
    fn bot_input(state: &Self::State, seat: Seat, difficulty: Difficulty) -> Input;
}
```

`Inputs` carries this tick's and last tick's input per seat, so "just pressed" is part of the simulation. A minigame never runs on its own: `Run<M>` wraps its state with what every minigame shares (input edges, pause-as-input, controller-loss halting), and `Run<M>` is the unit that rollback snapshots and checksums.

Determinism rules for simulation code:
- no `HashMap` iteration
- no wall-clock time
- no thread-local or global RNG; the seeded RNG is part of the state
- transcendental math (sin, cos, pow, …) goes through the `sim-core` wrapper, which uses `libm`
- no `target-cpu=native`

### 4.3 Two sync models ([ADR 0003](adr/0003-two-sync-models.md))

| Phase | Model | Details |
|---|---|---|
| Board | Host-authoritative, event-driven | Clients send intents (roll, pick path, buy star). The host applies them and broadcasts results. Dice use a seeded RNG on the host. |
| Minigame | Deterministic rollback over inputs only ([ADR 0002](adr/0002-rollback-with-ggrs-core.md)) | `ggrs` drives `Minigame::step`. Save = `clone`, load = restore, checksum = hash. 60 Hz, 2 frames input delay, max 8 frames of rollback. The host hands each minigame its seed, players and format. |

- **Pause is an input.** All peers see it on the same frame, and the simulation stops advancing game time while paused.
- **Physics:** custom and simple (circles, boxes, heightfields, gravity, knockback), in `sim_core::physics`: a `Vec2` floor plane (x right, y towards the camera; the renderer maps it to world `(x, 0, y)`), `Body` circles with velocities in units per second, and pairwise collisions resolved in index order.
- **Teams:** `Format::team(seat)` gives each seat's side, and `Outcome::from_team_scores` ranks whole teams. `check_all` runs every format a minigame declares. A minigame that really needs rigid-body physics may use `avian` with `enhanced-determinism`, behind the same trait. See [ADR 0006](adr/0006-deterministic-floats-and-custom-physics.md).

### 4.4 Online ([ADR 0004](adr/0004-transport-layer.md))

- **Friends only**, by invite or code. No public matchmaking.
- **Our own transport layer.** Plain UDP comes first: localhost for development, and direct IP over Tailscale or ZeroTier for playing with friends before Steam. A Steam backend (lobbies and invites, SteamNetworkingMessages via a small ggrs `NonBlockingSocket` adapter) slots in behind the same layer if the game ships on Steam.
- **Network disconnects:**
  - **Board phase:** pause with "Waiting for P3…". On reconnect, the host sends a full state snapshot. After ~2 minutes, the host gets options: *keep waiting*, *end and save*, or, as a last resort chosen by the host, *bot takes over*.
  - **Minigame:** the minigame stops and **restarts from the beginning with the same seed** once the player is back.
  - **Host leaves:** the session ends. There's no host migration.
- **Saves:** whole board games can be saved and resumed, between turns only. Board state is a plain serializable struct.

### 4.5 Rendering and look

- HDR camera, `Bloom`, TonyMcMapface or AgX tonemapping, emissive materials, volumetric fog, SSAO on desktop presets, and `bevy_hanabi` GPU particles.
- **Board camera:** follows the active player. **Minigame cameras:** a single arena view.
- Rendering interpolates between simulation ticks: the runner keeps the state from one tick back, and views blend by `Time<Fixed>::overstep_fraction()`.
- Shared placeholder pieces (night camera, lantern folk, fireflies, paper lanterns) live in `game/src/scenery.rs`.

### 4.6 Content pipeline

- **3D:** Blender, which Claude drives through its MCP, plus procedural generation. Shapes are loose "voxel-ish": whatever is easy to build in Blender, with chunky forms and bevelled edges. Export as glTF (`.glb`), and use `bevy_skein` for components on Blender objects.
- **Boards:** the space graph is a **RON file**, the source of truth that the simulation loads without Bevy. The Blender scene is decoration around the graph's coordinates. A script can push the graph points into Blender as guides, and an in-game debug view draws the graph.
- **Audio:**
  - royalty-free/CC0 packs, and procedural SFX made in code (sfxr-style)
  - later experiment: music made in Reaper with your VSTs
  - playback through `bevy_kira_audio`
  - check Steam's rules on AI-generated content before using any
- **Localization:** English only at launch, but every UI string goes through Fluent keys from day one.

### 4.7 Dependencies (as of 2026-10-20)

| Purpose | Crate | Note |
|---|---|---|
| Engine | bevy 0.20 | Start on the latest ([ADR 0005](adr/0005-bevy-0-20.md)) |
| Rollback | ggrs 0.13 | Core only, no `bevy_ggrs` |
| Particles | bevy_hanabi | Use git main until the 0.20 release |
| Blender components | bevy_skein | Use git main until the 0.20 release. **Not** the crates.io `skein` crate, which is an unrelated hash crate |
| Input | leafwing-input-manager 0.22 or bevy_enhanced_input | Pick during the vertical slice |
| Audio | bevy_kira_audio 0.27 | |
| Steam (later) | steamworks 0.13 | Used directly, not through bevy-steamworks |
| Avoid | matchbox (stalled, incompatible with ggrs 0.13), Blenvy (abandoned) | |

## 5. Quality bars

A minigame is **done** when it passes all three:
1. **Determinism test:** the same input stream, run twice and on two simulated peers, gives matching state checksums on every frame.
2. **Headless bot run:** 4 bots finish the game with no panics.
3. **Manual feel check:** the developer plays it.

(1) and (2) are one call, `sim_check::check_all::<M>(seeds)`. Every minigame crate runs it as a unit test, and `cargo run -p tools -- check <minigame>` runs it from the CLI. They will also run in CI on GitLab.

## 6. Milestones

1. **Vertical slice:** 1 small board, the 3 minigames above, 4 characters, couch play plus bots, the Deck quality preset. The architecture is online-ready from day one.
2. **Online alpha:** the same content over the UDP transport, with reconnects and saves.
3. **Launch:** 3 boards, ~30 minigames, 6–8 characters, all formats including duels, cosmetics, and the Steam backend if it ships there.

## 7. Repository

- git, with **Git LFS** for `.blend`, `.glb`, audio and Reaper projects.
- Both `.blend` sources and exported `.glb` files are committed, so the game builds without Blender.
- Remote: a private GitLab repo.

## 8. Open and deferred

- the final title and the names for the reskinned currency and stars
- what the signature twist will be, if any
- the details of battles, items and shops
- whether a composer is hired for launch
- the input crate choice
- the unlock and progression system
