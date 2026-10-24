//! Revit's Draw panel: the shapes a line tool draws, and what the points clicked make of each. Detail lines, sketches
//! and walls draw with the same shapes (walls only those of lines and arcs), so what a click does is here, and a front
//! end only gathers the points.
//!
//! Each [`Shape`] takes so many points ([`Shape::clicks`]); a spline and a polyline take any number, finished by the
//! tool (Enter, in the desktop). [`make`] turns the points into the curves they draw, and the same with the cursor as
//! the last point is what a front end previews. Revit's Line, Rectangle, Inscribed and Circumscribed Polygon, Circle,
//! Start-End-Radius Arc, Center-ends Arc, Tangent End Arc, Spline, Ellipse and Partial Ellipse are all here; so are a
//! spline through points and a polyline, which Revit's detail lines don't have. Fillet Arc rounds two lines already
//! drawn ([`crate::curves::fillet`]), and Pick Lines copies one ([`pick`]).

use serde::{Deserialize, Serialize};

use crate::curves::{self, Curve};
use crate::geom::P2;
use crate::model::{Element, Id, Model, TOL};

/// What the next clicks draw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// A line between two points; chained, the next starts where it ended.
    #[default]
    Line,
    /// Four lines between two opposite corners.
    Rectangle,
    /// A regular polygon about its centre, a corner at the second point.
    Inscribed,
    /// A regular polygon about its centre, a side's middle at the second point.
    Circumscribed,
    /// A circle about its centre, through the second point.
    Circle,
    /// An arc from its start to its end through a third point: Revit's Start-End-Radius Arc.
    Arc,
    /// An arc about its centre from its start round to its end: Revit's Center-ends Arc.
    CentreArc,
    /// An arc from the end of the line before, leaving it the way that line arrived: Revit's Tangent End Arc.
    TangentArc,
    /// An arc rounding the corner of two lines already drawn: Revit's Fillet Arc.
    Fillet,
    /// A spline by its control points.
    Spline,
    /// A spline through the points.
    FitSpline,
    /// An ellipse about its centre, its first axis to the second point, its second as far as the third lies from it.
    Ellipse,
    /// Half an ellipse across its first axis, from the first point to the second, its other radius to the third.
    PartialEllipse,
    /// One line through all the points.
    Polyline,
    /// A copy of a line, an edge or a wall's location line, picked.
    PickLines,
}

/// The Options Bar's settings a shape draws with.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Options {
    /// A polygon's sides.
    pub sides: usize,
    /// Revit's Radius: a circle, a polygon or a fillet this big, from one click less.
    pub radius: Option<f64>,
    /// Revit's Offset: drawn this far to the left of where it's clicked (negative, to the right).
    pub offset: f64,
}

impl Default for Options {
    fn default() -> Options {
        Options { sides: 6, radius: None, offset: 0.0 }
    }
}

/// The most sides a polygon has.
pub const MAX_SIDES: usize = 360;

impl Shape {
    /// Every shape, in the Draw panel's order.
    pub const ALL: [Shape; 15] = [
        Shape::Line,
        Shape::Rectangle,
        Shape::Inscribed,
        Shape::Circumscribed,
        Shape::Circle,
        Shape::Arc,
        Shape::CentreArc,
        Shape::TangentArc,
        Shape::Fillet,
        Shape::Spline,
        Shape::FitSpline,
        Shape::Ellipse,
        Shape::PartialEllipse,
        Shape::Polyline,
        Shape::PickLines,
    ];

