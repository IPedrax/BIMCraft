//! A wall's edited profile (Revit's Edit Profile): its shape in elevation in place of the rectangle from its base to its top,
//! loops of `[along, up]` mm in the wall's own frame, the first its outline and the rest holes. A straight upright wall only
//! (`Model::check`), so `along` is a straight line in plan and any edge of the profile is a plane over it.
//!
//! The wall is cut along it at each corner of the profile; between two cuts, each stretch the profile covers there runs from
//! one of its edges up to another, a plane each. Those make the wall's prisms as its base and top do an unedited wall's, its
//! caps and joins as ever, its doors' and windows' holes cut out of them.

use super::{Axis, Plane};
use crate::geom::{self, P2};
use crate::model::{Model, Wall};

/// An edge of a profile, as a line `up = z + slope · along`.
#[derive(Clone, Copy, Debug)]
struct Edge {
    z: f64,
    slope: f64,
}

impl Edge {
    fn at(&self, s: f64) -> f64 {
        self.z + self.slope * s
    }
}

/// Where along the wall its profile turns: every corner's `along`, in order, those within a micron as one.
pub(crate) fn stations(w: &Wall) -> Vec<f64> {
    let mut out: Vec<f64> = w.profile.iter().flatten().map(|q| q.0).filter(|s| s.is_finite()).collect();
    out.sort_by(f64::total_cmp);
    out.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
    out
}

/// The stretches up the wall the profile covers at `s` along it: each from an edge to the one over it, as heights above the
/// wall's base along the cut it's in. Even-odd, so a hole leaves a gap.
fn edges_at(w: &Wall, s: f64) -> Vec<(Edge, Edge)> {
    let mut crossing: Vec<Edge> = Vec::new();
    for ring in &w.profile {
        for (a, b) in geom::ring_edges(ring) {
            let (lo, hi) = if a.0 <= b.0 { (a, b) } else { (b, a) };
            // Half-open, so a corner on the line counts once; an upright edge never.
            if !(lo.0 <= s && s < hi.0) || (hi.0 - lo.0).abs() < 1e-9 {
                continue;
            }
            let slope = (hi.1 - lo.1) / (hi.0 - lo.0);
            crossing.push(Edge { z: lo.1 - slope * lo.0, slope });
        }
    }
    crossing.sort_by(|a, b| a.at(s).total_cmp(&b.at(s)));
    crossing.as_chunks::<2>().0.iter().map(|[a, b]| (*a, *b)).collect()
}

/// The planes over the plan between which wall `w` stands at `s` along it: each stretch its profile covers there, its bottom
/// and its top, as heights in the model.
pub(crate) fn planes_at(m: &Model, w: &Wall, s: f64) -> Vec<(Plane, Plane)> {
    let axis = Axis::of(w);
    let u = axis.tangent(0.0);
    let base = m.own_base(w);
    // `along` of a point in plan is (q - start) · u: an edge's height there is a plane.
    let plane = |e: Edge| Plane { z: base + e.z - e.slope * w.start.dot(u), g: u * e.slope };
    edges_at(w, s).into_iter().map(|(b, t)| (plane(b), plane(t))).collect()
}

