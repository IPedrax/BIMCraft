//! A fill pattern made from lines drawn in a view, as pyRevit's Make Pattern makes one: the detail lines picked, cut to a
//! rectangle (the tile), become the families of a pattern that repeats that tile edge to edge.
//!
//! A line of the tile, repeated with it, is a family of parallel lines: one line runs on into the next tile's copy of
//! itself only if its slope is a fraction of the tile's, `j` tiles up for `i` across (a line at any other angle never
//! meets itself again), so a line is turned to a slope `(i W, j H)` with `i` and `j` at most [`MAX_STEPS`]: the simplest
//! within [`TURN`] of its own, else the nearest. Then the family's lines repeat every `L = |(i W, j H)|` along, a dash of the line's length and a
//! space for the rest; and lie `W H / L` apart, each shifted along from the last by the other vector of the tile's
//! lattice, `(c W, d H)` with `i d - j c = 1` (Euclid's). A line on the tile's edge and its copy on the opposite edge are
//! one family, kept once.

use crate::fills::{FillPattern, HatchLine, MAX_FAMILIES, MAX_SPACING, MIN_SPACING, Target};
use crate::geom::P2;
use crate::ids::Guid;
use crate::model::{Element, Id, Model};

/// The most tiles across or up a family's line runs before it meets itself again.
pub const MAX_STEPS: i64 = 48;

/// The most points the lines picked may have between them, ends and the points of their curves.
pub const MAX_POINTS: usize = 20_000;

/// The pattern the straight pieces `segments` make of the tile between corners `a` and `b` (any two opposite ones), the
/// pattern's origin at its lowest left corner, its lengths as theirs; a model pattern. What lies outside the tile is cut
/// off. An error says what to fix: a tile too small or too big, no line in it, more lines than a pattern has families.
pub fn from_segments(segments: &[[P2; 2]], a: P2, b: P2) -> Result<FillPattern, String> {
    if !(a.is_finite() && b.is_finite()) {
        return Err("the corners are points".into());
    }
    let (lo, hi) = (P2(a.0.min(b.0), a.1.min(b.1)), P2(a.0.max(b.0), a.1.max(b.1)));
    let (w, h) = (hi.0 - lo.0, hi.1 - lo.1);
    if !(w >= MIN_SPACING && h >= MIN_SPACING && w <= MAX_SPACING && h <= MAX_SPACING) {
        return Err(format!("the tile is {w:.3} by {h:.3}; each side is {MIN_SPACING} to {MAX_SPACING}"));
    }
    let tiny = 1e-6 * w.max(h);
    let mut lines: Vec<(i64, i64, P2, f64)> = Vec::new();
    for s in segments {
        if !(s[0].is_finite() && s[1].is_finite()) {
            continue;
        }
        let Some([p, q]) = clip(*s, lo, hi) else { continue };
        let len = p.dist(q);
        if len <= tiny {
            continue;
        }
        let (i, j) = steps(q - p, w, h);
        // Along the family's way: the line starts at its end that lies behind.
        let way = P2(i as f64 * w, j as f64 * h);
        let start = if (q - p).dot(way) >= 0.0 { p } else { q } - lo;
        let origin = P2(start.0.rem_euclid(w), start.1.rem_euclid(h));
        let same = |&(i2, j2, o, l): &(i64, i64, P2, f64)| {
            let d = o - origin;
            let whole = |x: f64| (x - x.round()).abs() < 1e-6;
            (i2, j2) == (i, j) && (l - len).abs() < tiny && whole(d.0 / w) && whole(d.1 / h)
        };
        if !lines.iter().any(same) {
            lines.push((i, j, origin, len));
        }
    }
    if lines.is_empty() {
        return Err("no line lies inside the tile: pick lines that cross the rectangle between its corners".into());
    }
    if lines.len() > MAX_FAMILIES {
        return Err(format!("{} lines inside the tile; a pattern holds at most {MAX_FAMILIES}", lines.len()));
    }
    let lines = lines.into_iter().map(|(i, j, origin, len)| family(i, j, origin, len, w, h)).collect();
    let pattern = FillPattern { target: Target::Model, lines };
    pattern.check("made").map_err(|e| e.replace("fill pattern `made`", "the pattern"))?;
    Ok(pattern)
}

