//! Stairs and ramps as runs and landings, and the railings along them: where each run goes and how
//! high, the landings between runs, the walk line up the middle, and the line a railing follows,
//! in from a side.

use crate::geom::{self, P2};
use crate::model::{Element, Model, RAILING_INSET, Railing, Ramp, Side, Stair, heading};
use crate::walls::Plane;

/// A run: from the middle of its foot, `start`, along unit `u`, `length` long, from height `z.0`
/// up to `z.1`. A stair's has `risers` risers, its first at its start and its last at its end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Run {
    pub start: P2,
    pub u: P2,
    pub length: f64,
    pub z: (f64, f64),
    pub risers: u32,
}

impl Run {
    pub fn end(&self) -> P2 {
        self.start + self.u * self.length
    }

    /// Its outline, `width` wide, counter-clockwise.
    pub fn ring(&self, width: f64) -> Vec<P2> {
        let n = self.u.perp() * (width / 2.0);
        vec![self.start - n, self.end() - n, self.end() + n, self.start + n]
    }
}

/// A landing: its outline, counter-clockwise, and its height.
#[derive(Clone, Debug, PartialEq)]
pub struct Pad {
    pub ring: Vec<P2>,
    pub z: f64,
}

/// A stair's or a ramp's runs and landings, how wide they are, and its walk line: up the middle of
/// each run, from its foot to its top, and across each landing, with the height of each point.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub runs: Vec<Run>,
    pub pads: Vec<Pad>,
    pub walk: Vec<(P2, f64)>,
    pub width: f64,
    /// How many of `pads` are winder treads, not landings.
    pub winders: usize,
}

/// Runs of `(length, heights, risers)` from `start` toward `angle` degrees, `width` wide, and a
/// landing after each but the last, as deep as they're wide, the next run going on as `turns` say:
/// straight on (0), left or right (90, -90), or back beside it, to its left or right (180, -180). A turn of
/// `(turn, winders)` with winders has no landing: that many treads fanned round its inside corner, `each` apart
/// in height, the first where the landing would be.
fn chain(start: P2, angle: f64, width: f64, runs: Vec<(f64, (f64, f64), u32)>, turns: &[Turn], each: f64) -> Layout {
    let (mut at, mut u) = (start, heading(angle));
    let mut out = Layout { runs: Vec::new(), pads: Vec::new(), walk: Vec::new(), width, winders: 0 };
    for (i, (length, z, risers)) in runs.into_iter().enumerate() {
        let run = Run { start: at, u, length, z, risers };
        out.walk.extend([(run.start, z.0), (run.end(), z.1)]);
        out.runs.push(run);
        let Some(&Turn { turn, winders, length: deep, into, balance }) = turns.get(i) else { break };
        // Winders balanced into the flights start before the corner and end past it.
        let (e, n, w) = (run.end() + u * into, u.perp(), width);
        // How deep the landing is: as the stair is wide unless it says, and no less where the next run leaves its side.
        let d = if winders > 0 || deep <= 0.0 {
            w
        } else if turn.abs() == 90.0 {
            deep.max(w)
        } else {
            deep
        };
        // Across the landing from the run's right edge to its left, and how far it reaches past the
        // run: to the next run's far edge, beside it when it turns back.
        let side = n * if turn > 0.0 { 1.0 } else { -1.0 };
        let (ring, via, next, dir) = if turn == 0.0 {
            (vec![e - n * (w / 2.0), e - n * (w / 2.0) + u * d, e + n * (w / 2.0) + u * d, e + n * (w / 2.0)], vec![], e + u * d, u)
        } else if turn.abs() == 90.0 {
            let ring = vec![e - n * (w / 2.0), e - n * (w / 2.0) + u * d, e + n * (w / 2.0) + u * d, e + n * (w / 2.0)];
            (ring, vec![e + u * (d - w / 2.0)], e + u * (d - w / 2.0) + side * (w / 2.0), side)
        } else {
            let ring = vec![e - side * (w / 2.0), e - side * (w / 2.0) + u * d, e + side * (1.5 * w) + u * d, e + side * (1.5 * w)];
            (ring, vec![e + u * (d / 2.0), e + u * (d / 2.0) + side * w], e + side * w, u * -1.0)
        };
        if winders > 0 && turn != 0.0 {
            let p = e + side * (w / 2.0);
            let rings =
                balanced(p, side, u, w, turn.abs(), winders, into, balance).unwrap_or_else(|| wind(p, side, u, w, turn.abs(), winders));
            out.winders += rings.len();
            // The walk line climbs through the turn: on into it, round the inside corner at the stair's middle, and on out.
            let top = (rings.len() as f64 - 1.0) * each;
            out.pads.extend(rings.into_iter().enumerate().map(|(k, ring)| Pad { ring: ccw(ring), z: z.1 + k as f64 * each }));
            let (r, sweep) = (w / 2.0, turn.abs());
            let total = 2.0 * into + r * sweep.to_radians();
            let high = |d: f64| z.1 + if total > 0.0 { top * d / total } else { 0.0 };
            if into > 0.0 {
                out.walk.push((e, high(into)));
            }
            let steps = (sweep / 15.0).round().max(1.0) as u32;
            for k in 1..=steps {
                let a = (sweep * f64::from(k) / f64::from(steps)).to_radians();
                out.walk.push((p - side * (r * a.cos()) + u * (r * a.sin()), high(into + r * a)));
            }
            let _ = via;
            (at, u) = (next + dir * into, dir);
            continue;
        } else {
            out.pads.push(Pad { ring: ccw(ring), z: z.1 });
            out.walk.extend(via.into_iter().map(|p| (p, z.1)));
        }
        (at, u) = (next, dir);
    }
    out
}