/// The lowest and the highest the profile reaches above the wall's base.
pub(crate) fn extent(w: &Wall) -> Option<(f64, f64)> {
    let ring = w.profile.first()?;
    let lo = ring.iter().map(|q| q.1).fold(f64::INFINITY, f64::min);
    let hi = ring.iter().map(|q| q.1).fold(f64::NEG_INFINITY, f64::max);
    (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
}

/// Its top at `s` along it, above its base: the highest the profile reaches there.
pub(crate) fn top_at(w: &Wall, s: f64) -> Option<f64> {
    // Just inside its ends, where the half-open edges would leave nothing.
    let len = w.start.dist(w.end);
    let s = s.clamp(1e-6, (len - 1e-6).max(1e-6));
    edges_at(w, s).last().map(|(_, t)| t.at(s))
}

/// The stretches along the wall the profile covers at height `up` above its base: where a plan cutting there cuts it.
pub(crate) fn spans_at_height(w: &Wall, up: f64) -> Vec<(f64, f64)> {
    let turned: Vec<Vec<P2>> = w.profile.iter().map(|r| r.iter().map(|q| P2(q.1, q.0)).collect()).collect();
    let mut along: Vec<f64> = Vec::new();
    for ring in &turned {
        along.extend(crate::view3d::crossings(ring, P2(up, 0.0), P2(0.0, 1.0)).into_iter().flat_map(|(a, b)| [a, b]));
    }
    along.sort_by(f64::total_cmp);
    along.as_chunks::<2>().0.iter().map(|[a, b]| (*a, *b)).filter(|(a, b)| b - a > 1e-6).collect()
}

/// `ring`, of wall `w` in plan, cut to where its profile reaches height `up` above its base: a part for each stretch, its caps
/// kept where a stretch runs to an end. Unedited, the ring itself.
pub(crate) fn clip(w: &Wall, up: f64, ring: &[P2]) -> Vec<Vec<P2>> {
    if w.profile.is_empty() {
        return vec![ring.to_vec()];
    }
    let len = w.start.dist(w.end);
    let u = (w.end - w.start).unit();
    let along = |q: P2| (q - w.start).dot(u);
    spans_at_height(w, up)
        .into_iter()
        .map(|(a, b)| {
            let r = if a > 1e-3 { geom::clip(ring, |q| along(q) - a) } else { ring.to_vec() };
            if b < len - 1e-3 { geom::clip(&r, |q| b - along(q)) } else { r }
        })
        .filter(|r| r.len() >= 3)
        .collect()
}

/// `ring` (in plan) from `bottom` up to `top`, less the level gaps `gaps` (`[from, to]`, in order): the parts, each a ring and its
/// bottom and top plane, only where it has some height. A gap over the top or under the bottom somewhere cuts nothing there.
pub(crate) fn stack(ring: Vec<P2>, bottom: Plane, top: Plane, gaps: &[(f64, f64)]) -> Vec<(Vec<P2>, Plane, Plane)> {
    let mut parts = vec![(ring, bottom, top)];
    for &(g0, g1) in gaps {
        let mut next = Vec::new();
        for (r, b, t) in parts {
            // Under the gap: up to its foot, or to the top where that's lower.
            let under = geom::clip(&r, |q| g0 - b.at(q));
            next.extend(split(under, t, g0, true).into_iter().map(|(r, t)| (r, b, t)));
            // Over it: from its head, or from the bottom where that's higher.
            let over = geom::clip(&r, |q| t.at(q) - g1);
            next.extend(split(over, b, g1, false).into_iter().map(|(r, b)| (r, b, t)));
        }
        parts = next;
    }
    parts
        .into_iter()
        .map(|(r, b, t)| (geom::clip(&r, |q| t.at(q) - b.at(q)), b, t))
        .filter(|(r, _, _)| r.len() >= 3 && geom::area(r).abs() > 1.0)
        .collect()
}

/// `ring` parted where plane `p` crosses the level `z`: where `p` is the nearer (under `z` with `lower`, else over it) keeping
/// `p`, elsewhere the level.
pub(super) fn split(ring: Vec<P2>, p: Plane, z: f64, lower: bool) -> Vec<(Vec<P2>, Plane)> {
    if ring.len() < 3 {
        return Vec::new();
    }
    let sign = if lower { 1.0 } else { -1.0 };
    let near = geom::clip(&ring, |q| sign * (z - p.at(q)));
    let far = geom::clip(&ring, |q| sign * (p.at(q) - z));
    [(near, p), (far, Plane::flat(z))].into_iter().filter(|(r, _)| r.len() >= 3 && geom::area(r).abs() > 1e-6).collect()
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
    fn a_gable_profile_stands_on_its_base_and_peaks_in_the_middle() {
        let mut s = session(r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000}"#);
        s.execute("modify", &serde_json::json!({"id": 100002, "profile": [[[0, 0], [6000, 0], [6000, 3000], [3000, 5000], [0, 3000]]]}))
            .unwrap_or_else(|e| panic!("{e}"));
        let w = s.model.wall(100002).unwrap_or_else(|| panic!("no wall"));
        assert_eq!(stations(w), [0.0, 3000.0, 6000.0]);
        assert_eq!(top_at(w, 3000.0).map(f64::round), Some(5000.0));
        assert_eq!(top_at(w, 1500.0).map(f64::round), Some(4000.0));
        assert_eq!((s.model.base_z(w), s.model.top_z(w)), (0.0, 5000.0));
        // Its prisms: either side of the peak, sloping up to it.
        let prisms = super::super::prisms(&s.model, 100002, true);
        assert_eq!(prisms.len(), 2, "{prisms:?}");
        for p in &prisms {
            assert!((p.top.at(P2(3000.0, 0.0)) - 5000.0).abs() < 1e-6);
        }
        // A plan at 4000 cuts only its middle: 1500 to 4500 along it.
        assert_eq!(spans_at_height(w, 4000.0), [(1500.0, 4500.0)]);
    }

    #[test]
    fn a_profile_with_a_hole_and_a_window_leaves_both_open() {
        let mut s = session(
            r#"wall {"points": [[0, 0], [8000, 0]], "height": 3000}
               window {"wall": 100002, "offset": 6000}"#,
        );
        s.execute(
            "modify",
            &serde_json::json!({"id": 100002, "profile": [[[0, 0], [8000, 0], [8000, 4000], [0, 4000]], [[1000, 1000], [2000, 1000], [2000, 2000], [1000, 2000]]]}),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let prisms = super::super::prisms(&s.model, 100002, true);
        let volume: f64 = prisms.iter().map(|p| geom::area(&p.ring).abs() * (p.top.at(p.ring[0]) - p.bottom.at(p.ring[0]))).sum();
        // 8 m by 4 m by 200 mm, less the 1 m square hole and the 1200 x 1500 window.
        let whole = 8000.0 * 4000.0 * 200.0;
        let expected = whole - 1000.0 * 1000.0 * 200.0 - 1200.0 * 1500.0 * 200.0;
        assert!((volume - expected).abs() < 1.0, "{volume} {expected}");
    }

    #[test]
    fn a_profile_must_fit_its_wall() {
        let mut s = session(r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000}"#);
        let short = serde_json::json!({"id": 100002, "profile": [[[0, 0], [5000, 0], [5000, 3000], [0, 3000]]]});
        let e = s.execute("modify", &short).err().unwrap_or_default();
        assert!(e.contains("both its ends"), "{e}");
        let raised = serde_json::json!({"id": 100002, "profile": [[[0, 500], [6000, 500], [6000, 3000], [0, 3000]]]});
        let e = s.execute("modify", &raised).err().unwrap_or_default();
        assert!(e.contains("base"), "{e}");
        let mut c = session(r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000, "arc": 90}"#);
        let e = c.execute("modify", &serde_json::json!({"id": 100002, "profile": [[[0, 0], [6000, 0], [6000, 3000], [0, 3000]]]}));
        assert!(e.is_err());
    }
}
