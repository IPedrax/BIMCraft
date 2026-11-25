//! Ramps as flights and landings: drawn along a line (straight stretches and arcs, each flight at its own slope),
//! sketched (two sides, a start line, an end line and two lines round each landing), or laid as runs and turns
//! (`stairs::ramp_chain`); and flipped, climbing the other way. Each piece has its two sides, which the 3D view's slab
//! follows, its outline in plan and its height at each end.

use crate::geom::{self, P2};
use crate::model::{Model, RAMP_RUN, Ramp, RampSeg};
use crate::stairs::{Layout, Pad, Run};

/// How near a sketch's lines must meet to be joined, mm.
const MEET: f64 = 1.0;

/// A flight or a landing of a ramp.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub landing: bool,
    /// Its sides from the end it was drawn from to the other, left and right looking that way, as many points each,
    /// each point of one across from the point of the other.
    pub left: Vec<P2>,
    pub right: Vec<P2>,
    /// Its height where it starts and where it ends (as it was drawn: a flipped ramp's fall).
    pub z: (f64, f64),
    /// Its outline, counter-clockwise.
    pub ring: Vec<P2>,
    /// The line across where it starts and where it ends, from left to right: two points, or round a corner a landing
    /// takes, more.
    pub start_cut: Vec<P2>,
    pub end_cut: Vec<P2>,
    /// The segment of the path it is drawn as, for a ramp drawn along a line.
    pub seg: Option<usize>,
    /// Its walk line: up its middle, and round the middle of a corner it takes.
    pub walk: Vec<P2>,
}

impl Piece {
    /// It cut across where its top is `z` high, a flight that passes that height: the part before the cut and the part
    /// after, as it was drawn.
    pub fn split(&self, z: f64) -> Option<(Piece, Piece)> {
        if self.landing || (self.z.1 - self.z.0).abs() < 1e-9 {
            return None;
        }
        let f = (z - self.z.0) / (self.z.1 - self.z.0);
        if !(1e-6..1.0 - 1e-6).contains(&f) {
            return None;
        }
        let (ll, lr, lw) = (len(&self.left), len(&self.right), len(&self.walk));
        let cut = vec![at(&self.left, f * ll), at(&self.right, f * lr)];
        let part = |a: f64, b: f64, z: (f64, f64), sc: Vec<P2>, ec: Vec<P2>| {
            let (left, right) = paired(&sub(&self.left, a * ll, b * ll), &sub(&self.right, a * lr, b * lr));
            Piece {
                landing: false,
                ring: outline(&left, &right, &sc, &ec),
                start_cut: sc,
                end_cut: ec,
                left,
                right,
                z,
                seg: self.seg,
                walk: sub(&self.walk, a * lw, b * lw),
            }
        };
        Some((part(0.0, f, (self.z.0, z), self.start_cut.clone(), cut.clone()), part(f, 1.0, (z, self.z.1), cut, self.end_cut.clone())))
    }

    /// Its middle line, from its start to its end.
    pub fn middle(&self) -> Vec<P2> {
        self.left.iter().zip(&self.right).map(|(a, b)| (*a + *b) * 0.5).collect()
    }

    /// How long it is along its middle.
    pub fn length(&self) -> f64 {
        len(&self.middle())
    }

    /// How steep it is, percent; 0 for a landing.
    pub fn slope(&self) -> f64 {
        let l = self.length();
        if self.landing || l <= 0.0 { 0.0 } else { (self.z.1 - self.z.0).abs() / l * 100.0 }
    }

    /// The height at each point of `side`, by how far along it is.
    pub fn heights(&self, side: &[P2]) -> Vec<f64> {
        let total = len(side);
        let mut d = 0.0;
        let mut out = Vec::with_capacity(side.len());
        for (i, p) in side.iter().enumerate() {
            if i > 0
                && let Some(q) = side.get(i - 1)
            {
                d += q.dist(*p);
            }
            let t = if total > 0.0 { d / total } else { 0.0 };
            out.push(self.z.0 + (self.z.1 - self.z.0) * t);
        }
        out
    }
}

fn len(line: &[P2]) -> f64 {
    line.windows(2).map(|w| if let [a, b] = w { a.dist(*b) } else { 0.0 }).sum()
}

fn ccw(ring: Vec<P2>) -> Vec<P2> {
    if geom::area(&ring) < 0.0 { ring.into_iter().rev().collect() } else { ring }
}

fn dedup(mut ring: Vec<P2>) -> Vec<P2> {
    ring.dedup_by(|b, a| a.dist(*b) < 1e-6);
    while ring.len() > 2 && ring.first().zip(ring.last()).is_some_and(|(a, b)| a.dist(*b) < 1e-6) {
        ring.pop();
    }
    ring
}

