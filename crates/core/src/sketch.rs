//! Sketches: Revit's sketch mode, the one way an outline is drawn for a floor, a ceiling, a roof's footprint, a shaft, and
//! what's built on them later (filled regions, topography).
//!
//! A sketch is lines and arcs ([`SketchLine`]), drawn in any order and either way round; the element they draw keeps them,
//! each line a part with an id and a GUID of its own. [`solve`] joins them, end to end within [`TOL`], into closed loops and
//! nests the loops: a loop inside no other is an outline, one inside an outline a hole in it, one inside a hole an island,
//! an outline again. What can't be solved it names, every problem at once and the lines it's about, so the lines can be
//! shown to whoever drew them: a line too short or out of range, two lines crossing or one ending on another, two lines
//! along each other, an end no other line meets, three or more lines meeting at a point.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::curves::Curve;
use crate::geom::{self, P2, Region};
use crate::ids::{Guid, Id};
use crate::model::{MAX_COORD, MAX_POINTS, TOL};

/// A straight line, from one end to the other.
pub type Line = [P2; 2];

/// A sketch line: any [`Curve`], a line, an arc, a polyline, a circle, an ellipse or a spline. Its id and GUID are its
/// own, given when what it draws is made or edited and kept while it stays in the sketch. In a file and a command it is
/// one flat object: its `id` and `guid` and its curve's fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawSketchLine", into = "RawSketchLine")]
pub struct SketchLine {
    pub id: Id,
    pub guid: Guid,
    pub curve: Curve,
}

crate::curve_struct! {
    /// A sketch line as a file and a command hold it.
    RawSketchLine {
        #[serde(default, skip_serializing_if = "crate::ids::unset")]
        id: Id,
        #[serde(default, skip_serializing_if = "Guid::is_nil")]
        guid: Guid,
    }
}

impl TryFrom<RawSketchLine> for SketchLine {
    type Error = String;
    fn try_from(r: RawSketchLine) -> Result<SketchLine, String> {
        Ok(SketchLine { id: r.id, guid: r.guid, curve: r.fields().curve()? })
    }
}

impl From<SketchLine> for RawSketchLine {
    fn from(l: SketchLine) -> RawSketchLine {
        let mut r = RawSketchLine { id: l.id, guid: l.guid, ..RawSketchLine::default() };
        r.set(&l.curve);
        r
    }
}

impl From<Line> for SketchLine {
    fn from([start, end]: Line) -> SketchLine {
        SketchLine::new(start, end)
    }
}

impl From<Curve> for SketchLine {
    fn from(curve: Curve) -> SketchLine {
        SketchLine { id: 0, guid: Guid::default(), curve }
    }
}

impl SketchLine {
    /// A straight line, with no id yet.
    pub fn new(start: P2, end: P2) -> SketchLine {
        Curve::line(start, end).into()
    }

    /// An arc from `start` to `end` through `through`, with no id yet; straight when `through` is no further than
    /// [`TOL`] off the line between them.
    pub fn arc(start: P2, end: P2, through: P2) -> SketchLine {
        Curve::arc_through(start, end, through).into()
    }

    /// Where it starts.
    pub fn start(&self) -> P2 {
        self.curve.start()
    }

    /// Where it ends: where it starts, when it's closed.
    pub fn end(&self) -> P2 {
        self.curve.end()
    }

    /// Its points from start to end, as drawn: see [`Curve::points`].
    pub fn points(&self) -> Vec<P2> {
        self.curve.points()
    }

    /// Its middle: half way along it.
    pub fn middle(&self) -> P2 {
        self.curve.middle()
    }

    /// How far `p` is from it.
    pub fn dist(&self, p: P2) -> f64 {
        self.curve.dist(p)
    }

    /// Each point it holds, moved by `f`.
    pub fn map(&self, f: impl Fn(P2) -> P2) -> SketchLine {
        SketchLine { curve: self.curve.map(f), ..self.clone() }
    }

    fn in_range(&self) -> bool {
        self.curve.in_range()
    }
}

/// The most lines a sketch may hold.
pub const MAX_LINES: usize = MAX_POINTS;

/// The most problems [`solve`] names: past that, the first ones are enough to start fixing.
pub const MAX_PROBLEMS: usize = 100;

/// Why lines don't make loops, and the lines it's about, numbered from 0 in the order given.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "problem", rename_all = "snake_case")]
pub enum Problem {
    /// No lines at all.
    Empty,
    /// More lines than [`MAX_LINES`].
    TooMany { count: usize },
    /// An end that isn't a number, or is further out than [`MAX_COORD`].
    OutOfRange { line: usize },
    /// A line [`TOL`] long or less.
    TooShort { line: usize },
    /// A line that isn't one: points one on another, a spline's knots out of order, an arc turning a whole circle.
    Malformed { line: usize, why: String },
    /// Two lines crossing, or one ending part way along the other, at `at`.
    Cross { lines: [usize; 2], at: P2 },
    /// Two lines lying along each other for more than [`TOL`].
    Overlap { lines: [usize; 2] },
    /// An end, at `at`, that no other line meets: the loop is open there.
    Open { line: usize, at: P2 },
    /// Three or more lines meeting at `at`: a loop goes through a point once.
    Branch { lines: Vec<usize>, at: P2 },
    /// A loop enclosing no area.
    NoArea { lines: Vec<usize> },
    /// More than one outline, where one is wanted: `count` of them.
    Separate { count: usize },
}

impl Problem {
    /// The lines it's about, to show.
    pub fn lines(&self) -> Vec<usize> {
        match self {
            Problem::Empty | Problem::TooMany { .. } | Problem::Separate { .. } => Vec::new(),
            Problem::OutOfRange { line } | Problem::TooShort { line } | Problem::Malformed { line, .. } | Problem::Open { line, .. } => {
                vec![*line]
            }
            Problem::Cross { lines, .. } | Problem::Overlap { lines } => lines.to_vec(),
            Problem::Branch { lines, .. } | Problem::NoArea { lines } => lines.clone(),
        }
    }

