//! A scene's triangles, each copy of a shape held once: the shape's points and what each of its triangles is (colour, skin,
//! surface, face), with where the copy stands. A triangle of a copy is worked out when it is asked for, by the very
//! [`ComponentInstance::place`] that found it a copy, so every triangle is to the bit the one the scene made; the others are
//! held as they are. Every triangle keeps its place in the scene, so what is numbered by it (planes, smooth normals, the
//! card's picking) is unchanged.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use super::{P3, Rgb, Tri};
use crate::component_geometry::ComponentInstance;

/// What a triangle of a shape is, but where it stands and whose it is: its copy's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Look {
    color: [u8; 3],
    tex: u32,
    /// The curved surface of its element it is a facet of, numbered in its element.
    curve: Option<usize>,
    face: Option<u32>,
    part: u8,
}

impl Look {
    /// What `t` is but its points and owner; none where it can't be told by its owner alone (a curve of another element's).
    fn of(t: &Tri) -> Option<Look> {
        let curve = match t.curve {
            Some((owner, k)) if owner == t.owner => Some(k),
            Some(_) => return None,
            None => None,
        };
        (t.vis == 0).then_some(Look { color: [t.color.0, t.color.1, t.color.2], tex: t.tex, curve, face: t.face, part: t.part })
    }
}

/// A copy of a shape among the triangles.
#[derive(Clone, Debug, PartialEq)]
struct Copied {
    /// Its first triangle's place among all of them.
    start: usize,
    /// How many triangles of copies come before it.
    before: usize,
    place: ComponentInstance,
    points: Arc<[[P3; 3]]>,
    looks: Arc<[Look]>,
}

impl Copied {
    fn len(&self) -> usize {
        self.points.len()
    }

    /// Its `j`th triangle; with no points (none of its own) unless `placed`.
    fn tri(&self, j: usize, placed: bool) -> Option<Tri> {
        let l = self.looks.get(j)?;
        let owner = self.place.owner;
        let p = if placed { self.points.get(j)?.map(|q| self.place.place(q)) } else { [[0.0; 3]; 3] };
        Some(Tri {
            p,
            owner,
            color: Rgb(l.color[0], l.color[1], l.color[2]),
            tex: l.tex,
            curve: l.curve.map(|k| (owner, k)),
            face: l.face,
            vis: 0,
            part: l.part,
        })
    }
}

/// A scene's triangles: those held as they are, in their order, and the copies among them.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Tris {
    own: Vec<Tri>,
    copies: Vec<Copied>,
    /// Where each triangle is, so one is found at once: below [`COPY`], which of `own`; from it, which of `copies` (less
    /// [`COPY`]). Four bytes a triangle, against the 128 a triangle of a copy no longer takes.
    slots: Vec<u32>,
}

/// The first slot that names a copy.
const COPY: u32 = 1 << 31;

/// How many triangles a thread takes at a time in [`Tris::par_map`].
const RUN: usize = 1 << 15;

impl From<Vec<Tri>> for Tris {
    fn from(own: Vec<Tri>) -> Tris {
        let mut out = Tris::default();
        out.extend(own);
        out
    }
}

impl Tris {
    /// `tris` with each of `placements` (where a copy's first triangle is, and the copy of which of `shapes`) held once,
    /// where its triangles are, to the bit, the shape placed and each is what its shape's others are: a copy that isn't
    /// stays as its triangles. `placements` are in the order of their first triangles, apart.
    pub(crate) fn compact(tris: Vec<Tri>, placements: &[(usize, ComponentInstance)], shapes: &[Vec<[P3; 3]>]) -> Tris {
        let mut points: HashMap<usize, Arc<[[P3; 3]]>> = HashMap::new();
        let mut tables: HashMap<Vec<Look>, Arc<[Look]>> = HashMap::new();
        let mut out = Tris::default();
        let mut at = 0;
        let mut before = 0;
        for (first, place) in placements {
            let Some(local) = shapes.get(place.shape).filter(|l| !l.is_empty()) else { continue };
            let end = first + local.len();
            let Some(made) = tris.get(*first..end).filter(|_| *first >= at) else { continue };
            let looks: Option<Vec<Look>> = made.iter().map(|t| Look::of(t).filter(|_| t.owner == place.owner)).collect();
            let Some(looks) = looks.filter(|_| local.iter().zip(made).all(|(l, t)| l.map(|q| place.place(q)) == t.p)) else {
                continue;
            };
            out.extend(tris.get(at..*first).unwrap_or_default().iter().copied());
            let looks = Arc::clone(tables.entry(looks).or_insert_with_key(|k| k.as_slice().into()));
            let points = Arc::clone(points.entry(place.shape).or_insert_with(|| local.as_slice().into()));
            out.push_copy(Copied { start: *first, before, place: *place, points, looks });
            before += local.len();
            at = end;
        }
        out.extend(tris.get(at..).unwrap_or_default().iter().copied());
        out.own.shrink_to_fit();
        out.slots.shrink_to_fit();
        out
    }

