//! A slanted wall's solid (Revit's Cross-Section Slanted). Its faces lean `slant` degrees from upright about its location line at
//! its base, so in plan at each height it is the outline it would have upright with its faces moved over ([`super::faces_at`]),
//! its ends mitred with what it joins there ([`super::cap_with`]). Its solid is pieces lofted between those outlines: cut along
//! it as an upright wall's prisms are (an arc's chords, its doors' and windows' jambs) and up it where an opening, or a wall it
//! joins, starts or stops. Each piece's bottom and top follow its base and top, what they are attached to included, at the
//! point each corner has leant to. Straight, a piece's faces are flat; curved, it is a cone's, in chords.
//!
//! An upright wall joined at an end to a slanted one is lofted the same way ([`lofted`]): its end follows the slanted wall's face
//! up the wall, as Revit's does; an edited profile's too, each stretch between its edges' planes.

use super::{Axis, Bound, Hole, Index, Plane, arms, bounds, cap_heights, cap_with, faces_at, hole, openings_of, profile};
use crate::geom::{self, P2};
use crate::model::{Element, Id, Model};

/// A point in space, mm.
type P3 = [f64; 3];

/// A closed piece of the wall: its triangles, each counter-clockwise seen from outside.
pub(crate) type Piece = Vec<[P3; 3]>;

/// How long a piece along a wall attached to a roof or a floor may be, mm: its top follows theirs in straight steps.
const STEP: f64 = 500.0;

/// Whether wall `id` is lofted rather than a stack of prisms: it leans, or a wall it joins at an end does, so its ends move as
/// it rises.
pub(crate) fn lofted(m: &Model, id: Id, index: Option<&Index>) -> bool {
    let Some(w) = m.wall(id) else { return false };
    if w.slant != 0.0 && w.slant.is_finite() {
        return true;
    }
    if !w.join || w.panel_of.is_some() || crate::curtain::pattern(m, w).is_some() {
        return false;
    }
    [false, true].into_iter().any(|at_end| arms(m, id, at_end, index, &|_| true, true, None).0.iter().any(|a| a.id != id && a.leans))
}

/// Wall `id`'s solid, lofted: closed pieces, round its doors and windows (with `openings`), but those whose families cut it with
/// voids of their own, which the voids cut (as [`super::prisms`] leaves them). Nothing for a wall that isn't ([`lofted`]): its
/// prisms are its solid.
pub(crate) fn pieces(m: &Model, id: Id, openings: bool, index: Option<&Index>) -> Vec<Piece> {
    pieces_holed(m, id, openings, true, index)
}

/// [`pieces`] with every opening its rectangle, voids or none: what stands where cutting by the voids fails.
pub(crate) fn pieces_rect(m: &Model, id: Id, index: Option<&Index>) -> Vec<Piece> {
    pieces_holed(m, id, true, false, index)
}

