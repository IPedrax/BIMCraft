//! A rail's path and its sweep, two steps apart. The solver ([`rail`]) takes the railing's line, as its hosts give it
//! point by point, and makes the rail's centre line: the points where it really turns or changes slope kept, the rest
//! left out, and each of those bends rounded as the rail asks (a plan corner by `corner`, a change of slope by `bend`;
//! 0 leaves it sharp). The sweeper ([`sweep`]) takes any centre line and a section and makes the rail's surface, its
//! section carried along without rolling, each stretch a straight prism of its own mitred on the plane halving each bend
//! it meets sharp.

use crate::geom::P2;
use crate::model::{PartShape, RailPart};
use crate::view3d::P3;

/// How many sides a round rail has.
const SIDES: usize = 24;
/// How far the rail's line may stray from the points it was given, mm, where the solver leaves points out.
const STRAY: f64 = 0.5;
/// A rounded bend is drawn in steps of at most this angle, radians.
const ARC_STEP: f64 = 0.15;

/// A circle of radius `r`, counter-clockwise, as a section across a rail: across it, then up.
pub(crate) fn circle(r: f64) -> Vec<(f64, f64)> {
    (0..SIDES)
        .map(|k| {
            let t = std::f64::consts::TAU * k as f64 / SIDES as f64;
            (r * t.cos(), r * t.sin())
        })
        .collect()
}

/// A rectangle `w` across and `d` high, counter-clockwise, round its middle.
pub(crate) fn rectangle(w: f64, d: f64) -> Vec<(f64, f64)> {
    let (a, b) = (w / 2.0, d / 2.0);
    vec![(a, -b), (a, b), (-a, b), (-a, -b)]
}

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn mul(a: P3, k: f64) -> P3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn length(a: P3) -> f64 {
    dot(a, a).sqrt()
}

fn unit(a: P3) -> P3 {
    let l = length(a);
    if l > 1e-12 { mul(a, 1.0 / l) } else { [0.0; 3] }
}

fn plan(a: P3) -> P2 {
    P2(a[0], a[1])
}

/// The centre line of rail `r` along `line` (the railing's line where the rail stands, moved across by its offset, each
/// point with the height of what it runs along): its top `height` over the line, measured straight up; the points left
/// out where the line goes on straight, within [`STRAY`]; and each bend rounded.
/// Rail `r`'s centre lines along `line`: one ([`rail`]); or, `apart`, one for each stretch between the line's upright
/// steps, each ending there.
pub fn rails(line: &[(P2, f64)], r: &RailPart, apart: bool) -> Vec<Vec<P3>> {
    if !apart {
        return vec![rail(line, r)];
    }
    let mut out = Vec::new();
    let mut from = 0;
    for i in 0..line.len().saturating_sub(1) {
        let (Some(a), Some(b)) = (line.get(i), line.get(i + 1)) else { continue };
        if a.0.dist(b.0) <= 1.0 + 1e-9 && (a.1 - b.1).abs() > 1.0 {
            if let Some(piece) = line.get(from..=i).filter(|p| p.len() >= 2) {
                out.push(rail(piece, r));
            }
            from = i + 1;
        }
    }
    if let Some(piece) = line.get(from..).filter(|p| p.len() >= 2) {
        out.push(rail(piece, r));
    }
    out
}

pub fn rail(line: &[(P2, f64)], r: &RailPart) -> Vec<P3> {
    let d = super::rail_depth(r);
    let raw: Vec<P3> = line.iter().map(|&(p, z)| [p.0, p.1, z + r.height - d / 2.0]).collect();
    let kept = lowered(&simplify(&raw), d / 2.0);
    round(&kept, r.corner, r.bend, if r.shape == PartShape::Circle { r.width } else { r.width.max(d) })
}

