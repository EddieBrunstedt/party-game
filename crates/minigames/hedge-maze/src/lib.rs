//! Hedge Maze: a race through a dark maze to the beacon in its middle. Everyone starts in a
//! corner and carries a lantern that lights the hedges near them; once lit, a hedge stays lit for
//! everyone, so whoever leads lights the way for whoever follows. A, once per maze, fires a flare
//! that lights a wide circle (for everyone too). [`MAZES`] mazes; each one scores by finishing
//! order (with four racers 3, 2, 1, 0) and ends once all but one are in, or when time runs out,
//! when the rest are ranked by how far they still have to go. Most points wins.
//!
//! The maze is a perfect maze per quarter, turned four times round the middle, with a few
//! openings between the quarters added the same way, so every corner is exactly as far from the
//! beacon as every other.
//!
//! In 1v3 the solo is the ghost: it starts on the beacon, can never go back onto it, moves faster,
//! and a runner it touches goes back to their corner. The three take a maze if
//! [`TRIO_NEEDED`] of them reach the beacon in time; first side to [`ROUNDS_TO_WIN`] mazes wins.
//!
//! Positions are whole milli-cells (a cell is [`UNIT`] wide; cell `(x, y)`'s centre is at
//! `(x * UNIT, y * UNIT)`, `y` towards the camera), so the simulation has no floats. Walkers
//! move along the lines between cell centres, like Pac-Man: holding a turn that isn't open yet
//! keeps you going until it is.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use sim_core::{
    Button, Difficulty, Format, FormatKind, Input, Inputs, MAX_PLAYERS, Minigame, Outcome,
    PlayerSetup, Rng, Seat,
};

/// The opening countdown, the same in every minigame.
pub const INTRO_TICKS: u16 = sim_core::COUNTDOWN_TICKS;
/// Cells along each side. Odd, so there's a middle cell.
pub const SIZE: usize = 15;
pub const MIDDLE: usize = SIZE / 2;
pub const CELLS: usize = SIZE * SIZE;
/// A cell's width in position units.
pub const UNIT: i32 = 1000;
/// Position units a runner moves per tick, and the ghost.
pub const SPEED: i32 = 75;
pub const GHOST_SPEED: i32 = 125;
/// A lantern lights every cell whose centre is this close.
pub const LIGHT_RADIUS: i32 = 1500;
/// A flare lights every cell this close.
pub const FLARE_RADIUS: i32 = 3200;
/// The beacon lights the middle cell and the four next to it.
pub const BEACON_RADIUS: i32 = 1000;
/// Openings added between neighbouring quarters (each is made four times, once per quarter).
pub const LOOPS: usize = 2;
/// How often the maze generator grows from its newest cell instead of a random one, in percent:
/// higher makes longer corridors with fewer branches.
pub const NEWEST_PERCENT: u32 = 90;
/// Steps from a corner to the beacon a maze must take at least, so the way doubles back...
pub const MIN_PATH: u16 = 24;
/// ...out of this many tries.
pub const CANDIDATES: usize = 64;

pub const MAZES: u8 = 3;
/// 1v3: first side to this many mazes wins (best of [`MAZES`]).
pub const ROUNDS_TO_WIN: u8 = 2;
/// 1v3: the three take a maze once this many of them have reached the beacon.
pub const TRIO_NEEDED: usize = 2;
/// The ghost catches a runner whose centre comes this close.
pub const CATCH_REACH: i32 = 600;
/// A caught runner stands in their corner this long...
pub const RESPAWN_TICKS: u16 = 150;
/// ...and can't be caught again for this long after being caught.
pub const SAFE_TICKS: u16 = RESPAWN_TICKS + 60;

/// Before each maze after the first, everyone waits in their corner this long.
pub const READY_TICKS: u16 = 60;
/// A maze's time limit, and 1v3's.
pub const RACE_TICKS: u16 = 45 * 60;
pub const GHOST_RACE_TICKS: u16 = 20 * 60;
/// The maze's result shows this long before the next one (the last one's a little longer).
pub const RESULT_TICKS: u16 = 120;
pub const FINAL_RESULT_TICKS: u16 = 150;
/// A turn pressed this soon after passing its cell's centre goes back and takes it.
pub const CORNER_SLACK: i32 = 300;
/// Stick values (out of 127) below this count as neutral.
const DEAD_ZONE: i32 = 40;

pub struct HedgeMaze;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Dir {
    Up,
    Right,
    Down,
    Left,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::Up, Dir::Right, Dir::Down, Dir::Left];

    pub fn bit(self) -> u8 {
        1 << self as u8
    }

    /// One cell's step in grid coordinates (`y` towards the camera).
    pub fn delta(self) -> (i32, i32) {
        match self {
            Dir::Up => (0, -1),
            Dir::Right => (1, 0),
            Dir::Down => (0, 1),
            Dir::Left => (-1, 0),
        }
    }

    pub fn opposite(self) -> Dir {
        Self::ALL[(self as usize + 2) % 4]
    }

    /// A quarter turn clockwise on screen.
    pub fn clockwise(self) -> Dir {
        Self::ALL[(self as usize + 1) % 4]
    }

    pub fn horizontal(self) -> bool {
        matches!(self, Dir::Left | Dir::Right)
    }
}

pub fn cell_at(x: usize, y: usize) -> usize {
    y * SIZE + x
}

pub fn cell_xy(cell: usize) -> (usize, usize) {
    (cell % SIZE, cell / SIZE)
}

/// The beacon's cell.
pub const fn middle() -> usize {
    MIDDLE * SIZE + MIDDLE
}

/// Corner `k` (0 top left, then clockwise).
pub fn corner(k: usize) -> usize {
    let mut cell = cell_at(0, 0);
    for _ in 0..k % 4 {
        cell = turn(cell);
    }
    cell
}

/// The cell a quarter turn clockwise round the middle.
fn turn(cell: usize) -> usize {
    let (x, y) = cell_xy(cell);
    cell_at(SIZE - 1 - y, x)
}

pub fn neighbour(cell: usize, dir: Dir) -> Option<usize> {
    let (x, y) = cell_xy(cell);
    let (dx, dy) = dir.delta();
    let (x, y) = (x as i32 + dx, y as i32 + dy);
    let inside = (0..SIZE as i32).contains(&x) && (0..SIZE as i32).contains(&y);
    inside.then(|| cell_at(x as usize, y as usize))
}