    /// Where it is, when it's at a point.
    pub fn at(&self) -> Option<P2> {
        match self {
            Problem::Cross { at, .. } | Problem::Open { at, .. } | Problem::Branch { at, .. } => Some(*at),
            _ => None,
        }
    }

    /// Which to fix first: what's wrong with a line itself, then lines that cross or lie along each other (which often
    /// leave ends open too), then where loops branch or stay open.
    pub fn rank(&self) -> u8 {
        match self {
            Problem::Empty
            | Problem::TooMany { .. }
            | Problem::OutOfRange { .. }
            | Problem::TooShort { .. }
            | Problem::Malformed { .. } => 0,
            Problem::Cross { .. } => 1,
            Problem::Overlap { .. } => 2,
            Problem::Branch { .. } => 3,
            Problem::Open { .. } => 4,
            Problem::NoArea { .. } => 5,
            Problem::Separate { .. } => 6,
        }
    }

    /// It said in a sentence, each line as `name` names it (`line 999812`, `edge 2`).
    pub fn said(&self, name: &dyn Fn(usize) -> String) -> String {
        let at = |p: &P2| format!("[{}, {}]", p.0.round(), p.1.round());
        let list = |ls: &[usize]| {
            let names: Vec<String> = ls.iter().map(|&i| name(i)).collect();
            match names.split_last() {
                Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
                _ => names.join(""),
            }
        };
        match self {
            Problem::Empty => "there are no lines: draw the outline".into(),
            Problem::TooMany { count } => format!("{count} lines, more than {MAX_LINES}"),
            Problem::OutOfRange { line } => format!("{} has a point that isn't a number within {MAX_COORD} mm", name(*line)),
            Problem::TooShort { line } => format!("{} is {TOL} mm long or less", name(*line)),
            Problem::Malformed { line, why } => format!("{} isn't a line it can draw: {why}", name(*line)),
            Problem::Cross { lines, at: p } => format!("{} intersect at {}", list(lines), at(p)),
            Problem::Overlap { lines } => format!("{} overlap", list(lines)),
            Problem::Open { line, at: p } => format!("{} ends at {} without meeting another: the loop is open", name(*line), at(p)),
            Problem::Branch { lines, at: p } => format!("{} meet at {}: a loop passes a point once", list(lines), at(p)),
            Problem::NoArea { lines } => format!("the loop of {} encloses no area", list(lines)),
            Problem::Separate { count } => format!("the lines make {count} separate outlines, where one is wanted"),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.said(&|i| format!("line {i}")))
    }
}

/// Problems said in one line, the first few named, each line as `name` names it.
pub fn say(problems: &[Problem], name: &dyn Fn(usize) -> String) -> String {
    let shown: Vec<String> = problems.iter().take(5).map(|p| p.said(name)).collect();
    match problems.len() {
        n if n > shown.len() => format!("{}; and {} more", shown.join("; "), n - shown.len()),
        _ => shown.join("; "),
    }
}

/// How a sketch's lines are named to whoever drew them: by id, or by place in the list (from 0) when one has none yet.
pub fn namer(lines: &[SketchLine]) -> impl Fn(usize) -> String + '_ {
    |i| match lines.get(i) {
        Some(l) if !crate::ids::unset(&l.id) => format!("line {}", l.id),
        _ => format!("line {i}"),
    }
}

/// What a sketch's lines enclose: each outline (counter-clockwise) with its holes (clockwise), in the order their first
/// lines were drawn. An island in a hole is an outline of its own.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub regions: Vec<Region>,
}

impl Profile {
    /// Its area: the outlines' less their holes'.
    pub fn area(&self) -> f64 {
        self.regions.iter().map(Region::area).sum()
    }
}

