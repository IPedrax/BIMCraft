//! A rail's path and its sweep, two steps apart. The solver ([`rail`]) takes the railing's line, as its hosts give it
//! point by point, and makes the rail's centre line: the points where it really turns or changes slope kept, the rest
//! left out, and each of those bends rounded as the rail asks (a plan corner by `corner`, a change of slope by `bend`;
//! 0 leaves it sharp). The sweeper ([`sweep`]) takes any centre line and a section and makes the rail's surface, its
//! section carried along without rolling and mitred on the plane halving each bend it meets sharp.

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
pub fn rail(line: &[(P2, f64)], r: &RailPart) -> Vec<P3> {
    let d = super::rail_depth(r);
    let raw: Vec<P3> = line.iter().map(|&(p, z)| [p.0, p.1, z + r.height - d / 2.0]).collect();
    let kept = lowered(&simplify(&raw), d / 2.0);
    round(&kept, r.corner, r.bend, if r.shape == PartShape::Circle { r.width } else { r.width.max(d) })
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

/// `profile` swept along `path`: a closed mesh of triangles turned out, its ends' first, and how many they are. The
/// profile's first number is across the path (to its left, level), the second up, square to it. Each stretch's section
/// is level across while the path is not upright, and carried on unturned up an upright stretch (a gooseneck's rise);
/// where two stretches meet, one ring of points on the plane halving the bend, where the lines of the stretch coming in
/// meet it, so the faces either side stay flat.
pub fn sweep(path: &[P3], profile: &[(f64, f64)]) -> (Vec<[P3; 3]>, usize) {
    let mut pts: Vec<P3> = Vec::with_capacity(path.len());
    for p in path {
        if pts.last().is_none_or(|q| length(sub(*p, *q)) > 1e-6) {
            pts.push(*p);
        }
    }
    if pts.len() < 2 || profile.len() < 3 {
        return (Vec::new(), 0);
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
    let off = |(_, s, u): (P3, P3, P3), (a, b): (f64, f64)| add(mul(s, a), mul(u, b));
    let rings: Vec<Vec<P3>> = (0..pts.len())
        .map(|i| {
            let p = pts.get(i).copied().unwrap_or([0.0; 3]);
            let before = i.checked_sub(1).and_then(|j| frames.get(j)).copied();
            let after = frames.get(i).copied();
            profile
                .iter()
                .map(|q| match (before, after) {
                    (Some(fa), Some(fb)) => {
                        // On the plane halving the bend, where the line of this point of the section coming in meets it.
                        let normal = unit(add(fa.0, fb.0));
                        let o = off(fa, *q);
                        let along = dot(normal, fa.0);
                        if along.abs() < 1e-6 { add(p, o) } else { add(add(p, o), mul(fa.0, -dot(normal, o) / along)) }
                    }
                    (Some(f), None) | (None, Some(f)) => add(p, off(f, *q)),
                    (None, None) => p,
                })
                .collect()
        })
        .collect();
    let n = profile.len();
    let next = |k: usize| (k + 1) % n;
    let mut tris = Vec::with_capacity(2 * n * pts.len());
    let (Some(first), Some(last)) = (rings.first(), rings.last()) else { return (Vec::new(), 0) };
    let middle = |r: &[P3]| mul(r.iter().fold([0.0; 3], |s, q| add(s, *q)), 1.0 / n as f64);
    let (a, b) = (middle(first), middle(last));
    for k in 0..n {
        tris.push([a, first[next(k)], first[k]]);
        tris.push([b, last[k], last[next(k)]]);
    }
    let caps = tris.len();
    for w in rings.windows(2) {
        let (ra, rb) = (&w[0], &w[1]);
        for k in 0..n {
            tris.push([ra[k], ra[next(k)], rb[k]]);
            tris.push([ra[next(k)], rb[next(k)], rb[k]]);
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
}