/// The family of a line from `origin`, `len` long, its way `i` tiles across for `j` up, in a tile `w` by `h`.
fn family(i: i64, j: i64, origin: P2, len: f64, w: f64, h: f64) -> HatchLine {
    let way = P2(i as f64 * w, j as f64 * h);
    let period = way.length();
    let u = way * (1.0 / period);
    // i x + j y = 1, so i d - j c = 1 with d = x, c = -y: (c W, d H) and the way span the tile's lattice.
    let (_, x, y) = euclid(i, j);
    let next = P2(-y as f64 * w, x as f64 * h);
    let gap = period - len;
    HatchLine {
        angle: way.1.atan2(way.0).to_degrees(),
        origin,
        shift: next.dot(u).rem_euclid(period),
        spacing: w * h / period,
        dashes: if gap > 1e-6 * period { vec![len, -gap] } else { Vec::new() },
    }
}

/// How far a line may be turned, degrees, for a way that repeats sooner: a line drawn by hand a little off a simple slope
/// takes it, rather than one that runs through dozens of tiles before it meets itself.
pub const TURN: f64 = 0.25;

/// The way `i` tiles across for `j` up, coprime, for the way of `d` (either way along it), in a tile `w` by `h`: the one that
/// repeats soonest of those within [`TURN`] of it, else the nearest. Ways whose lines would lie closer than a pattern's may
/// are passed over.
fn steps(d: P2, w: f64, h: f64) -> (i64, i64) {
    let target = d.1.atan2(d.0);
    let turn = TURN.to_radians();
    let mut best = (1, 0, f64::INFINITY, f64::INFINITY);
    for j in 0..=MAX_STEPS {
        for i in -MAX_STEPS..=MAX_STEPS {
            if (j == 0 && i <= 0) || euclid(i, j).0 != 1 {
                continue;
            }
            let (x, y) = (i as f64 * w, j as f64 * h);
            let len = x.hypot(y);
            if w * h / len < MIN_SPACING {
                continue;
            }
            // Lines have no way along them: an angle and its opposite are one.
            let off = (y.atan2(x) - target).rem_euclid(std::f64::consts::PI);
            let off = off.min(std::f64::consts::PI - off);
            let better = match (off <= turn, best.2 <= turn) {
                (true, true) => len < best.3,
                (true, false) => true,
                (false, true) => false,
                (false, false) => off < best.2 - 1e-12 || (off <= best.2 + 1e-12 && len < best.3),
            };
            if better {
                best = (i, j, off, len);
            }
        }
    }
    (best.0, best.1)
}

/// The greatest common divisor of `a` and `b`, positive, and `x`, `y` with `a x + b y` it.
fn euclid(a: i64, b: i64) -> (i64, i64, i64) {
    let (mut r0, mut r1, mut x0, mut x1, mut y0, mut y1) = (a, b, 1, 0, 0, 1);
    while r1 != 0 {
        let q = r0 / r1;
        (r0, r1) = (r1, r0 - q * r1);
        (x0, x1) = (x1, x0 - q * x1);
        (y0, y1) = (y1, y0 - q * y1);
    }
    if r0 < 0 { (-r0, -x0, -y0) } else { (r0, x0, y0) }
}