/// The lines solved into loops, nested into regions; or every problem found (up to [`MAX_PROBLEMS`]), what to fix first
/// first ([`Problem::rank`]).
pub fn solve(lines: &[SketchLine]) -> Result<Profile, Vec<Problem>> {
    if lines.is_empty() {
        return Err(vec![Problem::Empty]);
    }
    if lines.len() > MAX_LINES {
        return Err(vec![Problem::TooMany { count: lines.len() }]);
    }
    // Each line on its own first: its points are numbers, its ends apart.
    let mut problems: Vec<Problem> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if !l.in_range() {
            problems.push(Problem::OutOfRange { line: i });
        } else if let Err(why) = l.curve.sound("it") {
            problems.push(Problem::Malformed { line: i, why: why.trim_start_matches("it: ").to_string() });
        } else if l.curve.length() <= TOL {
            problems.push(Problem::TooShort { line: i });
        }
    }
    if !problems.is_empty() {
        return Err(cap(problems));
    }
    // Lines as straight pieces (an arc as its chords), each knowing its line.
    let mut pieces: Vec<(Line, usize)> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        pieces.extend(l.points().windows(2).map(|w| ([w[0], w[1]], i)));
    }
    // Ends within TOL of each other are one point.
    let mut points: Vec<P2> = Vec::new();
    let ends: Vec<[usize; 2]> = pieces.iter().map(|(l, _)| l.map(|p| weld(&mut points, p))).collect();
    // Each pair of pieces of two lines that could touch: sorted by where they start across, a piece is checked against
    // those starting before it ends.
    let lo = |l: &Line| l[0].0.min(l[1].0);
    let hi = |l: &Line| l[0].0.max(l[1].0);
    let mut order: Vec<usize> = (0..pieces.len()).collect();
    order.sort_by(|&i, &j| lo(&pieces[i].0).total_cmp(&lo(&pieces[j].0)).then(i.cmp(&j)));
    let mut seen = std::collections::BTreeSet::new();
    for (k, &i) in order.iter().enumerate() {
        for &j in &order[k + 1..] {
            if lo(&pieces[j].0) > hi(&pieces[i].0) + TOL {
                break;
            }
            let (li, lj) = (pieces[i].1, pieces[j].1);
            if li == lj {
                continue;
            }
            let pair = [li.min(lj), li.max(lj)];
            let (p, q) = if li < lj { (i, j) } else { (j, i) };
            if let Some(found) = meet(pieces[p].0, pieces[q].0, ends[p], ends[q], pair)
                && seen.insert((pair, found.rank()))
            {
                problems.push(found);
                if problems.len() > MAX_PROBLEMS {
                    return Err(cap(problems));
                }
            }
        }
    }
    // Two pieces at every point: one, it's open; three or more, it branches.
    let mut at: Vec<Vec<usize>> = vec![Vec::new(); points.len()];
    for (i, e) in ends.iter().enumerate() {
        for &n in e {
            at[n].push(i);
        }
    }
    for (n, ps) in at.iter().enumerate() {
        match ps.as_slice() {
            [_, _] => {}
            &[piece] => problems.push(Problem::Open { line: pieces[piece].1, at: points[n] }),
            _ => {
                let mut ls: Vec<usize> = ps.iter().map(|&p| pieces[p].1).collect();
                ls.sort_unstable();
                problems.push(Problem::Branch { lines: ls, at: points[n] });
            }
        }
    }
    if !problems.is_empty() {
        problems.sort_by_key(|p| (p.rank(), p.lines().first().copied().unwrap_or(0)));
        return Err(cap(problems));
    }
    // Every point has two pieces: each part of the sketch is one loop, walked from its first piece the way it was drawn.
    let mut used = vec![false; pieces.len()];
    let mut loops: Vec<(Vec<P2>, Vec<usize>)> = Vec::new();
    for first in 0..pieces.len() {
        if used[first] {
            continue;
        }
        let (mut ring, mut members) = (Vec::new(), Vec::new());
        let (mut piece, mut from) = (first, ends[first][0]);
        while !used[piece] {
            used[piece] = true;
            if members.last() != Some(&pieces[piece].1) && !members.contains(&pieces[piece].1) {
                members.push(pieces[piece].1);
            }
            ring.push(points[from]);
            let to = if ends[piece][0] == from { ends[piece][1] } else { ends[piece][0] };
            from = to;
            piece = at[to].iter().copied().find(|&p| p != piece).unwrap_or(piece);
        }
        if geom::area(&ring).abs() <= TOL * TOL {
            problems.push(Problem::NoArea { lines: members.clone() });
        }
        loops.push((ring, members));
    }
    if !problems.is_empty() {
        return Err(cap(problems));
    }
    // No two loops cross or touch, so a loop is inside another when any of its points is: how many it's inside says
    // whether it's an outline (none, or an even number) or a hole (odd), in the innermost loop around it.
    let inside: Vec<Vec<usize>> =
        (0..loops.len()).map(|i| (0..loops.len()).filter(|&j| j != i && geom::contains(&loops[j].0, loops[i].0[0])).collect()).collect();
    let mut regions: Vec<Region> = Vec::new();
    let mut region_of = vec![usize::MAX; loops.len()];
    for (i, (ring, _)) in loops.iter().enumerate() {
        if inside[i].len().is_multiple_of(2) {
            region_of[i] = regions.len();
            regions.push(Region { outer: wound(ring, true), holes: Vec::new() });
        }
    }
    for (i, (ring, _)) in loops.iter().enumerate() {
        if !inside[i].len().is_multiple_of(2)
            && let Some(&o) = inside[i].iter().find(|&&j| inside[j].len() + 1 == inside[i].len())
            && let Some(r) = regions.get_mut(region_of[o])
        {
            r.holes.push(wound(ring, false));
        }
    }
    Ok(Profile { regions })
}

/// The lines solved into the one outline (with its holes) a floor, a ceiling or a roof takes.
pub fn solve_one(lines: &[SketchLine]) -> Result<Region, Vec<Problem>> {
    let mut p = solve(lines)?;
    match p.regions.len() {
        1 => Ok(p.regions.remove(0)),
        count => Err(vec![Problem::Separate { count }]),
    }
}

/// An outline and its holes as sketch lines, to edit: each ring's edges in order, straight, with no ids yet.
pub fn lines_of(outline: &[P2], holes: &[Vec<P2>]) -> Vec<SketchLine> {
    std::iter::once(outline)
        .chain(holes.iter().map(Vec::as_slice))
        .flat_map(|r| geom::ring_edges(r).map(|(a, b)| SketchLine::new(a, b)))
        .collect()
}

/// Whether `lines` solve to `outline` and `holes`, each ring the same points round in the same order from any start, either
/// way round: a sketch kept with an element is still what it draws.
pub fn draws(lines: &[SketchLine], outline: &[P2], holes: &[Vec<P2>]) -> bool {
    let same = |a: &[P2], b: &[P2]| {
        let n = a.len();
        let close = |p: P2, q: P2| p.dist(q) <= TOL;
        n == b.len()
            && n > 0
            && (0..n).any(|k| (0..n).all(|i| close(a[i], b[(i + k) % n])) || (0..n).all(|i| close(a[i], b[(k + n - i) % n])))
    };
    match solve_one(lines) {
        Ok(r) => same(&r.outer, outline) && r.holes.len() == holes.len() && r.holes.iter().all(|h| holes.iter().any(|g| same(h, g))),
        Err(_) => false,
    }
}

/// A kept sketch's lines checked as [`crate::model::Model::check`] checks what a file holds: no more than [`MAX_LINES`],
/// every point a number within range.
pub fn check(who: &str, lines: &[SketchLine]) -> Result<(), String> {
    if lines.len() > MAX_LINES {
        return Err(format!("{who}: its sketch has {} lines, more than {MAX_LINES}", lines.len()));
    }
    if let Some(i) = lines.iter().position(|l| l.curve.count() > MAX_POINTS) {
        return Err(format!("{who}: sketch line {i} has more than {MAX_POINTS} points"));
    }
    match lines.iter().position(|l| !l.in_range()) {
        Some(i) => Err(format!("{who}: sketch line {i} has a point that isn't a number within {MAX_COORD} mm")),
        None => Ok(()),
    }
}

