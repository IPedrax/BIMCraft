//! Falls to drains (MOD-16 phase 5, [shape editing](../../../docs/shape-editing.md)): a floor's or a flat roof's shape worked
//! out from where its drains are and how steeply it must fall, as an architect describes a terrace, not point by point.
//!
//! Each drain takes the part of the outline nearer it than any other (its catchment, the outline cut by the lines halfway
//! between drains). Where two catchments meet, a ridge runs at the top's height; from each drain a valley runs to each
//! corner of its catchment; the drain goes down as far as `fall` takes its farthest corner. Every plane between a drain and
//! two corners of its catchment then falls at least `fall` toward its drain, and so does every valley.

use crate::geom::{self, P2};
use crate::model::TOL;

/// The most drains one top takes.
pub const MAX_DRAINS: usize = 200;

/// The steepest fall asked, %.
pub const MAX_FALL: f64 = 100.0;

/// A top's falls: each drain with how far it lies under the top (negative, mm, to the tenth down), the points a ridge
/// bends or ends at (at the top's height, 0), and the split lines along the ridges and the valleys.
#[derive(Debug, PartialEq)]
pub struct Falls {
    pub drains: Vec<(P2, f64)>,
    pub ridges: Vec<P2>,
    pub lines: Vec<(P2, P2)>,
}

/// The falls over `outline` (with no openings) to `drains`, at no less than `fall` %. Refused, saying why: no drain, too
/// many, one outside the outline or within a millimetre of another, a fall not over 0 or past `MAX_FALL`, or a corner its
/// drain can't see across the outline (a drain there too, then).
pub fn falls(outline: &[P2], drains: &[P2], fall: f64) -> Result<Falls, String> {
    if drains.is_empty() {
        return Err("give at least one drain".into());
    }
    if drains.len() > MAX_DRAINS {
        return Err(format!("at most {MAX_DRAINS} drains"));
    }
    if !(fall > 0.0 && fall <= MAX_FALL) {
        return Err(format!("`fall` is a percentage over 0 and at most {MAX_FALL}, not {fall}"));
    }
    let at = |p: P2| format!("({:.0}, {:.0})", p.0, p.1);
    let outline = if geom::area(outline) < 0.0 { outline.iter().rev().copied().collect() } else { outline.to_vec() };
    let on_outline = |q: P2| geom::ring_edges(&outline).any(|(a, b)| geom::dist_to_segment(q, a, b) <= TOL);
    for (k, &d) in drains.iter().enumerate() {
        if !geom::contains(&outline, d) || on_outline(d) {
            return Err(format!("the drain at {} is outside the outline", at(d)));
        }
        if let Some(e) = drains.iter().skip(k + 1).find(|e| e.dist(d) <= TOL) {
            return Err(format!("the drains at {} and {} are within a millimetre of each other: one is enough", at(d), at(*e)));
        }
    }
    let corner = |q: P2| outline.iter().any(|c| c.dist(q) <= TOL);
    let mut out = Falls { drains: Vec::new(), ridges: Vec::new(), lines: Vec::new() };
    for (i, &d) in drains.iter().enumerate() {
        // Its catchment: the outline where it's the nearest drain, the halfway lines cutting it.
        let mut cell = outline.clone();
        for (j, &e) in drains.iter().enumerate() {
            if j != i {
                cell = geom::clip(&cell, |p| (p - e).dot(p - e) - (p - d).dot(p - d));
            }
        }
        let cell = distinct(&cell);
        let mut far: f64 = 0.0;
        for &v in &cell {
            if !sees(&outline, d, v) {
                return Err(format!(
                    "the drain at {} can't see the corner at {} across the outline: add a drain on that side",
                    at(d),
                    at(v)
                ));
            }
            far = far.max(d.dist(v));
            push_line(&mut out.lines, d, v);
            if !corner(v) && !out.ridges.iter().any(|r| r.dist(v) <= TOL) {
                out.ridges.push(v);
            }
        }
        // A side of the catchment along no edge of the outline is a ridge.
        for (a, b) in geom::ring_edges(&cell) {
            if !on_outline((a + b) * 0.5) {
                push_line(&mut out.lines, a, b);
            }
        }
        out.drains.push((d, -((fall / 100.0 * far * 10.0).ceil() / 10.0)));
    }
    Ok(out)
}

