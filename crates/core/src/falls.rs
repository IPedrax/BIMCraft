//! Falls to drains (MOD-16 phase 5, [shape editing](../../../docs/shape-editing.md)): a floor's or a flat roof's shape worked
//! out from where its drains are and how steeply it must fall, as an architect describes a terrace, not point by point.
//!
//! Ridges drawn split the top into parts, each with its drains. In a part, each drain takes what is nearer it than any other
//! (its catchment, the part cut by the lines halfway between drains), less the openings in it. Where two catchments meet a
//! ridge runs at the top's height, and the outline stays there too. From each drain a valley runs to each corner of its
//! catchment, straight where it sees it, else round the openings' corners by the shortest way; an opening's corner lies on
//! the cone that falls from the drain at `fall`, so water reaching it runs on down the valley. Each drain goes down as far
//! as `fall` takes its farthest corner. The command then checks every plane falls at least `fall` toward a drain or a valley.

use crate::geom::{self, P2};
use crate::model::TOL;

/// The most drains one top takes.
pub const MAX_DRAINS: usize = 200;

/// The most ridges one top takes.
pub const MAX_RIDGES: usize = 200;

/// The steepest fall asked, %.
pub const MAX_FALL: f64 = 100.0;

/// A top's falls: each drain with how far it lies under the top (negative, mm, to the tenth down); the points a ridge bends
/// or ends at (at the top's height, 0) and the openings' corners on their drains' cones (under it); and the split lines along
/// the ridges and the valleys.
#[derive(Debug, PartialEq)]
pub struct Falls {
    pub drains: Vec<(P2, f64)>,
    pub points: Vec<(P2, f64)>,
    pub lines: Vec<(P2, P2)>,
}