    /// What it's called, as the Draw panel names it.
    pub fn label(self) -> &'static str {
        match self {
            Shape::Line => "Line",
            Shape::Rectangle => "Rectangle",
            Shape::Inscribed => "Inscribed Polygon",
            Shape::Circumscribed => "Circumscribed Polygon",
            Shape::Circle => "Circle",
            Shape::Arc => "Start-End-Radius Arc",
            Shape::CentreArc => "Center-ends Arc",
            Shape::TangentArc => "Tangent End Arc",
            Shape::Fillet => "Fillet Arc",
            Shape::Spline => "Spline",
            Shape::FitSpline => "Spline Through Points",
            Shape::Ellipse => "Ellipse",
            Shape::PartialEllipse => "Partial Ellipse",
            Shape::Polyline => "Polyline",
            Shape::PickLines => "Pick Lines",
        }
    }

    /// Whether walls draw it: walls are lines and arcs.
    pub fn for_walls(self) -> bool {
        !matches!(self, Shape::Spline | Shape::FitSpline | Shape::Ellipse | Shape::PartialEllipse | Shape::Polyline)
    }

    /// Whether it takes any number of points, until the tool finishes it.
    pub fn open_ended(self) -> bool {
        matches!(self, Shape::Spline | Shape::FitSpline | Shape::Polyline)
    }

    /// Whether, chained, the next starts where it ended.
    pub fn chains(self) -> bool {
        matches!(self, Shape::Line | Shape::Arc | Shape::TangentArc)
    }

    /// How many points make it; for one that takes any number, the fewest. Fillet Arc and Pick Lines pick lines
    /// instead (none).
    pub fn clicks(self, o: &Options) -> usize {
        let sized = usize::from(!o.radius.is_some_and(|r| r > TOL));
        match self {
            Shape::Line | Shape::Rectangle | Shape::TangentArc | Shape::Spline | Shape::FitSpline | Shape::Polyline => 2,
            Shape::Inscribed | Shape::Circumscribed | Shape::Circle => 1 + sized,
            Shape::Arc | Shape::CentreArc | Shape::Ellipse | Shape::PartialEllipse => 3,
            Shape::Fillet | Shape::PickLines => 0,
        }
    }
}

/// What the points make as `shape`, once there are enough of them (for a shape of any number, as many as there are):
/// the curves, offset as the options say; None while there aren't, or when they make nothing (on each other, in a
/// line). `tangent` is the way the line before ended, which a tangent arc leaves along.
pub fn make(shape: Shape, points: &[P2], o: &Options, tangent: Option<P2>) -> Option<Vec<Curve>> {
    let n = shape.clicks(o);
    if points.len() < n || (!shape.open_ended() && points.len() > n) || n == 0 {
        return None;
    }
    if points.iter().any(|p| !p.is_finite()) {
        return None;
    }
    let apart = |a: P2, b: P2| a.dist(b) > TOL;
    let radius = o.radius.filter(|r| r.is_finite() && *r > TOL);
    let made: Vec<Curve> = match (shape, points) {
        (Shape::Line, &[a, b]) if apart(a, b) => vec![Curve::line(a, b)],
        (Shape::Rectangle, &[a, b]) if (a.0 - b.0).abs() > TOL && (a.1 - b.1).abs() > TOL => {
            let c = [a, P2(b.0, a.1), b, P2(a.0, b.1)];
            (0..4).map(|k| Curve::line(c[k], c[(k + 1) % 4])).collect()
        }
        (Shape::Inscribed | Shape::Circumscribed, pts) => {
            let centre = *pts.first()?;
            let at = match (radius, pts.get(1)) {
                (Some(r), None) => centre + P2(r, 0.0),
                (_, Some(p)) => *p,
                _ => return None,
            };
            let sides = o.sides.clamp(3, MAX_SIDES);
            curves::explode(&curves::polygon(centre, at, sides, shape == Shape::Inscribed)?)
        }
        (Shape::Circle, pts) => {
            let centre = *pts.first()?;
            let r = match (radius, pts.get(1)) {
                (Some(r), None) => r,
                (_, Some(p)) => centre.dist(*p),
                _ => return None,
            };
            if r <= TOL {
                return None;
            }
            vec![Curve::Circle { centre, radius: r }]
        }
        (Shape::Arc, &[a, b, t]) if apart(a, b) => vec![Curve::arc_through(a, b, t)],
        (Shape::CentreArc, &[c, a, b]) if apart(c, a) && apart(c, b) => {
            // The shorter way round from its start to its end.
            let ccw = (a - c).cross(b - c) >= 0.0;
            vec![Curve::arc_centred(c, a, b, ccw)]
        }
        (Shape::TangentArc, &[a, b]) if apart(a, b) => vec![Curve::arc_tangent(a, tangent?, b)],
        (Shape::Spline, pts) if distinct(pts).len() >= 2 => vec![Curve::Spline { control: distinct(pts), knots: None }],
        (Shape::FitSpline, pts) if distinct(pts).len() >= 2 => vec![Curve::Fit { points: distinct(pts), tangents: [None, None] }],
        (Shape::Polyline, pts) => {
            let mut pts = distinct(pts);
            // Back on its first point, it closes.
            let closed = pts.len() > 3 && pts.first().zip(pts.last()).is_some_and(|(a, b)| !apart(*a, *b));
            if closed {
                pts.pop();
            }
            if pts.len() < 2 {
                return None;
            }
            vec![Curve::Poly { points: pts, arcs: Vec::new(), closed }]
        }
        (Shape::Ellipse, &[c, a, b]) if apart(c, a) => {
            let u = (a - c).unit();
            let second = (b - c).cross(u).abs();
            if second <= TOL {
                return None;
            }
            vec![Curve::Ellipse { centre: c, radii: [c.dist(a), second], angle: u.1.atan2(u.0).to_degrees(), span: None }]
        }
        (Shape::PartialEllipse, &[a, b, t]) if apart(a, b) => {
            // Across from `b` to `a` round the side `t` is on: from parameter 0 (at `b`) to 180 (at `a`) when it's to the
            // left of `a` to `b`, else the other half.
            let (c, u) = ((a + b) * 0.5, (b - a).unit());
            let side = u.cross(t - c);
            if side.abs() <= TOL {
                return None;
            }
            let span = if side > 0.0 { [0.0, 180.0] } else { [180.0, 360.0] };
            vec![Curve::Ellipse { centre: c, radii: [a.dist(b) / 2.0, side.abs()], angle: u.1.atan2(u.0).to_degrees(), span: Some(span) }]
        }
        _ => return None,
    };
    let made: Vec<Curve> = if o.offset != 0.0 && o.offset.is_finite() {
        made.iter().map(|c| offset(c, o.offset)).collect::<Option<Vec<Curve>>>()?
    } else {
        made
    };
    made.iter().all(|c| c.check("").is_ok()).then_some(made)
}

