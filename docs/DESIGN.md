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
  - an in-game frame-time overlay (F3) that shows GPU time from timestamp queries; `--gpu-report` prints average/p95/max and a per-pass breakdown at exit
  - a "Deck" quality preset at 1280×800 (F4 or Select toggles it, `--quality deck` starts in it): one shadow cascade at 1024², no point-light shadows, no MSAA, a shorter bloom chain
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
  - **Firefly Catch:** chase fleeing fireflies. Proves spawning, simple AI-driven objects, particles and scoring. A swings a net, B dashes. A startled firefly sprints and hops over its chaser, which tires it; tired fireflies are slow and easy to net. A ring on the ground in front of each player shows exactly where the net catches, and each firefly's ground spot pulses while it's tired. Golden fireflies are worth 3; in 1v3 the solo's catches count triple.
  - **Quick Draw:** press when the lantern lights. Proves reflex/timing games and is the first minigame used to build the pipeline.
- **Free-play mode:** play any minigame directly from a menu. The same entry point is available from the CLI for development, with N bots.

### 3.4 Players, bots and controllers

- **One controller per player.** The game is designed for gamepads; keyboard input is for development only.
- **Joining:** on the join screen, each controller that presses A takes the next seat and stays bound to it. Left/right picks a character (each character once), B leaves, X cycles the bots' difficulty, up/down sets the board length, and Start begins. Online, each machine joins its local controllers the same way.
- **Screens:** title (board game / free play / quit) → join → board ↔ minigames → results → title. Free play goes join → minigame picker → minigame, and B after a result returns to the picker.
- **Every minigame starts the same way** (`game/src/intro.rs`): an instruction card (title, format, goal, how to play, controls) that waits until every human has pressed A, then a 3-2-1 countdown of `sim_core::COUNTDOWN_TICKS` in which nobody can act. The simulation doesn't tick while the card is up. A minigame only supplies Fluent keys (`{id}-title`, `-goal`, `-how`, `-a`, and optionally `-b`, `-stick`, `-goal-teams`, `-goal-solo`); a test checks the required ones. "Play again" in free play skips the card. Online, readiness will travel as a board-protocol message.
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
- **Models** (`art/lantern_folk.py`, `game/src/characters.rs`): a Python script builds all four characters in Blender from scratch and exports `crates/game/assets/characters/<id>.glb`. One rig (`root`, `body`, `head`, `arm.L/R`, `leg.L/R`; every part rigidly skinned to one bone) with four animations, `idle`, `hop`, `celebrate` and `slump`, each keying every bone. In game the `Body`, `Glow` and `Accent` materials take the player colour, the head's point light rides on the `head` bone, and views animate a character by setting its `Pose`; the model blends to the matching clip. Code still moves the root for things the clips don't cover (walking, leaning into a dash, falling off).
- **Customization (not early):** attachable cosmetics on standard sockets (head, back, hand) and a personal glow colour. Unlocks come later. Cosmetics never change the hitbox and never affect the simulation.

## 4. Architecture

### 4.1 Workspace

