//! The puzzles, made from the game's RNG. Each is a [`Board`]: what to show, four answers dealt
//! to the stick's four directions, and which one is right. Everything a puzzle shows is numbers
//! and shapes, so only the prompts need translating.

use sim_core::{Input, Rng};

/// A stick past this (of 127) along one axis, and further along it than the other, points at
/// that answer.
pub const AIM: i16 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dir {
    Up,
    Right,
    Down,
    Left,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::Up, Dir::Right, Dir::Down, Dir::Left];

    pub fn index(self) -> usize {
        self as usize
    }

    /// The input a player gives to point this way.
    pub fn stick(self) -> Input {
        let (x, y) = match self {
            Dir::Up => (0, 127),
            Dir::Right => (127, 0),
            Dir::Down => (0, -127),
            Dir::Left => (-127, 0),
        };
        Input {
            buttons: 0,
            stick_x: x,
            stick_y: y,
        }
    }

    /// Where a stick points, if it's pushed far enough.
    pub fn of_stick(input: &Input) -> Option<Dir> {
        let (x, y) = (input.stick_x as i16, input.stick_y as i16);
        if x.abs() >= AIM && x.abs() > y.abs() {
            Some(if x > 0 { Dir::Right } else { Dir::Left })
        } else if y.abs() >= AIM && y.abs() > x.abs() {
            Some(if y > 0 { Dir::Up } else { Dir::Down })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// How many lanterns of one colour?
    Count,
    /// A sum with up to three numbers, × before + and −.
    Sum,
    /// What comes next in a sequence of four numbers?
    Sequence,
    /// Three turned copies of a shape and one that differs.
    OddOneOut,
    /// Which rope of a rope ladder leads to the lantern?
    Ladder,
    /// Which token is heaviest, from three balance scales?
    Scales,
}

impl Kind {
    pub const ALL: [Kind; 6] = [
        Kind::Count,
        Kind::Sum,
        Kind::Sequence,
        Kind::OddOneOut,
        Kind::Ladder,
        Kind::Scales,
    ];
}

/// Puzzles come in this many levels of difficulty, from 0.
pub const LEVELS: u8 = 5;

/// The four things the ladder and the scales are about, told apart by shape (and, in the view,
/// colour).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Token {
    Circle,
    Square,
    Triangle,
    Diamond,
}

impl Token {
    pub const ALL: [Token; 4] = [
        Token::Circle,
        Token::Square,
        Token::Triangle,
        Token::Diamond,
    ];

    pub fn index(self) -> usize {
        self as usize
    }
}

/// A regular polygon with dots on some of its corners (bit `i` of `dots` for corner `i`, the
/// corners counted anticlockwise), turned `turn` twelfths of a full turn anticlockwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Glyph {
    pub sides: u8,
    pub dots: u8,
    pub turn: u8,
}

/// Turns in a full turn, for [`Glyph::turn`].
pub const TURNS: u8 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Answer {
    Number(i32),
    Token(Token),
    Glyph(Glyph),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Op {
    Add,
    Sub,
    Mul,
}

/// The counting puzzle's lanterns sit on a grid this big, one at most to a cell.
pub const COUNT_COLUMNS: usize = 9;
pub const COUNT_ROWS: usize = 5;
pub const COUNT_CELLS: usize = COUNT_COLUMNS * COUNT_ROWS;
/// Lantern colours, numbered from 1.
pub const COLOURS: u8 = 3;
pub const MAX_RUNGS: usize = 11;
pub const SCALES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Puzzle {
    /// Lanterns on a grid, row by row from the top left: 0 for none, else its colour
    /// (1..=[`COLOURS`]). How many are `colour`?
    Count { grid: [u8; COUNT_CELLS], colour: u8 },
    /// `terms[0] ops[0] terms[1]`, then `ops[1] terms[2]` when `len` is 3.
    Sum {
        terms: [i32; 3],
        ops: [Op; 2],
        len: u8,
    },
    /// What comes after these four?
    Sequence { terms: [i32; 4] },
    /// The answers are the shapes: three are turned copies of one shape, and one isn't.
    OddOneOut,
    /// Four ropes hang side by side, `tops[i]` on top of rope `i` (from the left). Rung `k` (from
    /// the top) joins rope `rungs[k]` and the one to its right, and following a rope down you
    /// cross every rung you meet. Which top leads to the lantern under rope `prize`?
    Ladder {
        tops: [Token; 4],
        rungs: [u8; MAX_RUNGS],
        len: u8,
        prize: u8,
    },
    /// Each scale holds two tokens, left and right, and tips to the heavier by `weights` (by
    /// [`Token::index`]). Which token is heaviest?
    Scales {
        scales: [[Token; 2]; SCALES],
        weights: [u8; 4],
    },
}

/// One puzzle in play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Board {
    pub kind: Kind,
    pub level: u8,
    pub puzzle: Puzzle,
    /// By [`Dir::index`].
    pub answers: [Answer; 4],
    pub right: Dir,
    /// Answers someone locked in wrongly, there for everyone to see.
    pub wrong: [bool; 4],
}

