//! Step 9: jogs and bumps. Each changes one cell and leaves every other room where it is; a change that fails the links or the
//! checks is undone.

use std::collections::{BTreeMap, BTreeSet};

use super::super::fit::Fitter;
use super::super::links::{Edge, Rc, shared};
use super::{Acc, Bump, Cell, Ctx, Fail, Kind, Orient, R, Rnd, connect, outline, place, verify};

/// What the changes need to judge a plan.
pub struct Step<'s, 'a> {
    pub cx: &'s Ctx<'a>,
    pub n_main: usize,
    /// The catalogue's corner count for the shape.
    pub corners: usize,
}

/// A bump as made, before the plan is mirrored and turned: the cell's outward direction.
pub struct Placed {
    room: usize,
    normal: (i64, i64),
    offset: i64,
}

impl Placed {
    /// The bump in the final plan: the side its wall faces after the mirror and the quarter turns.
    pub fn turned(self, mirror: bool, turns: usize) -> Bump {
        let (mut x, mut y) = self.normal;
        if mirror {
            x = -x;
        }
        for _ in 0..turns {
            (x, y) = (-y, x);
        }
        let side = match (x, y) {
            (0, 1) => "N",
            (0, -1) => "S",
            (1, 0) => "E",
            _ => "W",
        };
        Bump { room: self.room, side, offset: self.offset }
    }
}

/// The cell's rectangle with its depth `d`: x, y, width, height.
fn cell_rect(c: &Cell, d: i64) -> (i64, i64, i64, i64) {
    let r = place(&[(0, 0, 0, c.f, d)], c.ox, c.oy, c.orient)[0];
    (r.1, r.2, r.3, r.4)
}

/// The front or the back of a cell of depth `d`.
fn edge(c: &Cell, d: i64, back: bool) -> Edge {
    let (x, y, w, h) = cell_rect(c, d);
    match c.orient {
        Orient::A | Orient::B => Edge { vertical: false, at: if (c.orient == Orient::A) == back { y + h } else { y }, lo: x, hi: x + w },
        Orient::L | Orient::R => Edge { vertical: true, at: if (c.orient == Orient::R) == back { x + w } else { x }, lo: y, hi: y + h },
    }
}

/// The rectangles, not of the rooms in `skip`, that have a side on the edge, each with the length they share with it.
fn touching(rects: &[Rc], e: &Edge, skip: &BTreeSet<usize>) -> Vec<(Rc, i64)> {
    let mut out = Vec::new();
    for r in rects.iter().filter(|r| !skip.contains(&r.0)) {
        let (a0, b0, on) = if e.vertical {
            (r.2, r.2 + r.4, e.at == r.1 || e.at == r.1 + r.3)
        } else {
            (r.1, r.1 + r.3, e.at == r.2 || e.at == r.2 + r.4)
        };
        let len = e.hi.min(b0) - e.lo.max(a0);
        if on && len > 0 {
            out.push((*r, len));
        }
    }
    out
}

/// A cell fitted again `deeper` mm deeper, its front moved `shift` mm away from the spine.
fn refit(fx: &Fitter, c: &Cell, deeper: i64, shift: i64) -> Option<Vec<Rc>> {
    let d = c.d + deeper;
    let got = fx.fit(&c.cl, c.f, d, c.lat, c.cover, c.pref).or_else(|| fx.fit(&c.cl, c.f, d, c.lat, c.cover, 0))?;
    let (ox, oy) = match c.orient {
        Orient::R => (c.ox + shift, c.oy),
        Orient::L => (c.ox - shift, c.oy),
        Orient::A => (c.ox, c.oy + shift),
        Orient::B => (c.ox, c.oy - shift),
    };
    Some(place(&got, ox, oy, c.orient))
}

/// Would this plan pass? Links are tried with a throwaway random source, so the real one is not used up.
fn passes(step: &Step, rects: &[Rc], corners: Option<usize>) -> bool {
    let cx = step.cx;
    let mut trial = Rnd::new(0, 0);
    connect(cx.fam, cx.rooms, cx.g, rects, step.n_main, &mut trial)
        .and_then(|linked| verify(cx.fam, cx.rooms, rects, &linked, corners))
        .is_ok()
}

fn offsets(rng: &mut Rnd) -> Vec<i64> {
    let mut all: Vec<i64> = (300..=1200).step_by(50).collect();
    rng.shuffle(&mut all);
    all.truncate(6);
    all
}

