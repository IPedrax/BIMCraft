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
    /// How wide it is.
    pub width: f64,
}

impl Run {
    pub fn end(&self) -> P2 {
        self.start + self.u * self.length
    }

    /// How deep each of its treads is: its length shared among them (a tread fewer than its risers).
    pub fn going(&self) -> f64 {
        if self.risers > 1 { self.length / f64::from(self.risers - 1) } else { 0.0 }
    }

    /// Its outline, counter-clockwise.
    pub fn ring(&self) -> Vec<P2> {
        let n = self.u.perp() * (self.width / 2.0);
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
    /// How many of `pads` each turn between two runs has, in order: one for a landing, its treads for one that winds.
    pub turns: Vec<usize>,
    /// The risers that are no run's (round an arc, a spiral's, where two flights meet): where a finish's nosing goes.
    pub nosings: Vec<Nosing>,
}

/// A riser that is no run's: its line in plan from `a` to `b`, the way down the stair there (`out`, square to it), the
/// height of the tread over it and how high it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Nosing {
    pub a: P2,
    pub b: P2,
    pub out: P2,
    pub z: f64,
    pub rise: f64,
}

/// Runs of `(length, heights, risers)` from `start` toward `angle` degrees, `width` wide, and a
/// landing after each but the last, as deep as they're wide, the next run going on as `turns` say:
/// straight on (0), left or right (90, -90), or back beside it, to its left or right (180, -180). A turn of
/// `(turn, winders)` with winders has no landing: that many treads fanned round its inside corner, `each` apart
/// in height, the first where the landing would be.
fn chain(start: P2, angle: f64, width: f64, runs: Vec<(f64, (f64, f64), u32)>, turns: &[Turn], each: f64) -> Layout {
    let (mut at, mut u) = (start, heading(angle));
    let mut out =
        Layout { runs: Vec::new(), pads: Vec::new(), walk: Vec::new(), width, winders: 0, turns: Vec::new(), nosings: Vec::new() };
    // How wide the run being laid is: the stair's, until a landing gives the next a width of its own.
    let mut width = width;
    for (i, (length, z, risers)) in runs.into_iter().enumerate() {
        let run = Run { start: at, u, length, z, risers, width };
        out.walk.extend([(run.start, z.0), (run.end(), z.1)]);
        out.runs.push(run);
        let Some(&Turn { turn, winders, length: deep, into, balance, width: next_width }) = turns.get(i) else { break };
        // The run after the turn: as wide as this one unless the turn says.
        let w2 = if next_width > 0.0 { next_width } else { width };
        // Winders balanced into the flights start before the corner and end past it.
        let (e, n, w) = (run.end() + u * into, u.perp(), width);
        // How deep the landing is: as the stair is wide unless it says, and no less where the next run leaves its side.
        let d = if winders > 0 {
            w
        } else if turn.abs() == 90.0 {
            // No less deep than the run that leaves its side is wide.
            if deep > 0.0 { deep.max(w2) } else { w2 }
        } else if deep > 0.0 {
            deep
        } else {
            w
        };
        // Across the landing from the run's right edge to its left, and how far it reaches past the
        // run: to the next run's far edge, beside it when it turns back.
        let side = n * if turn > 0.0 { 1.0 } else { -1.0 };
        let (ring, via, next, dir) = if turn == 0.0 {
            // As wide as the wider of the two runs.
            let h = w.max(w2) / 2.0;
            (vec![e - n * h, e - n * h + u * d, e + n * h + u * d, e + n * h], vec![], e + u * d, u)
        } else if turn.abs() == 90.0 {
            let ring = vec![e - n * (w / 2.0), e - n * (w / 2.0) + u * d, e + n * (w / 2.0) + u * d, e + n * (w / 2.0)];
            (ring, vec![e + u * (d - w2 / 2.0)], e + u * (d - w2 / 2.0) + side * (w / 2.0), side)
        } else if turn.abs() == 180.0 {
            // Across both runs, the next beside this one.
            let far = w / 2.0 + w2;
            let ring = vec![e - side * (w / 2.0), e - side * (w / 2.0) + u * d, e + side * far + u * d, e + side * far];
            let beside = (w + w2) / 2.0;
            (ring, vec![e + u * (d / 2.0), e + u * (d / 2.0) + side * beside], e + side * beside, u * -1.0)
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
            out.turns.push(rings.len());
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
            out.turns.push(1);
            out.pads.push(Pad { ring: ccw(ring), z: z.1, tread: false });
            out.walk.extend(via.into_iter().map(|p| (p, z.1)));
        }
        (at, u, width) = (next, dir, w2);
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
    width: f64,
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

/// A segment of the middle line of a stair drawn along a path: from `a` to `b`, straight or round `arc`'s centre.
#[derive(Clone, Copy, Debug)]
struct Piece {
    a: P2,
    b: P2,
    landing: bool,
    winders: u32,
    /// Its centre, its radius, the angle `a` is at from the centre, and the angle it turns through (radians, signed).
    arc: Option<(P2, f64, f64, f64)>,
    /// How wide it is, and the segments of the path it is drawn from (the first and the last).
    width: f64,
    segs: (usize, usize),
}

impl Piece {
    fn length(&self) -> f64 {
        self.arc.map_or_else(|| self.a.dist(self.b), |(_, r, _, sweep)| r * sweep.abs())
    }

    /// The point `d` along it.
    fn at(&self, d: f64) -> P2 {
        match self.arc {
            Some((c, r, a0, sweep)) => {
                let (s, co) = (a0 + sweep.signum() * d / r).sin_cos();
                c + P2(co, s) * r
            }
            None => self.a + (self.b - self.a).unit() * d,
        }
    }

    /// The way it goes `d` along it.
    fn dir(&self, d: f64) -> P2 {
        match self.arc {
            Some((_, r, a0, sweep)) => {
                let (s, co) = (a0 + sweep.signum() * d / r).sin_cos();
                P2(co, s).perp() * sweep.signum()
            }
            None => (self.b - self.a).unit(),
        }
    }
}

/// The middle line of a stair drawn along a path, piece by piece: its path moved to the side its location line says,
/// each segment by half its own width, mitred where two meet (an arc moved so stays an arc about the same centre,
/// through the same angle); two flights in one line, as wide, made one.
fn pieces(st: &Stair) -> Vec<Piece> {
    use crate::model::StairLine;
    let side = match st.line {
        StairLine::Left => -1.0,
        StairLine::Centre => 0.0,
        StairLine::Right => 1.0,
    };
    let points: Vec<P2> = std::iter::once(st.start).chain(st.path.iter().map(|s| s.to)).collect();
    let wide = |i: usize| st.path.get(i).and_then(|s| s.width).unwrap_or(st.width);
    let shift = |i: usize| side * wide(i) / 2.0 + st.offset;
    let rot = |v: P2, deg: f64| {
        let (s, c) = deg.to_radians().sin_cos();
        P2(v.0 * c - v.1 * s, v.0 * s + v.1 * c)
    };
    // Each segment's way at its start and at its end.
    let ways: Vec<Option<(P2, P2)>> = (st.path.iter().zip(points.windows(2)))
        .map(|(seg, w)| {
            let (a, b) = (*w.first()?, *w.get(1)?);
            (a.dist(b) > 1e-6).then(|| {
                let chord = (b - a).unit();
                (rot(chord, -seg.arc / 2.0), rot(chord, seg.arc / 2.0))
            })
        })
        .collect();
    let way = |i: usize| ways.get(i).copied().flatten();
    // Where segment `i` ends and `i + 1` starts, each moved aside by its own shift: where the two lines meet, or across
    // the step between them where they run on in one line.
    let join = |i: usize| -> (P2, P2) {
        let p = points.get(i + 1).copied().unwrap_or(st.start);
        let (s1, s2) = (shift(i), shift(i + 1));
        match (way(i).map(|w| w.1), way(i + 1).map(|w| w.0)) {
            (Some(a), Some(b)) => {
                let cross = a.0 * b.1 - a.1 * b.0;
                if cross.abs() < 1e-9 {
                    (p + a.perp() * s1, p + b.perp() * s2)
                } else {
                    let r = b.perp() * s2 - a.perp() * s1;
                    let t = (r.0 * b.1 - r.1 * b.0) / cross;
                    let q = p + a.perp() * s1 + a * t;
                    (q, q)
                }
            }
            (Some(a), None) => (p + a.perp() * s1, p + a.perp() * s1),
            (None, Some(b)) => (p + b.perp() * s2, p + b.perp() * s2),
            (None, None) => (p, p),
        }
    };
    let mut out: Vec<Piece> = Vec::new();
    let count = st.path.len();
    for (i, seg) in st.path.iter().enumerate() {
        let (Some(&a0), Some(&b0)) = (points.get(i), points.get(i + 1)) else { continue };
        let Some((w0, w1)) = way(i) else { continue };
        let a = if i == 0 { a0 + w0.perp() * shift(0) } else { join(i - 1).1 };
        let b = if i + 1 == count { b0 + w1.perp() * shift(i) } else { join(i).0 };
        let (a, b) = if a.is_finite() && b.is_finite() { (a, b) } else { (a0, b0) };
        let chord = a.dist(b);
        if chord < 1e-6 {
            continue;
        }
        let arc = (seg.arc != 0.0).then(|| {
            let sweep = seg.arc.to_radians();
            let r = chord / (2.0 * (sweep.abs() / 2.0).sin()).max(1e-9);
            let centre = (a + b) * 0.5 + (b - a).unit().perp() * (chord / 2.0 / (sweep / 2.0).tan());
            (centre, r, (a - centre).1.atan2((a - centre).0), sweep)
        });
        let piece = Piece { a, b, landing: seg.landing, winders: seg.winders, arc, width: wide(i), segs: (i, i) };
        // A flight straight on from the flight before it, as wide, is the same flight.
        if let Some(last) = out.last_mut()
            && !last.landing
            && !piece.landing
            && last.arc.is_none()
            && piece.arc.is_none()
            && (last.width - piece.width).abs() < 1e-6
            && last.dir(0.0).dot(piece.dir(0.0)) > 0.999_99
        {
            last.b = piece.b;
            last.segs.1 = i;
            continue;
        }
        out.push(piece);
    }
    out
}

/// A part of a stair drawn along a path, as it is picked and edited on its own: a flight or a landing, the segments of
/// the path it is drawn from (the first and the last: a flight drawn in two strokes in one line is one), how wide it is,
/// and its outline in plan (its middle line moved half its width to each side).
#[derive(Clone, Debug, PartialEq)]
pub struct StairPart {
    pub landing: bool,
    pub segs: (usize, usize),
    pub width: f64,
    pub ring: Vec<P2>,
}

/// The flights and landings of a stair drawn along a path, from its foot up.
pub fn stair_parts(st: &Stair) -> Vec<StairPart> {
    pieces(st)
        .into_iter()
        .map(|p| {
            let len = p.length();
            let steps = if p.arc.is_some() { 24 } else { 1 };
            let line: Vec<(P2, f64)> = (0..=steps).map(|k| (p.at(len * f64::from(k) / f64::from(steps)), 0.0)).collect();
            let mut ring: Vec<P2> = offset(&line, p.width / 2.0).into_iter().map(|q| q.0).collect();
            ring.extend(offset(&line, -p.width / 2.0).into_iter().rev().map(|q| q.0));
            StairPart { landing: p.landing, segs: p.segs, width: p.width, ring: ccw(ring) }
        })
        .collect()
}

/// The part of a stair drawn along a path that `at` is on: of those whose outline holds it, the one whose middle is
/// nearest (a corner is on two).
pub fn part_at(st: &Stair, at: P2) -> Option<usize> {
    let parts = stair_parts(st);
    (0..parts.len()).filter(|i| parts.get(*i).is_some_and(|p| geom::contains(&p.ring, at))).min_by(|a, b| {
        let far = |i: &usize| parts.get(*i).map_or(f64::INFINITY, |p| geom::centroid(&p.ring).dist(at));
        far(a).total_cmp(&far(b))
    })
}

/// What the pieces of a path come to on a stair of `n` risers: how far in from each piece's ends the corners with its
/// neighbours take (`lo`, `hi` along it), how many risers each flight has (a tread every `tread` along what is left of
/// it, the last flight the risers left over), how many pieces are used before the risers run out, and whether the
/// flights as drawn hold all the risers.
struct Allotted {
    usable: Vec<(f64, f64)>,
    risers: Vec<u32>,
    /// How deep each flight's treads are: what fits its length, inside the range.
    goings: Vec<f64>,
    /// A flight going straight on into the next flight, no landing or corner between: its last tread is the step onto
    /// the next one's first riser, so it has a riser fewer than its treads and the next goes on from where it ends.
    chained: Vec<bool>,
    used: usize,
    full: bool,
}

/// How deep a stair's treads may be: what they are unless a flight's length asks otherwise, and the least and the most a
/// flight may make them to end where it was drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Goings {
    pub usual: f64,
    pub min: f64,
    pub max: f64,
}

impl Goings {
    /// Treads of one depth, whatever a flight's length.
    pub fn fixed(tread: f64) -> Goings {
        Goings { usual: tread, min: tread, max: tread }
    }

    /// The treads a flight `length` long has, and how deep each is: as near the usual depth as shares the length
    /// evenly, inside the range; a length the range cannot share leaves a little over.
    fn share(&self, length: f64) -> (u32, f64) {
        if self.usual <= 0.0 || length <= 0.0 {
            return (1, self.usual.max(0.0));
        }
        let (min, max) = (self.min.min(self.usual).max(1.0), self.max.max(self.usual));
        let mut treads = (length / self.usual).round().max(1.0);
        if length / treads > max + 1e-9 {
            treads = (length / max - 1e-9).ceil().max(1.0);
        }
        if length / treads < min - 1e-9 {
            treads = (length / min + 1e-9).floor().max(1.0);
        }
        (treads.clamp(1.0, 1e6) as u32, (length / treads).clamp(min, max))
    }
}

/// How far a path turns from piece `i` into the next, in degrees, to the left.
fn turning(ps: &[Piece], i: usize) -> f64 {
    let (Some(a), Some(b)) = (ps.get(i), ps.get(i + 1)) else { return 0.0 };
    let (u, v) = (a.dir(a.length()), b.dir(0.0));
    (u.0 * v.1 - u.1 * v.0).atan2(u.dot(v)).to_degrees()
}

/// Whether winders go round the corner piece `i` starts at: a straight flight meeting a straight flight at 5 to 135
/// degrees, with winders asked.
fn winds(ps: &[Piece], i: usize) -> bool {
    let Some(p) = ps.get(i) else { return false };
    let Some(before) = i.checked_sub(1).and_then(|j| ps.get(j)) else { return false };
    p.winders > 0
        && !p.landing
        && !before.landing
        && p.arc.is_none()
        && before.arc.is_none()
        && (5.0..=135.0).contains(&turning(ps, i - 1).abs())
}

/// Whether flight `i` goes straight on into the next piece, a flight, with no corner or landing between.
fn chains(ps: &[Piece], i: usize) -> bool {
    let (Some(p), Some(q)) = (ps.get(i), ps.get(i + 1)) else { return false };
    !p.landing && !q.landing && turning(ps, i).abs() < 0.5 && !winds(ps, i + 1)
}

/// The risers a flight of `treads` treads holds: one more, or as many where it goes straight on into the next flight.
fn held(ps: &[Piece], i: usize, treads: u32) -> u32 {
    if chains(ps, i) { treads.max(1) } else { treads.saturating_add(1).max(2) }
}

fn allot(ps: &[Piece], goings: Goings, n: u32) -> Allotted {
    // Each corner takes of the piece on one side of it what a mitre across the other's width needs.
    let corner = |i: usize, of: usize| {
        let t = turning(ps, i).abs();
        let h = ps.get(of).map_or(0.0, |p| p.width / 2.0);
        if t < 0.5 { 0.0 } else { (h * (t.to_radians() / 2.0).tan()).min(3.0 * h) }
    };
    let usable: Vec<(f64, f64)> = (0..ps.len())
        .map(|i| {
            let len = ps.get(i).map_or(0.0, Piece::length);
            let lo = if i > 0 { corner(i - 1, i - 1) } else { 0.0 };
            let hi = len - if i + 1 < ps.len() { corner(i, i + 1) } else { 0.0 };
            if hi < lo { (len / 2.0, len / 2.0) } else { (lo, hi) }
        })
        .collect();
    let flights: Vec<usize> = (0..ps.len()).filter(|i| ps.get(*i).is_some_and(|p| !p.landing)).collect();
    let mut out = Allotted {
        usable,
        risers: vec![0; ps.len()],
        goings: vec![goings.usual; ps.len()],
        chained: vec![false; ps.len()],
        used: ps.len(),
        full: false,
    };
    let mut below = 0u32;
    for (k, &i) in flights.iter().enumerate() {
        let wound = if winds(ps, i) { ps.get(i).map_or(0, |p| p.winders.saturating_sub(1)) } else { 0 };
        let (lo, hi) = out.usable.get(i).copied().unwrap_or_default();
        let (treads, going) = goings.share(hi - lo);
        let drawn = held(ps, i, treads);
        let left = n.saturating_sub(below.saturating_add(wound));
        // The last flight, or one after which no other would have its two risers: it takes what is left, and the
        // path ends with it.
        if k + 1 == flights.len() || drawn.saturating_add(2) > left {
            if let Some(r) = out.risers.get_mut(i) {
                *r = left;
            }
            // Its treads as deep as ends it where it was drawn, where the range allows; else as deep as the range lets.
            if let Some(g) = out.goings.get_mut(i) {
                let (min, max) = (goings.min.min(goings.usual), goings.max.max(goings.usual));
                *g = if left > 1 { ((hi - lo) / f64::from(left - 1)).clamp(min, max) } else { goings.usual };
            }
            out.full = drawn >= left;
            if k + 1 < flights.len() {
                out.used = i + 1;
            }
            break;
        }
        if let Some(r) = out.risers.get_mut(i) {
            *r = drawn;
        }
        if let Some(g) = out.goings.get_mut(i) {
            *g = going;
        }
        if let Some(c) = out.chained.get_mut(i) {
            *c = chains(ps, i);
        }
        below = below.saturating_add(wound).saturating_add(drawn);
    }
    out
}

/// How many risers the flights of a stair drawn along a path hold as they are drawn, each as long as it was drawn, the
/// last too (winders round a corner counted): what a stair finished before its top climbs.
pub fn drawn_risers(m: &Model, st: &Stair) -> u32 {
    if st.path.is_empty() {
        return 0;
    }
    let (n, each, _) = m.flight(st);
    let ps = pieces(st);
    let g = goings(m, st, each);
    let plan = allot(&ps, g, n);
    (0..ps.len())
        .filter(|i| ps.get(*i).is_some_and(|p| !p.landing))
        .map(|i| {
            let wound = if winds(&ps, i) { ps.get(i).map_or(0, |p| p.winders.saturating_sub(1)) } else { 0 };
            let (lo, hi) = plan.usable.get(i).copied().unwrap_or_default();
            wound.saturating_add(held(&ps, i, g.share(hi - lo).0))
        })
        .fold(0u32, u32::saturating_add)
}

/// Whether the flights of a stair drawn along a path hold all its risers as they are drawn: the stair reaches its top
/// by the path alone, and its last flight is no longer than drawn.
pub fn path_full(m: &Model, st: &Stair) -> bool {
    let (n, each, _) = m.flight(st);
    !st.path.is_empty() && allot(&pieces(st), goings(m, st, each), n).full
}

/// How deep the treads of `st` may be, its risers `each` high: its `tread`, and for a stair drawn along a path the
/// range its rules give a tread, inside what two risers and a tread may come to, so a flight ends where it was drawn.
pub fn goings(m: &Model, st: &Stair, each: f64) -> Goings {
    if st.path.is_empty() || st.lock_going {
        return Goings::fixed(st.tread);
    }
    let rules = crate::codes::limits(m, st).rules;
    let min = rules.going_min.max(rules.step_min - 2.0 * each);
    let max = rules.going_max.min(rules.step_max - 2.0 * each);
    if min <= max { Goings { usual: st.tread.clamp(min, max), min, max } } else { Goings::fixed(st.tread) }
}

/// A stair drawn along a path: a run along each straight flight, a tread every `tread` of it; ring-shaped treads along a
/// flight that is an arc; a landing along each stretch drawn as one and round each corner, as wide as the stair; winders
/// round a corner where two flights meet and winders are asked. The last flight has the risers left over, however long
/// it was drawn, and what was drawn past the last riser is left out.
fn pathed(st: &Stair, n: u32, each: f64, base: f64, goings: Goings) -> Layout {
    let mut out = Layout {
        runs: Vec::new(),
        pads: Vec::new(),
        walk: Vec::new(),
        width: st.width,
        winders: 0,
        turns: Vec::new(),
        nosings: Vec::new(),
    };
    let ps = pieces(st);
    let plan = allot(&ps, goings, n);
    // The risers under where the stair has got to, the middle line of the landing being gathered (each point with half
    // the width of the piece the line comes to it along), and the landings and winders since the last run.
    let (mut below, mut group, mut since): (u32, Vec<(P2, f64)>, usize) = (0, Vec::new(), 0);
    // A landing: the ground to each side of its middle line, each stretch as wide as its piece, its corners mitred.
    let flush = |out: &mut Layout, group: &mut Vec<(P2, f64)>, since: &mut usize, z: f64| {
        group.dedup_by(|b, a| a.0.dist(b.0) < 1e-6);
        let length: f64 = group.windows(2).map(|s| s.first().zip(s.get(1)).map_or(0.0, |(a, b)| a.0.dist(b.0))).sum();
        if length > 1.0 {
            let mut ring = beside(group, 1.0);
            ring.extend(beside(group, -1.0).into_iter().rev());
            out.pads.push(Pad { ring: ccw(ring), z, tread: false });
            out.walk.extend(group.iter().map(|q| (q.0, z)));
            *since += 1;
        }
        group.clear();
    };
    // How far the pieces after the last flight are moved: its end is where its risers put it, not where it was drawn.
    let mut moved = P2(0.0, 0.0);
    for (i, p) in ps.iter().enumerate().take(plan.used) {
        let (lo, hi) = plan.usable.get(i).copied().unwrap_or_default();
        let z = base + f64::from(below) * each;
        let (h, w) = (p.width / 2.0, p.width);
        // Half as wide as the piece before it.
        let was = i.checked_sub(1).and_then(|j| ps.get(j)).map_or(h, |q| q.width / 2.0);
        if p.landing {
            if group.is_empty() {
                group.push((p.a + moved, h));
            }
            match p.arc {
                Some((_, r, _, sweep)) => {
                    let parts = ((sweep.abs().to_degrees() / 10.0).ceil() as u32).clamp(1, 36);
                    group.extend((1..=parts).map(|k| (p.at(r * sweep.abs() * f64::from(k) / f64::from(parts)) + moved, h)));
                }
                None => group.push((p.b + moved, h)),
            }
            continue;
        }
        let risers = plan.risers.get(i).copied().unwrap_or(0);
        let going = plan.goings.get(i).copied().unwrap_or(st.tread);
        let start = p.at(lo) + moved;
        if winds(&ps, i) {
            // Winders round the corner: a landing first for what the flight before left short of it.
            let before = ps.get(i.wrapping_sub(1)).copied().unwrap_or(*p);
            let (a, turn) = (before.dir(before.length()), turning(&ps, i - 1));
            let side = a.perp() * turn.signum();
            let corner = before.b - a * lo + moved;
            group.push((corner, was));
            flush(&mut out, &mut group, &mut since, z);
            let inside = corner + side * h;
            let rings = wind(inside, side, a, w, turn.abs(), p.winders);
            let count = rings.len();
            let top = (count as f64 - 1.0) * each;
            out.pads.extend(rings.into_iter().enumerate().map(|(k, ring)| Pad { ring: ccw(ring), z: z + k as f64 * each, tread: true }));
            out.winders += count;
            since += count;
            let steps = (turn.abs() / 15.0).round().max(1.0) as u32;
            for k in 1..=steps {
                let f = f64::from(k) / f64::from(steps);
                let ang = (turn.abs() * f).to_radians();
                out.walk.push((inside - side * (h * ang.cos()) + a * (h * ang.sin()), z + top * f));
            }
            below = below.saturating_add(p.winders.saturating_sub(1));
        } else {
            // The landing before it ends where it starts: round the corner, if it starts at one.
            if !group.is_empty() || i > 0 {
                if group.is_empty() {
                    group.push((p.a + moved, was));
                }
                if lo > 1e-6 {
                    group.push((p.a + moved, was));
                }
                group.push((start, h));
                flush(&mut out, &mut group, &mut since, z);
            }
        }
        let z = base + f64::from(below) * each;
        let top = z + f64::from(risers) * each;
        let end = match p.arc {
            None => {
                let run = Run { start, u: p.dir(0.0), length: f64::from(risers.saturating_sub(1)) * going, z: (z, top), risers, width: w };
                if !out.runs.is_empty() {
                    out.turns.push(since);
                }
                since = 0;
                out.walk.extend([(run.start, z), (run.end(), top)]);
                out.runs.push(run);
                run.end()
            }
            // Round an arc: each tread the ring between two risers, a tread apart on the middle line.
            Some(_) => {
                let across = |d: f64, x: f64| p.at(d) + p.dir(d).perp() * x + moved;
                let nosing = |d: f64, z: f64| Nosing { a: across(d, h), b: across(d, -h), out: p.dir(d) * -1.0, z, rise: each };
                out.walk.push((start, z));
                for k in 1..risers {
                    let (d0, d1) = (lo + f64::from(k - 1) * going, lo + f64::from(k) * going);
                    let mid = (d0 + d1) / 2.0;
                    let ring = vec![across(d0, h), across(mid, h), across(d1, h), across(d1, -h), across(mid, -h), across(d0, -h)];
                    out.pads.push(Pad { ring: ccw(ring), z: z + f64::from(k) * each, tread: true });
                    out.nosings.push(nosing(d0, z + f64::from(k) * each));
                    out.walk.push((p.at(d1) + moved, z + f64::from(k) * each));
                    out.winders += 1;
                    since += 1;
                }
                let last = lo + f64::from(risers.saturating_sub(1)) * going;
                if risers > 0 {
                    out.nosings.push(nosing(last, top));
                }
                if let Some(last) = out.walk.last_mut() {
                    last.1 = top;
                }
                p.at(last) + moved
            }
        };
        below = below.saturating_add(risers);
        if plan.chained.get(i).copied().unwrap_or(false) {
            // Straight on into the next flight: its last tread the step onto that flight's first riser, a tread deep,
            // and that flight on from where it ends.
            let (d0, d1) = (lo + f64::from(risers.saturating_sub(1)) * going, lo + f64::from(risers) * going);
            let mid = (d0 + d1) / 2.0;
            let across = |d: f64, x: f64| p.at(d) + p.dir(d).perp() * x + moved;
            let ring = vec![across(d0, h), across(mid, h), across(d1, h), across(d1, -h), across(mid, -h), across(d0, -h)];
            out.pads.push(Pad { ring: ccw(ring), z: top, tread: true });
            out.walk.push((p.at(d1) + moved, top));
            out.winders += 1;
            since += 1;
            moved = moved + p.at(d1) - p.at(hi.max(lo));
            continue;
        }
        group.push((end, h));
        // What is drawn after the last flight starts where that flight ends.
        if below >= n {
            let was = moved;
            moved = end - p.at(hi.max(lo));
            if (hi - lo) > f64::from(risers.saturating_sub(1)) * going {
                // Drawn longer than its risers need: the rest of it is landing.
                moved = was;
            }
        }
        // On to the corner it ends at.
        if i + 1 < plan.used && p.length() - hi > 1e-6 {
            group.push((p.b + moved, h));
        }
    }
    let z = base + f64::from(below) * each;
    flush(&mut out, &mut group, &mut since, z);
    out
}

/// A stair drawn by hand: no runs, a tread after each riser but the last, each a riser above the one before: its own
/// outline, or the ground between its riser and the next; the walk line through the middles of the risers.
fn sketched(st: &Stair, each: f64, base: f64) -> Layout {
    let mut out = Layout {
        runs: Vec::new(),
        pads: Vec::new(),
        walk: Vec::new(),
        width: st.width,
        winders: 0,
        turns: Vec::new(),
        nosings: Vec::new(),
    };
    for (i, pair) in st.sketch.windows(2).enumerate() {
        let [a, b] = pair else { continue };
        let ring = if a.outline.len() >= 3 { a.outline.clone() } else { vec![a.riser[0], a.riser[1], b.riser[1], b.riser[0]] };
        out.pads.push(Pad { ring: ccw(ring), z: base + (i as f64 + 1.0) * each, tread: true });
    }
    out.winders = out.pads.len();
    let (n, rise) = (st.sketch.len(), st.sketch.len() as f64 * each);
    out.walk =
        st.sketch.iter().enumerate().map(|(i, s)| (s.mid(), base + if n > 1 { rise * i as f64 / (n as f64 - 1.0) } else { 0.0 })).collect();
    out
}

/// A stair made by its runs and turns, as one drawn by hand: a riser for each of its risers, where it is, and the
/// outline of each landing and winder as the tread after the riser before it. None for a spiral, and for a stair
/// that is one already.
pub fn as_sketch(m: &Model, st: &Stair) -> Option<Vec<crate::model::SketchStep>> {
    use crate::model::SketchStep;
    if !st.sketch.is_empty() {
        return None;
    }
    // A spiral: each tread with the riser before it, the edge it shares with the tread below (the first, its edge at
    // the foot), and the last riser, the top tread's riser turned on by one tread.
    if let (Some(sp), Some(c)) = (st.spiral, spiral_centre(st)) {
        let l = first(m, st);
        let pads: Vec<&Pad> = l.pads.iter().filter(|p| p.tread).collect();
        // Its left end first, looking up the stair into the tread.
        let orient = |p: P2, q: P2, pad: &Pad| {
            let mid = (p + q) * 0.5;
            let into = (geom::centroid(&pad.ring) - mid).unit();
            if (p - mid).dot(into.perp()) >= 0.0 { [p, q] } else { [q, p] }
        };
        let mut steps: Vec<SketchStep> = Vec::new();
        for (i, pad) in pads.iter().enumerate() {
            let edge = match i.checked_sub(1).and_then(|j| pads.get(j)) {
                Some(prev) => {
                    let shared: Vec<P2> = pad.ring.iter().copied().filter(|q| prev.ring.iter().any(|o| o.dist(*q) < 1e-3)).collect();
                    (*shared.first()?, *shared.last()?)
                }
                None => geom::ring_edges(&pad.ring)
                    .min_by(|a, b| ((a.0 + a.1) * 0.5).dist(st.foot()).total_cmp(&((b.0 + b.1) * 0.5).dist(st.foot())))?,
            };
            steps.push(SketchStep { riser: orient(edge.0, edge.1, pad), outline: pad.ring.clone() });
        }
        let (n, _, _) = m.flight(st);
        let turn = spiral_step(st, sp, n) * if sp.radius < 0.0 { -1.0 } else { 1.0 };
        let rot = |q: P2| {
            let (s, k) = turn.sin_cos();
            let d = q - c;
            c + P2(d.0 * k - d.1 * s, d.0 * s + d.1 * k)
        };
        let last = steps.last()?.riser;
        steps.push(SketchStep { riser: [rot(last[0]), rot(last[1])], outline: Vec::new() });
        return (steps.len() >= 2).then_some(steps);
    }
    // One round an arc, or with a landing at its foot, is not converted yet.
    if st.path.iter().any(|s| s.arc != 0.0) || st.path.first().is_some_and(|s| s.landing) {
        return None;
    }
    let l = first(m, st);
    let mut steps: Vec<SketchStep> = Vec::new();
    let mut pads = l.pads.iter();
    for (i, run) in l.runs.iter().enumerate() {
        let half = run.width / 2.0;
        let n = run.u.perp();
        for k in 0..run.risers {
            let at = run.start + run.u * (f64::from(k) * run.going());
            steps.push(SketchStep { riser: [at + n * half, at - n * half], outline: Vec::new() });
        }
        // The turn after it: the first of its pads is the tread after the run's last riser; each one after it starts at
        // the edge it shares with the one before.
        let count = l.turns.get(i).copied().unwrap_or(0);
        let mut before: Option<&Pad> = None;
        for pad in pads.by_ref().take(count) {
            if let Some(prev) = before {
                let shared: Vec<P2> = pad.ring.iter().copied().filter(|q| prev.ring.iter().any(|o| o.dist(*q) < 1e-6)).collect();
                let (&p, &q) = (shared.first()?, shared.get(1)?);
                let last = steps.last()?;
                // Its left end is the one nearer the left end of the riser before.
                let riser = if p.dist(last.riser[0]) + q.dist(last.riser[1]) <= q.dist(last.riser[0]) + p.dist(last.riser[1]) {
                    [p, q]
                } else {
                    [q, p]
                };
                steps.push(SketchStep { riser, outline: Vec::new() });
            }
            if let Some(last) = steps.last_mut() {
                last.outline = pad.ring.clone();
            }
            before = Some(pad);
        }
    }
    (steps.len() >= 2).then_some(steps)
}

/// The angle each tread of spiral `sp` turns through, in radians, on a stair of `n` risers.
pub fn spiral_step(st: &Stair, sp: crate::model::Spiral, n: u32) -> f64 {
    let treads = f64::from(n.saturating_sub(1)).max(1.0);
    if sp.sweep > 0.0 { sp.sweep.to_radians() / treads } else { st.tread / sp.radius.abs().max(1.0) }
}

/// The centre a spiral stair winds about.
pub fn spiral_centre(st: &Stair) -> Option<P2> {
    let sp = st.spiral?;
    let side = if sp.radius < 0.0 { -1.0 } else { 1.0 };
    Some(st.foot() + heading(st.angle).perp() * (side * sp.radius.abs()))
}

/// A spiral stair: no runs, a tread for each riser but the last, each a piece of the ring about its centre, a riser
/// above the one before; the walk line round its middle.
fn spiral(st: &Stair, sp: crate::model::Spiral, n: u32, each: f64, base: f64) -> Layout {
    let mut out = Layout {
        runs: Vec::new(),
        pads: Vec::new(),
        walk: Vec::new(),
        width: st.width,
        winders: 0,
        turns: Vec::new(),
        nosings: Vec::new(),
    };
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
        out.nosings.push(Nosing {
            a: at(inner, a),
            b: at(outer, a),
            out: (at(r, a - 0.01) - at(r, a)).unit(),
            z: base + f64::from(k + 1) * each,
            rise: each,
        });
    }
    out.winders = out.pads.len();
    let (end, rise) = (f64::from(treads) * step, f64::from(n) * each);
    // The last riser, onto the floor above.
    if n > 0 {
        out.nosings.push(Nosing {
            a: at(inner, end),
            b: at(outer, end),
            out: (at(r, end - 0.01) - at(r, end)).unit(),
            z: base + rise,
            rise: each,
        });
    }
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
        out.turns.extend(l.turns);
        out.nosings.extend(l.nosings);
    }
    out
}

