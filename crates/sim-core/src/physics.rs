//! Simple deterministic 2D physics for minigames (ADR 0006): circles moving on the arena floor,
//! pushing each other apart and trading momentum. Everything is plain `f32` arithmetic plus
//! `sqrt` (an exact IEEE operation); anything transcendental goes through [`crate::math`].
//!
//! The floor plane uses `x` to the right and `y` towards the camera, so a renderer maps
//! `Vec2 { x, y }` to world `(x, 0, y)`.

use std::hash::{Hash, Hasher};
use std::ops::{Add, AddAssign, Mul, MulAssign, Neg, Sub, SubAssign};

use crate::hash::hash_f32;
use crate::{Input, math};

/// Seconds per simulation tick.
pub const DT: f32 = 1.0 / crate::TICK_HZ as f32;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Unit vector at `angle` radians from the +x axis.
    pub fn from_angle(angle: f32) -> Self {
        Self::new(math::cos(angle), math::sin(angle))
    }

    /// The stick as a floor direction with length 0..=1. Stick up moves away from the camera
    /// (−y). Tiny deflections count as neutral so worn sticks don't drift.
    pub fn from_stick(input: &Input) -> Self {
        const DEADZONE: f32 = 0.15;
        let v = Self::new(
            input.stick_x as f32 / 127.0,
            -(input.stick_y as f32) / 127.0,
        );
        let len = v.length();
        if len < DEADZONE {
            Self::ZERO
        } else {
            v.clamp_length(1.0)
        }
    }

    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    /// The z component of the 3D cross product: positive when `other` is counter-clockwise.
    pub fn perp_dot(self, other: Self) -> f32 {
        self.x * other.y - self.y * other.x
    }

    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }

    pub fn normalize_or_zero(self) -> Self {
        let len = self.length();
        if len > 1e-6 {
            self * (1.0 / len)
        } else {
            Self::ZERO
        }
    }

    pub fn clamp_length(self, max: f32) -> Self {
        let len = self.length();
        if len > max { self * (max / len) } else { self }
    }

    /// Rotated a quarter turn counter-clockwise.
    pub fn perp(self) -> Self {
        Self::new(-self.y, self.x)
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        self + (other - self) * t
    }

    /// Angle in radians from the +x axis.
    pub fn angle(self) -> f32 {
        math::atan2(self.y, self.x)
    }
}

impl Hash for Vec2 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_f32(self.x, state);
        hash_f32(self.y, state);
    }
}

impl Add for Vec2 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for Vec2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Mul<f32> for Vec2 {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl Neg for Vec2 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl MulAssign<f32> for Vec2 {
    fn mul_assign(&mut self, rhs: f32) {
        *self = *self * rhs;
    }
}

/// A circle on the floor. Velocity is in units per second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    pub pos: Vec2,
    pub vel: Vec2,
    pub radius: f32,
    pub mass: f32,
}

impl Hash for Body {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pos.hash(state);
        self.vel.hash(state);
        hash_f32(self.radius, state);
        hash_f32(self.mass, state);
    }
}

impl Body {
    pub fn new(pos: Vec2, radius: f32, mass: f32) -> Self {
        Self {
            pos,
            vel: Vec2::ZERO,
            radius,
            mass,
        }
    }

    /// Steer towards `direction * max_speed`, changing velocity by at most `accel * DT` this tick.
    pub fn steer(&mut self, direction: Vec2, max_speed: f32, accel: f32) {
        let wanted = direction.clamp_length(1.0) * max_speed;
        self.vel += (wanted - self.vel).clamp_length(accel * DT);
    }

    /// Instant change of velocity, e.g. a dash or a hit.
    pub fn impulse(&mut self, impulse: Vec2) {
        self.vel += impulse * (1.0 / self.mass);
    }

    /// Advance one tick, then bleed off speed. `drag` is the fraction of velocity lost per second.
    pub fn integrate(&mut self, drag: f32) {
        self.pos += self.vel * DT;
        self.vel *= (1.0 - drag * DT).max(0.0);
    }

    pub fn overlaps(&self, other: &Body) -> bool {
        let reach = self.radius + other.radius;
        (self.pos - other.pos).length_squared() < reach * reach
    }

    /// Keep the body inside an axis-aligned rectangle centred on the origin, bouncing off walls.
    pub fn bounce_in_rect(&mut self, half_extents: Vec2, restitution: f32) -> bool {
        let mut hit = false;
        let max = Vec2::new(half_extents.x - self.radius, half_extents.y - self.radius);
        if self.pos.x < -max.x || self.pos.x > max.x {
            self.pos.x = self.pos.x.clamp(-max.x, max.x);
            self.vel.x = -self.vel.x * restitution;
            hit = true;
        }
        if self.pos.y < -max.y || self.pos.y > max.y {
            self.pos.y = self.pos.y.clamp(-max.y, max.y);
            self.vel.y = -self.vel.y * restitution;
            hit = true;
        }
        hit
    }
}

