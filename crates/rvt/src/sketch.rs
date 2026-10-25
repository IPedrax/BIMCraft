// Portions of this file are ported from rvt-rs (https://github.com/DrunkOnJava/rvt-rs), Apache-2.0,
// Copyright 2026 Griffin Long, at commit b6086d41b9758fd9b92f42f0a47b8345b9c58056: src/element_record_plan_profiles.rs
// (a sketched element's boundary from its sketch lines, RE-25, RE-50, RE-96) and src/partition_beam_axes.rs (a sketch line's
// curve, RE-96).
// Changed: rewritten for BIMCraft; one tracer serves lines of known ends and lines known by their boxes, loops are
// bounded in number and size, and the pieces and voids are found by containment alone.
//! The boundary of a sketched element (a floor, a ceiling, a roof): its sketch lines, closed into loops.
//!
//! A floor is a sketch: boundary curves closing one outer loop and any number of inner loops. In a 2024 or later project
//! each curve is a record of its own in the category `OST_SketchLines`, with its box and the sketch's owner among the ids of its
//! second reference list; the curve itself, a line or an arc, is the first curve record in its data, and both readings come out of
//! it, so the box says which the curve is. Before 2024
//! only the box is read, and a segment parallel to an axis is its box: a diagonal leaves two candidate ends, which the loop
//! itself settles when exactly one open pair fits.
//!
//! Every end must be shared by exactly two curves, or the whole sketch is refused: nothing here guesses a vertex.

use crate::curves::{BoundedArc, BoundedLine, arc_values_at, line_at};

/// Vertex-coincidence tolerance, in feet.
const VERTEX_EPS: f64 = 1e-6;
/// How far apart two dangling ends may be and still be one vertex, in feet: a millimetre and a half, the rounding of a drawing
/// rather than a gap (the Snowdon sample has a floor whose ends miss by 0.0002 ft, and one of 0.0009).
const SNAP_EPS: f64 = 0.005;
/// Relative cross product below which three vertices are collinear.
const COLLINEAR_EPS: f64 = 1e-9;
/// Largest distance an arc's chords may stray from it, in feet.
const ARC_TOLERANCE: f64 = 0.0005;
/// Most curves a sketch is read from, and most points an arc is drawn with.
pub const MAX_EDGES: usize = 4096;
const MAX_ARC_POINTS: usize = 512;
/// How far past its box a curve's end may lie, in feet.
const BOX_EPS: f64 = 1e-4;

pub type Pt = (f64, f64);

/// A boundary curve: its two ends and the points between them, in order from `a`.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub a: Pt,
    pub b: Pt,
    pub mid: Vec<Pt>,
}

/// A region: an outer loop (counter-clockwise) and the voids in it (clockwise).
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub outer: Vec<Pt>,
    pub holes: Vec<Vec<Pt>>,
}

/// An arc drawn as points from its start to its end, each chord within [`ARC_TOLERANCE`] of the arc. None for an arc sweeping
/// no angle or more than a full turn.
fn arc_points(arc: &BoundedArc) -> Option<Vec<[f64; 3]>> {
    let sweep = arc.end_angle - arc.start_angle;
    if !(sweep.is_finite() && sweep > 0.0 && sweep <= std::f64::consts::TAU) {
        return None;
    }
    let step = 2.0 * (1.0 - ARC_TOLERANCE / arc.radius).clamp(-1.0, 1.0).acos();
    let count = ((sweep / step).ceil() as usize).clamp(1, MAX_ARC_POINTS);
    Some((0..=count).map(|i| arc.point(arc.start_angle + sweep * i as f64 / count as f64)).collect())
}

/// A sketch line's first curve record, read as a line and as an arc where it reads as either: the record does not say which it
/// is, so the box decides.
pub fn sketch_curve(data: &[u8]) -> Option<(Option<BoundedLine>, Option<BoundedArc>)> {
    const TAG: [u8; 4] = [0x04, 0x00, 0x08, 0x01];
    memchr::memmem::find_iter(data, &TAG).find_map(|at| {
        let (line, arc) = (line_at(data, at), arc_values_at(data, at));
        (line.is_some() || arc.is_some()).then_some((line, arc))
    })
}

fn in_box(p: [f64; 3], b: &[f64; 6]) -> bool {
    p[0] >= b[0] - BOX_EPS && p[0] <= b[3] + BOX_EPS && p[1] >= b[1] - BOX_EPS && p[1] <= b[4] + BOX_EPS
}