/// The layout of a stair's first storey, which is the stair as it is drawn and edited.
pub fn first(m: &Model, st: &Stair) -> Layout {
    let (n, each, _) = m.flight(st);
    ended(m, st, laid(st, &st.landings, n, each, m.stair_span(st).0, goings(m, st, each)))
}

/// A storey's layout with the treads its type puts at its ends: one level with the floor before its first riser where
/// it does not begin with a riser, one level with the floor above after its last where it does not end with one.
fn ended(m: &Model, st: &Stair, mut l: Layout) -> Layout {
    let Some(t) = m.types.stair.get(&st.type_name) else { return l };
    let tread = |run: &Run, from: f64, to: f64, z: f64| {
        let (u, n) = (run.u, run.u.perp() * (run.width / 2.0));
        let (a, b) = (run.start + u * from, run.start + u * to);
        Pad { ring: ccw(vec![a - n, b - n, b + n, a + n]), z, tread: true }
    };
    let (foot, top) = (l.walk.first().map(|w| w.1), l.walk.last().map(|w| w.1));
    if t.start_tread
        && let Some(run) = l.runs.first()
        && foot.is_some_and(|z| (run.z.0 - z).abs() < 1e-6)
    {
        let g = if run.risers > 1 { run.going() } else { st.tread };
        let pad = tread(run, -g, 0.0, run.z.0);
        l.pads.insert(0, pad);
        // The walk line starts on it, so the UP mark stands clear of it.
        l.walk.insert(0, (run.start - run.u * g, run.z.0));
    }
    if t.end_tread
        && let Some(run) = l.runs.last()
        && top.is_some_and(|z| (run.z.1 - z).abs() < 1e-6)
    {
        let g = if run.risers > 1 { run.going() } else { st.tread };
        let pad = tread(run, run.length, run.length + g, run.z.1);
        l.pads.push(pad);
        l.walk.push((run.end() + run.u * g, run.z.1));
    }
    l
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
                return (level, ended(m, st, laid(st, &st.landings, n, each, base, goings(m, st, each))));
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
            (level, ended(m, st, laid(st, if holds { &moved } else { &[] }, n, each, base, goings(m, st, each))))
        })
        .collect()
}

