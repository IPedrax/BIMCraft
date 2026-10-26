//! Curves: the shapes a detail line and a sketch line take, as Revit's draw tools make them and more.
//!
//! A [`Curve`] is a polyline (straight segments and arcs, open or closed; a single line or arc is one of two points), a
//! circle, an ellipse (whole or part of one), a spline by its control points (a cubic B-spline with its knots), or a
//! spline fitted through points (a cubic through each in turn). Each is drawn as the points [`Curve::points`] gives,
//! close enough to it that no chord strays more than [`FLAT`] from it, is found under a cursor ([`Curve::nearest`]), and
//! can be split anywhere along it ([`Curve::split`]).
//!
//! Splitting is exact for all but a fitted spline: a polyline's segment and an arc divide in two, an ellipse's span does,
//! and a control-point spline is split by inserting the knot there until it is as many times over as the degree
//! (Boehm's knot insertion; Piegl and Tiller, *The NURBS Book*, 5.2), which leaves the two halves' control points the
//! original curve's, exactly. Revit refuses to split a spline at all. A fitted spline can't be split exactly into two
//! that are still fitted through points, because a cubic through fewer points bends differently: QCAD answers by turning
//! the halves into control-point splines. Here each half stays a fitted spline: through the original's points on its
//! side and the split point, leaving the cut the way the original runs there (its end tangent), and with points of the
//! original added where it strays most, until no part of it lies further than [`FIT_TOL`] from the original.
//!
//! In a file and in a command's params a curve is flat, the fields of the shape it is: `start` and `end` (with `arc`,
//! degrees counter-clockwise as a wall's, or `through`, a point the arc passes), `points` (with `arcs` and `closed`),
//! `centre` and `radius`, `centre` and `radii` (with `angle` and `span`), `control` (with `knots`), or `fit` (with
//! `start_tangent` and `end_tangent`).

use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};

use crate::geom::{self, P2};
use crate::model::{MAX_COORD, MAX_POINTS, TOL};
use crate::walls::{Axis, sweep_through};

/// How far a chord drawn for a curve may stray from it, mm.
pub const FLAT: f64 = 0.25;

/// How far a fitted spline split in two may stray from the original, mm.
pub const FIT_TOL: f64 = 0.05;

/// The most points a curve is drawn with.
pub const MAX_DRAWN: usize = 8192;

/// The highest degree a control-point spline takes.
pub const MAX_DEGREE: usize = 5;

/// A line, an arc, a polyline, a circle, an ellipse or a spline.
#[derive(Clone, Debug, PartialEq)]
pub enum Curve {
    /// Through `points` in turn, each segment straight or, by `arcs` (one for each, or none at all), turning so many
    /// degrees counter-clockwise as a wall's arc does; `closed`, the last back to the first.
    Poly { points: Vec<P2>, arcs: Vec<f64>, closed: bool },
    /// A whole circle; it starts and ends due east of its centre.
    Circle { centre: P2, radius: f64 },
    /// An ellipse about `centre`, `radii` along its axes, the first turned `angle` degrees counter-clockwise from east;
    /// with `span`, only the part from one parameter angle round counter-clockwise to the other (degrees).
    Ellipse { centre: P2, radii: [f64; 2], angle: f64, span: Option<[f64; 2]> },
    /// A spline by its control points: a B-spline of degree three (fewer for fewer points), clamped to its first and last
    /// points; its knots uniform unless given.
    Spline { control: Vec<P2>, knots: Option<Vec<f64>> },
    /// A spline through `points`: a cubic through each in turn, by their distance apart; each end leaving the way a
    /// tangent says, or free (straight there).
    Fit { points: Vec<P2>, tangents: [Option<P2>; 2] },
}

impl Curve {
    /// A straight line.
    pub fn line(a: P2, b: P2) -> Curve {
        Curve::Poly { points: vec![a, b], arcs: Vec::new(), closed: false }
    }

    /// An arc from `a` to `b` turning `sweep` degrees counter-clockwise.
    pub fn arc(a: P2, b: P2, sweep: f64) -> Curve {
        let arcs = if sweep == 0.0 { Vec::new() } else { vec![sweep] };
        Curve::Poly { points: vec![a, b], arcs, closed: false }
    }

    /// An arc from `a` to `b` through `t`; straight when `t` is no further than [`TOL`] off the line between them.
    pub fn arc_through(a: P2, b: P2, t: P2) -> Curve {
        if geom::dist_to_segment(t, a, b) <= TOL {
            return Curve::line(a, b);
        }
        Curve::arc(a, b, sweep_through(a, t, b))
    }

    /// An arc about `centre` from `a` round to where `b` lies from it (on the radius `a` makes), counter-clockwise or
    /// not: Revit's Center-ends Arc.
    pub fn arc_centred(centre: P2, a: P2, b: P2, ccw: bool) -> Curve {
        let r = centre.dist(a);
        let (aa, ab) = ((a.1 - centre.1).atan2(a.0 - centre.0), (b.1 - centre.1).atan2(b.0 - centre.0));
        let mut sweep = (ab - aa).rem_euclid(TAU);
        if !ccw {
            sweep -= TAU;
        }
        let end = centre + P2(ab.cos(), ab.sin()) * r;
        Curve::arc(a, end, sweep.to_degrees())
    }

    /// An arc from `a`, leaving it along `dir`, to `b`: Revit's Tangent End Arc.
    pub fn arc_tangent(a: P2, dir: P2, b: P2) -> Curve {
        // The chord turns from the tangent by half the sweep.
        let (d, c) = (dir.unit(), b - a);
        let half = d.cross(c).atan2(d.dot(c));
        Curve::arc(a, b, (2.0 * half).to_degrees())
    }

    /// Whether it ends where it starts: a closed polyline, a circle, a whole ellipse.
    pub fn is_closed(&self) -> bool {
        match self {
            Curve::Poly { closed, .. } => *closed,
            Curve::Circle { .. } => true,
            Curve::Ellipse { span, .. } => span.is_none(),
            Curve::Spline { .. } | Curve::Fit { .. } => false,
        }
    }

    /// Whether it is a line or an arc, or a polyline of them: what joins into one.
    pub fn is_poly(&self) -> bool {
        matches!(self, Curve::Poly { .. })
    }