    /// `c` put after these triangles, where it starts.
    fn push_copy(&mut self, c: Copied) {
        let slot = COPY | u32::try_from(self.copies.len()).unwrap_or(COPY - 1);
        self.slots.extend(std::iter::repeat_n(slot, c.len()));
        self.copies.push(c);
    }

    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    /// How many triangles are held once, as copies of a shape.
    pub(crate) fn copied(&self) -> usize {
        self.copies.last().map_or(0, |c| c.before + c.len())
    }

    /// Where triangle `i` is: in the copy it is of and which of its triangles, or which of those held as they are.
    fn locate(&self, i: usize) -> Option<Result<(&Copied, usize), usize>> {
        let slot = *self.slots.get(i)?;
        Some(if slot >= COPY {
            let c = self.copies.get((slot - COPY) as usize)?;
            Ok((c, i - c.start))
        } else {
            Err(slot as usize)
        })
    }

    /// Triangle `i`, or the triangles of a range (borrowed where none is a copy's), as a slice's `get` gives.
    pub(crate) fn get<I: Index>(&self, i: I) -> Option<I::Out<'_>> {
        i.of(self)
    }

    /// Triangle `i`.
    fn one(&self, i: usize) -> Option<Tri> {
        match self.locate(i)? {
            Ok((c, j)) => c.tri(j, true),
            Err(j) => self.own.get(j).copied(),
        }
    }

    /// The triangles of `range`, borrowed where none of them is a copy's.
    pub(crate) fn range(&self, range: std::ops::Range<usize>) -> Option<Cow<'_, [Tri]>> {
        if range.start > range.end || range.end > self.len() {
            return None;
        }
        if range.is_empty() {
            return Some(Cow::Borrowed(&[]));
        }
        if let (Some(Err(a)), Some(Err(b))) = (self.locate(range.start), self.locate(range.end - 1))
            && b - a == range.end - 1 - range.start
        {
            return self.own.get(a..=b).map(Cow::Borrowed);
        }
        Some(Cow::Owned(self.iter_from(range.start).take(range.len()).collect()))
    }

    /// `f` of each triangle and its place, in order: on the render threads, a run of triangles each, for many.
    pub(crate) fn par_map<U: Send>(&self, f: impl Fn(usize, Tri) -> U + Sync) -> Vec<U> {
        self.par_map_of(true, f)
    }

    /// [`Tris::par_map`] of what each triangle is but its points, which are none of its own: what never asks where it is
    /// needn't wait for the points of every copy to be placed.
    pub(crate) fn par_map_unplaced<U: Send>(&self, f: impl Fn(usize, Tri) -> U + Sync) -> Vec<U> {
        self.par_map_of(false, f)
    }

    fn par_map_of<U: Send>(&self, placed: bool, f: impl Fn(usize, Tri) -> U + Sync) -> Vec<U> {
        match (self.len() >= 2 * RUN).then(super::render_pool).flatten() {
            Some(pool) => {
                use rayon::prelude::*;
                let runs: Vec<usize> = (0..self.len()).step_by(RUN).collect();
                let parts: Vec<Vec<U>> = pool.install(|| {
                    runs.par_iter().map(|&a| self.iter_of(a, placed).take(RUN).enumerate().map(|(k, t)| f(a + k, t)).collect()).collect()
                });
                let mut out = Vec::with_capacity(self.len());
                for part in parts {
                    out.extend(part);
                }
                out
            }
            None => self.iter_of(0, placed).enumerate().map(|(i, t)| f(i, t)).collect(),
        }
    }

    /// Whether `f` holds of any triangle, told by what each is but its points, which are none of its own.
    pub(crate) fn par_any_unplaced(&self, f: impl Fn(Tri) -> bool + Sync) -> bool {
        match (self.len() >= 2 * RUN).then(super::render_pool).flatten() {
            Some(pool) => {
                use rayon::prelude::*;
                let runs: Vec<usize> = (0..self.len()).step_by(RUN).collect();
                pool.install(|| runs.par_iter().any(|&a| self.iter_of(a, false).take(RUN).any(&f)))
            }
            None => self.iter_of(0, false).any(f),
        }
    }

    /// The `len` triangles from `first` with all but their points, where they are one copy held once: what they are (colour,
    /// skin, surface, face, part, owner) without placing them. Their points are none of theirs.
    pub(crate) fn unplaced(&self, first: usize, len: usize) -> Option<impl Iterator<Item = Tri> + '_> {
        match self.locate(first)? {
            Ok((c, 0)) if c.len() == len => Some((0..len).filter_map(move |j| c.tri(j, false))),
            _ => None,
        }
    }

    /// Every triangle, in order.
    pub(crate) fn iter(&self) -> Iter<'_> {
        self.iter_from(0)
    }

    /// The triangles from `i` on.
    fn iter_from(&self, i: usize) -> Iter<'_> {
        self.iter_of(i, true)
    }

    /// The triangles from `i` on; a copy's with no points unless `placed`.
    fn iter_of(&self, i: usize, placed: bool) -> Iter<'_> {
        let k = self.copies.partition_point(|c| c.start + c.len() <= i);
        let own = match self.locate(i) {
            Some(Err(j)) => j,
            Some(Ok((c, _))) => c.start - c.before,
            None => self.own.len(),
        };
        Iter { tris: self, i, own, k, placed }
    }

    /// The triangles of `range` of `from`, put after these: a copy among them stays one.
    pub(crate) fn extend_from(&mut self, from: &Tris, range: std::ops::Range<usize>) {
        if range.start >= range.end || range.end > from.len() {
            return;
        }
        let k = from.copies.partition_point(|c| c.start + c.len() <= range.start);
        let mut i = range.start;
        for c in from.copies.get(k..).unwrap_or_default().iter().take_while(|c| c.start < range.end) {
            if c.start < i || c.start + c.len() > range.end {
                // Cut by the range: its triangles as they are.
                let (a, b) = (i.max(c.start), (c.start + c.len()).min(range.end));
                self.extend_own_from(from, i..a);
                self.extend(from.iter_from(a).take(b - a));
                i = b;
                continue;
            }
            self.extend_own_from(from, i..c.start);
            let before = self.copied();
            self.push_copy(Copied { start: self.len(), before, ..c.clone() });
            i = c.start + c.len();
        }
        self.extend_own_from(from, i..range.end);
    }

    /// Triangles of `from` none of which is a copy's.
    fn extend_own_from(&mut self, from: &Tris, range: std::ops::Range<usize>) {
        if let Some(Cow::Borrowed(own)) = from.range(range.clone()) {
            self.extend(own.iter().copied());
        } else if let Some(made) = from.range(range) {
            self.extend(made.iter().copied());
        }
    }
}