/// What happened when two bodies touched. `normal` points from the first body to the second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    pub normal: Vec2,
    /// Momentum exchanged along the normal (0 if they were already separating).
    pub impulse: f32,
}

/// Separate two overlapping circles (split by mass) and bounce them along the contact normal.
pub fn collide(a: &mut Body, b: &mut Body, restitution: f32) -> Option<Contact> {
    let delta = b.pos - a.pos;
    let reach = a.radius + b.radius;
    let dist_sq = delta.length_squared();
    if dist_sq >= reach * reach {
        return None;
    }
    let dist = dist_sq.sqrt();
    // Perfectly stacked bodies get pushed apart along +x rather than not at all.
    let normal = if dist > 1e-6 {
        delta * (1.0 / dist)
    } else {
        Vec2::new(1.0, 0.0)
    };
    let inv_a = 1.0 / a.mass;
    let inv_b = 1.0 / b.mass;
    let inv_sum = inv_a + inv_b;

    let overlap = reach - dist;
    a.pos -= normal * (overlap * inv_a / inv_sum);
    b.pos += normal * (overlap * inv_b / inv_sum);

    let closing = (a.vel - b.vel).dot(normal);
    let impulse = if closing > 0.0 {
        let j = (1.0 + restitution) * closing / inv_sum;
        a.vel -= normal * (j * inv_a);
        b.vel += normal * (j * inv_b);
        j
    } else {
        0.0
    };
    Some(Contact { normal, impulse })
}

/// Collide every pair once, in index order (so results never depend on anything but the slice).
/// Calls `on_contact(i, j, contact)` for each touching pair with `i < j`.
pub fn collide_all(
    bodies: &mut [Body],
    restitution: f32,
    mut on_contact: impl FnMut(usize, usize, Contact),
) {
    for i in 0..bodies.len() {
        for j in i + 1..bodies.len() {
            let (left, right) = bodies.split_at_mut(j);
            if let Some(contact) = collide(&mut left[i], &mut right[0], restitution) {
                on_contact(i, j, contact);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checksum;

    #[test]
    fn head_on_equal_masses_swap_velocities() {
        let mut a = Body::new(Vec2::new(0.0, 0.0), 0.5, 1.0);
        let mut b = Body::new(Vec2::new(0.9, 0.0), 0.5, 1.0);
        a.vel = Vec2::new(4.0, 0.0);
        let contact = collide(&mut a, &mut b, 1.0).unwrap();
        assert_eq!(contact.normal, Vec2::new(1.0, 0.0));
        assert!((a.vel.x - 0.0).abs() < 1e-5);
        assert!((b.vel.x - 4.0).abs() < 1e-5);
        assert!(a.pos.distance(b.pos) >= 1.0 - 1e-5);
    }

    #[test]
    fn heavy_bodies_barely_move() {
        let mut light = Body::new(Vec2::new(0.0, 0.0), 0.5, 1.0);
        let mut heavy = Body::new(Vec2::new(0.8, 0.0), 0.5, 10.0);
        light.vel = Vec2::new(5.0, 0.0);
        collide(&mut light, &mut heavy, 0.5).unwrap();
        assert!(heavy.vel.x < 1.0);
        assert!(light.vel.x < 0.0);
    }

    #[test]
    fn separating_bodies_exchange_no_impulse() {
        let mut a = Body::new(Vec2::new(0.0, 0.0), 0.5, 1.0);
        let mut b = Body::new(Vec2::new(0.9, 0.0), 0.5, 1.0);
        b.vel = Vec2::new(3.0, 0.0);
        assert_eq!(collide(&mut a, &mut b, 1.0).unwrap().impulse, 0.0);
    }

    #[test]
    fn steering_respects_acceleration() {
        let mut body = Body::new(Vec2::ZERO, 0.5, 1.0);
        body.steer(Vec2::new(1.0, 0.0), 10.0, 60.0);
        assert!((body.vel.x - 1.0).abs() < 1e-5);
    }

    #[test]
    fn stick_up_points_away_from_camera() {
        let up = Input {
            stick_y: 127,
            ..Input::default()
        };
        assert_eq!(Vec2::from_stick(&up), Vec2::new(0.0, -1.0));
        let drift = Input {
            stick_x: 10,
            ..Input::default()
        };
        assert_eq!(Vec2::from_stick(&drift), Vec2::ZERO);
    }

    #[test]
    fn walls_bounce() {
        let mut body = Body::new(Vec2::new(4.9, 0.0), 0.5, 1.0);
        body.vel = Vec2::new(2.0, 0.0);
        assert!(body.bounce_in_rect(Vec2::new(5.0, 5.0), 0.5));
        assert_eq!(body.pos.x, 4.5);
        assert_eq!(body.vel.x, -1.0);
    }

    #[test]
    fn hashes_by_bit_pattern() {
        assert_ne!(
            checksum(&Vec2::new(0.0, 0.0)),
            checksum(&Vec2::new(-0.0, 0.0))
        );
    }
}