    /// What it's called: line, arc, polyline, circle, ellipse, partial ellipse, spline, fitted spline.
    pub fn kind(&self) -> &'static str {
        match self {
            Curve::Poly { points, arcs, closed: false } if points.len() == 2 => {
                if arcs.first().is_some_and(|a| *a != 0.0) {
                    "arc"
                } else {
                    "line"
                }
            }
            Curve::Poly { .. } => "polyline",
            Curve::Circle { .. } => "circle",
            Curve::Ellipse { span: None, .. } => "ellipse",
            Curve::Ellipse { .. } => "partial ellipse",
            Curve::Spline { .. } => "spline",
            Curve::Fit { .. } => "fitted spline",
        }
    }

    /// Its parameter's range: a polyline's segment count, a circle's or an ellipse's degrees, a spline's knots, a fitted
    /// spline's distance along its points.
    pub fn domain(&self) -> (f64, f64) {
        match self {
            Curve::Poly { .. } => (0.0, self.segments().len() as f64),
            Curve::Circle { .. } => (0.0, 360.0),
            Curve::Ellipse { span, .. } => match span {
                Some([a, b]) => (*a, *a + span_of(*a, *b)),
                None => (0.0, 360.0),
            },
            Curve::Spline { control, knots } => {
                let (p, u) = (degree(control.len()), knot_vector(control.len(), knots.as_deref()));
                (u.get(p).copied().unwrap_or(0.0), u.get(control.len()).copied().unwrap_or(1.0))
            }
            Curve::Fit { points, .. } => (0.0, chords(points).last().copied().unwrap_or(0.0)),
        }
    }

    /// The point at parameter `t` (see [`Curve::domain`]), `t` held within it.
    pub fn at(&self, t: f64) -> P2 {
        let (t0, t1) = self.domain();
        let t = if t.is_finite() { t.clamp(t0, t1) } else { t0 };
        match self {
            Curve::Poly { .. } => {
                let segs = self.segments();
                let i = (t.floor() as usize).min(segs.len().saturating_sub(1));
                match segs.get(i) {
                    Some(ax) => ax.at((t - i as f64).clamp(0.0, 1.0) * ax.length(), 0.0),
                    None => P2(0.0, 0.0),
                }
            }
            Curve::Circle { centre, radius } => {
                let a = t.to_radians();
                *centre + P2(a.cos(), a.sin()) * *radius
            }
            Curve::Ellipse { centre, radii, angle, .. } => ellipse_at(*centre, *radii, *angle, t),
            Curve::Spline { control, knots } => de_boor(control, &knot_vector(control.len(), knots.as_deref()), t),
            Curve::Fit { points, tangents } => match Cubic::through(points, *tangents) {
                Some(c) => c.at(t),
                None => points.first().copied().unwrap_or_default(),
            },
        }
    }

    /// The way it runs at `t`, a unit vector.
    pub fn tangent(&self, t: f64) -> P2 {
        let (t0, t1) = self.domain();
        if let Curve::Poly { .. } = self {
            let segs = self.segments();
            let i = (t.floor().max(0.0) as usize).min(segs.len().saturating_sub(1));
            if let Some(ax) = segs.get(i) {
                return ax.tangent((t - i as f64).clamp(0.0, 1.0) * ax.length());
            }
        }
        if let Curve::Fit { points, tangents } = self
            && let Some(c) = Cubic::through(points, *tangents)
        {
            return c.slope(t.clamp(t0, t1)).unit();
        }
        let h = (t1 - t0).abs() * 1e-6;
        let (a, b) = ((t - h).max(t0), (t + h).min(t1));
        (self.at(b) - self.at(a)).unit()
    }

    /// Where it starts.
    pub fn start(&self) -> P2 {
        self.at(self.domain().0)
    }

    /// Where it ends: where it starts, when it's closed.
    pub fn end(&self) -> P2 {
        match self {
            Curve::Poly { closed: true, .. } => self.start(),
            _ => self.at(self.domain().1),
        }
    }

    /// A polyline's segments, the closing one too, each straight or an arc.
    pub fn segments(&self) -> Vec<Axis> {
        let Curve::Poly { points, arcs, closed } = self else { return Vec::new() };
        let n = points.len();
        let count = if *closed && n >= 2 { n } else { n.saturating_sub(1) };
        (0..count)
            .filter_map(|i| Some(Axis::new(*points.get(i)?, *points.get((i + 1) % n)?, arcs.get(i).copied().unwrap_or(0.0))))
            .collect()
    }

    /// Its points as drawn, from start to end (a closed one's first again at its end), each with its parameter.
    pub fn samples(&self) -> Vec<(f64, P2)> {
        let (t0, t1) = self.domain();
        let mut out: Vec<(f64, P2)> = Vec::new();
        match self {
            Curve::Poly { .. } => {
                for (i, ax) in self.segments().iter().enumerate() {
                    let len = ax.length().max(f64::MIN_POSITIVE);
                    let skip = usize::from(i > 0);
                    out.extend(ax.stations(0.0, ax.length()).into_iter().skip(skip).map(|s| (i as f64 + s / len, ax.at(s, 0.0))));
                }
            }
            Curve::Circle { radius, .. } => {
                let n = round_steps(*radius, TAU);
                out.extend((0..=n).map(|k| t0 + (t1 - t0) * k as f64 / n as f64).map(|t| (t, self.at(t))));
            }
            Curve::Ellipse { radii, .. } => {
                let sweep = (t1 - t0).to_radians();
                let n = round_steps(radii[0].abs().max(radii[1].abs()), sweep).saturating_mul(2).min(MAX_DRAWN);
                out.extend((0..=n).map(|k| t0 + (t1 - t0) * k as f64 / n as f64).map(|t| (t, self.at(t))));
            }
            Curve::Spline { control, knots } => {
                let u = knot_vector(control.len(), knots.as_deref());
                let mut spans: Vec<f64> = u.iter().copied().filter(|k| *k >= t0 && *k <= t1).collect();
                spans.dedup();
                let per = (MAX_DRAWN / spans.len().max(1)).clamp(2, 32);
                for w in spans.windows(2) {
                    let skip = usize::from(!out.is_empty());
                    out.extend(
                        (0..=per).skip(skip).map(|k| w[0] + (w[1] - w[0]) * k as f64 / per as f64).map(|t| (t, de_boor(control, &u, t))),
                    );
                }
            }
            Curve::Fit { points, tangents } => {
                let Some(c) = Cubic::through(points, *tangents) else { return points.iter().map(|p| (0.0, *p)).collect() };
                let per = (MAX_DRAWN / c.t.len().max(1)).clamp(2, 32);
                for w in c.t.windows(2) {
                    let skip = usize::from(!out.is_empty());
                    out.extend((0..=per).skip(skip).map(|k| w[0] + (w[1] - w[0]) * k as f64 / per as f64).map(|t| (t, c.at(t))));
                }
            }
        }
        if out.is_empty() {
            out.push((t0, self.start()));
        }
        out
    }

    /// Its points as drawn: see [`Curve::samples`].
    pub fn points(&self) -> Vec<P2> {
        self.samples().into_iter().map(|(_, p)| p).collect()
    }

    /// How long it is.
    pub fn length(&self) -> f64 {
        match self {
            Curve::Poly { .. } => self.segments().iter().map(Axis::length).sum(),
            Curve::Circle { radius, .. } => TAU * radius,
            _ => self.points().windows(2).map(|w| w[0].dist(w[1])).sum(),
        }
    }

    /// The centres of its round parts: a circle's or an ellipse's, each arc's of a polyline (each once). What a centre
    /// mark marks.
    pub fn centres(&self) -> Vec<P2> {
        let mut out: Vec<P2> = Vec::new();
        match self {
            Curve::Circle { centre, .. } | Curve::Ellipse { centre, .. } => out.push(*centre),
            Curve::Poly { .. } => {
                for c in self.segments().iter().filter_map(Axis::centre) {
                    if out.iter().all(|q| q.dist(c) > TOL) {
                        out.push(c);
                    }
                }
            }
            Curve::Spline { .. } | Curve::Fit { .. } => {}
        }
        out
    }

    /// Its middle: half way along it.
    pub fn middle(&self) -> P2 {
        let pts = self.points();
        let half = self.length() / 2.0;
        let mut run = 0.0;
        for w in pts.windows(2) {
            let d = w[0].dist(w[1]);
            if run + d >= half && d > 0.0 {
                return w[0] + (w[1] - w[0]) * ((half - run) / d);
            }
            run += d;
        }
        pts.first().copied().unwrap_or_default()
    }

    /// The parameter of its point nearest `p`, and how far `p` is from it.
    pub fn nearest(&self, p: P2) -> (f64, f64) {
        let s = self.samples();
        let mut best = (self.domain().0, f64::INFINITY);
        for w in s.windows(2) {
            let ((ta, a), (tb, b)) = (w[0], w[1]);
            let k = geom::param_on_segment(p, a, b).clamp(0.0, 1.0);
            let d = p.dist(a + (b - a) * k);
            if d < best.1 {
                best = (ta + (tb - ta) * k, d);
            }
        }
        if let [(t, q)] = s.as_slice() {
            best = (*t, p.dist(*q));
        }
        // Closer, on the curve itself: a few steps of a golden-section search about the chord's point.
        let (t0, t1) = self.domain();
        let step = (t1 - t0) / s.len().max(1) as f64;
        let (mut lo, mut hi) = ((best.0 - step).max(t0), (best.0 + step).min(t1));
        let g = 0.618_033_988_749_895;
        for _ in 0..40 {
            let (m1, m2) = (hi - (hi - lo) * g, lo + (hi - lo) * g);
            if p.dist(self.at(m1)) < p.dist(self.at(m2)) {
                hi = m2;
            } else {
                lo = m1;
            }
        }
        let t = (lo + hi) / 2.0;
        let d = p.dist(self.at(t));
        if d < best.1 { (t, d) } else { best }
    }

    /// How far `p` is from it.
    pub fn dist(&self, p: P2) -> f64 {
        self.points().windows(2).map(|w| geom::dist_to_segment(p, w[0], w[1])).fold(f64::INFINITY, f64::min)
    }

    /// The points a grip drags: a polyline's corners and the middle of each of its arcs, a spline's control points or the
    /// points it passes through, a circle's centre and a point on it (due east), an ellipse's centre and the ends of its
    /// axes (and a partial one's ends).
    pub fn handles(&self) -> Vec<P2> {
        match self {
            Curve::Poly { points, arcs, .. } => {
                let mut v = points.clone();
                v.extend(
                    self.segments()
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| arcs.get(*i).is_some_and(|a| *a != 0.0))
                        .map(|(_, s)| s.at(s.length() / 2.0, 0.0)),
                );
                v
            }
            Curve::Fit { points, .. } | Curve::Spline { control: points, .. } => points.clone(),
            Curve::Circle { centre, radius } => vec![*centre, *centre + P2(*radius, 0.0)],
            Curve::Ellipse { centre, radii, angle, span } => {
                let a = angle.to_radians();
                let (u, w) = (P2(a.cos(), a.sin()), P2(-a.sin(), a.cos()));
                let mut v = vec![*centre, *centre + u * radii[0], *centre + w * radii[1]];
                if span.is_some() {
                    v.extend([self.start(), self.end()]);
                }
                v
            }
        }
    }

    /// The corners of a polyline, the points of a spline: what a line's ends and bends are, to snap to.
    pub fn vertices(&self) -> Vec<P2> {
        match self {
            Curve::Poly { points, .. } | Curve::Fit { points, .. } | Curve::Spline { control: points, .. } => points.clone(),
            _ => Vec::new(),
        }
    }

    /// The handle within [`TOL`] of `from` (see [`Curve::handles`]) moved to `to`: a corner, a control point or a point
    /// it passes through goes there; an arc's middle bends the arc through it; a centre takes the circle or the ellipse
    /// with it; a circle's point on it sets its radius, an ellipse's axis ends its radii and turn, a partial one's ends
    /// where it starts and ends.
    pub fn moved(&self, from: P2, to: P2) -> Curve {
        let near = |p: &P2| p.dist(from) <= TOL;
        let mut c = self.clone();
        match &mut c {
            Curve::Poly { points, arcs, closed } => {
                if points.iter().any(near) {
                    points.iter_mut().filter(|p| near(p)).for_each(|p| *p = to);
                } else {
                    // An arc's middle: the arc through it.
                    let n = points.len();
                    let segs = Curve::Poly { points: points.clone(), arcs: arcs.clone(), closed: *closed }.segments();
                    for (i, seg) in segs.iter().enumerate() {
                        if arcs.get(i).is_some_and(|a| *a != 0.0) && near(&seg.at(seg.length() / 2.0, 0.0)) {
                            let (a, b) = (points[i], points[(i + 1) % n.max(1)]);
                            if let Some(slot) = arcs.get_mut(i) {
                                *slot = sweep_through(a, to, b);
                            }
                        }
                    }
                }
            }
            Curve::Fit { points, .. } | Curve::Spline { control: points, .. } => {
                points.iter_mut().filter(|p| near(p)).for_each(|p| *p = to);
            }
            Curve::Circle { centre, radius } => {
                if near(centre) {
                    *centre = to;
                } else if near(&(*centre + P2(*radius, 0.0))) {
                    *radius = centre.dist(to);
                }
            }
            Curve::Ellipse { centre, radii, angle, span } => {
                let a = angle.to_radians();
                let (u, w) = (P2(a.cos(), a.sin()), P2(-a.sin(), a.cos()));
                let param = |q: P2, r: [f64; 2]| {
                    let d = q - *centre;
                    (d.dot(w) / r[1]).atan2(d.dot(u) / r[0]).to_degrees()
                };
                let ends = span.map(|_| (self.start(), self.end()));
                if near(centre) {
                    *centre = to;
                } else if near(&(*centre + u * radii[0])) {
                    let d = to - *centre;
                    radii[0] = d.length();
                    *angle = d.1.atan2(d.0).to_degrees();
                } else if near(&(*centre + w * radii[1])) {
                    radii[1] = (to - *centre).dot(w);
                } else if let (Some((s0, s1)), Some([from_deg, to_deg])) = (ends, span.as_mut()) {
                    if near(&s0) {
                        *from_deg = param(to, *radii);
                    } else if near(&s1) {
                        *to_deg = param(to, *radii);
                    }
                }
            }
        }
        c
    }

    /// The same curve run the other way.
    pub fn reversed(&self) -> Curve {
        match self {
            Curve::Poly { points, arcs, closed } => {
                let n = points.len();
                if *closed {
                    // From the same first point, round the other way: segment i (p_i to p_i+1) becomes the one into p_i.
                    let pts: Vec<P2> = points.first().into_iter().chain(points.iter().skip(1).rev()).copied().collect();
                    let arcs: Vec<f64> = if arcs.is_empty() {
                        Vec::new()
                    } else {
                        (0..n).map(|k| -arcs.get((n - 1 + n - k) % n).copied().unwrap_or(0.0)).collect()
                    };
                    Curve::Poly { points: pts, arcs, closed: true }
                } else {
                    let arcs: Vec<f64> = arcs.iter().rev().map(|a| -a).collect();
                    Curve::Poly { points: points.iter().rev().copied().collect(), arcs, closed: false }
                }
            }
            Curve::Circle { .. } => self.clone(),
            Curve::Ellipse { centre, radii, angle, span } => match span {
                // Run the other way by mirroring the parameter: the second axis turned about.
                Some([a, b]) => Curve::Ellipse { centre: *centre, radii: [radii[0], -radii[1]], angle: *angle, span: Some([-b, -a]) },
                None => self.clone(),
            },
            Curve::Spline { control, knots } => {
                let u = knot_vector(control.len(), knots.as_deref());
                let (lo, hi) = (u.first().copied().unwrap_or(0.0), u.last().copied().unwrap_or(1.0));
                let knots = knots.as_ref().map(|_| u.iter().rev().map(|k| lo + hi - k).collect());
                Curve::Spline { control: control.iter().rev().copied().collect(), knots }
            }
            Curve::Fit { points, tangents } => Curve::Fit {
                points: points.iter().rev().copied().collect(),
                tangents: [tangents[1].map(|t| t * -1.0), tangents[0].map(|t| t * -1.0)],
            },
        }
    }

    /// Each point it holds moved by `f`, a move, a turn or a mirror: a circle's centre, an ellipse's centre and axes, a
    /// spline's points and the ways its ends leave; a mirror turns arcs the other way.
    pub fn map(&self, f: impl Fn(P2) -> P2) -> Curve {
        // Directions moved as long vectors, so a move that rounds its points to a micron leaves them true.
        let o = f(P2(0.0, 0.0));
        let vector = |v: P2| (f(v * 1e6) - o) * 1e-6;
        let mirrored = vector(P2(1.0, 0.0)).cross(vector(P2(0.0, 1.0))) < 0.0;
        match self {
            Curve::Poly { points, arcs, closed } => Curve::Poly {
                points: points.iter().map(|p| f(*p)).collect(),
                arcs: arcs.iter().map(|a| if mirrored { -a } else { *a }).collect(),
                closed: *closed,
            },
            Curve::Circle { centre, radius } => Curve::Circle { centre: f(*centre), radius: *radius },
            // Mirrored, its second axis runs the other way: the same parameters, the same start.
            Curve::Ellipse { centre, radii, angle, span } => {
                let a = angle.to_radians();
                let u = vector(P2(a.cos(), a.sin()));
                let radii = if mirrored { [radii[0], -radii[1]] } else { *radii };
                Curve::Ellipse { centre: f(*centre), radii, angle: u.1.atan2(u.0).to_degrees(), span: *span }
            }
            Curve::Spline { control, knots } => Curve::Spline { control: control.iter().map(|p| f(*p)).collect(), knots: knots.clone() },
            Curve::Fit { points, tangents } => {
                Curve::Fit { points: points.iter().map(|p| f(*p)).collect(), tangents: tangents.map(|t| t.map(vector)) }
            }
        }
    }

    /// Cut in two at parameter `t`: the part before and the part after, which meet there. A closed curve is opened there
    /// instead, into one that starts and ends at `t`. None at its ends, where there's nothing to cut off.
    pub fn split(&self, t: f64) -> Option<(Curve, Option<Curve>)> {
        let (t0, t1) = self.domain();
        if !t.is_finite() {
            return None;
        }
        let p = self.at(t);
        let open = !self.is_closed();
        if open && (t <= t0 || t >= t1 || p.dist(self.start()) <= TOL || p.dist(self.end()) <= TOL) {
            return None;
        }
        match self {
            Curve::Poly { points, arcs, closed } => {
                let segs = self.segments();
                let i = (t.floor().max(0.0) as usize).min(segs.len().saturating_sub(1));
                let ax = segs.get(i)?;
                let k = (t - i as f64).clamp(0.0, 1.0);
                let sweep = arcs.get(i).copied().unwrap_or(0.0);
                let n = points.len();
                let arc_of = |j: usize| arcs.get(j).copied().unwrap_or(0.0);
                // On a vertex, nothing is cut but the polyline there.
                let at_vertex = k * ax.length() <= TOL || (1.0 - k) * ax.length() <= TOL;
                let cut = if k * ax.length() <= TOL {
                    Some(i)
                } else if (1.0 - k) * ax.length() <= TOL {
                    Some((i + 1) % n.max(1))
                } else {
                    None
                };
                if *closed {
                    // Opened at the point: round from it back to it.
                    let (mut pts, mut arcs2) = (Vec::new(), Vec::new());
                    match cut {
                        Some(v) => {
                            for j in 0..n {
                                pts.push(points[(v + j) % n]);
                                arcs2.push(arc_of((v + j) % n));
                            }
                            pts.push(points[v]);
                        }
                        None => {
                            pts.push(p);
                            arcs2.push(sweep * (1.0 - k));
                            for j in 1..=n {
                                pts.push(points[(i + j) % n]);
                                arcs2.push(if j == n { sweep * k } else { arc_of((i + j) % n) });
                            }
                            pts.push(p);
                        }
                    }
                    arcs2.truncate(pts.len() - 1);
                    return Some((Curve::Poly { points: pts, arcs: tidy(arcs2), closed: false }, None));
                }
                let (mut a_pts, mut a_arcs) = (points[..=i].to_vec(), (0..i).map(arc_of).collect::<Vec<f64>>());
                let (mut b_pts, mut b_arcs) = (Vec::new(), Vec::new());
                match cut {
                    Some(v) if at_vertex => {
                        a_pts = points[..=v].to_vec();
                        a_arcs = (0..v).map(arc_of).collect();
                        b_pts = points[v..].to_vec();
                        b_arcs = (v..n - 1).map(arc_of).collect();
                    }
                    _ => {
                        a_pts.push(p);
                        a_arcs.push(sweep * k);
                        b_pts.push(p);
                        b_pts.extend_from_slice(&points[i + 1..]);
                        b_arcs.push(sweep * (1.0 - k));
                        b_arcs.extend((i + 1..n - 1).map(arc_of));
                    }
                }
                if a_pts.len() < 2 || b_pts.len() < 2 {
                    return None;
                }
                Some((
                    Curve::Poly { points: a_pts, arcs: tidy(a_arcs), closed: false },
                    Some(Curve::Poly { points: b_pts, arcs: tidy(b_arcs), closed: false }),
                ))
            }
            Curve::Circle { centre, radius } => {
                // Opened into two half circles from the point round to the point.
                let opposite = *centre * 2.0 - p;
                let _ = radius;
                Some((Curve::Poly { points: vec![p, opposite, p], arcs: vec![180.0, 180.0], closed: false }, None))
            }
            Curve::Ellipse { centre, radii, angle, span } => match span {
                None => Some((Curve::Ellipse { centre: *centre, radii: *radii, angle: *angle, span: Some([t, t + 360.0]) }, None)),
                Some(_) => Some((
                    Curve::Ellipse { centre: *centre, radii: *radii, angle: *angle, span: Some([t0, t]) },
                    Some(Curve::Ellipse { centre: *centre, radii: *radii, angle: *angle, span: Some([t, t1]) }),
                )),
            },
            Curve::Spline { control, knots } => {
                let (a, b) = split_spline(control, &knot_vector(control.len(), knots.as_deref()), t)?;
                Some((a, Some(b)))
            }
            Curve::Fit { points, tangents } => {
                let c = Cubic::through(points, *tangents)?;
                let dir = c.slope(t).unit();
                let i = c.t.iter().position(|&s| s >= t).unwrap_or(points.len());
                let before: Vec<P2> = points[..i].iter().copied().filter(|q| q.dist(p) > TOL).chain([p]).collect();
                let after: Vec<P2> = std::iter::once(p).chain(points[i..].iter().copied().filter(|q| q.dist(p) > TOL)).collect();
                let a = refit(&c, (t0, t), before, [tangents[0], Some(dir)]);
                let b = refit(&c, (t, t1), after, [Some(dir), tangents[1]]);
                Some((a, Some(b)))
            }
        }
    }

    /// With a vertex where it passes nearest `at`: a polyline's segment cut there (an arc's turn shared out); a control
    /// point added by inserting a knot there, the curve exactly as it was; a point to pass through added there. None for a
    /// circle or an ellipse, or at an end.
    pub fn with_vertex(&self, at: P2) -> Option<Curve> {
        let (t, _) = self.nearest(at);
        let (t0, t1) = self.domain();
        if t <= t0 || t >= t1 {
            return None;
        }
        match self {
            Curve::Poly { closed, .. } => {
                let (a, b) = self.split(t)?;
                // The two halves joined again at the cut: one more corner. A closed one opens at the cut, so close it back.
                match (b, closed) {
                    (Some(b), false) => join(&[a, b]),
                    (None, true) => {
                        let Curve::Poly { mut points, arcs, .. } = a else { return None };
                        points.pop();
                        Some(Curve::Poly { points, arcs, closed: true })
                    }
                    _ => None,
                }
            }
            Curve::Spline { control, knots } => {
                let (q, v) = insert_knot(control, &knot_vector(control.len(), knots.as_deref()), t);
                Some(Curve::Spline { control: q, knots: Some(v) })
            }
            Curve::Fit { points, tangents } => {
                let c = Cubic::through(points, *tangents)?;
                let p = c.at(t);
                let i = c.t.iter().position(|&s| s >= t).unwrap_or(points.len());
                if points.get(i).is_some_and(|q| q.dist(p) <= TOL)
                    || i.checked_sub(1).and_then(|j| points.get(j)).is_some_and(|q| q.dist(p) <= TOL)
                {
                    return None;
                }
                let mut pts = points.clone();
                pts.insert(i, p);
                Some(Curve::Fit { points: pts, tangents: *tangents })
            }
            Curve::Circle { .. } | Curve::Ellipse { .. } => None,
        }
    }

    /// Without the vertex nearest `at`: a polyline's corner (the segments either side made one, straight unless both were
    /// arcs, when it turns as much as both did), a control point, a point it passed through. None when it would have too
    /// few to be a line.
    pub fn without_vertex(&self, at: P2) -> Option<Curve> {
        let near = |pts: &[P2]| pts.iter().enumerate().min_by(|a, b| a.1.dist(at).total_cmp(&b.1.dist(at))).map(|(i, _)| i);
        match self {
            Curve::Poly { points, arcs, closed } => {
                let n = points.len();
                if n <= if *closed { 3 } else { 2 } {
                    return None;
                }
                let i = near(points)?;
                // Each new segment's turn: the old one's, or where it spans the corner taken out, both of those turns
                // added (straight unless both were arcs).
                let turn = |k: usize| arcs.get(k).copied().unwrap_or(0.0);
                let kept: Vec<usize> = (0..n).filter(|&k| k != i).collect();
                let m = kept.len();
                let segs = if *closed { m } else { m - 1 };
                let turns: Vec<f64> = (0..segs)
                    .map(|j| {
                        let (a, b) = (kept[j], kept[(j + 1) % m]);
                        if (a + 1) % n == b {
                            turn(a)
                        } else {
                            let (s1, s2) = (turn(a), turn((a + 1) % n));
                            if s1 != 0.0 && s2 != 0.0 { s1 + s2 } else { 0.0 }
                        }
                    })
                    .collect();
                let pts: Vec<P2> = kept.iter().map(|&k| points[k]).collect();
                let c = Curve::Poly { points: pts, arcs: tidy(turns), closed: *closed };
                c.check("").is_ok().then_some(c)
            }
            Curve::Spline { control, knots } => {
                if control.len() <= 2 {
                    return None;
                }
                let i = near(control)?;
                let mut q = control.clone();
                q.remove(i);
                // Its own knots lose one inside, near where the point led it; uniform ones stay uniform.
                let knots = knots.as_ref().and_then(|u| {
                    let p = degree_of(control.len(), u);
                    let inner = p + 1..u.len().saturating_sub(p + 1);
                    if inner.is_empty() {
                        return None;
                    }
                    let k = (i + 1).clamp(inner.start, inner.end - 1);
                    let mut v = u.clone();
                    v.remove(k);
                    Some(v)
                });
                let c = Curve::Spline { control: q, knots };
                c.check("").is_ok().then_some(c)
            }
            Curve::Fit { points, tangents } => {
                if points.len() <= 2 {
                    return None;
                }
                let i = near(points)?;
                let mut pts = points.clone();
                pts.remove(i);
                Some(Curve::Fit { points: pts, tangents: *tangents })
            }
            Curve::Circle { .. } | Curve::Ellipse { .. } => None,
        }
    }

    /// A spline through points as one by its control points, exactly the same curve (a cubic B-spline with a knot at each
    /// point it passed through); or a spline by its control points as one through points, within [`FIT_TOL`] of it. None
    /// for any other shape.
    pub fn converted(&self) -> Option<Curve> {
        match self {
            Curve::Fit { points, tangents } => fit_to_control(points, *tangents),
            Curve::Spline { .. } => {
                let (t0, t1) = self.domain();
                // Its ends, and a point at each knot between, then whatever more it takes to lie within the tolerance.
                let u = match self {
                    Curve::Spline { control, knots } => knot_vector(control.len(), knots.as_deref()),
                    _ => Vec::new(),
                };
                let mut ts: Vec<f64> = u.iter().copied().filter(|k| *k > t0 && *k < t1).collect();
                ts.dedup();
                ts.insert(0, t0);
                ts.push(t1);
                let pts: Vec<P2> = ts.iter().map(|t| self.at(*t)).collect();
                let tangents = [Some(self.tangent(t0)), Some(self.tangent(t1))];
                Some(refit_to(self, pts, ts, tangents))
            }
            _ => None,
        }
    }

    /// Cut in two where it passes nearest `p`: see [`Curve::split`].
    pub fn split_at(&self, p: P2) -> Option<(Curve, Option<Curve>)> {
        self.split(self.nearest(p).0)
    }

    /// An error saying what to fix, unless every number is one and within range, it has enough points and few enough, and
    /// it isn't too small to see.
    pub fn check(&self, who: &str) -> Result<(), String> {
        self.sound(who)?;
        if self.length() <= TOL {
            return Err(format!("{who}: it is {TOL} mm long or less"));
        }
        Ok(())
    }

    /// How many points it holds: a polyline's, a spline's control points or the points it passes through, or its knots
    /// when there are more of those; a circle's or an ellipse's centre.
    pub fn count(&self) -> usize {
        match self {
            Curve::Poly { points, arcs, .. } => points.len().max(arcs.len()),
            Curve::Fit { points, .. } => points.len(),
            Curve::Spline { control, knots } => control.len().max(knots.as_ref().map_or(0, Vec::len)),
            Curve::Circle { .. } | Curve::Ellipse { .. } => 1,
        }
    }

    /// Whether every point it holds is a number within [`MAX_COORD`].
    pub fn in_range(&self) -> bool {
        let fine = |p: &P2| p.is_finite() && p.0.abs() <= MAX_COORD && p.1.abs() <= MAX_COORD;
        match self {
            Curve::Poly { points, .. } | Curve::Fit { points, .. } | Curve::Spline { control: points, .. } => points.iter().all(fine),
            Curve::Circle { centre, .. } | Curve::Ellipse { centre, .. } => fine(centre),
        }
    }

    /// [`Curve::check`] but for its length: what it holds is a curve, however small.
    pub fn sound(&self, who: &str) -> Result<(), String> {
        let fine = |p: &P2| p.is_finite() && p.0.abs() <= MAX_COORD && p.1.abs() <= MAX_COORD;
        let all_fine = |ps: &[P2]| ps.iter().all(fine);
        let size = |r: f64, what: &str| {
            if !(r.is_finite() && r > TOL && r <= MAX_COORD) {
                Err(format!("{who}: its {what} is {r} mm; it's more than {TOL} mm and at most {MAX_COORD}"))
            } else {
                Ok(())
            }
        };
        let distinct = |ps: &[P2]| ps.windows(2).all(|w| w[0].dist(w[1]) > 1e-6);
        match self {
            Curve::Poly { points, arcs, closed } => {
                let n = points.len();
                if n < 2 {
                    return Err(format!("{who}: a line needs two points or more, not {n}"));
                }
                if n > MAX_POINTS {
                    return Err(format!("{who}: a line has at most {MAX_POINTS} points"));
                }
                if *closed && n < 3 && arcs.iter().all(|a| *a == 0.0) {
                    return Err(format!("{who}: a closed line needs three points or more, or arcs"));
                }
                if !all_fine(points) {
                    return Err(format!("{who}: a point isn't a number within {MAX_COORD} mm"));
                }
                if !distinct(points) || (*closed && points.first().zip(points.last()).is_some_and(|(a, b)| a.dist(*b) <= 1e-6)) {
                    return Err(format!("{who}: two points one after another are the same; a line runs between points apart"));
                }
                let segs = if *closed { n } else { n - 1 };
                if !arcs.is_empty() && arcs.len() != segs {
                    return Err(format!("{who}: `arcs` gives one turn for each of its {segs} segments, not {}", arcs.len()));
                }
                if let Some(a) = arcs.iter().find(|a| !(a.is_finite() && a.abs() < 360.0)) {
                    return Err(format!("{who}: an arc turns {a}°; it turns less than a whole circle, either way"));
                }
            }
            Curve::Circle { centre, radius } => {
                if !fine(centre) {
                    return Err(format!("{who}: its centre isn't a point within {MAX_COORD} mm"));
                }
                size(*radius, "radius")?;
            }
            Curve::Ellipse { centre, radii, angle, span } => {
                if !fine(centre) {
                    return Err(format!("{who}: its centre isn't a point within {MAX_COORD} mm"));
                }
                size(radii[0].abs(), "first radius")?;
                size(radii[1].abs(), "second radius")?;
                if !angle.is_finite() || angle.abs() > 1e6 {
                    return Err(format!("{who}: its angle {angle}° isn't a number of degrees"));
                }
                if let Some([a, b]) = span {
                    if !(a.is_finite() && b.is_finite() && a.abs() <= 1e6 && b.abs() <= 1e6) {
                        return Err(format!("{who}: its span isn't two angles in degrees"));
                    }
                    if span_of(*a, *b) < 1e-6 {
                        return Err(format!("{who}: its span from {a}° to {b}° is nothing; give two angles apart"));
                    }
                }
            }
            Curve::Spline { control, knots } => {
                let n = control.len();
                if n < 2 {
                    return Err(format!("{who}: a spline needs two control points or more, not {n}"));
                }
                if n > MAX_POINTS {
                    return Err(format!("{who}: a spline has at most {MAX_POINTS} control points"));
                }
                if !all_fine(control) {
                    return Err(format!("{who}: a control point isn't a number within {MAX_COORD} mm"));
                }
                if let Some(u) = knots {
                    let p = u.len().checked_sub(n + 1).unwrap_or(usize::MAX);
                    if p == 0 || p > MAX_DEGREE || p >= n {
                        return Err(format!(
                            "{who}: a spline of {n} control points has {n} plus its degree plus one knots (degree 1 to {MAX_DEGREE}, below {n}), not {}",
                            u.len()
                        ));
                    }
                    if u.iter().any(|k| !k.is_finite() || k.abs() > 1e9) || u.windows(2).any(|w| w[1] < w[0]) {
                        return Err(format!("{who}: its knots are numbers, each no less than the one before"));
                    }
                    let (first, last) = (u[0], u[u.len() - 1]);
                    if u[..=p].iter().any(|k| *k != first) || u[u.len() - p - 1..].iter().any(|k| *k != last) || last - first <= 0.0 {
                        return Err(format!(
                            "{who}: its first {0} knots are the same, and its last {0}, so it runs from its first control point to its last",
                            p + 1
                        ));
                    }
                    if u[p + 1..u.len() - p - 1].windows(1).any(|w| u.iter().filter(|k| **k == w[0]).count() > p) {
                        return Err(format!("{who}: no knot inside it comes more than {p} times"));
                    }
                }
            }
            Curve::Fit { points, tangents } => {
                let n = points.len();
                if n < 2 {
                    return Err(format!("{who}: a spline needs two points or more to pass through, not {n}"));
                }
                if n > MAX_POINTS {
                    return Err(format!("{who}: a spline passes through at most {MAX_POINTS} points"));
                }
                if !all_fine(points) {
                    return Err(format!("{who}: a point isn't a number within {MAX_COORD} mm"));
                }
                if !distinct(points) {
                    return Err(format!("{who}: two points one after another are the same; a spline passes through points apart"));
                }
                if tangents.iter().flatten().any(|t| !t.is_finite() || t.length() < 1e-9 || t.length() > MAX_COORD) {
                    return Err(format!("{who}: a tangent is a direction [x, y], not nothing"));
                }
            }
        }
        Ok(())
    }
}

