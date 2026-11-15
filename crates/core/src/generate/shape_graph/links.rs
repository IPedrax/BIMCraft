//! Step 8: what rectangles share, the links between rooms, the outside door and the checks every plan must hold.

use std::collections::{BTreeMap, BTreeSet};

use super::doors::cased_gap;
use super::family::{Conn, Family, Use};
use super::fit::{Fail, R, Rnd, TE, TI};
use super::plan::{Graph, Parent, Room};

/// A rectangle: x, y, width, depth.
pub type Box4 = (i64, i64, i64, i64);
/// A room's rectangle: the room, then the box.
pub type Rc = (usize, i64, i64, i64, i64);

/// A stretch of wall: along y = `at` (not vertical) or x = `at` (vertical), from `lo` to `hi`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Edge {
    pub vertical: bool,
    pub at: i64,
    pub lo: i64,
    pub hi: i64,
}

impl Edge {
    pub fn len(&self) -> i64 {
        self.hi - self.lo
    }

    pub fn point(&self, along: i64) -> [i64; 2] {
        if self.vertical { [self.at, along] } else { [along, self.at] }
    }
}

pub fn boxed(r: &Rc) -> Box4 {
    (r.1, r.2, r.3, r.4)
}

/// The wall two rectangles share.
pub fn shared(a: Box4, b: Box4) -> Option<Edge> {
    let (ax, ay, aw, ah) = a;
    let (bx, by, bw, bh) = b;
    if ax + aw == bx || bx + bw == ax {
        let (lo, hi) = (ay.max(by), (ay + ah).min(by + bh));
        if hi - lo > 0 {
            return Some(Edge { vertical: true, at: if ax + aw == bx { bx } else { ax }, lo, hi });
        }
    }
    if ay + ah == by || by + bh == ay {
        let (lo, hi) = (ax.max(bx), (ax + aw).min(bx + bw));
        if hi - lo > 0 {
            return Some(Edge { vertical: false, at: if ay + ah == by { by } else { ay }, lo, hi });
        }
    }
    None
}

pub fn by_room(rects: &[Rc]) -> BTreeMap<usize, Vec<Box4>> {
    let mut by: BTreeMap<usize, Vec<Box4>> = BTreeMap::new();
    for r in rects {
        by.entry(r.0).or_default().push(boxed(r));
    }
    by
}

/// The longest wall rooms `i` and `j` share (the first of equals).
pub fn wall(by: &BTreeMap<usize, Vec<Box4>>, i: usize, j: usize) -> Option<Edge> {
    let mut best: Option<Edge> = None;
    for a in by.get(&i).into_iter().flatten() {
        for b in by.get(&j).into_iter().flatten() {
            if let Some(e) = shared(*a, *b)
                && best.is_none_or(|x| e.len() > x.len())
            {
                best = Some(e);
            }
        }
    }
    best
}

fn sides(r: Box4) -> [Edge; 4] {
    let (x, y, w, h) = r;
    [
        Edge { vertical: false, at: y, lo: x, hi: x + w },
        Edge { vertical: false, at: y + h, lo: x, hi: x + w },
        Edge { vertical: true, at: x, lo: y, hi: y + h },
        Edge { vertical: true, at: x + w, lo: y, hi: y + h },
    ]
}

/// The parts of a rectangle's sides that no other rectangle touches.
pub fn outside(rects: &[Rc], rect: Box4) -> Vec<Edge> {
    let mut out = Vec::new();
    for side in sides(rect) {
        let mut segs = vec![(side.lo, side.hi)];
        for r in rects {
            if boxed(r) == rect {
                continue;
            }
            if let Some(e) = shared(rect, boxed(r))
                && e.vertical == side.vertical
                && e.at == side.at
            {
                segs = segs.iter().flat_map(|(a0, b0)| [(*a0, (*b0).min(e.lo)), ((*a0).max(e.hi), *b0)]).filter(|p| p.1 > p.0).collect();
            }
        }
        out.extend(segs.into_iter().map(|(lo, hi)| Edge { lo, hi, ..side }));
    }
    out
}