/// A turn after a run: which way, how many winders (0: a landing), the landing's length (0: the stair's width), and how
/// far into the flight on each side its winders run, in mm (whole treads).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Turn {
    turn: f64,
    winders: u32,
    length: f64,
    into: f64,
    balance: u32,
}

/// The point `d` along `line`.
fn along(line: &[P2], d: f64) -> P2 {
    let mut left = d.max(0.0);
    for w in line.windows(2) {
        let &[a, b] = w else { continue };
        let len = a.dist(b);
        if left <= len && len > 0.0 {
            return a + (b - a) * (left / len);
        }
        left -= len;
    }
    line.last().copied().unwrap_or(P2(0.0, 0.0))
}

/// The corners of `line` strictly between `from` and `to` along it.
fn corners(line: &[P2], from: f64, to: f64) -> Vec<P2> {
    let mut out = Vec::new();
    let mut d = 0.0;
    for w in line.windows(2) {
        let &[a, b] = w else { continue };
        if d > from + 1e-6 && d < to - 1e-6 {
            out.push(a);
        }
        d += a.dist(b);
    }
    out
}

/// The treads of a turn whose winders are balanced: the turn takes `into` mm of the flight on each side, and its
/// `count` winders and the treads of those stretches are redrawn as steps of equal depth along the walk line (the
/// middle of the stair) and of equal depth along the inside string, each riser the line through its two points. So no
/// tread comes to a point. None when `into` is nothing, or the risers so drawn would cross.
#[allow(clippy::too_many_arguments)]
fn balanced(p: P2, side: P2, u: P2, w: f64, sweep: f64, count: u32, into: f64, per_side: u32) -> Option<Vec<Vec<P2>>> {
    if into <= 0.0 || per_side == 0 || count == 0 || w <= 0.0 {
        return None;
    }
    let back = sweep == 180.0;
    let at = |x: f64, y: f64| p + side * x + u * y;
    // In the turn's own frame: x toward the side it turns to, y on along the first run, the inside corner at the origin.
    let inner = if back { vec![(0.0, -into), (0.0, 0.0), (0.0, -into)] } else { vec![(0.0, -into), (0.0, 0.0), (into, 0.0)] };
    let outer = if back { vec![(-w, -into), (-w, w), (w, w), (w, -into)] } else { vec![(-w, -into), (-w, w), (into, w)] };
    let (inner, outer): (Vec<P2>, Vec<P2>) =
        (inner.into_iter().map(|(x, y)| P2(x, y)).collect(), outer.into_iter().map(|(x, y)| P2(x, y)).collect());
    let r = w / 2.0;
    let arc = r * sweep.to_radians();
    let walk = |d: f64| {
        if d <= into {
            P2(-r, d - into)
        } else if d <= into + arc {
            let a = (d - into) / r;
            P2(-r * a.cos(), r * a.sin())
        } else if back {
            P2(r, -(d - into - arc))
        } else {
            P2(d - into - arc, r)
        }
    };
    let n = f64::from(count) + 2.0 * f64::from(per_side);
    {
        let (li, lw) = (2.0 * into, 2.0 * into + arc);
        let lo: f64 = outer.windows(2).map(|s| s.first().zip(s.get(1)).map_or(0.0, |(a, b)| a.dist(*b))).sum();
        // Each riser: where it leaves the inside string, and where it meets the outside one, as a length along it.
        let mut risers: Vec<(f64, f64)> = Vec::new();
        for i in 0..=(n as u32) {
            let f = f64::from(i) / n;
            let so = if i == 0 {
                0.0
            } else if f64::from(i) == n {
                lo
            } else {
                let (a, b) = (along(&inner, f * li), walk(f * lw));
                let dir = b - a;
                let mut d = 0.0;
                let mut hit = None;
                for s in outer.windows(2) {
                    let &[c, e] = s else { continue };
                    let (edge, den) = (e - c, dir.0 * (e - c).1 - dir.1 * (e - c).0);
                    if den.abs() > 1e-12 {
                        let t = ((c - a).0 * edge.1 - (c - a).1 * edge.0) / den;
                        let k = ((c - a).0 * dir.1 - (c - a).1 * dir.0) / den;
                        if t > 1e-9 && (-1e-9..=1.0 + 1e-9).contains(&k) && hit.is_none() {
                            hit = Some(d + k.clamp(0.0, 1.0) * c.dist(e));
                        }
                    }
                    d += c.dist(e);
                }
                hit?
            };
            if risers.last().is_some_and(|(_, last)| so <= *last + 1e-6) {
                return None;
            }
            risers.push((f * li, so));
        }
        Some(
            risers
                .windows(2)
                .filter_map(|pair| {
                    let &[(i0, o0), (i1, o1)] = pair else { return None };
                    let mut ring = vec![along(&inner, i0)];
                    ring.extend(corners(&inner, i0, i1));
                    ring.push(along(&inner, i1));
                    ring.push(along(&outer, o1));
                    ring.extend(corners(&outer, o0, o1).into_iter().rev());
                    ring.push(along(&outer, o0));
                    ring.dedup_by(|a, b| a.dist(*b) < 1e-6);
                    Some(ring.into_iter().map(|q| at(q.0, q.1)).collect())
                })
                .collect(),
        )
    }
}