/// The pairs of rooms, neither of them a spine room, that share a wall of at least 300 mm and face the same spine room on the same
/// side from lines 300 to 1200 mm apart: what a check calls a jog.
fn jog_pairs(rects: &[Rc], spines: &BTreeSet<usize>) -> usize {
    // For each room, the spine room it faces, the side, and the lines it faces it across with how much of its wall is on each.
    let mut face: BTreeMap<(usize, usize, bool, i64), BTreeMap<i64, i64>> = BTreeMap::new();
    for a in rects.iter().filter(|r| !spines.contains(&r.0)) {
        for q in rects.iter().filter(|r| spines.contains(&r.0)) {
            let mut add = |vertical: bool, sign: i64, at: i64, len: i64| {
                *face.entry((a.0, q.0, vertical, sign)).or_default().entry(at).or_default() += len;
            };
            let along_x = (a.1 + a.3).min(q.1 + q.3) - a.1.max(q.1);
            if along_x > 0 {
                if q.2 + q.4 == a.2 {
                    add(false, -1, a.2, along_x);
                }
                if q.2 == a.2 + a.4 {
                    add(false, 1, a.2 + a.4, along_x);
                }
            }
            let along_y = (a.2 + a.4).min(q.2 + q.4) - a.2.max(q.2);
            if along_y > 0 {
                if q.1 + q.3 == a.1 {
                    add(true, -1, a.1, along_y);
                }
                if q.1 == a.1 + a.3 {
                    add(true, 1, a.1 + a.3, along_y);
                }
            }
        }
    }
    // Room, spine, side: the line with most of the wall.
    let mut best: BTreeMap<(usize, bool, i64), Vec<(usize, i64)>> = BTreeMap::new();
    for ((room, spine, vertical, sign), lines) in &face {
        if let Some((line, _)) = lines.iter().max_by_key(|(line, len)| (**len, -**line)) {
            best.entry((*spine, *vertical, *sign)).or_default().push((*room, *line));
        }
    }
    let wall = |a: usize, b: usize| -> i64 {
        let mut total = 0;
        for x in rects.iter().filter(|r| r.0 == a) {
            for y in rects.iter().filter(|r| r.0 == b) {
                total += shared((x.1, x.2, x.3, x.4), (y.1, y.2, y.3, y.4)).map_or(0, |e| e.len());
            }
        }
        total
    };
    let mut count = 0;
    for rooms in best.values() {
        for (n, (a, la)) in rooms.iter().enumerate() {
            for (b, lb) in &rooms[n + 1..] {
                if (300..=1200).contains(&(la - lb).abs()) && wall(*a, *b) >= 300 {
                    count += 1;
                }
            }
        }
    }
    count
}

/// Sets some cells' fronts back from the spine by 300 to 1200 mm. The spine room takes the strip they leave. A plan of eight rooms
/// or more needs two jogs, a smaller one a jog.
pub fn jog(step: &Step, acc: &mut Acc, rng: &mut Rnd) -> R<()> {
    let cx = step.cx;
    let want = if cx.rooms.len() >= 8 { 2 } else { 1 };
    let spines: BTreeSet<usize> = cx.g.spines.iter().copied().collect();
    let mut order: Vec<usize> = (0..acc.cells.len()).filter(|i| acc.cells[*i].kind == Kind::Band).collect();
    rng.shuffle(&mut order);
    let mut count = jog_pairs(&acc.rects, &spines);
    let (mut jogs, mut enough) = (0, None);
    for i in order {
        if count >= want {
            let n = match enough {
                Some(n) => n,
                None => {
                    let n = rng.pick(&[1, 2, 3])?;
                    enough = Some(n);
                    n
                }
            };
            if jogs >= n {
                break;
            }
        }
        let c = acc.cells[i].clone();
        // The front must lie on one rectangle of one spine room, along its whole length.
        let on = touching(&acc.rects, &edge(&c, c.d, false), &c.rooms);
        let Some(&(spine, len)) = on.first().filter(|_| on.len() == 1) else { continue };
        if !spines.contains(&spine.0) || len != c.f {
            continue;
        }
        for e in offsets(rng) {
            let Some(new) = refit(cx.fx, &c, -e, e) else { continue };
            let strip = place(&[(spine.0, 0, 0, c.f, e)], c.ox, c.oy, c.orient);
            let mut trial: Vec<Rc> = acc.rects.iter().filter(|r| !c.rooms.contains(&r.0)).copied().collect();
            trial.extend(new);
            trial.extend(strip);
            let now = jog_pairs(&trial, &spines);
            if now <= count || !passes(step, &trial, Some(step.corners)) {
                continue;
            }
            (acc.rects, count) = (trial, now);
            acc.cells[i].kind = Kind::Changed;
            jogs += 1;
            break;
        }
    }
    if count < want { Err(Fail("no jog")) } else { Ok(()) }
}

