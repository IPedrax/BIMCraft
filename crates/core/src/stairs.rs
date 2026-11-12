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
    /// A tread (a winder, a spiral's), not a landing.
    pub tread: bool,
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
        } else if turn.abs() == 180.0 {
            let ring = vec![e - side * (w / 2.0), e - side * (w / 2.0) + u * d, e + side * (1.5 * w) + u * d, e + side * (1.5 * w)];
            (ring, vec![e + u * (d / 2.0), e + u * (d / 2.0) + side * w], e + side * w, u * -1.0)
        } else {
            // Any other angle: the two runs share the turn's inside corner, and the landing reaches to where their
            // outside edges meet.
            let (s, c) = turn.abs().to_radians().sin_cos();
            let (p, reach) = (e + side * (w / 2.0), (turn.abs().to_radians() / 2.0).tan());
            let (dir, out2) = (u * c + side * s, u * s - side * c);
            let ring = vec![p, p - side * w, p - side * w + u * (w * reach), p + out2 * w];
            (ring, vec![p + (u * reach - side) * (w / 2.0)], p + out2 * (w / 2.0), dir)
        };
        if winders > 0 && turn != 0.0 {
            let p = e + side * (w / 2.0);
            let rings =
                balanced(p, side, u, w, turn.abs(), winders, into, balance).unwrap_or_else(|| wind(p, side, u, w, turn.abs(), winders));
            out.winders += rings.len();
            // The walk line climbs through the turn: on into it, round the inside corner at the stair's middle, and on out.
            let top = (rings.len() as f64 - 1.0) * each;
            out.pads.extend(rings.into_iter().enumerate().map(|(k, ring)| Pad { ring: ccw(ring), z: z.1 + k as f64 * each, tread: true }));
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
            out.pads.push(Pad { ring: ccw(ring), z: z.1, tread: false });
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
    // The way the next run goes, and the way out across it, in that frame.
    let (sn, cs) = sweep.to_radians().sin_cos();
    let (dir, out2) = ((sn, cs), (-cs, sn));
    let reach = w * (sweep.to_radians() / 2.0).tan();
    let inner =
        if back { vec![(0.0, -into), (0.0, 0.0), (0.0, -into)] } else { vec![(0.0, -into), (0.0, 0.0), (dir.0 * into, dir.1 * into)] };
    let outer = if back {
        vec![(-w, -into), (-w, w), (w, w), (w, -into)]
    } else {
        vec![(-w, -into), (-w, reach), (out2.0 * w + dir.0 * into, out2.1 * w + dir.1 * into)]
    };
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
            let past = d - into - arc;
            P2(out2.0 * r + dir.0 * past, out2.1 * r + dir.1 * past)
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
    let back = sweep == 180.0;
    let hit = |deg: f64| {
        let (s, c) = deg.to_radians().sin_cos();
        // Back beside the run, the landing is a rectangle; else it ends at the next run's outside edge.
        let t = if back {
            (w / c.abs().max(1e-12)).min(w / s.abs().max(1e-12))
        } else {
            (w / c.max(1e-12)).min(w / (sweep - deg).to_radians().cos().max(1e-12))
        };
        at(-c * t, s * t)
    };
    let reach = w * (sweep.to_radians() / 2.0).tan();
    let corners = if back { vec![(45.0, at(-w, w)), (135.0, at(w, w))] } else { vec![(sweep / 2.0, at(-w, reach))] };
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

/// The angle each tread of spiral `sp` turns through, in radians, on a stair of `n` risers.
pub fn spiral_step(st: &Stair, sp: crate::model::Spiral, n: u32) -> f64 {
    let treads = f64::from(n.saturating_sub(1)).max(1.0);
    if sp.sweep > 0.0 { sp.sweep.to_radians() / treads } else { st.tread / sp.radius.abs().max(1.0) }
}

/// A spiral stair: no runs, a tread for each riser but the last, each a piece of the ring about its centre, a riser
/// above the one before; the walk line round its middle.
fn spiral(st: &Stair, sp: crate::model::Spiral, n: u32, each: f64, base: f64) -> Layout {
    let mut out = Layout { runs: Vec::new(), pads: Vec::new(), walk: Vec::new(), width: st.width, winders: 0 };
    let (foot, side, r) = (st.foot(), if sp.radius < 0.0 { -1.0 } else { 1.0 }, sp.radius.abs());
    let centre = foot + heading(st.angle).perp() * (side * r);
    let out0 = (foot - centre).unit();
    // The point `radius` from the centre, `a` radians round from the first riser.
    let at = |radius: f64, a: f64| {
        let (s, c) = (side * a).sin_cos();
        centre + P2(out0.0 * c - out0.1 * s, out0.0 * s + out0.1 * c) * radius
    };
    let treads = n.saturating_sub(1).min(2000);
    let step = spiral_step(st, sp, n);
    let (inner, outer) = ((r - st.width / 2.0).max(0.0), r + st.width / 2.0);
    let parts = ((step.to_degrees() / 10.0).ceil() as u32).clamp(1, 36);
    for k in 0..treads {
        let (a, b) = (f64::from(k) * step, f64::from(k + 1) * step);
        let angles: Vec<f64> = (0..=parts).map(|i| a + (b - a) * f64::from(i) / f64::from(parts)).collect();
        let mut ring: Vec<P2> = angles.iter().map(|x| at(outer, *x)).collect();
        if inner > 1e-6 {
            ring.extend(angles.iter().rev().map(|x| at(inner, *x)));
        } else {
            ring.push(centre);
        }
        out.pads.push(Pad { ring: ccw(ring), z: base + f64::from(k + 1) * each, tread: true });
    }
    out.winders = out.pads.len();
    let (end, rise) = (f64::from(treads) * step, f64::from(n) * each);
    let points = (treads * parts).max(1);
    out.walk = (0..=points)
        .map(|i| {
            let f = f64::from(i) / f64::from(points);
            (at(r, end * f), base + rise * f)
        })
        .collect();
    out
}

/// A stair's runs: its risers split at its landings, each run a tread shorter than its risers.
pub fn stair(m: &Model, st: &Stair) -> Layout {
    let mut all = storeys(m, st).into_iter().map(|(_, l)| l);
    let Some(mut out) = all.next() else { return first(m, st) };
    // The storeys above, one after another.
    for l in all {
        out.runs.extend(l.runs);
        out.pads.extend(l.pads);
        out.walk.extend(l.walk);
        out.winders += l.winders;
    }
    out
}

/// The layout of a stair's first storey, which is the stair as it is drawn and edited.
pub fn first(m: &Model, st: &Stair) -> Layout {
    let (n, each, _) = m.flight(st);
    laid(st, &st.landings, n, each, m.elevation(st.level))
}

/// Each storey of a stair, lowest first: the level it stands on and its layout there. A storey higher or lower than the
/// first has the risers its height asks, and the first's landings moved with them: after the same share of its risers,
/// where every run still holds two; else that storey has one flight.
pub fn storeys(m: &Model, st: &Stair) -> Vec<(crate::model::Guid, Layout)> {
    let (n0, _, _) = m.flight(st);
    m.storeys(st)
        .into_iter()
        .enumerate()
        .map(|(i, (level, base, rise))| {
            let (n, each, _) = m.flight_of(st, rise);
            if i == 0 || n == n0 {
                return (level, laid(st, &st.landings, n, each, base));
            }
            let scale = if n0 > 0 { f64::from(n) / f64::from(n0) } else { 1.0 };
            let moved: Vec<crate::model::Landing> =
                st.landings.iter().map(|l| crate::model::Landing { at: (l.at * scale).round(), ..*l }).collect();
            let mut last = 0.0;
            let holds = moved.iter().all(|l| {
                let (wound, into) = (f64::from(l.wound()), f64::from(l.balance));
                let ok = l.at - into >= last + 2.0 && l.at + wound + into <= f64::from(n) - 2.0;
                last = l.at + wound + into;
                ok
            });
            (level, laid(st, if holds { &moved } else { &[] }, n, each, base))
        })
        .collect()
}

/// A stair's layout over one storey: `n` risers each `each` high from `base`, with `landings`.
fn laid(st: &Stair, landings: &[crate::model::Landing], n: u32, each: f64, base: f64) -> Layout {
    if let Some(sp) = st.spiral {
        return spiral(st, sp, n, each, base);
    }
    // Each run from the riser it starts at to the one that climbs off it; winders climb some between two runs.
    let mut spans: Vec<(u32, u32)> = Vec::new();
    let mut from = 0u32;
    for l in landings {
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
    let turns: Vec<Turn> = landings
        .iter()
        .map(|l| Turn { turn: l.turn, winders: l.winders, length: l.length, into: f64::from(l.balance) * st.tread, balance: l.balance })
        .collect();
    chain(st.foot(), st.angle, st.width, runs, &turns, each)
}

/// `st` climbing the other way: its first riser where its top was, its runs, landings and winders the same ones met in
/// the other order, its top where its foot was. None for a stair with no run.
pub fn flipped(m: &Model, st: &Stair) -> Option<Stair> {
    let (n, _, _) = m.flight(st);
    let l = first(m, st);
    if let Some(sp) = st.spiral {
        // From its top round the other way: the centre on its other side.
        let (&(top, _), &(before, _)) = (l.walk.last()?, l.walk.get(l.walk.len().checked_sub(2)?)?);
        let centre = st.foot() + heading(st.angle).perp() * sp.radius;
        let back = (centre - top).unit().perp() * if sp.radius < 0.0 { -1.0 } else { 1.0 };
        let _ = before;
        let mut out = st.clone();
        out.angle = (back.1.atan2(back.0).to_degrees() * 1e9).round() / 1e9;
        out.spiral = Some(crate::model::Spiral { radius: -sp.radius, sweep: sp.sweep });
        out.offset = -st.offset + 0.0;
        out.line = match st.line {
            crate::model::StairLine::Left => crate::model::StairLine::Right,
            crate::model::StairLine::Right => crate::model::StairLine::Left,
            line => line,
        };
        out.start = top;
        let off = out.foot() - out.start;
        out.start = crate::commands::micron(out.start - off);
        return Some(out);
    }
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
    let l = first(m, st);
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
        if t.winders == 0 && matches!(t.turn.abs(), 0.0 | 90.0 | 180.0) {
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
    let l = first(m, st);
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

/// A stair's solids as its type builds it, each a ring in plan between two planes; none for a stair of no type, which is
/// solid down to its level. With a waist: each tread from its top down to the slab's sloping underside, and each landing
/// that thick. With none: treads standing free, risers between them where the type has any, and a stringer down each
/// side of each run where it has those.
pub fn parts(m: &Model, st: &Stair) -> Option<Vec<(Vec<P2>, Plane, Plane)>> {
    let t = m.types.stair.get(&st.type_name)?;
    let (_, first_each, _) = m.flight(st);
    let l = stair(m, st);
    let (half, base) = (st.width / 2.0, m.elevation(st.level));
    // The heights its storeys stand at: a run from one of them stands on that floor.
    let floors: Vec<f64> = m.storeys(st).into_iter().map(|s| s.1).collect();
    let mut out: Vec<(Vec<P2>, Plane, Plane)> = Vec::new();
    let open = t.waist <= 0.0;
    // How thick a landing or a winder is.
    let flat = if open { t.tread_thickness.max(1.0) } else { t.waist };
    for run in &l.runs {
        // Each riser of this run, which a storey of its own height makes its own; and how far a sloping slab reaches
        // down under the corner of a step.
        let each = if run.risers > 0 { (run.z.1 - run.z.0) / f64::from(run.risers) } else { first_each };
        let slope = if st.tread > 0.0 { each / st.tread } else { 0.0 };
        let down = t.waist * (1.0 + slope * slope).sqrt();
        let (u, n) = (run.u, run.u.perp());
        let rect = |a: f64, b: f64, c: f64, d: f64| {
            ccw(vec![run.start + u * a + n * c, run.start + u * b + n * c, run.start + u * b + n * d, run.start + u * a + n * d])
        };
        // A plane sloping with the run, `z` high at its foot.
        let sloping = |z: f64| {
            let g = u * slope;
            Plane { z: z - g.dot(run.start), g }
        };
        // A piece under a sloping plane that would dip below `floor` near the foot: flat-bottomed there.
        let under = |out: &mut Vec<(Vec<P2>, Plane, Plane)>, a: f64, b: f64, c: f64, d: f64, bottom: f64, floor: f64, top: Plane| {
            // Where the sloping bottom, `bottom` high at the foot, comes up to `floor`.
            let cross = if slope > 0.0 { (floor - bottom) / slope } else { f64::INFINITY };
            if cross > a + 1e-6 {
                out.push((rect(a, b.min(cross), c, d), Plane::flat(floor), top));
            }
            if cross < b - 1e-6 {
                out.push((rect(a.max(cross), b, c, d), sloping(bottom), top));
            }
        };
        // A run from the floor stands on it; one from a landing ends level with the landing's underside.
        let floor = if floors.iter().any(|z| (run.z.0 - z).abs() < 1e-6) { run.z.0 } else { run.z.0 - flat };
        for k in 1..run.risers {
            let k = f64::from(k);
            let (a, b, top) = ((k - 1.0) * st.tread, k * st.tread, run.z.0 + k * each);
            if open {
                // The tread, out over the one below by its nosing; and the riser under its front edge.
                out.push((rect(a - t.nosing, b, -half, half), Plane::flat(top - t.tread_thickness), Plane::flat(top)));
                if t.riser_thickness > 0.0 {
                    let from = if k > 1.0 { top - each } else { run.z.0 };
                    out.push((rect(a, a + t.riser_thickness, -half, half), Plane::flat(from), Plane::flat(top - t.tread_thickness)));
                }
            } else {
                under(&mut out, a, b, -half, half, run.z.0 - down, floor, Plane::flat(top));
            }
        }
        if open {
            // The last riser, up to what the run climbs onto.
            if t.riser_thickness > 0.0 && run.risers > 0 {
                let from = if run.risers > 1 { run.z.1 - each } else { run.z.0 };
                out.push((rect(run.length, run.length + t.riser_thickness, -half, half), Plane::flat(from), Plane::flat(run.z.1 - flat)));
            }
            // A stringer down each side: its top 50 over the nosings' line, its underside its depth below that.
            if t.stringer_width > 0.0 && t.stringer_depth > 0.0 && run.length > 0.0 {
                let top = run.z.0 + each + 50.0;
                for (c, d) in [(-half, -half + t.stringer_width), (half - t.stringer_width, half)] {
                    under(&mut out, 0.0, run.length, c, d, top - t.stringer_depth, floor, sloping(top));
                }
            }
        }
    }
    for pad in &l.pads {
        // A winder is a step: with a slab under it, the slab is under the step below it too.
        let thick = if pad.tread && !open { first_each + t.waist } else { flat };
        out.push((pad.ring.clone(), Plane::flat((pad.z - thick).max(base)), Plane::flat(pad.z)));
    }
    Some(out)
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