/// A stair's layout over one storey: `n` risers each `each` high from `base`, with `landings`, its landings' edges moved
/// as it says.
fn laid(st: &Stair, landings: &[crate::model::Landing], n: u32, each: f64, base: f64, goings: Goings) -> Layout {
    let mut l = unmoved(st, landings, n, each, base, goings);
    for e in &st.edges {
        let n = heading(st.angle + e.normal);
        let Some(pad) = l.pads.iter().filter(|p| !p.tread).nth(e.landing as usize) else { continue };
        let ring = pad.ring.clone();
        // The corners of each free edge that faces that way, each moved once.
        let mut moving: Vec<usize> = Vec::new();
        for (i, a, b) in free_edges(&l, &ring) {
            if facing(a, b).dot(n) > (1.0f64).to_radians().cos() {
                moving.extend([i, (i + 1) % ring.len()]);
            }
        }
        moving.sort_unstable();
        moving.dedup();
        if let Some(pad) = l.pads.iter_mut().filter(|p| !p.tread).nth(e.landing as usize) {
            for i in moving {
                if let Some(q) = pad.ring.get_mut(i) {
                    *q = *q + n * e.by;
                }
            }
        }
    }
    l
}

/// The way out of a counter-clockwise ring square to its edge from `a` to `b`.
fn facing(a: P2, b: P2) -> P2 {
    let d = (b - a).unit();
    P2(d.1, -d.0)
}