/// What a front end shows while the points are clicked and the cursor is at `cursor`: what they'd make with the cursor
/// as the next point, or the lines between the points so far and the cursor.
pub fn preview(shape: Shape, points: &[P2], cursor: P2, o: &Options, tangent: Option<P2>) -> Vec<Curve> {
    let mut with: Vec<P2> = points.to_vec();
    with.push(cursor);
    if let Some(made) = make(shape, &with, o, tangent) {
        return made;
    }
    with.windows(2).filter(|w| w[0].dist(w[1]) > TOL).map(|w| Curve::line(w[0], w[1])).collect()
}

/// A copy of `c`, `by` mm to its left (to its right when negative): Revit's Offset. Lines and arcs, polylines of them and
/// circles; None for an ellipse or a spline, or when it would turn inside out.
pub fn offset(c: &Curve, by: f64) -> Option<Curve> {
    if by == 0.0 {
        return Some(c.clone());
    }
    match c {
        Curve::Circle { centre, radius } => {
            // A circle runs counter-clockwise: its left is inside.
            let r = radius - by;
            (r > TOL).then_some(Curve::Circle { centre: *centre, radius: r })
        }
        Curve::Poly { points, arcs, closed } => {
            let segs = c.segments();
            let n = points.len();
            if segs.is_empty() {
                return None;
            }
            // Each segment's way off at its ends, to its left.
            let normals: Vec<(P2, P2)> = segs.iter().map(|s| (s.tangent(0.0).perp(), s.tangent(s.length()).perp())).collect();
            let corner = |i: usize| -> P2 {
                let p = points[i];
                let before =
                    if i == 0 { if *closed { normals.last().map(|x| x.1) } else { None } } else { normals.get(i - 1).map(|x| x.1) };
                let after = if i < segs.len() { normals.get(i).map(|x| x.0) } else { None };
                match (before, after) {
                    (Some(a), Some(b)) => {
                        // Where the two offset ways meet: the mitre.
                        let m = (a + b).unit();
                        let cos = m.dot(a);
                        if cos.abs() < 0.1 { p + a * by } else { p + m * (by / cos) }
                    }
                    (Some(a), None) | (None, Some(a)) => p + a * by,
                    (None, None) => p,
                }
            };
            let pts: Vec<P2> = (0..n).map(corner).collect();
            let out = Curve::Poly { points: pts, arcs: arcs.clone(), closed: *closed };
            out.check("").is_ok().then_some(out)
        }
        Curve::Ellipse { .. } | Curve::Spline { .. } | Curve::Fit { .. } => None,
    }
}