/// The top-left quarter: the maze generator's patch, which is turned to make the other three.
fn in_first_quarter(cell: usize) -> bool {
    let (x, y) = cell_xy(cell);
    x < MIDDLE && y <= MIDDLE
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Maze {
    /// Per cell, a [`Dir::bit`] for each way out with no hedge.
    pub open: [u8; CELLS],
}

impl Maze {
    pub fn is_open(&self, cell: usize, dir: Dir) -> bool {
        self.open[cell] & dir.bit() != 0
    }

    /// Whether a walker may step from `cell` towards `dir`: the ghost never steps onto the beacon.
    pub fn passable(&self, cell: usize, dir: Dir, ghost: bool) -> bool {
        self.is_open(cell, dir) && !(ghost && neighbour(cell, dir) == Some(middle()))
    }

    fn carve(&mut self, cell: usize, dir: Dir) {
        let next = neighbour(cell, dir).expect("carving inside the maze");
        self.open[cell] |= dir.bit();
        self.open[next] |= dir.opposite().bit();
    }

    /// Carve `cell` towards `dir` and the same passage in the other three quarters.
    fn carve_four(&mut self, mut cell: usize, mut dir: Dir) {
        for _ in 0..4 {
            self.carve(cell, dir);
            cell = turn(cell);
            dir = dir.clockwise();
        }
    }

    /// A maze whose way from a corner to the beacon is at least [`MIN_PATH`] steps: the first
    /// of up to [`CANDIDATES`] that is, or else the longest.
    pub fn generate(rng: &mut Rng) -> Self {
        let mut best = Self::grow(rng, NEWEST_PERCENT, LOOPS);
        for _ in 1..CANDIDATES {
            let path = |m: &Maze| m.steps_to_middle()[corner(0)];
            if path(&best) >= MIN_PATH {
                break;
            }
            let next = Self::grow(rng, NEWEST_PERCENT, LOOPS);
            if path(&next) > path(&best) {
                best = next;
            }
        }
        best
    }

    fn grow(rng: &mut Rng, newest_percent: u32, loops: usize) -> Self {
        let mut maze = Maze { open: [0; CELLS] };
        // A "growing tree" spanning tree of the first quarter.
        let mut visited = [false; CELLS];
        let start = cell_at(
            rng.range_u32(0, MIDDLE as u32 - 1) as usize,
            rng.range_u32(0, MIDDLE as u32) as usize,
        );
        visited[start] = true;
        let mut growing = vec![start];
        while !growing.is_empty() {
            let i = if rng.chance(newest_percent) {
                growing.len() - 1
            } else {
                rng.range_u32(0, growing.len() as u32 - 1) as usize
            };
            let cell = growing[i];
            let ways: Vec<(Dir, usize)> = Dir::ALL
                .into_iter()
                .filter_map(|d| neighbour(cell, d).map(|n| (d, n)))
                .filter(|(_, n)| in_first_quarter(*n) && !visited[*n])
                .collect();
            if ways.is_empty() {
                growing.remove(i);
                continue;
            }
            let (dir, next) = ways[rng.range_u32(0, ways.len() as u32 - 1) as usize];
            maze.carve_four(cell, dir);
            visited[next] = true;
            growing.push(next);
        }
        // The four quarters meet at the beacon...
        maze.carve_four(cell_at(MIDDLE - 1, MIDDLE), Dir::Right);
        // ...and through a few openings in the hedge between neighbouring quarters (not the one
        // right by the beacon, which would only make a tiny loop).
        let mut rows: Vec<usize> = (0..MIDDLE - 1).collect();
        for _ in 0..loops.min(rows.len()) {
            let y = rows.remove(rng.range_u32(0, rows.len() as u32 - 1) as usize);
            maze.carve_four(cell_at(MIDDLE - 1, y), Dir::Right);
        }
        maze
    }

    /// Steps from every cell to the beacon, along the maze.
    pub fn steps_to_middle(&self) -> [u16; CELLS] {
        let mut steps = [u16::MAX; CELLS];
        steps[middle()] = 0;
        let mut queue = std::collections::VecDeque::from([middle()]);
        while let Some(cell) = queue.pop_front() {
            for dir in Dir::ALL {
                if let Some(next) = neighbour(cell, dir)
                    && self.is_open(cell, dir)
                    && steps[next] == u16::MAX
                {
                    steps[next] = steps[cell] + 1;
                    queue.push_back(next);
                }
            }
        }
        steps
    }
}

/// A position in milli-cells. Walkers are always on a line between two cell centres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
}

impl Pos {
    pub fn of(cell: usize) -> Self {
        let (x, y) = cell_xy(cell);
        Pos {
            x: x as i32 * UNIT,
            y: y as i32 * UNIT,
        }
    }

    /// The nearest cell.
    pub fn cell(self) -> usize {
        cell_at(
            ((self.x + UNIT / 2) / UNIT) as usize,
            ((self.y + UNIT / 2) / UNIT) as usize,
        )
    }

    pub fn at_centre(self) -> bool {
        self.x % UNIT == 0 && self.y % UNIT == 0
    }

    /// The cells at the two ends of the line this position is on (the same cell twice at a
    /// centre).
    pub fn ends(self) -> (usize, usize) {
        let (x, y) = ((self.x / UNIT) as usize, (self.y / UNIT) as usize);
        if self.x % UNIT != 0 {
            (cell_at(x, y), cell_at(x + 1, y))
        } else if self.y % UNIT != 0 {
            (cell_at(x, y), cell_at(x, y + 1))
        } else {
            (cell_at(x, y), cell_at(x, y))
        }
    }

    pub fn distance_squared(self, other: Pos) -> i64 {
        let (dx, dy) = ((self.x - other.x) as i64, (self.y - other.y) as i64);
        dx * dx + dy * dy
    }

    /// How far it is to the next cell centre going `dir` (a whole cell from a centre).
    pub fn to_next_centre(self, dir: Dir) -> i32 {
        let along = if dir.horizontal() { self.x } else { self.y };
        let into = along.rem_euclid(UNIT);
        let left = match dir {
            Dir::Right | Dir::Down => UNIT - into,
            Dir::Left | Dir::Up => into,
        };
        if left == 0 { UNIT } else { left }
    }

    fn moved(self, dir: Dir, amount: i32) -> Pos {
        let (dx, dy) = dir.delta();
        Pos {
            x: self.x + dx * amount,
            y: self.y + dy * amount,
        }
    }
}