/// `ring` with its corners within a millimetre of the one before taken out.
fn distinct(ring: &[P2]) -> Vec<P2> {
    let mut out: Vec<P2> = Vec::with_capacity(ring.len());
    for &p in ring {
        if out.last().is_none_or(|q| q.dist(p) > TOL) {
            out.push(p);
        }
    }
    while out.len() > 1 && out.first().zip(out.last()).is_some_and(|(a, b)| a.dist(*b) <= TOL) {
        out.pop();
    }
    out
}

/// Whether the line from `d` to `v` stays over the outline: crossing none of its edges short of `v`, its middle inside.
fn sees(outline: &[P2], d: P2, v: P2) -> bool {
    let mid = (d + v) * 0.5;
    let inside = geom::contains(outline, mid) || geom::ring_edges(outline).any(|(a, b)| geom::dist_to_segment(mid, a, b) <= TOL);
    inside && !geom::ring_edges(outline).any(|(a, b)| crosses(d, v, a, b))
}

/// Whether segments `p`→`q` and `a`→`b` cross each other, away from their ends.
fn crosses(p: P2, q: P2, a: P2, b: P2) -> bool {
    let side = |o: P2, s: P2, t: P2| (s - o).cross(t - o);
    let (d1, d2, d3, d4) = (side(a, b, p), side(a, b, q), side(p, q, a), side(p, q, b));
    let eps = 1e-6 * (q - p).length().max(1.0) * (b - a).length().max(1.0);
    ((d1 > eps && d2 < -eps) || (d1 < -eps && d2 > eps)) && ((d3 > eps && d4 < -eps) || (d3 < -eps && d4 > eps))
}

/// `a`→`b` among `lines`, once either way round.
fn push_line(lines: &mut Vec<(P2, P2)>, a: P2, b: P2) {
    let same = |(c, d): &(P2, P2)| (c.dist(a) <= TOL && d.dist(b) <= TOL) || (c.dist(b) <= TOL && d.dist(a) <= TOL);
    if a.dist(b) > TOL && !lines.iter().any(same) {
        lines.push((a, b));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_drain_fans_a_valley_to_each_corner_and_two_share_a_ridge() {
        let square = [P2(0.0, 0.0), P2(4000.0, 0.0), P2(4000.0, 4000.0), P2(0.0, 4000.0)];
        let one = falls(&square, &[P2(2000.0, 2000.0)], 2.0).unwrap();
        assert_eq!((one.lines.len(), one.ridges.len()), (4, 0));
        // 2828.4 mm to a corner at 2 %: 56.6 mm.
        assert_eq!(one.drains, [(P2(2000.0, 2000.0), -56.6)]);
        let two = falls(&square, &[P2(1000.0, 2000.0), P2(3000.0, 2000.0)], 2.0).unwrap();
        // The ridge across the middle, its ends points; four valleys from each drain, the ridge once.
        assert_eq!(two.ridges.len(), 2, "{two:?}");
        assert_eq!(two.lines.len(), 9, "{two:?}");
        assert!(two.lines.iter().any(|(a, b)| a.0 == 2000.0 && b.0 == 2000.0), "{two:?}");
    }

    #[test]
    fn segments_that_only_touch_dont_cross() {
        let (o, x) = (P2(0.0, 0.0), P2(10.0, 0.0));
        assert!(crosses(o, P2(10.0, 10.0), x, P2(0.0, 10.0)));
        assert!(!crosses(o, x, x, P2(10.0, 10.0)));
        assert!(!crosses(o, P2(5.0, 0.0), P2(6.0, 0.0), x));
    }
}