/// A room's area (m2) and its two clear sides (mm), on the wall faces, as BIMCraft's room tag will give them. The sides come twice:
/// between solid wall faces, and as the extracted boundary has them, reaching a cased gap's separation line.
pub fn measure(rects: &[Rc], linked: &Linked, i: usize) -> (f64, [(f64, f64); 2]) {
    let links = &linked.links;
    let by = by_room(rects);
    // The cased openings of the room: the gap's ends on the pair's longest wall, whose sides have no wall there.
    let cased: Vec<(usize, Edge, (i64, i64))> = links
        .iter()
        .filter(|(a, b, t)| *t == Conn::Cased && (*a == i || *b == i))
        .filter_map(|(a, b, _)| {
            let e = wall(&by, *a, *b)?;
            let (t1, t2) = linked.place.get(&(*a, *b)).copied().unwrap_or((0.0, 0.0));
            Some((if *a == i { *b } else { *a }, e, cased_gap(e.lo, e.len(), t1, t2)?))
        })
        .collect();
    let open_to: BTreeSet<usize> = links
        .iter()
        .filter(|(a, b, t)| *t == Conn::OpenFull && (*a == i || *b == i))
        .map(|(a, b, _)| if *a == i { *b } else { *a })
        .collect();
    let (mut area, mut dims) = (0.0, None::<[(f64, f64); 2]>);
    for rect in rects.iter().filter(|r| r.0 == i).map(boxed) {
        let free_pieces = outside(rects, rect);
        let (mut loss, mut solid, mut reach) = ([0.0; 4], [0.0; 4], [0.0; 4]);
        for (k, side) in sides(rect).iter().enumerate() {
            let free: i64 = free_pieces.iter().filter(|p| p.vertical == side.vertical && p.at == side.at).map(Edge::len).sum();
            let mut none = 0;
            for r in rects {
                if boxed(r) == rect || !(r.0 == i || open_to.contains(&r.0)) {
                    continue;
                }
                if let Some(e) = shared(rect, boxed(r))
                    && e.vertical == side.vertical
                    && e.at == side.at
                {
                    let len = side.hi.min(e.hi) - side.lo.max(e.lo);
                    if len > 0 {
                        none += len;
                    }
                }
            }
            let len = side.len() as f64;
            // The sides a room is said to have are between wall faces: a cased gap does not widen them.
            solid[k] = (free as f64 * TE as f64 / 2.0 + (side.len() - free - none) as f64 * TI as f64 / 2.0) / len;
            for (other, e, (g0, g1)) in &cased {
                if e.vertical == side.vertical && e.at == side.at {
                    for r in rects.iter().filter(|r| r.0 == *other) {
                        if shared(rect, boxed(r)) == Some(*e) {
                            let (lo, hi) = ((*g0).max(side.lo.max(e.lo)), (*g1).min(side.hi.min(e.hi)));
                            if hi > lo {
                                none += hi - lo;
                            }
                        }
                    }
                }
            }
            loss[k] = (free as f64 * TE as f64 / 2.0 + (side.len() - free - none) as f64 * TI as f64 / 2.0) / len;
            // The extracted boundary's box reaches the side's furthest point: an open or cased stretch, else an interior face.
            reach[k] = if none > 0 {
                0.0
            } else if side.len() > free {
                TI as f64 / 2.0
            } else {
                TE as f64 / 2.0
            };
        }
        area += (rect.2 as f64 - loss[2] - loss[3]) * (rect.3 as f64 - loss[0] - loss[1]) / 1e6;
        let (w, h) = (rect.2 as f64 - solid[2] - solid[3], rect.3 as f64 - solid[0] - solid[1]);
        if dims.is_none_or(|d| w * h > d[0].0 * d[0].1) {
            dims = Some([(w, h), (rect.2 as f64 - reach[2] - reach[3], rect.3 as f64 - reach[0] - reach[1])]);
        }
    }
    (area, dims.unwrap_or([(0.0, 0.0); 2]))
}