/// A curve picked to copy, `by` mm to the side it was picked on (`at`), as Pick Lines with an Offset does.
pub fn pick(c: &Curve, at: P2, by: f64) -> Option<Curve> {
    if by == 0.0 {
        return Some(c.clone());
    }
    let (t, _) = c.nearest(at);
    let left = c.tangent(t).cross(at - c.at(t)) >= 0.0;
    offset(c, if left { by.abs() } else { -by.abs() })
}

/// The line an element is drawn along, what Pick Lines copies: a detail line's curve, a wall's location line (straight or
/// an arc), a grid's line, a beam's or a pipe's axis.
pub fn element_curve(m: &Model, id: Id) -> Option<Curve> {
    match m.elements.get(id)? {
        Element::DetailLine(l) => Some(l.curve.clone()),
        Element::Wall(w) => Some(Curve::arc(w.start, w.end, w.arc)),
        Element::Grid(g) => Some(Curve::line(g.start, g.end)),
        crate::model::run_arm!(r) => Some(Curve::line(r.start, r.end)),
        _ => None,
    }
}

/// The walls curves make, each a segment of a location line: from, to, and how far it turns (degrees counter-clockwise,
/// as a wall's arc). A circle is two half circles; an ellipse or a spline makes none.
pub fn wall_segments(curves: &[Curve]) -> Vec<(P2, P2, f64)> {
    let mut out = Vec::new();
    for c in curves {
        match c {
            Curve::Poly { arcs, .. } => {
                for (i, s) in c.segments().iter().enumerate() {
                    out.push((s.start, s.end, arcs.get(i).copied().unwrap_or(0.0)));
                }
            }
            Curve::Circle { centre, radius } => {
                let (a, b) = (*centre + P2(*radius, 0.0), *centre - P2(*radius, 0.0));
                out.push((a, b, 180.0));
                out.push((b, a, 180.0));
            }
            _ => {}
        }
    }
    out
}

/// Two straight lines rounded where they meet (or would, extended) by an arc of `radius`: the first line cut back to the
/// arc, the arc, the second cut back to it, each line running the way it ran. None unless both are straight, they meet at
/// an angle and are long enough for it: Fillet Arc.
pub fn fillet_lines(a: &Curve, b: &Curve, radius: f64) -> Option<(Curve, Curve, Curve)> {
    let (Curve::Poly { points: pa, closed: false, .. }, Curve::Poly { points: pb, closed: false, .. }) = (a, b) else { return None };
    if pa.len() != 2 || pb.len() != 2 || a.kind() != "line" || b.kind() != "line" {
        return None;
    }
    let corner = crate::geom::line_intersection(pa[0], pa[1], pb[0], pb[1])?;
    // Each keeps its end away from the corner.
    let far = |p: &[P2]| if p[0].dist(corner) > p[1].dist(corner) { (p[0], true) } else { (p[1], false) };
    let ((fa, a_starts), (fb, b_starts)) = (far(pa), far(pb));
    let (p, q, arc) = curves::fillet(fa, corner, fb, radius)?;
    let la = if a_starts { Curve::line(fa, p) } else { Curve::line(p, fa) };
    let lb = if b_starts { Curve::line(fb, q) } else { Curve::line(q, fb) };
    (la.check("").is_ok() && lb.check("").is_ok()).then_some((la, arc, lb))
}

/// What a line or an arc runs along, out past its ends: a whole line, or a whole circle.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Along {
    Line(P2, P2),
    Circle(P2, f64),
}

fn along(c: &Curve) -> Option<Along> {
    match c {
        Curve::Poly { points, closed: false, .. } if points.len() == 2 => {
            let segs = c.segments();
            let s = segs.first()?;
            match (s.centre(), s.radius()) {
                (Some(o), Some(r)) => Some(Along::Circle(o, r)),
                _ => Some(Along::Line(s.start, s.end)),
            }
        }
        _ => None,
    }
}