/// The stringers round landing `pad`'s free edges, `width` wide inside them: each its outer edge and its inner, as
/// (outer start, inner start), (outer end, inner end); where two meet at a corner, mitred.
pub fn landing_stringers(l: &Layout, pad: &Pad, width: f64) -> Vec<((P2, P2), (P2, P2))> {
    let ring = ccw(pad.ring.clone());
    free_edges(l, &ring)
        .into_iter()
        .map(|(_, a, b)| {
            let inward = facing(a, b) * -1.0;
            ((a, a + inward * width), (b, b + inward * width))
        })
        .collect()
}

/// The edges of a landing's `ring` (counter-clockwise) that no step stands on, each with the index of its first corner:
/// none is on a run's first or last riser, or on a riser or a side of a tread.
fn free_edges(l: &Layout, ring: &[P2]) -> Vec<(usize, P2, P2)> {
    // How far `q` is from the segment `a`–`b`.
    let off = |q: P2, a: P2, b: P2| {
        let d = b - a;
        let f = if d.dot(d) > 0.0 { ((q - a).dot(d) / d.dot(d)).clamp(0.0, 1.0) } else { 0.0 };
        q.dist(a + d * f)
    };
    let on = |a: P2, b: P2, c: P2, d: P2| off(a, c, d) < 1.0 && off(b, c, d) < 1.0;
    let mut out = Vec::new();
    for i in 0..ring.len() {
        let (Some(&a), Some(&b)) = (ring.get(i), ring.get((i + 1) % ring.len())) else { continue };
        if a.dist(b) < 1.0 {
            continue;
        }
        let riser = l.runs.iter().any(|r| {
            let n = r.u.perp() * (r.width / 2.0);
            on(a, b, r.start - n, r.start + n) || on(a, b, r.end() - n, r.end() + n)
        });
        let tread = l.pads.iter().filter(|p| p.tread).any(|p| geom::ring_edges(&p.ring).any(|(c, d)| on(a, b, c, d) || on(c, d, a, b)));
        if !riser && !tread {
            out.push((i, a, b));
        }
    }
    out
}

/// A landing's edges a grip moves: the `k`th landing's free edges, by which way they face (tenths of a degree round from
/// the way the stair starts off), each at the middle of the first edge facing that way.
pub fn landing_edges(m: &Model, st: &Stair) -> Vec<(u32, i64, P2)> {
    let l = first(m, st);
    let mut out: Vec<(u32, i64, P2)> = Vec::new();
    for (k, pad) in l.pads.iter().filter(|p| !p.tread).enumerate() {
        for (_, a, b) in free_edges(&l, &pad.ring) {
            let f = facing(a, b);
            let deg = (f.1.atan2(f.0).to_degrees() - st.angle).rem_euclid(360.0);
            let tenths = ((deg * 10.0).round() as i64).rem_euclid(3600);
            let k = k as u32;
            if !out.iter().any(|(j, t, _)| *j == k && *t == tenths) {
                out.push((k, tenths, (a + b) * 0.5));
            }
        }
    }
    out
}

fn unmoved(st: &Stair, landings: &[crate::model::Landing], n: u32, each: f64, base: f64, goings: Goings) -> Layout {
    if !st.path.is_empty() {
        return pathed(st, n, each, base, goings);
    }
    if !st.sketch.is_empty() {
        return sketched(st, each, base);
    }
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
        .map(|l| Turn {
            turn: l.turn,
            winders: l.winders,
            length: l.length,
            into: f64::from(l.balance) * st.tread,
            balance: l.balance,
            width: l.width,
        })
        .collect();
    chain(st.foot(), st.angle, st.width, runs, &turns, each)
}

/// `st` climbing the other way: its first riser where its top was, its runs, landings and winders the same ones met in
/// the other order, its top where its foot was. None for a stair with no run.
pub fn flipped(m: &Model, st: &Stair) -> Option<Stair> {
    let (n, _, _) = m.flight(st);
    if !st.path.is_empty() {
        // Its path the other way round: each segment back to where it came from, arcs turning the other way, the winders
        // of a corner on the flight that now comes after it.
        let mut out = st.clone();
        let points: Vec<P2> = std::iter::once(st.start).chain(st.path.iter().map(|s| s.to)).collect();
        out.start = *points.last()?;
        let count = st.path.len();
        out.path = (0..count)
            .rev()
            .filter_map(|i| {
                let seg = st.path.get(i)?;
                let winders = st.path.get(i + 1).map_or(0, |next| next.winders);
                Some(crate::model::PathSeg {
                    to: *points.get(i)?,
                    landing: seg.landing,
                    arc: -seg.arc + 0.0,
                    winders: if seg.landing { 0 } else { winders },
                    width: seg.width,
                })
            })
            .collect();
        out.offset = -st.offset + 0.0;
        out.line = match st.line {
            crate::model::StairLine::Left => crate::model::StairLine::Right,
            crate::model::StairLine::Right => crate::model::StairLine::Left,
            line => line,
        };
        if let Some(first) = out.path.first() {
            let way = first.to - out.start;
            out.angle = (way.1.atan2(way.0).to_degrees() * 1e9).round() / 1e9;
        }
        return Some(out);
    }
    if !st.sketch.is_empty() {
        // Its risers the other way round, each with its ends swapped; a tread's outline goes to the riser now before it.
        let mut out = st.clone();
        let count = st.sketch.len();
        out.sketch = (0..count)
            .filter_map(|j| {
                let old = st.sketch.get(count - 1 - j)?;
                let outline = count.checked_sub(2 + j).and_then(|k| st.sketch.get(k)).map(|s| s.outline.clone()).unwrap_or_default();
                Some(crate::model::SketchStep { riser: [old.riser[1], old.riser[0]], outline })
            })
            .collect();
        if let [a, b, ..] = out.sketch.as_slice() {
            let way = b.mid() - a.mid();
            (out.start, out.angle) = (a.mid(), (way.1.atan2(way.0).to_degrees() * 1e9).round() / 1e9);
        }
        return Some(out);
    }
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
    // Its runs' widths, met the other way round: the stair is as wide as its last run was, and each landing gives the
    // run after it the width of the run that was before it.
    let widths: Vec<f64> = l.runs.iter().rev().map(|r| r.width).collect();
    out.width = widths.first().copied().unwrap_or(st.width);
    out.landings = st
        .landings
        .iter()
        .rev()
        .enumerate()
        .map(|(j, t)| {
            let (before, after) = (widths.get(j).copied().unwrap_or(st.width), widths.get(j + 1).copied().unwrap_or(st.width));
            crate::model::Landing {
                at: f64::from(n) - t.at - f64::from(t.wound()),
                turn: -t.turn + 0.0,
                width: if (after - before).abs() > 1e-9 { after } else { 0.0 },
                ..*t
            }
        })
        .collect();
    // Its line is where it was: `start` is put so the middle of the first riser is the old top's.
    out.start = last.end();
    let off = out.foot() - out.start;
    out.start = crate::commands::micron(out.start - off);
    // Its landings met the other way round: their edges as laid out.
    out.edges.clear();
    Some(out)
}