impl Board {
    pub fn new(rng: &mut Rng, kind: Kind, level: u8) -> Board {
        let level = level.min(LEVELS - 1);
        let (puzzle, answers, right) = match kind {
            Kind::Count => count(rng, level),
            Kind::Sum => sum(rng, level),
            Kind::Sequence => sequence(rng, level),
            Kind::OddOneOut => odd_one_out(rng, level),
            Kind::Ladder => ladder(rng, level),
            Kind::Scales => scales(rng, level),
        };
        Board {
            kind,
            level,
            puzzle,
            answers,
            right,
            wrong: [false; 4],
        }
    }

    /// About how many ticks a Normal bot needs to work this puzzle out.
    pub fn effort(&self) -> u32 {
        let level = self.level as u32;
        match self.puzzle {
            Puzzle::Count { grid, .. } => 60 + 10 * grid.iter().filter(|c| **c != 0).count() as u32,
            Puzzle::Sum { .. } => 100 + 32 * level,
            Puzzle::Sequence { .. } => 110 + 30 * level,
            Puzzle::OddOneOut => 90 + 35 * level,
            Puzzle::Ladder { len, .. } => 80 + 14 * len as u32,
            Puzzle::Scales { .. } => 100 + 25 * level,
        }
    }
}

/// Deals four answers to the four directions; the first is the right one.
fn deal(rng: &mut Rng, items: [Answer; 4]) -> ([Answer; 4], Dir) {
    let mut order = [0, 1, 2, 3];
    shuffle(rng, &mut order);
    let answers = order.map(|i| items[i]);
    let right = Dir::ALL[order.iter().position(|i| *i == 0).unwrap_or(0)];
    (answers, right)
}

/// Fisher-Yates, from the game's own RNG.
pub fn shuffle<T>(rng: &mut Rng, items: &mut [T]) {
    for i in (1..items.len()).rev() {
        let j = rng.range_u32(0, i as u32) as usize;
        items.swap(i, j);
    }
}

fn between(rng: &mut Rng, lo: i32, hi: i32) -> i32 {
    rng.range_u32(lo as u32, hi as u32) as i32
}

/// The truth and three other numbers: the likely slips in `traps` first (those that differ and
/// aren't negative), then others within `spread` of the truth.
fn numbers(rng: &mut Rng, truth: i32, spread: i32, traps: &[i32]) -> ([Answer; 4], Dir) {
    let mut values = vec![truth];
    for trap in traps {
        if values.len() < 4 && *trap >= 0 && !values.contains(trap) {
            values.push(*trap);
        }
    }
    while values.len() < 4 {
        let value = truth + between(rng, 0, 2 * spread) - spread;
        if value >= 0 && !values.contains(&value) {
            values.push(value);
        }
    }
    deal(rng, std::array::from_fn(|i| Answer::Number(values[i])))
}

fn count(rng: &mut Rng, level: u8) -> (Puzzle, [Answer; 4], Dir) {
    let total = 8 + 3 * level as usize;
    let colour = between(rng, 1, COLOURS as i32) as u8;
    let truth = between(rng, total as i32 / 4 + 1, total as i32 / 2 + 1);
    let mut cells: Vec<usize> = (0..COUNT_CELLS).collect();
    shuffle(rng, &mut cells);
    let mut grid = [0; COUNT_CELLS];
    for (k, cell) in cells.into_iter().take(total).enumerate() {
        grid[cell] = if (k as i32) < truth {
            colour
        } else {
            // One of the other two colours.
            (colour + between(rng, 1, COLOURS as i32 - 1) as u8 - 1) % COLOURS + 1
        };
    }
    let (answers, right) = numbers(rng, truth, 2, &[]);
    (Puzzle::Count { grid, colour }, answers, right)
}