/// What [`Tris::get`] takes: a triangle's place, or a range of them.
pub(crate) trait Index {
    type Out<'a>;
    fn of(self, tris: &Tris) -> Option<Self::Out<'_>>;
}

impl Index for usize {
    type Out<'a> = Tri;
    fn of(self, tris: &Tris) -> Option<Tri> {
        tris.one(self)
    }
}

impl Index for std::ops::Range<usize> {
    type Out<'a> = Cow<'a, [Tri]>;
    fn of(self, tris: &Tris) -> Option<Cow<'_, [Tri]>> {
        tris.range(self)
    }
}

impl Extend<Tri> for Tris {
    fn extend<I: IntoIterator<Item = Tri>>(&mut self, items: I) {
        let n = self.own.len();
        self.own.extend(items);
        let slots = (n..self.own.len()).map(|j| u32::try_from(j).unwrap_or(COPY - 1).min(COPY - 1));
        self.slots.extend(slots);
    }
}

/// [`Tris::iter`].
#[derive(Clone)]
pub(crate) struct Iter<'a> {
    tris: &'a Tris,
    /// The next triangle's place, the next held as it is, and the next copy not passed.
    i: usize,
    own: usize,
    k: usize,
    placed: bool,
}

impl Iterator for Iter<'_> {
    type Item = Tri;

    fn next(&mut self) -> Option<Tri> {
        if self.i >= self.tris.len() {
            return None;
        }
        let i = self.i;
        self.i += 1;
        if let Some(c) = self.tris.copies.get(self.k)
            && c.start <= i
        {
            let t = c.tri(i - c.start, self.placed);
            if self.i >= c.start + c.len() {
                self.k += 1;
            }
            return t;
        }
        let t = self.tris.own.get(self.own).copied();
        self.own += 1;
        t
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.tris.len().saturating_sub(self.i);
        (n, Some(n))
    }
}