/// The treads of a turn that winds: `count` of them fanned in equal angles round the turn's inside corner `p`,
/// through `sweep` degrees (90 or 180), filling the landing's place: `w` deep along `u`, and `w` out from `p` away
/// from `side` (and toward it too, when the stair turns back).
fn wind(p: P2, side: P2, u: P2, w: f64, sweep: f64, count: u32) -> Vec<Vec<P2>> {
    let at = |x: f64, y: f64| p + side * x + u * y;
    // Where the ray at `deg` from the first run's end line meets the landing's edge.
    let hit = |deg: f64| {
        let (s, c) = deg.to_radians().sin_cos();
        let t = (w / c.abs().max(1e-12)).min(w / s.abs().max(1e-12));
        at(-c * t, s * t)
    };
    let corners = [(45.0, at(-w, w)), (135.0, at(w, w))];
    (0..count)
        .map(|k| {
            let (a, b) = (sweep * f64::from(k) / f64::from(count), sweep * f64::from(k + 1) / f64::from(count));
            let mut ring = vec![p, hit(a)];
            ring.extend(corners.iter().filter(|(deg, _)| *deg > a + 1e-9 && *deg < b - 1e-9).map(|(_, c)| *c));
            ring.push(hit(b));
            ring
        })
        .collect()
}

fn ccw(ring: Vec<P2>) -> Vec<P2> {
    if geom::area(&ring) < 0.0 { ring.into_iter().rev().collect() } else { ring }
}