fn inside(poly: &[(i64, i64)], x: f64, y: f64) -> bool {
    let mut c = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let ((ax, ay), (bx, by)) = ((a.0 as f64, a.1 as f64), (b.0 as f64, b.1 as f64));
        if (ay > y) != (by > y) && x < ax + (y - ay) / (by - ay) * (bx - ax) {
            c = !c;
        }
    }
    c
}

/// The parts of `a` outside `b`, each one as its bounding box and whether it is a rectangle.
fn regions(a: &[(i64, i64)], b: &[(i64, i64)]) -> Vec<([i64; 4], bool)> {
    let grid = |axis: usize| -> Vec<i64> {
        let v: BTreeSet<i64> = a.iter().chain(b).map(|p| if axis == 0 { p.0 } else { p.1 }).collect();
        v.into_iter().collect()
    };
    let (xs, ys) = (grid(0), grid(1));
    let mut cells: BTreeSet<(usize, usize)> = BTreeSet::new();
    for i in 0..xs.len().saturating_sub(1) {
        for j in 0..ys.len().saturating_sub(1) {
            let (cx, cy) = ((xs[i] + xs[i + 1]) as f64 / 2.0, (ys[j] + ys[j + 1]) as f64 / 2.0);
            if inside(a, cx, cy) && !inside(b, cx, cy) {
                cells.insert((i, j));
            }
        }
    }
    let mut out = Vec::new();
    let mut seen: BTreeSet<(usize, usize)> = BTreeSet::new();
    for &c in &cells {
        if !seen.insert(c) {
            continue;
        }
        let (mut comp, mut todo) = (vec![c], vec![c]);
        while let Some((i, j)) = todo.pop() {
            for d in [(i + 1, j), (i.wrapping_sub(1), j), (i, j + 1), (i, j.wrapping_sub(1))] {
                if cells.contains(&d) && seen.insert(d) {
                    comp.push(d);
                    todo.push(d);
                }
            }
        }
        let area: i64 = comp.iter().map(|&(i, j)| (xs[i + 1] - xs[i]) * (ys[j + 1] - ys[j])).sum();
        let lo = |k: usize, v: &[i64]| v[comp.iter().map(|c| if k == 0 { c.0 } else { c.1 }).min().unwrap_or(0)];
        let hi = |k: usize, v: &[i64]| v[comp.iter().map(|c| if k == 0 { c.0 } else { c.1 }).max().unwrap_or(0) + 1];
        let bb = [lo(0, &xs), lo(1, &ys), hi(0, &xs), hi(1, &ys)];
        out.push((bb, area == (bb[2] - bb[0]) * (bb[3] - bb[1])));
    }
    out
}

/// What the outline has gained (+1) and lost (-1) against the base, as (sign, depth) for each part, or `None` unless every part is
/// one rectangle that stands on one side of the base outline.
fn bump_parts(out: &[(i64, i64)], base: &[(i64, i64)]) -> Option<Vec<(i64, i64)>> {
    let mut parts = Vec::new();
    for (sign, a, b) in [(1, out, base), (-1, base, out)] {
        for (bb, rect) in regions(a, b) {
            if !rect {
                return None;
            }
            let (mx, my) = ((bb[0] + bb[2]) as f64 / 2.0, (bb[1] + bb[3]) as f64 / 2.0);
            let (x0, y0, x1, y1) = (bb[0] as f64, bb[1] as f64, bb[2] as f64, bb[3] as f64);
            let sides = [
                ((mx, y0 - 5.0), bb[3] - bb[1]),
                ((mx, y1 + 5.0), bb[3] - bb[1]),
                ((x0 - 5.0, my), bb[2] - bb[0]),
                ((x1 + 5.0, my), bb[2] - bb[0]),
            ];
            let on: Vec<i64> = sides.iter().filter(|((px, py), _)| inside(base, *px, *py) == (sign > 0)).map(|(_, depth)| *depth).collect();
            if on.len() != 1 {
                return None;
            }
            parts.push((sign, on[0]));
        }
    }
    parts.sort_unstable();
    Some(parts)
}

