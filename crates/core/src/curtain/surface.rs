//! Where a curtain wall's parts stand: its grid's surface. A layout is worked out flat, `along` its run, `up` its face and
//! `across` from its grid plane (toward the exterior); the surface puts those in the model.
//!
//! - **Along.** A straight wall runs from its start to its end. A curved one is faceted: a straight chord from each vertical grid
//!   line to the next, its ends on the wall's arc, so every panel is flat and as wide as its segment (Revit's curved curtain wall
//!   with a vertical grid). Its run is the chords' length, which [`unrolled`] finds for the wall's sweep.
//! - **Up.** Upright, up is up. A slanted wall's face leans `slant` degrees toward its exterior, so its rows measure up the
//!   slope ([`Surface::height`]) and each part tilts with it.

use std::f64::consts::PI;

use crate::geom::P2;
use crate::model::{Model, Wall};
use crate::walls::{Axis, MAX_SLANT};

use super::{Direction, GridWay, SAME, Span, by_hand, divide};

/// A point in space, mm.
type P3 = [f64; 3];

/// A curtain wall's grid surface: its run in plan, its base, its grid plane and its lean.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Surface {
    /// Where the run turns: how far along it, and the point on the wall's location line there. A straight wall's are its ends.
    knots: Vec<(f64, P2)>,
    base: f64,
    plane: f64,
    /// The sine and cosine of how far its face leans toward its exterior.
    lean: (f64, f64),
}

impl Surface {
    /// A straight wall's: from its start to its end.
    pub(crate) fn straight(m: &Model, w: &Wall, plane: f64) -> Surface {
        let len = w.start.dist(w.end);
        Surface { knots: vec![(0.0, w.start), (len, w.end)], base: m.own_base(w), plane, lean: lean(w) }
    }

    /// A curved wall's, its chords between the lines of `columns` (along a run `length` long), their ends on its arc.
    pub(crate) fn curved(m: &Model, w: &Wall, plane: f64, columns: &[Span], length: f64) -> Surface {
        let axis = Axis::of(w);
        let (Some((c, r, a0, sweep)), true) = (axis.arc(), length > 0.0) else { return Surface::straight(m, w, plane) };
        let mut lines: Vec<f64> = columns.iter().map(|s| s.from).chain([length]).collect();
        lines.dedup_by(|a, b| (*a - *b).abs() < SAME);
        // Each chord turns as much as its length subtends; together scaled to the wall's sweep, so the last ends at its end.
        let turns: Vec<f64> = lines.windows(2).map(|p| chord_turn(p[1] - p[0], r)).collect();
        let total: f64 = turns.iter().sum();
        let scale = if total > 0.0 { sweep.abs() / total } else { 1.0 };
        let mut angle = 0.0;
        let mut knots = vec![(0.0, w.start)];
        for (i, turn) in turns.iter().enumerate() {
            angle += turn * scale;
            let a = a0 + sweep.signum() * angle;
            let at = if i + 1 == turns.len() { w.end } else { c + P2(a.cos(), a.sin()) * r };
            knots.push((lines[i + 1], at));
        }
        Surface { knots, base: m.own_base(w), plane, lean: lean(w) }
    }

    /// How long its run is.
    pub fn length(&self) -> f64 {
        self.knots.last().map_or(0.0, |k| k.0)
    }

    /// How tall its face is, up the slope, for a wall rising `rise` mm.
    pub fn height(&self, rise: f64) -> f64 {
        if self.lean.1 > 0.0 { rise / self.lean.1 } else { rise }
    }

    /// Whether it leans.
    pub fn leans(&self) -> bool {
        self.lean.0 != 0.0
    }

    /// Whether its run turns: a curved wall's.
    pub fn faceted(&self) -> bool {
        self.knots.len() > 2
    }

    /// The chord `along` lies on: its start, its end and how far along each is.
    fn chord(&self, along: f64) -> Option<((f64, P2), (f64, P2))> {
        let n = self.knots.len();
        if n < 2 {
            return None;
        }
        let i = self.knots.iter().position(|k| k.0 > along).unwrap_or(n - 1).clamp(1, n - 1);
        Some((self.knots[i - 1], self.knots[i]))
    }

    /// The way it runs at `along`, in plan: its chord's; at a line between two chords, halfway between theirs.
    pub fn way(&self, along: f64) -> P2 {
        let n = self.knots.len();
        if let Some(i) = (1..n.saturating_sub(1)).find(|i| (self.knots[*i].0 - along).abs() < SAME) {
            let (a, b) = ((self.knots[i].1 - self.knots[i - 1].1).unit(), (self.knots[i + 1].1 - self.knots[i].1).unit());
            let mid = a + b;
            if mid.length() > 1e-9 {
                return mid.unit();
            }
            return a;
        }
        self.chord(along).map_or(P2(1.0, 0.0), |(a, b)| (b.1 - a.1).unit())
    }