/// How far a curve's plan extent is from the box the file drew around it, on its worst side. A line record reads as an arc and an
/// arc record as a line, and it is the box that says which it is: an arc record's line reading often lands inside the arc's box,
/// so "the ends are inside the box" alone is no test.
fn misfit<'a>(points: impl Iterator<Item = Pt> + 'a, b: &[f64; 6]) -> f64 {
    let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    for p in points {
        lo = (lo.0.min(p.0), lo.1.min(p.1));
        hi = (hi.0.max(p.0), hi.1.max(p.1));
    }
    if lo.0 > hi.0 {
        return f64::MAX;
    }
    (lo.0 - b[0]).abs().max((lo.1 - b[1]).abs()).max((hi.0 - b[3]).abs()).max((hi.1 - b[4]).abs())
}

/// The curve of a sketch line whose box is `bbox`, from its data: the line reading or the arc reading, whichever fits the box,
/// and none where neither is inside it.
pub fn edge_of(data: &[u8], bbox: &[f64; 6]) -> Option<Edge> {
    let (line, arc) = sketch_curve(data)?;
    let straight = line.filter(|l| in_box(l.start(), bbox) && in_box(l.end(), bbox)).and_then(|l| {
        let (s, e) = (l.start(), l.end());
        ((s[0] - e[0]).hypot(s[1] - e[1]) > VERTEX_EPS).then(|| Edge { a: (s[0], s[1]), b: (e[0], e[1]), mid: Vec::new() })
    });
    let curved = arc.as_ref().and_then(arc_points).filter(|points| points.iter().all(|p| in_box(*p, bbox))).and_then(|points| {
        let (first, last) = (points.first()?, points.last()?);
        let mid = points[1..points.len() - 1].iter().map(|p| (p[0], p[1])).collect();
        Some(Edge { a: (first[0], first[1]), b: (last[0], last[1]), mid })
    });
    match (straight, curved) {
        (Some(s), Some(c)) => {
            let ends = |e: &Edge| -> Vec<Pt> { std::iter::once(e.a).chain(e.mid.iter().copied()).chain(std::iter::once(e.b)).collect() };
            Some(if misfit(ends(&c).into_iter(), bbox) < misfit(ends(&s).into_iter(), bbox) { c } else { s })
        }
        (straight, curved) => straight.or(curved),
    }
}

fn same(a: Pt, b: Pt) -> bool {
    (a.0 - b.0).abs() <= VERTEX_EPS && (a.1 - b.1).abs() <= VERTEX_EPS
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= VERTEX_EPS
}

/// Vertex registry with tolerance matching.
#[derive(Default)]
struct Vertices {
    points: Vec<Pt>,
    degree: Vec<usize>,
}

impl Vertices {
    fn intern(&mut self, p: Pt) -> usize {
        match self.points.iter().position(|held| same(*held, p)) {
            Some(i) => i,
            None => {
                self.points.push(p);
                self.degree.push(0);
                self.points.len() - 1
            }
        }
    }
}

/// Edges between vertices (a, b), and the points between their ends in order from a.
struct Graph {
    vertices: Vertices,
    edges: Vec<(usize, usize)>,
    mids: Vec<Vec<Pt>>,
    /// Which curve of the sketch each edge came from.
    source: Vec<usize>,
}

impl Graph {
    fn new() -> Self {
        Graph { vertices: Vertices::default(), edges: Vec::new(), mids: Vec::new(), source: Vec::new() }
    }

    /// Adds the edge; none when it joins a vertex to itself, repeats an edge, or gives a vertex a third edge.
    fn push(&mut self, a: Pt, b: Pt, mid: Vec<Pt>) -> Option<()> {
        if self.edges.len() >= MAX_EDGES {
            return None;
        }
        let (ia, ib) = (self.vertices.intern(a), self.vertices.intern(b));
        if ia == ib
            || self.edges.iter().enumerate().any(|(i, &(p, q))| ((p == ia && q == ib) || (p == ib && q == ia)) && self.mids[i] == mid)
        {
            return None;
        }
        self.vertices.degree[ia] += 1;
        self.vertices.degree[ib] += 1;
        if self.vertices.degree[ia] > 2 || self.vertices.degree[ib] > 2 {
            return None;
        }
        self.edges.push((ia, ib));
        self.mids.push(mid);
        Some(())
    }