/// A stair's runs: its risers split at its landings, each run a tread shorter than its risers.
pub fn stair(m: &Model, st: &Stair) -> Layout {
    let (n, each, _) = m.flight(st);
    let base = m.elevation(st.level);
    // Each run from the riser it starts at to the one that climbs off it; winders climb some between two runs.
    let mut spans: Vec<(u32, u32)> = Vec::new();
    let mut from = 0u32;
    for l in &st.landings {
        let to = (l.at.max(0.0).min(f64::from(n))) as u32;
        // Winders balanced into the flights take treads of the run before them and of the one after.
        spans.push((from, to.saturating_sub(l.balance).max(from)));
        from = to.saturating_add(l.wound()).saturating_add(l.balance).min(n);
    }
    spans.push((from, n));
    let runs = spans
        .into_iter()
        .map(|(a, b)| {
            (
                f64::from(b.saturating_sub(a).saturating_sub(1)) * st.tread,
                (base + f64::from(a) * each, base + f64::from(b.max(a)) * each),
                b.saturating_sub(a),
            )
        })
        .collect();
    let turns: Vec<Turn> = st
        .landings
        .iter()
        .map(|l| Turn { turn: l.turn, winders: l.winders, length: l.length, into: f64::from(l.balance) * st.tread, balance: l.balance })
        .collect();
    chain(st.foot(), st.angle, st.width, runs, &turns, each)
}

/// `st` climbing the other way: its first riser where its top was, its runs, landings and winders the same ones met in
/// the other order, its top where its foot was. None for a stair with no run.
pub fn flipped(m: &Model, st: &Stair) -> Option<Stair> {
    let (n, _, _) = m.flight(st);
    let l = stair(m, st);
    let last = l.runs.last()?;
    let u = last.u * -1.0;
    let mut out = st.clone();
    out.angle = (u.1.atan2(u.0).to_degrees() * 1e9).round() / 1e9;
    out.offset = -st.offset + 0.0;
    out.line = match st.line {
        crate::model::StairLine::Left => crate::model::StairLine::Right,
        crate::model::StairLine::Right => crate::model::StairLine::Left,
        line => line,
    };
    out.landings = st
        .landings
        .iter()
        .rev()
        .map(|t| crate::model::Landing { at: f64::from(n) - t.at - f64::from(t.wound()), turn: -t.turn + 0.0, ..*t })
        .collect();
    // Its line is where it was: `start` is put so the middle of the first riser is the old top's.
    out.start = last.end();
    let off = out.foot() - out.start;
    out.start = crate::commands::micron(out.start - off);
    Some(out)
}

/// A handle on a stair, to drag: its foot (the whole stair moves), its top (it turns about its foot), the end of a run
/// before a turn (the run takes or gives risers), a side (its width, from its location line) or a landing's far edge
/// (the landing's length).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    Foot,
    Top,
    Turn(usize),
    Width(Side),
    Landing(usize),
}

/// Where each of a stair's grips is, in plan.
pub fn grips(m: &Model, st: &Stair) -> Vec<(Grip, P2)> {
    use crate::model::StairLine;
    let l = stair(m, st);
    let mut out = vec![(Grip::Foot, st.start)];
    let Some(first) = l.runs.first() else { return out };
    let (mid, n) = (first.start + first.u * (first.length / 2.0), first.u.perp() * (st.width / 2.0));
    // A side that is not the location line is dragged for the width.
    if st.line != StairLine::Left {
        out.push((Grip::Width(Side::Left), mid + n));
    }
    if st.line != StairLine::Right {
        out.push((Grip::Width(Side::Right), mid - n));
    }
    for (i, (run, t)) in l.runs.iter().zip(&st.landings).enumerate() {
        out.push((Grip::Turn(i), run.end()));
        if t.winders == 0 {
            let deep = if t.length > 0.0 { if t.turn.abs() == 90.0 { t.length.max(st.width) } else { t.length } } else { st.width };
            let side = run.u.perp() * if t.turn > 0.0 { 1.0 } else { -1.0 };
            let across = if t.turn.abs() == 180.0 { side * (st.width / 2.0) } else { P2(0.0, 0.0) };
            out.push((Grip::Landing(i), run.end() + run.u * deep + across));
        }
    }
    if let Some(last) = l.runs.last() {
        out.push((Grip::Top, last.end()));
    }
    out
}