    /// The point `along` it, `off` mm toward its exterior (its left) from the location line, in plan, at its base.
    pub fn plan(&self, along: f64, off: f64) -> P2 {
        let Some(((a0, p0), (a1, p1))) = self.chord(along) else { return P2(0.0, 0.0) };
        let t = if a1 > a0 { (along - a0) / (a1 - a0) } else { 0.0 };
        let on = p0 + (p1 - p0) * t;
        on + self.way(along).perp() * off
    }

    /// The point of a part's frame `along` the run, `up` the face and `across` from the grid plane, in the model.
    pub fn at(&self, along: f64, up: f64, across: f64) -> P3 {
        let (sin, cos) = self.lean;
        let q = self.plan(along, self.plane + up * sin + across * cos);
        [q.0, q.1, self.base + up * cos - across * sin]
    }

    /// A part's axes at `along`: along the run, across the face toward the exterior, and up the face.
    pub fn axes(&self, along: f64, turn: f64) -> [P3; 3] {
        let (sin, cos) = self.lean;
        let u = self.way(along);
        let u = if turn == 0.0 { u } else { P2(u.0 * turn.cos() - u.1 * turn.sin(), u.0 * turn.sin() + u.1 * turn.cos()) };
        let n = u.perp();
        [[u.0, u.1, 0.0], [n.0 * cos, n.1 * cos, -sin], [n.0 * sin, n.1 * sin, cos]]
    }

    /// Where a point in plan is along it, and how far toward its exterior from the location line: on the nearest chord.
    pub fn project(&self, p: P2) -> (f64, f64) {
        let mut best = (f64::INFINITY, 0.0, 0.0);
        for k in self.knots.windows(2) {
            let ((a0, p0), (a1, p1)) = (k[0], k[1]);
            let d = p1 - p0;
            let len = d.length();
            if len <= 0.0 {
                continue;
            }
            let u = d * (1.0 / len);
            let t = (p - p0).dot(u).clamp(0.0, len);
            let off = (p - p0).dot(u.perp());
            let dist = p.dist(p0 + u * t);
            if dist < best.0 {
                best = (dist, a0 + (a1 - a0) * t / len, off);
            }
        }
        (best.1, best.2)
    }

    /// Where the line from `a` through `b` meets the grid's face: along the run and up the face.
    pub fn hit(&self, a: P3, b: P3) -> Option<(f64, f64)> {
        let (sin, cos) = self.lean;
        let mut best: Option<(f64, f64, f64)> = None;
        for k in self.knots.windows(2) {
            let ((a0, p0), (a1, p1)) = (k[0], k[1]);
            let len = p0.dist(p1);
            if len <= 0.0 {
                continue;
            }
            let u = (p1 - p0) * (1.0 / len);
            let n = u.perp();
            // The face's plane: through the chord at the grid plane, its normal across it.
            let o = p0 + n * self.plane;
            let normal = [n.0 * cos, n.1 * cos, -sin];
            let side = |p: P3| (p[0] - o.0) * normal[0] + (p[1] - o.1) * normal[1] + (p[2] - self.base) * normal[2];
            let (da, db) = (side(a), side(b));
            if (da - db).abs() < 1e-12 {
                continue;
            }
            let t = da / (da - db);
            let q = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
            let s = (P2(q[0], q[1]) - o).dot(u);
            let up = (q[2] - self.base) * cos + (P2(q[0], q[1]) - o).dot(n) * sin;
            let inside = (-SAME..=len + SAME).contains(&s);
            let along = a0 + (a1 - a0) * s / len;
            if (inside || self.knots.len() == 2) && best.is_none_or(|x| t.abs() < x.2.abs()) {
                best = Some((along, up, t));
            }
        }
        best.map(|(along, up, _)| (along, up))
    }
}

/// How far a wall leans, as a sine and cosine.
fn lean(w: &Wall) -> (f64, f64) {
    if w.slant == 0.0 || !w.slant.is_finite() {
        return (0.0, 1.0);
    }
    let a = w.slant.clamp(-MAX_SLANT, MAX_SLANT).to_radians();
    (a.sin(), a.cos())
}

/// How far a chord `length` long turns round a circle of radius `r`.
fn chord_turn(length: f64, r: f64) -> f64 {
    2.0 * (length / (2.0 * r)).clamp(-1.0, 1.0).asin()
}