fn pieces_holed(m: &Model, id: Id, openings: bool, voided: bool, index: Option<&Index>) -> Vec<Piece> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    if !lofted(m, id, index) {
        return Vec::new();
    }
    let guid = m.guid(id);
    let axis = Axis::of(w);
    let len = axis.length();
    let holes: Vec<Hole> = if openings {
        openings_of(m, id, index)
            .filter_map(|e| match e {
                Element::Door(o) | Element::Window(o) if o.host == guid => {
                    if voided && !m.opening_voids_local(e).is_empty() {
                        return None;
                    }
                    hole(m, w, e, o)
                }
                _ => None,
            })
            .collect()
    } else {
        Vec::new()
    };
    let (base, top) = bounds(m, w);
    let attached = w.attach_base.is_some() || w.attach_top.is_some();
    let mut cuts = axis.stations(0.0, len);
    if attached {
        let n = (len / STEP).ceil().clamp(1.0, 400.0) as usize;
        cuts.extend((1..n).map(|i| len * i as f64 / n as f64));
    }
    cuts.extend(holes.iter().flat_map(|h| [h.s0, h.s1]).filter(|s| *s > 0.0 && *s < len));
    // An edited profile turns at its corners.
    cuts.extend(profile::stations(w).into_iter().filter(|s| *s > 0.0 && *s < len));
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let bands = cap_heights(m, id, index);
    let mut out = Vec::new();
    for pair in cuts.windows(2) {
        let &[a, b] = pair else { continue };
        // The piece's outline in plan at height `z`, joined to what stands at `standing`.
        let ring = |z: f64, standing: f64| {
            let (ext, int) = faces_at(m, w, Some(z));
            let mut r = if a <= 0.0 { cap_with(m, id, false, index, z, standing) } else { vec![axis.at(a, int), axis.at(a, ext)] };
            r.extend(if b >= len { cap_with(m, id, true, index, z, standing) } else { vec![axis.at(b, ext), axis.at(b, int)] });
            r.dedup_by(|p, q| p.dist(*q) < 1e-6);
            if r.len() > 2 && r.first().zip(r.last()).is_some_and(|(p, q)| p.dist(*q) < 1e-6) {
                r.pop();
            }
            r
        };
        // Its stretches up: from its base to its top, less the openings over its middle, and parted where a wall it joins starts
        // or stops (at its ends only).
        let mid = (a + b) / 2.0;
        let mut gaps: Vec<(f64, f64)> = holes.iter().filter(|h| h.s0 <= mid && mid <= h.s1).map(|h| (h.bottom, h.top)).collect();
        if a <= 0.0 || b >= len {
            let open = |z: f64| gaps.iter().any(|(g0, g1)| *g0 <= z && z <= *g1);
            let edges: Vec<f64> = bands.iter().copied().filter(|z| !open(*z)).collect();
            gaps.extend(edges.into_iter().map(|z| (z, z)));
        }
        gaps.sort_by(|x, y| x.0.total_cmp(&y.0));
        // From its base to its top; an edited profile's, each stretch it covers here, from one edge's plane to another's.
        let stretches: Vec<(Level, Level)> = if w.profile.is_empty() {
            vec![(Level::Bound(&base), Level::Bound(&top))]
        } else {
            profile::planes_at(m, w, mid).into_iter().map(|(b, t)| (Level::Plane(b), Level::Plane(t))).collect()
        };
        for &(low, high) in &stretches {
            let mut floors = vec![low];
            let mut ceilings = Vec::new();
            for &(g0, g1) in &gaps {
                ceilings.push(Level::Flat(g0));
                floors.push(Level::Flat(g1));
            }
            ceilings.push(high);
            for (under, over) in floors.into_iter().zip(ceilings) {
                if let Some(piece) = loft(&ring, (&under, &over), (&low, &high), m.own_base(w)) {
                    out.push(piece);
                }
            }
        }
    }
    out
}

/// A piece's bottom or top: a level, what the wall's base or top is (attached, under a roof's slopes), or an edge of its profile.
#[derive(Clone, Copy)]
enum Level<'a> {
    Flat(f64),
    Bound(&'a Bound),
    Plane(Plane),
}

impl Level<'_> {
    /// Its height where corner `i` of `ring` stands there: the corner leans as it rises, so the two are found together.
    fn at(&self, ring: &impl Fn(f64, f64) -> Vec<P2>, i: usize, guess: f64, standing: f64) -> Option<f64> {
        match self {
            Level::Flat(z) => Some(*z),
            Level::Bound(_) | Level::Plane(_) => {
                let mut z = guess;
                for _ in 0..8 {
                    let q = *ring(z, standing).get(i)?;
                    let next = self.rough(q);
                    if !next.is_finite() {
                        return None;
                    }
                    if (next - z).abs() < 1e-6 {
                        return Some(next);
                    }
                    z = next;
                }
                Some(z)
            }
        }
    }

    /// Its height at the middle of the piece, roughly: where it parts the wall.
    fn rough(&self, q: P2) -> f64 {
        match self {
            Level::Flat(z) => *z,
            Level::Bound(b) => b.at(q),
            Level::Plane(p) => p.at(q),
        }
    }
}

