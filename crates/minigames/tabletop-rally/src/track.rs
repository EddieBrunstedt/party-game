//! The hand-laid tracks. Each is a closed Catmull-Rom loop through a few control points, laid
//! out across the same giant table, with props that are solid to the cars (teacups, spools,
//! pencils as rails). The loop is sampled into evenly spaced centreline points once, with plain
//! arithmetic, so every machine builds exactly the same track.

use std::sync::OnceLock;

use sim_core::math;
use sim_core::physics::Vec2;

/// Centreline samples are about this far apart (each track divides its length evenly).
pub const SPACING: f32 = 0.4;
/// Half the table's size. Beyond it a car falls off.
pub const TABLE_HALF: Vec2 = Vec2::new(34.0, 23.0);
pub const TRACK_COUNT: usize = TRACKS.len();
/// A stretch this long with no bend sharper than [`STRAIGHT_CURVATURE`] is a straight: rounds
/// start on one.
pub const STRAIGHT: f32 = 6.0;
pub const STRAIGHT_CURVATURE: f32 = 0.1;
const INDEX_SLACK: f32 = 1e-3;

/// What a table is laid with, for the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Theme {
    Breakfast,
    Desk,
    Picnic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropKind {
    Teacup,
    Spool,
    SugarCube,
    Pencil,
    Apple,
    Eraser,
}

impl PropKind {
    pub fn radius(self) -> f32 {
        match self {
            PropKind::Teacup => 1.1,
            PropKind::Spool => 0.65,
            PropKind::SugarCube => 0.35,
            PropKind::Pencil => 0.18,
            PropKind::Apple => 0.85,
            PropKind::Eraser => 0.5,
        }
    }
}

/// Where a prop goes across the road.
#[derive(Debug, Clone, Copy)]
pub enum Across {
    /// This far beyond the road's edge on the outside of the bend there.
    Outside(f32),
    /// This far beyond the road's edge on the inside of the bend.
    Inside(f32),
    /// On the road, this far from the middle towards the outside (negative: the inside).
    Road(f32),
}

/// A prop placed relative to the track: `at` is how far round the loop (0..1), and `length`
/// how long a pencil lies along the road.
#[derive(Debug, Clone, Copy)]
pub struct PropDef {
    pub kind: PropKind,
    pub at: f32,
    pub across: Across,
    pub length: f32,
}

const fn prop(kind: PropKind, at: f32, across: Across) -> PropDef {
    PropDef {
        kind,
        at,
        across,
        length: 0.0,
    }
}

/// A pencil lying along the outside of the bend, as a rail.
const fn rail(at: f32, length: f32) -> PropDef {
    PropDef {
        kind: PropKind::Pencil,
        at,
        across: Across::Outside(0.35),
        length,
    }
}

pub struct TrackDef {
    pub theme: Theme,
    pub width: f32,
    pub points: &'static [(f32, f32)],
    pub props: &'static [PropDef],
}

use Across::{Inside, Outside, Road};
use PropKind::{Apple, Eraser, Spool, SugarCube, Teacup};