/// How many steps a round of `radius` takes to turn `sweep` radians, none further than [`FLAT`] from it or 5° round.
fn round_steps(radius: f64, sweep: f64) -> usize {
    let r = radius.abs().max(FLAT * 2.0);
    let step = (2.0 * (1.0 - FLAT / r).acos()).min(5f64.to_radians()).max(1e-4);
    ((sweep.abs() / step).ceil() as usize).clamp(8, MAX_DRAWN)
}

/// How far a span from `a` round counter-clockwise to `b` turns, degrees: more than 0, at most 360.
fn span_of(a: f64, b: f64) -> f64 {
    let s = (b - a).rem_euclid(360.0);
    if s < 1e-9 && b != a { 360.0 } else { s }
}

/// The point of an ellipse at parameter angle `t` degrees.
fn ellipse_at(c: P2, [a, b]: [f64; 2], angle: f64, t: f64) -> P2 {
    let (u, v) = (angle.to_radians(), t.to_radians());
    let (x, y) = (a * v.cos(), b * v.sin());
    c + P2(x * u.cos() - y * u.sin(), x * u.sin() + y * u.cos())
}

/// No arcs at all when every segment is straight.
fn tidy(arcs: Vec<f64>) -> Vec<f64> {
    if arcs.iter().all(|a| *a == 0.0) { Vec::new() } else { arcs }
}