/// A handle on a stair, to drag: its foot (the whole stair moves), its top (it turns about its foot), the end of a run
/// before a turn (the run takes or gives risers), a side (its width, from its location line) or a landing's far edge
/// (the landing's length).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    /// An end of a riser of a stair drawn by hand: which riser, and whether its right end.
    Riser(usize, bool),
    /// The centre a spiral winds about: dragged, its first riser stays and its radius follows.
    Centre,
    /// A point of the path a stair is drawn along: the end of that segment.
    Vertex(usize),
    Foot,
    Top,
    Turn(usize),
    Width(Side),
    Landing(usize),
    /// A free edge of a landing: which landing, and the way it faces in tenths of a degree from the way the stair starts.
    Edge(u32, i64),
}

/// Where each of a stair's grips is, in plan.
pub fn grips(m: &Model, st: &Stair) -> Vec<(Grip, P2)> {
    use crate::model::StairLine;
    // A stair drawn by hand: its first riser's middle moves it all, and every end of every riser is its own.
    if !st.sketch.is_empty() {
        let mut out = vec![(Grip::Foot, st.sketch.first().map_or(st.start, |s| s.mid()))];
        out.extend(
            st.sketch.iter().enumerate().flat_map(|(i, s)| [(Grip::Riser(i, false), s.riser[0]), (Grip::Riser(i, true), s.riser[1])]),
        );
        return out;
    }
    // Drawn along a path: its start moves it all, each point of the path is its own, and its sides set its width.
    if !st.path.is_empty() {
        let mut out = vec![(Grip::Foot, st.start)];
        out.extend(st.path.iter().enumerate().map(|(i, seg)| (Grip::Vertex(i), seg.to)));
        out.extend(landing_edges(m, st).into_iter().map(|(k, t, at)| (Grip::Edge(k, t), at)));
        if let Some(run) = first(m, st).runs.first() {
            let (mid, n) = (run.start + run.u * (run.length / 2.0), run.u.perp() * (st.width / 2.0));
            if st.line != StairLine::Left {
                out.push((Grip::Width(Side::Left), mid + n));
            }
            if st.line != StairLine::Right {
                out.push((Grip::Width(Side::Right), mid - n));
            }
        }
        return out;
    }
    // A spiral: its foot moves it, its centre sets its radius.
    if let Some(sp) = st.spiral {
        return vec![(Grip::Foot, st.start), (Grip::Centre, st.foot() + heading(st.angle).perp() * sp.radius)];
    }
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
    // A landing's free edges, but where a landing's own grip already sets how deep it is.
    let edges: Vec<(Grip, P2)> = landing_edges(m, st)
        .into_iter()
        .filter(|(_, _, at)| !out.iter().any(|(_, q)| q.dist(*at) < 1.0))
        .map(|(k, t, at)| (Grip::Edge(k, t), at))
        .collect();
    out.extend(edges);
    out
}