/// The directions the stick asks for: the stronger axis first, then the other one if it's pushed
/// too, so a diagonal takes whichever of its two turns is open.
pub fn wanted(input: &Input) -> [Option<Dir>; 2] {
    let (x, y) = (input.stick_x as i32, input.stick_y as i32);
    let horizontal = (x.abs() >= DEAD_ZONE).then_some(if x > 0 { Dir::Right } else { Dir::Left });
    let vertical = (y.abs() >= DEAD_ZONE).then_some(if y > 0 { Dir::Up } else { Dir::Down });
    if x.abs() >= y.abs() {
        [horizontal, vertical]
    } else {
        [vertical, horizontal]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Walker {
    pub pos: Pos,
    /// The way it last moved (or faces, standing still).
    pub heading: Dir,
    /// False for seats outside a duel.
    pub playing: bool,
    pub ghost: bool,
    /// The corner this walker starts the maze in (the ghost starts on the beacon).
    pub start: usize,
    /// Moved on the last tick.
    pub moving: bool,
    /// The play tick this runner reached the beacon, this maze.
    pub finished_at: Option<u32>,
    pub flare_used: bool,
    /// The play tick of this maze's flare.
    pub flared_at: Option<u32>,
    /// Ticks left standing in the corner after being caught.
    pub frozen: u16,
    /// Ticks left before the ghost can catch this runner again.
    pub safe: u16,
    /// The play tick this runner was last caught.
    pub caught_at: Option<u32>,
    /// The ghost: runners caught, this match.
    pub catches: u16,
    pub points: u32,
    /// Points scored in the last scored maze, for the view.
    pub gained: u32,
}

impl Walker {
    pub fn finished(&self) -> bool {
        self.finished_at.is_some()
    }

    /// Steps along the maze's lines as the stick asks, `speed` units at most. `momentum` says
    /// it moved last tick (only then does a held turn that isn't open yet keep it going).
    /// Returns whether it moved.
    fn advance(
        &mut self,
        maze: &Maze,
        wanted: [Option<Dir>; 2],
        speed: i32,
        momentum: bool,
    ) -> bool {
        if wanted[0].is_none() {
            return false;
        }
        let ghost = self.ghost;
        let mut budget = speed;
        let mut moved = false;
        while budget > 0 {
            let dir = if self.pos.at_centre() {
                let cell = self.pos.cell();
                if !ghost && cell == middle() && moved {
                    break;
                }
                let turn = wanted
                    .into_iter()
                    .flatten()
                    .find(|d| maze.passable(cell, *d, ghost));
                match turn {
                    Some(dir) => dir,
                    // Holding a turn that isn't open yet keeps you going, so you can turn early.
                    None if (momentum || moved)
                        && maze.passable(cell, self.heading, ghost)
                        && wanted[0] != Some(self.heading.opposite()) =>
                    {
                        self.heading
                    }
                    None => break,
                }
            } else if wanted.contains(&Some(self.heading.opposite())) {
                self.heading.opposite()
            } else if let Some(turn) =
                wanted[0].filter(|d| d.horizontal() != self.heading.horizontal())
            {
                // A turn pressed just after passing it still takes it, unless it's open ahead
                // too.
                let ahead = self.pos.to_next_centre(self.heading);
                let back = UNIT - ahead;
                let behind = self.pos.moved(self.heading.opposite(), back).cell();
                let next = self.pos.moved(self.heading, ahead).cell();
                if back <= CORNER_SLACK
                    && maze.passable(behind, turn, ghost)
                    && !maze.passable(next, turn, ghost)
                {
                    self.heading.opposite()
                } else {
                    self.heading
                }
            } else {
                self.heading
            };
            let step = budget.min(self.pos.to_next_centre(dir));
            // Between cells the ghost can't turn back onto the beacon either.
            if ghost && self.pos.moved(dir, self.pos.to_next_centre(dir)) == Pos::of(middle()) {
                break;
            }
            self.pos = self.pos.moved(dir, step);
            self.heading = dir;
            budget -= step;
            moved = true;
        }
        moved
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Intro {
        t: u16,
    },
    /// A new maze: everyone waits in their corner.
    Ready {
        t: u16,
    },
    Racing {
        t: u16,
    },
    /// The maze is scored.
    Result {
        t: u16,
    },
    Finished,
}

/// 1v3's sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Ghost,
    Trio,
}

#[derive(Debug, Clone, Hash)]
pub struct State {
    pub seed: u64,
    pub players: PlayerSetup,
    pub format: Format,
    pub phase: Phase,
    rng: Rng,
    /// Ticks since the intro ended, for effects and bots.
    pub tick: u32,
    pub maze: Maze,
    /// Steps from each cell to the beacon in this maze.
    pub steps: [u16; CELLS],
    /// The play tick each cell was lit, this maze.
    pub lit_at: [Option<u32>; CELLS],
    pub walkers: [Walker; MAX_PLAYERS],
    /// The maze being played, from 0.
    pub round: u8,
    /// 1v3: mazes won by each side, and who took the last scored one.
    pub ghost_rounds: u8,
    pub trio_rounds: u8,
    pub round_winner: Option<Side>,
    /// Set once the last maze is scored.
    pub outcome: Option<Outcome>,
}

impl State {
    pub fn walker(&self, seat: Seat) -> &Walker {
        &self.walkers[seat.index()]
    }

    pub fn ghost(&self) -> Option<Seat> {
        match self.format {
            Format::OneVsThree { solo } => Some(solo),
            _ => None,
        }
    }

    /// Seats in the game (benched duel seats aren't).
    pub fn playing(&self) -> impl Iterator<Item = Seat> + '_ {
        self.players
            .seats()
            .filter(|s| self.walkers[s.index()].playing)
    }

    /// Seats racing for the beacon: everyone playing but the ghost.
    pub fn runners(&self) -> impl Iterator<Item = Seat> + '_ {
        self.playing().filter(|s| !self.walkers[s.index()].ghost)
    }

    pub fn is_lit(&self, cell: usize) -> bool {
        self.lit_at[cell].is_some()
    }

    pub fn race_ticks(&self) -> u16 {
        if self.ghost().is_some() {
            GHOST_RACE_TICKS
        } else {
            RACE_TICKS
        }
    }

    pub fn ticks_left(&self) -> u16 {
        match self.phase {
            Phase::Racing { t } => self.race_ticks().saturating_sub(t),
            Phase::Intro { .. } | Phase::Ready { .. } => self.race_ticks(),
            Phase::Result { .. } | Phase::Finished => 0,
        }
    }

    pub fn last_round(&self) -> bool {
        self.round + 1 >= MAZES
    }

    /// How far a position still is from the beacon along the maze, in position units.
    pub fn remaining(&self, pos: Pos) -> i32 {
        let (a, b) = pos.ends();
        let via = |cell: usize| {
            let along = (pos.x - Pos::of(cell).x).abs() + (pos.y - Pos::of(cell).y).abs();
            self.steps[cell] as i32 * UNIT + along
        };
        via(a).min(via(b))
    }

    fn light(&mut self, at: Pos, radius: i32) {
        let reach = (radius as i64).pow(2);
        for cell in 0..CELLS {
            if self.lit_at[cell].is_none() && Pos::of(cell).distance_squared(at) <= reach {
                self.lit_at[cell] = Some(self.tick);
            }
        }
    }

    /// A fresh maze from the seed, with everyone back at the start.
    fn start_maze(&mut self) {
        self.maze = Maze::generate(&mut self.rng);
        self.steps = self.maze.steps_to_middle();
        self.lit_at = [None; CELLS];
        self.round_winner = None;
        // Runners take the corners in turn order, shifted each maze, so with three nobody is
        // always the one in the middle.
        let runners: Vec<Seat> = self.runners().collect();
        let corners: &[usize] = match runners.len() {
            0..=2 => &[0, 2],
            3 => &[0, 1, 2],
            _ => &[0, 1, 2, 3],
        };
        for (i, seat) in runners.iter().enumerate() {
            let k = corners[(i + self.round as usize) % runners.len()];
            self.walkers[seat.index()].start = corner(k);
        }
        for walker in &mut self.walkers {
            walker.pos = Pos::of(walker.start);
            walker.heading = if walker.ghost {
                Dir::Down
            } else {
                // Face along the hedge out of the corner.
                [Dir::Right, Dir::Down, Dir::Left, Dir::Up]
                    [(0..4).find(|k| corner(*k) == walker.start).unwrap_or(0)]
            };
            walker.moving = false;
            walker.finished_at = None;
            walker.flare_used = false;
            walker.flared_at = None;
            walker.frozen = 0;
            walker.safe = 0;
            walker.caught_at = None;
        }
        self.light_up();
    }

    /// Lanterns and the beacon light what's near them.
    fn light_up(&mut self) {
        self.light(Pos::of(middle()), BEACON_RADIUS);
        for seat in self.playing().collect::<Vec<_>>() {
            let pos = self.walkers[seat.index()].pos;
            self.light(pos, LIGHT_RADIUS);
        }
    }

    fn step_race(&mut self, inputs: &Inputs) {
        for seat in self.playing().collect::<Vec<_>>() {
            let maze = self.maze;
            let walker = &mut self.walkers[seat.index()];
            walker.safe = walker.safe.saturating_sub(1);
            if walker.finished() {
                walker.moving = false;
                continue;
            }
            if walker.frozen > 0 {
                walker.frozen -= 1;
                walker.moving = false;
                continue;
            }
            let speed = if walker.ghost { GHOST_SPEED } else { SPEED };
            let momentum = walker.moving;
            walker.moving = walker.advance(
                &maze,
                wanted(&inputs.current[seat.index()]),
                speed,
                momentum,
            );
            if inputs.just_pressed(seat, Button::A) && !walker.flare_used {
                walker.flare_used = true;
                walker.flared_at = Some(self.tick);
                let at = walker.pos;
                self.light(at, FLARE_RADIUS);
            }
        }
        self.light_up();
        for seat in self.runners().collect::<Vec<_>>() {
            let walker = &mut self.walkers[seat.index()];
            if !walker.finished() && walker.pos == Pos::of(middle()) {
                walker.finished_at = Some(self.tick);
            }
        }
        self.catch();
    }

    /// The ghost sends any runner it touches back to their corner, once it has left the beacon
    /// (so it can't keep everyone off it by staying put).
    fn catch(&mut self) {
        let Some(ghost) = self.ghost() else {
            return;
        };
        let at = self.walkers[ghost.index()].pos;
        if at.cell() == middle() {
            return;
        }
        for seat in self.runners().collect::<Vec<_>>() {
            let maze = self.maze;
            let runner = &mut self.walkers[seat.index()];
            if runner.finished() || runner.safe > 0 || !touching(&maze, at, runner.pos) {
                continue;
            }
            runner.pos = Pos::of(runner.start);
            runner.frozen = RESPAWN_TICKS;
            runner.safe = SAFE_TICKS;
            runner.caught_at = Some(self.tick);
            self.walkers[ghost.index()].catches += 1;
        }
    }

    fn race_over(&self, t: u16) -> bool {
        if t >= self.race_ticks() {
            return true;
        }
        let finished = self
            .runners()
            .filter(|s| self.walker(*s).finished())
            .count();
        if self.ghost().is_some() {
            finished >= TRIO_NEEDED
        } else {
            finished >= self.runners().count().saturating_sub(1).max(1)
        }
    }

    fn score_maze(&mut self) {
        for walker in &mut self.walkers {
            walker.gained = 0;
        }
        if self.ghost().is_some() {
            let finished = self
                .runners()
                .filter(|s| self.walker(*s).finished())
                .count();
            let winner = if finished >= TRIO_NEEDED {
                self.trio_rounds += 1;
                Side::Trio
            } else {
                self.ghost_rounds += 1;
                Side::Ghost
            };
            self.round_winner = Some(winner);
            if self.ghost_rounds.max(self.trio_rounds) >= ROUNDS_TO_WIN || self.last_round() {
                let mut scores = [0; MAX_PLAYERS];
                scores[0] = self.ghost_rounds as u32;
                scores[1] = self.trio_rounds as u32;
                self.finish(scores);
            }
            return;
        }
        // Earlier is better; the unfinished are ranked by what's left.
        let runners: Vec<Seat> = self.runners().collect();
        let keys: Vec<(u8, i64)> = runners
            .iter()
            .map(|s| match self.walker(*s).finished_at {
                Some(tick) => (0, tick as i64),
                None => (1, self.remaining(self.walker(*s).pos) as i64),
            })
            .collect();
        for (seat, key) in runners.iter().zip(&keys) {
            let better = keys.iter().filter(|o| *o < key).count();
            let points = (runners.len() - 1 - better) as u32;
            let walker = &mut self.walkers[seat.index()];
            walker.gained = points;
            walker.points += points;
        }
        if self.last_round() {
            let mut scores = [0; MAX_PLAYERS];
            for seat in &runners {
                if let Some(team) = self.format.team(*seat) {
                    scores[team as usize] += self.walker(*seat).points;
                }
            }
            self.finish(scores);
        }
    }

    fn finish(&mut self, team_scores: [u32; MAX_PLAYERS]) {
        self.outcome = Some(Outcome::from_team_scores(
            &self.players,
            &self.format,
            &team_scores,
        ));
    }
}