```
crates/
  sim-core/     # no Bevy. Math wrapper (libm), portable hasher, seeded RNG, Minigame trait, Run wrapper, input types
  sim-check/    # no Bevy. The automated quality bars: replay, ggrs sync test, bot runs
  sim-board/    # no Bevy. Board graph (RON), turn state machine, rules, board bots, headless games
  minigames/*   # no Bevy. One crate per minigame: State: Clone + Hash, step(), bot_input()
  registry/     # no Bevy. minigame-registry: the list of every minigame, looked up by id
  net/          # no Bevy. Transport (UDP, in-memory, fault injection), Endpoint (handshake, reliable
                #   channel, ggrs socket), rollback driver, online check; later the board protocol
  game/         # Bevy 0.20: rendering, interpolation, UI, audio, input, menus.
                #   flow.rs (screen states), runner.rs (input + minigame driver), board_view.rs,
                #   menus.rs, intro.rs (instruction card), one view module per minigame,
                #   characters.rs, particles.rs, audio.rs + sfx.rs, quality.rs, i18n.rs
  tools/        # headless CLI: `check` (all quality bars), `net-check` (online bar over localhost UDP),
                #   `bots` (bot-only matches), `board` (bot-only board games)
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
| Minigame | Deterministic rollback over inputs only ([ADR 0002](adr/0002-rollback-with-ggrs-core.md)) | `ggrs` drives `Minigame::step`. Save = `clone`, load = restore, checksum = hash. 60 Hz, 2 frames input delay, max 8 frames of rollback. The host hands each minigame its seed, players and format. Each machine sends the input for its own seats; the host also sends the bots' and the empty seats'. The outcome counts once its tick is confirmed (`net::rollback`). |

- **Pause is an input.** All peers see it on the same frame, and the simulation stops advancing game time while paused.
- **Physics:** custom and simple (circles, boxes, heightfields, gravity, knockback), in `sim_core::physics`: a `Vec2` floor plane (x right, y towards the camera; the renderer maps it to world `(x, 0, y)`), `Body` circles with velocities in units per second, and pairwise collisions resolved in index order.
- **Teams:** `Format::team(seat)` gives each seat's side, and `Outcome::from_team_scores` ranks whole teams. `check_all` runs every format a minigame declares. A minigame that really needs rigid-body physics may use `avian` with `enhanced-determinism`, behind the same trait. See [ADR 0006](adr/0006-deterministic-floats-and-custom-physics.md).

### 4.4 Online ([ADR 0004](adr/0004-transport-layer.md))

- **Friends only**, by invite or code. No public matchmaking.
- **Our own transport layer.** Plain UDP comes first: localhost for development, and direct IP over Tailscale or ZeroTier for playing with friends before Steam. A Steam backend (lobbies and invites, SteamNetworkingMessages via a small ggrs `NonBlockingSocket` adapter) slots in behind the same layer if the game ships on Steam.
- **Endpoints** (`net`): each machine has one `Endpoint` over one transport and port. It handshakes (protocol and game version must match exactly, or the peer is refused with the reason), carries reliable ordered messages for the board phase (our own fragment, ack and resend layer), and hands `ggrs` a socket for minigames. Minigames need a full mesh (every machine reaches every other); the board phase only needs everyone to reach the host.
- **Network disconnects:**
  - **Board phase:** pause with "Waiting for P3…". On reconnect, the host sends a full state snapshot. After ~2 minutes, the host gets options: *keep waiting*, *end and save*, or, as a last resort chosen by the host, *bot takes over*.
  - **Minigame:** the minigame stops and **restarts from the beginning with the same seed** once the player is back.
  - **Host leaves:** the session ends. There's no host migration.
- **Saves:** whole board games can be saved and resumed, between turns only. Board state is a plain serializable struct.

### 4.5 Rendering and look

- HDR camera, `Bloom`, TonyMcMapface or AgX tonemapping, emissive materials, volumetric fog, SSAO on desktop presets, and `bevy_hanabi` GPU particles.
- **Board camera:** follows the active player. **Minigame cameras:** a single arena view.
- Rendering interpolates between simulation ticks: the runner keeps the state from one tick back, and views blend by `Time<Fixed>::overstep_fraction()`.
- Shared scenery (night camera, lantern folk, fireflies, paper lanterns) lives in `game/src/scenery.rs`. The lantern folk are Blender models (§3.5); the rest are code-built placeholders.

### 4.6 Content pipeline

- **3D:** Blender, which Claude drives through its MCP, plus procedural generation. Shapes are loose "voxel-ish": whatever is easy to build in Blender, with chunky forms and bevelled edges. Export as glTF (`.glb`) into `crates/game/assets/`, and use `bevy_skein` for components on Blender objects. Models are built by Python scripts in `art/` that rebuild the `.blend` from scratch and export, so changes go into the script, then a re-run (through the MCP, or `blender --background --python <script>`) rewrites both.
- **Boards:** the space graph is a **RON file**, the source of truth that the simulation loads without Bevy. The Blender scene is decoration around the graph's coordinates. A script can push the graph points into Blender as guides, and an in-game debug view draws the graph.
- **Audio:**
  - royalty-free/CC0 packs, and procedural SFX made in code (sfxr-style)
  - today every effect is procedural: `game/src/sfx.rs` is a small deterministic synth, and `game/src/audio.rs` holds one recipe per `Sound` plus a looping night ambience (crickets over wind) on its own channel. They're rendered at startup, so no audio files ship. `--dump-sfx <dir>` writes them all as WAVs for listening.
  - views fire sounds and particle bursts once per event: minigames from `MinigameSet::React` (in `FixedUpdate`, after the step), diffing `ActiveRun::ticked()` (the state before and after that tick); the board as each event starts playing
  - later experiment: music made in Reaper with your VSTs
  - playback through `bevy_kira_audio` (Bevy's own audio plugin is disabled)
  - check Steam's rules on AI-generated content before using any
- **Localization:** English only at launch, but every UI string goes through Fluent keys from day one: `crates/game/locales/en/main.ftl`, read with `tr!("key", name = value)`. Minigame names are keyed by registry id (`minigame-<id>`).

### 4.7 Dependencies (as of 2026-10-20)

| Purpose | Crate | Note |
|---|---|---|
| Engine | bevy 0.20 | Start on the latest ([ADR 0005](adr/0005-bevy-0-20.md)) |
| Rollback | ggrs 0.13 | Core only, no `bevy_ggrs` |
| Wire format | bincode 1.3 | Same as ggrs; decoded with a size limit |
| Particles | bevy_hanabi | Use git main until the 0.20 release |
| Blender components | bevy_skein | Use git main until the 0.20 release. **Not** the crates.io `skein` crate, which is an unrelated hash crate |
| Input | none for now: our own small mapper | [ADR 0007](adr/0007-input-mapping.md); revisit (bevy_enhanced_input first) when rebinding arrives |
| Audio | bevy_kira_audio 0.27 (+ kira 0.12 for building sounds from samples) | Default features off but `wav`, which it needs to build |
| Steam (later) | steamworks 0.13 | Used directly, not through bevy-steamworks |
| Avoid | matchbox (stalled, incompatible with ggrs 0.13), Blenvy (abandoned) | |

## 5. Quality bars

A minigame is **done** when it passes all three:
1. **Determinism test:** the same input stream, run twice and on two simulated peers, gives matching state checksums on every frame.
2. **Headless bot run:** 4 bots finish the game with no panics.
3. **Manual feel check:** the developer plays it.

(1) and (2) are one call, `sim_check::check_all::<M>(seeds)`. Online, `cargo run -p tools -- net-check <minigame>` plays bot matches across 2–4 machines over localhost UDP with simulated lag and loss, and every machine must agree on every confirmed tick and on the outcome; `net`'s tests run a smaller version in CI. Every minigame crate runs it as a unit test, and `cargo run -p tools -- check <minigame>` runs it from the CLI. They will also run in CI on GitLab.

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
- the unlock and progression system