/// `st` as it is with `grip` dragged to `to`. A run's end takes the whole risers that fit, and the landings after it
/// keep their runs; a stair that cannot be (a run of one riser) is for `Model::check` to refuse.
pub fn dragged(m: &Model, st: &Stair, grip: Grip, to: P2) -> Stair {
    use crate::model::StairLine;
    let mut out = st.clone();
    let l = stair(m, st);
    let whole = |x: f64| (x * 1000.0).round() / 1000.0 + 0.0;
    match grip {
        Grip::Foot => out.start = P2(whole(to.0), whole(to.1)),
        Grip::Top => {
            let Some(top) = l.runs.last().map(Run::end) else { return out };
            let (was, now) = (top - st.start, to - st.start);
            if was.length() > 1e-6 && now.length() > 1e-6 {
                let turn = now.1.atan2(now.0).to_degrees() - was.1.atan2(was.0).to_degrees();
                out.angle = whole((st.angle + turn + 540.0).rem_euclid(360.0) - 180.0);
            }
        }
        Grip::Turn(i) => {
            let (Some(run), Some(t)) = (l.runs.get(i), st.landings.get(i)) else { return out };
            let from = t.at - f64::from(run.risers);
            let risers = if st.tread > 0.0 { ((to - run.start).dot(run.u) / st.tread).round() + 1.0 } else { f64::from(run.risers) };
            let by = (from + risers.max(2.0)) - t.at;
            for t in out.landings.iter_mut().skip(i) {
                t.at += by;
            }
        }
        Grip::Width(_) => {
            let n = heading(st.angle).perp();
            let d = (to - (st.start + n * st.offset)).dot(n);
            let w = match st.line {
                StairLine::Centre => 2.0 * d.abs(),
                StairLine::Left => -d,
                StairLine::Right => d,
            };
            out.width = whole(w.max(100.0));
        }
        Grip::Landing(i) => {
            let (Some(run), Some(t)) = (l.runs.get(i), out.landings.get_mut(i)) else { return out };
            t.length = whole((to - run.end()).dot(run.u).max(100.0));
        }
    }
    out
}

/// A ramp's runs: its rise split at its landings' heights, each run as long as its slope makes it.
pub fn ramp(m: &Model, r: &Ramp) -> Layout {
    let (base, top) = m.span(r.level, 0.0, r.top_level, r.height);
    let k = if r.slope > 0.0 { r.slope / 100.0 } else { 1.0 };
    let mut ups: Vec<f64> = vec![0.0];
    ups.extend(r.landings.iter().map(|l| l.at));
    ups.push(top - base);
    let runs = ups
        .windows(2)
        .filter_map(|w| match *w {
            [a, b] => Some(((b - a) / k, (base + a, base + b), 0)),
            _ => None,
        })
        .collect();
    let turns: Vec<Turn> = r.landings.iter().map(|l| Turn { turn: l.turn, length: l.length, ..Turn::default() }).collect();
    chain(r.start, r.angle, r.width, runs, &turns, 0.0)
}

/// Element `e`'s layout, a stair's or a ramp's.
pub fn layout(m: &Model, e: &Element) -> Option<Layout> {
    match e {
        Element::Stair(st) => Some(stair(m, st)),
        Element::Ramp(r) => Some(ramp(m, r)),
        _ => None,
    }
}

/// The line a railing follows, in plan, with the height of what it runs along at each point: on a
/// stair or a ramp, its walk line moved out to the side, [`RAILING_INSET`] in from its edge; else
/// its path, on its level and `base_offset` above it.
pub fn railing(m: &Model, rl: &Railing) -> Vec<(P2, f64)> {
    let host = rl.host.and_then(|h| m.elements.get(h)).and_then(|e| layout(m, e));
    match (host, rl.side) {
        (Some(l), Some(side)) => {
            let d = (l.width / 2.0 - RAILING_INSET).max(0.0) * if side == Side::Left { 1.0 } else { -1.0 };
            offset(&l.walk, d)
        }
        _ => {
            let z = m.elevation(rl.level) + rl.base_offset;
            rl.path.iter().map(|p| (*p, z)).collect()
        }
    }
}