/// The piece from `under` up to `over` of the outline `ring` gives at each height: each corner from where it leans to at the one
/// to where it leans to at the other, never below the wall's own bottom there nor over its top (`within`: where a roof it follows
/// or its profile's edge dips under an opening's head, that is the top). None where it has no height, or its outline changes shape
/// as it rises.
fn loft(ring: &impl Fn(f64, f64) -> Vec<P2>, (under, over): (&Level, &Level), within: (&Level, &Level), base: f64) -> Option<Piece> {
    let first = ring(base, base);
    let centre = first.iter().fold(P2(0.0, 0.0), |s, q| s + *q) * (1.0 / first.len().max(1) as f64);
    let (lo, hi) = (under.rough(centre), over.rough(centre));
    if !(lo.is_finite() && hi.is_finite()) || hi - lo < 1e-6 {
        return None;
    }
    let standing = (lo + hi) / 2.0;
    let n = ring(standing, standing).len();
    if n < 3 {
        return None;
    }
    let mut bottom = Vec::with_capacity(n);
    let mut top = Vec::with_capacity(n);
    for i in 0..n {
        let z0 = under.at(ring, i, lo, standing)?.max(within.0.at(ring, i, lo, standing)?);
        let z1 = over.at(ring, i, hi, standing)?.min(within.1.at(ring, i, hi, standing)?).max(z0);
        let (r0, r1) = (ring(z0, standing), ring(z1, standing));
        if r0.len() != n || r1.len() != n {
            return None;
        }
        bottom.push([r0[i].0, r0[i].1, z0]);
        top.push([r1[i].0, r1[i].1, z1]);
    }
    if bottom.iter().zip(&top).all(|(b, t)| t[2] - b[2] < 1e-6) {
        return None;
    }
    Some(closed(bottom, top))
}