    /// The loops the edges close when the pieces that do not close are left out, and which edges those were: every vertex of a
    /// kept piece has exactly two edges, and a piece with a dangling end is dropped whole.
    fn loops_of_closed_pieces(&self) -> (Vec<Vec<Pt>>, Vec<usize>) {
        // Which piece each vertex belongs to, by walking the edges.
        let mut piece = vec![usize::MAX; self.vertices.points.len()];
        let mut next = 0;
        for &(a, b) in &self.edges {
            match (piece[a], piece[b]) {
                (usize::MAX, usize::MAX) => {
                    piece[a] = next;
                    piece[b] = next;
                    next += 1;
                }
                (usize::MAX, p) => piece[a] = p,
                (p, usize::MAX) => piece[b] = p,
                (p, q) if p != q => {
                    for slot in piece.iter_mut().filter(|s| **s == q) {
                        *slot = p;
                    }
                }
                _ => {}
            }
        }
        let mut closed = vec![true; next];
        for (i, &d) in self.vertices.degree.iter().enumerate() {
            if d != 2 && piece[i] != usize::MAX {
                closed[piece[i]] = false;
            }
        }
        let mut kept = Vec::new();
        let mut dropped = Vec::new();
        for (i, &(a, _)) in self.edges.iter().enumerate() {
            if closed[piece[a]] {
                kept.push(i);
            } else {
                dropped.push(i);
            }
        }
        let loops = self.walk(&kept);
        (loops, dropped)
    }

    /// The loops the edges close, or none unless every vertex has exactly two.
    fn loops(&self) -> Option<Vec<Vec<Pt>>> {
        if self.vertices.degree.iter().any(|&d| d != 2) {
            return None;
        }
        Some(self.walk(&(0..self.edges.len()).collect::<Vec<_>>()))
    }

    /// The loops the edges at `kept` close, every one of their vertices having exactly two of them.
    fn walk(&self, kept: &[usize]) -> Vec<Vec<Pt>> {
        let mut at: Vec<Vec<usize>> = vec![Vec::new(); self.vertices.points.len()];
        for &i in kept {
            let (a, b) = self.edges[i];
            at[a].push(i);
            at[b].push(i);
        }
        let mut used = vec![false; self.edges.len()];
        let mut loops = Vec::new();
        for &first in kept {
            if used[first] {
                continue;
            }
            used[first] = true;
            let (start, mut current) = self.edges[first];
            let mut ring = vec![self.vertices.points[start]];
            ring.extend(self.mids[first].iter().copied());
            while current != start {
                ring.push(self.vertices.points[current]);
                // Every vertex of a kept piece has two edges, so one of them is always unvisited.
                let Some(next) = at[current].iter().copied().find(|&e| !used[e]) else { break };
                used[next] = true;
                let (a, b) = self.edges[next];
                if a == current {
                    ring.extend(self.mids[next].iter().copied());
                    current = b;
                } else {
                    ring.extend(self.mids[next].iter().rev().copied());
                    current = a;
                }
            }
            loops.push(ring);
        }
        loops
    }
}

/// The regions the curves close, or none unless they close with no choice left.
pub fn from_edges(edges: &[Edge]) -> Option<Vec<Piece>> {
    if edges.len() < 3 {
        return None;
    }
    let mut g = Graph::new();
    for e in edges {
        g.push(e.a, e.b, e.mid.clone())?;
    }
    pieces(g.loops()?)
}

/// The regions the curves close, leaving out the curves that close nothing (a sketch line floating inside a floor's outline, as
/// the structural sample has), the index of each of those, or none where no loop closes at all.
pub fn from_edges_open(edges: &[Edge]) -> Option<(Vec<Piece>, Vec<usize>)> {
    if edges.len() < 3 {
        return None;
    }
    let mut g = Graph::new();
    for (i, e) in edges.iter().enumerate() {
        g.push(e.a, e.b, e.mid.clone())?;
        g.source.push(i);
    }
    let (loops, dropped) = g.loops_of_closed_pieces();
    if loops.is_empty() {
        return None;
    }
    Some((pieces(loops)?, dropped.into_iter().map(|i| g.source[i]).collect()))
}

/// The regions the curves close with the ends that do not meet joined by a straight line, or none where more than
/// MAX_CLOSING ends are open (that is no sketch but damage), or where the lines they make still close nothing.
pub fn from_edges_closed(edges: &[Edge]) -> Option<Vec<Piece>> {
    if edges.len() < 3 {
        return None;
    }
    let mut ends: Vec<(Pt, usize)> = Vec::new();
    for e in edges {
        for p in [e.a, e.b] {
            match ends.iter_mut().find(|(q, _)| same(*q, p)) {
                Some((_, n)) => *n += 1,
                None => ends.push((p, 1)),
            }
        }
    }
    let dangling: Vec<Pt> = ends.iter().filter(|(_, n)| *n != 2).map(|(p, _)| *p).collect();
    if dangling.is_empty() || dangling.len() > MAX_CLOSING || !dangling.len().is_multiple_of(2) {
        return None;
    }
    // Nearest first, each end taken once: a sketch left open by a drawing's slip closes the short way.
    let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
    for (i, a) in dangling.iter().enumerate() {
        for (j, b) in dangling.iter().enumerate().skip(i + 1) {
            pairs.push(((a.0 - b.0).hypot(a.1 - b.1), i, j));
        }
    }
    pairs.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut taken = vec![false; dangling.len()];
    let mut all: Vec<Edge> = edges.to_vec();
    for (_, i, j) in pairs {
        if taken[i] || taken[j] {
            continue;
        }
        taken[i] = true;
        taken[j] = true;
        all.push(Edge { a: dangling[i], b: dangling[j], mid: Vec::new() });
    }
    from_edges(&all)
}