pub const TRACKS: [TrackDef; 3] = [
    TrackDef {
        theme: Theme::Breakfast,
        width: 4.0,
        points: &[
            (-26.0, -14.0),
            (-6.0, -16.0),
            (4.0, -11.0),
            (14.0, -16.0),
            (26.0, -12.0),
            (28.5, -2.0),
            (24.5, 5.0),
            (27.5, 13.0),
            (19.0, 19.0),
            (8.0, 17.5),
            (0.0, 12.5),
            (-10.0, 18.5),
            (-24.0, 15.0),
            (-28.5, 2.0),
        ],
        props: &[
            prop(SugarCube, 0.05, Road(0.6)),
            prop(Teacup, 0.15, Outside(0.6)),
            prop(SugarCube, 0.24, Road(-0.6)),
            rail(0.33, 6.0),
            prop(Teacup, 0.42, Inside(1.0)),
            rail(0.5, 5.0),
            prop(SugarCube, 0.58, Road(0.6)),
            prop(Spool, 0.66, Outside(0.4)),
            prop(SugarCube, 0.75, Road(-0.6)),
            rail(0.86, 6.0),
            prop(Teacup, 0.95, Inside(1.5)),
        ],
    },
    TrackDef {
        theme: Theme::Desk,
        width: 4.0,
        points: &[
            (-28.0, -16.0),
            (8.0, -17.0),
            (16.0, -12.0),
            (10.0, -6.0),
            (-6.0, -6.0),
            (-10.0, 0.0),
            (-4.0, 6.0),
            (14.0, 4.0),
            (26.0, 8.0),
            (26.0, 17.0),
            (-2.0, 18.0),
            (-26.0, 16.0),
            (-28.0, 2.0),
        ],
        props: &[
            prop(Eraser, 0.08, Road(0.5)),
            rail(0.2, 6.0),
            prop(Spool, 0.27, Inside(0.5)),
            prop(Eraser, 0.33, Road(-0.5)),
            rail(0.4, 6.0),
            prop(Spool, 0.48, Outside(0.4)),
            prop(Eraser, 0.56, Road(0.5)),
            rail(0.62, 7.0),
            prop(Spool, 0.72, Inside(0.6)),
            prop(Eraser, 0.8, Road(-0.5)),
            rail(0.9, 7.0),
        ],
    },
    TrackDef {
        theme: Theme::Picnic,
        width: 4.0,
        points: &[
            (-28.0, -12.0),
            (-14.0, -18.0),
            (0.0, -12.0),
            (12.0, -18.0),
            (27.0, -14.0),
            (22.0, -4.0),
            (8.0, -2.0),
            (6.0, 8.0),
            (22.0, 10.0),
            (24.0, 18.0),
            (4.0, 19.0),
            (-10.0, 12.0),
            (-22.0, 18.0),
            (-30.0, 6.0),
        ],
        props: &[
            prop(Apple, 0.07, Outside(0.5)),
            prop(SugarCube, 0.14, Road(0.6)),
            prop(Teacup, 0.22, Inside(0.8)),
            rail(0.3, 6.0),
            prop(SugarCube, 0.38, Road(-0.6)),
            prop(Apple, 0.46, Outside(0.5)),
            prop(SugarCube, 0.55, Road(0.6)),
            rail(0.64, 6.0),
            prop(Apple, 0.72, Inside(0.8)),
            prop(SugarCube, 0.8, Road(-0.6)),
            prop(Teacup, 0.9, Outside(0.8)),
        ],
    },
];

/// A solid prop: a capsule from `a` to `b` (a circle when they're the same point).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prop {
    pub kind: PropKind,
    pub a: Vec2,
    pub b: Vec2,
    pub radius: f32,
}

impl Prop {
    /// The point on the prop's core segment nearest `p`.
    pub fn closest(&self, p: Vec2) -> Vec2 {
        let ab = self.b - self.a;
        let len_sq = ab.length_squared();
        if len_sq < 1e-9 {
            return self.a;
        }
        let t = ((p - self.a).dot(ab) / len_sq).clamp(0.0, 1.0);
        self.a + ab * t
    }

    pub fn centre(&self) -> Vec2 {
        self.a.lerp(self.b, 0.5)
    }
}

pub struct Track {
    pub theme: Theme,
    pub width: f32,
    pub length: f32,
    /// Distance between neighbouring samples.
    pub spacing: f32,
    pub centre: Vec<Vec2>,
    /// Unit direction of travel at each sample.
    pub tangent: Vec<Vec2>,
    /// Signed curvature at each sample (radians per unit; positive turns towards the normal).
    pub curvature: Vec<f32>,
    pub props: Vec<Prop>,
}

/// Track `index`, built on first use.
pub fn track(index: usize) -> &'static Track {
    static BUILT: OnceLock<Vec<Track>> = OnceLock::new();
    &BUILT.get_or_init(|| TRACKS.iter().map(Track::build).collect())[index]
}

fn catmull_rom(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    let t2 = t * t;
    let t3 = t2 * t;
    (p1 * 2.0
        + (p2 - p0) * t
        + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * t2
        + (-p0 + p1 * 3.0 - p2 * 3.0 + p3) * t3)
        * 0.5
}

