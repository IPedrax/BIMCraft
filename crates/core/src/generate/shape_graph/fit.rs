//! Random numbers for an attempt, the grid, and the fitting of one cluster of rooms in one rectangle.

use std::cell::RefCell;
use std::collections::BTreeMap;

use super::super::Rng;
use super::family::Ty;

/// One attempt could not be finished; the caller starts the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fail(pub &'static str);

pub type R<T> = Result<T, Fail>;

pub const GRID: i64 = 50;
pub const TE: i64 = 250;
pub const TI: i64 = 130;
/// The most a cell loses across one direction in the usual case: half an exterior wall and half an interior one.
pub const LOSS_MAX: i64 = TE / 2 + TI / 2;
/// The least: an interior wall on both sides, half of each.
pub const LOSS_MIN: i64 = TI;

pub fn up(x: f64) -> i64 {
    ((x / GRID as f64 - 1e-9).ceil() as i64) * GRID
}

pub fn dn(x: f64) -> i64 {
    ((x / GRID as f64 + 1e-9).floor() as i64) * GRID
}

pub fn rnd(x: f64) -> i64 {
    ((x / GRID as f64).round() as i64) * GRID
}

/// The random numbers of attempt `k`: they come from the seed and `k` only.
pub struct Rnd(Rng);

impl Rnd {
    pub fn new(seed: u64, salt: u64) -> Rnd {
        Rnd(Rng::new(seed, 0x5A9E_0000_0000 ^ salt))
    }