/// The outline of a piece from its sides and its cuts.
fn outline(left: &[P2], right: &[P2], start_cut: &[P2], end_cut: &[P2]) -> Vec<P2> {
    let mut ring: Vec<P2> = left.to_vec();
    ring.extend(end_cut.iter().skip(1).take(end_cut.len().saturating_sub(2)));
    ring.extend(right.iter().rev());
    ring.extend(start_cut.iter().rev().skip(1).take(start_cut.len().saturating_sub(2)));
    ccw(dedup(ring))
}

/// A ramp's pieces from its foot as drawn: none when it can't be laid (`check` says why).
pub fn pieces(m: &Model, r: &Ramp) -> Vec<Piece> {
    laid(m, r).unwrap_or_default()
}

/// A ramp's pieces, flipped when it is; or what's wrong with its path or its sketch.
pub fn laid(m: &Model, r: &Ramp) -> Result<Vec<Piece>, String> {
    let mut out = if !r.path.is_empty() {
        along(m.elevation(r.level), r)?
    } else if !r.sketch.is_empty() {
        sketched(m, r)?
    } else {
        chained(m, r)
    };
    if r.flipped {
        let base = m.elevation(r.level);
        let top = out.iter().map(|p| p.z.0.max(p.z.1)).fold(base, f64::max);
        for p in &mut out {
            p.z = (base + top - p.z.0, base + top - p.z.1);
        }
    }
    Ok(out)
}

/// A ramp laid as runs and turns: each run a flight, each turn a landing.
fn chained(m: &Model, r: &Ramp) -> Vec<Piece> {
    let l = crate::stairs::ramp_chain(m, r);
    let mut out = Vec::new();
    for (i, run) in l.runs.iter().enumerate() {
        let n = run.u.perp() * (run.width / 2.0);
        let (left, right) = (vec![run.start + n, run.end() + n], vec![run.start - n, run.end() - n]);
        out.push(Piece {
            landing: false,
            ring: run.ring(),
            start_cut: vec![run.start + n, run.start - n],
            end_cut: vec![run.end() + n, run.end() - n],
            walk: vec![run.start, run.end()],
            left,
            right,
            z: run.z,
            seg: None,
        });
        if let Some(pad) = l.pads.get(i) {
            let c = geom::centroid(&pad.ring);
            out.push(Piece {
                landing: true,
                ring: pad.ring.clone(),
                start_cut: Vec::new(),
                end_cut: Vec::new(),
                left: vec![c],
                right: vec![c],
                z: (pad.z, pad.z),
                seg: None,
                walk: Vec::new(),
            });
        }
    }
    out
}

/// A segment's middle line from `a`: straight, or the arc `seg.arc` degrees; and the way it leaves and the way it arrives.
fn segment(a: P2, seg: &RampSeg) -> (Vec<P2>, P2, P2) {
    let b = seg.to;
    let chord = (b - a).unit();
    if seg.arc == 0.0 || a.dist(b) < 1e-6 {
        return (vec![a, b], chord, chord);
    }
    let th = seg.arc.to_radians();
    let turn = |v: P2, t: f64| {
        let (s, c) = t.sin_cos();
        P2(v.0 * c - v.1 * s, v.0 * s + v.1 * c)
    };
    let t0 = turn(chord, -th / 2.0);
    let radius = a.dist(b) / (2.0 * (th / 2.0).sin());
    let centre = a + t0.perp() * radius;
    let steps = ((seg.arc.abs() / 5.0).ceil() as usize).clamp(2, 72);
    let pts = (0..=steps).map(|k| centre + turn(a - centre, th * k as f64 / steps as f64)).collect();
    (pts, t0, turn(chord, th / 2.0))
}

/// Where the line through `p` along `d` meets the line through `q` along `e`.
fn meet(p: P2, d: P2, q: P2, e: P2) -> Option<P2> {
    geom::line_intersection(p, p + d, q, q + e)
}