/// `st` as it is with `grip` dragged to `to`. A run's end takes the whole risers that fit, and the landings after it
/// keep their runs; a stair that cannot be (a run of one riser) is for `Model::check` to refuse.
pub fn dragged(m: &Model, st: &Stair, grip: Grip, to: P2) -> Stair {
    use crate::model::StairLine;
    let mut out = st.clone();
    // A landing's free edge: out or in square to itself, as far as the cursor is from where it is.
    if let Grip::Edge(k, tenths) = grip {
        let Some((_, _, at)) = landing_edges(m, st).into_iter().find(|(j, t, _)| *j == k && *t == tenths) else { return out };
        let normal = tenths as f64 / 10.0;
        let by = (to - at).dot(heading(st.angle + normal));
        let by = (by * 1000.0).round() / 1000.0;
        match out
            .edges
            .iter_mut()
            .find(|e| e.landing == k && ((e.normal - normal).rem_euclid(360.0)).min((normal - e.normal).rem_euclid(360.0)) < 0.5)
        {
            Some(e) => e.by += by,
            None => out.edges.push(crate::model::LandingEdge { landing: k, normal, by }),
        }
        out.edges.retain(|e| e.by.abs() > 1e-6);
        return out;
    }
    let l = first(m, st);
    let whole = |x: f64| (x * 1000.0).round() / 1000.0 + 0.0;
    if !st.sketch.is_empty() {
        match grip {
            // All of it, by as far as its first riser's middle goes.
            Grip::Foot => {
                let by = to - st.sketch.first().map_or(st.start, |s| s.mid());
                let moved = |q: P2| P2(whole(q.0 + by.0), whole(q.1 + by.1));
                for step in &mut out.sketch {
                    step.riser = [moved(step.riser[0]), moved(step.riser[1])];
                    step.outline = step.outline.iter().map(|q| moved(*q)).collect();
                }
            }
            // A riser's end, and with it the corner of the outlines that stood there.
            Grip::Riser(i, right) => {
                let Some(was) = st.sketch.get(i).map(|s| s.riser[usize::from(right)]) else { return out };
                let to = P2(whole(to.0), whole(to.1));
                for step in &mut out.sketch {
                    for q in step.riser.iter_mut().chain(step.outline.iter_mut()) {
                        if q.dist(was) < 1e-6 {
                            *q = to;
                        }
                    }
                }
            }
            _ => {}
        }
        // It starts where its first riser is, toward its second.
        if let [a, b, ..] = out.sketch.as_slice() {
            let way = b.mid() - a.mid();
            (out.start, out.angle) = (a.mid(), whole(way.1.atan2(way.0).to_degrees()));
        }
        return out;
    }
    if !st.path.is_empty() {
        let to = P2(whole(to.0), whole(to.1));
        match grip {
            Grip::Foot => {
                let by = to - st.start;
                out.start = to;
                for seg in &mut out.path {
                    seg.to = P2(whole(seg.to.0 + by.0), whole(seg.to.1 + by.1));
                }
            }
            Grip::Vertex(i) => {
                if let Some(seg) = out.path.get_mut(i) {
                    seg.to = to;
                }
            }
            // Its width, from the line it is drawn by, measured across its first flight.
            Grip::Width(_) => {
                if let Some(run) = l.runs.first() {
                    let n = run.u.perp();
                    let half = match st.line {
                        StairLine::Left => -st.width / 2.0,
                        StairLine::Centre => 0.0,
                        StairLine::Right => st.width / 2.0,
                    };
                    let d = (to - (run.start - n * half)).dot(n);
                    let wide = match st.line {
                        StairLine::Centre => 2.0 * d.abs(),
                        StairLine::Left => -d,
                        StairLine::Right => d,
                    };
                    out.width = whole(wide.max(100.0));
                }
            }
            _ => {}
        }
        if let Some(first) = out.path.first() {
            let way = first.to - out.start;
            out.angle = whole(way.1.atan2(way.0).to_degrees());
        }
        return out;
    }
    match grip {
        Grip::Riser(..) | Grip::Vertex(_) | Grip::Edge(..) => {}
        // The centre goes where it is put: the first riser stays, pointing at it, and the radius is how far it is, no
        // nearer than half the stair's width.
        Grip::Centre => {
            let Some(sp) = st.spiral else { return out };
            let foot = st.foot();
            let d = to - foot;
            if d.length() < 1.0 {
                return out;
            }
            let (side, d) = (if sp.radius < 0.0 { -1.0 } else { 1.0 }, d.unit());
            // The way it climbs from its first riser: square to the line to its centre, the centre on its own side.
            let way = P2(d.1, -d.0) * side;
            out.angle = whole(way.1.atan2(way.0).to_degrees());
            out.spiral = Some(crate::model::Spiral { radius: side * whole((to - foot).length().max(st.width / 2.0)), sweep: sp.sweep });
            // Its location line is where it was: the first riser's middle does not move.
            let off = out.foot() - out.start;
            out.start = P2(whole(foot.0 - off.0), whole(foot.1 - off.1));
        }
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
    let turns: Vec<Turn> = r.landings.iter().map(|l| Turn { turn: l.turn, length: l.length, width: l.width, ..Turn::default() }).collect();
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

/// Each step of a stair that is stood on going up, in plan, with its height: every tread of its runs, its winders and its
/// landings. (Its foot and the floor it climbs onto are floors.)
pub fn treads(m: &Model, st: &Stair) -> Vec<(Vec<P2>, f64)> {
    let l = stair(m, st);
    let mut out = Vec::new();
    for run in &l.runs {
        let half = run.width / 2.0;
        let each = if run.risers > 0 { (run.z.1 - run.z.0) / f64::from(run.risers) } else { 0.0 };
        let n = run.u.perp();
        for k in 1..run.risers {
            let (a, b) = (f64::from(k - 1) * run.going(), f64::from(k) * run.going());
            let ring = vec![
                run.start + run.u * a - n * half,
                run.start + run.u * b - n * half,
                run.start + run.u * b + n * half,
                run.start + run.u * a + n * half,
            ];
            out.push((ring, run.z.0 + f64::from(k) * each));
        }
    }
    out.extend(l.pads.into_iter().map(|p| (p.ring, p.z)));
    out
}

/// A stair's edges in plan that a dimension measures to, in a fixed order: of each run its sides, its foot and its head,
/// then each riser up it; then each landing's and tread's edges.
pub fn reference_edges(m: &Model, st: &Stair) -> Vec<(P2, P2)> {
    let l = stair(m, st);
    let mut out = Vec::new();
    for run in &l.runs {
        let (u, n) = (run.u, run.u.perp() * (run.width / 2.0));
        let (a, b) = (run.start, run.end());
        out.extend([(a - n, b - n), (a + n, b + n), (a - n, a + n), (b - n, b + n)]);
        let going = if run.risers > 1 { run.going() } else { st.tread };
        for k in 1..run.risers {
            let at = a + u * (f64::from(k) * going);
            out.push((at - n, at + n));
        }
    }
    for pad in &l.pads {
        out.extend(geom::ring_edges(&pad.ring));
    }
    out.retain(|(a, b)| a.dist(*b) > 1e-6);
    out
}

/// The room a stair's headroom asks to be clear over it: over each tread and landing, a block as high as the headroom of
/// the rules it is held to, each a ring in plan between two planes.
pub fn clearance(m: &Model, st: &Stair) -> Vec<(Vec<P2>, Plane, Plane)> {
    let least = crate::codes::limits(m, st).rules.headroom;
    if least <= 0.0 {
        return Vec::new();
    }
    treads(m, st).into_iter().map(|(ring, z)| (ring, Plane::flat(z), Plane::flat(z + least))).collect()
}

/// Where a floor, a ceiling or a beam stands over a stair's steps with less than `least` between them: it, the least
/// height from a step up to its underside, and the steps under it that are short of `least`, in plan. A floor or a
/// ceiling cut open over a step (an opening of its own, or a shaft through it) is not over it.
pub fn headroom(m: &Model, st: &Stair, least: f64) -> Vec<(crate::model::Id, f64, Vec<Vec<P2>>)> {
    let steps = treads(m, st);
    let Some(lowest) = steps.iter().map(|(_, z)| *z).reduce(f64::min) else { return Vec::new() };
    // Shafts, with the heights they pass through.
    let shafts: Vec<(&[P2], f64, f64)> = (m.elements.iter())
        .filter_map(|(_, e)| match e {
            Element::Shaft(sh) => {
                let (bottom, top) = m.span(sh.level, sh.base_offset, sh.top_level, sh.height);
                Some((sh.boundary.as_slice(), bottom, top))
            }
            _ => None,
        })
        .collect();
    // What a step's headroom is measured to: a floor's underside, a ceiling's, a beam's; each with its outline, the holes
    // through it, its underside and its top.
    type Over = (crate::model::Id, Vec<P2>, Vec<Vec<P2>>, Plane, f64);
    let mut over_head: Vec<Over> = Vec::new();
    for (id, e) in m.elements.iter() {
        match e {
            Element::Floor(f) => {
                let top = m.elevation(f.level) + f.offset;
                let under = top - m.types.floor.get(&f.type_name).map_or(0.0, |t| t.thickness);
                over_head.push((*id, f.boundary.clone(), f.openings.clone(), Plane::flat(under), top));
            }
            Element::Ceiling(c) => {
                let under = m.elevation(c.level) + c.offset;
                let top = under + m.types.ceiling.get(&c.type_name).map_or(0.0, |t| t.thickness);
                over_head.push((*id, c.boundary.clone(), c.openings.clone(), Plane::flat(under), top));
            }
            Element::Roof(r) => {
                let thick = m.types.roof.get(&r.type_name).map_or(0.0, |t| t.thickness);
                for (loops, plane) in m.roof_faces(r).unwrap_or_default() {
                    let Some((outer, holes)) = loops.split_first() else { continue };
                    let high = outer.iter().map(|q| plane.at(*q)).fold(f64::NEG_INFINITY, f64::max);
                    over_head.push((*id, outer.clone(), holes.to_vec(), Plane { z: plane.z - thick, g: plane.g }, high));
                }
            }
            Element::Beam(_) => {
                if let Some(g) = m.run_geom(e).filter(|g| !g.round) {
                    let (_, bottom, top) = g.prism();
                    let ring = crate::runs::outline(m, *id);
                    let high = ring.iter().map(|q| top.at(*q)).fold(f64::NEG_INFINITY, f64::max);
                    over_head.push((*id, ring, Vec::new(), bottom, high));
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for (id, ring, holes, bottom, top) in &over_head {
        let lowest_under = ring.iter().map(|q| bottom.at(*q)).fold(f64::INFINITY, f64::min);
        if *top <= lowest + 1.0 || lowest_under - lowest > 1e5 {
            continue;
        }
        // Whether it is over a point: inside its outline, and in no hole through it.
        let over = |q: P2| {
            let under = bottom.at(q);
            geom::contains(ring, q)
                && !holes.iter().any(|o| geom::contains(o, q))
                && !shafts.iter().any(|(r, b, up)| *b < *top && *up > under && geom::contains(r, q))
        };
        // Measured over the whole step, its front edge (the nosing line) as much as its middle: at its middle and at each
        // corner drawn a little in, so an opening's edge on the step's own doesn't count as over it.
        let clear = |r: &Vec<P2>, z: f64| -> Option<f64> {
            let c = geom::centroid(r);
            std::iter::once(c)
                .chain(r.iter().map(|q| *q + (c - *q).unit() * (c.dist(*q) * 0.05).min(20.0)))
                .filter(|q| over(*q))
                .map(|q| bottom.at(q) - z)
                .reduce(f64::min)
        };
        let short: Vec<(&Vec<P2>, f64)> = (steps.iter())
            // A step under its top: one inside its thickness has no headroom at all.
            .filter(|(_, z)| *z < *top - 1.0)
            .filter_map(|(r, z)| clear(r, *z).filter(|h| *h < least - 1e-6).map(|h| (r, h.max(0.0))))
            .collect();
        if let Some(least_here) = short.iter().map(|(_, c)| *c).reduce(f64::min) {
            out.push((*id, least_here, short.into_iter().map(|(r, _)| r.clone()).collect()));
        }
    }
    out
}

/// The outlines round `rings` taken together: steps side by side made one outline, its corners only. Their hull,
/// should they not triangulate.
pub fn united(rings: &[Vec<P2>]) -> Vec<Vec<P2>> {
    let all: Vec<P2> = rings.iter().flatten().copied().collect();
    let Some(first) = all.first() else { return Vec::new() };
    let (lo, hi) = all.iter().fold((*first, *first), |(lo, hi), q| (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1))));
    let frame = vec![
        P2(lo.0 - 100.0, lo.1 - 100.0),
        P2(hi.0 + 100.0, lo.1 - 100.0),
        P2(hi.0 + 100.0, hi.1 + 100.0),
        P2(lo.0 - 100.0, hi.1 + 100.0),
    ];
    let lines: Vec<(P2, P2)> = rings.iter().flat_map(|r| geom::ring_edges(r)).filter(|(a, b)| a.dist(*b) > 1e-6).collect();
    let Ok(t) = crate::triangulation::constrained(&frame, &[], &[], &lines) else { return vec![geom::hull(all)] };
    let inside: Vec<bool> = (t.triangles.iter())
        .map(|k| {
            let c = k.iter().filter_map(|i| t.points.get(*i)).fold(P2(0.0, 0.0), |a, q| a + *q) * (1.0 / 3.0);
            rings.iter().any(|r| geom::contains(r, c))
        })
        .collect();
    let pieces = crate::triangulation::merge(&t, &inside, &|within: &bool, _| *within);
    let mut out: Vec<Vec<P2>> = (pieces.into_iter())
        .filter(|(_, within)| *within)
        .filter_map(|(loops, _)| loops.into_iter().next())
        .map(|ring| {
            // Its corners only: a point on a straight edge is none.
            let n = ring.len();
            let kept: Vec<P2> = (0..n)
                .filter(|i| {
                    let (a, b, c) = (ring[(i + n - 1) % n], ring[*i], ring[(i + 1) % n]);
                    ((b - a).0 * (c - b).1 - (b - a).1 * (c - b).0).abs()
                        > 1e-3 * (b - a).length().max(1.0) * (c - b).length().max(1.0) / 1e3
                })
                .map(|i| ring[i])
                .collect();
            if kept.len() >= 3 { kept } else { ring }
        })
        .filter(|ring| geom::area(ring).abs() > 1.0)
        .collect();
    if out.is_empty() {
        out.push(geom::hull(all));
    }
    out
}

/// A stair's solids as its type builds it, each a ring in plan between two planes; none for a stair of no type, which is
/// solid down to its level. With a waist: each tread from its top down to the slab's sloping underside, and each landing
/// that thick. With none: treads standing free, risers between them where the type has any, and a stringer down each
/// side of each run where it has those.
pub fn parts(m: &Model, st: &Stair) -> Option<Vec<(Vec<P2>, Plane, Plane)>> {
    parts_tagged(m, st).map(|v| v.into_iter().map(|(_, ring, lo, hi)| (ring, lo, hi)).collect())
}

/// A stair's part, as a view's subcategories tell them apart.
pub use crate::graphics::StairPart as PartKind;

/// A solid of a stair with the part of it it is: a ring in plan between two planes.
pub type Tagged = (PartKind, Vec<P2>, Plane, Plane);

/// [`parts`], each with what part of the stair it is (its run, a riser, a landing, a support).
pub fn parts_tagged(m: &Model, st: &Stair) -> Option<Vec<Tagged>> {
    // Each solid's part, as the solids are pushed: those pushed since the last call are `p`.
    fn tag(out: &[(Vec<P2>, Plane, Plane)], tags: &mut Vec<PartKind>, p: PartKind) {
        tags.resize(out.len(), p);
    }
    let mut tags: Vec<PartKind> = Vec::new();
    let t = m.types.stair.get(&st.type_name)?;
    let (_, first_each, _) = m.flight(st);
    let l = stair(m, st);
    let base = m.stair_span(st).0;
    // The heights its storeys stand at: a run from one of them stands on that floor.
    let floors: Vec<f64> = m.storeys(st).into_iter().map(|s| s.1).collect();
    let mut out: Vec<(Vec<P2>, Plane, Plane)> = Vec::new();
    let open = !t.monolithic();
    // How thick a landing or a winder is.
    let flat = if open { t.tread() } else { t.slab() };
    // The finish on each tread: risers are measured finish to finish, so the structure sits that much lower.
    let fin = t.tread_cover.max(0.0);
    let lean = lean(t);
    for run in &l.runs {
        let half = run.width / 2.0;
        // Each riser of this run, which a storey of its own height makes its own; and how far a sloping slab reaches
        // down under the corner of a step.
        let each = if run.risers > 0 { (run.z.1 - run.z.0) / f64::from(run.risers) } else { first_each };
        let going = if run.risers > 1 { run.going() } else { st.tread };
        let slope = if going > 0.0 { each / going } else { 0.0 };
        let down = t.slab() * (1.0 + slope * slope).sqrt();
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
        let floor = if floors.iter().any(|z| (run.z.0 - z).abs() < 1e-6) { run.z.0 } else { run.z.0 - flat - fin };
        for k in 1..run.risers {
            let k = f64::from(k);
            let (a, b, top) = ((k - 1.0) * going, k * going, run.z.0 + k * each - fin);
            if open {
                // The tread, out over the one below by its nosing; and the riser under its front edge.
                out.push((rect(a - t.nosing, b, -half, half), Plane::flat(top - t.tread()), Plane::flat(top)));
                tag(&out, &mut tags, PartKind::Runs);
                if t.riser_thickness > 0.0 {
                    let from = if k > 1.0 { top - each } else { run.z.0 };
                    out.push((rect(a, a + t.riser_thickness, -half, half), Plane::flat(from), Plane::flat(top - t.tread())));
                    tag(&out, &mut tags, PartKind::Risers);
                }
            } else {
                under(&mut out, a, b, -half, half, run.z.0 - down - fin, floor, Plane::flat(top));
                // A slanted riser: the tread's front edge out over its foot, the riser leaning back under it.
                if lean > 0.0 {
                    out.extend(extrude(run, &[(a - lean, top), (a, top), (a, top - each)], -half, half));
                }
                tag(&out, &mut tags, PartKind::Runs);
            }
        }
        // Onto a landing: the slab's underside goes on up under it until it meets the landing's.
        if !open && slope > 0.0 && run.risers > 0 {
            let (a, b) = (run.end() - n * half, run.end() + n * half);
            out.extend(joint(&l, a, b, u, run.z.1, run.z.1 - each - down - fin, slope, flat + fin));
        }
        // Onto a landing, its riser slanted too.
        if lean > 0.0 && run.risers > 0 && l.pads.iter().any(|p| !p.tread && (p.z - run.z.1).abs() < 1e-6) {
            let top = run.z.1 - fin;
            out.extend(extrude(run, &[(run.length - lean, top), (run.length, top), (run.length, top - each)], -half, half));
        }
        tag(&out, &mut tags, PartKind::Runs);
        if open {
            // The last riser, up to what the run climbs onto.
            if t.riser_thickness > 0.0 && run.risers > 0 {
                let from = if run.risers > 1 { run.z.1 - each - fin } else { run.z.0 };
                out.push((
                    rect(run.length, run.length + t.riser_thickness, -half, half),
                    Plane::flat(from),
                    Plane::flat(run.z.1 - flat - fin),
                ));
                tag(&out, &mut tags, PartKind::Risers);
            }
            // A stringer down each side: its top 50 over the nosings' line, its underside its depth below that.
            if let (Some((sw, sd)), true) = (t.side_stringers(), run.length > 0.0) {
                let top = run.z.0 + each + 50.0;
                for (c, d) in [(-half, -half + sw), (half - sw, half)] {
                    under(&mut out, 0.0, run.length, c, d, top - sd, floor, sloping(top));
                }
            }
            // A carriage down the middle: its top through the front corners of the treads' undersides.
            if let (Some((cw, cd)), true) = (t.centre_stringer(), run.length > 0.0) {
                let top = run.z.0 + each - t.tread() - fin;
                let c = (cw / 2.0).min(half);
                under(&mut out, 0.0, run.length, -c, c, top - cd, floor, sloping(top));
            }
            tag(&out, &mut tags, PartKind::Supports);
        }
    }
    // A stringed stair's stringers round its landings too, along each edge no step stands on, where its type says.
    if open
        && t.landing_stringers
        && let Some((sw, sd)) = t.side_stringers().or_else(|| t.centre_stringer())
    {
        for pad in l.pads.iter().filter(|p| !p.tread) {
            for (a, b) in landing_stringers(&l, pad, sw) {
                out.push((ccw(vec![a.0, a.1, b.1, b.0]), Plane::flat((pad.z + 50.0 - sd).max(base)), Plane::flat(pad.z + 50.0)));
            }
        }
    }
    // A spiral's post, from its foot to its top.
    if let (Some(c), true) = (spiral_centre(st), t.post > 0.0) {
        let r = t.post / 2.0;
        let ring: Vec<P2> = (0..24).map(|i| c + heading(f64::from(i) * 15.0) * r).collect();
        let top = l.walk.last().map_or(base, |w| w.1);
        out.push((ring, Plane::flat(base), Plane::flat(top)));
    }
    tag(&out, &mut tags, PartKind::Supports);
    // Treads that are no part of a run (a spiral's, a curved flight's, winders), lowest first: each with the corners
    // it shares with the tread under it and the tread over it, so the slab's underside runs on unbroken from one to
    // the next.
    let steps: Vec<&Pad> = {
        let mut t: Vec<&Pad> = l.pads.iter().filter(|p| p.tread).collect();
        t.sort_by(|a, b| a.z.total_cmp(&b.z));
        t
    };
    let shares = |p: &Pad, q: &Pad| -> Vec<P2> { p.ring.iter().copied().filter(|v| q.ring.iter().any(|w| w.dist(*v) < 1e-3)).collect() };
    let mut smooth: Vec<bool> = vec![false; l.pads.len()];
    if !open {
        // The slab's thickness, square to the stair's usual slope.
        let slope = if st.tread > 0.0 { first_each / st.tread } else { 0.0 };
        let down = t.slab() * (1.0 + slope * slope).sqrt();
        for (k, pad) in steps.iter().enumerate() {
            let near = |j: Option<usize>, rise: f64| {
                j.and_then(|j| steps.get(j))
                    .filter(|q| ((q.z - pad.z) - rise).abs() < first_each * 0.5)
                    .map(|q| shares(pad, q))
                    .filter(|v| v.len() >= 2)
            };
            let (front, back) = (near(k.checked_sub(1), -first_each), near(Some(k + 1), first_each));
            let mid = |v: &[P2]| v.iter().fold(P2(0.0, 0.0), |a, q| a + *q) * (1.0 / v.len().max(1) as f64);
            let c = geom::centroid(&pad.ring);
            let (fm, bm) = match (&front, &back) {
                (Some(f), Some(b)) => (mid(f), mid(b)),
                (Some(f), None) => (mid(f), c * 2.0 - mid(f)),
                (None, Some(b)) => (c * 2.0 - mid(b), mid(b)),
                (None, None) => continue,
            };
            let span = bm - fm;
            if span.length() < 1.0 {
                continue;
            }
            // How far from its front edge to its back a corner is, 0 to 1; a shared corner exactly so.
            let on = |v: P2, of: &Option<Vec<P2>>| of.as_ref().is_some_and(|e| e.iter().any(|w| w.dist(v) < 1e-3));
            let height = |v: P2| {
                let t = if on(v, &front) {
                    0.0
                } else if on(v, &back) {
                    1.0
                } else {
                    ((v - fm).dot(span) / span.dot(span)).clamp(0.0, 1.0)
                };
                (pad.z - first_each + t * first_each - down - fin).max(base)
            };
            let top = Plane::flat(pad.z - fin);
            for tri in geom::triangulate(&pad.ring) {
                let [Some(a), Some(b), Some(c)] = tri.map(|i| pad.ring.get(i).copied()) else { continue };
                let (za, zb, zc) = (height(a), height(b), height(c));
                // The plane through the three: z = z0 + g·p.
                let (e1, e2) = (b - a, c - a);
                let det = e1.0 * e2.1 - e1.1 * e2.0;
                if det.abs() < 1e-9 {
                    continue;
                }
                let (d1, d2) = (zb - za, zc - za);
                let g = P2((d1 * e2.1 - d2 * e1.1) / det, (e1.0 * d2 - e2.0 * d1) / det);
                out.push((ccw(vec![a, b, c]), Plane { z: za - g.dot(a), g }, top));
            }
            if let Some(i) = l.pads.iter().position(|p| std::ptr::eq(p, *pad)) {
                smooth[i] = true;
            }
        }
    }
    if !open && st.tread > 0.0 {
        let slope = first_each / st.tread;
        let down = t.slab() * (1.0 + slope * slope).sqrt();
        for s in &l.nosings {
            out.extend(joint(&l, s.a, s.b, s.out * -1.0, s.z, s.z - s.rise - down - fin, slope, flat + fin));
        }
    }
    tag(&out, &mut tags, PartKind::Runs);
    for (i, pad) in l.pads.iter().enumerate() {
        // What the last one left is a tread's.
        tag(&out, &mut tags, PartKind::Runs);
        // Done smooth above; and a tread level with the floor it stands on is the floor.
        if smooth.get(i).copied().unwrap_or(false) || (pad.tread && pad.z <= base + 1e-6) {
            continue;
        }
        if !pad.tread || open {
            out.push((pad.ring.clone(), Plane::flat((pad.z - flat - fin).max(base)), Plane::flat(pad.z - fin)));
            tag(&out, &mut tags, if pad.tread { PartKind::Runs } else { PartKind::Landings });
            // A tread round a turn on a centre stringer: a piece of the stringer under it, along the walk line.
            if let (true, Some((cw, cd))) = (pad.tread && open, t.centre_stringer()) {
                let c = geom::centroid(&pad.ring);
                let way = l.walk.windows(2).filter_map(|w| Some((w.first()?.0, w.get(1)?.0))).filter(|(a, b)| a.dist(*b) > 1e-6).min_by(
                    |x, y| {
                        let far = |(a, b): &(P2, P2)| geom::dist_to_segment(c, *a, *b);
                        far(x).total_cmp(&far(y))
                    },
                );
                if let Some((a, b)) = way {
                    let (u, n) = ((b - a).unit(), (b - a).unit().perp());
                    let along: Vec<f64> = pad.ring.iter().map(|q| (*q - c).dot(u)).collect();
                    let (lo, hi) = along.iter().fold((0.0f64, 0.0f64), |(lo, hi), x| (lo.min(*x), hi.max(*x)));
                    let h = cw / 2.0;
                    let ring = ccw(vec![c + u * lo - n * h, c + u * hi - n * h, c + u * hi + n * h, c + u * lo + n * h]);
                    let top = pad.z - fin - flat;
                    out.push((ring, Plane::flat((top - cd).max(base)), Plane::flat(top)));
                    tag(&out, &mut tags, PartKind::Supports);
                }
            }
            continue;
        }
        // A tread that is no part of a run (a winder, a spiral's, one drawn by hand) on a slab: the slab's underside
        // slopes under it the way the stair goes there, as it does under a run, so the underside goes on unbroken.
        let c = geom::centroid(&pad.ring);
        let way = l.walk.windows(2).filter_map(|w| Some((w.first()?.0, w.get(1)?.0))).filter(|(a, b)| a.dist(*b) > 1e-6).min_by(|x, y| {
            let far = |(a, b): &(P2, P2)| ((*a + *b) * 0.5).dist(c);
            far(x).total_cmp(&far(y))
        });
        let level = Plane::flat((pad.z - first_each - t.slab() - fin).max(base));
        let top = Plane::flat(pad.z - fin);
        let Some((a, b)) = way else {
            out.push((pad.ring.clone(), level, top));
            continue;
        };
        let u = (b - a).unit();
        // How deep the tread is the way the stair goes, and where its front edge is.
        let (near, far) = pad.ring.iter().map(|q| q.dot(u)).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)));
        let depth = far - near;
        if depth < 1.0 {
            out.push((pad.ring.clone(), level, top));
            continue;
        }
        let slope = first_each / depth;
        // Under its front edge, the slab is its thickness (square to the slope) below the step under this one.
        let front = pad.z - fin - first_each - t.slab() * (1.0 + slope * slope).sqrt();
        if front < base - 1e-6 {
            out.push((pad.ring.clone(), Plane::flat(base), top));
        } else {
            let g = u * slope;
            out.push((pad.ring.clone(), Plane { z: front - slope * near, g }, top));
        }
        tag(&out, &mut tags, PartKind::Runs);
    }
    tag(&out, &mut tags, PartKind::Runs);
    Some(tags.into_iter().zip(out).map(|(k, (ring, lo, hi))| (k, ring, lo, hi)).collect())
}

/// Where a flight's sloping slab meets a landing at `z` past the riser line `a`–`b`, `inward` the way the stair goes on:
/// the wedge under the landing between the slab's underside, `under` high at the line and rising `slope` inward, and the
/// landing's underside, `deep` below `z`, so the two undersides meet. None where there is no landing there.
#[allow(clippy::too_many_arguments)]
fn joint(l: &Layout, a: P2, b: P2, inward: P2, z: f64, under: f64, slope: f64, deep: f64) -> Option<(Vec<P2>, Plane, Plane)> {
    let bottom = z - deep;
    if slope <= 0.0 || under >= bottom - 1e-6 {
        return None;
    }
    let probe = (a + b) * 0.5 + inward * 1.0;
    let pad = l.pads.iter().find(|p| !p.tread && (p.z - z).abs() < 1e-6 && geom::contains(&p.ring, probe))?;
    // As far as the landing goes that way.
    let reach = pad.ring.iter().map(|q| (*q - a).dot(inward)).fold(0.0, f64::max);
    // A landing too short to take it (what a flight leaves short of winders) has none.
    let x = (bottom - under) / slope;
    if x < 1.0 || x > reach {
        return None;
    }
    let g = inward * slope;
    Some((ccw(vec![a, b, b + inward * x, a + inward * x]), Plane { z: under - g.dot(a), g }, Plane::flat(bottom)))
}

/// A solid along `run`, from `c` to `d` across it, whose section up the run is the convex polygon `section` of points
/// (along, height): as pieces each a ring in plan between two planes, one for each stretch between its corners.
fn extrude(run: &Run, section: &[(f64, f64)], c: f64, d: f64) -> Vec<(Vec<P2>, Plane, Plane)> {
    let (u, n) = (run.u, run.u.perp());
    let mut xs: Vec<f64> = section.iter().map(|q| q.0).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    // How low and how high the polygon reaches at `x`.
    let span = |x: f64| {
        let mut zs = Vec::new();
        for i in 0..section.len() {
            let (p, q) = (section[i], section[(i + 1) % section.len()]);
            if (p.0 - q.0).abs() > 1e-9 && (p.0.min(q.0)..=p.0.max(q.0)).contains(&x) {
                zs.push(p.1 + (q.1 - p.1) * (x - p.0) / (q.0 - p.0));
            }
        }
        zs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| (lo.min(*z), hi.max(*z)))
    };
    // The plane through heights `z0` at `x0` and `z1` at `x1` along the run.
    let plane = |x0: f64, z0: f64, x1: f64, z1: f64| {
        let slope = (z1 - z0) / (x1 - x0);
        Plane { z: z0 - slope * x0 - slope * u.dot(run.start), g: u * slope }
    };
    let mut out = Vec::new();
    for w in xs.windows(2) {
        let (x0, x1) = (w[0], w[1]);
        if x1 - x0 < 1e-6 {
            continue;
        }
        // Inside the stretch both chains are straight: read them a quarter in from each end.
        let (p, q) = (x0 + (x1 - x0) * 0.25, x0 + (x1 - x0) * 0.75);
        let ((lo_p, hi_p), (lo_q, hi_q)) = (span(p), span(q));
        if !(lo_p.is_finite() && hi_p.is_finite() && lo_q.is_finite() && hi_q.is_finite()) {
            continue;
        }
        let ring =
            ccw(vec![run.start + u * x0 + n * c, run.start + u * x1 + n * c, run.start + u * x1 + n * d, run.start + u * x0 + n * d]);
        out.push((ring, plane(p, lo_p, q, lo_q), plane(p, hi_p, q, hi_q)));
    }
    out
}