/// A spline's degree for `n` control points: three, fewer for fewer.
fn degree(n: usize) -> usize {
    n.saturating_sub(1).clamp(1, 3)
}

/// Its knots, or for `n` control points uniform ones clamped to the ends, from 0 to 1.
fn knot_vector(n: usize, knots: Option<&[f64]>) -> Vec<f64> {
    if let Some(u) = knots {
        return u.to_vec();
    }
    let p = degree(n);
    let inner = n.saturating_sub(p + 1);
    let mut u = vec![0.0; p + 1];
    u.extend((1..=inner).map(|i| i as f64 / (inner + 1) as f64));
    u.extend(std::iter::repeat_n(1.0, p + 1));
    u
}

/// The degree of a spline of `n` control points over knots `u`.
fn degree_of(n: usize, u: &[f64]) -> usize {
    u.len().saturating_sub(n + 1).max(1)
}

/// The point of the B-spline of `control` over knots `u` at `t`: de Boor's algorithm.
fn de_boor(control: &[P2], u: &[f64], t: f64) -> P2 {
    let n = control.len();
    let p = degree_of(n, u);
    if n == 0 || u.len() < n + p + 1 {
        return control.first().copied().unwrap_or_default();
    }
    // The span: u[k] <= t < u[k + 1], the last one taking the end.
    let mut k = p;
    while k + 1 < n && u[k + 1] <= t {
        k += 1;
    }
    let mut d: Vec<P2> = (0..=p).map(|j| control[(j + k).saturating_sub(p).min(n - 1)]).collect();
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = j + k - p;
            let den = u[i + p + 1 - r] - u[i];
            let a = if den.abs() < 1e-300 { 0.0 } else { (t - u[i]) / den };
            d[j] = d[j - 1] * (1.0 - a) + d[j] * a;
        }
    }
    d[p]
}

/// `t` inserted once into the B-spline of `control` over `u`: Boehm's algorithm, the curve unchanged.
fn insert_knot(control: &[P2], u: &[f64], t: f64) -> (Vec<P2>, Vec<f64>) {
    let n = control.len();
    let p = degree_of(n, u);
    let mut k = p;
    while k + 1 < n && u[k + 1] <= t {
        k += 1;
    }
    let mut q = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let pt = if i + p <= k {
            control[i]
        } else if i > k {
            control[i - 1]
        } else {
            let den = u[i + p] - u[i];
            let a = if den.abs() < 1e-300 { 0.0 } else { (t - u[i]) / den };
            control[i - 1] * (1.0 - a) + control[i] * a
        };
        q.push(pt);
    }
    let mut v = u.to_vec();
    v.insert(k + 1, t);
    (q, v)
}

/// A spline split at `t`, exactly: `t` inserted until it's a knot as many times as the degree, the control points
/// divided there.
fn split_spline(control: &[P2], u: &[f64], t: f64) -> Option<(Curve, Curve)> {
    let p = degree_of(control.len(), u);
    let (mut q, mut v) = (control.to_vec(), u.to_vec());
    let have = v.iter().filter(|k| **k == t).count();
    for _ in have..p {
        (q, v) = insert_knot(&q, &v, t);
    }
    let m = v.iter().filter(|k| **k < t).count();
    if m < 1 || m > q.len() {
        return None;
    }
    let left_knots: Vec<f64> = v.iter().copied().filter(|k| *k < t).chain(std::iter::repeat_n(t, p + 1)).collect();
    let right_knots: Vec<f64> = std::iter::repeat_n(t, p + 1).chain(v.iter().copied().filter(|k| *k > t)).collect();
    let (left, right) = (q[..m].to_vec(), q[m - 1..].to_vec());
    if left.len() < 2 || right.len() < 2 {
        return None;
    }
    Some((Curve::Spline { control: left, knots: Some(left_knots) }, Curve::Spline { control: right, knots: Some(right_knots) }))
}

/// Each point's distance along the points from the first: a fitted spline's parameters.
fn chords(points: &[P2]) -> Vec<f64> {
    let mut t = 0.0;
    let mut out = Vec::with_capacity(points.len());
    for (i, p) in points.iter().enumerate() {
        if i > 0 {
            t += p.dist(points[i - 1]);
        }
        out.push(t);
    }
    out
}

/// A cubic through points, x and y each a cubic spline of the distance along them: its parameters, points, and second
/// derivatives at each.
struct Cubic {
    t: Vec<f64>,
    p: Vec<P2>,
    m: Vec<P2>,
}

impl Cubic {
    /// The spline through `points`, each end leaving along its tangent when there's one (as fast as the distance along
    /// grows) or free, straight there.
    fn through(points: &[P2], tangents: [Option<P2>; 2]) -> Option<Cubic> {
        let n = points.len();
        if n < 2 {
            return None;
        }
        let t = chords(points);
        let h: Vec<f64> = t.windows(2).map(|w| w[1] - w[0]).collect();
        if h.iter().any(|h| *h <= 0.0 || !h.is_finite()) {
            return None;
        }
        // The tridiagonal system for the second derivatives, x and y at once: the Thomas algorithm.
        let (mut a, mut b, mut c, mut d) = (vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![P2(0.0, 0.0); n]);
        let slope = |i: usize| (points[i + 1] - points[i]) * (1.0 / h[i]);
        match tangents[0] {
            Some(s) => {
                b[0] = 2.0 * h[0];
                c[0] = h[0];
                d[0] = (slope(0) - s.unit()) * 6.0;
            }
            None => b[0] = 1.0,
        }
        for i in 1..n - 1 {
            a[i] = h[i - 1];
            b[i] = 2.0 * (h[i - 1] + h[i]);
            c[i] = h[i];
            d[i] = (slope(i) - slope(i - 1)) * 6.0;
        }
        match tangents[1] {
            Some(e) => {
                a[n - 1] = h[n - 2];
                b[n - 1] = 2.0 * h[n - 2];
                d[n - 1] = (e.unit() - slope(n - 2)) * 6.0;
            }
            None => b[n - 1] = 1.0,
        }
        for i in 1..n {
            let w = a[i] / b[i - 1];
            b[i] -= w * c[i - 1];
            d[i] = d[i] - d[i - 1] * w;
        }
        let mut m = vec![P2(0.0, 0.0); n];
        m[n - 1] = d[n - 1] * (1.0 / b[n - 1]);
        for i in (0..n - 1).rev() {
            m[i] = (d[i] - m[i + 1] * c[i]) * (1.0 / b[i]);
        }
        m.iter().all(|q| q.is_finite()).then(|| Cubic { t, p: points.to_vec(), m })
    }