/// A ramp drawn along its path: each segment a piece, two flights or two landings meeting at a corner mitred, a flight
/// meeting a landing square, the landing taking the corner.
fn along(base: f64, r: &Ramp) -> Result<Vec<Piece>, String> {
    // How far its sides are from the line drawn: to the left and to the right, as its location line says.
    let w = r.width;
    let (dl, dr) = match r.line {
        crate::model::StairLine::Left => (0.0, w),
        crate::model::StairLine::Centre => (w / 2.0, w / 2.0),
        crate::model::StairLine::Right => (w, 0.0),
    };
    // Its middle, to the left of the line drawn.
    let c = (dl - dr) / 2.0;
    let mut from = r.start;
    let mut segs = Vec::new();
    for (i, s) in r.path.iter().enumerate() {
        if from.dist(s.to) < 1.0 {
            return Err(format!("segment {} of its path has no length", i + 1));
        }
        let (mid, t0, t1) = segment(from, s);
        segs.push((mid, t0, t1, *s));
        from = s.to;
    }
    // The cuts between pieces, left to right: at the foot, at each joint (the end of one, the start of the next), at the top.
    let n = segs.len();
    let mut starts: Vec<Vec<P2>> = vec![Vec::new(); n];
    let mut ends: Vec<Vec<P2>> = vec![Vec::new(); n];
    // The walk line's point at each joint, and at the foot and the top.
    let mut joints: Vec<P2> = vec![P2(0.0, 0.0); n + 1];
    if let (Some(first), Some(s)) = (segs.first(), starts.first_mut()) {
        let p = first.0.first().copied().unwrap_or(r.start);
        *s = vec![p + first.1.perp() * dl, p - first.1.perp() * dr];
        if let Some(j) = joints.first_mut() {
            *j = p + first.1.perp() * c;
        }
    }
    if let (Some(last), Some(e)) = (segs.last(), ends.last_mut()) {
        let p = last.0.last().copied().unwrap_or(r.start);
        *e = vec![p + last.2.perp() * dl, p - last.2.perp() * dr];
        if let Some(j) = joints.last_mut() {
            *j = p + last.2.perp() * c;
        }
    }
    for i in 0..n.saturating_sub(1) {
        let (Some((ma, _, a, sa)), Some((_, b, _, sb))) = (segs.get(i), segs.get(i + 1)) else { continue };
        let p = ma.last().copied().unwrap_or(r.start);
        let (a, b) = (*a, *b);
        let (na, nb) = (a.perp(), b.perp());
        let (cross, dot) = (a.cross(b), a.dot(b));
        if let Some(j) = joints.get_mut(i + 1) {
            *j = meet(p + na * c, a, p + nb * c, b).filter(|_| cross.abs() >= 1e-6).unwrap_or(p + na * c);
        }
        let (end, start) = if cross.abs() < 1e-6 && dot > 0.0 {
            let cut = vec![p + na * dl, p - na * dr];
            (cut.clone(), cut)
        } else if cross.abs() < 1e-6 {
            return Err(format!("its path turns back on itself after segment {}: draw a landing across to turn", i + 1));
        } else {
            let left = meet(p + na * dl, a, p + nb * dl, b).ok_or("its path's corner can't be mitred")?;
            let right = meet(p - na * dr, a, p - nb * dr, b).ok_or("its path's corner can't be mitred")?;
            if sa.landing == sb.landing {
                let c = vec![left, right];
                (c.clone(), c)
            } else {
                // The inside corner is on the side the path turns to.
                let inner_left = cross > 0.0;
                let (i_pt, o_pt) = if inner_left { (left, right) } else { (right, left) };
                let (a_out, b_out) = if inner_left { (i_pt - na * w, i_pt - nb * w) } else { (i_pt + na * w, i_pt + nb * w) };
                let a_end = if inner_left { vec![i_pt, a_out] } else { vec![a_out, i_pt] };
                let b_start = if inner_left { vec![i_pt, b_out] } else { vec![b_out, i_pt] };
                if sb.landing {
                    let corner = if inner_left { vec![i_pt, a_out, o_pt, b_out] } else { vec![b_out, o_pt, a_out, i_pt] };
                    (a_end, corner)
                } else {
                    let corner = if inner_left { vec![i_pt, b_out, o_pt, a_out] } else { vec![a_out, o_pt, b_out, i_pt] };
                    (corner, b_start)
                }
            }
        };
        if let Some(e) = ends.get_mut(i) {
            *e = end;
        }
        if let Some(s) = starts.get_mut(i + 1) {
            *s = start;
        }
    }
    let mut z = base;
    let mut out = Vec::new();
    for (i, (mid, _, _, s)) in segs.iter().enumerate() {
        let (Some(sc), Some(ec)) = (starts.get(i), ends.get(i)) else { continue };
        let (Some(&l0), Some(&r0), Some(&l1), Some(&r1)) = (sc.first(), sc.last(), ec.first(), ec.last()) else { continue };
        // Its sides: from its start cut, through its middle line's points moved out square to it, to its end cut.
        let (mut left, mut right) = (vec![l0], vec![r0]);
        let mut walk = vec![joints.get(i).copied().unwrap_or(l0)];
        for k in 1..mid.len().saturating_sub(1) {
            let (Some(&q), Some(&before), Some(&after)) = (mid.get(k), mid.get(k - 1), mid.get(k + 1)) else { continue };
            let nn = (after - before).unit().perp();
            left.push(q + nn * dl);
            right.push(q - nn * dr);
            walk.push(q + nn * c);
        }
        left.push(l1);
        right.push(r1);
        walk.push(joints.get(i + 1).copied().unwrap_or(l1));
        // As long as it is between its cuts: a flight that meets a landing at a corner ends square at the inside corner.
        let middle: Vec<P2> = left.iter().zip(&right).map(|(a, b)| (*a + *b) * 0.5).collect();
        let rise = if s.landing { 0.0 } else { len(&middle) * s.slope.unwrap_or(r.slope) / 100.0 };
        out.push(Piece {
            landing: s.landing,
            ring: outline(&left, &right, sc, ec),
            start_cut: sc.clone(),
            end_cut: ec.clone(),
            left,
            right,
            z: (z, z + rise),
            seg: Some(i),
            walk,
        });
        z += rise;
    }
    if !out.iter().any(|p| !p.landing) {
        return Err("its path has no flight: a ramp needs one at least".into());
    }
    Ok(out)
}