/// What of `lines` a point picked near `p` (within `reach`) sticks to, nearest first: their ends, their middles, and where
/// two of them cross; the sketch's own, which the plan doesn't draw.
pub fn snaps(lines: &[SketchLine], p: P2, reach: f64) -> Vec<(P2, crate::snap::Kind)> {
    use crate::snap::Kind;
    let near: Vec<&SketchLine> = lines.iter().filter(|l| l.dist(p) <= reach).collect();
    let mut out: Vec<(P2, Kind)> = Vec::new();
    for l in lines {
        out.extend([(l.start(), Kind::Endpoint), (l.end(), Kind::Endpoint), (l.middle(), Kind::Midpoint)]);
        out.extend(crate::snap::rounds(&l.curve));
    }
    for (i, a) in near.iter().enumerate() {
        for b in &near[i + 1..] {
            let (pa, pb) = (a.points(), b.points());
            for x in pa.windows(2) {
                for y in pb.windows(2) {
                    if let Some(t) = geom::crossing(x[0], x[1], y[0], y[1]) {
                        out.push((x[0] + (x[1] - x[0]) * t, Kind::Intersection));
                    }
                }
            }
        }
    }
    out.retain(|(q, _)| q.dist(p) <= reach);
    out.sort_by(|a, b| a.0.dist(p).total_cmp(&b.0.dist(p)));
    out
}

/// The point among `points` within [`TOL`] of `p`, or `p` as a new one.
fn weld(points: &mut Vec<P2>, p: P2) -> usize {
    match points.iter().position(|q| q.dist(p) <= TOL) {
        Some(n) => n,
        None => {
            points.push(p);
            points.len() - 1
        }
    }
}

/// How two straight pieces (of lines `pair`, given in order) go wrong where they meet, if they do: along each other,
/// crossing, or one ending part way along the other. Meeting end to end is right.
fn meet([a, b]: Line, [c, d]: Line, e: [usize; 2], f: [usize; 2], pair: [usize; 2]) -> Option<Problem> {
    if (e[0] == f[0] && e[1] == f[1]) || (e[0] == f[1] && e[1] == f[0]) {
        return Some(Problem::Overlap { lines: pair });
    }
    // Along the same line: overlapping when what they share of it is longer than TOL.
    let off = |p: P2, s: P2, t: P2| (t - s).unit().cross(p - s).abs();
    if off(c, a, b) <= TOL && off(d, a, b) <= TOL {
        let u = (b - a).unit();
        let (s, t) = ((c - a).dot(u), (d - a).dot(u));
        let shared = s.max(t).min(a.dist(b)) - s.min(t).max(0.0);
        return (shared > TOL).then_some(Problem::Overlap { lines: pair });
    }
    // An end on the other piece, away from its ends.
    let on = |p: P2, s: P2, t: P2| geom::dist_to_segment(p, s, t) <= TOL && p.dist(s) > TOL && p.dist(t) > TOL;
    if let Some(p) = [c, d].into_iter().find(|&p| on(p, a, b)).or_else(|| [a, b].into_iter().find(|&p| on(p, c, d))) {
        return Some(Problem::Cross { lines: pair, at: p });
    }
    let x = a + (b - a) * geom::crossing(a, b, c, d)?;
    [a, b, c, d].iter().all(|p| p.dist(x) > TOL).then_some(Problem::Cross { lines: pair, at: x })
}

/// `ring` counter-clockwise (`ccw`) or clockwise, from the same first point.
fn wound(ring: &[P2], ccw: bool) -> Vec<P2> {
    match ring.split_first() {
        Some((first, rest)) if (geom::area(ring) > 0.0) != ccw => std::iter::once(*first).chain(rest.iter().rev().copied()).collect(),
        _ => ring.to_vec(),
    }
}