/// The part of the piece from `a` to `b` inside the box from `lo` to `hi` (Liang and Barsky's), if any.
fn clip([a, b]: [P2; 2], lo: P2, hi: P2) -> Option<[P2; 2]> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [(-d.0, a.0 - lo.0), (d.0, hi.0 - a.0), (-d.1, a.1 - lo.1), (d.1, hi.1 - a.1)] {
        if p.abs() < 1e-15 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            t0 = t0.max(r);
        } else {
            t1 = t1.min(r);
        }
    }
    (t1 > t0).then(|| [a + d * t0, a + d * t1])
}

/// What a pattern made from lines of a view is made of: the pattern in the view's lengths (a model pattern), and how many
/// of them a mm on paper is, the view's scale (1 on a sheet, whose lines are paper mm already).
#[derive(Clone, Debug, PartialEq)]
pub struct Made {
    pub pattern: FillPattern,
    pub scale: f64,
    /// The tile's width and height, in the view's lengths.
    pub tile: P2,
}

impl Made {
    /// The pattern as a `target` one: a drafting pattern is the drawing on paper, at the view's scale.
    pub fn as_target(&self, target: Target) -> FillPattern {
        let k = match target {
            Target::Model => 1.0,
            Target::Drafting => 1.0 / self.scale,
        };
        FillPattern { target, ..self.pattern.scaled(k) }
    }
}

