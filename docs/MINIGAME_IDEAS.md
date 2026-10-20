# Minigame ideas

Ideas for minigames, before they become roadmap items. Launch needs about 30 (DESIGN.md §6). Add anything here; when one gets built, move it to the roadmap and mark it here as taken.

Every minigame has to fit the same constraints (DESIGN.md §3.3–3.4):
- **One shared screen.** There's no split screen, and online players see the same view as the couch.
- **A stick plus 2–3 buttons**, so it plays on any controller.
- **Formats:** each minigame says which it supports, out of free-for-all, 1v3, 2v2 and duel.
- **Deterministic simulation with bots** at every difficulty, passing `check_all`. A bot is a pure function of the state, so it must be able to play well enough to be fun.

## From a friend (2026-10-20)

In his words first, then a note on how each might fit.

### Racing

> "you want something simple like who clicks a specific button fastest and one hard where you want to use your brain a bit to reach a goal. And one with an obstacle course. And one where you need to remember a sequence to move on!"

> "And one just like the horse racing in tivoli where you need to get a ball in to move your piece forward!"

- **Button mash / fastest press race:** a simple race where pressing the right button fast moves you forward. Close to Quick Draw's reflex idea, but sustained. Easy for bots. All formats.
- **Brainy race:** a race where you have to think a bit to reach the goal (route choices, switches, small puzzles along the track).
- **Obstacle course:** run, jump and dodge through a course. Needs simple platforming physics in `sim_core::physics` (heightfields, gravity).
- **Memory race:** to move on, repeat a sequence (Simon-style) you were shown. A wrong press costs time. Sequences come from the seeded RNG, so everyone gets the same one.
- **Tivoli horse race:** roll or aim a ball into holes, and the hole you hit decides how far your piece moves (like the classic fairground "Derby" race). Each player has their own lane, so it reads well on one screen.

### Tower stack

> "remember that game on your phone 20 years ago were you made a tower one piece at a time. Yeah that. But here you can somehow sabotage your friends"

- A block slides back and forth and you press A to drop it; whatever overhangs is cut off, and the tower gets narrower. Everyone builds side by side. The twist: a way to sabotage the others (speed up their slider, wobble their tower, steal a piece). Timing-based, easy to read, good for bots.

### Puzzles

> "who goes trough a maze fastest; who guesses which picture is the right one. Who guesses which solution is the right one to a problem"

- **Maze race:** everyone in the same maze (or mirrored copies), first out wins. Mazes are generated from the seed.
- **Spot the right picture:** several pictures and one is right (the odd one out, the one matching a description, the one that changed). Pick it first. Needs picture content.
- **Pick the right solution:** a problem with several candidate answers. Bots need a tuned chance of being right per difficulty.

### Silliness

> "hide and seek, screen goes dark and someone needs to find people"

- **Lights out hide and seek:** the screen goes dark, the hiders hide, then the seeker searches with a small light. A natural 1v3 (seeker against hiders). On one shared screen, everyone sees where the hiders are while the lights are on, so the dark phase has to do the hiding work: e.g. hiders move only while it's dark, and the seeker sees only what their lantern lights. Fits the lantern theme well.

### Quizzes

> "everyone loves kahoot type shit"

- **Quiz:** questions on screen, everyone picks an answer on their controller in secret, and the answers are revealed together, with faster right answers worth more. Answers stay hidden even on a shared screen because they're just inputs until the reveal. Needs a question bank, and localized questions later. Bots answer right with a chance set by difficulty.

## Musical chairs (2026-10-20)

From a second friend:

> "Dance around the (decreasing) chairs and try to sit down when music stops"

The developer's concern:

> "One constraint is that it is up to 4 players so it's a little tight starting with 3 chairs, but I am thinking of variants of this"

A third friend's variant:

> "You dont have to go down in numbers, you can get points every time you get one, and instead .... it gets more troublesome to get chairs?"

Undecided. With four players, classic elimination is over in three short rounds and eliminated players sit out. Variants to weigh:
- **Elimination:** the classic, 3 chairs, then 2, then 1. Short, and players who are out watch.
- **Points instead of elimination** (the third friend): always one chair fewer than players, a point for every round you sit, and the chairs get harder to reach over time. They could move or drift apart, the music could stop sooner or with less warning, or obstacles could come in. Nobody sits out, and the match length is set by the number of rounds.
- Both versions fit the controls: the stick to move, A to sit. Shoving can come from Lantern Push's knockback. In 1v3 or 2v2, a team scores when any member sits.
- The "music stops" moment is a seeded random time in the simulation; the music itself is presentation.