/// How high a ramp drawn along a line climbs: what its flights rise.
pub fn path_rise(r: &Ramp) -> f64 {
    along(0.0, r).ok().and_then(|ps| ps.last().map(|p| p.z.1)).unwrap_or(0.0)
}

/// The part of `line` from `a` to `b` along it.
fn sub(line: &[P2], a: f64, b: f64) -> Vec<P2> {
    let mut out = vec![at(line, a)];
    let mut d = 0.0;
    for w in line.windows(2) {
        let [p, q] = w else { continue };
        d += p.dist(*q);
        if d > a + 1e-6 && d < b - 1e-6 {
            out.push(*q);
        }
    }
    out.push(at(line, b));
    out
}

/// The point `d` along `line`.
fn at(line: &[P2], d: f64) -> P2 {
    let mut left = d.max(0.0);
    for w in line.windows(2) {
        let [a, b] = w else { continue };
        let l = a.dist(*b);
        if left <= l && l > 0.0 {
            return *a + (*b - *a) * (left / l);
        }
        left -= l;
    }
    line.last().copied().unwrap_or(P2(0.0, 0.0))
}

/// How far along `line` the point nearest `p` is, and how far `p` is from it.
fn param(line: &[P2], p: P2) -> (f64, f64) {
    let (mut best, mut d) = ((0.0, f64::INFINITY), 0.0);
    for w in line.windows(2) {
        let [a, b] = w else { continue };
        let l = a.dist(*b);
        let t = geom::param_on_segment(p, *a, *b).clamp(0.0, 1.0);
        let q = *a + (*b - *a) * t;
        if q.dist(p) < best.1 {
            best = (d + t * l, q.dist(p));
        }
        d += l;
    }
    best
}

/// Two sides as many points each, each across from the other: the points of both at their share of the way along.
fn paired(left: &[P2], right: &[P2]) -> (Vec<P2>, Vec<P2>) {
    let (ll, lr) = (len(left), len(right));
    let shares = |line: &[P2], total: f64| -> Vec<f64> {
        let mut d = 0.0;
        let mut out = vec![0.0];
        for w in line.windows(2) {
            let [a, b] = w else { continue };
            d += a.dist(*b);
            out.push(if total > 0.0 { d / total } else { 1.0 });
        }
        out
    };
    let mut ts: Vec<f64> = shares(left, ll).into_iter().chain(shares(right, lr)).collect();
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|b, a| (*a - *b).abs() < 1e-6);
    (ts.iter().map(|t| at(left, t * ll)).collect(), ts.iter().map(|t| at(right, t * lr)).collect())
}

/// Polylines joined end to end where they meet.
fn chains(mut lines: Vec<Vec<P2>>) -> Vec<Vec<P2>> {
    let mut out: Vec<Vec<P2>> = Vec::new();
    while let Some(mut c) = lines.pop() {
        while let (Some(&first), Some(&last)) = (c.first(), c.last()) {
            let Some(k) = lines.iter().position(|l| {
                let (Some(a), Some(b)) = (l.first(), l.last()) else { return false };
                a.dist(last) < MEET || b.dist(last) < MEET || a.dist(first) < MEET || b.dist(first) < MEET
            }) else {
                break;
            };
            let mut l = lines.swap_remove(k);
            let (Some(&a), Some(&b)) = (l.first(), l.last()) else { continue };
            if a.dist(last) < MEET {
                c.extend(l.into_iter().skip(1));
            } else if b.dist(last) < MEET {
                l.reverse();
                c.extend(l.into_iter().skip(1));
            } else if b.dist(first) < MEET {
                l.extend(c.into_iter().skip(1));
                c = l;
            } else {
                l.reverse();
                l.extend(c.into_iter().skip(1));
                c = l;
            }
        }
        out.push(c);
    }
    out
}