/// The pattern detail lines `ids`, all of one view, make of the tile between corners `a` and `b` (in the view's terms).
pub fn from_lines(m: &Model, ids: &[Id], a: P2, b: P2) -> Result<Made, String> {
    let mut view: Option<Guid> = None;
    let mut segments = Vec::new();
    let mut points = 0usize;
    for &id in ids {
        let Some(e) = m.elements.get(id) else { return Err(format!("no element {id}")) };
        let Element::DetailLine(l) = e else {
            return Err(format!("{} {id} is not a detail line; a pattern is made of detail lines", e.category()));
        };
        if view.is_some_and(|v| v != l.view) {
            return Err("the lines are in different views; a pattern's lines are all in one".into());
        }
        view = Some(l.view);
        let pts = l.drawn();
        points = points.saturating_add(pts.len());
        if points > MAX_POINTS {
            return Err(format!("the lines have more than {MAX_POINTS} points between them"));
        }
        segments.extend(pts.windows(2).map(|w| [w[0], w[1]]));
    }
    let Some(view) = view else { return Err("pick the detail lines the pattern is made of".into()) };
    let scale = match m.elements.get(view) {
        Some(Element::Sheet(_)) => 1.0,
        _ => crate::graphics::scale_of(m, view),
    };
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let tile = P2((a.0 - b.0).abs(), (a.1 - b.1).abs());
    Ok(Made { pattern: from_segments(&segments, a, b)?, scale, tile })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn a_line_across_the_tile_is_a_whole_family_and_its_copy_on_the_far_edge_is_left_out() {
        let tile = (P2(0.0, 0.0), P2(300.0, 200.0));
        let lines = [[P2(0.0, 0.0), P2(300.0, 0.0)], [P2(0.0, 200.0), P2(300.0, 200.0)]];
        let p = from_segments(&lines, tile.0, tile.1).unwrap();
        assert_eq!(p.target, Target::Model);
        assert_eq!(p.lines, vec![HatchLine::simple(0.0, 200.0)]);
    }

    #[test]
    fn a_short_line_is_a_dash_and_a_space_repeating_with_the_tile() {
        // A brick of 200 x 100, its bed joint along the bottom and its head joint at the left: the joint's lines dash.
        let lines = [[P2(1000.0, 500.0), P2(1200.0, 500.0)], [P2(1000.0, 500.0), P2(1000.0, 550.0)]];
        let p = from_segments(&lines, P2(1200.0, 600.0), P2(1000.0, 500.0)).unwrap();
        let [bed, head] = p.lines.as_slice() else { panic!("two families: {p:?}") };
        assert_eq!(*bed, HatchLine::simple(0.0, 100.0), "whole along");
        assert!(close(head.angle, 90.0) && close(head.spacing, 200.0), "{head:?}");
        assert_eq!(head.dashes, vec![50.0, -50.0]);
        assert_eq!(head.origin, P2(0.0, 0.0));
    }

    #[test]
    fn what_hangs_out_of_the_tile_is_cut_off() {
        let lines = [[P2(-50.0, 30.0), P2(40.0, 30.0)]];
        let p = from_segments(&lines, P2(0.0, 0.0), P2(100.0, 100.0)).unwrap();
        let l = &p.lines[0];
        assert_eq!((l.origin, l.dashes.clone()), (P2(0.0, 30.0), vec![40.0, -60.0]));
        assert!(from_segments(&[[P2(200.0, 0.0), P2(300.0, 0.0)]], P2(0.0, 0.0), P2(100.0, 100.0)).is_err(), "none inside");
    }

    #[test]
    fn a_diagonal_repeats_through_the_tiles_corners_and_its_families_lie_the_tiles_lattice_apart() {
        // The diagonal of a 300 x 100 tile runs through the next tile's corner: one tile across for one up.
        let p = from_segments(&[[P2(0.0, 0.0), P2(300.0, 100.0)]], P2(0.0, 0.0), P2(300.0, 100.0)).unwrap();
        let l = &p.lines[0];
        let diag = 300.0_f64.hypot(100.0);
        assert!(close(l.angle, 100.0_f64.atan2(300.0).to_degrees()));
        assert!(close(l.spacing, 300.0 * 100.0 / diag), "the area of a tile over the length of a repeat");
        assert!(l.dashes.is_empty(), "the diagonal is whole");
        // Its lines' strokes over the tile and the next: each lies along a copy of the diagonal.
        let ring = vec![P2(0.0, 0.0), P2(600.0, 0.0), P2(600.0, 200.0), P2(0.0, 200.0)];
        let strokes = crate::fills::strokes(&p, &[ring], 1.0, (P2(0.0, 0.0), P2(600.0, 200.0)), 1000).unwrap();
        for [a, b] in strokes {
            for q in [a, b] {
                // Every point of a copy is (x, y) with x/300 - y/100 a whole number.
                let k = q.0 / 300.0 - q.1 / 100.0;
                assert!((k - k.round()).abs() < 1e-6, "{q:?} off the copies");
            }
        }
    }

    #[test]
    fn a_line_at_an_odd_angle_is_turned_to_the_nearest_that_repeats_and_reversed_lines_are_one() {
        let p = from_segments(&[[P2(0.0, 0.0), P2(100.0, 50.3)]], P2(0.0, 0.0), P2(100.0, 100.0)).unwrap();
        // Two across for one up, the nearest slope with small steps.
        assert!(close(p.lines[0].angle, 0.5_f64.atan().to_degrees()), "{:?}", p.lines[0]);
        let once = from_segments(&[[P2(0.0, 10.0), P2(50.0, 10.0)], [P2(50.0, 10.0), P2(0.0, 10.0)]], P2(0.0, 0.0), P2(100.0, 100.0));
        assert_eq!(once.unwrap().lines.len(), 1);
    }

    #[test]
    fn euclid_finds_the_lattices_other_vector() {
        for (a, b) in [(1, 0), (0, 1), (3, 2), (-5, 3), (7, 48)] {
            let (g, x, y) = euclid(a, b);
            assert_eq!((g, a * x + b * y), (1, 1), "{a} {b}");
        }
    }

    #[test]
    fn a_tile_too_small_or_not_a_number_is_refused() {
        let l = [[P2(0.0, 0.0), P2(1.0, 0.0)]];
        assert!(from_segments(&l, P2(0.0, 0.0), P2(0.0, 10.0)).is_err());
        assert!(from_segments(&l, P2(f64::NAN, 0.0), P2(10.0, 10.0)).is_err());
        assert!(from_segments(&l, P2(0.0, 0.0), P2(1e9, 10.0)).is_err());
    }
}
