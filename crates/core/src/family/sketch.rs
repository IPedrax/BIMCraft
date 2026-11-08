//! A form's sketch (F5): the lines and arcs a form's profile is drawn with, on its host plane, in that plane's own axes, as
//! Revit's Create Extrusion and Create Revolve sketch them. Each line is a part of its family, with an id and a GUID of its
//! own, so a dimension or a lock (F7) can name it. Its numbers are numbers or formulas over the family's parameters, as a
//! form's other numbers are: what a sketch is drawn with on screen is numbers, and a profile from before sketches (its
//! corners) keeps its formulas as lines.
//!
//! A form is built from its sketch's lines as the project's sketches are ([`crate::sketch::solve`]): joined end to end into
//! loops, an outline with its holes, an island in a hole an outline again; every loop is swept (or turned), so several loops
//! make one form of several solids. An arc is cut into chords as it is built, [`SEGMENTS`] to a whole turn. A revolve's axis is
//! a line of its sketch too, marked `axis`: one at most.
//!
//! In a file and a command a line is one flat object: `{id?, guid?, start: [a, b], end: [a, b], turn?}` (an arc turning
//! `turn` degrees counter-clockwise from its start to its end), `{id?, guid?, centre: [a, b], radius}` (a whole circle), or
//! a straight one with `axis: true`. A form's `profile`, `circle` and `axis`, the shorthands from before sketches, are read
//! as such lines ([`profile_lines`]).

use serde::{Deserialize, Serialize};

use super::{Circle, Expr, Form};
use crate::geom::{P2, Region};
use crate::ids::{Guid, Id};
use crate::model::TOL;
use crate::sketch::{Problem, SketchLine};
use crate::walls::Axis;

/// Chords a whole turn of an arc is built with: a circle's, as before sketches.
pub const SEGMENTS: usize = super::SEGMENTS;
/// Lines one form's sketch may have.
pub const MAX_SKETCH: usize = 1000;

/// A line of a form's sketch: its id and GUID, what it is, and whether it is a revolve's axis.
#[derive(Clone, Debug, PartialEq)]
pub struct FormLine {
    pub id: Id,
    pub guid: Guid,
    pub curve: FormCurve,
    /// A revolve's axis: the line its profile turns about, not a line of the profile.
    pub axis: bool,
}

/// What a sketch line is: from one end to the other, straight or turning; or a whole circle.
#[derive(Clone, Debug, PartialEq)]
pub enum FormCurve {
    /// From `start` to `end`; with a `turn`, an arc turning so many degrees counter-clockwise (clockwise when negative).
    Line {
        start: [Expr; 2],
        end: [Expr; 2],
        turn: Option<Expr>,
    },
    Circle {
        centre: [Expr; 2],
        radius: Expr,
    },
}

