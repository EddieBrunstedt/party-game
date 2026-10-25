//! The course: a canyon, then a cave whose walls close in, and the obstacles along it, all made
//! from the seed. Distances along the course are `z` (forward); across it, a [`Vec2`] with `x` to
//! the right and `y` up, centred on the middle of the canyon or cave.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use sim_core::Rng;
use sim_core::physics::Vec2;

/// The open canyon's half width and half height (it has an invisible lid).
pub const CANYON_HALF: Vec2 = Vec2::new(8.0, 4.5);
/// The cave's mouth is here: the canyon narrows to it over [`MOUTH_LENGTH`] before.
pub const CAVE_Z: f32 = 450.0;
pub const MOUTH_LENGTH: f32 = 40.0;
pub const MOUTH_HALF: Vec2 = Vec2::new(5.5, 3.5);
/// Inside, the walls close in steadily over this distance, to [`NARROWEST_HALF`].
pub const SHRINK_LENGTH: f32 = 700.0;
pub const NARROWEST_HALF: Vec2 = Vec2::new(2.3, 1.6);
/// Obstacles are made this far; nobody gets near the end before time runs out.
pub const COURSE_LENGTH: f32 = 3200.0;
pub const FIRST_OBSTACLE: f32 = 45.0;
/// Gaps between obstacles: in the canyon, at the cave's mouth and where it's narrowest.
pub const CANYON_SPACING: f32 = 40.0;
pub const MOUTH_SPACING: f32 = 34.0;
pub const NARROWEST_SPACING: f32 = 25.0;
/// Every obstacle leaves a way through at least this wide.
const PASSAGE: f32 = 1.5;

/// The canyon's or cave's half extents at `z`.
pub fn half_extents(z: f32) -> Vec2 {
    if z < CAVE_Z - MOUTH_LENGTH {
        CANYON_HALF
    } else if z < CAVE_Z {
        CANYON_HALF.lerp(MOUTH_HALF, (z - (CAVE_Z - MOUTH_LENGTH)) / MOUTH_LENGTH)
    } else {
        MOUTH_HALF.lerp(NARROWEST_HALF, ((z - CAVE_Z) / SHRINK_LENGTH).min(1.0))
    }
}