/// The closed loop round the rectangles, as the corner points. Fails if they make more than one loop or meet at a point.
pub fn outline(rects: &[Rc]) -> R<Vec<(i64, i64)>> {
    let xs: BTreeSet<i64> = rects.iter().flat_map(|r| [r.1, r.1 + r.3]).collect();
    let ys: BTreeSet<i64> = rects.iter().flat_map(|r| [r.2, r.2 + r.4]).collect();
    let (xs, ys): (Vec<i64>, Vec<i64>) = (xs.into_iter().collect(), ys.into_iter().collect());
    let at = |v: &[i64], x: i64| v.binary_search(&x).unwrap_or(0);
    let mut full: BTreeSet<(usize, usize)> = BTreeSet::new();
    for r in rects {
        for a in at(&xs, r.1)..at(&xs, r.1 + r.3) {
            for b in at(&ys, r.2)..at(&ys, r.2 + r.4) {
                full.insert((a, b));
            }
        }
    }
    let has = |a: i64, b: i64| a >= 0 && b >= 0 && full.contains(&(a as usize, b as usize));
    let mut next: BTreeMap<(i64, i64), (i64, i64)> = BTreeMap::new();
    let mut add = |p: (i64, i64), q: (i64, i64)| -> R<()> {
        if next.insert(p, q).is_some() { Err(Fail("outline meets at a point")) } else { Ok(()) }
    };
    for &(a, b) in &full {
        let (a, b) = (a as i64, b as i64);
        if !has(a, b - 1) {
            add((a, b), (a + 1, b))?;
        }
        if !has(a + 1, b) {
            add((a + 1, b), (a + 1, b + 1))?;
        }
        if !has(a, b + 1) {
            add((a + 1, b + 1), (a, b + 1))?;
        }
        if !has(a - 1, b) {
            add((a, b + 1), (a, b))?;
        }
    }
    let Some((&start, _)) = next.iter().next() else { return Err(Fail("no outline")) };
    let mut walk = vec![start];
    let mut p = next[&start];
    while p != start {
        walk.push(p);
        p = *next.get(&p).ok_or(Fail("outline is open"))?;
        if walk.len() > next.len() {
            return Err(Fail("outline is open"));
        }
    }
    if walk.len() != next.len() {
        return Err(Fail("outline is more than one loop"));
    }
    let n = walk.len();
    let mut corners = Vec::new();
    for i in 0..n {
        let (prev, here, after) = (walk[(i + n - 1) % n], walk[i], walk[(i + 1) % n]);
        if (here.0 == prev.0) != (after.0 == here.0) {
            corners.push((xs[here.0 as usize], ys[here.1 as usize]));
        }
    }
    Ok(corners)
}

/// What `connect` settles.
#[derive(Clone, Debug)]
pub struct Linked {
    pub links: Vec<(usize, usize, Conn)>,
    /// Rooms placed against their parent with no opening.
    pub closed: Vec<usize>,
    /// The entry room, and the stretch of outline the outside door goes on.
    pub entry: Option<(usize, Edge)>,
    /// Where each link's door or opening stands along its wall, as fractions from 0 to 1 (a door's place, a cased gap's length and
    /// place); drawn last, so the picks above them are as they were.
    pub place: BTreeMap<(usize, usize), (f64, f64)>,
    /// The same for the outside door.
    pub entry_at: f64,
}

/// The links, the extra links where rooms happen to share a wall, and the outside door.
pub fn connect(fam: &Family, rooms: &[Room], g: &Graph, rects: &[Rc], n_main: usize, rng: &mut Rnd) -> R<Linked> {
    let by = by_room(rects);
    let ty = |i: usize| rooms[i].ty;
    let length = |i: usize, j: usize| wall(&by, i, j).map_or(0, |e| e.len());
    let mut links: BTreeMap<(usize, usize), Conn> = BTreeMap::new();
    let mut closed = Vec::new();
    let join = |i: usize, j: usize, types: &[Conn], rng: &mut Rnd, links: &mut BTreeMap<(usize, usize), Conn>| -> R<()> {
        let ok: Vec<Conn> = types.iter().copied().filter(|t| length(i, j) >= t.need()).collect();
        links.insert((i.min(j), i.max(j)), rng.pick(&ok).map_err(|_| Fail("no wall for a link"))?);
        Ok(())
    };
    let types_of =
        |a: usize, b: usize| fam.links.iter().find(|l| l.use_ == Use::Parent && l.from == ty(a) && l.to == ty(b)).map(|l| l.types.clone());
    let sp = &g.spines;
    if let Some(root) = g.root {
        join(root, sp[0], &types_of(root, sp[0]).unwrap_or_default(), rng, &mut links)?;
    }
    for i in 1..sp.len() {
        let prev = if i < n_main {
            sp[i - 1]
        } else {
            let mut best = sp[0];
            for &j in &sp[..n_main.min(sp.len())] {
                if length(sp[i], j) > length(sp[i], best) {
                    best = j;
                }
            }
            best
        };
        join(prev, sp[i], &types_of(prev, sp[i]).unwrap_or_default(), rng, &mut links)?;
    }
    for (&i, &parent) in &g.parent {
        let p = match parent {
            Parent::Spine => {
                let mut best = sp[0];
                for &j in sp {
                    if length(i, j) > length(i, best) {
                        best = j;
                    }
                }
                best
            }
            Parent::Room(p) => p,
        };
        let types = g.ltypes.get(&i).cloned().unwrap_or_default();
        if !types.is_empty() {
            join(i, p, &types, rng, &mut links)?;
        } else if length(i, p) == 0 {
            return Err(Fail("closed room is not beside its parent"));
        } else {
            closed.push(i);
        }
    }
    let mut count: BTreeMap<usize, usize> = BTreeMap::new();
    for (a, b) in links.keys() {
        *count.entry(*a).or_default() += 1;
        *count.entry(*b).or_default() += 1;
    }
    let n = rooms.len();
    for l in fam.links.iter().filter(|l| l.use_ == Use::Extra) {
        for a in 0..n {
            for b in 0..n {
                if ty(a) != l.from || ty(b) != l.to || a == b || links.contains_key(&(a.min(b), a.max(b))) {
                    continue;
                }
                let ok: Vec<Conn> = l.types.iter().copied().filter(|t| length(a, b) >= t.need()).collect();
                if !ok.is_empty()
                    && count.get(&a).copied().unwrap_or(0) < fam.types[ty(a)].max_links
                    && count.get(&b).copied().unwrap_or(0) < fam.types[ty(b)].max_links
                    && rng.random() < l.chance
                {
                    links.insert((a.min(b), a.max(b)), rng.pick(&ok)?);
                    *count.entry(a).or_default() += 1;
                    *count.entry(b).or_default() += 1;
                }
            }
        }
    }
    if count.iter().any(|(i, c)| *c > fam.types[ty(*i)].max_links) {
        return Err(Fail("too many links"));
    }
    closed.sort_unstable();
    let mut entry = None;
    if let Some(root) = g.root {
        let mut best: Option<Edge> = None;
        for r in rects.iter().filter(|r| r.0 == root) {
            for e in outside(rects, boxed(r)) {
                if best.is_none_or(|b| (e.len(), e) > (b.len(), b)) {
                    best = Some(e);
                }
            }
        }
        match best {
            Some(e) if e.len() >= 1300 => entry = Some((root, e)),
            _ => return Err(Fail("no outside wall for the entry")),
        }
    }
    let place: BTreeMap<(usize, usize), (f64, f64)> = links.keys().map(|k| (*k, (rng.random(), rng.random()))).collect();
    let entry_at = rng.random();
    Ok(Linked { links: links.into_iter().map(|((a, b), t)| (a, b, t)).collect(), closed, entry, place, entry_at })
}

