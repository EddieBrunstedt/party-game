# Every UI string in the game, by screen. Code refers to them by key only.

## Characters
character-paper-lantern = Paper Lantern
character-firefly-jar = Firefly Jar
character-candle = Candle
character-neon-ring = Neon Ring

difficulty-easy = Easy
difficulty-normal = Normal
difficulty-hard = Hard

## Title
title-tagline = A board game of lanterns, fireflies and friendly shoving
title-continue = Continue
title-board = Board game
title-free-play = Free play
title-online = Play online
title-host-board = Host a board game
title-host-saved = Host the saved game
title-host-free-play = Host free play
title-join = Join a game
title-back = Back
title-quit = Quit
title-controls = Stick / arrows to choose, A / Space to confirm

## Join
join-title = Who's playing?
join-hint = Press A to join. Left/right picks your character, B leaves.
join-hint-resume = Press A to take your seat back. Left/right picks which seat, B leaves.
join-not-in-game = Not in this game
join-resume-round = Saved game: round { $round } of { $rounds }
join-press-a = Press A to join
    (otherwise a { $difficulty } bot)
join-keyboard = keyboard
join-gamepad = gamepad
join-bots = Bots: { $difficulty } (X)
join-rounds = Rounds: { $rounds } (up/down)
join-start = Press Start (Enter) to begin
join-online = online
join-hosting = Hosting. Friends join at { $addresses }
join-hosting-here = 127.0.0.1:{ $port } (this PC)
join-hosting-lan = { $address }:{ $port } (local network)
join-hosting-tailscale = { $address }:{ $port } (Tailscale)
join-connected = Connected to { $address }
join-pause = Pause when a controller disconnects: { $pause } (Y)
join-pause-on = On
join-pause-off = Off
join-host-settings = Bots: { $difficulty }   Rounds: { $rounds }   Pause when a controller disconnects: { $pause }   (the host chooses)
join-host-settings-resume = Saved game: round { $round } of { $rounds }   Bots: { $difficulty }   Pause when a controller disconnects: { $pause }   (the host chooses)
join-host-settings-free-play = Bots: { $difficulty }   Pause when a controller disconnects: { $pause }   (the host chooses)
join-waiting-host = Waiting for the host to press Start
join-starting = Starting...
join-rejoin = Press A on each of your controllers to take your seats back
join-rejoining = Taking your seats back...
join-press-a-rejoin = Press A to take this seat back

## Joining an online game
connect-title = Join a game
connect-hint = Type the host's address, or use the d-pad: left/right moves, up/down changes a digit, X deletes.
connect-actions = A / Enter: connect    B / Esc: back
connect-connecting = Connecting to { $address }...
connect-no-answer = No answer from { $address }. Is the host on the join screen, and is the address right?
connect-no-address = Type the host's address first.
connect-bad-address = "{ $address }" isn't an address.
connect-refused-version = The host runs version { $theirs } and you run { $ours }. You both need the same version.
connect-refused-not-hosting = That machine isn't hosting a game.
connect-refused-full = That game is full.
connect-refused-started = That game has already started.
connect-socket-error = Couldn't open a network port: { $error }

## Notices on the title screen
notice-host-left = The host closed the game.
notice-host-lost = Lost the connection to the host.
notice-ended = You ended the game.
notice-ended-saved = You ended the game. Continue picks it up from the last turn.
notice-no-save = There is no saved game.
notice-save-unreadable = Couldn't read the saved game: { $error }
notice-save-version = The saved game is from another version and can't be continued.
notice-save-damaged = The saved game is damaged and can't be continued.
notice-host-failed = Couldn't host on port { $port }: { $error }

## Free play
pick-title = Free play
pick-hint = Up/down: minigame   Left/right: teams   A: play   B: back
pick-waiting-host = The host is choosing a minigame...
pick-hint-client = B: leave
waiting-for = Waiting for { $players }...
waiting-options = A: keep waiting   X: bots take over   B: end the game

## Results
results-title = { $winners } { $count ->
        [one] wins
       *[other] win
    } the game!
results-line = { $place }.  { $player }  { $character }   { $stars } stars   { $coins } coins
results-continue = Press A to return to the title