/// `path` run on level, `start` before its first point and `end` past its last, the way it goes in plan there.
pub fn extended(path: &[P3], start: f64, end: f64) -> Vec<P3> {
    let mut out = path.to_vec();
    let way = |a: P3, q: Option<&P3>| q.map(|q| (plan(*q) - plan(a)).unit());
    let off = |a: P3| move |q: &&P3| plan(**q).dist(plan(a)) > 1e-6;
    if start > 0.0
        && let Some(&a) = path.first()
        && let Some(u) = way(a, path.iter().find(off(a)))
    {
        out.insert(0, [a[0] - u.0 * start, a[1] - u.1 * start, a[2]]);
    }
    if end > 0.0
        && let Some(&a) = path.last()
        && let Some(u) = way(a, path.iter().rev().find(off(a)))
    {
        out.push([a[0] - u.0 * end, a[1] - u.1 * end, a[2]]);
    }
    out
}

/// `pts`, each straight stretch lowered as far as a section `half` high each way, square to it, reaches higher than
/// `half` straight up on its slope; a point between two stretches where their lowered lines meet, along the way it goes
/// in plan (a turn in plan is level, and keeps its place).
fn lowered(pts: &[P3], half: f64) -> Vec<P3> {
    let drop = |i: usize| -> Option<f64> {
        let (a, b) = (*pts.get(i)?, *pts.get(i + 1)?);
        let run = plan(b).dist(plan(a));
        // Steeper than a stair (a gooseneck's rise): its section is not lowered, the bends either side meet it.
        let k = if run > 1e-6 { length(sub(b, a)) / run } else { f64::INFINITY };
        Some(if k > 2.0 { 0.0 } else { half * (k - 1.0) })
    };
    let grade = |i: usize| -> Option<f64> {
        let (a, b) = (*pts.get(i)?, *pts.get(i + 1)?);
        let run = plan(b).dist(plan(a));
        (run > 1e-6).then(|| (b[2] - a[2]) / run).filter(|g| g.abs() <= 2.0)
    };
    (0..pts.len())
        .filter_map(|j| {
            let p = *pts.get(j)?;
            let before = j.checked_sub(1);
            match (before.and_then(drop), drop(j)) {
                (Some(da), Some(db)) => {
                    let (Some(ga), Some(gb)) = (before.and_then(grade), grade(j)) else {
                        return Some([p[0], p[1], p[2] - da.max(db)]);
                    };
                    let (a, b) = (*pts.get(j - 1)?, *pts.get(j + 1)?);
                    let (u, v) = ((plan(p) - plan(a)).unit(), (plan(b) - plan(p)).unit());
                    if (ga - gb).abs() < 1e-9 || u.dot(v) < 0.99995 {
                        return Some([p[0], p[1], p[2] - da.max(db)]);
                    }
                    // Along the way: z = −da + ga·t and z = −db + gb·t meet at t = (da − db) / (ga − gb).
                    let reach = (plan(p).dist(plan(a)).min(plan(b).dist(plan(p)))) / 2.0;
                    let t = ((da - db) / (ga - gb)).clamp(-reach, reach);
                    let at = plan(p) + u * t;
                    Some([at.0, at.1, p[2] - da + ga * t])
                }
                (Some(x), None) | (None, Some(x)) => Some([p[0], p[1], p[2] - x]),
                (None, None) => Some(p),
            }
        })
        .collect()
}

/// `pts` with the points left out that lie within [`STRAY`] of the straight line between the points kept round them
/// (Douglas–Peucker), and those where it turns in plan always kept.
fn simplify(pts: &[P3]) -> Vec<P3> {
    let mut out: Vec<P3> = Vec::with_capacity(pts.len());
    for p in pts {
        if out.last().is_none_or(|q| length(sub(*p, *q)) > 1e-6) {
            out.push(*p);
        }
    }
    let n = out.len();
    if n < 3 {
        return out;
    }
    let mut keep = vec![false; n];
    for i in [0, n - 1] {
        if let Some(k) = keep.get_mut(i) {
            *k = true;
        }
    }
    // A turn in plan stays a point of its own.
    for i in 1..n - 1 {
        let (Some(a), Some(b), Some(c)) = (out.get(i - 1), out.get(i), out.get(i + 1)) else { continue };
        let (u, v) = ((plan(*b) - plan(*a)).unit(), (plan(*c) - plan(*b)).unit());
        if u.dot(v) < 0.99995
            && u.length() > 0.0
            && v.length() > 0.0
            && let Some(k) = keep.get_mut(i)
        {
            *k = true;
        }
    }
    let fixed: Vec<usize> = (0..n).filter(|i| keep.get(*i).copied().unwrap_or(false)).collect();
    for w in fixed.windows(2) {
        let &[a, b] = w else { continue };
        let mut stack = vec![(a, b)];
        while let Some((a, b)) = stack.pop() {
            let (Some(&pa), Some(&pb)) = (out.get(a), out.get(b)) else { continue };
            let ab = sub(pb, pa);
            let far = (a + 1..b)
                .filter_map(|i| {
                    let p = *out.get(i)?;
                    let t = if dot(ab, ab) > 1e-12 { (dot(sub(p, pa), ab) / dot(ab, ab)).clamp(0.0, 1.0) } else { 0.0 };
                    Some((i, length(sub(p, add(pa, mul(ab, t))))))
                })
                .max_by(|x, y| x.1.total_cmp(&y.1));
            if let Some((i, d)) = far
                && d > STRAY
            {
                if let Some(k) = keep.get_mut(i) {
                    *k = true;
                }
                stack.push((a, i));
                stack.push((i, b));
            }
        }
    }
    out.into_iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| p).collect()
}