/// How far through the cave's narrowing `z` is: 0 outside or at its mouth, 1 at its narrowest.
pub fn narrowing(z: f32) -> f32 {
    ((z - CAVE_Z) / SHRINK_LENGTH).clamp(0.0, 1.0)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    /// A boulder or a hanging lantern: a disc across the course.
    Rock { center: Vec2, radius: f32 },
    /// A column from floor to roof.
    Pillar { x: f32, half_width: f32 },
    /// A beam from wall to wall.
    Beam { y: f32, half_height: f32 },
    /// A wall across the whole course with one round hole in it.
    Gate { center: Vec2, radius: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Obstacle {
    /// Where it starts along the course, and how deep it is.
    pub z: f32,
    pub depth: f32,
    pub shape: Shape,
}

impl Obstacle {
    /// Whether it spans anything from `z - reach` to `z + reach` along the course.
    pub fn spans(&self, z: f32, reach: f32) -> bool {
        z + reach >= self.z && z - reach <= self.z + self.depth
    }

    pub fn middle(&self) -> f32 {
        self.z + self.depth * 0.5
    }

    /// How far `p` is from its surface across the course: negative inside it.
    pub fn clearance(&self, p: Vec2) -> f32 {
        match self.shape {
            Shape::Rock { center, radius } => p.distance(center) - radius,
            Shape::Pillar { x, half_width } => (p.x - x).abs() - half_width,
            Shape::Beam { y, half_height } => (p.y - y).abs() - half_height,
            Shape::Gate { center, radius } => radius - p.distance(center),
        }
    }

    /// The way out of it from `p`: the direction its clearance grows fastest.
    pub fn away(&self, p: Vec2) -> Vec2 {
        let dir = match self.shape {
            Shape::Rock { center, .. } => p - center,
            Shape::Pillar { x, .. } => Vec2::new(p.x - x, 0.0),
            Shape::Beam { y, .. } => Vec2::new(0.0, p.y - y),
            Shape::Gate { center, .. } => center - p,
        }
        .normalize_or_zero();
        if dir == Vec2::ZERO {
            Vec2::new(1.0, 0.0)
        } else {
            dir
        }
    }
}

/// Every obstacle along the course, in order of `z`. It's made from the seed (which the state
/// hashes), so it hashes only its length, and clones share it.
#[derive(Debug, Clone)]
pub struct Course {
    pub obstacles: Arc<[Obstacle]>,
}

impl Hash for Course {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.obstacles.len().hash(state);
    }
}

impl Course {
    pub fn new(seed: u64) -> Self {
        Self {
            obstacles: generate(seed).into(),
        }
    }

    /// The obstacles that start between `from` and `to`, in order.
    pub fn between(&self, from: f32, to: f32) -> &[Obstacle] {
        // The deepest obstacle is a couple of units deep, so look back that far.
        let start = self.obstacles.partition_point(|o| o.z + o.depth < from);
        let end = self.obstacles.partition_point(|o| o.z <= to);
        &self.obstacles[start..end.max(start)]
    }
}

fn unit(rng: &mut Rng) -> f32 {
    rng.range_u32(0, 10_000) as f32 / 10_000.0
}

/// A value from -1 to 1.
fn signed(rng: &mut Rng) -> f32 {
    unit(rng) * 2.0 - 1.0
}

fn spacing(z: f32) -> f32 {
    if z < CAVE_Z {
        CANYON_SPACING
    } else {
        MOUTH_SPACING + (NARROWEST_SPACING - MOUTH_SPACING) * narrowing(z)
    }
}

fn generate(seed: u64) -> Vec<Obstacle> {
    let mut rng = Rng::new(seed ^ 0x6c61_6e74_6572_6e00);
    let mut obstacles = Vec::new();
    let mut z = FIRST_OBSTACLE;
    while z < COURSE_LENGTH {
        let half = half_extents(z);
        let small = half.x.min(half.y);
        // More gates in the cave, where they bunch everyone into one hole.
        let gate_percent = if z < CAVE_Z { 12 } else { 30 };
        let roll = rng.range_u32(0, 99);
        let shape = if roll < gate_percent {
            let radius = (small * 0.45).max(1.05);
            Shape::Gate {
                center: Vec2::new(
                    signed(&mut rng) * (half.x - radius - 0.2).max(0.0),
                    signed(&mut rng) * (half.y - radius - 0.2).max(0.0),
                ),
                radius,
            }
        } else if roll < gate_percent + 30 {
            let half_width = 0.35 + unit(&mut rng) * 0.08 * half.x;
            let reach = (half.x - half_width - PASSAGE * 0.5).max(0.0);
            Shape::Pillar {
                x: signed(&mut rng) * reach,
                half_width,
            }
        } else if roll < gate_percent + 50 {
            let half_height = 0.3 + unit(&mut rng) * 0.08 * half.y;
            let reach = (half.y - half_height - PASSAGE * 0.5).max(0.0);
            Shape::Beam {
                y: signed(&mut rng) * reach,
                half_height,
            }
        } else {
            let radius = small * (0.3 + unit(&mut rng) * 0.25);
            Shape::Rock {
                center: Vec2::new(
                    signed(&mut rng) * half.x * 0.75,
                    signed(&mut rng) * half.y * 0.75,
                ),
                radius,
            }
        };
        let depth = match shape {
            Shape::Rock { radius, .. } => radius.min(1.5),
            _ => 1.0,
        };
        obstacles.push(Obstacle { z, depth, shape });
        z += spacing(z) * (0.8 + unit(&mut rng) * 0.4);
    }
    obstacles
}