## Shared by every minigame
bot-tag = {" "}(bot)
solo-tag = {"  "}SOLO
controller-lost = controller lost
controller-disconnected = Controller disconnected
controller-reconnect = Reconnect { $players }'s controller
paused = Paused
paused-resume = Press Start to resume
winners = { $names } { $count ->
        [one] wins!
       *[other] win!
    }
card-stick = Stick
card-ready = READY
card-press-a = Press A
card-prompt = Press A when you're ready
minigame-again = A: play again   B: choose another
minigame-waiting-host = The host chooses what's next
minigame-continue = Press A to continue
place = { $place ->
        [1] 1st
        [2] 2nd
        [3] 3rd
       *[other] { $place }th
    }
points = { $points ->
        [one] { $points } point
       *[other] { $points } points
    }
format-free-for-all = Free-for-all
format-versus = { $left }  vs  { $right }

## Minigame names, by registry id
minigame-quick-draw = Quick Draw
minigame-lantern-push = Lantern Push
minigame-firefly-catch = Firefly Catch
minigame-lantern-echo = Lantern Echo
minigame-lantern-tower = Lantern Tower
minigame-sky-lanterns = Sky Lanterns
minigame-lantern-lottery = Lantern Lottery
minigame-musical-stools = Musical Stools
minigame-hedge-maze = Hedge Maze
minigame-lantern-derby = Lantern Derby
minigame-tabletop-rally = Tabletop Rally
minigame-triple-jump = Triple Jump
minigame-puzzle-buzzer = Puzzle Buzzer
minigame-lantern-masquerade = Lantern Masquerade
minigame-lantern-flail = Lantern Flail
minigame-lantern-run = Lantern Run

## Quick Draw
quick-draw-title = QUICK DRAW
quick-draw-goal = First to 3 points wins!
quick-draw-how = The lantern lights up after a random wait: press A the instant it does. Press too early and you sit the round out.
quick-draw-a = draw
quick-draw-wait = Wait for it...
quick-draw-round = Round { $round }
quick-draw-draw = DRAW!
quick-draw-round-won = { $players } { $count ->
        [one] takes
       *[other] take
    } the round!
quick-draw-ms = { $ms } ms
quick-draw-nobody = Nobody scores
quick-draw-false-start = FALSE START
quick-draw-too-slow = too slow

## Lantern Push
lantern-push-title = LANTERN PUSH
lantern-push-how = Shove rivals off the platform. It starts shrinking after a few seconds, so don't get caught near the edge.
lantern-push-stick = move
lantern-push-a = shove
lantern-push-b = brace (heavy for a moment, shoves bounce off)
lantern-push-goal = Last one standing wins!
lantern-push-goal-teams = Knock the other team off!
lantern-push-goal-solo = { $player } is big and heavy. Three against one!
lantern-push-go = PUSH!
lantern-push-out = { $player } is out!
lantern-push-everyone = Everyone survives!
lantern-push-bracing = bracing
lantern-push-reeling = reeling!
lantern-push-standing = standing
lantern-push-is-out = OUT

## Firefly Catch
firefly-catch-title = FIREFLY CATCH
firefly-catch-how = Chase a firefly until it tires and its spot on the ground pulses, then swing when it's inside the ring in front of you. Rested fireflies hop out of reach.
firefly-catch-stick = run
firefly-catch-a = swing net
firefly-catch-b = dash
firefly-catch-goal = Most points wins! Golden fireflies are worth 3.
firefly-catch-goal-teams = Team scores add up!
firefly-catch-goal-solo = { $player }'s catches count triple!
firefly-catch-go = GO!
firefly-catch-triple-tag = {"  "}x3

## Lantern Echo
lantern-echo-title = LANTERN ECHO
lantern-echo-how = Your lanterns light up one after another: flick the stick the same way, in the same order. Each right echo moves you a step, and the next sequence adds one more. A wrong flick costs you time.
lantern-echo-stick = repeat the lanterns (or the d-pad)
lantern-echo-goal = First to the end of the lane wins!
lantern-echo-goal-teams = Your team's steps add up: 6 each to finish!
lantern-echo-goal-solo = { $player } needs 6 steps; the other three need 17 between them!
lantern-echo-go = GO!
lantern-echo-watch = watch...
lantern-echo-echo = echo { $entered }/{ $length }
lantern-echo-right = right!
lantern-echo-slip = WRONG
lantern-echo-done = done!
lantern-echo-team = { $players }  { $steps }/{ $needed }