/// The most ends a sketch may leave open for from_edges_closed to join: a floor or a roof has a handful, a hostile file more.
const MAX_CLOSING: usize = 8;

/// The regions the curves close, the ends that miss one another by less than [`SNAP_EPS`] taken for the one vertex they are, or
/// none unless they close with no choice left.
pub fn from_edges_snapped(edges: &[Edge]) -> Option<Vec<Piece>> {
    if let Some(pieces) = from_edges(edges) {
        return Some(pieces);
    }
    // Every end, and how many curves meet at it; the ones no loop leaves alone are the ends a snap may join.
    let mut ends: Vec<(Pt, usize)> = Vec::new();
    for e in edges {
        for p in [e.a, e.b] {
            match ends.iter_mut().find(|(q, _)| same(*q, p)) {
                Some((_, n)) => *n += 1,
                None => ends.push((p, 1)),
            }
        }
    }
    let dangling: Vec<Pt> = ends.iter().filter(|(_, n)| *n != 2).map(|(p, _)| *p).collect();
    // A handful of ends may be a drawing's rounding; many are damage, and nothing is guessed then.
    if dangling.is_empty() || dangling.len() > 8 {
        return None;
    }
    // The pairs worth joining, nearest first, each end taken once.
    let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
    for (i, a) in dangling.iter().enumerate() {
        for (j, b) in dangling.iter().enumerate().skip(i + 1) {
            let d = (a.0 - b.0).hypot(a.1 - b.1);
            if d > VERTEX_EPS && d <= SNAP_EPS {
                pairs.push((d, i, j));
            }
        }
    }
    pairs.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut taken = vec![false; dangling.len()];
    let mut moves: Vec<(Pt, Pt)> = Vec::new();
    for (_, i, j) in pairs {
        if taken[i] || taken[j] {
            continue;
        }
        taken[i] = true;
        taken[j] = true;
        moves.push((dangling[i], dangling[j]));
    }
    if moves.is_empty() {
        return None;
    }
    let moved = |p: Pt| moves.iter().find(|(from, _)| same(*from, p)).map_or(p, |(_, to)| *to);
    let joined: Vec<Edge> = edges.iter().map(|e| Edge { a: moved(e.a), b: moved(e.b), mid: e.mid.clone() }).collect();
    from_edges(&joined)
}

/// The regions the curves close when only their plan boxes `[min x, min y, max x, max y]` are known.
pub fn from_boxes(boxes: &[[f64; 4]]) -> Option<Vec<Piece>> {
    if boxes.len() < 3 || boxes.len() > MAX_EDGES {
        return None;
    }
    let mut g = Graph::new();
    let mut pending: Vec<[f64; 4]> = Vec::new();
    for &[x0, y0, x1, y1] in boxes {
        if ![x0, y0, x1, y1].iter().all(|v| v.is_finite()) {
            return None;
        }
        match (near(x0, x1), near(y0, y1)) {
            (true, true) => return None,
            (true, false) => g.push((x0, y0), (x0, y1), Vec::new())?,
            (false, true) => g.push((x0, y0), (x1, y0), Vec::new())?,
            (false, false) => pending.push([x0, y0, x1, y1]),
        }
    }
    while !pending.is_empty() {
        let (index, (a, b)) = pending.iter().enumerate().find_map(|(i, span)| sole_candidate(&g, span).map(|c| (i, c)))?;
        pending.remove(index);
        g.push(a, b, Vec::new())?;
    }
    pieces(g.loops()?)
}