/// A sketched ramp: its sides from its start line to its end line, cut across by its other run lines, each two of them
/// a landing; its flights sharing its rise as they're long.
fn sketched(m: &Model, r: &Ramp) -> Result<Vec<Piece>, String> {
    let runs: Vec<[P2; 2]> = r.sketch.iter().filter(|l| l.style.as_deref() == Some(RAMP_RUN)).map(|l| [l.start(), l.end()]).collect();
    let sides: Vec<Vec<P2>> = r.sketch.iter().filter(|l| l.style.as_deref() != Some(RAMP_RUN)).map(|l| l.points()).collect();
    let near = |p: P2, l: &[P2; 2]| l.iter().any(|q| q.dist(p) < MEET);
    let mut two = chains(sides);
    // Its start and its end: the run lines across the sides' ends, the first drawn its start; the rest its landings' ends.
    let ends: Vec<P2> = two.iter().flat_map(|c| [c.first().copied(), c.last().copied()]).flatten().collect();
    let at_ends = |l: &[P2; 2]| l.iter().all(|q| ends.iter().any(|e| e.dist(*q) < MEET));
    let mut runs = runs;
    if let Some(k) = runs.iter().position(at_ends) {
        let first = runs.remove(k);
        runs.insert(0, first);
        if let Some(j) = runs.iter().skip(1).position(at_ends) {
            let second = runs.remove(j + 1);
            runs.insert(1, second);
        }
    }
    let [start, end, cuts @ ..] = runs.as_slice() else {
        return Err("a sketched ramp needs a start line and an end line (Run lines), and its two sides (Boundary lines)".into());
    };
    if two.len() != 2 {
        return Err(format!(
            "a sketched ramp's boundary is its two sides, each running from its start line to its end line; these lines make {} {}",
            two.len(),
            if two.len() == 1 { "line" } else { "lines" }
        ));
    }
    for c in &mut two {
        let (Some(&a), Some(&b)) = (c.first(), c.last()) else { continue };
        if near(b, start) && !near(a, start) {
            c.reverse();
        }
        let (Some(&a), Some(&b)) = (c.first(), c.last()) else { continue };
        if !(near(a, start) && near(b, end)) {
            return Err("each of a sketched ramp's two sides runs from an end of its start line (the first Run line) to an end of its end line (the second)".into());
        }
    }
    let [one, other] = [two.swap_remove(0), two.swap_remove(0)];
    let mut ring = one.clone();
    ring.extend(other.iter().rev());
    let (left, right) = if geom::area(&ring) > 0.0 { (other, one) } else { (one, other) };
    let (ll, lr) = (len(&left), len(&right));
    // The lines across, each from where it meets the left side to where it meets the right, in order up the ramp.
    let mut across = Vec::new();
    for c in cuts {
        let [p, q] = *c;
        let (a, b) = ((param(&left, p), param(&right, q)), (param(&left, q), param(&right, p)));
        let (l, rr) = if a.0.1 + a.1.1 <= b.0.1 + b.1.1 { a } else { b };
        if l.1 > MEET || rr.1 > MEET {
            return Err("each landing line of a sketched ramp runs across it, from one side to the other".into());
        }
        across.push((l.0, rr.0));
    }
    across.sort_by(|a, b| a.0.total_cmp(&b.0));
    if across.len() % 2 == 1 {
        return Err("a sketched ramp's landings are each between two run lines: add the other end of the landing".into());
    }
    let mut cut_at = vec![(0.0, 0.0)];
    cut_at.extend(across);
    cut_at.push((ll, lr));
    let mut out = Vec::new();
    for (k, w) in cut_at.windows(2).enumerate() {
        let [(a0, b0), (a1, b1)] = *w else { continue };
        if a1 < a0 - 1e-6 || b1 < b0 - 1e-6 {
            return Err("a sketched ramp's run lines cross each other".into());
        }
        let (l, rr) = paired(&sub(&left, a0, a1), &sub(&right, b0, b1));
        let (Some(&l0), Some(&r0), Some(&l1), Some(&r1)) = (l.first(), rr.first(), l.last(), rr.last()) else { continue };
        let (sc, ec) = (vec![l0, r0], vec![l1, r1]);
        let walk = l.iter().zip(&rr).map(|(a, b)| (*a + *b) * 0.5).collect();
        out.push(Piece {
            landing: k % 2 == 1,
            ring: outline(&l, &rr, &sc, &ec),
            start_cut: sc,
            end_cut: ec,
            left: l,
            right: rr,
            z: (0.0, 0.0),
            seg: None,
            walk,
        });
    }
    // The rise shared among the flights as they are long.
    let (base, top) = m.span(r.level, 0.0, r.top_level, r.height);
    let total: f64 = out.iter().filter(|p| !p.landing).map(Piece::length).sum();
    let mut z = base;
    for p in &mut out {
        let rise = if p.landing || total <= 0.0 { 0.0 } else { (top - base) * p.length() / total };
        p.z = (z, z + rise);
        z += rise;
    }
    Ok(out)
}