## Lantern Tower
lantern-tower-title = LANTERN TOWER
lantern-tower-how = A block slides over your tower: drop it right on top. Whatever hangs over the edge is cut off, and a block that misses knocks your top floor off. 3 perfect drops charge a gust: blow it to make the tallest rival's blocks slide faster.
lantern-tower-a = drop the block
lantern-tower-b = blow a gust (after 3 perfect drops)
lantern-tower-goal = First to 20 floors wins!
lantern-tower-goal-teams = Your team shares a tower and takes turns. First to 20 floors wins!
lantern-tower-goal-solo = { $player } builds alone; the other three share a tower and take turns. First to 20 floors!
lantern-tower-go = BUILD!
lantern-tower-gust = Gust on { $players }!
lantern-tower-floors = { $floors }/{ $goal } floors
lantern-tower-charge = gust { $charge }/{ $needed }
lantern-tower-gust-ready = GUST READY: B!
lantern-tower-gusted = gusted!
lantern-tower-your-turn = your turn

## Sky Lanterns
sky-lanterns-title = SKY LANTERNS
sky-lanterns-how = Paper lanterns drift up the night sky. Move your sight onto one and fire a spark to light it in your colour. Golden lanterns are worth 3. A spark that hits nothing fizzles for a moment.
sky-lanterns-stick = move your sight (in 1v3 the three fly)
sky-lanterns-a = fire a spark
sky-lanterns-b = dash (the three in 1v3)
sky-lanterns-goal = Light the most lanterns in 40 seconds!
sky-lanterns-goal-teams = Light the most lanterns with your partner in 40 seconds!
sky-lanterns-goal-solo = { $player } has the only sight: light the other three's lanterns 23 times in 40 seconds! The three fly and hide behind the other lanterns, which block sparks.
sky-lanterns-go = FIRE!
sky-lanterns-points = { $points } points
sky-lanterns-team = team: { $points }
sky-lanterns-hits = { $hits }/{ $needed } hits
sky-lanterns-lit-times = lit { $times ->
    [one] once
   *[other] { $times } times
}
sky-lanterns-dash-ready = dash ready: B

## Lantern Lottery
lantern-lottery-title = LANTERN LOTTERY
lantern-lottery-how = Four lanterns hold fireflies, and a moth hides in one. Point the stick at a lantern and press A to lock in: nobody sees your pick until everyone's is in. A lantern's fireflies are shared among everyone who picked it, and the moth's lantern pays nothing. The last round counts double (the last two when taking turns).
lantern-lottery-stick = point at a lantern (or the d-pad)
lantern-lottery-a = lock in your pick
lantern-lottery-goal = Most fireflies after 5 rounds wins! With two players you take turns: each round one of you hides a moth and the other picks a lantern, for 6 rounds.
lantern-lottery-goal-teams = Teams take turns: one hides a moth each, the other picks lanterns, for 6 rounds. Your team's fireflies add up, so don't both pick the same lantern!
lantern-lottery-goal-solo = { $player } hides the moth each round, and the other three pick lanterns. { $player } needs 4 catches in 5 rounds!
lantern-lottery-round = Round { $round } of { $rounds }
lantern-lottery-double = Double fireflies!
lantern-lottery-pick = Pick a lantern!  { $secs }
lantern-lottery-hiders = { $players } { $count ->
        [one] hides a moth
       *[other] hide moths
    }
lantern-lottery-team = { $players }  { $fireflies }
lantern-lottery-fireflies = { $fireflies ->
        [one] { $fireflies } firefly
       *[other] { $fireflies } fireflies
    }
lantern-lottery-catches = { $catches }/{ $needed } catches
lantern-lottery-picking = picking...
lantern-lottery-hiding = hiding a moth
lantern-lottery-locked = locked in
lantern-lottery-gain = +{ $fireflies }
lantern-lottery-gain-each = +{ $fireflies } each
lantern-lottery-moth = moth
lantern-lottery-moth-caught = MOTH! caught { $count }
lantern-lottery-caught = MOTH!
lantern-lottery-safe = safe
lantern-lottery-caught-them = caught them!
lantern-lottery-missed = missed