/// How long a curved wall's run is: the length of chords between its vertical grid lines whose turns add up to its sweep. Found by
/// halving, as the lines a pattern lays depend on the length it's given. None for a straight wall.
pub(crate) fn unrolled(w: &Wall, columns: &Direction, count: usize) -> Option<f64> {
    let axis = Axis::of(w);
    let (_, r, _, sweep) = axis.arc()?;
    let sweep = sweep.abs();
    if !(r.is_finite() && r > 0.0) {
        return None;
    }
    let turns = |length: f64| {
        let mut spans = divide(columns, length);
        if let Some(e) = w.curtain.as_ref().filter(|e| !e.grids.is_empty()) {
            by_hand(&mut spans, e, GridWay::Vertical, length, count);
        }
        let mut lines: Vec<f64> = spans.iter().map(|s| s.from).chain([length]).collect();
        lines.dedup_by(|a, b| (*a - *b).abs() < SAME);
        lines.windows(2).map(|p| chord_turn(p[1] - p[0], r)).sum::<f64>()
    };
    // Between the chord of the whole and the arc itself.
    let (mut lo, mut hi) = ((2.0 * r * (sweep / 2.0).min(PI / 2.0).sin()).max(SAME), r * sweep);
    for _ in 0..48 {
        let mid = (lo + hi) / 2.0;
        if turns(mid) < sweep { lo = mid } else { hi = mid }
    }
    Some((lo + hi) / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;

    fn session(script: &str) -> Session {
        let mut s = Session::new();
        s.script(script).unwrap_or_else(|e| panic!("{e}"));
        s
    }

    #[test]
    fn a_curved_curtain_walls_panels_are_flat_chords_of_its_segment_on_its_arc() {
        // A half circle 10 m across, on a 1500 mm grid: each panel a chord 1500 long, its ends on the arc.
        let s = session(
            r#"type {"category": "wall", "name": "CW", "curtain": {"columns": {"segments": [1500]}, "rows": {"segments": [3000]}}}
               wall {"points": [[-5000, 0], [5000, 0]], "arc": 180, "type": "CW", "height": 3000}"#,
        );
        let w = s.model.wall(100002).unwrap_or_else(|| panic!("no wall"));
        let l = super::super::layout(&s.model, w);
        assert!(l.surface.faceted(), "{:?}", l.surface);
        let r = 5000.0;
        for k in &l.surface.knots {
            assert!((k.1.length() - r).abs() < 1e-6, "on the arc: {k:?}");
        }
        let whole: Vec<f64> = l.surface.knots.windows(2).map(|p| p[0].1.dist(p[1].1)).collect();
        // All but the last are the segment; the last takes what's left.
        assert!(whole[..whole.len() - 1].iter().all(|c| (c - 1500.0).abs() < 1e-3), "{whole:?}");
        assert!(*whole.last().unwrap_or(&0.0) <= 1500.0 + 1e-3);
        // Its run is those chords' length, a little less than the arc's.
        assert!((l.surface.length() - whole.iter().sum::<f64>()).abs() < 1e-3);
        assert!(l.surface.length() < PI * r);
        // A panel's centre is on its chord, at its offset from the grid plane: as far from the chord's line as those two.
        let panel = l.parts.iter().find(|p| l.shapes[p.shape as usize].kind == super::super::PartKind::Panel).unwrap_or_else(|| panic!());
        let c = l.surface.at(panel.centre[0], panel.centre[1], panel.centre[2]);
        let ((_, a), (_, b)) = l.surface.chord(panel.centre[0]).unwrap_or_else(|| panic!("no chord"));
        let off = (P2(c[0], c[1]) - a).dot((b - a).unit().perp());
        assert!((off - (l.surface.plane + panel.centre[2])).abs() < 1e-6, "{off} {c:?}");
    }

    #[test]
    fn a_slanted_curtain_walls_rows_run_up_its_slope() {
        let s = session(
            r#"type {"category": "wall", "name": "CW", "curtain": {"columns": {"segments": [1500]}, "rows": {"segments": [1000]}}}
               wall {"points": [[0, 0], [6000, 0]], "type": "CW", "height": 3000, "slant": 30}"#,
        );
        let w = s.model.wall(100002).unwrap_or_else(|| panic!("no wall"));
        let l = super::super::layout(&s.model, w);
        let face = 3000.0 / 30f64.to_radians().cos();
        // 3464 mm up the slope: three whole rows and a cut one.
        assert_eq!(l.rows.len(), 4);
        assert!((l.rows.last().map_or(0.0, |r| r.to) - face).abs() < 1e-6);
        // Its top row's top is at the wall's top, leant toward its exterior (north).
        let (top, foot) = (l.surface.at(3000.0, face, 0.0), l.surface.at(3000.0, 0.0, 0.0));
        assert!((top[2] - 3000.0).abs() < 1e-6 && (top[1] - foot[1] - 3000.0 * 30f64.to_radians().tan()).abs() < 1e-6, "{top:?}");
        // A ray at the face meets it where it was aimed.
        let aim = l.surface.at(2000.0, 1500.0, 0.0);
        let hit = l.surface.hit([aim[0], aim[1] - 5000.0, aim[2]], aim).unwrap_or_else(|| panic!("a hit"));
        assert!((hit.0 - 2000.0).abs() < 1e-6 && (hit.1 - 1500.0).abs() < 1e-6, "{hit:?}");
    }
}