/// The ground an outward bump moves onto, and 600 mm round that, holds no other room.
fn ground_clear(rects: &[Rc], c: &Cell, e: i64) -> bool {
    let back = edge(c, c.d, true);
    let (lo, hi) = (back.lo - 600, back.hi + 600);
    let reach = e + 600;
    let (x0, x1, y0, y1) = match c.orient {
        Orient::A => (lo, hi, back.at, back.at + reach),
        Orient::B => (lo, hi, back.at - reach, back.at),
        Orient::R => (back.at, back.at + reach, lo, hi),
        Orient::L => (back.at - reach, back.at, lo, hi),
    };
    !rects.iter().any(|r| !c.rooms.contains(&r.0) && x0 < r.1 + r.3 && r.1 < x1 && y0 < r.2 + r.4 && r.2 < y1)
}

/// Moves the back wall of one to three cells out past the outline, or in from it, by 300 to 1200 mm. Fails with no bump.
pub fn bump(step: &Step, acc: &mut Acc, base: &[(i64, i64)], rng: &mut Rnd) -> R<Vec<Placed>> {
    let cx = step.cx;
    let want = rng.pick(&[1usize, 2, 3])?;
    let mut order: Vec<usize> = (0..acc.cells.len()).filter(|i| matches!(acc.cells[*i].kind, Kind::Band | Kind::Cap)).collect();
    rng.shuffle(&mut order);
    let mut done: Vec<Placed> = Vec::new();
    let mut bumped: Vec<usize> = Vec::new();
    for i in order {
        if done.len() >= want {
            break;
        }
        let c = acc.cells[i].clone();
        // The whole back wall must be on the outline, and no neighbour may have bumped: two bumps side by side are one part.
        if !touching(&acc.rects, &edge(&c, c.d, true), &c.rooms).is_empty() {
            continue;
        }
        let me = cell_rect(&c, c.d);
        if bumped.iter().any(|j| shared(me, cell_rect(&acc.cells[*j], acc.cells[*j].d)).is_some()) {
            continue;
        }
        // Each amount is tried both ways, in an order the seed picks.
        let tries: Vec<(i64, bool)> = offsets(rng)
            .into_iter()
            .flat_map(|e| if rng.random() < 0.5 { [(e, true), (e, false)] } else { [(e, false), (e, true)] })
            .collect();
        for (e, out) in tries {
            let deeper = if out { e } else { -e };
            let Some(new) = refit(cx.fx, &c, deeper, 0) else { continue };
            if out && !ground_clear(&acc.rects, &c, e) {
                continue;
            }
            let mut trial: Vec<Rc> = acc.rects.iter().filter(|r| !c.rooms.contains(&r.0)).copied().collect();
            trial.extend(new);
            let Ok(shape) = outline(&trial) else { continue };
            let mut wanted: Vec<(i64, i64)> = done.iter().map(|b| (b.offset.signum(), b.offset.abs())).collect();
            wanted.push((if out { 1 } else { -1 }, e));
            wanted.sort_unstable();
            if bump_parts(&shape, base) != Some(wanted) || !passes(step, &trial, None) {
                continue;
            }
            // The room the wall belongs to: the one of the cell with most of its side on the wall's new place.
            let now = touching(&trial, &edge(&c, c.d + deeper, true), &BTreeSet::new());
            let room = now.iter().filter(|(r, _)| c.rooms.contains(&r.0)).max_by_key(|(_, len)| *len).map_or(c.cl.head, |(r, _)| r.0);
            let normal = match c.orient {
                Orient::A => (0, 1),
                Orient::B => (0, -1),
                Orient::R => (1, 0),
                Orient::L => (-1, 0),
            };
            done.push(Placed { room, normal, offset: if out { e } else { -e } });
            bumped.push(i);
            acc.rects = trial;
            acc.cells[i].kind = Kind::Changed;
            break;
        }
    }
    if done.is_empty() { Err(Fail("no bump")) } else { Ok(done) }
}