/// `pts` with each bend rounded by an arc tangent to both its stretches: of radius `corner` where it turns in plan,
/// `bend` where only its slope changes (0: left sharp); smaller where its stretches are too short for it, each giving at
/// most half its length to the arc at either end. A bend sharper than the section is `thick` across is left sharp too.
fn round(pts: &[P3], corner: f64, bend: f64, thick: f64) -> Vec<P3> {
    let n = pts.len();
    if n < 3 || (corner <= 0.0 && bend <= 0.0) {
        return pts.to_vec();
    }
    let mut out = Vec::with_capacity(n * 4);
    out.extend(pts.first());
    for i in 1..n - 1 {
        let (Some(&a), Some(&b), Some(&c)) = (pts.get(i - 1), pts.get(i), pts.get(i + 1)) else { continue };
        let (u, v) = (unit(sub(b, a)), unit(sub(c, b)));
        let turns = (plan(b) - plan(a)).unit().dot((plan(c) - plan(b)).unit()) < 0.99995;
        let radius = if turns { corner } else { bend };
        let angle = dot(u, v).clamp(-1.0, 1.0).acos();
        if radius <= 0.0 || angle < 1e-3 || angle > std::f64::consts::PI - 0.05 {
            out.push(b);
            continue;
        }
        // How far back along each stretch the arc starts: its radius by the half angle's tangent, at most half of each.
        let reach = (radius * (angle / 2.0).tan()).min(length(sub(b, a)) / 2.0).min(length(sub(c, b)) / 2.0);
        let r = reach / (angle / 2.0).tan();
        if r < thick / 2.0 {
            out.push(b);
            continue;
        }
        let (start, end) = (sub(b, mul(u, reach)), add(b, mul(v, reach)));
        // The arc's middle: from the bend toward the inside, its radius over the half angle's cosine.
        let inward = unit(sub(v, u));
        let centre = add(b, mul(inward, r / (angle / 2.0).cos()));
        let (from, to) = (sub(start, centre), sub(end, centre));
        let steps = (angle / ARC_STEP).ceil().max(1.0) as usize;
        // Slerp between the two radii: each point on the arc.
        for k in 0..=steps {
            let t = k as f64 / steps as f64;
            let (wa, wb) = (((1.0 - t) * angle).sin() / angle.sin(), (t * angle).sin() / angle.sin());
            out.push(add(centre, add(mul(from, wa), mul(to, wb))));
        }
    }
    out.extend(pts.last());
    out
}