/// A finish `fin` thick, its top at `top`, reaching `depth` back from its front edge: `ring(x0, x1)` is its piece from
/// `x0` to `x1` back from that edge. With a profile other than square, its front edge is shaped in thin slices, each set
/// back as the profile is at its height.
fn profiled(
    out: &mut Vec<(Vec<P2>, Plane, Plane)>,
    profile: crate::model::FinishProfile,
    fin: f64,
    top: f64,
    depth: f64,
    ring: impl Fn(f64, f64) -> Vec<P2>,
) {
    let r = if profile.is_square() { 0.0 } else { (fin * 0.5).min(depth) };
    if depth - r > 1e-6 {
        out.push((ring(r, depth), Plane::flat(top - fin), Plane::flat(top)));
    }
    if r <= 0.0 {
        return;
    }
    const SLICES: u32 = 6;
    for k in 0..SLICES {
        let (z0, z1) = (top - fin + fin * f64::from(k) / f64::from(SLICES), top - fin + fin * f64::from(k + 1) / f64::from(SLICES));
        let inset = fin * profile.inset((f64::from(k) + 0.5) / f64::from(SLICES));
        if inset < r - 1e-6 {
            out.push((ring(inset, r), Plane::flat(z0), Plane::flat(z1)));
        }
    }
}