/// The closed solid between `bottom` and `top`, corner for corner: its two ends, and a side between each two corners.
pub(crate) fn closed(mut bottom: Vec<P3>, mut top: Vec<P3>) -> Piece {
    let plan = |r: &[P3]| r.iter().map(|p| P2(p[0], p[1])).collect::<Vec<P2>>();
    // Counter-clockwise in plan, so its sides face out.
    if geom::area(&plan(&bottom)) < 0.0 {
        bottom.reverse();
        top.reverse();
    }
    let n = bottom.len();
    let mut tris = Vec::with_capacity(4 * n);
    for t in geom::triangulate(&plan(&bottom)) {
        let [a, b, c] = t.map(|i| bottom.get(i).copied().unwrap_or_default());
        tris.push([a, c, b]);
    }
    for t in geom::triangulate(&plan(&top)) {
        tris.push(t.map(|i| top.get(i).copied().unwrap_or_default()));
    }
    for i in 0..n {
        let j = (i + 1) % n;
        let (b0, b1, t0, t1) = (bottom[i], bottom[j], top[i], top[j]);
        // A side of no height (a top flush with the base there) is left out, as is one of no length.
        if b0 != t0 || b1 != t1 {
            tris.push([b0, b1, t1]);
            tris.push([b0, t1, t0]);
        }
    }
    tris.retain(|[a, b, c]| {
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        n.iter().map(|x| x * x).sum::<f64>() > 1e-12
    });
    tris
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

    fn extent(pieces: &[Piece]) -> (P3, P3) {
        pieces.iter().flatten().flatten().fold(([f64::MAX; 3], [f64::MIN; 3]), |(lo, hi), p| {
            (std::array::from_fn(|k| lo[k].min(p[k])), std::array::from_fn(|k| hi[k].max(p[k])))
        })
    }

    /// The volume a closed mesh holds, by the divergence theorem.
    fn volume(tris: &[[P3; 3]]) -> f64 {
        tris.iter()
            .map(|[a, b, c]| {
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0
            })
            .sum()
    }

    #[test]
    fn a_slanted_wall_leans_toward_its_exterior_and_keeps_its_volume() {
        // East along y = 0, so its exterior is north (its left): leaning 30°, 3000 high, its top is 1732 mm north.
        let s = session(r#"wall {"points": [[0, 0], [4000, 0]], "height": 3000, "slant": 30}"#);
        let pieces = pieces(&s.model, 100002, true, None);
        assert_eq!(pieces.len(), 1);
        let (lo, hi) = extent(&pieces);
        let lean = 3000.0 * 30f64.to_radians().tan();
        let wide = 200.0 / 30f64.to_radians().cos();
        assert!((lo[1] + wide / 2.0).abs() < 1e-6 && (hi[1] - (lean + wide / 2.0)).abs() < 1e-6, "{lo:?} {hi:?}");
        assert!((lo[2]).abs() < 1e-9 && (hi[2] - 3000.0).abs() < 1e-9);
        // A parallelogram across it: as much as the upright wall of the same thickness along the slope.
        let v = volume(&pieces[0]);
        assert!((v - 4000.0 * 3000.0 * wide).abs() < 1.0, "{v}");
        // Upright, it has no pieces of its own: it is prisms.
        let upright = session(r#"wall {"points": [[0, 0], [4000, 0]], "height": 3000}"#);
        assert!(super::pieces(&upright.model, 100002, true, None).is_empty());
    }

    #[test]
    fn a_window_cuts_a_slanted_wall_through_its_lean() {
        let s = session(
            r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000, "slant": -20}
               window {"wall": 100002, "offset": 3000}"#,
        );
        let pieces = pieces(&s.model, 100002, true, None);
        // Either side of it, under it and over it.
        assert_eq!(pieces.len(), 4, "{}", pieces.len());
        let whole = super::pieces(&s.model, 100002, false, None);
        let less: f64 = pieces.iter().map(|p| volume(p)).sum();
        assert!(less < volume(&whole[0]) - 1e5);
    }

    #[test]
    fn slanted_walls_meet_in_a_mitre_at_every_height() {
        // Two walls leaning out at a square corner: their faces meet on the corner's diagonal at the base and at the top.
        let s = session(
            r#"wall {"points": [[4000, 0], [0, 0]], "height": 3000, "slant": 15}
               wall {"points": [[0, 0], [0, 4000]], "height": 3000, "slant": 15}"#,
        );
        for id in [100002, 100003] {
            let p = pieces(&s.model, id, true, None);
            let corner: Vec<&P3> = p.iter().flatten().flatten().filter(|q| (q[0] - q[1]).abs() < 1e-6 && q[0] < 0.0).collect();
            assert!(!corner.is_empty(), "{id}: its mitre lies on the diagonal");
            assert!(corner.iter().any(|q| (q[2] - 3000.0).abs() < 1e-9), "{id}: up to its top");
        }
    }

    #[test]
    fn a_curved_slanted_wall_attached_top_and_bottom_is_a_cone_between_them() {
        let mut s = session(r#"wall {"points": [[-3000, 0], [3000, 0]], "arc": 180, "height": 3000, "slant": 10}"#);
        let wall = 100002;
        let square = serde_json::json!([[-5000, -5000], [5000, -5000], [5000, 5000], [-5000, 5000]]);
        let mut slab = |offset: f64| {
            let made = s.execute("floor", &serde_json::json!({"boundary": square, "offset": offset})).unwrap_or_else(|e| panic!("{e}"));
            made["id"].as_u64().unwrap_or_else(|| panic!("{made}"))
        };
        let (under, over) = (slab(300.0), slab(2600.0));
        s.execute("modify", &serde_json::json!({"id": wall, "attach_base": under, "attach_top": over})).unwrap_or_else(|e| panic!("{e}"));
        let p = pieces(&s.model, wall, true, None);
        assert!(p.len() > 30, "chords along the arc: {}", p.len());
        let (lo, hi) = extent(&p);
        // On the lower slab's top, under the upper one's underside.
        assert!((lo[2] - 300.0).abs() < 1e-6 && hi[2] < 2600.0 - 1.0 && hi[2] > 2000.0, "{lo:?} {hi:?}");
        // Its exterior is to its left, toward the arc's centre: leaning in, its top is a smaller circle.
        let r = |q: &&P3| (q[0] * q[0] + q[1] * q[1]).sqrt();
        let top: Vec<f64> = p.iter().flatten().flatten().filter(|q| (q[2] - hi[2]).abs() < 1e-6).map(|q| r(&q)).collect();
        let bottom: Vec<f64> = p.iter().flatten().flatten().filter(|q| (q[2] - lo[2]).abs() < 1e-6).map(|q| r(&q)).collect();
        let max = |v: &[f64]| v.iter().copied().fold(f64::MIN, f64::max);
        assert!(max(&top) < max(&bottom) - 300.0, "{} {}", max(&top), max(&bottom));
    }
}