## Musical Stools
musical-stools-title = MUSICAL STOOLS
musical-stools-how = Dance round the fence while the music plays: the stools spin inside it. When the music stops the fence drops, and the first to reach a free stool sits on it. There's always one stool too few! The music slows down just before it stops, but less each round.
musical-stools-stick = move
musical-stools-a = dash (shoves whoever you run into)
musical-stools-goal = A seat is a point, and whoever sits first gets one more. Most points after 7 rounds wins!
musical-stools-goal-teams = A seat is a point for your team, and whoever sits first gets one more. Most points after 7 rounds wins!
musical-stools-goal-solo = { $player } is the cat and can't sit: once the music stops, the cat's dash catches you. The three of you must fill both stools to win a round. First to 3 rounds wins!
musical-stools-round = Round { $round } of { $rounds }
musical-stools-dance = Dance!
musical-stools-slowing = The music's slowing down...
musical-stools-sit = SIT!
musical-stools-first = { $players } sat first!
musical-stools-standing = { $players } left standing
musical-stools-cat-round = The cat takes the round!
musical-stools-trio-round = Both stools filled!
musical-stools-rounds = cat { $cat } : { $trio } the three  (first to { $needed })
musical-stools-team = team: { $points }
musical-stools-points = { $points ->
        [one] { $points } point
       *[other] { $points } points
    }
musical-stools-dancing = dancing
musical-stools-seated = seated +{ $gain }
musical-stools-left-out = left standing
musical-stools-caught = caught!
musical-stools-cat = the cat

## Hedge Maze
hedge-maze-title = HEDGE MAZE
hedge-maze-how = The maze is dark. Your lantern lights the hedges near you, and whatever's lit stays lit for everyone. Find the way to the beacon in the middle! Hold a turn early and you'll take it when it opens.
hedge-maze-stick = walk
hedge-maze-a = flare: lights up a wide circle, for everyone (once per maze)
hedge-maze-goal = 3 mazes, each scored by who gets home first. Most points wins!
hedge-maze-goal-teams = 3 mazes, each scored by who gets home first; your team's points add up. Most points wins!
hedge-maze-goal-solo = { $player } is the ghost: faster, and its touch sends you back to your corner. Two of you three must reach the beacon within 20 seconds to take a maze. Best of 3!
hedge-maze-maze = Maze { $maze } of { $mazes }
hedge-maze-go = Find the beacon!
hedge-maze-time = { $seconds } s
hedge-maze-home = { $player } is home!
hedge-maze-first = { $players } got home first!
hedge-maze-nobody = Nobody got home!
hedge-maze-ghost-maze = The ghost takes the maze!
hedge-maze-trio-maze = Two got home!
hedge-maze-rounds = ghost { $ghost } : { $trio } the three  (first to { $needed })
hedge-maze-team = team: { $points }
hedge-maze-points = { $points ->
        [one] { $points } point
       *[other] { $points } points
    }
hedge-maze-ghost = ghost, { $catches } caught
hedge-maze-runner = runner
hedge-maze-home-place = home { $place }
hedge-maze-home-gain = home { $place } +{ $gain }
hedge-maze-caught = caught!
hedge-maze-lost = still lost
hedge-maze-flare-ready = flare ready (A)
hedge-maze-flare-used = searching

lantern-derby-title = LANTERN DERBY
lantern-derby-how = Roll glowing balls up your board. A ball drops into the hole it stops on and moves your runner by the hole's dots: the small hole at the top is worth 3. The meter beside your board, and the line across it, show where the ball will stop. Balls wobble a little, and one on a rim may drop in or bounce out.
lantern-derby-stick = aim (it stays where you leave it); the barker moves the lid
lantern-derby-a = hold to charge, let go to roll; the barker snaps the lid shut
lantern-derby-goal = First runner to the finish wins!
lantern-derby-goal-teams = You and your partner share a runner: first to the finish wins!
lantern-derby-goal-solo = { $player } is the barker: move a lid over the three's boards and snap it shut on a hole with A. Every ball that bounces off it moves the barker's runner. The three share a runner with further to go. First to the finish wins!
lantern-derby-go = Roll!
lantern-derby-points = { $points } of { $goal }
lantern-derby-team = team: { $points } of { $goal }
lantern-derby-barker = barker: { $points } of { $goal }
lantern-derby-in = +{ $value }!
lantern-derby-rim = off the rim!
lantern-derby-shut = lid!
lantern-derby-short = missed
lantern-derby-wall = too far!
lantern-derby-lid-ready = lid ready (A)
lantern-derby-lid-shut = lid shut
lantern-derby-lid-cooling = lid resetting