/// A railing's solids, each a ring in plan between two planes: its rail along each stretch of its
/// line, 50 mm square, its type's height over the line; and its balusters, 20 mm square and its
/// type's `spacing` apart, from the line up under the rail.
pub fn railing_parts(m: &Model, rl: &Railing) -> Vec<(Vec<P2>, Plane, Plane)> {
    let Some(t) = m.types.railing.get(&rl.type_name) else { return Vec::new() };
    let mut out = Vec::new();
    // How far along the next stretch the next baluster stands.
    let mut next = 0.0;
    for w in railing(m, rl).windows(2) {
        let &[(p, zp), (q, zq)] = w else { continue };
        let len = p.dist(q);
        if len < 1e-9 {
            continue;
        }
        let u = (q - p) * (1.0 / len);
        let (n, g) = (u.perp() * 25.0, u * ((zq - zp) / len));
        let top = Plane { z: zp + t.height - g.dot(p), g };
        out.push((vec![p - n, q - n, q + n, p + n], Plane { z: top.z - 50.0, g }, top));
        if t.spacing > 0.0 {
            while next <= len {
                let (at, z) = (p + u * next, zp + (zq - zp) * next / len);
                let (a, b) = (u * 10.0, u.perp() * 10.0);
                out.push((vec![at - a - b, at + a - b, at + a + b, at - a + b], Plane::flat(z), Plane::flat(z + t.height - 50.0)));
                next += t.spacing;
            }
            next -= len;
        }
    }
    out
}

/// `line` moved `d` to its left, its corners mitred, each point keeping its height.
fn offset(line: &[(P2, f64)], d: f64) -> Vec<(P2, f64)> {
    let mut pts: Vec<(P2, f64)> = line.to_vec();
    pts.dedup_by(|b, a| a.0.dist(b.0) < 1e-9);
    let dirs: Vec<P2> = pts.windows(2).filter_map(|w| Some((w.get(1)?.0 - w.first()?.0).unit())).collect();
    pts.iter()
        .enumerate()
        .map(|(i, &(p, z))| {
            let before = i.checked_sub(1).and_then(|j| dirs.get(j)).copied();
            let after = dirs.get(i).copied();
            let shift = match (before, after) {
                (Some(a), Some(b)) if a.dot(b) > -1.0 + 1e-9 => (a.perp() + b.perp()) * (d / (1.0 + a.dot(b))),
                (Some(a), _) | (None, Some(a)) => a.perp() * d,
                (None, None) => P2(0.0, 0.0),
            };
            (p + shift, z)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_moved_aside_keeps_its_corners_square() {
        let line = [(P2(0.0, 0.0), 0.0), (P2(0.0, 1000.0), 500.0), (P2(1000.0, 1000.0), 500.0)];
        // Going north, then east: to the left (west, then north), the corner moves out diagonally.
        let left = offset(&line, 100.0);
        assert_eq!(left.iter().map(|(p, _)| *p).collect::<Vec<_>>(), [P2(-100.0, 0.0), P2(-100.0, 1100.0), P2(1000.0, 1100.0)]);
        assert_eq!(left.iter().map(|(_, z)| *z).collect::<Vec<_>>(), [0.0, 500.0, 500.0]);
        let right = offset(&line, -100.0);
        assert_eq!(right[1].0, P2(100.0, 900.0), "the inside corner moves in");
    }

    #[test]
    fn landings_turn_the_next_run_square_or_back() {
        let runs = vec![(1000.0, (0.0, 500.0), 0), (1000.0, (500.0, 1000.0), 0)];
        let near = |a: P2, b: P2| a.dist(b) < 1e-9;
        let l = chain(P2(0.0, 0.0), 90.0, 1000.0, runs.clone(), &[Turn { turn: -90.0, ..Turn::default() }], 0.0);
        // North, then a metre square landing, then east from its middle.
        assert!(near(l.runs[1].start, P2(500.0, 1500.0)) && near(l.runs[1].u, P2(1.0, 0.0)), "{:?}", l.runs[1]);
        assert!((geom::area(&l.pads[0].ring) - 1e6).abs() < 1e-6);
        let back = chain(P2(0.0, 0.0), 90.0, 1000.0, runs.clone(), &[Turn { turn: 180.0, ..Turn::default() }], 0.0);
        assert!(near(back.runs[1].start, P2(-1000.0, 1000.0)) && near(back.runs[1].u, P2(0.0, -1.0)), "{:?}", back.runs[1]);
        assert!((geom::area(&back.pads[0].ring) - 2e6).abs() < 1e-6, "across both runs");
        let on = chain(P2(0.0, 0.0), 90.0, 1000.0, runs, &[Turn::default()], 0.0);
        assert!(near(on.runs[1].start, P2(0.0, 2000.0)));
        assert_eq!(on.walk.len(), 4);
    }
}
