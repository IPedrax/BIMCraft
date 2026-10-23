//! Sketches: Revit's sketch mode, the one way an outline is drawn for a floor, a ceiling, a roof's footprint, a shaft, and
//! what's built on them later (filled regions, topography).
//!
//! A sketch is lines, drawn in any order and either way round. [`solve`] joins them, end to end within [`TOL`], into closed
//! loops and nests the loops: a loop inside no other is an outline, one inside an outline a hole in it, one inside a hole an
//! island, an outline again. What can't be solved it names, every problem at once and the lines it's about, so the lines
//! can be shown to whoever drew them: a line too short or out of range, two lines crossing or one ending on another, two
//! lines along each other, an end no other line meets, three or more lines meeting at a point.

use std::fmt;

use serde::Serialize;

use crate::geom::{self, P2, Region};
use crate::model::{MAX_COORD, MAX_POINTS, TOL};

/// A sketch line, from one end to the other.
pub type Line = [P2; 2];

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
            Problem::OutOfRange { line } | Problem::TooShort { line } | Problem::Open { line, .. } => vec![*line],
            Problem::Cross { lines, .. } | Problem::Overlap { lines } => lines.to_vec(),
            Problem::Branch { lines, .. } | Problem::NoArea { lines } => lines.clone(),
        }
    }
}

impl Problem {
    /// It said in a sentence, the lines called `noun` (`line`, or `edge` for a ring's).
    pub fn said(&self, noun: &str) -> String {
        let at = |p: &P2| format!("[{}, {}]", p.0.round(), p.1.round());
        let list = |ls: &[usize]| ls.iter().map(usize::to_string).collect::<Vec<_>>().join(", ");
        match self {
            Problem::Empty => format!("there are no {noun}s: draw the outline"),
            Problem::TooMany { count } => format!("{count} {noun}s, more than {MAX_LINES}"),
            Problem::OutOfRange { line } => format!("{noun} {line} has an end that isn't a number within {MAX_COORD} mm"),
            Problem::TooShort { line } => format!("{noun} {line} is {TOL} mm long or less"),
            Problem::Cross { lines: [a, b], at: p } => format!("{noun}s {a} and {b} intersect at {}", at(p)),
            Problem::Overlap { lines: [a, b] } => format!("{noun}s {a} and {b} overlap"),
            Problem::Open { line, at: p } => format!("{noun} {line} ends at {} without meeting another: the loop is open", at(p)),
            Problem::Branch { lines, at: p } => format!("{noun}s {} meet at {}: a loop passes a point once", list(lines), at(p)),
            Problem::NoArea { lines } => format!("the loop of {noun}s {} encloses no area", list(lines)),
            Problem::Separate { count } => format!("the {noun}s make {count} separate outlines, where one is wanted"),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.said("line"))
    }
}