/// The one open vertex pair that fits `span`, none when there is no such pair or more than one. A pair across the box's
/// diagonal wins over a straight sub-line spanning it along one axis (a segment whose box is looser than the line).
fn sole_candidate(g: &Graph, span: &[f64; 4]) -> Option<(Pt, Pt)> {
    let [x0, y0, x1, y1] = *span;
    let open: Vec<usize> = (0..g.vertices.points.len())
        .filter(|&i| {
            let (x, y) = g.vertices.points[i];
            g.vertices.degree[i] < 2 && x >= x0 - VERTEX_EPS && x <= x1 + VERTEX_EPS && y >= y0 - VERTEX_EPS && y <= y1 + VERTEX_EPS
        })
        .collect();
    let (mut corner, mut spanning) = (Vec::new(), Vec::new());
    for (slot, &ia) in open.iter().enumerate() {
        for &ib in open.iter().skip(slot + 1) {
            if g.edges.iter().any(|&(p, q)| (p == ia && q == ib) || (p == ib && q == ia)) {
                continue;
            }
            let (a, b) = (g.vertices.points[ia], g.vertices.points[ib]);
            let fills_x = near(a.0.min(b.0), x0) && near(a.0.max(b.0), x1);
            let fills_y = near(a.1.min(b.1), y0) && near(a.1.max(b.1), y1);
            if fills_x && fills_y {
                corner.push((a, b));
            } else if (fills_x && near(a.1, b.1)) || (fills_y && near(a.0, b.0)) {
                spanning.push((a, b));
            }
        }
    }
    let tier = if corner.is_empty() { spanning } else { corner };
    (tier.len() == 1).then(|| tier[0])
}

fn signed_area(points: &[Pt]) -> f64 {
    let n = points.len();
    (0..n).map(|i| points[i].0 * points[(i + 1) % n].1 - points[(i + 1) % n].0 * points[i].1).sum::<f64>() / 2.0
}

/// Whether `p` is inside the closed loop (even-odd rule).
fn inside(polygon: &[Pt], p: Pt) -> bool {
    let mut within = false;
    let Some(mut previous) = polygon.last().copied() else { return false };
    for &current in polygon {
        let ((x0, y0), (x1, y1)) = (previous, current);
        if (y1 > p.1) != (y0 > p.1) && p.0 < x1 + (p.1 - y1) * (x0 - x1) / (y0 - y1) {
            within = !within;
        }
        previous = current;
    }
    within
}

/// Drops the vertices that lie on the line between their neighbours.
fn merge_collinear(points: &[Pt]) -> Vec<Pt> {
    let n = points.len();
    (0..n)
        .filter(|&i| {
            let (previous, current, next) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
            let (first, second) = ((current.0 - previous.0, current.1 - previous.1), (next.0 - current.0, next.1 - current.1));
            let cross = first.0 * second.1 - first.1 * second.0;
            let scale = first.0.hypot(first.1) * second.0.hypot(second.1);
            !(scale > 0.0 && (cross / scale).abs() < COLLINEAR_EPS)
        })
        .map(|i| points[i])
        .collect()
}

/// Orders loops into regions and voids: a loop's depth is how many larger loops contain it. An even depth starts a region, an
/// odd one is a void of the loop it sits in. A loop only partly inside another is not a region at all.
fn pieces(loops: Vec<Vec<Pt>>) -> Option<Vec<Piece>> {
    let mut merged: Vec<Vec<Pt>> = Vec::with_capacity(loops.len());
    for one in loops {
        let simple = merge_collinear(&one);
        if simple.len() < 3 {
            return None;
        }
        merged.push(simple);
    }
    merged.sort_by(|a, b| signed_area(b).abs().total_cmp(&signed_area(a).abs()));
    let (mut container, mut depth): (Vec<Option<usize>>, Vec<usize>) = (Vec::new(), Vec::new());
    for (index, one) in merged.iter().enumerate() {
        let mut parent = None;
        for (earlier, candidate) in merged[..index].iter().enumerate() {
            let within = one.iter().filter(|p| inside(candidate, **p)).count();
            if within == one.len() {
                parent = Some(earlier);
            } else if within > 0 {
                return None;
            }
        }
        depth.push(parent.map_or(0, |p| depth[p] + 1));
        container.push(parent);
    }
    let mut out: Vec<Piece> = Vec::new();
    let mut piece_of: Vec<Option<usize>> = vec![None; merged.len()];
    for (index, one) in merged.iter().enumerate() {
        let mut ring = one.clone();
        if depth[index] % 2 == 0 {
            if signed_area(&ring) < 0.0 {
                ring.reverse();
            }
            piece_of[index] = Some(out.len());
            out.push(Piece { outer: ring, holes: Vec::new() });
        } else {
            if signed_area(&ring) > 0.0 {
                ring.reverse();
            }
            let piece = container[index].and_then(|c| piece_of[c])?;
            out[piece].holes.push(ring);
        }
    }
    Some(out)
}