/// What's wrong with a ramp's path or sketch, if anything.
pub fn check(who: &str, m: &Model, r: &Ramp) -> Result<(), String> {
    if !r.path.is_empty() && !r.sketch.is_empty() {
        return Err(format!("{who}: a ramp is drawn along a `path` or sketched, not both"));
    }
    if (!r.path.is_empty() || !r.sketch.is_empty()) && !r.landings.is_empty() {
        return Err(format!("{who}: a ramp drawn along a path or sketched has its landings in what was drawn, not in `landings`"));
    }
    if r.path.len() > 200 {
        return Err(format!("{who}: its path has more than 200 segments"));
    }
    for (i, s) in r.path.iter().enumerate() {
        if !(s.to.is_finite() && s.to.0.abs() <= crate::model::MAX_COORD && s.to.1.abs() <= crate::model::MAX_COORD) {
            return Err(format!("{who}: segment {} of its path ends at a point that isn't a number within range", i + 1));
        }
        if !(s.arc.is_finite() && s.arc.abs() <= 340.0) {
            return Err(format!("{who}: segment {} of its path turns {}°; an arc turns 340° at most", i + 1, s.arc));
        }
        if let Some(k) = s.slope
            && !(1.0..=50.0).contains(&k)
        {
            return Err(format!("{who}: segment {} of its path slopes {k}%, not from 1% to 50%", i + 1));
        }
    }
    if !r.path.is_empty() || !r.sketch.is_empty() {
        laid(m, r).map_err(|e| format!("{who}: {e}"))?;
    }
    Ok(())
}

/// A ramp drawn along a line with flight `seg` at `slope`: the flight as long as it must be to climb what it did, and
/// what comes after it moved along with its end.
pub fn with_slope(r: &Ramp, seg: usize, slope: f64) -> Ramp {
    let mut out = r.clone();
    let from = if seg == 0 { r.start } else { r.path.get(seg - 1).map_or(r.start, |s| s.to) };
    let Some(s) = r.path.get(seg) else { return out };
    let Some(piece) = along(0.0, r).ok().and_then(|ps| ps.into_iter().find(|p| p.seg == Some(seg))) else { return out };
    if s.landing || slope <= 0.0 {
        return out;
    }
    // What its corners take off the segment stays; the rest is as long as the rise at the new slope asks. An arc keeps
    // its turn, so what comes after only moves.
    let whole = seg_length(from, s);
    let want = (piece.z.1 - piece.z.0).abs() / (slope / 100.0) + (whole - piece.length());
    if whole <= 0.0 || want <= 1.0 {
        return out;
    }
    let k = want / whole;
    let to = from + (s.to - from) * k;
    let delta = to - s.to;
    for (i, x) in out.path.iter_mut().enumerate() {
        if i == seg {
            x.to = to;
            x.slope = Some(slope);
        } else if i > seg {
            x.to = x.to + delta;
        }
    }
    out
}

/// A ramp with every flight at `slope`: one laid as runs takes it as its slope; one drawn along a line has each flight as
/// long as it must be to climb what it did ([`with_slope`]), and the slope as its own.
pub fn with_all_slopes(r: &Ramp, slope: f64) -> Ramp {
    let mut out = r.clone();
    if !r.path.is_empty() {
        for i in 0..r.path.len() {
            if r.path.get(i).is_some_and(|s| !s.landing) {
                out = with_slope(&out, i, slope);
            }
        }
        out.path.iter_mut().for_each(|s| s.slope = None);
    }
    out.slope = slope;
    out
}

/// A slope of `percent` as degrees, and as a grade's run for a rise of one (1 : that).
pub fn degrees(percent: f64) -> f64 {
    (percent / 100.0).atan().to_degrees()
}

pub fn grade(percent: f64) -> f64 {
    if percent > 0.0 { 100.0 / percent } else { f64::INFINITY }
}

/// A side of a ramp going up it (left or right, as a flipped one climbs), in plan with its height at each point: its
/// pieces' sides one after the other, round the outside of a corner a landing takes.
pub fn side(m: &Model, r: &Ramp, left: bool) -> Vec<(P2, f64)> {
    let ps = pieces(m, r);
    // As drawn, the side asked for: a flipped ramp's left going up is its right as drawn, the other way.
    let drawn_left = left != r.flipped;
    let mut out: Vec<(P2, f64)> = Vec::new();
    for (i, p) in ps.iter().enumerate() {
        let s = if drawn_left { &p.left } else { &p.right };
        let zs = p.heights(s);
        // Round a corner: the cut's points on this side, from where the last piece's side ended.
        if i > 0
            && let Some(prev) = ps.get(i - 1)
        {
            let corner = if p.start_cut.len() > 2 { &p.start_cut } else { &prev.end_cut };
            let other = if drawn_left { p.right.first() } else { p.left.first() };
            let gap = out.last().zip(s.first()).is_some_and(|(l, f)| l.0.dist(*f) > 1e-6);
            if corner.len() > 2
                && gap
                && let (Some(&(last, _)), Some(other)) = (out.last(), other)
            {
                let mut pts: Vec<P2> = corner.iter().copied().filter(|q| q.dist(*other) > 1e-6).collect();
                if pts.last().is_some_and(|q| q.dist(last) < pts.first().map_or(f64::INFINITY, |f| f.dist(last))) {
                    pts.reverse();
                }
                let zc = if prev.landing { prev.z.1 } else { p.z.0 };
                out.extend(pts.into_iter().map(|q| (q, zc)));
            }
        }
        out.extend(s.iter().copied().zip(zs));
    }
    out.dedup_by(|b, a| a.0.dist(b.0) < 1e-6);
    if r.flipped {
        out.reverse();
    }
    out
}

