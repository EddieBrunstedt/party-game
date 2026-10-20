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
title-board = Board game
title-free-play = Free play
title-quit = Quit
title-controls = Stick / arrows to choose, A / Space to confirm

## Join
join-title = Who's playing?
join-hint = Press A to join. Left/right picks your character, B leaves.
join-press-a = Press A to join
    (otherwise a { $difficulty } bot)
join-keyboard = keyboard
join-gamepad = gamepad
join-bots = Bots: { $difficulty } (X)
join-rounds = Rounds: { $rounds } (up/down)
join-start = Press Start (Enter) to begin

## Free play
pick-title = Free play
pick-hint = Up/down: minigame   Left/right: teams   A: play   B: back

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
minigame-again = A: play again   B: choose another
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

## Quick Draw
quick-draw-title = QUICK DRAW
quick-draw-rules = Press A the instant the lantern lights. First to { $points } wins!
quick-draw-wait = Wait for it...
quick-draw-round = Round { $round }
quick-draw-draw = DRAW!
quick-draw-round-won = { $players } takes the round!
quick-draw-ms = { $ms } ms
quick-draw-nobody = Nobody scores
quick-draw-false-start = FALSE START
quick-draw-too-slow = too slow

## Lantern Push
lantern-push-title = LANTERN PUSH
lantern-push-controls = A: shove   B: brace   { $goal }
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
firefly-catch-controls = A: swing net   B: dash   { $goal }
firefly-catch-goal = Most points wins! Golden fireflies are worth 3.
firefly-catch-goal-teams = Team scores add up!
firefly-catch-goal-solo = { $player }'s catches count triple!
firefly-catch-go = GO!
firefly-catch-triple-tag = {"  "}x3

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