    fn piece(&self, s: f64) -> usize {
        let n = self.t.len();
        self.t.iter().skip(1).position(|&k| s <= k).unwrap_or(n.saturating_sub(2)).min(n.saturating_sub(2))
    }

    fn at(&self, s: f64) -> P2 {
        let i = self.piece(s);
        let h = self.t[i + 1] - self.t[i];
        let (u, v) = ((self.t[i + 1] - s) / h, (s - self.t[i]) / h);
        let (m0, m1, p0, p1) = (self.m[i], self.m[i + 1], self.p[i], self.p[i + 1]);
        m0 * (u * u * u * h * h / 6.0) + m1 * (v * v * v * h * h / 6.0) + (p0 - m0 * (h * h / 6.0)) * u + (p1 - m1 * (h * h / 6.0)) * v
    }

    fn slope(&self, s: f64) -> P2 {
        let i = self.piece(s);
        let h = self.t[i + 1] - self.t[i];
        let (u, v) = ((self.t[i + 1] - s) / h, (s - self.t[i]) / h);
        let (m0, m1, p0, p1) = (self.m[i], self.m[i + 1], self.p[i], self.p[i + 1]);
        m1 * (v * v * h / 2.0) - m0 * (u * u * h / 2.0) + (p1 - p0) * (1.0 / h) - (m1 - m0) * (h / 6.0)
    }
}

/// The part of `original` from `range.0` to `range.1` as a fitted spline through `points` with its ends' `tangents`:
/// see [`refit_with`].
fn refit(original: &Cubic, range: (f64, f64), points: Vec<P2>, tangents: [Option<P2>; 2]) -> Curve {
    let truth = |s: f64| original.at(s);
    // Where each point lies along the original, to keep them in order as more go in.
    let k = 256;
    let samples: Vec<(f64, P2)> =
        (0..=k).map(|i| range.0 + (range.1 - range.0) * i as f64 / k as f64).map(|s| (s, original.at(s))).collect();
    let along: Vec<f64> =
        points.iter().map(|p| samples.iter().min_by(|a, b| a.1.dist(*p).total_cmp(&b.1.dist(*p))).map_or(range.0, |(s, _)| *s)).collect();
    refit_with(&truth, range, points, along, tangents)
}

/// The curve `truth` gives from `range.0` to `range.1` as a fitted spline through `points` (lying `along` it) with its
/// ends' `tangents`: points of it added, one at a time, where the fit strays furthest, until none of it lies further than
/// [`FIT_TOL`] from it (or it has as many as it may).
fn refit_with(truth: &dyn Fn(f64) -> P2, range: (f64, f64), mut points: Vec<P2>, mut along: Vec<f64>, tangents: [Option<P2>; 2]) -> Curve {
    let (lo, hi) = range;
    let k = 256;
    let samples: Vec<(f64, P2)> = (0..=k).map(|i| lo + (hi - lo) * i as f64 / k as f64).map(|s| (s, truth(s))).collect();
    for _ in 0..64 {
        let fit = Curve::Fit { points: points.clone(), tangents };
        let drawn = fit.points();
        let off = |q: P2| drawn.windows(2).map(|w| geom::dist_to_segment(q, w[0], w[1])).fold(f64::INFINITY, f64::min);
        let Some((s, q, d)) = samples.iter().map(|(s, q)| (*s, *q, off(*q))).max_by(|a, b| a.2.total_cmp(&b.2)) else { break };
        if d <= FIT_TOL || points.len() >= MAX_POINTS {
            break;
        }
        let at = along.iter().position(|a| *a > s).unwrap_or(along.len());
        if points.get(at.wrapping_sub(1)).is_some_and(|p| p.dist(q) <= 1e-3) || points.get(at).is_some_and(|p| p.dist(q) <= 1e-3) {
            break;
        }
        points.insert(at, q);
        along.insert(at, s);
    }
    Curve::Fit { points, tangents }
}

/// A copy of an ellipse or a spline `by` mm to its left (to its right when negative), as a spline through points lying
/// within [`FIT_TOL`] of the true offset: Revit's Offset can't do these. None where it would fold over itself, or isn't a
/// curve that can be drawn.
pub fn offset_fit(c: &Curve, by: f64) -> Option<Curve> {
    let (t0, t1) = c.domain();
    if !by.is_finite() || t1.partial_cmp(&t0) != Some(std::cmp::Ordering::Greater) {
        return None;
    }
    let truth = |t: f64| c.at(t) + c.tangent(t).perp() * by;
    let n = 8;
    let along: Vec<f64> = (0..=n).map(|i| t0 + (t1 - t0) * f64::from(i) / f64::from(n)).collect();
    let points: Vec<P2> = along.iter().map(|t| truth(*t)).collect();
    let out = refit_with(&truth, (t0, t1), points, along, [Some(c.tangent(t0)), Some(c.tangent(t1))]);
    // Folded over itself, the offset runs back against the curve somewhere.
    let k = 64;
    let folds = (0..k).any(|i| {
        let (a, b) = (t0 + (t1 - t0) * f64::from(i) / f64::from(k), t0 + (t1 - t0) * f64::from(i + 1) / f64::from(k));
        (truth(b) - truth(a)).dot(c.at(b) - c.at(a)) < 0.0
    });
    (!folds && out.check("").is_ok()).then_some(out)
}

/// A spline by its control points `c` as one through points, within [`FIT_TOL`] of it: through its ends and its knots,
/// leaving and arriving as it does, and through more of it where need be.
fn refit_to(c: &Curve, points: Vec<P2>, along: Vec<f64>, tangents: [Option<P2>; 2]) -> Curve {
    let truth = |t: f64| c.at(t);
    refit_with(&truth, c.domain(), points, along, tangents)
}

/// The spline through `points` (with its ends' `tangents`) as a cubic B-spline by its control points, exactly: a knot at
/// each point's distance along them, as the fitted spline's own parameters, and the control points that make it pass
/// through each and leave and arrive as it does (Piegl and Tiller, *The NURBS Book*, 9.2.2): a tridiagonal system.
fn fit_to_control(points: &[P2], tangents: [Option<P2>; 2]) -> Option<Curve> {
    let n = points.len();
    if n < 2 {
        return None;
    }
    let t = chords(points);
    // Knots: the ends four times, each point between once.
    let mut u = vec![t[0]; 4];
    u.extend(t[1..n - 1].iter().copied());
    u.extend(std::iter::repeat_n(t[n - 1], 4));
    // Unknowns Q1..Q(m-2): row 0 the start, rows 1..n-2 the points between, row n-1 the end.
    let (mut a, mut b, mut c, mut d) = (vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![P2(0.0, 0.0); n]);
    let (q0, ql) = (points[0], points[n - 1]);
    let h0 = t[1] - t[0];
    let hl = t[n - 1] - t[n - 2];
    if h0 <= 0.0 || hl <= 0.0 {
        return None;
    }
    match tangents[0] {
        // Q1 = Q0 + h0/3 · the way it leaves.
        Some(s) => {
            b[0] = 1.0;
            d[0] = q0 + s.unit() * (h0 / 3.0);
        }
        // Straight at its start: (Q2 − Q1)/(t2 − t0) = (Q1 − Q0)/h0.
        None => {
            let w = t.get(2).copied().unwrap_or(t[n - 1]) - t[0];
            let r = w / h0;
            b[0] = -(1.0 + r);
            c[0] = 1.0;
            d[0] = q0 * -r;
        }
    }
    for k in 1..n - 1 {
        // At the knot u[3 + k], the three B-splines that aren't nought there.
        let j = 3 + k;
        let n0 = (u[j + 1] - u[j]).powi(2) / ((u[j + 1] - u[j - 2]) * (u[j + 1] - u[j - 1]));
        let n2 = (u[j] - u[j - 1]).powi(2) / ((u[j + 2] - u[j - 1]) * (u[j + 1] - u[j - 1]));
        let n1 = 1.0 - n0 - n2;
        // Q_k, Q_k+1, Q_k+2: unknowns k-1, k, k+1 (the first and last control points, known, are never among them).
        a[k] = n0;
        b[k] = n1;
        c[k] = n2;
        d[k] = points[k];
    }
    let last = n - 1;
    match tangents[1] {
        Some(e) => {
            b[last] = 1.0;
            a[last] = 0.0;
            d[last] = ql - e.unit() * (hl / 3.0);
        }
        // Straight at its end: (Q(m−1) − Q(m−2))/hl = (Q(m−2) − Q(m−3))/(t(n−1) − t(n−3)).
        None => {
            let w = t[n - 1] - t.get(n.wrapping_sub(3)).copied().filter(|_| n >= 3).unwrap_or(t[0]);
            let r = w / hl;
            if last == 0 {
                return None;
            }
            a[last] = 1.0;
            b[last] = -(1.0 + r);
            d[last] = ql * -r;
        }
    }
    // Thomas: forward then back.
    for i in 1..n {
        if b[i - 1].abs() < 1e-300 {
            return None;
        }
        let w = a[i] / b[i - 1];
        b[i] -= w * c[i - 1];
        d[i] = d[i] - d[i - 1] * w;
    }
    let mut q = vec![P2(0.0, 0.0); n];
    q[n - 1] = d[n - 1] * (1.0 / b[n - 1]);
    for i in (0..n - 1).rev() {
        q[i] = (d[i] - q[i + 1] * c[i]) * (1.0 / b[i]);
    }
    let control: Vec<P2> = std::iter::once(q0).chain(q).chain(std::iter::once(ql)).collect();
    control.iter().all(|p| p.is_finite()).then_some(Curve::Spline { control, knots: Some(u) })
}

/// Lines and arcs joined end to end into one polyline, in the order they meet, each turned round as it must be: Join
/// into Polyline. None unless all are lines, arcs or polylines that make one chain, each end meeting the next within
/// [`TOL`]; closed when the chain comes back to where it began.
pub fn join(curves: &[Curve]) -> Option<Curve> {
    if curves.len() < 2 || curves.iter().any(|c| !c.is_poly() || c.is_closed()) {
        return None;
    }
    let mut left: Vec<Curve> = curves[1..].to_vec();
    let mut chain: Vec<Curve> = vec![curves[0].clone()];
    while !left.is_empty() {
        let (head, tail) = (chain.first()?.start(), chain.last()?.end());
        let found = left.iter().enumerate().find_map(|(i, c)| {
            let (s, e) = (c.start(), c.end());
            match () {
                () if s.dist(tail) <= TOL => Some((i, false, false)),
                () if e.dist(tail) <= TOL => Some((i, false, true)),
                () if e.dist(head) <= TOL => Some((i, true, false)),
                () if s.dist(head) <= TOL => Some((i, true, true)),
                () => None,
            }
        });
        let (i, front, flip) = found?;
        let c = left.remove(i);
        let c = if flip { c.reversed() } else { c };
        if front {
            chain.insert(0, c);
        } else {
            chain.push(c);
        }
    }
    let (mut points, mut arcs): (Vec<P2>, Vec<f64>) = (Vec::new(), Vec::new());
    for c in &chain {
        let Curve::Poly { points: p, arcs: a, .. } = c else { return None };
        let skip = usize::from(!points.is_empty());
        points.extend(p.iter().skip(skip));
        arcs.extend((0..p.len() - 1).map(|k| a.get(k).copied().unwrap_or(0.0)));
    }
    let closed = points.len() > 3 && points.first().zip(points.last()).is_some_and(|(a, b)| a.dist(*b) <= TOL);
    if closed {
        points.pop();
    }
    Some(Curve::Poly { points, arcs: tidy(arcs), closed })
}