/// A ramp drawn along a line or sketched as runs, landings and a walk line, as other parts read any ramp: each flight
/// a run from the middle of its foot to the middle of its top, each landing a pad.
pub fn layout(m: &Model, r: &Ramp) -> Layout {
    let ps = pieces(m, r);
    let mut out =
        Layout { runs: Vec::new(), pads: Vec::new(), walk: Vec::new(), width: r.width, winders: 0, turns: Vec::new(), nosings: Vec::new() };
    for p in &ps {
        let mid = p.middle();
        let line = if p.walk.len() >= 2 { p.walk.clone() } else { mid.clone() };
        let zs = p.heights(&line);
        out.walk.extend(line.iter().copied().zip(zs));
        if p.landing {
            out.pads.push(Pad { ring: p.ring.clone(), z: p.z.0, tread: false });
            out.turns.push(1);
        } else if let (Some(&a), Some(&b)) = (mid.first(), mid.last()) {
            let width = p.left.first().zip(p.right.first()).map_or(r.width, |(l, rr)| l.dist(*rr));
            out.runs.push(Run { start: a, u: (b - a).unit(), length: p.length(), z: p.z, risers: 0, width });
        }
    }
    out.walk.dedup_by(|b, a| a.0.dist(b.0) < 1e-6);
    if r.flipped {
        out.walk.reverse();
    }
    out
}

/// How long segment `seg` from `a` is along its middle.
pub fn seg_length(a: P2, seg: &RampSeg) -> f64 {
    len(&segment(a, seg).0)
}