/// The falls over `outline` with `holes` (its openings) to `drains`, at no less than `fall` %, along `ridges`. Refused, saying
/// why: no drain, too many, one outside the outline or in an opening or within a millimetre of another; a fall not over 0
/// or past `MAX_FALL`; a ridge that divides nothing, or a part of the top between ridges with no drain; a corner its drain
/// can reach only over the outline or a ridge (a drain there too, then).
pub fn falls(outline: &[P2], holes: &[Vec<P2>], drains: &[P2], fall: f64, ridges: &[(P2, P2)]) -> Result<Falls, String> {
    if drains.is_empty() {
        return Err("give at least one drain".into());
    }
    if drains.len() > MAX_DRAINS {
        return Err(format!("at most {MAX_DRAINS} drains"));
    }
    if ridges.len() > MAX_RIDGES {
        return Err(format!("at most {MAX_RIDGES} ridges"));
    }
    if !(fall > 0.0 && fall <= MAX_FALL) {
        return Err(format!("`fall` is a percentage over 0 and at most {MAX_FALL}, not {fall}"));
    }
    let outline = ccw(outline);
    let holes: Vec<Vec<P2>> = holes.iter().map(|h| ccw(h)).collect();
    let on = |r: &[P2], q: P2| geom::ring_edges(r).any(|(a, b)| geom::dist_to_segment(q, a, b) <= TOL);
    let in_hole = |q: P2| holes.iter().any(|h| geom::contains(h, q) && !on(h, q));
    for (k, &d) in drains.iter().enumerate() {
        if !geom::contains(&outline, d) || on(&outline, d) {
            return Err(format!("the drain at {} is outside the outline", at(d)));
        }
        if holes.iter().any(|h| geom::contains(h, d) || on(h, d)) {
            return Err(format!("the drain at {} is in an opening", at(d)));
        }
        if let Some(e) = drains.iter().skip(k + 1).find(|e| e.dist(d) <= TOL) {
            return Err(format!("the drains at {} and {} are within a millimetre of each other: one is enough", at(d), at(*e)));
        }
    }
    // The parts the ridges make: each must end on the outline, an opening or another ridge.
    let mut edges: Vec<(P2, P2)> = std::iter::once(&outline).chain(&holes).flat_map(|r| geom::ring_edges(r)).collect();
    for (k, &(a, b)) in ridges.iter().enumerate() {
        let ends = |q: P2| {
            edges.iter().any(|&(c, d)| geom::dist_to_segment(q, c, d) <= TOL)
                || ridges.iter().enumerate().any(|(j, &(c, d))| j != k && geom::dist_to_segment(q, c, d) <= TOL)
        };
        if a.dist(b) <= TOL || !ends(a) || !ends(b) {
            return Err(format!(
                "the ridge from {} to {} doesn't divide the top: run it to the outline, an opening or another ridge",
                at(a),
                at(b)
            ));
        }
        if !geom::contains(&outline, (a + b) * 0.5) || in_hole((a + b) * 0.5) {
            return Err(format!("the ridge from {} to {} runs off the top", at(a), at(b)));
        }
    }
    edges.extend(ridges.iter().copied());
    let parts: Vec<Vec<P2>> = if ridges.is_empty() {
        vec![outline.clone()]
    } else {
        geom::faces(&edges, TOL)
            .into_iter()
            .filter(|f| geom::area(f) > 1.0)
            .filter(|f| inside(f).is_some_and(|q| geom::contains(&outline, q) && !in_hole(q)))
            .collect()
    };
    let mut out = Falls { drains: Vec::new(), points: Vec::new(), lines: Vec::new() };
    for (a, b) in ridges {
        push_line(&mut out.lines, *a, *b);
    }
    let corner = |q: P2| outline.iter().chain(holes.iter().flatten()).any(|c| c.dist(q) <= TOL);
    let hole_corner = |q: P2| holes.iter().flatten().any(|c| c.dist(q) <= TOL);
    for part in &parts {
        let mine: Vec<P2> = drains.iter().copied().filter(|d| geom::contains(part, *d) && !on(part, *d)).collect();
        if mine.is_empty() {
            let q = inside(part).unwrap_or_default();
            return Err(format!("the part of the top between ridges round {} has no drain: give it one", at(q)));
        }
        for &d in &mine {
            // Its catchment: the part where it's the nearest drain, the halfway lines cutting it.
            let mut cell = part.clone();
            for &e in mine.iter().filter(|e| e.dist(d) > TOL) {
                cell = geom::clip(&cell, |p| (p - e).dot(p - e) - (p - d).dot(p - d));
            }
            let cell = distinct(&cell);
            let on_cell = |q: P2| on(&cell, q);
            // Its corners less the openings: the catchment's outside any opening, the openings' inside it, and where its
            // sides cross an opening's.
            let mut corners: Vec<P2> = cell.iter().copied().filter(|q| !in_hole(*q)).collect();
            corners.extend(holes.iter().flatten().copied().filter(|q| geom::contains(&cell, *q) || on_cell(*q)));
            for (a, b) in geom::ring_edges(&cell) {
                for h in &holes {
                    for (c, e) in geom::ring_edges(h) {
                        if let Some(t) = geom::crossing(a, b, c, e) {
                            corners.push(a + (b - a) * t);
                        }
                    }
                }
            }
            let corners = distinct_all(&corners);
            // What blocks a valley: the outline, the openings, the ridges, and the catchment's own sides.
            let mut walls = edges.clone();
            walls.extend(geom::ring_edges(&cell));
            let open = |p: P2, q: P2| {
                let mid = (p + q) * 0.5;
                !walls.iter().any(|&(a, b)| crosses(p, q, a, b))
                    && (geom::contains(&cell, mid) || on_cell(mid))
                    && (geom::contains(&outline, mid) || on(&outline, mid))
                    && !in_hole(mid)
            };
            // The shortest way from the drain to each corner, round the openings' corners (Dijkstra over what sees what).
            let n = corners.len();
            let (mut dist, mut via, mut done) = (vec![f64::INFINITY; n], vec![None::<usize>; n], vec![false; n]);
            for (i, &q) in corners.iter().enumerate() {
                if open(d, q) {
                    dist[i] = d.dist(q);
                }
            }
            for _ in 0..n {
                let Some(i) = (0..n).filter(|i| !done[*i] && dist[*i].is_finite()).min_by(|a, b| dist[*a].total_cmp(&dist[*b])) else {
                    break;
                };
                done[i] = true;
                // Only an opening's corner is a way on: past one at the top's height the water would have to climb.
                if !hole_corner(corners[i]) {
                    continue;
                }
                for j in 0..n {
                    let through = dist[i] + corners[i].dist(corners[j]);
                    if !done[j] && through < dist[j] - 1e-9 && open(corners[i], corners[j]) {
                        (dist[j], via[j]) = (through, Some(i));
                    }
                }
            }
            if let Some(i) = (0..n).find(|i| !dist[*i].is_finite()) {
                return Err(format!(
                    "the drain at {} can't see the corner at {} across the outline or a ridge: add a drain on that side",
                    at(d),
                    at(corners[i])
                ));
            }
            // As deep as the fall takes its farthest corner at the top's height.
            let rate = fall / 100.0;
            let far = (0..n).filter(|i| !hole_corner(corners[*i])).map(|i| dist[i]).fold(0.0, f64::max);
            let depth = (rate * far * 10.0).ceil() / 10.0;
            out.drains.push((d, -depth));
            for (i, &q) in corners.iter().enumerate() {
                let from = via[i].map_or(d, |k| corners[k]);
                // A valley, unless it runs along an opening's or the catchment's side, already an edge.
                if !walls.iter().any(|&(a, b)| geom::dist_to_segment((from + q) * 0.5, a, b) <= TOL) {
                    push_line(&mut out.lines, from, q);
                }
                let z = if hole_corner(q) { ((rate * dist[i] - depth) * 10.0).ceil() / 10.0 } else { 0.0 };
                let z = z.min(0.0);
                if (hole_corner(q) || !corner(q)) && !out.points.iter().any(|(r, _)| r.dist(q) <= TOL) {
                    out.points.push((q, z));
                }
            }
            // A side of the catchment along no edge of the outline, an opening or a ridge drawn is a ridge, its parts out
            // of the openings.
            for (a, b) in geom::ring_edges(&cell) {
                if !edges.iter().any(|&(c, e)| geom::dist_to_segment((a + b) * 0.5, c, e) <= TOL) {
                    for (p, q) in outside(a, b, &holes) {
                        push_line(&mut out.lines, p, q);
                    }
                }
            }
        }
    }
    // A ridge's ends, where they're no corner, at the top's height.
    for q in ridges.iter().flat_map(|(a, b)| [*a, *b]) {
        if !corner(q) && !out.points.iter().any(|(r, _)| r.dist(q) <= TOL) {
            out.points.push((q, 0.0));
        }
    }
    Ok(out)
}