/// A polyline taken apart into its lines and arcs; anything else as it is.
pub fn explode(c: &Curve) -> Vec<Curve> {
    match c {
        Curve::Poly { arcs, .. } => {
            let segs = c.segments();
            segs.iter().enumerate().map(|(i, ax)| Curve::arc(ax.start, ax.end, arcs.get(i).copied().unwrap_or(0.0))).collect()
        }
        _ => vec![c.clone()],
    }
}

/// A regular polygon of `sides` about `centre`, one corner (inscribed) or one side's middle (circumscribed) at `at`:
/// Revit's Inscribed and Circumscribed Polygon.
pub fn polygon(centre: P2, at: P2, sides: usize, inscribed: bool) -> Option<Curve> {
    if !(3..=360).contains(&sides) || centre.dist(at) <= TOL {
        return None;
    }
    let step = TAU / sides as f64;
    let a0 = (at.1 - centre.1).atan2(at.0 - centre.0);
    let (r, a0) = if inscribed { (centre.dist(at), a0) } else { (centre.dist(at) / (step / 2.0).cos(), a0 + step / 2.0) };
    let points = (0..sides).map(|k| a0 + step * k as f64).map(|a| centre + P2(a.cos(), a.sin()) * r).collect();
    Some(Curve::Poly { points, arcs: Vec::new(), closed: true })
}

/// The arc of `radius` that rounds the corner where line `a` (towards `corner`) meets line `b` (away from it), and
/// where it touches each: Revit's Fillet Arc. None when they're straight on or too short for it.
pub fn fillet(a: P2, corner: P2, b: P2, radius: f64) -> Option<(P2, P2, Curve)> {
    let (u, v) = ((a - corner).unit(), (b - corner).unit());
    let half = u.dot(v).clamp(-1.0, 1.0).acos() / 2.0;
    if !(radius > 0.0 && half > 1e-6 && half < std::f64::consts::FRAC_PI_2 - 1e-6) {
        return None;
    }
    let back = radius / half.tan();
    if back >= a.dist(corner) + TOL || back >= b.dist(corner) + TOL {
        return None;
    }
    let (p, q) = (corner + u * back, corner + v * back);
    let arc = Curve::arc_tangent(p, (corner - p).unit(), q);
    Some((p, q, arc))
}

/// The radius of the round that touches both lines `a`–`corner` and `corner`–`b` and passes nearest `pick`: what a
/// Fillet Arc's third click picks.
pub fn fillet_radius(a: P2, corner: P2, b: P2, pick: P2) -> Option<f64> {
    let (u, v) = ((a - corner).unit(), (b - corner).unit());
    let bis = (u + v).unit();
    let half = u.dot(v).clamp(-1.0, 1.0).acos() / 2.0;
    if bis.length() < 0.5 || half < 1e-6 {
        return None;
    }
    // The arc's middle lies on the bisector, d from the corner: d = r / sin(half) - r.
    let d = (pick - corner).dot(bis).max(TOL);
    let r = d / (1.0 / half.sin() - 1.0);
    (r.is_finite() && r > 0.0).then_some(r)
}

/// The flat fields of a curve, as a file and a command hold them: every shape's, those it doesn't use left out.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fields {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<P2>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<P2>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through: Option<P2>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<P2>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arcs: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub closed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub centre: Option<P2>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radii: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<Vec<P2>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knots: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit: Option<Vec<P2>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_tangent: Option<P2>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_tangent: Option<P2>,
}

/// The name of every field a curve may hold, for a command that sets one shape's and takes away the rest.
pub const FIELD_NAMES: &[&str] = &[
    "start",
    "end",
    "through",
    "arc",
    "points",
    "arcs",
    "closed",
    "centre",
    "radius",
    "radii",
    "angle",
    "span",
    "control",
    "knots",
    "fit",
    "start_tangent",
    "end_tangent",
];

/// What a curve's fields may be, said when they're none of them.
pub const FORMS: &str = "`start` and `end` (with `arc` degrees or a `through` point), `points` (with `arcs` and `closed`), `centre` and `radius`, `centre` and `radii` (with `angle` and `span`), `control` (with `knots`), or `fit` (with `start_tangent` and `end_tangent`)";

impl Fields {
    /// The curve they give, or an error naming the fields of each shape when they make none or mix two.
    pub fn curve(&self) -> Result<Curve, String> {
        let f = self;
        let has = [
            ("start", f.start.is_some()),
            ("end", f.end.is_some()),
            ("through", f.through.is_some()),
            ("arc", f.arc.is_some()),
            ("points", f.points.is_some()),
            ("arcs", f.arcs.is_some()),
            ("closed", f.closed),
            ("centre", f.centre.is_some()),
            ("radius", f.radius.is_some()),
            ("radii", f.radii.is_some()),
            ("angle", f.angle.is_some()),
            ("span", f.span.is_some()),
            ("control", f.control.is_some()),
            ("knots", f.knots.is_some()),
            ("fit", f.fit.is_some()),
            ("start_tangent", f.start_tangent.is_some()),
            ("end_tangent", f.end_tangent.is_some()),
        ];
        let only = |allowed: &[&str]| -> Result<(), String> {
            match has.iter().find(|(k, on)| *on && !allowed.contains(k)) {
                Some((k, _)) => Err(format!("`{k}` doesn't go with the {} given: a line is {FORMS}", allowed.join(", "))),
                None => Ok(()),
            }
        };
        if let (Some(a), Some(b)) = (f.start, f.end) {
            only(&["start", "end", "through", "arc"])?;
            return match (f.through, f.arc) {
                (Some(_), Some(_)) => Err("give an arc's `through` point or its `arc` turn, not both".into()),
                (Some(t), None) => Ok(Curve::arc_through(a, b, t)),
                (None, arc) => Ok(Curve::arc(a, b, arc.unwrap_or(0.0))),
            };
        }
        if let Some(points) = &f.points {
            only(&["points", "arcs", "closed"])?;
            return Ok(Curve::Poly { points: points.clone(), arcs: tidy(f.arcs.clone().unwrap_or_default()), closed: f.closed });
        }
        if let (Some(centre), Some(radius)) = (f.centre, f.radius) {
            only(&["centre", "radius"])?;
            return Ok(Curve::Circle { centre, radius });
        }
        if let (Some(centre), Some(radii)) = (f.centre, f.radii) {
            only(&["centre", "radii", "angle", "span"])?;
            return Ok(Curve::Ellipse { centre, radii, angle: f.angle.unwrap_or(0.0), span: f.span });
        }
        if let Some(control) = &f.control {
            only(&["control", "knots"])?;
            return Ok(Curve::Spline { control: control.clone(), knots: f.knots.clone() });
        }
        if let Some(fit) = &f.fit {
            only(&["fit", "start_tangent", "end_tangent"])?;
            return Ok(Curve::Fit { points: fit.clone(), tangents: [f.start_tangent, f.end_tangent] });
        }
        Err(format!("a line is {FORMS}"))
    }

    /// A curve's fields.
    pub fn of(c: &Curve) -> Fields {
        match c {
            Curve::Poly { points, arcs, closed: false } if points.len() == 2 => Fields {
                start: points.first().copied(),
                end: points.get(1).copied(),
                arc: arcs.first().copied().filter(|a| *a != 0.0),
                ..Fields::default()
            },
            Curve::Poly { points, arcs, closed } => Fields {
                points: Some(points.clone()),
                arcs: Some(arcs.clone()).filter(|a| !a.is_empty()),
                closed: *closed,
                ..Fields::default()
            },
            Curve::Circle { centre, radius } => Fields { centre: Some(*centre), radius: Some(*radius), ..Fields::default() },
            Curve::Ellipse { centre, radii, angle, span } => Fields {
                centre: Some(*centre),
                radii: Some(*radii),
                angle: Some(*angle).filter(|a| *a != 0.0),
                span: *span,
                ..Fields::default()
            },
            Curve::Spline { control, knots } => Fields { control: Some(control.clone()), knots: knots.clone(), ..Fields::default() },
            Curve::Fit { points, tangents } => {
                Fields { fit: Some(points.clone()), start_tangent: tangents[0], end_tangent: tangents[1], ..Fields::default() }
            }
        }
    }
}

