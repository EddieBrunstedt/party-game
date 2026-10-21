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
- **Memory race** (taken: Lantern Echo, see ROADMAP.md)**:** to move on, repeat a sequence (Simon-style) you were shown. A wrong press costs time. Sequences come from the seeded RNG, so everyone gets the same one.
- **Tivoli horse race:** roll or aim a ball into holes, and the hole you hit decides how far your piece moves (like the classic fairground "Derby" race). Each player has their own lane, so it reads well on one screen.

### Tower stack

> "remember that game on your phone 20 years ago were you made a tower one piece at a time. Yeah that. But here you can somehow sabotage your friends"

- **Taken: Lantern Tower** (see ROADMAP.md). A block slides back and forth and you press A to drop it; whatever overhangs is cut off, and the tower gets narrower. Everyone builds side by side. The twist: a way to sabotage the others (speed up their slider, wobble their tower, steal a piece). Timing-based, easy to read, good for bots.

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

Taken: Musical Stools (see ROADMAP.md), the points variant on a fenced turntable, with the 1v3 solo as the cat (the developer's choices, 2026-10-21). The notes that led there:

With four players, classic elimination is over in three short rounds and eliminated players sit out. Variants to weigh:
- **Elimination:** the classic, 3 chairs, then 2, then 1. Short, and players who are out watch.
- **Points instead of elimination** (the third friend): always one chair fewer than players, a point for every round you sit, and the chairs get harder to reach over time. They could move or drift apart, the music could stop sooner or with less warning, or obstacles could come in. Nobody sits out, and the match length is set by the number of rounds.
- Both versions fit the controls: the stick to move, A to sit. Shoving can come from Lantern Push's knockback. In 1v3 or 2v2, a team scores when any member sits.
- The "music stops" moment is a seeded random time in the simulation; the music itself is presentation.

## From a fourth friend (2026-10-20)

Their list, in their words:

> "Duck hunt!
>
> Brawlhalla!
>
> Fifa penalty kicks, but obviously not penalty kicks, just hitting the mark somehow
>
> All the racing games from the 90 and 00 where you needed to jump and duck at the right time or you would fall behind
>
> A game of luck where your skill is irrelevant"

All undecided. How each might fit:

- **Duck hunt (shooting gallery)** (taken: Sky Lanterns, see ROADMAP.md)**:** targets fly up out of the scenery and each player moves a crosshair (in their colour and shape) with the stick and shoots with A. Without a light gun, how fast the crosshair moves sets the difficulty. It suits the shared screen, since every crosshair aims at the same targets. 1v3 can turn it around: three players steer the targets and dodge while the solo shoots. Bots aim at a target with a reaction delay and some jitter that shrink with difficulty. Firefly Catch is also about chasing moving things, so the targets should look and move differently from fireflies.
- **Brawlhalla (platform fighter):** a side-on stage with floating platforms, where the goal is to knock rivals off it. It's Lantern Push's ring-out idea with jumping, so it needs the same platforming physics as the obstacle course (gravity, jumps, one-way platforms). Controls fit: stick, A to jump, B to attack, and maybe X to dodge. Knockback grows with the damage taken, as in Smash and Brawlhalla. 2v2 works well, and fighting games are what rollback netcode was made for. Bots are the hard part: a pure-function fighter has to choose between approaching, attacking and recovering back onto the stage, and still be fun to play against at Easy.
- **Hit the mark (aim and power):** kick or throw a ball at a target, the way a penalty kick is aimed but without the goalie. A sweeping meter sets the aim and a second press sets the power, or the stick aims and holding A charges the shot, with wind or a moving target added later. Everyone could shoot at their own target side by side, or take turns at one target with the closest shot scoring. Bots time their presses with an error that shrinks with difficulty. It's close to the Tivoli horse race (both aim a ball at holes), so the two could become one game.
- **Jump-and-duck race:** an auto-running race where each player has a lane, low hurdles need A (jump) and high bars need B (duck), and getting it wrong makes you stumble and fall behind. Every lane gets the same seeded course. It's the simple version of the first friend's obstacle course, since it needs only timing and no free movement or platforming physics. Bots press inside a timing window that gets tighter with difficulty. All formats fit, and teams could add up their distances or run it as a relay.
- **Pure luck** (taken: Lantern Lottery, see ROADMAP.md, with a mind game added: shared lanterns split their prize)**:** skill doesn't matter. For example, everyone secretly picks one of several lanterns, chests or doors, the picks are revealed together, and the seeded RNG decides which one held the prize or the dud. Picks stay hidden on a shared screen the same way quiz answers do. Bots pick at random at every difficulty, so a bot is exactly as good as a human. It's useful as a catch-up game, since a player who is far behind on the board has as good a chance as anyone. The fun is in the suspense, so it depends almost entirely on presentation (the slow reveal, near misses). Teams could share one pick or pool several.