    /// A number in [0, 1).
    pub fn random(&mut self) -> f64 {
        (self.0.step() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn uniform(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.random()
    }

    pub fn below(&mut self, n: usize) -> usize {
        self.0.below(n as u64) as usize
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> R<T> {
        if items.is_empty() {
            return Err(Fail("nothing to choose from"));
        }
        Ok(items[self.below(items.len())])
    }

    /// The index of one of the weights, chosen in proportion to it.
    pub fn weighted(&mut self, weights: &[f64]) -> R<usize> {
        let total: f64 = weights.iter().sum();
        if weights.is_empty() || total <= 0.0 {
            return Err(Fail("nothing to choose from"));
        }
        let mut at = self.random() * total;
        for (i, w) in weights.iter().enumerate() {
            if at < *w {
                return Ok(i);
            }
            at -= w;
        }
        Ok(weights.len() - 1)
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        self.0.shuffle(items);
    }
}

/// A room and the rooms inside its cell.
#[derive(Clone, Debug)]
pub struct Cl {
    pub head: usize,
    pub kids: Vec<Cl>,
    /// The shortest wall the head's link needs.
    pub need: i64,
    /// A room of the cluster (the head, or one inside its cell) that must touch the named wall of the cell: a `required` link
    /// with a room across that wall.
    pub goal: Option<(usize, Wall)>,
}

/// A wall of a cell, in its own frame, that a room may be asked to touch: the back (v = max), the left (u = 0) and the right. The
/// front is the head's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Wall {
    Back,
    Left,
    Right,
}

impl Cl {
    /// Whether `room` is the head or stands inside this cluster's cell.
    pub fn holds(&self, room: usize) -> bool {
        self.head == room || self.kids.iter().any(|k| k.holds(room))
    }
}

/// A cluster, and whether its head must reach its left and its right wall (a neighbour it is linked to).
pub type Member = (Cl, bool, bool);
pub type Group = Vec<Member>;

/// A rectangle in a cell's own frame: the room, the offset along the front and away from it, the length along the front and
/// the depth.
pub type Local = (usize, i64, i64, i64, i64);

pub type Range = (i64, i64);

/// A cluster's front range by its head, the depth and the walls it must reach.
type Cache = BTreeMap<(usize, i64, bool, bool, Option<Wall>), Option<Range>>;

pub struct Fitter<'a> {
    pub types: &'a [Ty],
    /// The room type of each room.
    pub room_ty: &'a [usize],
    /// Each room's wanted net area, m2.
    pub target: &'a [f64],
    cache: RefCell<Cache>,
}

impl<'a> Fitter<'a> {
    pub fn new(types: &'a [Ty], room_ty: &'a [usize], target: &'a [f64]) -> Fitter<'a> {
        Fitter { types, room_ty, target, cache: RefCell::new(BTreeMap::new()) }
    }

    fn spec(&self, room: usize) -> &Ty {
        &self.types[self.room_ty[room]]
    }

    /// A room whose cell has one side `x`: the lengths the cell's other side may have. Which walls are outside walls is not
    /// known yet, so this is cautious: the smallest area and the shortest side are held with the larger loss, the largest area
    /// with the smaller.
    pub fn other(spec: &Ty, x: i64) -> Option<Range> {
        let (small, big) = ((x - LOSS_MAX) as f64, (x - LOSS_MIN) as f64);
        if small < spec.min_side as f64 {
            return None;
        }
        let r = spec.ratio.unwrap_or(99.0);
        let lo = (spec.min_side as f64 + LOSS_MAX as f64).max(spec.area.0 * 1e6 / small + LOSS_MAX as f64).max(big / r + LOSS_MIN as f64);
        let hi = (spec.area.1 * 1e6 / big + LOSS_MIN as f64).min(small * r + LOSS_MAX as f64);
        let (lo, hi) = (up(lo), dn(hi));
        (lo <= hi).then_some((lo, hi))
    }

    fn gross(&self, cl: &Cl) -> f64 {
        self.target[cl.head] * 1.12e6 + cl.kids.iter().map(|k| self.gross(k)).sum::<f64>()
    }

    /// Rectangles for a cluster in a cell `f` long along the front and `d` deep, its front on v = 0. The head keeps the front; its
    /// children take a strip along the back or down one side. `lat`: the head must reach the left and the right wall. `cover`:
    /// an interval of the front the head must hold.
    pub fn fit(&self, cl: &Cl, f: i64, d: i64, lat: (bool, bool), cover: Option<Range>, pref: usize) -> Option<Vec<Local>> {
        let spec = self.spec(cl.head);
        if cl.kids.is_empty() {
            return Self::other(spec, f).filter(|iv| iv.0 <= d && d <= iv.1).map(|_| vec![(cl.head, 0, 0, f, d)]);
        }
        let all = [0usize, 1, 2];
        let g: Vec<f64> = cl.kids.iter().map(|k| self.gross(k)).collect();
        let sum_g: f64 = g.iter().sum();
        for step in 0..3 {
            let mode = all[(pref % 3 + step) % 3];
            if (mode == 1 && lat.1) || (mode == 2 && lat.0) {
                continue;
            }
            let (length, across) = if mode == 0 { (f, d) } else { (d, f) };
            // Which wall of a child's cell is the wanted wall of this one, if a room must touch it.
            let mut kid_goal: Vec<Option<(usize, Wall)>> = vec![None; cl.kids.len()];
            if let Some((room, wall)) = cl.goal {
                if room == cl.head {
                    let lost = match mode {
                        0 => Wall::Back,
                        1 => Wall::Right,
                        _ => Wall::Left,
                    };
                    if wall == lost {
                        continue;
                    }
                } else {
                    let Some(ki) = cl.kids.iter().position(|k| k.holds(room)) else { continue };
                    let first = ki == 0;
                    let last = ki + 1 == cl.kids.len();
                    let w = match (mode, wall) {
                        (0, Wall::Back) => Some(Wall::Back),
                        (0, Wall::Left) if first => Some(Wall::Left),
                        (0, Wall::Right) if last => Some(Wall::Right),
                        (1, Wall::Right) => Some(Wall::Back),
                        (2, Wall::Left) => Some(Wall::Back),
                        (1 | 2, Wall::Back) if last => Some(Wall::Right),
                        _ => None,
                    };
                    let Some(w) = w else { continue };
                    kid_goal[ki] = Some((room, w));
                }
            }
            let mut lens = Vec::new();
            let mut used = 0;
            for (i, gi) in g.iter().enumerate() {
                let li = if i + 1 < g.len() { rnd(length as f64 * gi / sum_g) } else { length - used };
                lens.push(li);
                used += li;
            }
            if lens.iter().zip(&cl.kids).any(|(li, k)| *li < k.need.max(600)) {
                continue;
            }
            let (mut lo, mut hi) = (600, across);
            for (k, li) in cl.kids.iter().zip(&lens) {
                if !k.kids.is_empty() {
                    continue;
                }
                match Self::other(self.spec(k.head), *li) {
                    Some(iv) => {
                        lo = lo.max(iv.0);
                        hi = hi.min(iv.1);
                    }
                    None => (lo, hi) = (1, 0),
                }
            }
            let Some(ivh) = Self::other(spec, length) else { continue };
            lo = lo.max(across - ivh.1);
            hi = hi.min(across - ivh.0);
            if lo > hi {
                continue;
            }
            let want = rnd(sum_g / length as f64).max(lo).min(hi);
            let mut tried: Vec<i64> = Vec::new();
            for t in [want, rnd((lo + hi) as f64 / 2.0), lo, hi] {
                if tried.contains(&t) {
                    continue;
                }
                tried.push(t);
                if let Some(c) = cover
                    && ((mode == 1 && c.1 > f - t) || (mode == 2 && c.0 < t))
                {
                    continue;
                }
                let mut out = Vec::new();
                let mut pos = 0;
                let mut ok = true;
                for ((k, li), goal) in cl.kids.iter().zip(&lens).zip(&kid_goal) {
                    let kid = Cl { goal: *goal, ..k.clone() };
                    let Some(sub) = self.fit(&kid, *li, t, (false, false), None, 0) else {
                        ok = false;
                        break;
                    };
                    for (room, u, v, du, dv) in sub {
                        out.push(match mode {
                            0 => (room, pos + u, d - t + v, du, dv),
                            1 => (room, f - t + v, pos + u, dv, du),
                            _ => (room, t - v - dv, pos + u, dv, du),
                        });
                    }
                    pos += li;
                }
                if ok {
                    let head = match mode {
                        0 => (cl.head, 0, 0, f, d - t),
                        1 => (cl.head, 0, 0, f - t, d),
                        _ => (cl.head, t, 0, f - t, d),
                    };
                    let mut all = vec![head];
                    all.extend(out);
                    return Some(all);
                }
            }
        }
        None
    }

    /// The fronts a cluster can have in a cell `d` deep: the shortest and the longest.
    pub fn frange(&self, cl: &Cl, d: i64, lat: (bool, bool)) -> Option<Range> {
        let key = (cl.head, d, lat.0, lat.1, cl.goal.map(|g| g.1));
        if let Some(hit) = self.cache.borrow().get(&key) {
            return *hit;
        }
        let found = if cl.kids.is_empty() {
            Self::other(self.spec(cl.head), d)
        } else {
            // A cluster that must reach a wall may fill a whole side of the plan: its fronts are scanned further.
            let most = if cl.goal.is_some() { 40_000 } else { 14_000 };
            let fits = |f: &i64| self.fit(cl, *f, d, lat, None, 0).is_some();
            let first = (900..=most).step_by(100).find(fits);
            first.map(|a| (a, (a..=most).rev().step_by(100).find(fits).unwrap_or(a)))
        };
        self.cache.borrow_mut().insert(key, found);
        found
    }

    pub fn grange(&self, group: &Group, d: i64) -> Option<Range> {
        let (mut lo, mut hi) = (0, 0);
        for (cl, l, r) in group {
            let iv = self.frange(cl, d, (*l, *r))?;
            lo += iv.0;
            hi += iv.1;
        }
        Some((lo, hi))
    }

    /// The depths a cap can have when its front is `f` long.
    pub fn drange(&self, cl: &Cl, f: i64, cover: Range) -> Option<Range> {
        let mut ok = (1500..=16_000).step_by(100).filter(|d| self.fit(cl, f, *d, (false, false), Some(cover), 0).is_some());
        let first = ok.next();
        first.map(|a| (a, ok.last().unwrap_or(a)))
    }
}

/// Lengths inside their ranges that add up to `total`.
pub fn share(total: i64, ranges: &[Range]) -> Option<Vec<i64>> {
    let mut lens: Vec<i64> = ranges.iter().map(|r| r.0).collect();
    let mut rem = total - lens.iter().sum::<i64>();
    let mut slack: Vec<i64> = ranges.iter().map(|r| r.1 - r.0).collect();
    let all: i64 = slack.iter().sum();
    if rem < 0 || rem > all {
        return None;
    }
    if rem > 0 {
        let f = rem as f64 / all as f64;
        for (i, s) in slack.iter_mut().enumerate() {
            let add = dn(*s as f64 * f);
            lens[i] += add;
            *s -= add;
            rem -= add;
        }
        for (i, s) in slack.iter().enumerate() {
            let add = (*s).min(rem);
            lens[i] += add;
            rem -= add;
        }
    }
    (rem == 0).then_some(lens)
}