/// A sketch line as a file and a command give it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFormLine {
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    id: Id,
    #[serde(default, skip_serializing_if = "Guid::is_nil")]
    guid: Guid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    start: Option<[Expr; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    end: Option<[Expr; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    turn: Option<Expr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    centre: Option<[Expr; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    radius: Option<Expr>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    axis: bool,
}

impl TryFrom<RawFormLine> for FormLine {
    type Error = String;
    fn try_from(r: RawFormLine) -> Result<FormLine, String> {
        let curve = match (r.start, r.end, r.centre, r.radius) {
            (Some(start), Some(end), None, None) => FormCurve::Line { start, end, turn: r.turn },
            (None, None, Some(centre), Some(radius)) if r.turn.is_none() => FormCurve::Circle { centre, radius },
            _ => return Err("a sketch line takes `start` and `end` (and a `turn` for an arc), or a circle's `centre` and `radius`".into()),
        };
        if r.axis && !matches!(curve, FormCurve::Line { turn: None, .. }) {
            return Err("a revolve's `axis` is a straight line: `start` and `end`".into());
        }
        Ok(FormLine { id: r.id, guid: r.guid, curve, axis: r.axis })
    }
}

impl From<FormLine> for RawFormLine {
    fn from(l: FormLine) -> RawFormLine {
        let mut r = RawFormLine { id: l.id, guid: l.guid, axis: l.axis, ..RawFormLine::default() };
        match l.curve {
            FormCurve::Line { start, end, turn } => (r.start, r.end, r.turn) = (Some(start), Some(end), turn),
            FormCurve::Circle { centre, radius } => (r.centre, r.radius) = (Some(centre), Some(radius)),
        }
        r
    }
}

impl Serialize for FormLine {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RawFormLine::from(self.clone()).serialize(s)
    }
}

impl<'de> Deserialize<'de> for FormLine {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<FormLine, D::Error> {
        FormLine::try_from(RawFormLine::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

impl crate::library::HasPart for FormLine {
    fn ident(&mut self) -> (&mut Id, &mut Guid) {
        (&mut self.id, &mut self.guid)
    }
}

impl FormLine {
    /// A straight line, with no id yet.
    pub fn line(start: [Expr; 2], end: [Expr; 2]) -> FormLine {
        FormLine { id: 0, guid: Guid::default(), curve: FormCurve::Line { start, end, turn: None }, axis: false }
    }

    /// Its numbers, each once.
    pub fn exprs(&self) -> Vec<&Expr> {
        match &self.curve {
            FormCurve::Line { start, end, turn } => start.iter().chain(end).chain(turn).collect(),
            FormCurve::Circle { centre, radius } => centre.iter().chain([radius]).collect(),
        }
    }

    /// Its numbers, each once, to change.
    pub fn exprs_mut(&mut self) -> Vec<&mut Expr> {
        match &mut self.curve {
            FormCurve::Line { start, end, turn } => start.iter_mut().chain(end.iter_mut()).chain(turn.iter_mut()).collect(),
            FormCurve::Circle { centre, radius } => centre.iter_mut().chain([radius]).collect(),
        }
    }

    /// Its points (its ends, a circle's centre) mapped by `f`; a mirroring `f` (`flip`) turns an arc the other way.
    pub fn map_points(&mut self, f: impl Fn(&[Expr; 2]) -> [Expr; 2], flip: bool) {
        match &mut self.curve {
            FormCurve::Line { start, end, turn } => {
                (*start, *end) = (f(start), f(end));
                if flip && let Some(t) = turn {
                    *t = t.negated();
                }
            }
            FormCurve::Circle { centre, .. } => *centre = f(centre),
        }
    }

    /// It as the project's sketches draw it, its numbers read by `num`: a line, an arc, or a circle. Refused where a number
    /// can't be read, or what it comes to is no line.
    pub fn curve<'a>(&'a self, num: &mut dyn FnMut(&'a Expr) -> Result<f64, String>) -> Result<SketchLine, String> {
        let pt = |p: &'a [Expr; 2], num: &mut dyn FnMut(&'a Expr) -> Result<f64, String>| -> Result<P2, String> {
            Ok(P2(num(&p[0])?, num(&p[1])?))
        };
        let curve = match &self.curve {
            FormCurve::Line { start, end, turn } => {
                let (a, b) = (pt(start, num)?, pt(end, num)?);
                let turn = match turn {
                    Some(t) => num(t)?,
                    None => 0.0,
                };
                if !(turn.is_finite() && turn.abs() < 360.0) {
                    return Err(format!("an arc turns less than a whole turn either way, not {turn} degrees"));
                }
                let arcs = if turn == 0.0 { Vec::new() } else { vec![turn] };
                crate::curves::Curve::Poly { points: vec![a, b], arcs, closed: false }
            }
            FormCurve::Circle { centre, radius } => {
                let c = pt(centre, num)?;
                let r = num(radius)?;
                if !(r.is_finite() && r > 0.0) {
                    return Err(format!("a circle's radius is {r}; it needs one above zero"));
                }
                crate::curves::Curve::Circle { centre: c, radius: r }
            }
        };
        Ok(SketchLine { id: self.id, guid: self.guid, ..SketchLine::from(curve) })
    }
}

/// The ends and turn of a straight line or an arc, or a circle's centre and radius: what [`FormLine::curve`] gives, read
/// back as sketch lines are drawn. None for what a form's sketch doesn't hold (an ellipse, a spline, a polyline of more
/// than one piece).
pub fn line_of(l: &SketchLine) -> Option<FormLine> {
    use crate::curves::Curve;
    let n = |x: f64| Expr::Number(x);
    let pt = |p: P2| [n(p.0), n(p.1)];
    let curve = match &l.curve {
        Curve::Poly { points, arcs, closed: false } if points.len() == 2 => {
            let turn = arcs.first().copied().filter(|t| *t != 0.0);
            FormCurve::Line { start: pt(points[0]), end: pt(points[1]), turn: turn.map(n) }
        }
        Curve::Circle { centre, radius } => FormCurve::Circle { centre: pt(*centre), radius: n(*radius) },
        _ => return None,
    };
    Some(FormLine { id: l.id, guid: l.guid, curve, axis: false })
}

/// Sketch lines as a form's sketch holds them: each polyline cut into its pieces (the first keeping the line's id and GUID),
/// an ellipse or a spline left out (a form's sketch has none).
pub fn lines_of(lines: &[SketchLine]) -> Vec<FormLine> {
    let mut out = Vec::new();
    for l in lines {
        match line_of(l) {
            Some(f) => out.push(f),
            None => {
                for (k, piece) in crate::curves::explode(&l.curve).into_iter().enumerate() {
                    let sl = SketchLine::from(piece);
                    if let Some(mut f) = line_of(&sl) {
                        if k == 0 {
                            (f.id, f.guid) = (l.id, l.guid);
                        }
                        out.push(f);
                    }
                }
            }
        }
    }
    out
}

/// A profile's corners, in turn, as a closed loop of straight lines, the last back to the first.
pub fn profile_lines(corners: &[[Expr; 2]]) -> Vec<FormLine> {
    let n = corners.len();
    (0..n).map(|i| FormLine::line(corners[i].clone(), corners[(i + 1) % n].clone())).collect()
}

/// A circle as a sketch line.
pub fn circle_line(c: &Circle) -> FormLine {
    FormLine { id: 0, guid: Guid::default(), curve: FormCurve::Circle { centre: c.centre.clone(), radius: c.radius.clone() }, axis: false }
}

/// A revolve's axis as a sketch line.
pub fn axis_line(axis: &[[Expr; 2]; 2]) -> FormLine {
    FormLine { axis: true, ..FormLine::line(axis[0].clone(), axis[1].clone()) }
}

/// `new` (lines drawn on screen, by sketch mode) with what `old` held where they are the same: a line that keeps the GUID
/// it had and comes, read by `num`, to where it was (within [`TOL`]) keeps its formulas; a line moved or new holds numbers.
pub fn kept(old: &[FormLine], new: Vec<FormLine>, num: &mut dyn FnMut(&Expr) -> Result<f64, String>) -> Vec<FormLine> {
    let mut same = |a: &FormLine, b: &FormLine| -> bool {
        let (Ok(x), Ok(y)) = (a.curve(num), b.curve(num)) else { return false };
        let (px, py) = (x.curve.handles(), y.curve.handles());
        matches!(&x.curve, crate::curves::Curve::Circle { .. }) == matches!(&y.curve, crate::curves::Curve::Circle { .. })
            && px.len() == py.len()
            && px.iter().zip(&py).all(|(p, q)| p.dist(*q) <= TOL)
            && (x.curve.length() - y.curve.length()).abs() <= TOL
    };
    new.into_iter()
        .map(|l| match old.iter().find(|o| !o.guid.is_nil() && o.guid == l.guid && o.axis == l.axis) {
            Some(o) if same(o, &l) => o.clone(),
            _ => l,
        })
        .collect()
}

impl Form {
    /// The lines of its profile: all of its sketch but its axis.
    pub fn boundary(&self) -> impl Iterator<Item = &FormLine> {
        self.sketch.iter().filter(|l| !l.axis)
    }

    /// Its axis' line, when it is a revolve.
    pub fn axis_line(&self) -> Option<&FormLine> {
        self.sketch.iter().find(|l| l.axis)
    }

    /// The ends of its axis, when it is a revolve.
    pub fn axis(&self) -> Option<[[Expr; 2]; 2]> {
        self.axis_ends().map(|[a, b]| [a.clone(), b.clone()])
    }

    /// The ends of its axis, as it holds them.
    pub fn axis_ends(&self) -> Option<[&[Expr; 2]; 2]> {
        match self.axis_line().map(|l| &l.curve) {
            Some(FormCurve::Line { start, end, .. }) => Some([start, end]),
            _ => None,
        }
    }

    /// Whether it is a revolve: its sketch has an axis.
    pub fn revolves(&self) -> bool {
        self.axis_line().is_some()
    }

    /// Its profile's corners in turn, when its profile is one closed loop of straight lines, each starting where the one
    /// before it ended, as a profile from before sketches is (and as the Forms tab's corners type one).
    pub fn corners(&self) -> Option<Vec<[Expr; 2]>> {
        Some(self.corner_refs()?.into_iter().cloned().collect())
    }

    /// Its corners as it holds them, as [`Form::corners`] finds them.
    fn corner_refs(&self) -> Option<Vec<&[Expr; 2]>> {
        let lines: Vec<&FormLine> = self.boundary().collect();
        if lines.is_empty() {
            return None;
        }
        let mut out = Vec::with_capacity(lines.len());
        for (i, l) in lines.iter().enumerate() {
            let FormCurve::Line { start, end, turn: None } = &l.curve else { return None };
            let FormCurve::Line { start: next, .. } = &lines[(i + 1) % lines.len()].curve else { return None };
            if end != next {
                return None;
            }
            out.push(start);
        }
        Some(out)
    }

    /// Its profile's circle, when its profile is one circle.
    pub fn circle(&self) -> Option<Circle> {
        self.circle_ref().map(|(centre, radius)| Circle { centre: centre.clone(), radius: radius.clone() })
    }

    /// Its circle's centre and radius as it holds them.
    pub(crate) fn circle_ref(&self) -> Option<(&[Expr; 2], &Expr)> {
        let mut lines = self.boundary();
        match (lines.next().map(|l| &l.curve), lines.next()) {
            (Some(FormCurve::Circle { centre, radius }), None) => Some((centre, radius)),
            _ => None,
        }
    }

    /// Its profile made these corners, a closed loop of straight lines: the lines it had keep their ids and GUIDs by place.
    pub fn set_corners(&mut self, corners: &[[Expr; 2]]) {
        self.set_boundary(profile_lines(corners));
    }

    /// Its profile made this circle, its line keeping the id and GUID of its profile's first.
    pub fn set_circle(&mut self, c: &Circle) {
        self.set_boundary(vec![circle_line(c)]);
    }

    /// Its profile's lines made `lines`, each without an id taking the one at its place before; its axis stays.
    pub fn set_boundary(&mut self, mut lines: Vec<FormLine>) {
        let mut old: Vec<FormLine> = self.boundary().cloned().collect();
        crate::library::keep_parts(&mut old, &mut lines);
        let axis = self.axis_line().cloned();
        self.sketch = lines;
        self.sketch.extend(axis);
    }

    /// Its axis made `axis` (none, no longer a revolve), its line keeping the id and GUID it had.
    pub fn set_axis(&mut self, axis: Option<&[[Expr; 2]; 2]>) {
        let was = self.axis_line().cloned();
        self.sketch.retain(|l| !l.axis);
        if let Some(a) = axis {
            let mut line = axis_line(a);
            if let Some(w) = was {
                (line.id, line.guid) = (w.id, w.guid);
            }
            self.sketch.push(line);
        }
    }

    /// Its sketch's lines without an id of their own taking those of `old`'s by place, its profile's from its profile's and
    /// its axis' from its axis': a form given again keeps its lines.
    pub fn keep_lines(&mut self, old: &Form) {
        for axis in [false, true] {
            let mut was = old.sketch.iter().filter(|l| l.axis == axis);
            for l in self.sketch.iter_mut().filter(|l| l.axis == axis) {
                let Some(o) = was.next() else { break };
                if crate::ids::unset(&l.id) && l.guid.is_nil() {
                    (l.id, l.guid) = (o.id, o.guid);
                }
            }
        }
    }

    /// Its sketch's numbers, each once.
    pub fn sketch_exprs(&self) -> impl Iterator<Item = &Expr> {
        self.sketch.iter().flat_map(FormLine::exprs)
    }

    /// The loops its profile makes, read by `num` for a type: each outline counter-clockwise with its holes clockwise, the
    /// first drawn first; an arc cut into chords. Refused, saying why: a number it can't read, more lines than it may have,
    /// and the problems [`crate::sketch::solve`] names (lines that don't close, cross or overlap), each line by its id.
    pub fn loops<'a>(&'a self, num: &mut dyn FnMut(&'a Expr) -> Result<f64, String>) -> Result<Vec<Region>, String> {
        let lines: Vec<&'a FormLine> = self.boundary().collect();
        if self.sketch.len() > MAX_SKETCH {
            return Err(format!("its sketch has {} lines, more than {MAX_SKETCH}", self.sketch.len()));
        }
        if self.sketch.iter().filter(|l| l.axis).count() > 1 {
            return Err("its sketch has more than one axis: a revolve turns about one".into());
        }
        // A profile from before sketches (one loop of straight lines, or a circle) is built as it was then.
        if let Some((centre, radius)) = self.circle_ref() {
            let (x, y, r) = (num(&centre[0])?, num(&centre[1])?, num(radius)?);
            if r <= 0.0 {
                return Err(format!("its circle's radius is {r}; it needs one above zero"));
            }
            let ring = (0..SEGMENTS)
                .map(|k| (k as f64 * std::f64::consts::TAU / SEGMENTS as f64).sin_cos())
                .map(|(s, c)| P2(x + r * c, y + r * s))
                .collect();
            return Ok(vec![Region { outer: ring, holes: Vec::new() }]);
        }
        if let Some(corners) = self.corner_refs() {
            let ring: Vec<P2> = corners.iter().map(|[a, b]| Ok(P2(num(a)?, num(b)?))).collect::<Result<_, String>>()?;
            let flat = ring.len() < 3 || crate::geom::area(&ring).abs() < 1.0;
            let ring: Vec<P2> = if crate::geom::area(&ring) < 0.0 { ring.into_iter().rev().collect() } else { ring };
            if !flat && super::cap(&ring).is_ok() {
                return Ok(vec![Region { outer: ring, holes: Vec::new() }]);
            }
            // A loop that crosses itself is named where, as any sketch's lines are; one that only folds flat has no area.
            let crossing = match crate::sketch::solve(&lines.iter().map(|l| l.curve(num)).collect::<Result<Vec<_>, _>>()?) {
                Ok(_) => false,
                Err(problems) => problems.iter().any(|p| matches!(p, Problem::Cross { .. })),
            };
            if flat && !crossing {
                return Err("its profile has no area: it needs three corners or more, or a circle".into());
            }
            if !crossing {
                return Err("its profile crosses itself".into());
            }
        }
        if lines.is_empty() {
            return Err("its sketch has no lines: draw its profile".into());
        }
        let mut drawn = Vec::with_capacity(lines.len());
        for l in &lines {
            drawn.push(chorded(&l.curve(num)?));
        }
        let solved = crate::sketch::solve(&drawn).map_err(|problems| {
            let named: Vec<SketchLine> =
                lines.iter().map(|l| SketchLine { id: l.id, ..SketchLine::new(P2(0.0, 0.0), P2(1.0, 0.0)) }).collect();
            crate::sketch::say(&problems, &crate::sketch::namer(&named))
        })?;
        Ok(solved
            .regions
            .into_iter()
            .map(|r| {
                let ccw = |ring: Vec<P2>, want: bool| {
                    if (crate::geom::area(&ring) > 0.0) == want { ring } else { ring.into_iter().rev().collect() }
                };
                Region { outer: ccw(r.outer, true), holes: r.holes.into_iter().map(|h| ccw(h, false)).collect() }
            })
            .collect())
    }
}

/// Sketch lines read off screen checked as Finish Edit Mode checks them, before any form is made of them: what
/// [`crate::sketch::solve`] finds wrong (lines that don't close into loops, cross or overlap, no lines at all), with each
/// line's place in `lines`; then, for a revolve, its `axis` (none, more than one) and loops that cross it.
pub fn finish_check(lines: &[SketchLine], axis: Option<[P2; 2]>, revolve: bool) -> Result<(), Finish> {
    let drawn: Vec<SketchLine> = lines.iter().map(chorded).collect();
    let solved = crate::sketch::solve(&drawn).map_err(Finish::Problems)?;
    if revolve {
        let [a, b] = axis.ok_or(Finish::NoAxis)?;
        if a.dist(b) <= TOL {
            return Err(Finish::NoAxis);
        }
        let left = (b - a).unit().perp();
        let sides: Vec<f64> = solved
            .regions
            .iter()
            .flat_map(|r| r.outer.iter().chain(r.holes.iter().flatten()))
            .map(|p| (*p - a).dot(left))
            .map(|d| if d.abs() <= TOL { 0.0 } else { d })
            .collect();
        if sides.iter().any(|d| *d < 0.0) && sides.iter().any(|d| *d > 0.0) {
            return Err(Finish::CrossesAxis);
        }
    }
    Ok(())
}

/// Why Finish Edit Mode can't make a form of a sketch.
#[derive(Clone, Debug, PartialEq)]
pub enum Finish {
    /// The lines don't make loops, as [`crate::sketch::solve`] says.
    Problems(Vec<Problem>),
    /// A revolve with no axis line.
    NoAxis,
    /// A revolve's profile on both sides of its axis.
    CrossesAxis,
}

/// A line or an arc as straight pieces, an arc's [`SEGMENTS`] to a whole turn (at least one), a circle's all round: what a
/// form is built of, and what its loops are found from. It keeps its id and GUID.
fn chorded(l: &SketchLine) -> SketchLine {
    use crate::curves::Curve;
    let steps = |turn: f64| ((turn.abs() / 360.0 * SEGMENTS as f64) - 1e-9).ceil().max(1.0) as usize;
    let curve = match &l.curve {
        Curve::Poly { points, arcs, closed: false } if points.len() == 2 && arcs.first().is_some_and(|t| *t != 0.0) => {
            let ax = Axis::new(points[0], points[1], arcs[0]);
            let n = steps(arcs[0]);
            let len = ax.length();
            let mut pts: Vec<P2> = (0..n).map(|k| ax.at(len * k as f64 / n as f64, 0.0)).collect();
            pts[0] = points[0];
            pts.push(points[1]);
            Curve::Poly { points: pts, arcs: Vec::new(), closed: false }
        }
        Curve::Circle { centre, radius } => {
            let ring = (0..SEGMENTS)
                .map(|k| (k as f64 * std::f64::consts::TAU / SEGMENTS as f64).sin_cos())
                .map(|(s, c)| P2(centre.0 + radius * c, centre.1 + radius * s))
                .collect();
            Curve::Poly { points: ring, arcs: Vec::new(), closed: true }
        }
        other => other.clone(),
    };
    SketchLine { curve, ..l.clone() }
}