/// Close enough to touch, and not with a hedge between.
fn touching(maze: &Maze, a: Pos, b: Pos) -> bool {
    if a.distance_squared(b) > (CATCH_REACH as i64).pow(2) {
        return false;
    }
    let (from, to) = (a.cell(), b.cell());
    from == to
        || Dir::ALL
            .into_iter()
            .any(|d| neighbour(from, d) == Some(to) && maze.is_open(from, d))
}

impl Minigame for HedgeMaze {
    type State = State;

    const ID: &'static str = "hedge-maze";
    const SUPPORTED_FORMATS: &'static [FormatKind] = &[
        FormatKind::FreeForAll,
        FormatKind::OneVsThree,
        FormatKind::TwoVsTwo,
        FormatKind::Duel,
    ];
    const MAX_TICKS: u32 = INTRO_TICKS as u32
        + MAZES as u32 * (READY_TICKS + RACE_TICKS + FINAL_RESULT_TICKS) as u32
        + 60;

    fn new(seed: u64, players: PlayerSetup, format: Format) -> State {
        let ghost = match format {
            Format::OneVsThree { solo } => Some(solo),
            _ => None,
        };
        let mut walkers = [Walker {
            pos: Pos::default(),
            heading: Dir::Right,
            playing: false,
            ghost: false,
            start: middle(),
            moving: false,
            finished_at: None,
            flare_used: false,
            flared_at: None,
            frozen: 0,
            safe: 0,
            caught_at: None,
            catches: 0,
            points: 0,
            gained: 0,
        }; MAX_PLAYERS];
        // Seats outside a duel sit this one out.
        for seat in players.seats().filter(|s| format.team(*s).is_some()) {
            let walker = &mut walkers[seat.index()];
            walker.playing = true;
            walker.ghost = Some(seat) == ghost;
        }
        let mut state = State {
            seed,
            players,
            format,
            phase: Phase::Intro { t: 0 },
            rng: Rng::new(seed),
            tick: 0,
            maze: Maze { open: [0; CELLS] },
            steps: [0; CELLS],
            lit_at: [None; CELLS],
            walkers,
            round: 0,
            ghost_rounds: 0,
            trio_rounds: 0,
            round_winner: None,
            outcome: None,
        };
        state.start_maze();
        state
    }

    fn step(state: &mut State, inputs: &Inputs) {
        if let Phase::Intro { t } = state.phase {
            state.phase = if t + 1 >= INTRO_TICKS {
                Phase::Racing { t: 0 }
            } else {
                Phase::Intro { t: t + 1 }
            };
            return;
        }
        if state.phase == Phase::Finished {
            return;
        }
        state.tick += 1;
        state.phase = match state.phase {
            Phase::Ready { t } if t + 1 >= READY_TICKS => Phase::Racing { t: 0 },
            Phase::Ready { t } => Phase::Ready { t: t + 1 },
            Phase::Racing { t } => {
                state.step_race(inputs);
                if state.race_over(t + 1) {
                    state.score_maze();
                    Phase::Result { t: 0 }
                } else {
                    Phase::Racing { t: t + 1 }
                }
            }
            Phase::Result { t } => {
                for walker in &mut state.walkers {
                    walker.moving = false;
                }
                let length = if state.outcome.is_some() {
                    FINAL_RESULT_TICKS
                } else {
                    RESULT_TICKS
                };
                if t + 1 < length {
                    Phase::Result { t: t + 1 }
                } else if state.outcome.is_some() {
                    Phase::Finished
                } else {
                    state.round += 1;
                    state.start_maze();
                    Phase::Ready { t: 0 }
                }
            }
            Phase::Intro { .. } | Phase::Finished => unreachable!(),
        };
    }

    fn outcome(state: &State) -> Option<Outcome> {
        if state.phase == Phase::Finished {
            state.outcome
        } else {
            None
        }
    }

    fn bot_input(state: &State, seat: Seat, difficulty: Difficulty) -> Input {
        let walker = state.walker(seat);
        if !matches!(state.phase, Phase::Racing { .. })
            || !walker.playing
            || walker.finished()
            || walker.frozen > 0
        {
            return Input::default();
        }
        bot::input(state, seat, difficulty)
    }
}