impl Track {
    fn build(def: &TrackDef) -> Track {
        const DENSE: usize = 200;
        let points: Vec<Vec2> = def.points.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
        let n = points.len();
        let mut dense = Vec::with_capacity(n * DENSE);
        for i in 0..n {
            let p = |k: usize| points[(i + n + k - 1) % n];
            for step in 0..DENSE {
                dense.push(catmull_rom(
                    p(0),
                    p(1),
                    p(2),
                    p(3),
                    step as f32 / DENSE as f32,
                ));
            }
        }
        let mut lengths = Vec::with_capacity(dense.len());
        let mut total = 0.0;
        for i in 0..dense.len() {
            lengths.push(total);
            total += dense[i].distance(dense[(i + 1) % dense.len()]);
        }
        let count = (total / SPACING).round() as usize;
        let spacing = total / count as f32;
        let mut centre = Vec::with_capacity(count);
        let mut j = 0;
        for k in 0..count {
            let s = k as f32 * spacing;
            while j + 1 < dense.len() && lengths[j + 1] <= s {
                j += 1;
            }
            let next = (j + 1) % dense.len();
            let seg = dense[j].distance(dense[next]);
            let t = if seg > 1e-6 {
                (s - lengths[j]) / seg
            } else {
                0.0
            };
            centre.push(dense[j].lerp(dense[next], t));
        }
        let wrap = |i: isize| i.rem_euclid(count as isize) as usize;
        let tangent: Vec<Vec2> = (0..count as isize)
            .map(|i| (centre[wrap(i + 1)] - centre[wrap(i - 1)]).normalize_or_zero())
            .collect();
        const REACH: isize = 3;
        let curvature: Vec<f32> = (0..count as isize)
            .map(|i| {
                let (a, b) = (tangent[wrap(i - REACH)], tangent[wrap(i + REACH)]);
                math::atan2(a.perp_dot(b), a.dot(b)) / (2 * REACH) as f32 / spacing
            })
            .collect();
        let mut track = Track {
            theme: def.theme,
            width: def.width,
            length: total,
            spacing,
            centre,
            tangent,
            curvature,
            props: Vec::new(),
        };
        // The loop starts (and the first round with it) on the first straight.
        let first = (0..count).find(|i| track.straight_at(*i)).unwrap_or(0);
        track.centre.rotate_left(first);
        track.tangent.rotate_left(first);
        track.curvature.rotate_left(first);
        track.props = def
            .props
            .iter()
            .map(|p| {
                // Placed from where the loop's control points begin, before the turn above.
                let s = p.at * total - first as f32 * spacing;
                let side = track.place_across(s, p.across, p.kind.radius());
                let middle = track.point_at(s) + track.normal_at(s) * side;
                let along = track.tangent_at(s) * (p.length / 2.0);
                Prop {
                    kind: p.kind,
                    a: middle - along,
                    b: middle + along,
                    radius: p.kind.radius(),
                }
            })
            .collect();
        track
    }

    /// Which way the road bends around `s`: 1 towards the normal, -1 away (straight counts as
    /// bending away, so "outside" is the normal's side).
    fn bend(&self, s: f32) -> f32 {
        let i = self.index(s) as isize;
        let reach = (4.0 / self.spacing) as isize;
        let total: f32 = (-reach..=reach)
            .map(|k| self.curvature[self.wrap(i + k)])
            .sum();
        if total > 0.0 { 1.0 } else { -1.0 }
    }

    /// How far along the normal at `s` a prop of `radius` sits for `across`.
    fn place_across(&self, s: f32, across: Across, radius: f32) -> f32 {
        let outside = -self.bend(s);
        let edge = self.width / 2.0 + radius;
        match across {
            Across::Outside(gap) => outside * (edge + gap),
            Across::Inside(gap) => -outside * (edge + gap),
            Across::Road(offset) => outside * offset,
        }
    }

    /// The start of the first straight at or after `s` (laps included), on a sample.
    pub fn straight_from(&self, s: f32) -> f32 {
        let start = self.index(s);
        let k = (0..self.samples())
            .find(|k| self.straight_at(start + k))
            .unwrap_or(0);
        s - self.offset(s) + k as f32 * self.spacing
    }

    /// Whether a straight starts at sample `i`.
    fn straight_at(&self, i: usize) -> bool {
        let reach = (STRAIGHT / self.spacing) as usize;
        (0..=reach).all(|j| self.curvature[(i + j) % self.samples()].abs() <= STRAIGHT_CURVATURE)
    }

    /// How far past its sample `s` lies.
    fn offset(&self, s: f32) -> f32 {
        (s - (s / self.spacing + INDEX_SLACK).floor() * self.spacing).max(0.0)
    }

    pub fn samples(&self) -> usize {
        self.centre.len()
    }

    pub fn wrap(&self, i: isize) -> usize {
        i.rem_euclid(self.samples() as isize) as usize
    }

    /// The sample at or before distance `s` round the loop (any `s`, laps included). A distance
    /// a hair short of a sample counts as on it, so whole multiples of the spacing round right.
    pub fn index(&self, s: f32) -> usize {
        self.wrap((s / self.spacing + INDEX_SLACK).floor() as isize)
    }

    fn blend(&self, s: f32, of: &[Vec2]) -> Vec2 {
        let i = (s / self.spacing).floor();
        let t = s / self.spacing - i;
        let i = i as isize;
        of[self.wrap(i)].lerp(of[self.wrap(i + 1)], t)
    }