/// Fields that are a curve, for a type that holds one among others (`#[serde(flatten)]` won't take `deny_unknown_fields`):
/// `$name` is a struct of `$extra` fields and every curve field, read and written as one flat object, and `$into`
/// builds the type from it.
#[macro_export]
macro_rules! curve_struct {
    ($(#[$m:meta])* $name:ident { $($(#[$fm:meta])* $f:ident: $t:ty),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            $($(#[$fm])* pub $f: $t,)*
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub start: Option<$crate::geom::P2>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub end: Option<$crate::geom::P2>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub through: Option<$crate::geom::P2>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub arc: Option<f64>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub points: Option<Vec<$crate::geom::P2>>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub arcs: Option<Vec<f64>>,
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub closed: bool,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub centre: Option<$crate::geom::P2>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub radius: Option<f64>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub radii: Option<[f64; 2]>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub angle: Option<f64>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub span: Option<[f64; 2]>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub control: Option<Vec<$crate::geom::P2>>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub knots: Option<Vec<f64>>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub fit: Option<Vec<$crate::geom::P2>>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub start_tangent: Option<$crate::geom::P2>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub end_tangent: Option<$crate::geom::P2>,
        }

        impl $name {
            /// Its curve's fields.
            pub fn fields(&self) -> $crate::curves::Fields {
                $crate::curves::Fields {
                    start: self.start, end: self.end, through: self.through, arc: self.arc, points: self.points.clone(),
                    arcs: self.arcs.clone(), closed: self.closed, centre: self.centre, radius: self.radius, radii: self.radii,
                    angle: self.angle, span: self.span, control: self.control.clone(), knots: self.knots.clone(),
                    fit: self.fit.clone(), start_tangent: self.start_tangent, end_tangent: self.end_tangent,
                }
            }

            /// Its curve's fields set from a curve.
            pub fn set(&mut self, c: &$crate::curves::Curve) {
                let f = $crate::curves::Fields::of(c);
                (self.start, self.end, self.through, self.arc, self.points, self.arcs, self.closed) =
                    (f.start, f.end, f.through, f.arc, f.points, f.arcs, f.closed);
                (self.centre, self.radius, self.radii, self.angle, self.span) = (f.centre, f.radius, f.radii, f.angle, f.span);
                (self.control, self.knots, self.fit, self.start_tangent, self.end_tangent) =
                    (f.control, f.knots, f.fit, f.start_tangent, f.end_tangent);
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: P2, b: P2, tol: f64) -> bool {
        a.dist(b) <= tol
    }

    /// The furthest any point of `part` lies from `whole`.
    fn strays(part: &Curve, whole: &Curve) -> f64 {
        part.points().iter().map(|p| whole.nearest(*p).1).fold(0.0, f64::max)
    }

    #[test]
    fn a_line_an_arc_and_a_polyline_run_through_their_points() {
        let l = Curve::line(P2(0.0, 0.0), P2(1000.0, 0.0));
        assert_eq!((l.kind(), l.length(), l.middle()), ("line", 1000.0, P2(500.0, 0.0)));
        // A half circle from (0, 0) to (2000, 0) turning counter-clockwise bulges below, as a wall's does.
        let a = Curve::arc(P2(0.0, 0.0), P2(2000.0, 0.0), 180.0);
        assert_eq!(a.kind(), "arc");
        assert!((a.length() - std::f64::consts::PI * 1000.0).abs() < 1e-6);
        assert!(close(a.middle(), P2(1000.0, -1000.0), 1.0), "{:?}", a.middle());
        let t = Curve::arc_through(P2(0.0, 0.0), P2(2000.0, 0.0), P2(1000.0, 1000.0));
        assert!(close(t.middle(), P2(1000.0, 1000.0), 1.0), "{:?}", t.middle());
        assert!(a.points().iter().all(|p| (p.dist(P2(1000.0, 0.0)) - 1000.0).abs() < 1e-6));
        let poly = Curve::Poly { points: vec![P2(0.0, 0.0), P2(1000.0, 0.0), P2(1000.0, 1000.0)], arcs: vec![], closed: true };
        assert_eq!((poly.kind(), poly.is_closed(), poly.start(), poly.end()), ("polyline", true, P2(0.0, 0.0), P2(0.0, 0.0)));
        assert!((poly.length() - (2000.0 + 2f64.sqrt() * 1000.0)).abs() < 1e-6);
    }

    #[test]
    fn centre_ends_and_tangent_arcs_are_the_arcs_revit_draws() {
        let c = Curve::arc_centred(P2(0.0, 0.0), P2(1000.0, 0.0), P2(0.0, 500.0), true);
        assert!(close(c.end(), P2(0.0, 1000.0), 1e-6), "the end on the radius: {:?}", c.end());
        assert!(close(c.middle(), P2(1000.0 * 0.5f64.sqrt(), 1000.0 * 0.5f64.sqrt()), 1.0));
        let cw = Curve::arc_centred(P2(0.0, 0.0), P2(1000.0, 0.0), P2(0.0, 500.0), false);
        assert!((cw.length() - 1.5 * std::f64::consts::PI * 1000.0).abs() < 1e-3, "the other way round: {}", cw.length());
        // Leaving (0, 0) east, to (1000, 1000): a quarter circle about (0, 1000).
        let t = Curve::arc_tangent(P2(0.0, 0.0), P2(1.0, 0.0), P2(1000.0, 1000.0));
        assert!(t.points().iter().all(|p| (p.dist(P2(0.0, 1000.0)) - 1000.0).abs() < 1e-6));
        assert!(close(t.tangent(0.0), P2(1.0, 0.0), 1e-9), "{:?}", t.tangent(0.0));
    }

    #[test]
    fn circles_and_ellipses_whole_and_partial() {
        let c = Curve::Circle { centre: P2(0.0, 0.0), radius: 500.0 };
        assert!(c.is_closed() && close(c.start(), P2(500.0, 0.0), 1e-9) && close(c.end(), c.start(), 1e-9));
        assert!(c.points().windows(2).all(|w| geom::dist_to_segment(P2(0.0, 0.0), w[0], w[1]) >= 500.0 - FLAT - 1e-9));
        let e = Curve::Ellipse { centre: P2(0.0, 0.0), radii: [2000.0, 1000.0], angle: 90.0, span: None };
        assert!(close(e.start(), P2(0.0, 2000.0), 1e-6), "turned a quarter: {:?}", e.start());
        let half = Curve::Ellipse { centre: P2(0.0, 0.0), radii: [2000.0, 1000.0], angle: 0.0, span: Some([0.0, 180.0]) };
        assert_eq!(half.kind(), "partial ellipse");
        assert!(close(half.end(), P2(-2000.0, 0.0), 1e-6) && close(half.at(90.0), P2(0.0, 1000.0), 1e-6));
        // Ramanujan's perimeter, to the chords' millimetre.
        let (a, b) = (2000.0f64, 1000.0f64);
        let h = ((a - b) / (a + b)).powi(2);
        let perimeter = std::f64::consts::PI * (a + b) * (1.0 + 3.0 * h / (10.0 + (4.0 - 3.0 * h).sqrt()));
        assert!((e.length() - perimeter).abs() < 1.0, "{} vs {perimeter}", e.length());
    }

    #[test]
    fn a_control_point_spline_starts_and_ends_on_its_end_points_and_follows_the_others() {
        let s = Curve::Spline { control: vec![P2(0.0, 0.0), P2(1000.0, 2000.0), P2(3000.0, 2000.0), P2(4000.0, 0.0)], knots: None };
        assert!(close(s.start(), P2(0.0, 0.0), 1e-9) && close(s.end(), P2(4000.0, 0.0), 1e-9));
        // A single Bézier span: its middle is an eighth of the ends and three eighths of the inner points.
        assert!(close(s.at(0.5), P2(2000.0, 1500.0), 1e-9), "{:?}", s.at(0.5));
        let long =
            Curve::Spline { control: (0..7).map(|i| P2(i as f64 * 1000.0, if i % 2 == 0 { 0.0 } else { 1000.0 })).collect(), knots: None };
        assert!(long.check("s").is_ok(), "{:?}", long.check("s"));
        assert!(long.points().len() > 30);
    }

    #[test]
    fn a_control_point_spline_splits_into_two_exactly_the_same_curve() {
        let s = Curve::Spline {
            control: vec![P2(0.0, 0.0), P2(1000.0, 2000.0), P2(2500.0, -1000.0), P2(4000.0, 1500.0), P2(5000.0, 0.0), P2(6000.0, 800.0)],
            knots: None,
        };
        for t in [0.13, 0.3333, 0.5, 0.71] {
            let Some((a, Some(b))) = s.split(t) else { panic!("split at {t}") };
            assert!(close(a.end(), s.at(t), 1e-9) && close(b.start(), s.at(t), 1e-9), "they meet at the cut");
            assert!(close(a.start(), s.start(), 1e-9) && close(b.end(), s.end(), 1e-9));
            // Each half is the original over its part, to a micron, at the same parameters.
            for k in 0..=20 {
                let u = t * k as f64 / 20.0;
                assert!(close(a.at(u), s.at(u), 1e-6), "left at {u}: {:?} vs {:?}", a.at(u), s.at(u));
                let v = t + (1.0 - t) * k as f64 / 20.0;
                assert!(close(b.at(v), s.at(v), 1e-6), "right at {v}");
            }
            assert!(a.check("a").is_ok() && b.check("b").is_ok(), "{:?} {:?}", a.check("a"), b.check("b"));
            // And split again, still exact.
            let Some((c, Some(_))) = b.split(t + (1.0 - t) / 2.0) else { panic!("again") };
            assert!(strays(&c, &s) < 1e-6);
        }
    }

    #[test]
    fn a_fitted_spline_passes_through_its_points_and_its_halves_stay_fitted_and_within_tolerance() {
        let pts = vec![P2(0.0, 0.0), P2(1500.0, 1200.0), P2(3000.0, -800.0), P2(4200.0, 900.0), P2(6000.0, 0.0)];
        let f = Curve::Fit { points: pts.clone(), tangents: [None, None] };
        for p in &pts {
            assert!(f.nearest(*p).1 < 1e-6, "through {p:?}");
        }
        let given = Curve::Fit { points: pts.clone(), tangents: [Some(P2(0.0, 1.0)), Some(P2(0.0, -1.0))] };
        assert!(close(given.tangent(0.0), P2(0.0, 1.0), 1e-6), "leaving north: {:?}", given.tangent(0.0));
        for curve in [&f, &given] {
            let (t0, t1) = curve.domain();
            for k in [0.21, 0.5, 0.77] {
                let t = t0 + (t1 - t0) * k;
                let cut = curve.at(t);
                let Some((a, Some(b))) = curve.split(t) else { panic!("split") };
                let (Curve::Fit { points: pa, .. }, Curve::Fit { points: pb, .. }) = (&a, &b) else { panic!("still fitted: {a:?}") };
                assert!(close(a.end(), cut, 1e-9) && close(b.start(), cut, 1e-9), "they meet at the cut");
                assert!(pa.iter().chain(pb).all(|p| curve.nearest(*p).1 < 1e-6), "every point they pass is the original's");
                for (half, name) in [(&a, "before"), (&b, "after")] {
                    let off = strays(half, curve);
                    assert!(off <= FIT_TOL + FLAT, "{name} strays {off} mm");
                }
                // Each leaves the cut the way the original ran there.
                assert!(close(a.tangent(a.domain().1), curve.tangent(t), 1e-3) && close(b.tangent(0.0), curve.tangent(t), 1e-3));
            }
        }
    }

    #[test]
    fn polylines_split_at_a_segment_or_a_vertex_and_closed_curves_open_where_cut() {
        let p = Curve::Poly { points: vec![P2(0.0, 0.0), P2(1000.0, 0.0), P2(2000.0, 0.0)], arcs: vec![0.0, 90.0], closed: false };
        let Some((a, Some(b))) = p.split_at(P2(500.0, 10.0)) else { panic!("split") };
        assert!(close(a.end(), P2(500.0, 0.0), 1e-6));
        assert!(close(b.end(), P2(2000.0, 0.0), 1e-9) && (a.length() + b.length() - p.length()).abs() < 1e-6);
        // In the arc: the turn shared out.
        let mid = p.at(1.5);
        let Some((a, Some(b))) = p.split_at(mid) else { panic!("split in the arc") };
        assert!(strays(&a, &p) < 1e-6 && strays(&b, &p) < 1e-6 && (a.length() + b.length() - p.length()).abs() < 1e-6);
        // On a vertex: two polylines.
        let Some((a, Some(_))) = p.split_at(P2(1000.0, 0.0)) else { panic!("at the vertex") };
        assert_eq!(a, Curve::line(P2(0.0, 0.0), P2(1000.0, 0.0)));
        // At an end, nothing.
        assert!(p.split_at(P2(0.0, 0.0)).is_none());
        // A circle opens into two half circles, from the point back to it.
        let c = Curve::Circle { centre: P2(0.0, 0.0), radius: 1000.0 };
        let Some((o, None)) = c.split_at(P2(0.0, 1200.0)) else { panic!("opened") };
        assert!(close(o.start(), P2(0.0, 1000.0), 1e-4) && close(o.end(), o.start(), 1e-9) && strays(&o, &c) < 1e-4);
        let Some((h1, Some(h2))) = o.split_at(P2(0.0, -1000.0)) else { panic!("then halved") };
        assert!((h1.length() - h2.length()).abs() < 1e-6 && h1.kind() == "arc");
        let sq = polygon(P2(0.0, 0.0), P2(1000.0, 0.0), 4, true).unwrap_or_else(|| panic!("square"));
        let Some((open, None)) = sq.split_at(P2(500.0, 500.0)) else { panic!("square opened") };
        assert!((open.length() - sq.length()).abs() < 1e-6 && close(open.start(), P2(500.0, 500.0), 1e-6));
        let e = Curve::Ellipse { centre: P2(0.0, 0.0), radii: [2000.0, 1000.0], angle: 30.0, span: None };
        let Some((oe, None)) = e.split_at(e.at(70.0)) else { panic!("ellipse opened") };
        let Some((e1, Some(e2))) = oe.split_at(e.at(200.0)) else { panic!("ellipse halved") };
        assert!(strays(&e1, &e) < 1e-6 && strays(&e2, &e) < 1e-6 && (e1.length() + e2.length() - e.length()).abs() < 0.5);
    }

    #[test]
    fn moved_turned_and_mirrored_each_shape_lies_where_its_points_went() {
        let shapes = [
            Curve::arc(P2(0.0, 0.0), P2(2000.0, 0.0), 120.0),
            Curve::Circle { centre: P2(100.0, 200.0), radius: 700.0 },
            Curve::Ellipse { centre: P2(100.0, 200.0), radii: [2000.0, 900.0], angle: 25.0, span: Some([10.0, 200.0]) },
            Curve::Spline { control: vec![P2(0.0, 0.0), P2(1000.0, 2000.0), P2(3000.0, 2000.0), P2(4000.0, 0.0)], knots: None },
            Curve::Fit { points: vec![P2(0.0, 0.0), P2(1000.0, 800.0), P2(3000.0, -300.0)], tangents: [Some(P2(0.0, 1.0)), None] },
        ];
        let (s, c) = 0.6f64.sin_cos();
        let moves: [&dyn Fn(P2) -> P2; 3] =
            [&|p: P2| p + P2(500.0, -300.0), &|p: P2| P2(p.0 * c - p.1 * s, p.0 * s + p.1 * c), &|p: P2| P2(-p.0 + 100.0, p.1)];
        for shape in &shapes {
            for f in moves {
                let moved = shape.map(f);
                for (t, p) in shape.samples().iter().step_by(7) {
                    let _ = t;
                    assert!(moved.dist(f(*p)) < 0.5, "{} moved: {:?} is off it by {}", shape.kind(), f(*p), moved.dist(f(*p)));
                }
                if !matches!(shape, Curve::Circle { .. }) {
                    assert!(close(moved.start(), f(shape.start()), 1e-6), "{} starts where it started", shape.kind());
                }
            }
        }
    }

    #[test]
    fn grips_bend_an_arc_size_a_circle_and_turn_and_stretch_an_ellipse() {
        let arc = Curve::arc(P2(0.0, 0.0), P2(2000.0, 0.0), 90.0);
        let mid = arc.handles()[2];
        let bent = arc.moved(mid, P2(1000.0, -1000.0));
        assert!(
            bent.middle().dist(P2(1000.0, -1000.0)) < 1.0 && (bent.length() - std::f64::consts::PI * 1000.0).abs() < 1e-6,
            "a half circle now"
        );
        let c = Curve::Circle { centre: P2(0.0, 0.0), radius: 500.0 };
        assert_eq!(c.moved(P2(500.0, 0.0), P2(0.0, 800.0)), Curve::Circle { centre: P2(0.0, 0.0), radius: 800.0 });
        assert_eq!(c.moved(P2(0.0, 0.0), P2(10.0, 20.0)), Curve::Circle { centre: P2(10.0, 20.0), radius: 500.0 });
        let e = Curve::Ellipse { centre: P2(0.0, 0.0), radii: [2000.0, 1000.0], angle: 0.0, span: Some([0.0, 180.0]) };
        let turned = e.moved(P2(2000.0, 0.0), P2(0.0, 3000.0));
        assert_eq!(turned, Curve::Ellipse { centre: P2(0.0, 0.0), radii: [3000.0, 1000.0], angle: 90.0, span: Some([0.0, 180.0]) });
        let shorter = e.moved(P2(-2000.0, 0.0), P2(0.0, 1000.0));
        assert!(shorter.end().dist(P2(0.0, 1000.0)) < 1e-6, "its end where it was dragged: {shorter:?}");
        assert_eq!(
            e.moved(P2(0.0, 1000.0), P2(0.0, 400.0)),
            Curve::Ellipse { centre: P2(0.0, 0.0), radii: [2000.0, 400.0], angle: 0.0, span: Some([0.0, 180.0]) }
        );
    }

    #[test]
    fn vertices_come_and_go_a_spline_keeping_its_shape_when_one_is_added() {
        let poly = Curve::Poly { points: vec![P2(0.0, 0.0), P2(2000.0, 0.0), P2(2000.0, 2000.0)], arcs: vec![], closed: false };
        let more = poly.with_vertex(P2(1000.0, 10.0)).unwrap_or_else(|| panic!("added"));
        assert_eq!(more.vertices().len(), 4);
        assert!((more.length() - poly.length()).abs() < 1e-6);
        let fewer = more.without_vertex(P2(2000.0, 0.0)).unwrap_or_else(|| panic!("taken"));
        assert_eq!(fewer.vertices(), vec![P2(0.0, 0.0), P2(1000.0, 0.0), P2(2000.0, 2000.0)]);
        let ring =
            Curve::Poly { points: vec![P2(0.0, 0.0), P2(1000.0, 0.0), P2(1000.0, 1000.0), P2(0.0, 1000.0)], arcs: vec![], closed: true };
        let five = ring.with_vertex(P2(500.0, 1000.0)).unwrap_or_else(|| panic!("ring"));
        assert!(five.is_closed() && five.vertices().len() == 5 && (five.length() - 4000.0).abs() < 1e-6, "{five:?}");
        assert_eq!(ring.without_vertex(P2(1000.0, 1000.0)).map(|c| c.vertices().len()), Some(3));
        let spline = Curve::Spline {
            control: vec![P2(0.0, 0.0), P2(1000.0, 2000.0), P2(2500.0, -1000.0), P2(4000.0, 1500.0), P2(5000.0, 0.0)],
            knots: None,
        };
        let denser = spline.with_vertex(spline.at(0.4)).unwrap_or_else(|| panic!("knot"));
        assert_eq!(denser.vertices().len(), 6);
        for k in 0..=20 {
            let t = k as f64 / 20.0;
            assert!(denser.at(t).dist(spline.at(t)) < 1e-9, "the same curve at {t}");
        }
        assert!(spline.without_vertex(P2(2500.0, -1000.0)).is_some_and(|c| c.vertices().len() == 4));
        assert!(Curve::line(P2(0.0, 0.0), P2(10.0, 0.0)).without_vertex(P2(0.0, 0.0)).is_none(), "a line keeps its two ends");
    }

    #[test]
    fn splines_convert_both_ways_through_points_to_control_points_exactly() {
        let pts = vec![P2(0.0, 0.0), P2(1500.0, 1200.0), P2(3000.0, -800.0), P2(4200.0, 900.0), P2(6000.0, 0.0)];
        for tangents in [[None, None], [Some(P2(0.0, 1.0)), Some(P2(1.0, -1.0))], [Some(P2(1.0, 0.0)), None]] {
            let fit = Curve::Fit { points: pts.clone(), tangents };
            let ctrl = fit.converted().unwrap_or_else(|| panic!("converted"));
            assert_eq!(ctrl.kind(), "spline");
            assert_eq!(ctrl.domain(), fit.domain(), "the same parameters");
            let (t0, t1) = fit.domain();
            for k in 0..=50 {
                let t = t0 + (t1 - t0) * k as f64 / 50.0;
                assert!(ctrl.at(t).dist(fit.at(t)) < 1e-6, "{tangents:?} at {t}: {:?} vs {:?}", ctrl.at(t), fit.at(t));
            }
            assert!(ctrl.check("c").is_ok(), "{:?}", ctrl.check("c"));
            // And back: through points again, within the tolerance.
            let back = ctrl.converted().unwrap_or_else(|| panic!("back"));
            assert_eq!(back.kind(), "fitted spline");
            let off = back.points().iter().map(|p| ctrl.nearest(*p).1).fold(0.0, f64::max);
            assert!(off <= FIT_TOL + FLAT, "{off}");
        }
        let two = Curve::Fit { points: vec![P2(0.0, 0.0), P2(1000.0, 0.0)], tangents: [None, None] }.converted();
        assert!(two.is_some_and(|c| c.at(0.5 * 1000.0).dist(P2(500.0, 0.0)) < 1e-6), "two points: straight");
        assert!(Curve::Circle { centre: P2(0.0, 0.0), radius: 5.0 }.converted().is_none());
    }

    #[test]
    fn lines_and_arcs_join_into_a_polyline_and_explode_back() {
        let parts = [
            Curve::line(P2(1000.0, 0.0), P2(1000.0, 1000.0)),
            Curve::line(P2(0.0, 0.0), P2(1000.0, 0.0)),
            Curve::arc(P2(0.0, 1000.0), P2(1000.0, 1000.0), -90.0),
        ];
        let Some(j) = join(&parts) else { panic!("joined") };
        let Curve::Poly { points, arcs, closed } = &j else { panic!("a polyline") };
        assert_eq!((points.len(), *closed), (4, false));
        assert_eq!(arcs.iter().filter(|a| **a != 0.0).count(), 1);
        assert!((j.length() - parts.iter().map(Curve::length).sum::<f64>()).abs() < 1e-6);
        assert_eq!(explode(&j).len(), 3);
        // Closed when it comes round.
        let ring = [
            Curve::line(P2(0.0, 0.0), P2(1000.0, 0.0)),
            Curve::line(P2(1000.0, 0.0), P2(1000.0, 1000.0)),
            Curve::line(P2(0.0, 0.0), P2(1000.0, 1000.0)),
        ];
        assert!(join(&ring).is_some_and(|c| c.is_closed()));
        assert!(join(&[parts[0].clone(), Curve::line(P2(5000.0, 0.0), P2(6000.0, 0.0))]).is_none(), "apart");
        assert!(join(&[parts[0].clone(), Curve::Circle { centre: P2(0.0, 0.0), radius: 100.0 }]).is_none(), "a circle");
    }

    #[test]
    fn polygons_and_fillets() {
        let hex = polygon(P2(0.0, 0.0), P2(1000.0, 0.0), 6, true).unwrap_or_else(|| panic!("hex"));
        assert!((hex.length() - 6000.0).abs() < 1e-6, "inscribed: sides as long as the radius");
        let sq = polygon(P2(0.0, 0.0), P2(1000.0, 0.0), 4, false).unwrap_or_else(|| panic!("square"));
        assert!((sq.length() - 8000.0).abs() < 1e-6, "circumscribed: its sides touch the circle");
        let Some((p, q, arc)) = fillet(P2(-3000.0, 0.0), P2(0.0, 0.0), P2(0.0, 3000.0), 1000.0) else { panic!("fillet") };
        assert!(close(p, P2(-1000.0, 0.0), 1e-6) && close(q, P2(0.0, 1000.0), 1e-6));
        assert!(arc.points().iter().all(|x| (x.dist(P2(-1000.0, 1000.0)) - 1000.0).abs() < 1e-6));
        let r = fillet_radius(P2(-3000.0, 0.0), P2(0.0, 0.0), P2(0.0, 3000.0), arc.middle()).unwrap_or_default();
        assert!((r - 1000.0).abs() < 1.0, "{r}");
        assert!(fillet(P2(-300.0, 0.0), P2(0.0, 0.0), P2(0.0, 300.0), 1000.0).is_none(), "too short for it");
    }

    #[test]
    fn fields_read_each_shape_and_refuse_a_mix() {
        let read = |v: serde_json::Value| serde_json::from_value::<Fields>(v).map_err(|e| e.to_string()).and_then(|f| f.curve());
        let shapes = [
            serde_json::json!({"start": [0, 0], "end": [1000, 0]}),
            serde_json::json!({"start": [0, 0], "end": [1000, 0], "arc": 90}),
            serde_json::json!({"points": [[0, 0], [1000, 0], [1000, 1000]], "closed": true}),
            serde_json::json!({"centre": [0, 0], "radius": 500}),
            serde_json::json!({"centre": [0, 0], "radii": [800, 400], "angle": 30, "span": [0, 90]}),
            serde_json::json!({"control": [[0, 0], [1000, 1000], [2000, 0]]}),
            serde_json::json!({"fit": [[0, 0], [1000, 1000], [2000, 0]], "end_tangent": [0, -1]}),
        ];
        for v in shapes {
            let c = read(v.clone()).unwrap_or_else(|e| panic!("{v}: {e}"));
            assert!(c.check("it").is_ok(), "{v}: {:?}", c.check("it"));
            let back = serde_json::to_value(Fields::of(&c)).map_err(|e| e.to_string()).and_then(read);
            assert_eq!(back.as_ref(), Ok(&c), "{v} written as read");
        }
        let through =
            read(serde_json::json!({"start": [0, 0], "end": [2000, 0], "through": [1000, 1000]})).unwrap_or_else(|e| panic!("{e}"));
        assert!(close(through.middle(), P2(1000.0, 1000.0), 1.0));
        for bad in [
            serde_json::json!({"start": [0, 0], "end": [1000, 0], "radius": 5}),
            serde_json::json!({"centre": [0, 0]}),
            serde_json::json!({}),
            serde_json::json!({"points": [[0, 0]], "control": [[0, 0]]}),
        ] {
            assert!(read(bad.clone()).is_err(), "{bad}");
        }
        let checks = [
            (Curve::Circle { centre: P2(0.0, 0.0), radius: 0.5 }, "radius"),
            (Curve::Circle { centre: P2(f64::NAN, 0.0), radius: 50.0 }, "centre"),
            (Curve::Ellipse { centre: P2(0.0, 0.0), radii: [100.0, 100.0], angle: 0.0, span: Some([10.0, 10.0]) }, "span"),
            (Curve::Spline { control: vec![P2(0.0, 0.0), P2(10.0, 10.0), P2(20.0, 0.0)], knots: Some(vec![0.0, 1.0]) }, "knots"),
            (
                Curve::Spline {
                    control: vec![P2(0.0, 0.0), P2(10.0, 10.0), P2(20.0, 0.0)],
                    knots: Some(vec![0.0, 0.0, 0.5, 1.0, 1.0, 1.0]),
                },
                "first",
            ),
            (Curve::Fit { points: vec![P2(0.0, 0.0), P2(0.0, 0.0)], tangents: [None, None] }, "same"),
            (Curve::Fit { points: vec![P2(0.0, 0.0), P2(100.0, 0.0)], tangents: [Some(P2(0.0, 0.0)), None] }, "tangent"),
            (Curve::Poly { points: vec![P2(0.0, 0.0), P2(100.0, 0.0)], arcs: vec![400.0], closed: false }, "turns"),
            (Curve::Poly { points: vec![P2(0.0, 0.0), P2(100.0, 0.0)], arcs: vec![10.0, 10.0], closed: false }, "segments"),
        ];
        for (c, says) in checks {
            let e = c.check("it").err().unwrap_or_default();
            assert!(e.contains(says), "{c:?}: {e}");
        }
    }
}