impl ExactSizeIterator for Iter<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri(owner: u64, p: [P3; 3], color: u8) -> Tri {
        Tri { p, owner, color: Rgb(color, 0, 0), tex: 0, curve: None, face: Some(0), vis: 0, part: 0 }
    }

    #[test]
    fn a_copy_held_once_gives_back_every_triangle_as_it_was_made_and_one_that_differs_stays_as_it_is() {
        let shape = vec![[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]];
        let place = |owner: u64, x: f64| ComponentInstance {
            owner,
            shape: 0,
            at: [x, 0.3, 0.0],
            axes: [[0.7, 0.1, 0.0], [-0.1, 0.7, 0.0], [0.0, 0.0, 1.0]],
        };
        let mut tris = vec![tri(1, [[9.0; 3]; 3], 1)];
        let mut placements = Vec::new();
        for (owner, x) in [(2, 10.0), (3, 20.5), (4, 31.25)] {
            let c = place(owner, x);
            placements.push((tris.len(), c));
            tris.extend(shape.iter().enumerate().map(|(j, l)| tri(owner, l.map(|q| c.place(q)), 5 + j as u8)));
            tris.push(tri(100 + owner, [[x; 3]; 3], 2));
        }
        // The third's colour differs from its shape's others: it stays as made, and the triangles are the same all told.
        if let Some(t) = tris.get_mut(placements[2].0) {
            t.color = Rgb(77, 0, 0);
        }
        let made = Tris::compact(tris.clone(), &placements, &[shape]);
        assert_eq!(made.len(), tris.len());
        assert_eq!(made.copied(), 6, "every copy held once, the one that differs too: its looks are its own");
        assert_eq!(made.iter().collect::<Vec<_>>(), tris);
        for (i, t) in tris.iter().enumerate() {
            assert_eq!(made.get(i), Some(*t), "{i}");
            for j in i..=tris.len() {
                assert_eq!(made.range(i..j).as_deref(), tris.get(i..j), "{i}..{j}");
                assert_eq!(made.iter_from(i).take(j - i).collect::<Vec<_>>(), tris[i..j], "{i}..{j}");
                let mut spliced = Tris::default();
                spliced.extend_from(&made, 0..i);
                spliced.extend_from(&made, i..j);
                spliced.extend_from(&made, j..tris.len());
                assert_eq!(spliced.iter().collect::<Vec<_>>(), tris, "{i}..{j}");
            }
        }
        assert_eq!(made.get(tris.len()), None);
        let backwards = std::ops::Range { start: 3, end: 2 };
        assert!(made.range(backwards).is_none() && made.range(0..tris.len() + 1).is_none());
    }
}