mod bot {
    use super::*;

    /// A field's cost for one step onto a cell the bot has seen.
    const STEP: u32 = 10;
    const UNREACHABLE: u32 = u32::MAX;
    /// Runners steer clear of cells this close to the ghost...
    const BERTH: i32 = 2500;

    struct Skill {
        /// Ticks before a newly lit cell's hedges count for the bot: how fast it takes in what
        /// it sees.
        delay: u32,
        /// The most a dark cell costs on top of [`STEP`]: a guess per cell, different for each
        /// bot and maze, of how promising that way looks.
        noise: u32,
        /// It fires its flare once this many dark cells are in reach.
        flare_dark: usize,
        /// ...and this is what a cell near the ghost costs a runner extra.
        fear: u32,
        /// The ghost's chase weighs how close a runner is to the beacon by this, against how
        /// close the runner is to the ghost.
        urgency: u32,
        /// The ghost heads a runner off on its way home instead of following it.
        intercepts: bool,
    }

    fn skill(difficulty: Difficulty) -> Skill {
        match difficulty {
            Difficulty::Easy => Skill {
                delay: 30,
                noise: 30,
                flare_dark: 15,
                fear: 20,
                urgency: 1,
                intercepts: false,
            },
            Difficulty::Normal => Skill {
                delay: 15,
                noise: 15,
                flare_dark: 14,
                fear: 40,
                urgency: 2,
                intercepts: true,
            },
            Difficulty::Hard => Skill {
                delay: 6,
                noise: 6,
                flare_dark: 14,
                fear: 60,
                urgency: 2,
                intercepts: true,
            },
        }
    }

    /// Whether the bot has taken in this cell's hedges yet.
    fn seen(state: &State, cell: usize, skill: &Skill) -> bool {
        state.lit_at[cell].is_some_and(|t| t + skill.delay <= state.tick)
    }

    /// Whether the bot believes it can step from `cell` towards `dir`: hedges it hasn't seen
    /// are assumed open.
    fn believed(state: &State, cell: usize, dir: Dir, ghost: bool, skill: &Skill) -> Option<usize> {
        let next = neighbour(cell, dir)?;
        if ghost && next == middle() {
            return None;
        }
        if seen(state, cell, skill) || seen(state, next, skill) {
            state.maze.is_open(cell, dir).then_some(next)
        } else {
            Some(next)
        }
    }

    /// What it costs, as the bot believes, to get from every cell to one of `goals`.
    fn field(
        state: &State,
        seat: Seat,
        skill: &Skill,
        goals: &[usize],
        extra: impl Fn(usize) -> u32,
    ) -> [u32; CELLS] {
        let ghost = state.walker(seat).ghost;
        let mut cost = [UNREACHABLE; CELLS];
        let mut queue = BinaryHeap::new();
        for &goal in goals {
            cost[goal] = 0;
            queue.push(Reverse((0, goal)));
        }
        while let Some(Reverse((c, cell))) = queue.pop() {
            if c > cost[cell] {
                continue;
            }
            for dir in Dir::ALL {
                // The walker would step from `next` onto `cell`.
                let Some(next) = believed(state, cell, dir, false, skill) else {
                    continue;
                };
                // The ghost never steps onto the beacon, though it may start there.
                if ghost && (next == middle() || (cell == middle() && c > 0)) {
                    continue;
                }
                let guess = if seen(state, cell, skill) {
                    0
                } else {
                    noise(state, seat, cell, skill)
                };
                let through = c + STEP + guess + extra(cell);
                if through < cost[next] {
                    cost[next] = through;
                    queue.push(Reverse((through, next)));
                }
            }
        }
        cost
    }

    /// Which quarter turn the walker sees the maze from: its start corner's. Bots break ties
    /// and draw their guesses in that frame, so a bot plays the same maze the same way from
    /// any corner.
    fn frame(state: &State, seat: Seat) -> usize {
        let start = state.walker(seat).start;
        (0..4).find(|k| corner(*k) == start).unwrap_or(0)
    }

    fn noise(state: &State, seat: Seat, cell: usize, skill: &Skill) -> u32 {
        let mut cell = cell;
        for _ in 0..(4 - frame(state, seat)) % 4 {
            cell = turn(cell);
        }
        let mut rng = Rng::new(
            state.seed
                ^ (state.round as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15)
                ^ (seat.0 as u64 + 1).wrapping_mul(0xbf58_476d_1ce4_e5b9)
                ^ (cell as u64 + 1).wrapping_mul(0x94d0_49bb_1331_11eb),
        );
        rng.range_u32(0, skill.noise)
    }

    /// A way to go, and a second, weaker push on the other axis (taken if the first is a
    /// hedge, or, if it's back the way the walker came, turning it round).
    type Push = (Dir, Option<Dir>);

    fn stick((first, second): Push) -> Input {
        let (x, y) = first.delta();
        let (sx, sy) = second.map(|d| d.delta()).unwrap_or((0, 0));
        Input {
            buttons: 0,
            stick_x: (x * 127 + sx * 80) as i8,
            stick_y: (-y * 127 - sy * 80) as i8,
        }
    }

    /// The way downhill on `field` from where the walker is: at a centre, towards the cheapest
    /// neighbour; between two, towards the cheaper end, already holding the turn it will take
    /// there.
    fn downhill(state: &State, seat: Seat, skill: &Skill, field: &[u32; CELLS]) -> Option<Push> {
        let walker = state.walker(seat);
        let frame = frame(state, seat);
        let best_from = |cell: usize| {
            Dir::ALL
                .into_iter()
                .filter_map(|d| {
                    believed(state, cell, d, walker.ghost, skill).map(|n| (field[n], d))
                })
                .filter(|(cost, _)| *cost < field[cell])
                .min_by_key(|(cost, d)| (*cost, (*d as usize + 4 - frame) % 4))
                .map(|(_, d)| d)
        };
        let pos = walker.pos;
        if pos.at_centre() {
            return best_from(pos.cell()).map(|d| (d, None));
        }
        let heading = walker.heading;
        let ahead_travel = pos.to_next_centre(heading);
        let ahead = pos.moved(heading, ahead_travel).cell();
        let behind = pos.moved(heading.opposite(), UNIT - ahead_travel).cell();
        let via = |cell: usize, travel: i32| {
            field[cell].saturating_add((travel as u32) * STEP / UNIT as u32)
        };
        if via(ahead, ahead_travel) <= via(behind, UNIT - ahead_travel) {
            return Some((best_from(ahead).unwrap_or(heading), None));
        }
        let back = heading.opposite();
        match best_from(behind) {
            Some(turn) if turn.horizontal() != heading.horizontal() => Some((turn, Some(back))),
            _ => Some((back, None)),
        }
    }