/// How far a slanted riser of type `t` leans back from the tread's front edge to its foot; 0 for a square one.
fn lean(t: &crate::model::StairType) -> f64 {
    if t.profile == crate::model::StepProfile::Slanted && t.monolithic() { t.nosing.max(0.0) } else { 0.0 }
}

/// How far the front edge of a tread of type `t`, as seen from above, stands out past the line of its riser: the
/// structure's nosing, or the finish's, past the riser's cover.
pub fn reach(t: &crate::model::StairType) -> f64 {
    let structure = if t.monolithic() { lean(t) } else { t.nosing.max(0.0) };
    if t.tread_cover <= 0.0 {
        return structure;
    }
    let risers = t.riser_cover > 0.0 && (t.monolithic() || t.riser_thickness > 0.0);
    if lean(t) > 0.0 {
        return structure + if risers { t.riser_cover } else { 0.0 } + t.cover_nosing.max(0.0);
    }
    (if risers { t.riser_cover } else { 0.0 }).max(structure) + t.cover_nosing.max(0.0)
}

/// The finishes a stair's type lays over its structure, each a ring in plan between two planes: a cover on each tread
/// and landing, its top the finished step, standing out at the front past the riser's cover; and a cover on the face of
/// each riser. None for a stair of no type, or a type with no covers.
pub fn covers(m: &Model, st: &Stair) -> Vec<(Vec<P2>, Plane, Plane)> {
    let Some(t) = m.types.stair.get(&st.type_name) else { return Vec::new() };
    let (fin, rc) = (t.tread_cover.max(0.0), t.riser_cover.max(0.0));
    if fin <= 0.0 && rc <= 0.0 {
        return Vec::new();
    }
    let l = stair(m, st);
    let floors: Vec<f64> = m.storeys(st).into_iter().map(|s| s.1).collect();
    // A riser there is to cover: a monolithic stair's, or a type with risers.
    let risers = rc > 0.0 && (t.monolithic() || t.riser_thickness > 0.0);
    // A side with no stringer is open, and the tread's cover stands out past it.
    let side = if t.side_stringers().is_some() { 0.0 } else { t.cover_side.max(0.0) };
    let profile = t.finish_profile;
    // How far a tread's cover reaches out past the line of its riser.
    let front = reach(t);
    let slant = lean(t);
    let mut out = Vec::new();
    for run in &l.runs {
        let half = run.width / 2.0;
        let each = if run.risers > 0 { (run.z.1 - run.z.0) / f64::from(run.risers) } else { 0.0 };
        let going = if run.risers > 1 { run.going() } else { st.tread };
        let (u, n) = (run.u, run.u.perp());
        let rect = |a: f64, b: f64, c: f64, d: f64| {
            ccw(vec![run.start + u * a + n * c, run.start + u * b + n * c, run.start + u * b + n * d, run.start + u * a + n * d])
        };
        for k in 1..run.risers {
            let k = f64::from(k);
            let (a, b, top) = ((k - 1.0) * going, k * going, run.z.0 + k * each);
            if fin > 0.0 {
                let ring = |x0: f64, x1: f64| rect(a - front + x0, a - front + x1, -half - side, half + side);
                profiled(&mut out, profile, fin, top, b - a + front, ring);
            }
            if risers {
                let from = if k > 1.0 { top - each } else { run.z.0 };
                if slant > 0.0 {
                    // Along the slanted face of the structure, from the finish under it up to the tread's cover.
                    let face = |z: f64| a - slant * (z - (top - fin - each)) / each;
                    let (zb, zt) = (from, top - fin);
                    let section = [(face(zb) - rc, zb), (face(zb), zb), (face(zt), zt), (face(zt) - rc, zt)];
                    out.extend(extrude(run, &section, -half, half));
                } else {
                    out.push((rect(a - rc, a, -half, half), Plane::flat(from), Plane::flat(top - fin)));
                }
            }
        }
        // What the last riser climbs onto (a landing, the floor, a tread level with it) stands out over it as a tread
        // does, the finish's nosing.
        if fin > 0.0 && run.risers > 0 {
            let ring = |x0: f64, x1: f64| rect(run.length - front + x0, run.length - front + x1, -half - side, half + side);
            profiled(&mut out, profile, fin, run.z.1, front, ring);
        }
        // The last riser, up to that nosing.
        if risers && run.risers > 0 {
            let floor = floors.iter().any(|z| (run.z.1 - z).abs() < 1e-6);
            let onto = run.z.1 - fin;
            let from = if run.risers > 1 { run.z.1 - each } else { run.z.0 };
            let at = run.length;
            if slant > 0.0 && !floor {
                let face = |z: f64| at - slant * (z - (run.z.1 - fin - each)) / each;
                let section = [(face(from) - rc, from), (face(from), from), (face(onto), onto), (face(onto) - rc, onto)];
                out.extend(extrude(run, &section, -half, half));
            } else {
                out.push((rect(at - rc, at, -half, half), Plane::flat(from), Plane::flat(onto)));
            }
        }
    }
    if fin > 0.0 {
        out.extend(l.pads.iter().map(|pad| (pad.ring.clone(), Plane::flat(pad.z - fin), Plane::flat(pad.z))));
    }
    // The risers no run has (round an arc, a spiral's): the riser's cover on its face, and the tread's cover out over it.
    let ahead = if risers { rc } else { 0.0 } + t.cover_nosing.max(0.0);
    let mut nosings = l.nosings.clone();
    // And a winder's: the edge it shares with the step a riser under it.
    let rise = m.flight(st).1;
    let near = |q: P2, a: P2, b: P2| {
        let d = b - a;
        let f = if d.dot(d) > 0.0 { ((q - a).dot(d) / d.dot(d)).clamp(0.0, 1.0) } else { 0.0 };
        q.dist(a + d * f) < 1.0
    };
    for pad in l.pads.iter().filter(|p| p.tread) {
        for lower in l.pads.iter().filter(|q| ((pad.z - q.z) - rise).abs() < rise * 0.5) {
            let shared: Vec<P2> = pad.ring.iter().copied().filter(|v| lower.ring.iter().any(|w| w.dist(*v) < 1.0)).collect();
            let Some((a, b)) = shared
                .iter()
                .flat_map(|a| shared.iter().map(move |b| (*a, *b)))
                .max_by(|x, y| x.0.dist(x.1).total_cmp(&y.0.dist(y.1)))
                .filter(|(a, b)| a.dist(*b) > 1.0)
            else {
                continue;
            };
            let mid = (a + b) * 0.5;
            if nosings.iter().any(|s| near(mid, s.a, s.b)) {
                continue;
            }
            let mut way = (b - a).unit().perp();
            if way.dot(mid - geom::centroid(&pad.ring)) < 0.0 {
                way = way * -1.0;
            }
            nosings.push(Nosing { a, b, out: way, z: pad.z, rise: pad.z - lower.z });
        }
    }
    for s in &nosings {
        let strip = |w: f64| ccw(vec![s.a, s.b, s.b + s.out * w, s.a + s.out * w]);
        if fin > 0.0 && ahead > 0.0 {
            let ring = |x0: f64, x1: f64| {
                ccw(vec![s.a + s.out * (ahead - x1), s.b + s.out * (ahead - x1), s.b + s.out * (ahead - x0), s.a + s.out * (ahead - x0)])
            };
            profiled(&mut out, profile, fin, s.z, ahead, ring);
        }
        if risers {
            out.push((strip(rc), Plane::flat(s.z - s.rise), Plane::flat(s.z - fin)));
        }
    }
    out
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
/// A side of a landing's middle line: each point moved square to it, to the left (`side` 1) or the right (-1), by half the
/// width its point gives the stretch that comes to it (the first point's, the first stretch's), mitred where two stretches
/// meet, and across the step where two in one line differ.
fn beside(line: &[(P2, f64)], side: f64) -> Vec<P2> {
    let halves = |j: usize| line.get(j + 1).map_or(0.0, |q| q.1) * side;
    let dirs: Vec<P2> = line.windows(2).filter_map(|w| Some((w.get(1)?.0 - w.first()?.0).unit())).collect();
    let mut out = Vec::new();
    for (i, &(p, _)) in line.iter().enumerate() {
        let before = i.checked_sub(1).and_then(|j| dirs.get(j).map(|d| (*d, halves(j))));
        let after = dirs.get(i).map(|d| (*d, halves(i)));
        match (before, after) {
            (Some((a, s1)), Some((b, s2))) => {
                let cross = a.0 * b.1 - a.1 * b.0;
                if cross.abs() < 1e-9 {
                    out.push(p + a.perp() * s1);
                    if (s1 - s2).abs() > 1e-6 {
                        out.push(p + b.perp() * s2);
                    }
                } else {
                    let r = b.perp() * s2 - a.perp() * s1;
                    let t = (r.0 * b.1 - r.1 * b.0) / cross;
                    out.push(p + a.perp() * s1 + a * t);
                }
            }
            (Some((a, s)), None) | (None, Some((a, s))) => out.push(p + a.perp() * s),
            (None, None) => {}
        }
    }
    out
}

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