/// Problems said in one line, the first few named, the lines called `noun`.
pub fn say(problems: &[Problem], noun: &str) -> String {
    let shown: Vec<String> = problems.iter().take(5).map(|p| p.said(noun)).collect();
    match problems.len() {
        n if n > shown.len() => format!("{}; and {} more", shown.join("; "), n - shown.len()),
        _ => shown.join("; "),
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

/// The lines solved into loops, nested into regions; or every problem found (up to [`MAX_PROBLEMS`]).
pub fn solve(lines: &[Line]) -> Result<Profile, Vec<Problem>> {
    if lines.is_empty() {
        return Err(vec![Problem::Empty]);
    }
    if lines.len() > MAX_LINES {
        return Err(vec![Problem::TooMany { count: lines.len() }]);
    }
    // Each line on its own first: its ends are numbers, apart.
    let mut problems: Vec<Problem> = Vec::new();
    for (i, [a, b]) in lines.iter().enumerate() {
        let fine = |p: &P2| p.is_finite() && p.0.abs() <= MAX_COORD && p.1.abs() <= MAX_COORD;
        if !(fine(a) && fine(b)) {
            problems.push(Problem::OutOfRange { line: i });
        } else if a.dist(*b) <= TOL {
            problems.push(Problem::TooShort { line: i });
        }
    }
    if !problems.is_empty() {
        return Err(cap(problems));
    }
    // Ends within TOL of each other are one point.
    let mut points: Vec<P2> = Vec::new();
    let ends: Vec<[usize; 2]> = lines.iter().map(|l| l.map(|p| weld(&mut points, p))).collect();
    // Each pair of lines that could touch: sorted by where they start across, a line is checked against those starting
    // before it ends.
    let lo = |l: &Line| l[0].0.min(l[1].0);
    let hi = |l: &Line| l[0].0.max(l[1].0);
    let mut order: Vec<usize> = (0..lines.len()).collect();
    order.sort_by(|&i, &j| lo(&lines[i]).total_cmp(&lo(&lines[j])).then(i.cmp(&j)));
    for (k, &i) in order.iter().enumerate() {
        for &j in &order[k + 1..] {
            if lo(&lines[j]) > hi(&lines[i]) + TOL {
                break;
            }
            let pair = [i.min(j), i.max(j)];
            if let Some(p) = meet(lines[pair[0]], lines[pair[1]], ends[pair[0]], ends[pair[1]], pair) {
                problems.push(p);
                if problems.len() > MAX_PROBLEMS {
                    return Err(cap(problems));
                }
            }
        }
    }
    // Two lines at every point: one, it's open; three or more, it branches.
    let mut at: Vec<Vec<usize>> = vec![Vec::new(); points.len()];
    for (i, e) in ends.iter().enumerate() {
        for &n in e {
            at[n].push(i);
        }
    }
    for (n, ls) in at.iter().enumerate() {
        match ls.as_slice() {
            [_, _] => {}
            &[line] => problems.push(Problem::Open { line, at: points[n] }),
            _ => problems.push(Problem::Branch { lines: ls.clone(), at: points[n] }),
        }
    }
    if !problems.is_empty() {
        problems.sort_by_key(|p| p.lines().first().copied().unwrap_or(0));
        return Err(cap(problems));
    }
    // Every point has two lines: each piece of the sketch is one loop, walked from its first line the way it was drawn.
    let mut used = vec![false; lines.len()];
    let mut loops: Vec<(Vec<P2>, Vec<usize>)> = Vec::new();
    for first in 0..lines.len() {
        if used[first] {
            continue;
        }
        let (mut ring, mut members) = (Vec::new(), Vec::new());
        let (mut line, mut from) = (first, ends[first][0]);
        while !used[line] {
            used[line] = true;
            members.push(line);
            ring.push(points[from]);
            let to = if ends[line][0] == from { ends[line][1] } else { ends[line][0] };
            from = to;
            line = at[to].iter().copied().find(|&l| l != line).unwrap_or(line);
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
pub fn solve_one(lines: &[Line]) -> Result<Region, Vec<Problem>> {
    let mut p = solve(lines)?;
    match p.regions.len() {
        1 => Ok(p.regions.remove(0)),
        count => Err(vec![Problem::Separate { count }]),
    }
}

/// An outline and its holes as sketch lines, to edit: each ring's edges in order.
pub fn lines_of(outline: &[P2], holes: &[Vec<P2>]) -> Vec<Line> {
    std::iter::once(outline).chain(holes.iter().map(Vec::as_slice)).flat_map(|r| geom::ring_edges(r).map(|(a, b)| [a, b])).collect()
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

/// How two lines (`pair`, given in order) go wrong where they meet, if they do: along each other, crossing, or one ending
/// part way along the other. Meeting end to end is right.
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
    // An end on the other line, away from its ends.
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

    fn ring(pts: &[(f64, f64)]) -> Vec<Line> {
        let p: Vec<P2> = pts.iter().map(|&(x, y)| P2(x, y)).collect();
        geom::ring_edges(&p).map(|(a, b)| [a, b]).collect()
    }

    fn square(x: f64, y: f64, s: f64) -> Vec<Line> {
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
        ls[1] = [ls[1][1], ls[1][0]];
        ls[3] = [ls[3][1], ls[3][0]];
        // Ends a little apart still meet.
        ls[0][0].0 += 0.4;
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
        ls[3][1].1 += 5.0;
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
        ls.push([P2(2000.0, 0.0), P2(2000.0, 2000.0)]);
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
        ls.push([P2(1000.0, 0.0), P2(3000.0, 0.0)]);
        assert!(solve(&ls).unwrap_err().contains(&Problem::Overlap { lines: [0, 4] }));
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.push([ls[2][1], ls[2][0]]);
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
        assert!(e[0].to_string().contains("1, 2, 4, 7"), "{}", e[0]);
    }

    #[test]
    fn lines_too_short_or_out_of_range_are_named_before_anything_else() {
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.push([P2(9000.0, 0.0), P2(9000.5, 0.0)]);
        ls.push([P2(f64::NAN, 0.0), P2(1.0, 1.0)]);
        ls.push([P2(0.0, 0.0), P2(2e9, 0.0)]);
        assert_eq!(
            solve(&ls).unwrap_err(),
            vec![Problem::TooShort { line: 4 }, Problem::OutOfRange { line: 5 }, Problem::OutOfRange { line: 6 }]
        );
        assert_eq!(solve(&[]).unwrap_err(), vec![Problem::Empty]);
        let many = vec![[P2(0.0, 0.0), P2(10.0, 0.0)]; MAX_LINES + 1];
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
        let mut ls = vec![[P2(0.0, 0.0), P2(1e6, 0.0)]];
        ls.extend((1..500).map(|i| [P2(i as f64 * 1000.0, -500.0), P2(i as f64 * 1000.0, 500.0)]));
        let e = solve(&ls).unwrap_err();
        assert_eq!(e.len(), MAX_PROBLEMS);
        assert!(say(&e, "line").contains("more"), "{}", say(&e, "line"));
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
            let ls: Vec<Line> = (0..n).map(|_| [P2(next(), next()), P2(next(), next())]).collect();
            // Closed as a ring too, to make loops more often.
            let pts: Vec<P2> = ls.iter().map(|l| l[0]).collect();
            for sk in [ls, geom::ring_edges(&pts).map(|(a, b)| [a, b]).collect()] {
                if let Ok(p) = solve(&sk) {
                    for r in &p.regions {
                        assert!(geom::area(&r.outer) > 0.0 && r.holes.iter().all(|h| geom::area(h) < 0.0));
                    }
                    assert!(p.area() > 0.0);
                }
            }
        }
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
        assert!(e.contains("lines 0 and 3 intersect at [0, 2000]"), "{e}");
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
        assert!(e.contains("edges 0 and 2 intersect at [2000, 2000]"), "{e}");
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
        assert!(e.contains("floor, a ceiling, a roof's footprint or a shaft"), "{e}");
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
}