    pub fn input(state: &State, seat: Seat, difficulty: Difficulty) -> Input {
        let skill = skill(difficulty);
        let walker = state.walker(seat);
        let push = if walker.ghost {
            hunt(state, seat, &skill)
        } else {
            let ghost = state.ghost().map(|g| state.walker(g).pos);
            let berth = (BERTH as i64).pow(2);
            let field = field(state, seat, &skill, &[middle()], |cell| match ghost {
                Some(at) if Pos::of(cell).distance_squared(at) < berth => skill.fear,
                _ => 0,
            });
            downhill(state, seat, &skill, &field)
        };
        let mut input = push.map(stick).unwrap_or_default();
        if !walker.flare_used && dark_in_reach(state, walker.pos) >= skill.flare_dark {
            input.set(Button::A, true);
        }
        input
    }

    fn dark_in_reach(state: &State, at: Pos) -> usize {
        let reach = (FLARE_RADIUS as i64).pow(2);
        (0..CELLS)
            .filter(|c| !state.is_lit(*c) && Pos::of(*c).distance_squared(at) <= reach)
            .count()
    }

    /// The ghost chases the runner it can reach soonest, weighed by how close that runner is to
    /// the beacon; with nobody to chase it waits by the beacon.
    fn hunt(state: &State, seat: Seat, skill: &Skill) -> Option<Push> {
        let at = state.walker(seat).pos.cell();
        let from_ghost = field(state, seat, skill, &[at], |_| 0);
        let target = state
            .runners()
            .map(|s| state.walker(s))
            .filter(|w| !w.finished() && w.frozen == 0 && w.safe == 0)
            .min_by_key(|w| {
                let cell = w.pos.cell();
                from_ghost[cell].saturating_add(state.steps[cell] as u32 * STEP * skill.urgency)
            });
        let goals: Vec<usize> = match target {
            Some(runner) if skill.intercepts => vec![intercept(state, runner, &from_ghost)],
            Some(runner) => vec![runner.pos.cell()],
            None => Dir::ALL
                .into_iter()
                .filter_map(|d| neighbour(middle(), d))
                .collect(),
        };
        let field = field(state, seat, skill, &goals, |_| 0);
        downhill(state, seat, skill, &field)
    }

