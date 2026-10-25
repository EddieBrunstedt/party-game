//! Simple deterministic 2D physics for minigames (ADR 0006): circles moving on the arena floor,
//! pushing each other apart and trading momentum, slack ropes between them, and straight walls and
//! fixed posts they bounce off. Everything is plain `f32` arithmetic plus
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

/// A slack rope of at most `length` between two bodies: it does nothing while slack, and once
/// stretched pulls both back to `length` (split by mass) and takes away the speed they had apart
/// along it, as an inelastic rope does. Returns whether it was taut.
pub fn tether(a: &mut Body, b: &mut Body, length: f32) -> bool {
    let delta = b.pos - a.pos;
    let dist_sq = delta.length_squared();
    if dist_sq <= length * length {
        return false;
    }
    let dist = dist_sq.sqrt();
    let normal = delta * (1.0 / dist);
    let inv_a = 1.0 / a.mass;
    let inv_b = 1.0 / b.mass;
    let inv_sum = inv_a + inv_b;

    let stretch = dist - length;
    a.pos += normal * (stretch * inv_a / inv_sum);
    b.pos -= normal * (stretch * inv_b / inv_sum);

    let parting = (b.vel - a.vel).dot(normal);
    if parting > 0.0 {
        let j = parting / inv_sum;
        a.vel += normal * (j * inv_a);
        b.vel -= normal * (j * inv_b);
    }
    true
}

/// Keep a body on the inner side of a straight wall: the line of points `p` with
/// `p.dot(normal) == offset`, where `normal` is a unit vector pointing out of the arena. A body
/// pushing through is put back and bounces. Returns the speed it hit the wall with, if it touched.
pub fn bounce_off_wall(
    body: &mut Body,
    normal: Vec2,
    offset: f32,
    restitution: f32,
) -> Option<f32> {
    let depth = body.pos.dot(normal) - offset + body.radius;
    if depth <= 0.0 {
        return None;
    }
    body.pos -= normal * depth;
    let into = body.vel.dot(normal);
    if into > 0.0 {
        body.vel -= normal * ((1.0 + restitution) * into);
        Some(into)
    } else {
        Some(0.0)
    }
}

/// Push a body out of an immovable circle (a post or a bumper) and bounce it. The contact's
/// normal points from the circle to the body.
pub fn bounce_off_circle(
    body: &mut Body,
    center: Vec2,
    radius: f32,
    restitution: f32,
) -> Option<Contact> {
    let delta = body.pos - center;
    let reach = body.radius + radius;
    let dist_sq = delta.length_squared();
    if dist_sq >= reach * reach {
        return None;
    }
    let dist = dist_sq.sqrt();
    let normal = if dist > 1e-6 {
        delta * (1.0 / dist)
    } else {
        Vec2::new(1.0, 0.0)
    };
    body.pos = center + normal * reach;
    let into = -body.vel.dot(normal);
    let impulse = if into > 0.0 {
        body.vel += normal * ((1.0 + restitution) * into);
        into * body.mass
    } else {
        0.0
    };
    Some(Contact { normal, impulse })
}

/// The point on the segment from `a` to `b` closest to `p`, and how far along it lies (0 at `a`,
/// 1 at `b`).
pub fn closest_on_segment(a: Vec2, b: Vec2, p: Vec2) -> (Vec2, f32) {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq < 1e-12 {
        return (a, 0.0);
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    (a + ab * t, t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checksum;

    #[test]
    fn a_slack_tether_does_nothing() {
        let mut a = Body::new(Vec2::ZERO, 0.5, 1.0);
        let mut b = Body::new(Vec2::new(1.5, 0.0), 0.3, 0.5);
        b.vel = Vec2::new(3.0, 0.0);
        assert!(!tether(&mut a, &mut b, 2.0));
        assert_eq!(b.pos, Vec2::new(1.5, 0.0));
        assert_eq!(b.vel, Vec2::new(3.0, 0.0));
    }

    #[test]
    fn a_taut_tether_pulls_both_back_and_keeps_the_swing() {
        let mut a = Body::new(Vec2::ZERO, 0.5, 1.0);
        let mut b = Body::new(Vec2::new(3.0, 0.0), 0.3, 1.0);
        b.vel = Vec2::new(2.0, 5.0);
        assert!(tether(&mut a, &mut b, 2.0));
        assert!((a.pos.distance(b.pos) - 2.0).abs() < 1e-5);
        assert_eq!(a.pos, Vec2::new(0.5, 0.0));
        // The outward speed is shared out; the speed across the rope stays.
        assert!((b.vel.x - 1.0).abs() < 1e-5 && (a.vel.x - 1.0).abs() < 1e-5);
        assert_eq!(b.vel.y, 5.0);
    }

    #[test]
    fn walls_put_bodies_back_and_bounce_them() {
        let normal = Vec2::new(0.0, 1.0);
        let mut body = Body::new(Vec2::new(1.0, 4.8), 0.5, 1.0);
        body.vel = Vec2::new(1.0, 2.0);
        assert_eq!(bounce_off_wall(&mut body, normal, 5.0, 0.5), Some(2.0));
        assert!((body.pos.y - 4.5).abs() < 1e-5);
        assert_eq!(body.vel, Vec2::new(1.0, -1.0));
        let mut clear = Body::new(Vec2::new(0.0, 4.0), 0.5, 1.0);
        assert_eq!(bounce_off_wall(&mut clear, normal, 5.0, 0.5), None);
    }

    #[test]
    fn circles_push_out_and_bounce() {
        let mut body = Body::new(Vec2::new(-0.9, 0.0), 0.5, 2.0);
        body.vel = Vec2::new(3.0, 0.0);
        let contact = bounce_off_circle(&mut body, Vec2::ZERO, 0.5, 1.0).unwrap();
        assert_eq!(contact.normal, Vec2::new(-1.0, 0.0));
        assert_eq!(contact.impulse, 6.0);
        assert!((body.pos.x + 1.0).abs() < 1e-5);
        assert_eq!(body.vel, Vec2::new(-3.0, 0.0));
    }

    #[test]
    fn closest_point_clamps_to_the_ends() {
        let (a, b) = (Vec2::ZERO, Vec2::new(4.0, 0.0));
        assert_eq!(
            closest_on_segment(a, b, Vec2::new(1.0, 2.0)),
            (Vec2::new(1.0, 0.0), 0.25)
        );
        assert_eq!(closest_on_segment(a, b, Vec2::new(-3.0, 1.0)), (a, 0.0));
        assert_eq!(closest_on_segment(a, b, Vec2::new(9.0, 1.0)), (b, 1.0));
    }

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