fn at(p: P2) -> String {
    format!("({:.0}, {:.0})", p.0, p.1)
}

fn ccw(ring: &[P2]) -> Vec<P2> {
    if geom::area(ring) < 0.0 { ring.iter().rev().copied().collect() } else { ring.to_vec() }
}

/// A point well inside a ring: the middle of the largest of its triangles.
fn inside(ring: &[P2]) -> Option<P2> {
    let pt = |i: usize| ring.get(i).copied();
    let size = |(a, b, c): &(P2, P2, P2)| (*b - *a).cross(*c - *a).abs();
    geom::triangulate(ring)
        .into_iter()
        .filter_map(|[a, b, c]| Some((pt(a)?, pt(b)?, pt(c)?)))
        .max_by(|x, y| size(x).total_cmp(&size(y)))
        .map(|(a, b, c)| (a + b + c) * (1.0 / 3.0))
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

/// `pts` with those within a millimetre of an earlier one taken out.
fn distinct_all(pts: &[P2]) -> Vec<P2> {
    let mut out: Vec<P2> = Vec::with_capacity(pts.len());
    for &p in pts {
        if !out.iter().any(|q| q.dist(p) <= TOL) {
            out.push(p);
        }
    }
    out
}

/// The pieces of `a`→`b` outside every one of `holes`.
fn outside(a: P2, b: P2, holes: &[Vec<P2>]) -> Vec<(P2, P2)> {
    let mut ts = vec![0.0, 1.0];
    for h in holes {
        ts.extend(geom::ring_edges(h).filter_map(|(c, d)| geom::crossing(a, b, c, d)));
    }
    ts.sort_by(f64::total_cmp);
    let p = |t: f64| a + (b - a) * t;
    ts.windows(2)
        .filter_map(|w| match *w {
            [t0, t1] if !holes.iter().any(|h| geom::contains(h, p((t0 + t1) / 2.0))) => Some((p(t0), p(t1))),
            _ => None,
        })
        .filter(|(p, q)| p.dist(*q) > TOL)
        .collect()
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
        let one = falls(&square, &[], &[P2(2000.0, 2000.0)], 2.0, &[]).unwrap();
        assert_eq!((one.lines.len(), one.points.len()), (4, 0));
        // 2828.4 mm to a corner at 2 %: 56.6 mm.
        assert_eq!(one.drains, [(P2(2000.0, 2000.0), -56.6)]);
        let two = falls(&square, &[], &[P2(1000.0, 2000.0), P2(3000.0, 2000.0)], 2.0, &[]).unwrap();
        // The ridge across the middle, its ends points; four valleys from each drain, the ridge once.
        assert_eq!(two.points.len(), 2, "{two:?}");
        assert_eq!(two.lines.len(), 9, "{two:?}");
        assert!(two.lines.iter().any(|(a, b)| a.0 == 2000.0 && b.0 == 2000.0), "{two:?}");
    }

    #[test]
    fn behind_an_opening_a_valley_runs_round_its_corners_on_the_drains_cone() {
        let square = [P2(0.0, 0.0), P2(6000.0, 0.0), P2(6000.0, 6000.0), P2(0.0, 6000.0)];
        let hole = vec![P2(3500.0, 2500.0), P2(4500.0, 2500.0), P2(4500.0, 3500.0), P2(3500.0, 3500.0)];
        let f = falls(&square, &[hole], &[P2(2000.0, 3000.0)], 1.0, &[]).unwrap();
        // The opening's near corners straight from the drain, its far ones round them: each under the top, the far lower
        // on the cone than it would be straight.
        let z = |q: P2| f.points.iter().find(|(p, _)| p.dist(q) <= TOL).map(|(_, z)| *z);
        let (near, far) = (z(P2(3500.0, 2500.0)).unwrap(), z(P2(4500.0, 2500.0)).unwrap());
        assert!(near < far && far < 0.0, "{f:?}");
        // No valley straight to the far corners: theirs runs along the opening's side.
        assert!(!f.lines.iter().any(|(a, b)| a.dist(P2(2000.0, 3000.0)) <= TOL && b.dist(P2(4500.0, 2500.0)) <= TOL), "{f:?}");
    }

    #[test]
    fn segments_that_only_touch_dont_cross() {
        let (o, x) = (P2(0.0, 0.0), P2(10.0, 0.0));
        assert!(crosses(o, P2(10.0, 10.0), x, P2(0.0, 10.0)));
        assert!(!crosses(o, x, x, P2(10.0, 10.0)));
        assert!(!crosses(o, P2(5.0, 0.0), P2(6.0, 0.0), x));
    }
}