/// What a plan holds that the manifest reports.
#[derive(Clone, Debug)]
pub struct Verified {
    pub net_m2: f64,
    pub gross_m2: f64,
}

/// What every plan must hold: no overlap, one loop (with the catalogue's corner count when given), every room inside its type's
/// limits, every room that has a link reached.
pub fn verify(fam: &Family, rooms: &[Room], rects: &[Rc], linked: &Linked, corners: Option<usize>) -> R<Verified> {
    for (n, a) in rects.iter().enumerate() {
        for b in &rects[n + 1..] {
            if a.1 < b.1 + b.3 && b.1 < a.1 + a.3 && a.2 < b.2 + b.4 && b.2 < a.2 + a.4 {
                return Err(Fail("overlap"));
            }
        }
    }
    let lp = outline(rects)?;
    if corners.is_some_and(|c| c != lp.len()) {
        return Err(Fail("corners"));
    }
    let ids: BTreeSet<usize> = rects.iter().map(|r| r.0).collect();
    let mut net = 0.0;
    for &i in &ids {
        let spec = &fam.types[rooms[i].ty];
        let (area, sides) = measure(rects, linked, i);
        net += area;
        if area < spec.area.0 * 1.003 || area > spec.area.1 * 0.997 || sides.iter().any(|(w, h)| w.min(*h) < spec.min_side as f64) {
            return Err(Fail("a room is out of range"));
        }
        if spec.ratio.is_some_and(|r| sides.iter().any(|(w, h)| w.max(*h) / w.min(*h).max(1.0) > r)) {
            return Err(Fail("a room is too long"));
        }
    }
    let reach: Vec<usize> = ids.iter().copied().filter(|i| !linked.closed.contains(i)).collect();
    if let Some(&first) = reach.first() {
        let mut seen = BTreeSet::from([first]);
        let mut todo = vec![first];
        while let Some(i) = todo.pop() {
            for (a, b, _) in &linked.links {
                let j = if *a == i {
                    Some(*b)
                } else if *b == i {
                    Some(*a)
                } else {
                    None
                };
                if let Some(j) = j
                    && seen.insert(j)
                {
                    todo.push(j);
                }
            }
        }
        if seen.len() != reach.len() {
            return Err(Fail("not all reached"));
        }
    }
    let gross = rects.iter().map(|r| r.3 * r.4).sum::<i64>() as f64 / 1e6;
    Ok(Verified { net_m2: net, gross_m2: gross })
}