    /// The first cell on the runner's way to the beacon that the ghost gets to no later than the
    /// runner, to meet it head on; the runner's own cell if there's none.
    fn intercept(state: &State, runner: &Walker, from_ghost: &[u32; CELLS]) -> usize {
        let start = runner.pos.cell();
        let mut cell = start;
        for i in 0..=state.steps[start] as i64 {
            if cell == middle() {
                break;
            }
            let ghost_ticks =
                from_ghost[cell] as i64 * UNIT as i64 / STEP as i64 / GHOST_SPEED as i64;
            if ghost_ticks <= i * UNIT as i64 / SPEED as i64 {
                return cell;
            }
            let Some(next) = Dir::ALL.into_iter().find_map(|d| {
                neighbour(cell, d)
                    .filter(|n| state.maze.is_open(cell, d) && state.steps[*n] < state.steps[cell])
            }) else {
                break;
            };
            cell = next;
        }
        start
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{Run, RunConfig};

    const NONE: [Input; MAX_PLAYERS] = [Input {
        buttons: 0,
        stick_x: 0,
        stick_y: 0,
    }; MAX_PLAYERS];

    fn new_run(players: usize, format: Format) -> Run<HedgeMaze> {
        Run::new(7, PlayerSetup::first(players), format, RunConfig::default())
    }

    fn run_until(run: &mut Run<HedgeMaze>, done: impl Fn(&State) -> bool) {
        for _ in 0..HedgeMaze::MAX_TICKS {
            if done(&run.game) {
                return;
            }
            run.step(NONE);
        }
        panic!("never reached");
    }

    fn racing(players: usize, format: Format) -> Run<HedgeMaze> {
        let mut run = new_run(players, format);
        run_until(&mut run, |g| matches!(g.phase, Phase::Racing { .. }));
        run
    }

    fn hold(dir: Dir) -> Input {
        let (x, y) = dir.delta();
        Input {
            buttons: 0,
            stick_x: (x * 127) as i8,
            stick_y: (-y * 127) as i8,
        }
    }

    /// One seat holds `dir` for `ticks`; everyone else stands still.
    fn walk(run: &mut Run<HedgeMaze>, seat: usize, dir: Dir, ticks: u32) {
        for _ in 0..ticks {
            let mut inputs = NONE;
            inputs[seat] = hold(dir);
            run.step(inputs);
        }
    }

    /// A maze with only the hedges `open` lets through, for movement tests.
    fn open_maze(run: &mut Run<HedgeMaze>, passages: &[(usize, Dir)]) {
        let mut maze = Maze { open: [0; CELLS] };
        for &(cell, dir) in passages {
            maze.carve(cell, dir);
        }
        run.game.maze = maze;
    }

    /// Walks the shortest way to the beacon (the bots' knowledge isn't needed: tests know it).
    fn go_home(run: &mut Run<HedgeMaze>, seat: usize) {
        go_home_for(run, seat, HedgeMaze::MAX_TICKS);
    }

    fn go_home_for(run: &mut Run<HedgeMaze>, seat: usize, ticks: u32) {
        for _ in 0..ticks {
            let walker = run.game.walkers[seat];
            if walker.finished() || !matches!(run.game.phase, Phase::Racing { .. }) {
                return;
            }
            let pos = walker.pos;
            let game = &run.game;
            let downhill = |cell: usize| {
                Dir::ALL.into_iter().find(|d| {
                    game.maze.is_open(cell, *d)
                        && neighbour(cell, *d).is_some_and(|n| game.steps[n] < game.steps[cell])
                })
            };
            // Like a player: head for the nearer end, already holding the turn to take there.
            let dir = if pos.at_centre() {
                downhill(pos.cell()).unwrap()
            } else {
                let ahead = pos
                    .moved(walker.heading, pos.to_next_centre(walker.heading))
                    .cell();
                let (a, b) = pos.ends();
                let behind = if a == ahead { b } else { a };
                if game.steps[ahead] < game.steps[behind] {
                    downhill(ahead).unwrap_or(walker.heading)
                } else {
                    walker.heading.opposite()
                }
            };
            let mut inputs = NONE;
            inputs[seat] = hold(dir);
            run.step(inputs);
        }
    }

    #[test]
    fn every_corner_is_as_far_from_the_beacon() {
        for seed in 0..50 {
            let mut rng = Rng::new(seed);
            let maze = Maze::generate(&mut rng);
            let steps = maze.steps_to_middle();
            assert!(steps.iter().all(|s| *s != u16::MAX), "all connected");
            let corners: Vec<u16> = (0..4).map(|k| steps[corner(k)]).collect();
            assert!(corners.iter().all(|s| *s == corners[0]), "{corners:?}");
            // Turned a quarter, the maze is the same maze.
            for cell in 0..CELLS {
                for dir in Dir::ALL {
                    assert_eq!(
                        maze.is_open(cell, dir),
                        maze.is_open(turn(cell), dir.clockwise())
                    );
                }
            }
            // A tree plus the loops: one passage fewer than cells, plus four per loop.
            let passages: usize = maze.open.iter().map(|o| o.count_ones() as usize).sum();
            assert_eq!(passages / 2, CELLS - 1 + 4 * LOOPS);
        }
    }

    #[test]
    fn nobody_moves_during_the_intro() {
        let mut run = new_run(2, Format::FreeForAll);
        let start = run.game.walkers[0].pos;
        for _ in 0..INTRO_TICKS - 1 {
            let mut inputs = NONE;
            inputs[0] = hold(Dir::Right).with(Button::A);
            run.step(inputs);
        }
        assert_eq!(run.game.walkers[0].pos, start);
        assert!(!run.game.walkers[0].flare_used);
    }

    #[test]
    fn walkers_follow_corridors_and_stop_at_hedges() {
        let mut run = racing(1, Format::FreeForAll);
        let start = cell_at(0, 0);
        run.game.walkers[0].pos = Pos::of(start);
        // A corridor right two cells, then down one.
        open_maze(
            &mut run,
            &[
                (start, Dir::Right),
                (cell_at(1, 0), Dir::Right),
                (cell_at(2, 0), Dir::Down),
            ],
        );
        // Down isn't open in the corner: nothing happens.
        walk(&mut run, 0, Dir::Down, 10);
        assert_eq!(run.game.walkers[0].pos, Pos::of(start));
        walk(&mut run, 0, Dir::Right, 100);
        assert_eq!(
            run.game.walkers[0].pos,
            Pos::of(cell_at(2, 0)),
            "stops at the end"
        );
        walk(&mut run, 0, Dir::Down, 100);
        assert_eq!(run.game.walkers[0].pos, Pos::of(cell_at(2, 1)));
    }

    #[test]
    fn holding_a_turn_early_takes_it_when_it_opens() {
        let mut run = racing(1, Format::FreeForAll);
        let start = cell_at(0, 0);
        run.game.walkers[0].pos = Pos::of(start);
        run.game.walkers[0].heading = Dir::Right;
        open_maze(
            &mut run,
            &[
                (start, Dir::Right),
                (cell_at(1, 0), Dir::Right),
                (cell_at(2, 0), Dir::Down),
                (cell_at(2, 0), Dir::Right),
            ],
        );
        // Start right, then hold down the whole way: it goes on to the turn and takes it.
        walk(&mut run, 0, Dir::Right, 1);
        walk(&mut run, 0, Dir::Down, 40);
        assert_eq!(run.game.walkers[0].pos, Pos::of(cell_at(2, 1)));
    }

    #[test]
    fn walkers_turn_back_between_cells_and_stop_when_let_go() {
        let mut run = racing(1, Format::FreeForAll);
        let start = cell_at(0, 0);
        run.game.walkers[0].pos = Pos::of(start);
        open_maze(&mut run, &[(start, Dir::Right)]);
        walk(&mut run, 0, Dir::Right, 4);
        let mid = run.game.walkers[0].pos;
        assert_eq!(mid, Pos { x: 4 * SPEED, y: 0 });
        run.step(NONE);
        assert_eq!(run.game.walkers[0].pos, mid, "let go: stands still");
        walk(&mut run, 0, Dir::Left, 2);
        assert_eq!(run.game.walkers[0].pos, Pos { x: 2 * SPEED, y: 0 });
    }

    #[test]
    fn a_diagonal_takes_whichever_turn_is_open() {
        let mut run = racing(1, Format::FreeForAll);
        let start = cell_at(0, 0);
        run.game.walkers[0].pos = Pos::of(start);
        open_maze(&mut run, &[(start, Dir::Down)]);
        let mut inputs = NONE;
        // Mostly right, a little down: right is a hedge, so down.
        inputs[0] = Input {
            buttons: 0,
            stick_x: 120,
            stick_y: -60,
        };
        run.step(inputs);
        assert_eq!(run.game.walkers[0].pos, Pos { x: 0, y: SPEED });
    }

    #[test]
    fn lanterns_light_the_hedges_near_them_for_good() {
        let mut run = racing(2, Format::FreeForAll);
        let game = &run.game;
        // The beacon and both corners start lit.
        assert!(game.is_lit(middle()));
        assert!(game.is_lit(corner(0)) && game.is_lit(corner(2)));
        assert!(game.is_lit(cell_at(1, 1)), "diagonally next to the corner");
        assert!(!game.is_lit(cell_at(2, 0)));
        assert!(!game.is_lit(corner(1)));
        // Walk away: what was lit stays lit.
        let lit_before = game.lit_at.iter().filter(|l| l.is_some()).count();
        go_home(&mut run, 0);
        assert!(run.game.is_lit(corner(0)));
        let lit_after = run.game.lit_at.iter().filter(|l| l.is_some()).count();
        assert!(lit_after > lit_before);
    }

    #[test]
    fn a_flare_lights_a_wide_circle_once_per_maze() {
        let mut run = racing(2, Format::FreeForAll);
        run.game.walkers[0].pos = Pos::of(cell_at(3, 3));
        let far = cell_at(3, 6);
        assert!(!run.game.is_lit(far));
        let mut inputs = NONE;
        inputs[0] = Input::default().with(Button::A);
        run.step(inputs);
        assert!(run.game.is_lit(far));
        assert!(run.game.walkers[0].flare_used);
        assert!(run.game.walkers[0].flared_at.is_some());
        // A second flare does nothing.
        run.game.walkers[0].pos = Pos::of(cell_at(10, 3));
        run.step(NONE);
        run.step(inputs);
        assert!(!run.game.is_lit(cell_at(10, 6)));
    }

    #[test]
    fn finishing_order_scores_and_the_last_runner_isnt_waited_for() {
        let mut run = racing(4, Format::FreeForAll);
        go_home(&mut run, 2);
        go_home(&mut run, 0);
        assert!(matches!(run.game.phase, Phase::Racing { .. }));
        go_home(&mut run, 3);
        // Three in: the maze is over.
        assert!(matches!(run.game.phase, Phase::Result { .. }));
        let gained: Vec<u32> = run.game.walkers.iter().map(|w| w.gained).collect();
        assert_eq!(gained, vec![2, 0, 3, 1]);
        assert_eq!(run.game.walkers[2].points, 3);
    }

    #[test]
    fn at_time_up_the_rest_rank_by_how_far_they_are() {
        let mut run = racing(3, Format::FreeForAll);
        // P2 walks a little of the way; nobody else moves.
        go_home_for(&mut run, 1, 60);
        run_until(&mut run, |g| matches!(g.phase, Phase::Result { .. }));
        let gained: Vec<u32> = run.game.walkers.iter().map(|w| w.gained).collect();
        // P1 and P3 tie for second, and share it.
        assert_eq!(gained[..3], [1, 2, 1]);
    }

    #[test]
    fn three_mazes_then_most_points_wins() {
        let mut run = racing(2, Format::FreeForAll);
        for maze in 0..MAZES {
            run_until(&mut run, |g| matches!(g.phase, Phase::Racing { .. }));
            assert_eq!(run.game.round, maze);
            go_home(&mut run, if maze == 1 { 0 } else { 1 });
            run_until(&mut run, |g| !matches!(g.phase, Phase::Racing { .. }));
        }
        run_until(&mut run, |g| g.phase == Phase::Finished);
        assert_eq!(run.game.walkers[1].points, 2);
        assert_eq!(
            run.outcome().unwrap().winners().collect::<Vec<_>>(),
            vec![Seat(1)]
        );
    }

    #[test]
    fn a_new_maze_starts_everyone_in_their_corner_in_the_dark() {
        let mut run = racing(2, Format::FreeForAll);
        let first = run.game.maze;
        go_home(&mut run, 0);
        run_until(&mut run, |g| matches!(g.phase, Phase::Ready { .. }));
        let game = &run.game;
        assert_ne!(game.maze, first);
        assert!(!game.walkers[0].finished());
        assert_eq!(game.walkers[0].pos, Pos::of(game.walkers[0].start));
        assert!(!game.is_lit(cell_at(3, 3)));
    }

    #[test]
    fn team_points_add_up() {
        let format = Format::TwoVsTwo {
            team_of: [0, 1, 1, 0],
        };
        let mut run = racing(4, format);
        for _ in 0..MAZES {
            run_until(&mut run, |g| matches!(g.phase, Phase::Racing { .. }));
            go_home(&mut run, 1);
            go_home(&mut run, 0);
            go_home(&mut run, 3);
            run_until(&mut run, |g| !matches!(g.phase, Phase::Racing { .. }));
        }
        run_until(&mut run, |g| g.phase == Phase::Finished);
        // Each maze: P2 3, P1 2, P4 1, P3 0: team 0 (P1, P4) 9, team 1 (P2, P3) 9.
        assert_eq!(run.outcome().unwrap().placement, [Some(1); 4]);
    }

    #[test]
    fn seats_outside_a_duel_sit_out() {
        let run = new_run(
            4,
            Format::Duel {
                a: Seat(1),
                b: Seat(3),
            },
        );
        let playing: Vec<Seat> = run.game.playing().collect();
        assert_eq!(playing, vec![Seat(1), Seat(3)]);
        let starts: Vec<usize> = playing.iter().map(|s| run.game.walker(*s).start).collect();
        assert_eq!(starts, vec![corner(0), corner(2)]);
    }

    fn ghost_run() -> Run<HedgeMaze> {
        racing(4, Format::OneVsThree { solo: Seat(1) })
    }

    #[test]
    fn the_ghost_starts_on_the_beacon_and_can_never_go_back() {
        let mut run = ghost_run();
        let ghost = run.game.walkers[1];
        assert!(ghost.ghost);
        assert_eq!(ghost.pos, Pos::of(middle()));
        let out = Dir::ALL
            .into_iter()
            .find(|d| run.game.maze.is_open(middle(), *d))
            .unwrap();
        // Most of the way out, then back: it stops short of the beacon.
        walk(&mut run, 1, out, (UNIT / GHOST_SPEED) as u32);
        let out_there = run.game.walkers[1].pos;
        assert_ne!(out_there, Pos::of(middle()));
        walk(&mut run, 1, out.opposite(), 30);
        assert_eq!(run.game.walkers[1].pos, out_there);
        // And from the next cell's centre neither.
        run.game.walkers[1].pos = Pos::of(neighbour(middle(), out).unwrap());
        walk(&mut run, 1, out.opposite(), 30);
        assert_eq!(
            run.game.walkers[1].pos,
            Pos::of(neighbour(middle(), out).unwrap())
        );
    }

    #[test]
    fn the_ghost_catches_nobody_from_the_beacon() {
        let mut run = ghost_run();
        go_home(&mut run, 0);
        assert!(run.game.walkers[0].finished());
        assert_eq!(run.game.walkers[1].catches, 0);
    }

    #[test]
    fn the_ghost_sends_a_runner_back_to_their_corner() {
        let mut run = ghost_run();
        // Put the ghost right on P1, out of their corner.
        let p1 = run.game.walkers[0];
        let next = Dir::ALL
            .into_iter()
            .find(|d| run.game.maze.is_open(p1.start, *d))
            .unwrap();
        run.game.walkers[0].pos = Pos::of(neighbour(p1.start, next).unwrap());
        run.game.walkers[1].pos = Pos::of(neighbour(p1.start, next).unwrap());
        run.step(NONE);
        let caught = run.game.walkers[0];
        assert_eq!(caught.pos, Pos::of(caught.start));
        assert!(caught.caught_at.is_some());
        assert_eq!(run.game.walkers[1].catches, 1);
        // Frozen for a moment, and safe from another catch.
        walk(&mut run, 0, next, 10);
        assert_eq!(run.game.walkers[0].pos, Pos::of(caught.start));
        run.game.walkers[1].pos = Pos::of(caught.start);
        run.step(NONE);
        assert_eq!(run.game.walkers[1].catches, 1);
    }

    #[test]
    fn the_three_take_a_maze_when_two_get_home() {
        let mut run = ghost_run();
        go_home(&mut run, 0);
        assert!(matches!(run.game.phase, Phase::Racing { .. }));
        go_home(&mut run, 2);
        assert!(matches!(run.game.phase, Phase::Result { .. }));
        assert_eq!(run.game.round_winner, Some(Side::Trio));
        assert_eq!(run.game.trio_rounds, 1);
    }

    #[test]
    fn the_ghost_takes_a_maze_at_time_up_and_the_match_at_two() {
        let mut run = ghost_run();
        go_home(&mut run, 0);
        run_until(&mut run, |g| matches!(g.phase, Phase::Result { .. }));
        assert_eq!(run.game.round_winner, Some(Side::Ghost));
        run_until(&mut run, |g| {
            g.round == 1 && matches!(g.phase, Phase::Result { .. })
        });
        assert_eq!(run.game.ghost_rounds, 2);
        run_until(&mut run, |g| g.phase == Phase::Finished);
        assert_eq!(run.game.round, 1, "no third maze");
        assert_eq!(
            run.outcome().unwrap().winners().collect::<Vec<_>>(),
            vec![Seat(1)]
        );
    }

    #[test]
    fn harder_bots_get_home_sooner() {
        let time = |difficulty| {
            let mut total = 0;
            for seed in 0..30 {
                let mut run: Run<HedgeMaze> = Run::new(
                    seed,
                    PlayerSetup::first(1),
                    Format::FreeForAll,
                    RunConfig::default(),
                );
                run_until(&mut run, |g| matches!(g.phase, Phase::Racing { .. }));
                while run.game.walkers[0].finished_at.is_none() {
                    let inputs = Seat::ALL.map(|s| HedgeMaze::bot_input(&run.game, s, difficulty));
                    run.step(inputs);
                }
                total += run.game.walkers[0].finished_at.unwrap();
            }
            total
        };
        let (easy, normal, hard) = (
            time(Difficulty::Easy),
            time(Difficulty::Normal),
            time(Difficulty::Hard),
        );
        assert!(hard < normal && normal < easy, "{hard} {normal} {easy}");
    }

    #[test]
    fn bots_finish_every_format() {
        use sim_core::registry::play_with_bots;
        let entry = sim_core::MinigameEntry::of::<HedgeMaze>();
        for format in [
            Format::FreeForAll,
            Format::OneVsThree { solo: Seat(1) },
            Format::TwoVsTwo {
                team_of: [0, 1, 1, 0],
            },
            Format::Duel {
                a: Seat(0),
                b: Seat(2),
            },
        ] {
            for difficulty in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard] {
                let outcome = play_with_bots(
                    &entry,
                    3,
                    PlayerSetup::first(4),
                    format,
                    [difficulty; MAX_PLAYERS],
                );
                assert!(outcome.is_some(), "{format:?} {difficulty:?}");
            }
        }
    }

    #[test]
    fn passes_all_automated_quality_bars() {
        sim_check::check_all::<HedgeMaze>(8).unwrap();
    }
}