/// Where two lines or arcs meet, run on past their ends: the crossing nearest `near`, or none (parallel, apart).
pub fn meet(a: &Curve, b: &Curve, near: P2) -> Option<P2> {
    let (ga, gb) = (along(a)?, along(b)?);
    let line_circle = |p: P2, q: P2, o: P2, r: f64| -> Vec<P2> {
        let u = (q - p).unit();
        let f = p - o;
        let (bb, cc) = (f.dot(u), f.dot(f) - r * r);
        let disc = bb * bb - cc;
        if disc < 0.0 {
            return Vec::new();
        }
        let sq = disc.sqrt();
        vec![p + u * (-bb - sq), p + u * (-bb + sq)]
    };
    let found: Vec<P2> = match (ga, gb) {
        (Along::Line(p, q), Along::Line(r, t)) => crate::geom::line_intersection(p, q, r, t).into_iter().collect(),
        (Along::Line(p, q), Along::Circle(o, r)) | (Along::Circle(o, r), Along::Line(p, q)) => line_circle(p, q, o, r),
        (Along::Circle(o1, r1), Along::Circle(o2, r2)) => {
            let d = o1.dist(o2);
            if d < 1e-9 || d > r1 + r2 || d < (r1 - r2).abs() {
                Vec::new()
            } else {
                let x = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
                let h = (r1 * r1 - x * x).max(0.0).sqrt();
                let u = (o2 - o1).unit();
                let m = o1 + u * x;
                vec![m + u.perp() * h, m - u.perp() * h]
            }
        }
    };
    found.into_iter().filter(|p| p.is_finite()).min_by(|x, y| x.dist(near).total_cmp(&y.dist(near)))
}