fn cap(mut problems: Vec<Problem>) -> Vec<Problem> {
    problems.truncate(MAX_PROBLEMS);
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(pts: &[(f64, f64)]) -> Vec<SketchLine> {
        let p: Vec<P2> = pts.iter().map(|&(x, y)| P2(x, y)).collect();
        geom::ring_edges(&p).map(|(a, b)| SketchLine::new(a, b)).collect()
    }

    fn square(x: f64, y: f64, s: f64) -> Vec<SketchLine> {
        ring(&[(x, y), (x + s, y), (x + s, y + s), (x, y + s)])
    }

    fn lines_in(problems: &[Problem]) -> Vec<usize> {
        let mut v: Vec<usize> = problems.iter().flat_map(Problem::lines).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    #[test]
    fn lines_in_any_order_either_way_round_close_into_one_outline_counter_clockwise() {
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.swap(0, 2);
        ls[1] = SketchLine::new(ls[1].end(), ls[1].start());
        ls[3] = SketchLine::new(ls[3].end(), ls[3].start());
        // Ends a little apart still meet.
        ls[0] = SketchLine::new(ls[0].start() + P2(0.4, 0.0), ls[0].end());
        let r = solve_one(&ls).unwrap();
        assert!(geom::area(&r.outer) > 0.0 && r.holes.is_empty());
        assert!((r.area() - 16e6).abs() < 2000.0, "{}", r.area());
        // Drawn clockwise, it comes out the same way round.
        let cw = ring(&[(0.0, 0.0), (0.0, 3000.0), (3000.0, 3000.0), (3000.0, 0.0)]);
        assert!(geom::area(&solve_one(&cw).unwrap().outer) > 0.0);
    }

    #[test]
    fn a_loop_inside_is_a_hole_and_one_inside_that_an_island() {
        let mut ls = square(0.0, 0.0, 10_000.0);
        ls.extend(square(2000.0, 2000.0, 6000.0));
        ls.extend(square(4000.0, 4000.0, 2000.0));
        ls.extend(square(20_000.0, 0.0, 1000.0));
        let p = solve(&ls).unwrap();
        assert_eq!(p.regions.len(), 3, "the outline, the island, the one apart");
        assert_eq!(p.regions[0].holes.len(), 1);
        assert!(geom::area(&p.regions[0].holes[0]) < 0.0, "holes are clockwise");
        assert!(p.regions[1].holes.is_empty() && p.regions[2].holes.is_empty());
        assert!((p.area() - (100e6 - 36e6 + 4e6 + 1e6)).abs() < 1.0);
        assert_eq!(solve_one(&ls), Err(vec![Problem::Separate { count: 3 }]));
    }

    #[test]
    fn an_open_loop_names_its_loose_ends() {
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.pop();
        let e = solve(&ls).unwrap_err();
        assert_eq!(e, vec![Problem::Open { line: 0, at: P2(0.0, 0.0) }, Problem::Open { line: 2, at: P2(0.0, 4000.0) }]);
        assert!(e[0].to_string().contains("open"), "{}", e[0]);
        // A gap wider than TOL is open too.
        let mut ls = square(0.0, 0.0, 4000.0);
        ls[3] = SketchLine::new(ls[3].start(), ls[3].end() + P2(0.0, 5.0));
        assert!(matches!(solve(&ls).unwrap_err()[..], [Problem::Open { line: 0, .. }, Problem::Open { line: 3, .. }]));
    }

    #[test]
    fn crossing_lines_and_a_line_ending_on_another_intersect() {
        // A bow tie.
        let bow = ring(&[(0.0, 0.0), (4000.0, 4000.0), (4000.0, 0.0), (0.0, 4000.0)]);
        let e = solve(&bow).unwrap_err();
        assert_eq!(e, vec![Problem::Cross { lines: [0, 2], at: P2(2000.0, 2000.0) }]);
        // A line ending part way along another: a T, which also leaves three lines at no point but an open end.
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.push([P2(2000.0, 0.0), P2(2000.0, 2000.0)].into());
        let e = solve(&ls).unwrap_err();
        assert!(e.contains(&Problem::Cross { lines: [0, 4], at: P2(2000.0, 0.0) }), "{e:?}");
        assert!(e.contains(&Problem::Open { line: 4, at: P2(2000.0, 2000.0) }), "{e:?}");
        // Two loops crossing each other.
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.extend(square(2000.0, 2000.0, 4000.0));
        let e = solve(&ls).unwrap_err();
        assert_eq!(lines_in(&e), vec![1, 2, 4, 7]);
        assert!(e.iter().all(|p| matches!(p, Problem::Cross { .. })));
    }

    #[test]
    fn lines_along_each_other_overlap_and_a_line_drawn_twice_does_too() {
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.push([P2(1000.0, 0.0), P2(3000.0, 0.0)].into());
        assert!(solve(&ls).unwrap_err().contains(&Problem::Overlap { lines: [0, 4] }));
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.push(SketchLine::new(ls[2].end(), ls[2].start()));
        assert!(solve(&ls).unwrap_err().contains(&Problem::Overlap { lines: [2, 4] }));
        // A straight edge in two lines, end to end, is fine.
        let ls = ring(&[(0.0, 0.0), (2000.0, 0.0), (4000.0, 0.0), (4000.0, 4000.0), (0.0, 4000.0)]);
        assert!(solve_one(&ls).is_ok());
    }

    #[test]
    fn loops_touching_at_a_corner_branch_there() {
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.extend(square(4000.0, 4000.0, 4000.0));
        let e = solve(&ls).unwrap_err();
        assert_eq!(e, vec![Problem::Branch { lines: vec![1, 2, 4, 7], at: P2(4000.0, 4000.0) }]);
        assert!(e[0].to_string().contains("line 1, line 2, line 4 and line 7"), "{}", e[0]);
    }

    #[test]
    fn lines_too_short_or_out_of_range_are_named_before_anything_else() {
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.push([P2(9000.0, 0.0), P2(9000.5, 0.0)].into());
        ls.push([P2(f64::NAN, 0.0), P2(1.0, 1.0)].into());
        ls.push([P2(0.0, 0.0), P2(2e9, 0.0)].into());
        assert_eq!(
            solve(&ls).unwrap_err(),
            vec![Problem::TooShort { line: 4 }, Problem::OutOfRange { line: 5 }, Problem::OutOfRange { line: 6 }]
        );
        assert_eq!(solve(&[]).unwrap_err(), vec![Problem::Empty]);
        let many = vec![SketchLine::new(P2(0.0, 0.0), P2(10.0, 0.0)); MAX_LINES + 1];
        assert_eq!(solve(&many).unwrap_err(), vec![Problem::TooMany { count: MAX_LINES + 1 }]);
    }

    #[test]
    fn a_thin_loop_encloses_no_area() {
        // Three points almost in a line: the loop is closed but flat.
        let ls = ring(&[(0.0, 0.0), (4000.0, 0.5), (8000.0, 0.0)]);
        let e = solve(&ls).unwrap_err();
        assert!(
            matches!(&e[..], [Problem::NoArea { lines }] if *lines == vec![0, 1, 2])
                || e.iter().any(|p| matches!(p, Problem::Overlap { .. })),
            "{e:?}"
        );
    }

    #[test]
    fn an_outline_and_its_holes_come_back_as_lines_that_solve_to_them() {
        let outline = vec![P2(0.0, 0.0), P2(6000.0, 0.0), P2(6000.0, 6000.0), P2(0.0, 6000.0)];
        let hole = vec![P2(1000.0, 1000.0), P2(1000.0, 2000.0), P2(2000.0, 2000.0), P2(2000.0, 1000.0)];
        let ls = lines_of(&outline, std::slice::from_ref(&hole));
        assert_eq!(ls.len(), 8);
        let r = solve_one(&ls).unwrap();
        assert_eq!((r.outer, r.holes), (outline, vec![hole]));
    }

    #[test]
    fn many_problems_are_capped() {
        // A comb: every tooth crosses the spine.
        let mut ls = vec![SketchLine::new(P2(0.0, 0.0), P2(1e6, 0.0))];
        ls.extend((1..500).map(|i| SketchLine::new(P2(i as f64 * 1000.0, -500.0), P2(i as f64 * 1000.0, 500.0))));
        let e = solve(&ls).unwrap_err();
        assert_eq!(e.len(), MAX_PROBLEMS);
        let name = |i: usize| format!("line {i}");
        assert!(say(&e, &name).contains("more"), "{}", say(&e, &name));
    }

    #[test]
    fn random_lines_never_panic_and_what_solves_is_sound() {
        let mut seed = 7u64;
        let mut next = || {
            seed = crate::model::mix(seed);
            (seed % 9000) as f64
        };
        for _ in 0..400 {
            let n = 1 + (next() as usize) % 9;
            let ls: Vec<SketchLine> = (0..n).map(|_| SketchLine::new(P2(next(), next()), P2(next(), next()))).collect();
            // Closed as a ring too, to make loops more often.
            let pts: Vec<P2> = ls.iter().map(|l| l.start()).collect();
            for sk in [ls, geom::ring_edges(&pts).map(|(a, b)| SketchLine::new(a, b)).collect()] {
                if let Ok(p) = solve(&sk) {
                    for r in &p.regions {
                        assert!(geom::area(&r.outer) > 0.0 && r.holes.iter().all(|h| geom::area(h) < 0.0));
                    }
                    assert!(p.area() > 0.0);
                }
            }
        }
    }

    #[test]
    fn an_arc_closes_a_loop_and_is_one_line_however_many_chords_draw_it() {
        // A half disc: its diameter, then the arc back round through the top.
        let ls = vec![SketchLine::new(P2(0.0, 0.0), P2(4000.0, 0.0)), SketchLine::arc(P2(4000.0, 0.0), P2(0.0, 0.0), P2(2000.0, 2000.0))];
        let r = solve_one(&ls).unwrap();
        let half = std::f64::consts::PI * 2000.0 * 2000.0 / 2.0;
        assert!((r.area() - half).abs() / half < 0.01, "{} vs {half}", r.area());
        assert!(r.outer.len() > 20, "drawn by its chords");
        assert!((ls[1].middle().dist(P2(2000.0, 2000.0))) < 1.0, "{:?}", ls[1].middle());
        // A line across it hits the arc: one problem, naming the arc once.
        let mut crossed = ls.clone();
        crossed.push(SketchLine::new(P2(1000.0, 1000.0), P2(1000.0, 3000.0)));
        let e = solve(&crossed).unwrap_err();
        assert_eq!(e[0], Problem::Cross { lines: [1, 2], at: e[0].at().unwrap() }, "{e:?}");
        assert_eq!(e.iter().filter(|p| matches!(p, Problem::Cross { .. })).count(), 1);
        // A point through on the straight line between its ends is a straight line.
        assert_eq!(SketchLine::arc(P2(0.0, 0.0), P2(4000.0, 0.0), P2(2000.0, 0.5)).points().len(), 2);
    }

    #[test]
    fn crossings_come_before_the_open_ends_they_leave_and_lines_are_named_by_id() {
        // Lines overshooting each other at a corner: they cross, and leave ends hanging past it.
        let mut ls = ring(&[(0.0, 0.0), (4000.0, 0.0), (4000.0, 4000.0)]);
        ls.truncate(2);
        ls.push(SketchLine::new(P2(4000.0, 4500.0), P2(-500.0, 4500.0)));
        ls.push(SketchLine::new(P2(0.0, 5000.0), P2(0.0, 0.0)));
        let e = solve(&ls).unwrap_err();
        assert!(matches!(e[0], Problem::Cross { .. }), "{e:?}");
        assert!(e.iter().any(|p| matches!(p, Problem::Open { .. })));
        ls[3].id = 999_812;
        let name = namer(&ls);
        let said = say(&e, &name);
        assert!(said.contains("line 2 and line 999812 intersect at [0, 4500]"), "{said}");
    }

    #[test]
    fn a_kept_sketch_draws_its_outline_from_any_start_either_way_round() {
        let outline = vec![P2(0.0, 0.0), P2(6000.0, 0.0), P2(6000.0, 6000.0), P2(0.0, 6000.0)];
        let ls = lines_of(&outline, &[]);
        assert!(draws(&ls, &outline, &[]));
        let turned: Vec<P2> = outline.iter().rev().copied().collect();
        assert!(draws(&ls, &turned, &[]));
        let moved: Vec<P2> = outline.iter().map(|p| *p + P2(10.0, 0.0)).collect();
        assert!(!draws(&ls, &moved, &[]));
        assert!(!draws(&ls, &outline, std::slice::from_ref(&outline)));
    }
}

/// The sketch through the commands: a dry run, and the outlines of floors, ceilings, roofs and shafts drawn or edited as one.
#[cfg(test)]
mod command_tests {
    use serde_json::{Value, json};

    use crate::engine::Session;
    use crate::model::Element;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    /// A square's lines, `[x, y]` its south-west corner, drawn clockwise.
    fn square(x: f64, y: f64, size: f64) -> Vec<Value> {
        let c = [[x, y], [x, y + size], [x + size, y + size], [x + size, y]];
        (0..4).map(|i| json!([c[i], c[(i + 1) % 4]])).collect()
    }

    fn with_hole() -> Value {
        let mut ls = square(0.0, 0.0, 6000.0);
        ls.extend(square(2000.0, 2000.0, 1000.0));
        Value::Array(ls)
    }

    #[test]
    fn a_sketch_is_solved_without_changing_the_model_and_says_what_is_wrong_line_by_line() {
        let mut s = Session::new();
        let steps = s.history().len();
        let r = run(&mut s, "sketch", json!({"lines": with_hole()}));
        assert_eq!(r["regions"].as_array().map(Vec::len), Some(1));
        assert_eq!(r["regions"][0]["holes"].as_array().map(Vec::len), Some(1));
        assert_eq!(r["area_m2"], 35.0);
        assert_eq!(r["regions"][0]["outline"][1], json!([6000.0, 0.0]), "counter-clockwise whichever way it was drawn");
        assert_eq!(s.history().len(), steps, "no undo step");
        // Open, crossing: every problem, with the lines it's about.
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.pop();
        ls.push(json!([[-1000, 2000], [1000, 2000]]));
        let e = s.execute("sketch", &json!({"lines": ls})).unwrap_err();
        assert!(e.contains("line 0 and line 3 intersect at [0, 2000]"), "{e}");
        assert!(e.contains("line 2 ends at [4000, 0] without meeting another"), "{e}");
        let e = s.execute("sketch", &json!({"lines": [[[0, 0], [1]]]})).unwrap_err();
        assert!(e.contains("`lines` is"), "{e}");
    }

    #[test]
    fn floors_ceilings_roofs_and_shafts_take_their_outline_and_holes_as_a_sketch() {
        let mut s = Session::new();
        let f = run(&mut s, "floor", json!({"sketch": with_hole()}));
        assert_eq!(f["area_m2"], 35.0);
        let Some(Element::Floor(fl)) = s.model.elements.get(f["id"].as_u64().unwrap()).cloned() else { panic!() };
        assert_eq!((fl.boundary.len(), fl.openings.len()), (4, 1));
        assert!(crate::geom::area(&fl.boundary) > 0.0);
        let c = run(&mut s, "ceiling", json!({"sketch": with_hole()}));
        assert_eq!(c["area_m2"], 35.0);
        let r = run(&mut s, "roof", json!({"sketch": with_hole(), "slope": 0}));
        let Some(Element::Roof(rf)) = s.model.elements.get(r["id"].as_u64().unwrap()) else { panic!() };
        assert_eq!(rf.courtyards.len(), 1, "a roof's holes are its courtyards");
        run(&mut s, "shaft", json!({"sketch": square(0.0, 0.0, 1000.0)}));
        let e = s.execute("shaft", &json!({"sketch": with_hole()})).unwrap_err();
        assert!(e.contains("no holes"), "{e}");
        // One outline: two apart, or a sketch with a boundary too, is refused.
        let mut two = square(0.0, 0.0, 1000.0);
        two.extend(square(5000.0, 0.0, 1000.0));
        let e = s.execute("floor", &json!({"sketch": two})).unwrap_err();
        assert!(e.contains("2 separate outlines"), "{e}");
        let e = s.execute("floor", &json!({"sketch": with_hole(), "boundary": [[0, 0], [1000, 0], [0, 1000]]})).unwrap_err();
        assert!(e.contains("`sketch` or `boundary`"), "{e}");
    }

    #[test]
    fn a_boundary_given_as_points_is_judged_as_the_sketch_of_its_edges() {
        let mut s = Session::new();
        let e = s.execute("floor", &json!({"boundary": [[0, 0], [4000, 4000], [4000, 0], [0, 4000]]})).unwrap_err();
        assert!(e.contains("edge 0 and edge 2 intersect at [2000, 2000]"), "{e}");
        let e = s.execute("floor", &json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]], "openings": [[[1000, 1000], [2000, 2000], [2000, 1000], [1000, 2000]]]}))
            .unwrap_err();
        assert!(e.contains("opening 0") && e.contains("intersect"), "{e}");
        let e = s.execute("roof", &json!({"boundary": [[0, 0], [4000, 4000], [4000, 0], [0, 4000]]})).unwrap_err();
        assert!(e.contains("intersect"), "{e}");
        // Closed back to its first point, or a point twice, it's the same outline.
        let f = run(&mut s, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 0], [4000, 4000], [0, 4000], [0, 0]]}));
        let Some(Element::Floor(fl)) = s.model.elements.get(f["id"].as_u64().unwrap()) else { panic!() };
        assert_eq!(fl.boundary.len(), 4);
    }

    #[test]
    fn edit_boundary_redraws_an_outline_in_one_step_or_leaves_it_as_it_was() {
        let mut s = Session::new();
        let f = run(&mut s, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]}))["id"].clone();
        // What it is now, as lines to edit.
        let now = run(&mut s, "edit_boundary", json!({"id": f}));
        assert_eq!(now["sketch"].as_array().map(Vec::len), Some(4));
        let r = run(&mut s, "edit_boundary", json!({"id": f, "sketch": with_hole()}));
        assert_eq!(r["area_m2"], 35.0);
        let Some(Element::Floor(fl)) = s.model.elements.get(f.as_u64().unwrap()) else { panic!() };
        assert_eq!(fl.openings.len(), 1);
        assert_eq!(s.history().last(), Some(&"Edit Boundary"));
        let mut open = square(0.0, 0.0, 4000.0);
        open.pop();
        let before = s.model.clone();
        let e = s.execute("edit_boundary", &json!({"id": f, "sketch": open})).unwrap_err();
        assert!(e.contains("open"), "{e}");
        assert_eq!(s.model, before);
        run(&mut s, "undo", json!({}));
        let Some(Element::Floor(fl)) = s.model.elements.get(f.as_u64().unwrap()) else { panic!() };
        assert!(fl.openings.is_empty());
        // A wall has no boundary.
        let w = run(&mut s, "wall", json!({"points": [[0, 0], [1000, 0]]}))["ids"][0].clone();
        let e = s.execute("edit_boundary", &json!({"id": w, "sketch": with_hole()})).unwrap_err();
        assert!(e.contains("floor, a ceiling, a roof's footprint, a shaft or a filled region"), "{e}");
    }

    #[test]
    fn a_roofs_gable_edges_stay_gables_where_the_edited_footprint_keeps_them() {
        let mut s = Session::new();
        let r = run(&mut s, "roof", json!({"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "gables": [1, 3], "slope": 30}))["id"]
            .clone();
        // Its south edge moved down: the east and west edges are new lines, the north edge the same.
        let lines = json!([[[0, -1000], [8000, -1000]], [[8000, -1000], [8000, 6000]], [[8000, 6000], [0, 6000]], [[0, 6000], [0, -1000]]]);
        run(&mut s, "edit_boundary", json!({"id": r, "sketch": lines}));
        let Some(Element::Roof(rf)) = s.model.elements.get(r.as_u64().unwrap()) else { panic!() };
        assert!(rf.gables.is_empty(), "{:?}", rf.gables);
        // Back, and the gables on the east and west ends again, then the north edge in two lines.
        run(&mut s, "undo", json!({}));
        let lines = json!([
            [[0, 0], [8000, 0]],
            [[8000, 0], [8000, 6000]],
            [[8000, 6000], [4000, 6000]],
            [[4000, 6000], [0, 6000]],
            [[0, 6000], [0, 0]]
        ]);
        run(&mut s, "edit_boundary", json!({"id": r, "sketch": lines}));
        let Some(Element::Roof(rf)) = s.model.elements.get(r.as_u64().unwrap()) else { panic!() };
        assert_eq!(rf.gables, vec![1, 4]);
        // An extrusion roof is edited by its profile.
        let x = run(
            &mut s,
            "roof",
            json!({"extrusion": {"line": [[0, 0], [4000, 0]], "profile": [[0, 0], [2000, 1000], [4000, 0]], "to": 3000}}),
        )["id"]
            .clone();
        let e = s.execute("edit_boundary", &json!({"id": x, "sketch": with_hole()})).unwrap_err();
        assert!(e.contains("extrusion"), "{e}");
    }

    /// A floor's sketch lines, as `elements` gives them.
    fn lines(s: &mut Session, id: &Value) -> Vec<Value> {
        let r = run(s, "elements", json!({"category": "floor"}));
        let found = r["elements"].as_array().and_then(|es| es.iter().find(|e| e["id"] == *id).cloned()).unwrap_or_default();
        found["sketch"].as_array().cloned().unwrap_or_default()
    }

    #[test]
    fn each_sketch_line_has_an_id_and_a_guid_of_its_own_kept_through_edits_and_moves() {
        let mut s = Session::new();
        let f = run(&mut s, "floor", json!({"sketch": square(0.0, 0.0, 6000.0)}))["id"].clone();
        let kept = lines(&mut s, &f);
        assert_eq!(kept.len(), 4);
        let ids: Vec<u64> = kept.iter().filter_map(|l| l["id"].as_u64()).collect();
        assert_eq!(ids.len(), 4, "{kept:?}");
        assert!(ids.iter().all(|&i| (100_000..=999_999).contains(&i)), "{ids:?}");
        assert!(kept.iter().all(|l| l["guid"].is_string()));
        // Edit Boundary gives them back; three sent again by id keep theirs, the fourth redrawn as two is new.
        let now = run(&mut s, "edit_boundary", json!({"id": f}))["sketch"].clone();
        assert_eq!(now.as_array().map(Vec::len), Some(4));
        let mut again: Vec<Value> =
            now.as_array().unwrap()[..3].iter().map(|l| json!({"id": l["id"], "start": l["start"], "end": l["end"]})).collect();
        let last = &now[3];
        let mid = json!([3000.0, 0.0]);
        again.push(json!([last["start"], mid]));
        again.push(json!([mid, last["end"]]));
        run(&mut s, "edit_boundary", json!({"id": f, "sketch": again}));
        let after = lines(&mut s, &f);
        let after_ids: Vec<u64> = after.iter().filter_map(|l| l["id"].as_u64()).collect();
        assert_eq!(after_ids[..3], ids[..3]);
        assert!(after_ids[3..].iter().all(|i| !ids.contains(i)) && after_ids.len() == 5, "{after_ids:?}");
        // An id that isn't one of its lines is refused.
        let e = s.execute("edit_boundary", &json!({"id": f, "sketch": [{"id": 123_456, "start": [0, 0], "end": [1, 1]}]})).unwrap_err();
        assert!(e.contains("123456 isn't a line of this sketch"), "{e}");
        // Moved, the sketch goes with it and keeps its ids; copied, the copy's lines are new.
        run(&mut s, "move", json!({"ids": [f], "by": [1000, 0]}));
        let moved = lines(&mut s, &f);
        assert_eq!(moved[0]["id"], after[0]["id"]);
        assert_eq!(moved[0]["start"][0].as_f64(), after[0]["start"][0].as_f64().map(|x| x + 1000.0));
        let copy = run(&mut s, "copy", json!({"ids": [f], "by": [0, 10_000]}))["ids"][0].clone();
        let copied: Vec<u64> = lines(&mut s, &copy).iter().filter_map(|l| l["id"].as_u64()).collect();
        assert_eq!(copied.len(), 5);
        assert!(copied.iter().all(|i| !after_ids.contains(i)), "{copied:?}");
        // A floor made over the walls keeps none until its boundary is edited; its lines then come as new ones.
        run(&mut s, "wall", json!({"points": [[20000, 0], [26000, 0], [26000, 6000], [20000, 6000]], "closed": true}));
        let w = run(&mut s, "floor", json!({}))["id"].clone();
        assert!(lines(&mut s, &w).is_empty());
        let fresh = run(&mut s, "edit_boundary", json!({"id": w}))["sketch"].clone();
        assert!(fresh.as_array().unwrap().iter().all(|l| l.get("id").is_none()));
    }

    #[test]
    fn an_arc_sketched_rounds_the_floor() {
        let mut s = Session::new();
        let half = json!([[[0, 0], [4000, 0]], [[4000, 0], [0, 0], [2000, 2000]]]);
        let f = run(&mut s, "floor", json!({"sketch": half}));
        let a = f["area_m2"].as_f64().unwrap();
        assert!((a - std::f64::consts::TAU).abs() < 0.1, "{a}");
        // Kept by how far it turns, counter-clockwise as a wall's arc: west from (4000, 0), bulging north to its right.
        let turn = lines(&mut s, &f["id"])[1]["arc"].as_f64().unwrap_or_default();
        assert!((turn - 180.0).abs() < 1e-6, "{turn}");
    }
}