/// Segment `seg` from `a`, cut short (or drawn on) to `length` along its middle: a straight one along its line, an arc round
/// its centre.
pub fn cut_short(a: P2, seg: &RampSeg, length: f64) -> RampSeg {
    let whole = seg_length(a, seg);
    if whole <= 0.0 || (length - whole).abs() < 1e-9 {
        return *seg;
    }
    let k = length / whole;
    if seg.arc == 0.0 {
        return RampSeg { to: a + (seg.to - a) * k, ..*seg };
    }
    // Round the same centre, through less of the turn.
    let th = seg.arc.to_radians();
    let chord = (seg.to - a).unit();
    let (s, c) = (-th / 2.0).sin_cos();
    let t0 = P2(chord.0 * c - chord.1 * s, chord.0 * s + chord.1 * c);
    let radius = a.dist(seg.to) / (2.0 * (th / 2.0).sin());
    let centre = a + t0.perp() * radius;
    let (s, c) = (th * k).sin_cos();
    let v = a - centre;
    RampSeg { to: centre + P2(v.0 * c - v.1 * s, v.0 * s + v.1 * c), arc: seg.arc * k, ..*seg }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::engine::Session;
    use crate::geom::P2;
    use crate::model::Element;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    fn ramp(s: &Session, id: &Value) -> crate::model::Ramp {
        match s.model.elements.get(id.as_u64().unwrap()) {
            Some(Element::Ramp(r)) => r.clone(),
            _ => panic!("a ramp"),
        }
    }

    #[test]
    fn a_ramp_drawn_along_a_line_climbs_each_flight_at_its_own_slope_and_a_landing_takes_the_corner() {
        let mut s = Session::new();
        // East 6 m at 5%, a landing turning north 1.2 m, north 4 m at 10%.
        let id = run(
            &mut s,
            "ramp",
            json!({"start": [0, 0], "width": 1200, "slope": 5, "path": [
                {"to": [6000, 0]}, {"to": [6000, 1200], "landing": true}, {"to": [6000, 5200], "slope": 10}]}),
        )["id"]
            .clone();
        let r = ramp(&s, &id);
        let ps = super::pieces(&s.model, &r);
        assert_eq!(ps.len(), 3);
        // The first flight ends square at the landing's inside corner, 5.4 m along: 270 up; the second 4 m, 400 up.
        assert!((ps[0].z.1 - 270.0).abs() < 1e-6, "{:?}", ps[0].z);
        assert!((ps[2].z.1 - 670.0).abs() < 1e-6, "{:?}", ps[2].z);
        assert!((r.height - 670.0).abs() < 1e-6, "it climbs as high as its flights: {}", r.height);
        // The landing covers the corner: from x 5400 (the flight ends square at the inside corner) to 6600, y -600 to 1200.
        let a = crate::geom::area(&ps[1].ring);
        assert!((a - 1200.0 * 1800.0).abs() < 1.0, "landing area {a}");
        // A slope typed for the first flight: twice as steep, half as long (2.7 m, and the corner's 0.6), the rest moved back.
        let steeper = super::with_slope(&r, 0, 10.0);
        assert!((steeper.path[0].to.0 - 3300.0).abs() < 1e-6 && (steeper.path[2].to.0 - 3300.0).abs() < 1e-6, "{:?}", steeper.path);
        let ps = super::pieces(&s.model, &steeper);
        assert!((ps[0].z.1 - 270.0).abs() < 1e-6 && (ps[0].slope() - 10.0).abs() < 1e-6);
    }

    #[test]
    fn a_ramp_drawn_along_its_left_edge_and_a_layered_type() {
        let mut s = Session::new();
        let id =
            run(&mut s, "ramp", json!({"start": [0, 0], "width": 1000, "slope": 5, "line": "left", "path": [{"to": [5000, 0]}]}))["id"]
                .clone();
        let r = ramp(&s, &id);
        let ps = super::pieces(&s.model, &r);
        assert!(ps[0].left.iter().all(|p| p.1.abs() < 1e-6) && ps[0].right.iter().all(|p| (p.1 + 1000.0).abs() < 1e-6));
        assert_eq!(r.type_name, crate::model::RAMP_TYPE, "a new ramp has the starter type, 200 thick");
        let all = super::with_all_slopes(&r, 10.0);
        assert!((all.path[0].to.0 - 2500.0).abs() < 1e-6 && all.slope == 10.0);
        assert!((super::degrees(100.0) - 45.0).abs() < 1e-9 && (super::grade(8.0) - 12.5).abs() < 1e-9);
    }

    #[test]
    fn a_circular_ramp_and_a_flipped_one() {
        let mut s = Session::new();
        // Round a centre at the origin, radius 3 m: two half turns.
        let id = run(
            &mut s,
            "ramp",
            json!({"start": [3000, 0], "slope": 5, "path": [{"to": [-3000, 0], "arc": 180}, {"to": [3000, 0], "arc": 180}]}),
        )["id"]
            .clone();
        let r = ramp(&s, &id);
        let ps = super::pieces(&s.model, &r);
        let length = 2.0 * std::f64::consts::PI * 3000.0;
        assert!((ps.iter().map(super::Piece::length).sum::<f64>() - length).abs() < length * 0.01);
        assert!(ps[0].left.iter().all(|p| (p.length() - 2400.0).abs() < 1.0), "its inside edge, 600 in");
        run(&mut s, "flip", json!({"ids": [id]}));
        let r = ramp(&s, &id);
        let ps = super::pieces(&s.model, &r);
        assert!(ps[0].z.0 > ps[1].z.1, "flipped, it falls the way it was drawn");
    }

    #[test]
    fn a_ramp_sketched_with_two_landings_round_a_corner() {
        let mut s = Session::new();
        // The sketch of an L: east along y 0..1000 from x 0 to 6000, then north along x 5000..6000 up to y 4000.
        let line = |a: [f64; 2], b: [f64; 2]| json!({"start": a, "end": b});
        let id = run(
            &mut s,
            "ramp",
            json!({"height": 300, "sketch": [
                line([0.0, 0.0], [6000.0, 0.0]), line([6000.0, 0.0], [6000.0, 4000.0]),
                line([0.0, 1000.0], [5000.0, 1000.0]), line([5000.0, 1000.0], [5000.0, 4000.0])],
                "runs": [[[0, 0], [0, 1000]], [[5000, 4000], [6000, 4000]],
                         [[2500, 0], [2500, 1000]], [[3500, 0], [3500, 1000]],
                         [[5000, 1500], [6000, 1500]], [[5000, 2500], [6000, 2500]]]}),
        )["id"]
            .clone();
        let r = ramp(&s, &id);
        let ps = super::pieces(&s.model, &r);
        assert_eq!(ps.iter().map(|p| p.landing).collect::<Vec<_>>(), [false, true, false, true, false]);
        assert!((ps[4].z.1 - 300.0).abs() < 1e-6);
        assert!((ps[1].z.0 - ps[1].z.1).abs() < 1e-9, "a landing is flat");
        // The corner flight turns: its outline holds the corner square.
        assert!(crate::geom::contains(&ps[2].ring, P2(5500.0, 500.0)));
        // A railing down each side follows it round.
        let rl = run(&mut s, "railing", json!({"host": id, "side": "left"}))["ids"][0].as_u64().unwrap();
        let Some(Element::Railing(rl)) = s.model.elements.get(rl) else { panic!() };
        let line = crate::stairs::railing(&s.model, rl);
        assert!(line.len() >= 3, "{line:?}");
    }
}