/// A line or an arc trimmed or extended to `x` (on it, or on its line or circle run on): the end on the far side of `keep`
/// from `x` stays, the other goes to `x`. None when that leaves nothing.
pub fn trim_to(c: &Curve, keep: P2, x: P2) -> Option<Curve> {
    let s = c.segments().into_iter().next()?;
    let made = match along(c)? {
        Along::Line(p, q) => {
            let u = (q - p).unit();
            let t = |r: P2| (r - p).dot(u);
            if t(keep) < t(x) { Curve::line(p, x) } else { Curve::line(x, q) }
        }
        Along::Circle(o, _) => {
            // Angles round the centre the way it turns, from its start.
            let sweep = s.length() / s.radius()?;
            let turn = match c {
                Curve::Poly { arcs, .. } => arcs.first().map_or(1.0, |a| a.signum()),
                _ => 1.0,
            };
            let angle = |r: P2| (r.1 - o.1).atan2(r.0 - o.0);
            let from = |r: P2| (turn * (angle(r) - angle(s.start))).rem_euclid(std::f64::consts::TAU);
            let (tk, tx) = (from(keep).min(sweep), from(x));
            // Ahead of the part kept, its end goes there; behind it, its start.
            let ahead = (tx - tk + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
            if ahead > 0.0 {
                Curve::arc(s.start, x, turn * (tk + ahead).to_degrees())
            } else {
                Curve::arc(x, s.end, turn * (sweep - (tk + ahead)).to_degrees())
            }
        }
    };
    made.check("").is_ok().then_some(made)
}

/// Two lines or arcs trimmed or extended to where they meet, each keeping the part `keep` lies on: Trim/Extend to Corner.
pub fn corner(a: &Curve, keep_a: P2, b: &Curve, keep_b: P2) -> Option<(Curve, Curve)> {
    let x = meet(a, b, (keep_a + keep_b) * 0.5)?;
    Some((trim_to(a, keep_a, x)?, trim_to(b, keep_b, x)?))
}

/// The points, none on the one before.
fn distinct(points: &[P2]) -> Vec<P2> {
    let mut out: Vec<P2> = Vec::with_capacity(points.len());
    for p in points {
        if out.last().is_none_or(|q| q.dist(*p) > TOL) {
            out.push(*p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(shape: Shape, pts: &[(f64, f64)]) -> Vec<Curve> {
        let pts: Vec<P2> = pts.iter().map(|&(x, y)| P2(x, y)).collect();
        make(shape, &pts, &Options::default(), Some(P2(1.0, 0.0))).unwrap_or_else(|| panic!("{shape:?} makes nothing"))
    }

    #[test]
    fn each_shape_takes_its_clicks_and_makes_what_revit_draws() {
        let o = Options::default();
        assert_eq!(one(Shape::Line, &[(0.0, 0.0), (1000.0, 0.0)]), vec![Curve::line(P2(0.0, 0.0), P2(1000.0, 0.0))]);
        assert_eq!(one(Shape::Rectangle, &[(0.0, 0.0), (2000.0, 1000.0)]).len(), 4);
        let hex = one(Shape::Inscribed, &[(0.0, 0.0), (1000.0, 0.0)]);
        assert_eq!(hex.len(), 6, "six sides unless told");
        assert!(hex.iter().all(|c| (c.length() - 1000.0).abs() < 1e-6));
        let sq = make(Shape::Circumscribed, &[P2(0.0, 0.0), P2(1000.0, 0.0)], &Options { sides: 4, ..o }, None).unwrap_or_default();
        assert!(sq.iter().all(|c| (c.length() - 2000.0).abs() < 1e-6), "touching the circle: {sq:?}");
        assert_eq!(one(Shape::Circle, &[(0.0, 0.0), (0.0, 500.0)]), vec![Curve::Circle { centre: P2(0.0, 0.0), radius: 500.0 }]);
        let arc = &one(Shape::Arc, &[(0.0, 0.0), (2000.0, 0.0), (1000.0, 1000.0)])[0];
        assert!(arc.middle().dist(P2(1000.0, 1000.0)) < 1.0);
        let quarter = &one(Shape::CentreArc, &[(0.0, 0.0), (1000.0, 0.0), (0.0, 300.0)])[0];
        assert!(quarter.end().dist(P2(0.0, 1000.0)) < 1e-6 && (quarter.length() - std::f64::consts::FRAC_PI_2 * 1000.0).abs() < 1e-6);
        let t = &one(Shape::TangentArc, &[(0.0, 0.0), (1000.0, 1000.0)])[0];
        assert!(t.tangent(0.0).dist(P2(1.0, 0.0)) < 1e-9, "leaving as the line before arrived");
        assert!(make(Shape::TangentArc, &[P2(0.0, 0.0), P2(1000.0, 0.0)], &o, None).is_none(), "nothing to be tangent to");
        assert_eq!(one(Shape::Spline, &[(0.0, 0.0), (1000.0, 1000.0), (2000.0, 0.0)])[0].kind(), "spline");
        assert_eq!(one(Shape::FitSpline, &[(0.0, 0.0), (1000.0, 1000.0), (2000.0, 0.0)])[0].kind(), "fitted spline");
        let e = &one(Shape::Ellipse, &[(0.0, 0.0), (0.0, 2000.0), (700.0, 500.0)])[0];
        assert_eq!(e, &Curve::Ellipse { centre: P2(0.0, 0.0), radii: [2000.0, 700.0], angle: 90.0, span: None });
        let half = &one(Shape::PartialEllipse, &[(0.0, 0.0), (4000.0, 0.0), (2000.0, 1000.0)])[0];
        assert!(half.middle().dist(P2(2000.0, 1000.0)) < 1.0 && half.start().dist(P2(4000.0, 0.0)) < 1e-6, "{half:?}");
        let below = &one(Shape::PartialEllipse, &[(0.0, 0.0), (4000.0, 0.0), (2000.0, -1000.0)])[0];
        assert!(below.middle().dist(P2(2000.0, -1000.0)) < 1.0);
        let ring = &one(Shape::Polyline, &[(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0), (0.0, 0.0)])[0];
        assert!(ring.is_closed(), "back on its first point it closes");
        // Too few or too many points, or points on each other, make nothing.
        assert!(make(Shape::Arc, &[P2(0.0, 0.0), P2(1000.0, 0.0)], &o, None).is_none());
        assert!(make(Shape::Line, &[P2(0.0, 0.0), P2(0.5, 0.0)], &o, None).is_none());
        assert!(make(Shape::Ellipse, &[P2(0.0, 0.0), P2(1000.0, 0.0), P2(2000.0, 0.0)], &o, None).is_none(), "flat");
        assert!(make(Shape::Line, &[P2(f64::NAN, 0.0), P2(1.0, 0.0)], &o, None).is_none());
    }

    #[test]
    fn radius_takes_a_click_off_and_offset_draws_beside_the_clicks() {
        let o = Options { radius: Some(800.0), ..Options::default() };
        assert_eq!(Shape::Circle.clicks(&o), 1);
        assert_eq!(make(Shape::Circle, &[P2(10.0, 10.0)], &o, None), Some(vec![Curve::Circle { centre: P2(10.0, 10.0), radius: 800.0 }]));
        let off = Options { offset: 500.0, ..Options::default() };
        let l = make(Shape::Line, &[P2(0.0, 0.0), P2(1000.0, 0.0)], &off, None).unwrap_or_default();
        assert_eq!(l, vec![Curve::line(P2(0.0, 500.0), P2(1000.0, 500.0))], "to its left");
        let r = make(Shape::Rectangle, &[P2(0.0, 0.0), P2(2000.0, 1000.0)], &Options { offset: -100.0, ..Options::default() }, None)
            .unwrap_or_default();
        assert_eq!(r.len(), 4);
        assert!(make(Shape::Spline, &[P2(0.0, 0.0), P2(1.0e3, 1.0e3), P2(2.0e3, 0.0)], &off, None).is_none(), "a spline has no offset");
    }

    #[test]
    fn offset_keeps_a_polylines_corners_mitred_and_its_arcs_concentric() {
        let p = Curve::Poly { points: vec![P2(0.0, 0.0), P2(1000.0, 0.0), P2(1000.0, 1000.0)], arcs: vec![], closed: false };
        let o = offset(&p, 100.0).unwrap_or_else(|| panic!("offset"));
        assert_eq!(o, Curve::Poly { points: vec![P2(0.0, 100.0), P2(900.0, 100.0), P2(900.0, 1000.0)], arcs: vec![], closed: false });
        let arc = Curve::arc(P2(0.0, 0.0), P2(2000.0, 0.0), -180.0);
        let inside = offset(&arc, -200.0).unwrap_or_else(|| panic!("arc"));
        assert!(inside.points().iter().all(|q| (q.dist(P2(1000.0, 0.0)) - 800.0).abs() < 1e-6), "{inside:?}");
        assert!(offset(&Curve::Circle { centre: P2(0.0, 0.0), radius: 100.0 }, 200.0).is_none(), "inside out");
        let picked = pick(&Curve::line(P2(0.0, 0.0), P2(1000.0, 0.0)), P2(500.0, -50.0), 300.0).unwrap_or_else(|| panic!("pick"));
        assert_eq!(picked, Curve::line(P2(0.0, -300.0), P2(1000.0, -300.0)), "to the side it was picked on");
    }

    #[test]
    fn the_preview_is_the_shape_the_cursor_would_finish_or_the_lines_so_far() {
        let o = Options::default();
        let arc = preview(Shape::Arc, &[P2(0.0, 0.0), P2(2000.0, 0.0)], P2(1000.0, 1000.0), &o, None);
        assert_eq!(arc.len(), 1);
        assert_eq!(arc[0].kind(), "arc");
        let rubber = preview(Shape::Arc, &[P2(0.0, 0.0)], P2(1000.0, 0.0), &o, None);
        assert_eq!(rubber, vec![Curve::line(P2(0.0, 0.0), P2(1000.0, 0.0))]);
        let spline = preview(Shape::Spline, &[P2(0.0, 0.0), P2(1000.0, 1000.0)], P2(2000.0, 0.0), &o, None);
        assert_eq!(spline[0].kind(), "spline");
        assert!(Shape::ALL.iter().all(|s| !s.label().is_empty()));
        assert_eq!(Shape::ALL.iter().filter(|s| s.for_walls()).count(), 10);
    }

    #[test]
    fn trim_and_extend_bring_lines_and_arcs_to_where_they_meet() {
        // Two lines short of their corner: both extended to it.
        let (a, b) = (Curve::line(P2(0.0, 0.0), P2(800.0, 0.0)), Curve::line(P2(1000.0, 200.0), P2(1000.0, 1000.0)));
        let (ta, tb) = corner(&a, P2(100.0, 0.0), &b, P2(1000.0, 900.0)).unwrap_or_else(|| panic!("corner"));
        assert_eq!((ta, tb), (Curve::line(P2(0.0, 0.0), P2(1000.0, 0.0)), Curve::line(P2(1000.0, 0.0), P2(1000.0, 1000.0))));
        // Two lines crossing: each trimmed back to the crossing, keeping the part picked.
        let (a, b) = (Curve::line(P2(0.0, 0.0), P2(2000.0, 0.0)), Curve::line(P2(1000.0, -500.0), P2(1000.0, 1000.0)));
        let (ta, tb) = corner(&a, P2(1500.0, 0.0), &b, P2(1000.0, 500.0)).unwrap_or_else(|| panic!("corner"));
        assert_eq!((ta, tb), (Curve::line(P2(1000.0, 0.0), P2(2000.0, 0.0)), Curve::line(P2(1000.0, 0.0), P2(1000.0, 1000.0))));
        // An arc extended along its circle to a line.
        let arc = Curve::arc(P2(1000.0, 0.0), P2(0.0, 1000.0), 90.0);
        let line = Curve::line(P2(-2000.0, 0.0), P2(-500.0, 0.0));
        let x = meet(&arc, &line, P2(-1000.0, 0.0)).unwrap_or_else(|| panic!("meet"));
        assert!(x.dist(P2(-1000.0, 0.0)) < 1e-6, "{x:?}");
        let longer = trim_to(&arc, P2(700.0, 700.0), x).unwrap_or_else(|| panic!("trim"));
        assert!(longer.end().dist(P2(-1000.0, 0.0)) < 1e-6 && longer.start().dist(P2(1000.0, 0.0)) < 1e-6);
        assert!((longer.length() - std::f64::consts::PI * 1000.0).abs() < 1e-6, "a half circle now");
        assert!(
            meet(&Curve::line(P2(0.0, 0.0), P2(1.0e3, 0.0)), &Curve::line(P2(0.0, 5.0), P2(1.0e3, 5.0)), P2(0.0, 0.0)).is_none(),
            "parallel"
        );
    }

    #[test]
    fn fillet_rounds_two_lines_where_they_would_meet_and_walls_come_from_lines_arcs_and_circles() {
        let a = Curve::line(P2(-3000.0, 0.0), P2(-100.0, 0.0));
        let b = Curve::line(P2(0.0, 3000.0), P2(0.0, 200.0));
        let (la, arc, lb) = fillet_lines(&a, &b, 1000.0).unwrap_or_else(|| panic!("fillet"));
        assert!(la.start().dist(P2(-3000.0, 0.0)) < 1e-6 && la.end().dist(P2(-1000.0, 0.0)) < 1e-6, "cut back, running as it ran");
        assert!(lb.start().dist(P2(0.0, 3000.0)) < 1e-6 && lb.end().dist(P2(0.0, 1000.0)) < 1e-6);
        assert!(arc.start().dist(P2(-1000.0, 0.0)) < 1e-6 && arc.end().dist(P2(0.0, 1000.0)) < 1e-6);
        assert!(fillet_lines(&a, &Curve::line(P2(0.0, 10.0), P2(500.0, 10.0)), 100.0).is_none(), "parallel");
        let walls = wall_segments(&[Curve::Circle { centre: P2(0.0, 0.0), radius: 1000.0 }, a.clone()]);
        assert_eq!(
            walls,
            vec![
                (P2(1000.0, 0.0), P2(-1000.0, 0.0), 180.0),
                (P2(-1000.0, 0.0), P2(1000.0, 0.0), 180.0),
                (P2(-3000.0, 0.0), P2(-100.0, 0.0), 0.0)
            ]
        );
        assert!(wall_segments(&[Curve::Spline { control: vec![P2(0.0, 0.0), P2(1.0e3, 0.0)], knots: None }]).is_empty());
    }
}