/// Each stretch of `path` (repeated points left out) as `profile` stands at its two ends: its section level across (to its
/// left) while the stretch is not upright, carried on unturned up an upright one (a gooseneck's rise), and cut at each
/// bend on the plane halving it, so the stretch is a straight prism whose faces are flat and whose edges run along it.
pub(crate) fn stretches(path: &[P3], profile: &[(f64, f64)]) -> Vec<(Vec<P3>, Vec<P3>)> {
    let mut pts: Vec<P3> = Vec::with_capacity(path.len());
    for p in path {
        if pts.last().is_none_or(|q| length(sub(*p, *q)) > 1e-6) {
            pts.push(*p);
        }
    }
    // Each stretch's way, and its section's axes: across (level, to the left) and up (square to it).
    let mut frames: Vec<(P3, P3, P3)> = Vec::with_capacity(pts.len());
    let mut across_was = [0.0, 1.0, 0.0];
    for w in pts.windows(2) {
        let d = unit(sub(w[1], w[0]));
        let flat = [-d[1], d[0], 0.0];
        let across = if length(flat) > 1e-3 { unit(flat) } else { across_was };
        across_was = across;
        frames.push((d, across, unit(cross(d, across))));
    }
    // The section of stretch `f` round `p`, each point's line along the stretch cut by the plane square to `normal`.
    let ring = |p: P3, f: (P3, P3, P3), normal: Option<P3>| -> Vec<P3> {
        profile
            .iter()
            .map(|&(a, b)| {
                let o = add(mul(f.1, a), mul(f.2, b));
                let at = add(p, o);
                match normal {
                    Some(n) if dot(n, f.0).abs() > 1e-6 => add(at, mul(f.0, -dot(n, o) / dot(n, f.0))),
                    _ => at,
                }
            })
            .collect()
    };
    let halving = |a: Option<&(P3, P3, P3)>, b: Option<&(P3, P3, P3)>| Some(unit(add(a?.0, b?.0)));
    (0..frames.len())
        .filter_map(|j| {
            let (&f, &p, &q) = (frames.get(j)?, pts.get(j)?, pts.get(j + 1)?);
            let before = j.checked_sub(1).and_then(|i| frames.get(i));
            Some((ring(p, f, halving(before, Some(&f))), ring(q, f, halving(Some(&f), frames.get(j + 1)))))
        })
        .collect()
}