/// A sum's value, × before + and −.
pub fn evaluate(terms: &[i32], ops: &[Op]) -> i32 {
    // Fold the products first, then add and subtract left to right.
    let mut parts = vec![(Op::Add, terms[0])];
    for (op, term) in ops.iter().zip(&terms[1..]) {
        match op {
            Op::Mul => parts.last_mut().expect("starts with one").1 *= term,
            _ => parts.push((*op, *term)),
        }
    }
    parts.iter().fold(0, |total, (op, value)| match op {
        Op::Sub => total - value,
        _ => total + value,
    })
}

/// The slip of working a sum left to right, ignoring × first.
fn left_to_right(terms: &[i32], ops: &[Op]) -> i32 {
    ops.iter()
        .zip(&terms[1..])
        .fold(terms[0], |total, (op, term)| match op {
            Op::Add => total + term,
            Op::Sub => total - term,
            Op::Mul => total * term,
        })
}

fn sum(rng: &mut Rng, level: u8) -> (Puzzle, [Answer; 4], Dir) {
    let (terms, ops, len) = match level {
        0 => ([between(rng, 2, 9), between(rng, 2, 9), 0], [Op::Add; 2], 2),
        1 => {
            let a = between(rng, 6, 15);
            let b = between(rng, 2, 9);
            let c = between(rng, 2, a.min(9));
            ([a, b, c], [Op::Add, Op::Sub], 3)
        }
        2 => (
            [between(rng, 2, 9), between(rng, 2, 5), between(rng, 1, 9)],
            [Op::Mul, Op::Add],
            3,
        ),
        3 => (
            [between(rng, 2, 9), between(rng, 2, 9), between(rng, 2, 5)],
            [Op::Add, Op::Mul],
            3,
        ),
        _ => {
            let a = between(rng, 4, 9);
            let b = between(rng, 4, 12);
            ([a, b, between(rng, 2, 12)], [Op::Mul, Op::Sub], 3)
        }
    };
    let n = len as usize;
    let truth = evaluate(&terms[..n], &ops[..n - 1]);
    let slip = left_to_right(&terms[..n], &ops[..n - 1]);
    let (answers, right) = numbers(rng, truth, 3 + level as i32, &[slip]);
    (Puzzle::Sum { terms, ops, len }, answers, right)
}

fn sequence(rng: &mut Rng, level: u8) -> (Puzzle, [Answer; 4], Dir) {
    let either = rng.chance(50);
    let terms: [i32; 5] = match level {
        0 => {
            let (start, step) = (between(rng, 1, 20), between(rng, 2, 9));
            std::array::from_fn(|i| start + step * i as i32)
        }
        1 if either => {
            let start = between(rng, 1, 6);
            std::array::from_fn(|i| start << i)
        }
        1 => {
            let (start, step) = (between(rng, 45, 70), between(rng, 3, 9));
            std::array::from_fn(|i| start - step * i as i32)
        }
        2 if either => {
            let start = between(rng, 1, 4);
            std::array::from_fn(|i| start * 3i32.pow(i as u32))
        }
        2 => {
            // Two steps taken in turn.
            let (start, a) = (between(rng, 1, 15), between(rng, 1, 9));
            let b = (a + between(rng, 1, 8) - 1) % 9 + 1;
            let mut terms = [start; 5];
            for i in 1..5 {
                terms[i] = terms[i - 1] + if i % 2 == 1 { a } else { b };
            }
            terms
        }
        3 if either => {
            let n = between(rng, 1, 6);
            std::array::from_fn(|i| (n + i as i32).pow(2))
        }
        3 => {
            // Steps that grow by one each time.
            let (start, step) = (between(rng, 1, 15), between(rng, 1, 6));
            let mut terms = [start; 5];
            for i in 1..5 {
                terms[i] = terms[i - 1] + step + i as i32 - 1;
            }
            terms
        }
        _ if either => {
            // Each the sum of the two before.
            let a = between(rng, 1, 5);
            let mut terms = [a, between(rng, a, 8), 0, 0, 0];
            for i in 2..5 {
                terms[i] = terms[i - 1] + terms[i - 2];
            }
            terms
        }
        _ => {
            // Double and add one.
            let mut terms = [between(rng, 1, 5); 5];
            for i in 1..5 {
                terms[i] = terms[i - 1] * 2 + 1;
            }
            terms
        }
    };
    // The slip of repeating the last step.
    let slip = 2 * terms[3] - terms[2];
    let spread = 2 + (terms[4] - terms[3]).abs().min(12) / 3;
    let (answers, right) = numbers(rng, terms[4], spread, &[slip]);
    let shown = [terms[0], terms[1], terms[2], terms[3]];
    (Puzzle::Sequence { terms: shown }, answers, right)
}