tabletop-rally-title = TABLETOP RALLY
tabletop-rally-how = Toy cars race round a giant table at night. Steer like the driver: left and right are your car's own, so driving down the screen they swap. The camera follows whoever is in front: drop off the edge of the screen, or off the table, and you're out of the round. The tablecloth is slow and the props are solid. If a round drags on, the screen closes in.
tabletop-rally-stick = steer left and right, from the car's point of view
tabletop-rally-a = drive
tabletop-rally-b = brake, then reverse
tabletop-rally-goal = Be the last car on screen to take the round. First to 3 rounds wins!
tabletop-rally-goal-teams = Your team takes the round once only your cars are left on screen. First to 3 rounds wins!
tabletop-rally-goal-solo = { $player } is the getaway car: a touch faster, with a head start, and the camera follows it. It must leave all three chasers off screen to take the round. The three take it by touching the getaway car. First to 3 rounds wins!
tabletop-rally-round = Round { $round }
tabletop-rally-ready = Ready...
tabletop-rally-go = Go!
tabletop-rally-closing = The screen is closing in!
tabletop-rally-takes = { $names } { $count ->
        [one] takes
       *[other] take
    } the round!
tabletop-rally-caught = Caught!
tabletop-rally-escaped = Got away!
tabletop-rally-off-table = Off the table!
tabletop-rally-time = Time's up: the furthest along takes it
tabletop-rally-photo = Photo finish: the furthest along takes it
tabletop-rally-out-screen = off screen
tabletop-rally-out-table = off the table
tabletop-rally-out-caught = caught!
tabletop-rally-points = rounds { $points }/{ $goal }
tabletop-rally-getaway = getaway
tabletop-rally-chaser = chaser