/// `profile` swept along `path`: a closed mesh of triangles turned out, its ends' first, and how many they are. The
/// profile's first number is across the path (to its left, level), the second up, square to it. Each stretch is a
/// straight prism of its own ([`stretches`]), mitred on the plane halving each bend; where a bend turns in plan and in
/// slope at once, the two sections there are not the same, and each is closed on that plane.
pub fn sweep(path: &[P3], profile: &[(f64, f64)]) -> (Vec<[P3; 3]>, usize) {
    if profile.len() < 3 {
        return (Vec::new(), 0);
    }
    let parts = stretches(path, profile);
    let (Some(first), Some(last)) = (parts.first(), parts.last()) else { return (Vec::new(), 0) };
    let n = profile.len();
    let next = |k: usize| (k + 1) % n;
    let middle = |r: &[P3]| mul(r.iter().fold([0.0; 3], |s, q| add(s, *q)), 1.0 / n as f64);
    let mut tris = Vec::with_capacity(2 * n * (parts.len() + 1));
    // An end facing back along the path, another facing on.
    let back = |tris: &mut Vec<[P3; 3]>, r: &[P3]| {
        let c = middle(r);
        tris.extend((0..n).filter_map(|k| Some([c, *r.get(next(k))?, *r.get(k)?])));
    };
    let on = |tris: &mut Vec<[P3; 3]>, r: &[P3]| {
        let c = middle(r);
        tris.extend((0..n).filter_map(|k| Some([c, *r.get(k)?, *r.get(next(k))?])));
    };
    back(&mut tris, &first.0);
    on(&mut tris, &last.1);
    // Where two stretches' sections differ at a bend, each closed there.
    for w in parts.windows(2) {
        let (a, b) = (&w[0].1, &w[1].0);
        if a.iter().zip(b).any(|(p, q)| length(sub(*p, *q)) > 1e-6) {
            on(&mut tris, a);
            back(&mut tris, b);
        }
    }
    let caps = tris.len();
    for (ra, rb) in &parts {
        for k in 0..n {
            let (Some(&a0), Some(&a1), Some(&b0), Some(&b1)) = (ra.get(k), ra.get(next(k)), rb.get(k), rb.get(next(k))) else { continue };
            tris.push([a0, a1, b0]);
            tris.push([a1, b1, b0]);
        }
    }
    (tris, caps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_solver_keeps_only_the_points_where_the_rail_bends_and_rounds_them() {
        // A flat stretch, then a climb: sampled every 50 mm, as the hosts give it.
        let line: Vec<(P2, f64)> =
            (0..=40).map(|i| f64::from(i) * 50.0).map(|x| (P2(x, 0.0), if x <= 1000.0 { 0.0 } else { (x - 1000.0) * 0.6 })).collect();
        let sharp = RailPart { name: "Top".into(), width: 40.0, height: 900.0, ..RailPart::default() };
        let pts = rail(&line, &sharp);
        assert_eq!(pts.len(), 3, "its ends and the bend: {pts:?}");
        assert!((pts[1][0] - 1000.0).abs() < 10.0, "the bend, where the lowered slope meets the level: {pts:?}");
        let rounded = rail(&line, &RailPart { bend: 300.0, ..sharp });
        assert!(rounded.len() > 4, "an arc for the bend");
        // The arc starts 300 · tan(θ/2) before the bend, on the flat stretch, at the rail's height.
        let half = (0.6f64).atan() / 2.0;
        let start = rounded[1];
        let bend = pts[1][0];
        assert!((start[2] - 880.0).abs() < 1e-6 && (start[0] - (bend - 300.0 * half.tan())).abs() < 1.0, "{start:?}");
    }

    #[test]
    fn a_sweep_up_an_upright_stretch_keeps_its_section_unturned() {
        // A gooseneck: along, straight up, along again.
        let path = [[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [1000.0, 0.0, 200.0], [2000.0, 0.0, 200.0]];
        let (tris, caps) = sweep(&path, &rectangle(40.0, 40.0));
        assert_eq!(caps, 8);
        assert_eq!(tris.len(), 8 + 3 * 8);
        assert!(tris.iter().flatten().all(|p| p[1].abs() <= 20.0 + 1e-9), "never rolls sideways");
    }

    #[test]
    fn a_bend_turning_in_plan_and_slope_at_once_is_mitred_each_stretch_straight_and_flat() {
        // Up a flight, then turning left onto a landing, level: a turn in plan and in slope at one point.
        let path = [[0.0, 0.0, 0.0], [1000.0, 0.0, 600.0], [1000.0, 800.0, 600.0], [1600.0, 800.0, 950.0]];
        let parts = stretches(&path, &rectangle(50.0, 40.0));
        assert_eq!(parts.len(), 3);
        for (j, (a, b)) in parts.iter().enumerate() {
            let way = unit(sub(path[j + 1], path[j]));
            for (p, q) in a.iter().zip(b) {
                // Every edge of the stretch runs along it: none twists round.
                let e = unit(sub(*q, *p));
                assert!(length(cross(e, way)) < 1e-9, "stretch {j}: {p:?} to {q:?} is not along {way:?}");
            }
            // The section level across: square to the stretch, its two bottom corners as high as each other.
            let level = |r: &[P3]| {
                let across = sub(r[3], r[0]);
                (across[2] - way[2] * dot(across, way)).abs() < 1e-6
            };
            assert!(level(a) && level(b), "stretch {j}: {a:?} {b:?}");
        }
        // Each bend's two sections on the plane halving it.
        for j in 0..2 {
            let n = unit(add(unit(sub(path[j + 1], path[j])), unit(sub(path[j + 2], path[j + 1]))));
            for p in parts[j].1.iter().chain(&parts[j + 1].0) {
                assert!(dot(n, sub(*p, path[j + 1])).abs() < 1e-6);
            }
        }
        // Closed: every edge of the mesh is two triangles'.
        let (tris, _) = sweep(&path, &rectangle(50.0, 40.0));
        let key = |p: &P3| p.map(|x| (x * 1e6).round() as i64);
        let mut edges = std::collections::BTreeMap::new();
        for t in &tris {
            for k in 0..3 {
                let (a, b) = (key(&t[k]), key(&t[(k + 1) % 3]));
                *edges.entry(if a < b { (a, b) } else { (b, a) }).or_insert(0) += 1;
            }
        }
        assert!(edges.values().all(|c| c % 2 == 0), "open edges");
    }
}