/// A glyph's dots turned `by` corners anticlockwise.
fn rotated(sides: u8, dots: u8, by: u8) -> u8 {
    let full = (1u16 << sides) - 1;
    let dots = dots as u16;
    (((dots << by) | (dots >> (sides - by))) & full) as u8
}

/// A glyph's dots seen in a mirror (corner `i` goes to corner `-i`).
pub fn mirrored(sides: u8, dots: u8) -> u8 {
    (0..sides)
        .filter(|i| dots & (1 << i) != 0)
        .fold(0, |m, i| m | 1 << ((sides - i) % sides))
}

/// Whether two dot patterns on `sides` corners are the same shape turned (or, with `mirror`,
/// also flipped over).
pub fn same_shape(sides: u8, a: u8, b: u8, mirror: bool) -> bool {
    (0..sides).any(|by| {
        let turned = rotated(sides, a, by);
        turned == b || (mirror && mirrored(sides, turned) == b)
    })
}

fn random_dots(rng: &mut Rng, sides: u8, dots: u8) -> u8 {
    let mut corners: Vec<u8> = (0..sides).collect();
    shuffle(rng, &mut corners);
    corners[..dots as usize]
        .iter()
        .fold(0, |mask, corner| mask | 1 << corner)
}

fn odd_one_out(rng: &mut Rng, level: u8) -> (Puzzle, [Answer; 4], Dir) {
    // (sides, dots) of the three copies and of the odd one.
    let (base, odd) = match level {
        // A corner more.
        0 | 1 => {
            let sides = between(rng, 3, 5) as u8;
            let dots = if level == 0 {
                0
            } else {
                random_dots(rng, sides, 1)
            };
            ((sides, dots), (sides + 1, dots))
        }
        // A dot elsewhere.
        2 | 3 => {
            let sides = between(rng, 4 + level as i32 - 2, 6) as u8;
            let count = if sides == 6 {
                between(rng, 2, 3) as u8
            } else {
                2
            };
            let dots = random_dots(rng, sides, count);
            let other = loop {
                let other = random_dots(rng, sides, count);
                if !same_shape(sides, dots, other, true) {
                    break other;
                }
            };
            ((sides, dots), (sides, other))
        }
        // Its mirror image: dots on a hexagon with gaps of 1, 2 and 3 corners have no mirror
        // symmetry.
        _ => {
            let dots = rotated(6, 0b001011, between(rng, 0, 5) as u8);
            ((6, dots), (6, mirrored(6, dots)))
        }
    };
    // Everyone turned differently, so the odd one can't be spotted by lining them up.
    let mut turns: Vec<u8> = (0..TURNS).collect();
    shuffle(rng, &mut turns);
    let glyph = |(sides, dots): (u8, u8), turn: u8| Answer::Glyph(Glyph { sides, dots, turn });
    let items = [
        glyph(odd, turns[0]),
        glyph(base, turns[1]),
        glyph(base, turns[2]),
        glyph(base, turns[3]),
    ];
    let (answers, right) = deal(rng, items);
    (Puzzle::OddOneOut, answers, right)
}

/// Where following rope `from` down a ladder ends up.
pub fn follow(rungs: &[u8], from: u8) -> u8 {
    rungs.iter().fold(from, |rope, rung| {
        if *rung == rope {
            rope + 1
        } else if rung + 1 == rope {
            rope - 1
        } else {
            rope
        }
    })
}