triple-jump-title = TRIPLE JUMP
triple-jump-how = Mash A to run up: speed builds slowly and fades slowly, so keep a steady pace all the way down the runway (a burst at the end won't get you far). Press B before the line to take off and keep holding it: the needle climbs while you hold, and letting go jumps. 45 degrees is best. Then hop, step, jump: press B again just as you land, hold, and let go. Step over the line and it's a foul; press too early in the air, or too late, and you stumble.
triple-jump-a = mash to run (the coach: lock the angle; the wind: blow)
triple-jump-b = take off and land: hold for the angle, let go to jump (the wind: gust)
triple-jump-stick = the wind moves its fan up and down the lanes
triple-jump-goal = Three attempts each: the longest jump wins!
triple-jump-goal-teams = You share a jumper and swap jobs each attempt: the runner mashes A and presses B to jump, the coach presses A to stop the angle needle. Four attempts: the longest jump wins!
triple-jump-goal-solo = { $player } jumps first, alone, and that mark is the line. Then { $player } is the wind: the stick moves a fan between your lanes, mashing A blows a headwind and B is a gust. The three win if two of them land past the line in three attempts each!
triple-jump-go = Go!
triple-jump-attempt = Attempt { $attempt } of { $attempts }
triple-jump-sets = { $player } jumps first and sets the mark
triple-jump-mark-set = { $player } set the mark: { $metres } m
triple-jump-chase = Past { $player }'s { $metres } m: { $count } of { $needed }
triple-jump-leader = Longest so far: { $names }, { $metres } m
triple-jump-no-marks = No marks yet
triple-jump-metres = { $metres } m
triple-jump-stumble = Stumbled: { $metres } m
triple-jump-foul = Foul!
triple-jump-foul-short = foul
triple-jump-early = Too early!
triple-jump-perfect = Perfect!
triple-jump-good = Good
triple-jump-ok = OK
triple-jump-angle = { $angle } deg
triple-jump-best = best { $metres } m
triple-jump-no-mark = no mark yet
triple-jump-runner = running
triple-jump-coach = coaching: press A
triple-jump-wind = the wind
triple-jump-mark = the mark: { $metres } m
triple-jump-line = { $player }: { $metres } m
triple-jump-solo-first = jumping first
triple-jump-gust-ready = gust ready (B)
triple-jump-gust-wait = gust in { $seconds } s
triple-jump-past = past the mark!

puzzle-buzzer-title = PUZZLE BUZZER
puzzle-buzzer-how = Each puzzle has four answers: up, right, down and left. Point the stick at one and press A to lock it in (nobody sees where you point). The first right answer takes the puzzle. A wrong answer is out of that puzzle, and everyone sees it was wrong. You have 10 seconds, and the puzzles get harder as you go.
puzzle-buzzer-stick = point at an answer (or the d-pad)
puzzle-buzzer-a = lock in your answer
puzzle-buzzer-goal = First to 5 puzzles wins!
puzzle-buzzer-goal-teams = Each puzzle has two halves, one for each partner. Your team takes it once you've both locked in your half right, but one wrong half puts you both out of that puzzle. First team to 5 wins!
puzzle-buzzer-goal-solo = { $player } answers alone and needs 3 puzzles. The other three share one buzzer: one wrong answer from any of you puts all three out of that puzzle, and your buzzers open a moment after { $player }'s. You need 5 puzzles between you!
puzzle-buzzer-count = How many like the one in the middle?
puzzle-buzzer-sum = What does it make?
puzzle-buzzer-sequence = What comes next?
puzzle-buzzer-odd = Which shape is different?
puzzle-buzzer-ladder = Which rope leads to the lantern?
puzzle-buzzer-scales = Which is heaviest?
puzzle-buzzer-puzzle = Puzzle { $number }
puzzle-buzzer-goal-line = First to { $goal }
puzzle-buzzer-goal-line-solo = Solo needs 3, the three need 5
puzzle-buzzer-left = { $secs }
puzzle-buzzer-took = { $players } got it!
puzzle-buzzer-all-wrong = All wrong!
puzzle-buzzer-time = Time's up!
puzzle-buzzer-points = { $points }/{ $goal }
puzzle-buzzer-thinking = thinking...
puzzle-buzzer-wait = wait...
puzzle-buzzer-locked = locked in
puzzle-buzzer-wrong = wrong!
puzzle-buzzer-out = out
puzzle-buzzer-got-it = got it!

## Lantern Masquerade
lantern-masquerade-title = LANTERN MASQUERADE
lantern-masquerade-how = Everyone in the market looks alike, you too. Nobody is told which one they are: move and watch to find yourself! The crowd walks in straight lines and stops a while, so walk like them or stand out. Stand still under a bell and hold A to ring it. Everyone hears whose it was.
lantern-masquerade-stick = walk (in 1v3 the seeker moves the lantern)
lantern-masquerade-a = hold under a bell to ring it (the seeker: catch whoever's under the lantern)
lantern-masquerade-b = strike whoever's next to you (not in 1v3)
lantern-masquerade-goal = Ring all 5 bells, or be the last one standing: a struck player is out of the round. First to 2 rounds wins!
lantern-masquerade-goal-teams = Ring all 5 bells with your partner (you don't know which one they are either), or be the last team standing. First to 2 rounds wins!
lantern-masquerade-goal-solo = { $player } is the seeker. Their lantern catches whoever's under it, but catching anyone else puts it out for 3 seconds. The three of you must ring all 5 bells between you before you're all caught or 75 seconds run out. First to 2 rounds wins!
lantern-masquerade-round = Round { $round } - first to { $needed }
lantern-masquerade-shuffle = Find yourself!
lantern-masquerade-go = Ring the bells!
lantern-masquerade-go-solo = Ring the bells! The seeker is watching...
lantern-masquerade-time = { $seconds } s
lantern-masquerade-rounds-solo = seeker { $seeker } : { $trio } the three
lantern-masquerade-bells = bells { $bells }/5
lantern-masquerade-hiding = hiding
lantern-masquerade-out = out!
lantern-masquerade-seeker = the seeker
lantern-masquerade-lantern-ready = lantern ready
lantern-masquerade-lantern-dark = lantern out
lantern-masquerade-rang-all = { $players } rang every bell!
lantern-masquerade-last-standing = Last one standing: { $players }
lantern-masquerade-time-up = Time! Most bells: { $players }
lantern-masquerade-seeker-round = The seeker caught all three!
lantern-masquerade-seeker-time = Time's up: the seeker takes the round!
lantern-masquerade-trio-round = The three rang every bell!

## Lantern Flail
lantern-flail-title = LANTERN FLAIL
lantern-flail-how = Spin the stick round to whirl the lantern on your rope, and smash it into rivals: the faster it flies, the harder it hits. Every hit you take makes the next one knock you further.
lantern-flail-stick = run, or spin it round to whirl
lantern-flail-a = lunge
lantern-flail-b = hold to reel in, let go to fling out
lantern-flail-goal = Knock rivals into the pond: a point each. Most points in 60 seconds wins!
lantern-flail-goal-teams = Partners hold the ends of one rope with a lantern in the middle: sweep it across the raft together! Every fall of theirs is a point. Most points in 60 seconds wins!
lantern-flail-goal-solo = { $player } is a giant with a wrecking ball: knock the three in 8 times! The three win by dunking the giant 3 times. At 60 seconds, the side nearer its goal wins.
lantern-flail-go = SWING!
lantern-flail-sudden-death = SUDDEN DEATH
lantern-flail-sudden-death-sub = Level! The next fall decides.
lantern-flail-knocked = { $by } knocked { $player } in!
lantern-flail-fell = { $player } fell in!
lantern-flail-points = { $points ->
    [one] 1 point
   *[other] { $points } points
}
lantern-flail-damage = damage { $damage }
lantern-flail-in-pond = in the pond!
lantern-flail-safe = back on!
lantern-flail-solo-score = giant { $giant }/{ $giant_goal } : { $trio }/{ $trio_goal } the three
lantern-flail-everyone = Everyone ties!

## Lantern Run
lantern-run-title = LANTERN RUN
lantern-run-how = Your glider flies forward by itself: steer round what comes at you. Hold A to fly faster while your meter lasts; flat out, you steer slower and ram anyone you touch. Grazing past obstacles refills the meter. Ride a rival's slipstream, or hook the glider ahead to slingshot past it. Hooked? Swing so the rope catches on an obstacle!
lantern-run-stick = steer (you drift: push the other way to stop)
lantern-run-a = hold to fly faster, while the meter lasts
lantern-run-b = fire the grapple at the glider ahead, hold to be towed
lantern-run-goal = In the canyon the camera pushes you along. In the cave, fall behind it and you're out: watch your room bar. Last glider flying wins!
lantern-run-goal-teams = Hook your partner for a free tow that doesn't slow them. In the cave, fall behind the camera and you're out. The last team flying wins!
lantern-run-goal-solo = { $player } flies the barge with a head start, dropping mines (A) and cutting ropes (B). The three win if any of them gets past it within 75 seconds!
lantern-run-go = FLY!
lantern-run-cave = INTO THE CAVE
lantern-run-cave-sub = Fall behind the camera now and you're out!
lantern-run-closing = The camera closes in!
lantern-run-out = { $player } is out!
lantern-run-cut = The barge cut the ropes!
lantern-run-barge-ahead = barge { $gap } m ahead
lantern-run-barge-held = The barge held them off!
lantern-run-barge-passed = Past the barge!
lantern-run-barge-tag = the barge
lantern-run-is-out = out
lantern-run-spinning = spinning!
lantern-run-towing = towed by { $player }
lantern-run-hooked = hooked by { $player }!
lantern-run-throttle = flat out!
lantern-run-meter = throttle
lantern-run-room = room
lantern-run-slipstream = slipstream

## Board
board-turn-order = Turn order
board-rolled = { $player } rolled { $value }
board-round = Round { $round }
board-round-of = of { $rounds }
board-round-counter = Round { $round } / { $rounds }
board-turn = { $player }'s turn
board-star-bought = { $player } got a Star!
board-star-too-poor = Not enough coins
board-star-costs = A Star costs { $price } coins
board-star-moves = The Star moves!
board-minigame-won = { $players } won the minigame!
board-prize = +{ $coins } coins each
board-game-won = { $players } { $count ->
        [one] wins
       *[other] win
    } the game!
board-see-results = Press A to see the results
board-roll-for-order = Roll for turn order!
board-hit-die = { $player }: press A to hit the die
board-press-roll = { $player }: press A to roll
board-which-way = Which way?
board-point-stick = { $player }: point the stick, then press A
board-buy-star = Buy the Star for { $price } coins?
board-yes-no = A: yes   B: no
board-panel = Stars { $stars }   Coins { $coins }