    pub fn point_at(&self, s: f32) -> Vec2 {
        self.blend(s, &self.centre)
    }

    pub fn tangent_at(&self, s: f32) -> Vec2 {
        self.blend(s, &self.tangent).normalize_or_zero()
    }

    /// The tangent turned a quarter counter-clockwise: positive `side` offsets go this way.
    pub fn normal_at(&self, s: f32) -> Vec2 {
        self.tangent_at(s).perp()
    }

    /// The sample nearest `pos` within `reach` samples either side of `around`.
    pub fn nearest(&self, pos: Vec2, around: usize, reach: isize) -> usize {
        let mut best = around;
        let mut best_d = f32::MAX;
        for k in -reach..=reach {
            let i = self.wrap(around as isize + k);
            let d = (self.centre[i] - pos).length_squared();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        best
    }

    /// The sample nearest `pos` anywhere on the loop.
    pub fn nearest_anywhere(&self, pos: Vec2) -> usize {
        self.nearest(pos, 0, self.samples() as isize / 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_track_is_a_clean_loop_on_the_table() {
        for index in 0..TRACK_COUNT {
            let t = track(index);
            assert!(t.length > 120.0, "track {index} is {} long", t.length);
            for i in 0..t.samples() {
                let next = t.centre[(i + 1) % t.samples()];
                let gap = t.centre[i].distance(next);
                assert!(
                    (gap - t.spacing).abs() < 0.05,
                    "track {index} sample {i}: {gap}"
                );
                let p = t.centre[i];
                let margin = (TABLE_HALF.x - p.x.abs()).min(TABLE_HALF.y - p.y.abs());
                assert!(
                    margin >= t.width / 2.0 + 1.0,
                    "track {index} sample {i} is {margin} from the table's edge"
                );
                // Parts of the loop far apart along it must be far apart on the table, so the
                // road never runs into itself and corners can't be cut through a thin gap.
                for j in 0..t.samples() {
                    let along = (i as isize - j as isize).unsigned_abs();
                    let along = along.min(t.samples() - along) as f32 * t.spacing;
                    if along > 4.0 * t.width {
                        let d = p.distance(t.centre[j]);
                        assert!(
                            d > 2.0 * t.width,
                            "track {index}: samples {i} and {j} are only {d} apart"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn corners_are_drivable() {
        for index in 0..TRACK_COUNT {
            let t = track(index);
            // No tighter than this, or the road's inside edge folds over itself.
            let tightest = t.curvature.iter().fold(0.0f32, |m, c| m.max(c.abs()));
            assert!(tightest < 0.4, "track {index}: radius {}", 1.0 / tightest);
        }
    }

    #[test]
    fn props_off_the_road_leave_it_clear_and_cubes_leave_room_to_pass() {
        for index in 0..TRACK_COUNT {
            let t = track(index);
            for (k, prop) in t.props.iter().enumerate() {
                let i = t.nearest_anywhere(prop.centre());
                let off = t.centre[i].distance(prop.centre());
                let edge = t.width / 2.0;
                if off > edge {
                    // Outside the road: no part of it pokes onto the road.
                    for end in [prop.a, prop.b] {
                        let j = t.nearest_anywhere(end);
                        let d = t.centre[j].distance(end);
                        assert!(d - prop.radius >= edge - 0.05, "track {index} prop {k}");
                    }
                } else {
                    // On the road: there's room for a car on both sides.
                    let room = edge - off - prop.radius;
                    assert!(room > 0.9, "track {index} prop {k}: room {room}");
                }
            }
        }
    }

    #[test]
    fn every_track_starts_on_a_straight_and_has_more() {
        for index in 0..TRACK_COUNT {
            let t = track(index);
            assert_eq!(t.straight_from(0.0), 0.0, "track {index}");
            // Straights come round often enough that a round never restarts far on.
            let gap = (0..t.samples())
                .map(|i| t.straight_from(i as f32 * t.spacing) - i as f32 * t.spacing)
                .fold(0.0f32, f32::max);
            assert!(gap < 32.0, "track {index}: {gap} to the next straight");
        }
    }

    #[test]
    fn lookups_wrap_round_the_loop() {
        let t = track(0);
        assert_eq!(t.index(t.length + 0.01), 0);
        assert_eq!(t.index(-0.01), t.samples() - 1);
        assert!(t.point_at(t.length).distance(t.point_at(0.0)) < 1e-3);
        assert_eq!(t.nearest(t.centre[5], t.samples() - 2, 8), 5);
    }
}