fn ladder(rng: &mut Rng, level: u8) -> (Puzzle, [Answer; 4], Dir) {
    let len = 3 + 2 * level as usize;
    let mut rungs = [0; MAX_RUNGS];
    for k in 0..len {
        // Never straight under the last rung, which would undo it.
        rungs[k] = loop {
            let rung = between(rng, 0, 2) as u8;
            if k == 0 || rung != rungs[k - 1] {
                break rung;
            }
        };
    }
    let mut tops = Token::ALL;
    shuffle(rng, &mut tops);
    let prize = between(rng, 0, 3) as u8;
    let from = (0..4)
        .find(|rope| follow(&rungs[..len], *rope) == prize)
        .expect("a ladder pairs tops and bottoms one to one");
    let others: Vec<Token> = tops
        .into_iter()
        .filter(|t| *t != tops[from as usize])
        .collect();
    let items = [tops[from as usize], others[0], others[1], others[2]].map(Answer::Token);
    let (answers, right) = deal(rng, items);
    let puzzle = Puzzle::Ladder {
        tops,
        rungs,
        len: len as u8,
        prize,
    };
    (puzzle, answers, right)
}

fn scales(rng: &mut Rng, level: u8) -> (Puzzle, [Answer; 4], Dir) {
    // Heaviest first.
    let mut by_weight = Token::ALL;
    shuffle(rng, &mut by_weight);
    let mut weights = [0; 4];
    for (rank, token) in by_weight.iter().enumerate() {
        weights[token.index()] = 4 - rank as u8;
    }
    // Every token but the heaviest is shown losing to a heavier one: at the easy levels always
    // the heaviest, then any heavier one, and at the top level the next heavier, so it takes a
    // chain of reasoning.
    let mut scales = [[Token::Circle; 2]; SCALES];
    for rank in 1..4 {
        let heavier = match level {
            0 | 1 => 0,
            2 | 3 => between(rng, 0, rank as i32 - 1) as usize,
            _ => rank - 1,
        };
        let mut pair = [by_weight[heavier], by_weight[rank]];
        if rng.chance(50) {
            pair.swap(0, 1);
        }
        scales[rank - 1] = pair;
    }
    shuffle(rng, &mut scales);
    let (answers, right) = deal(rng, by_weight.map(Answer::Token));
    (Puzzle::Scales { scales, weights }, answers, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boards(kind: Kind) -> impl Iterator<Item = Board> {
        (0..LEVELS).flat_map(move |level| {
            (0..300).map(move |seed| Board::new(&mut Rng::new(seed), kind, level))
        })
    }

    /// Every puzzle has four different answers.
    #[test]
    fn answers_differ() {
        for kind in Kind::ALL {
            for board in boards(kind) {
                for i in 0..4 {
                    for j in 0..i {
                        assert_ne!(board.answers[i], board.answers[j], "{board:?}");
                    }
                }
                if let Answer::Number(n) = board.answers[0] {
                    assert!(
                        board
                            .answers
                            .iter()
                            .all(|a| matches!(a, Answer::Number(n) if *n >= 0)),
                        "{n} {board:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn counts_are_right() {
        for board in boards(Kind::Count) {
            let Puzzle::Count { grid, colour } = board.puzzle else {
                unreachable!()
            };
            let truth = grid.iter().filter(|c| **c == colour).count() as i32;
            assert_eq!(board.answers[board.right.index()], Answer::Number(truth));
            assert!(grid.iter().all(|c| *c <= COLOURS));
        }
    }

    #[test]
    fn sums_are_right() {
        assert_eq!(evaluate(&[7, 8, 2], &[Op::Add, Op::Mul]), 23);
        assert_eq!(evaluate(&[7, 8, 2], &[Op::Mul, Op::Sub]), 54);
        assert_eq!(left_to_right(&[7, 8, 2], &[Op::Add, Op::Mul]), 30);
        for board in boards(Kind::Sum) {
            let Puzzle::Sum { terms, ops, len } = board.puzzle else {
                unreachable!()
            };
            let n = len as usize;
            let truth = evaluate(&terms[..n], &ops[..n - 1]);
            assert!(truth >= 0, "{board:?}");
            assert_eq!(board.answers[board.right.index()], Answer::Number(truth));
        }
    }

    /// The right answer to a sequence is one of the rules the generator uses, carried on.
    #[test]
    fn sequences_carry_on() {
        for board in boards(Kind::Sequence) {
            let Puzzle::Sequence { terms } = board.puzzle else {
                unreachable!()
            };
            let Answer::Number(next) = board.answers[board.right.index()] else {
                unreachable!()
            };
            let [a, b, c, d] = terms;
            let fits = [
                d - c == c - b && c - b == b - a && next - d == d - c,
                b == 2 * a && next == 2 * d,
                b == 3 * a && next == 3 * d,
                d - c == b - a && next - d == c - b,
                (d - c) - (c - b) == (c - b) - (b - a) && next - d - (d - c) == (d - c) - (c - b),
                c == a + b && d == b + c && next == c + d,
                b == 2 * a + 1 && next == 2 * d + 1,
            ];
            assert!(fits.iter().any(|f| *f), "{terms:?} -> {next}");
        }
    }

    #[test]
    fn exactly_one_shape_is_odd() {
        for board in boards(Kind::OddOneOut) {
            let glyphs = board.answers.map(|a| match a {
                Answer::Glyph(g) => g,
                _ => unreachable!(),
            });
            let alike = |a: Glyph, b: Glyph| {
                a.sides == b.sides && same_shape(a.sides, a.dots, b.dots, false)
            };
            let odd = glyphs[board.right.index()];
            let copies: Vec<Glyph> = Dir::ALL
                .iter()
                .filter(|d| **d != board.right)
                .map(|d| glyphs[d.index()])
                .collect();
            assert!(copies.iter().all(|g| alike(*g, copies[0])), "{board:?}");
            assert!(!alike(odd, copies[0]), "{board:?}");
            let turns: Vec<u8> = glyphs.iter().map(|g| g.turn).collect();
            assert!(turns.iter().all(|t| *t < TURNS));
        }
        // The top level's odd one is only a mirror image.
        for seed in 0..50 {
            let board = Board::new(&mut Rng::new(seed), Kind::OddOneOut, LEVELS - 1);
            let Answer::Glyph(odd) = board.answers[board.right.index()] else {
                unreachable!()
            };
            let Answer::Glyph(copy) = board.answers[(board.right.index() + 1) % 4] else {
                unreachable!()
            };
            assert!(same_shape(6, odd.dots, copy.dots, true));
        }
    }

    #[test]
    fn the_right_rope_leads_to_the_lantern() {
        for board in boards(Kind::Ladder) {
            let Puzzle::Ladder {
                tops,
                rungs,
                len,
                prize,
            } = board.puzzle
            else {
                unreachable!()
            };
            let rungs = &rungs[..len as usize];
            assert!(rungs.windows(2).all(|w| w[0] != w[1] && w[0] < 3));
            let Answer::Token(token) = board.answers[board.right.index()] else {
                unreachable!()
            };
            let rope = tops.iter().position(|t| *t == token).unwrap() as u8;
            assert_eq!(follow(rungs, rope), prize);
        }
    }

    /// The scales alone pick out the heaviest: it never loses, and every other token does.
    #[test]
    fn the_scales_give_the_heaviest_away() {
        for board in boards(Kind::Scales) {
            let Puzzle::Scales { scales, weights } = board.puzzle else {
                unreachable!()
            };
            let Answer::Token(heaviest) = board.answers[board.right.index()] else {
                unreachable!()
            };
            assert_eq!(weights[heaviest.index()], 4);
            for token in Token::ALL {
                let loses = scales.iter().any(|pair| {
                    pair.contains(&token)
                        && pair
                            .iter()
                            .any(|other| weights[other.index()] > weights[token.index()])
                });
                assert_eq!(loses, token != heaviest, "{board:?}");
            }
        }
    }

    #[test]
    fn harder_levels_take_longer() {
        for kind in Kind::ALL {
            let effort = |level| {
                (0..100)
                    .map(|seed| Board::new(&mut Rng::new(seed), kind, level).effort())
                    .sum::<u32>()
            };
            assert!(effort(LEVELS - 1) > effort(0), "{kind:?}");
        }
    }

    #[test]
    fn mirrors_and_turns() {
        assert_eq!(rotated(6, 0b000011, 1), 0b000110);
        assert_eq!(rotated(6, 0b100001, 1), 0b000011);
        assert_eq!(mirrored(6, 0b000011), 0b100001);
        assert!(same_shape(6, 0b001011, rotated(6, 0b001011, 4), false));
        assert!(!same_shape(6, 0b001011, mirrored(6, 0b001011), false));
        assert!(same_shape(5, 0b00011, mirrored(5, 0b00011), false));
    }
}
