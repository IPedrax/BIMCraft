//! A family's dimensions and the constraints they hold (F7a), as Revit's family editor has them: an aligned dimension between
//! two or more parallel references (reference planes, an extrusion's start or end face) drawn in one of the family's views,
//! its segments labelled with a length parameter, locked (Revit's padlock) or kept equal (EQ); and the lock Align leaves, a
//! dimension of one segment locked at 0. Flexing a type solves them ([`Family::solve`]): each plane a constraint touches may
//! move along its own normal, each extrusion end one touches may move along its host's, and they take the least change from
//! where they are drawn that holds every constraint. The rows are linear with coefficients that never depend on the values,
//! so a constraint that adds nothing to those before it is refused for every type alike (over-constrained), and a family
//! that is accepted solves for any values but those that can't stand (a labelled length of 0 or less). Nothing solved is
//! stored: [`Family::flexed`] works it out from the values each time the family is built, so a type's geometry depends on
//! its values alone, never on the type flexed before it.

use super::sketch::{FormCurve, FormLine};
use super::*;
use crate::ids::{Guid, Id};
use crate::model::{Lock, TOL};
use crate::view3d::{dot, sub};

mod pivot;
pub use pivot::{MAX_SWING, Pivot, PivotAt, PivotFrom, PivotMark, PivotPoint, Pivoted, Rotation, pivot_marks, pivot_targets};

/// Dimensions one family may hold, and references one dimension may measure between.
pub const MAX_DIMENSIONS: usize = 1000;
pub const MAX_DIM_REFS: usize = 64;
/// Unknowns one connected group of constraints may hold.
const MAX_UNKNOWNS: usize = 512;
/// How much arithmetic one solve, or one check of every type, may do: past it, refused.
const BUDGET: u64 = 200_000_000;
/// How far apart two directions may be and still be parallel, as the sine of their angle.
const PARALLEL: f64 = 1e-6;
/// A row left this much of its length once the rows before it are taken out adds nothing to them.
const DEPENDENT: f64 = 1e-9;
/// How many steps a non-linear solve takes at most, and how many times one step may be halved back (F7c).
const MAX_STEPS: usize = 50;
const MAX_HALVINGS: usize = 20;
/// A labelled or locked angle is from 0 up to this, in degrees.
pub const MAX_ANGLE: f64 = 180.0;
/// What Revit says when constraints can't hold: the desktop's dialog title, and the start of the core's messages for it.
pub const NOT_SATISFIED: &str = "Constraints are not satisfied";
/// What the `family` command says of dimensions that measure to what the family hasn't: the desktop asks Delete
/// Dimension(s) or Cancel on seeing it (T14).
pub const INVALID_DIMENSIONS: &str = "measure to references the family no longer has";

/// The family view a dimension is drawn in, as [`views::Which`] names them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimView {
    #[default]
    Plan,
    Ceiling,
    Front,
    Back,
    Left,
    Right,
}

impl DimView {
    pub fn which(self) -> views::Which {
        use views::{Side, Which};
        match self {
            DimView::Plan => Which::Plan,
            DimView::Ceiling => Which::Ceiling,
            DimView::Front => Which::Elevation(Side::Front),
            DimView::Back => Which::Elevation(Side::Back),
            DimView::Left => Which::Elevation(Side::Left),
            DimView::Right => Which::Elevation(Side::Right),
        }
    }

    pub fn of(which: views::Which) -> DimView {
        use views::{Side, Which};
        match which {
            Which::Plan => DimView::Plan,
            Which::Ceiling => DimView::Ceiling,
            Which::Elevation(Side::Front) => DimView::Front,
            Which::Elevation(Side::Back) => DimView::Back,
            Which::Elevation(Side::Left) => DimView::Left,
            Which::Elevation(Side::Right) => DimView::Right,
        }
    }

    /// The way the view looks, in the family's axes: what a dimension drawn in it may not measure along.
    pub fn eye(self) -> P3 {
        match self.which() {
            views::Which::Plan | views::Which::Ceiling => [0.0, 0.0, 1.0],
            views::Which::Elevation(side) => {
                let l = side.looking();
                [l.0, l.1, 0.0]
            }
        }
    }
}

/// Which end of an extrusion: its `from` (Revit's Extrusion Start) or its `to` (Extrusion End).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Extent {
    Start,
    End,
}

/// A point of a form's sketch line (F7b): a line's or an arc's start or end, or a circle's centre.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Spot {
    Start,
    End,
    Centre,
}

impl Spot {
    fn said(self) -> &'static str {
        match self {
            Spot::Start => "start",
            Spot::End => "end",
            Spot::Centre => "centre",
        }
    }
}

/// What a dimension measures to: a reference plane (a datum or one of the family's own), an extrusion's start or end face, a
/// straight line of a form's sketch (the plane through it square to the form's host, as Revit's sketch line seen edge on;
/// a circle, for a radial or diameter dimension), a point of a form's sketch (along the way the dimension's other references
/// measure), or a face of a reference line. In JSON `{"plane": guid}`, `{"form": guid, "end": "start" | "end"}`,
/// `{"form": guid, "line": guid}`, `{"form": guid, "line": guid, "point": "start" | "end" | "centre"}` or `{"line": guid,
/// "face": "horizontal" | "vertical" | "start" | "end"}`; an id reads as the one it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RawDimRef", into = "RawDimRef")]
pub enum DimRef {
    Plane(Guid),
    Form(Guid, Extent),
    /// A line of a form's sketch: the form's GUID, the line's.
    Sketch(Guid, Guid),
    /// A point of a line of a form's sketch: the form's GUID, the line's, which point.
    Point(Guid, Guid, Spot),
    /// A face of a reference line.
    Line(Guid, Face),
    /// A reference line's start or end (F7c): the line's GUID, which end.
    LinePoint(Guid, Spot),
}

impl DimRef {
    /// Whether it names `g`: as its plane, its form, its form's sketch line or its reference line.
    pub fn names(&self, g: Guid) -> bool {
        match *self {
            DimRef::Plane(a) | DimRef::Form(a, _) | DimRef::Line(a, _) | DimRef::LinePoint(a, _) => a == g,
            DimRef::Sketch(f, l) | DimRef::Point(f, l, _) => f == g || l == g,
        }
    }

    /// Whether it is a point, with no direction of its own.
    pub fn is_point(&self) -> bool {
        matches!(self, DimRef::Point(..) | DimRef::LinePoint(..))
    }
}

impl std::fmt::Display for DimRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DimRef::Plane(g) => write!(f, "plane {g}"),
            DimRef::Form(g, Extent::Start) => write!(f, "form {g}'s start"),
            DimRef::Form(g, Extent::End) => write!(f, "form {g}'s end"),
            DimRef::Sketch(g, l) => write!(f, "form {g}'s line {l}"),
            DimRef::Point(g, l, s) => write!(f, "form {g}'s line {l}'s {}", s.said()),
            DimRef::Line(g, face) => write!(f, "line {g}'s {} face", face_said(*face)),
            DimRef::LinePoint(g, s) => write!(f, "line {g}'s {}", s.said()),
        }
    }
}

fn face_said(face: Face) -> &'static str {
    match face {
        Face::Horizontal => "horizontal",
        Face::Vertical => "vertical",
        Face::Start => "start",
        Face::End => "end",
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDimRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plane: Option<Guid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    form: Option<Guid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    end: Option<Extent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    line: Option<Guid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    point: Option<Spot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    face: Option<Face>,
}

impl TryFrom<RawDimRef> for DimRef {
    type Error = String;
    fn try_from(r: RawDimRef) -> Result<DimRef, String> {
        match (r.plane, r.form, r.end, r.line, r.point, r.face) {
            (Some(g), None, None, None, None, None) => Ok(DimRef::Plane(g)),
            (None, Some(g), Some(e), None, None, None) => Ok(DimRef::Form(g, e)),
            (None, Some(g), None, Some(l), None, None) => Ok(DimRef::Sketch(g, l)),
            (None, Some(g), None, Some(l), Some(s), None) => Ok(DimRef::Point(g, l, s)),
            (None, None, None, Some(l), None, Some(face)) => Ok(DimRef::Line(l, face)),
            (None, None, None, Some(l), Some(s), None) => Ok(DimRef::LinePoint(l, s)),
            _ => Err(
                "a dimension's reference is {\"plane\": guid}, {\"form\": guid, \"end\": \"start\" | \"end\"}, {\"form\": guid, \"line\": \
                      guid}, {\"form\": guid, \"line\": guid, \"point\": \"start\" | \"end\" | \"centre\"}, {\"line\": guid, \"face\": \
                      \"horizontal\" | \"vertical\" | \"start\" | \"end\"} or {\"line\": guid, \"point\": \"start\" | \"end\"}"
                    .into(),
            ),
        }
    }
}

impl From<DimRef> for RawDimRef {
    fn from(r: DimRef) -> RawDimRef {
        let none = RawDimRef::default();
        match r {
            DimRef::Plane(g) => RawDimRef { plane: Some(g), ..none },
            DimRef::Form(g, e) => RawDimRef { form: Some(g), end: Some(e), ..none },
            DimRef::Sketch(g, l) => RawDimRef { form: Some(g), line: Some(l), ..none },
            DimRef::Point(g, l, s) => RawDimRef { form: Some(g), line: Some(l), point: Some(s), ..none },
            DimRef::Line(l, face) => RawDimRef { line: Some(l), face: Some(face), ..none },
            DimRef::LinePoint(l, s) => RawDimRef { line: Some(l), point: Some(s), ..none },
        }
    }
}

/// What a dimension measures: the way between parallel references (aligned), a circle's or an arc's radius or diameter
/// (Revit's radial and diameter dimensions), or the angles between lines that cross (angular, F7c).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DimKind {
    #[default]
    Aligned,
    Radial,
    Diameter,
    Angular,
}

impl DimKind {
    pub fn is_aligned(&self) -> bool {
        *self == DimKind::Aligned
    }

    /// Whether it measures a radius or a diameter: one reference, a circle or an arc.
    pub fn is_round(&self) -> bool {
        matches!(self, DimKind::Radial | DimKind::Diameter)
    }

    /// The references a dimension of it holds at least.
    fn least(self) -> usize {
        if self.is_round() { 1 } else { 2 }
    }

    /// How many times the circle's radius it measures.
    fn times(self) -> f64 {
        if self == DimKind::Diameter { 2.0 } else { 1.0 }
    }
}

/// A dimension of the family's, as Revit's aligned dimension in the family editor: between `refs` (parallel, in order along
/// the way it measures), drawn in `view` with its line through `at` (the family's axes, mm). `label` names the length
/// parameter its one segment is (a reporting parameter is measured, not imposed); `locked` keeps segments at a length (a
/// labelled one's lock keeps its parameter from being changed by dragging, Family Types' Lock); `equal` keeps every segment
/// one length (EQ); `alignment` marks the lock Align leaves, one segment locked at 0, drawn as a padlock and never printed.
/// A radial or diameter one (`kind`) measures a circle of a form's sketch, its one reference, and takes a label or a lock.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyDimension {
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: Id,
    #[serde(default, skip_serializing_if = "Guid::is_nil")]
    pub guid: Guid,
    #[serde(default)]
    pub view: DimView,
    pub refs: Vec<DimRef>,
    pub at: [f64; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locked: Vec<Lock>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub equal: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub alignment: bool,
    /// Aligned (between parallel references), or a circle's radius or diameter (F7b): one reference, a circle of a sketch.
    #[serde(default, skip_serializing_if = "DimKind::is_aligned")]
    pub kind: DimKind,
}

impl FamilyDimension {
    /// How many segments it has: one less than its references (an angular one's, an angle each), one for a radius or a
    /// diameter.
    pub fn segments(&self) -> usize {
        if self.kind.is_round() { 1 } else { self.refs.len().saturating_sub(1) }
    }

    /// Whether it holds a constraint: a label (not a reporting one's, which `labels` says), a lock, equality, or Align's lock.
    fn constrains(&self, reporting: &dyn Fn(&str) -> bool) -> bool {
        self.label.as_deref().is_some_and(|l| !reporting(l)) || !self.locked.is_empty() || self.equal || self.alignment
    }

    /// Whether segment `i` is locked.
    pub fn is_locked(&self, i: usize) -> bool {
        self.locked.iter().any(|l| l.segment == i)
    }
}

/// Why constraints can't hold: what to say, and the dimensions (by their place in the family's list) that are in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Unsat {
    pub message: String,
    pub dims: Vec<usize>,
    /// Whether it is a constraint that adds nothing to the others (refused whatever the values), not values that can't stand.
    pub over: bool,
}

impl std::fmt::Display for Unsat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<String> for Unsat {
    fn from(message: String) -> Unsat {
        Unsat { message, dims: Vec::new(), over: false }
    }
}

/// What can move: a plane of the family's own (its place in `planes`) along its normal, an extrusion's end, a vertex of a
/// form's sketch along its host's x or y (the form's place, the vertex's, 0 or 1), a circle's radius (the form's place,
/// the line's); and (F7c) a reference line's start along x or y of what it is drawn on, its angle there (radians) and its
/// length (its place in `lines`), and an arc's turn (radians; the form's place, the line's).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Unknown {
    Plane(usize),
    End(usize, Extent),
    Vertex(usize, usize, usize),
    Radius(usize, usize),
    LineStart(usize, usize),
    LineAngle(usize),
    LineLength(usize),
    Turn(usize, usize),
    /// A plane its pivot turns, about its axis from as drawn (radians; F7d).
    PlaneTurn(usize),
}

impl Unknown {
    /// Whether rows that read it are non-linear: a line's angle and length and an arc's turn are read through sines.
    fn turns(&self) -> bool {
        matches!(self, Unknown::LineStart(..) | Unknown::LineAngle(_) | Unknown::LineLength(_) | Unknown::Turn(..) | Unknown::PlaneTurn(_))
    }
}

/// A number and how it changes with each unknown (forward-mode differentiation, sparse): what a non-linear row reads, at
/// the point the solve has reached. As a [`Lin`] it is the row linearised there, which for a linear row is the row itself.
#[derive(Clone, Debug, Default)]
struct Ad {
    v: f64,
    g: Vec<(usize, f64)>,
}

impl Ad {
    fn k(v: f64) -> Ad {
        Ad { v, g: Vec::new() }
    }

    fn var(u: usize, v: f64) -> Ad {
        Ad { v, g: vec![(u, 1.0)] }
    }

    /// `a·self + b·other`, its derivatives summed by unknown, in the unknowns' order.
    fn mix(&self, a: f64, other: &Ad, b: f64, v: f64) -> Ad {
        let mut g: Vec<(usize, f64)> = self.g.iter().map(|(u, k)| (*u, a * k)).chain(other.g.iter().map(|(u, k)| (*u, b * k))).collect();
        g.sort_by_key(|t| t.0);
        let mut out: Vec<(usize, f64)> = Vec::with_capacity(g.len());
        for (u, k) in g {
            match out.last_mut() {
                Some(last) if last.0 == u => last.1 += k,
                _ => out.push((u, k)),
            }
        }
        Ad { v, g: out }
    }

    fn add(&self, o: &Ad) -> Ad {
        self.mix(1.0, o, 1.0, self.v + o.v)
    }

    fn sub(&self, o: &Ad) -> Ad {
        self.mix(1.0, o, -1.0, self.v - o.v)
    }

    fn mul(&self, o: &Ad) -> Ad {
        self.mix(o.v, o, self.v, self.v * o.v)
    }

    fn scale(&self, k: f64) -> Ad {
        Ad { v: self.v * k, g: self.g.iter().map(|(u, d)| (*u, d * k)).collect() }
    }

    /// Of a function `f` whose derivative at `self.v` is `d`.
    fn chain(&self, v: f64, d: f64) -> Ad {
        Ad { v, g: self.g.iter().map(|(u, k)| (*u, k * d)).collect() }
    }

    fn div(&self, o: &Ad) -> Ad {
        let q = self.v / o.v;
        self.mix(1.0 / o.v, o, -q / o.v, q)
    }

    fn sin(&self) -> Ad {
        self.chain(self.v.sin(), self.v.cos())
    }

    fn cos(&self) -> Ad {
        self.chain(self.v.cos(), -self.v.sin())
    }

    fn sqrt(&self) -> Ad {
        let r = self.v.sqrt();
        self.chain(r, if r > 0.0 { 0.5 / r } else { 0.0 })
    }

    fn abs(&self) -> Ad {
        if self.v < 0.0 { self.scale(-1.0) } else { self.clone() }
    }

    /// The angle of `(x, y)`, `self` being y.
    fn atan2(&self, x: &Ad) -> Ad {
        let r2 = self.v * self.v + x.v * x.v;
        let v = self.v.atan2(x.v);
        if r2 <= 0.0 {
            return Ad::k(v);
        }
        self.mix(x.v / r2, x, -self.v / r2, v)
    }

    /// The row it is, linearised at `x`.
    fn lin(self, x: &[f64]) -> Lin {
        let c = self.v - self.g.iter().map(|(u, k)| k * x.get(*u).copied().unwrap_or(0.0)).sum::<f64>();
        Lin { terms: self.g, c }.tidy()
    }
}

/// A point or a way in a view's own axes whose parts are [`Ad`]s.
type Ad2 = (Ad, Ad);

/// A point or a direction in the family's axes whose parts are [`Ad`]s.
type V3 = [Ad; 3];

fn v3(p: P3) -> V3 {
    p.map(Ad::k)
}

fn vadd(a: &V3, b: &V3) -> V3 {
    std::array::from_fn(|k| a[k].add(&b[k]))
}

fn vsub(a: &V3, b: &V3) -> V3 {
    std::array::from_fn(|k| a[k].sub(&b[k]))
}

fn vmul(a: &V3, s: &Ad) -> V3 {
    std::array::from_fn(|k| a[k].mul(s))
}

fn vdot(a: &V3, b: &V3) -> Ad {
    a[0].mul(&b[0]).add(&a[1].mul(&b[1])).add(&a[2].mul(&b[2]))
}

fn vdotk(a: &V3, m: P3) -> Ad {
    a[0].scale(m[0]).add(&a[1].scale(m[1])).add(&a[2].scale(m[2]))
}

fn vcross(a: &V3, b: &V3) -> V3 {
    [a[1].mul(&b[2]).sub(&a[2].mul(&b[1])), a[2].mul(&b[0]).sub(&a[0].mul(&b[2])), a[0].mul(&b[1]).sub(&a[1].mul(&b[0]))]
}

fn vunit(a: &V3) -> V3 {
    let l = vdot(a, a).sqrt();
    if l.v > 1e-12 { std::array::from_fn(|k| a[k].div(&l)) } else { a.clone() }
}

/// A plane's axes whose parts are [`Ad`]s: what a frame that moves and turns with the unknowns is.
#[derive(Clone, Debug)]
struct AdFrame {
    origin: V3,
    x: V3,
    y: V3,
    z: V3,
}

impl AdFrame {
    fn point(&self, u: &Ad, v: &Ad, t: &Ad) -> V3 {
        vadd(&vadd(&self.origin, &vmul(&self.x, u)), &vadd(&vmul(&self.y, v), &vmul(&self.z, t)))
    }

    fn dir(&self, u: &Ad, v: &Ad) -> V3 {
        vadd(&vmul(&self.x, u), &vmul(&self.y, v))
    }
}

/// A position along a normal: the sum of some unknowns, each times its factor, and a constant.
#[derive(Clone, Debug, Default)]
struct Lin {
    terms: Vec<(usize, f64)>,
    c: f64,
}

impl Lin {
    fn number(c: f64) -> Lin {
        Lin { terms: Vec::new(), c }
    }

    fn of(u: usize, k: f64) -> Lin {
        Lin { terms: vec![(u, k)], c: 0.0 }
    }

    fn scaled(mut self, k: f64) -> Lin {
        self.terms.iter_mut().for_each(|t| t.1 *= k);
        self.c *= k;
        self
    }

    fn plus(mut self, other: Lin) -> Lin {
        self.terms.extend(other.terms);
        self.c += other.c;
        self
    }

    fn minus(mut self, other: &Lin) -> Lin {
        self.terms.extend(other.terms.iter().map(|(u, k)| (*u, -k)));
        self.c -= other.c;
        self
    }

    /// Without the terms whose factor is next to nothing: a direction square to another adds no unknown to a row.
    fn tidy(mut self) -> Lin {
        self.terms.retain(|t| t.1.abs() > 1e-12);
        self
    }

    fn at(&self, x: &[f64]) -> f64 {
        self.terms.iter().map(|(u, k)| k * x.get(*u).copied().unwrap_or(0.0)).sum::<f64>() + self.c
    }

    /// Its value and derivatives at `x`.
    fn ad(&self, x: &[f64]) -> Ad {
        Ad::k(0.0).mix(0.0, &Ad { v: 0.0, g: self.terms.clone() }, 1.0, self.at(x))
    }
}

/// One constraint: `lin = 0`, held for dimension `dim` (none: a pin a drag or Align adds, or a soft row). A soft row (a
/// sketch line kept parallel to itself) is left out where the rows before it hold it already, never refused for it.
struct Row {
    lin: Lin,
    dim: Option<usize>,
    soft: bool,
    /// The pivot it holds, as a message names it (F7d).
    pivot: Option<String>,
}

impl Row {
    fn hard(lin: Lin, dim: Option<usize>) -> Row {
        Row { lin, dim, soft: false, pivot: None }
    }
}

/// What a solve reads where things are drawn, kept for the steps after (F7c): each dimension's segments' ways and angles'
/// sectors, recorded in order the first time the rows are made and given back in the same order after.
#[derive(Debug, Default)]
struct Memo {
    record: bool,
    saved: Vec<Vec<f64>>,
    next: usize,
}

impl Memo {
    fn take(&mut self, f: impl FnOnce() -> Result<Vec<f64>, String>) -> Result<Vec<f64>, String> {
        if self.record {
            let v = f()?;
            self.saved.push(v.clone());
            return Ok(v);
        }
        let v = self.saved.get(self.next).cloned().ok_or("the solve lost its place")?;
        self.next += 1;
        Ok(v)
    }

    /// Recording done: what is read from here on is given back from the start.
    fn replay(&mut self) {
        self.record = false;
        self.next = 0;
    }
}

/// What one solve came to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Solution {
    /// Each plane of the family's own that moved: its place in `planes` and its new offset along its normal.
    pub planes: Vec<(usize, f64)>,
    /// Each extrusion end the constraints set: the form's place, which end, and its value along its host's normal.
    pub ends: Vec<(usize, Extent, f64)>,
    /// Each point of a sketch the constraints set: the form's place, the line's, which point, and where it is in its host's
    /// axes.
    pub points: Vec<(usize, usize, Spot, P2)>,
    /// Each circle's radius the constraints set: the form's place, the line's, and the radius.
    pub radii: Vec<(usize, usize, f64)>,
    /// Each reference line the constraints set (F7c): its place in `lines`, and its start (x, y), angle (radians) and length
    /// in the axes of what it is drawn on.
    pub lines: Vec<(usize, [f64; 4])>,
    /// Each arc's turn the constraints set: the form's place, the line's, and the turn in degrees.
    pub turns: Vec<(usize, usize, f64)>,
    /// Each plane its pivot turned (F7d): its place in `planes`, its axes as it now stands and the turn (radians).
    pub turned_planes: Vec<(usize, Frame, f64)>,
}

/// A change a drag or Align asks of the draft: a plane's offset along its normal, an extrusion end's value, a parameter's, a
/// point of a form's sketch (the form's place, the line's, which point, where in its host's axes), a reference line moved
/// by so much (in the family's axes, what is drawn on it with it), a reference line put at a start and an end (F7c: one the
/// constraints moved or turned, or one drawn on such a line), or an arc's turn (degrees).
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Plane(Guid, f64),
    End(usize, Extent, f64),
    Parameter(String, f64),
    Point(usize, usize, Spot, [f64; 2]),
    Line(Guid, P3),
    LineEnds(Guid, P3, P3),
    Turn(usize, usize, f64),
    /// A pivot's own angle (degrees), where no parameter labels it (F7d): the line's or the plane's GUID.
    PivotAngle(Guid, f64),
}

/// What a solve holds where it is told, besides the constraints: a plane at an offset along its normal, or a reference at a
/// place along a way (a sketch line both its ends).
#[derive(Clone, Copy, Debug)]
enum Pin {
    Plane(Guid, f64),
    Ref(DimRef, P3, f64),
}

/// The work a solve may do, shared by the solves of one check.
#[derive(Debug)]
pub struct Meter(u64);

impl Default for Meter {
    fn default() -> Meter {
        Meter(BUDGET)
    }
}

impl Meter {
    fn spend(&mut self, n: u64) -> Result<(), String> {
        self.0 = self.0.checked_sub(n).ok_or("its constraints take too much work to solve: hold fewer of them")?;
        Ok(())
    }
}

/// What a line of a sketch is, as a dimension sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Straight,
    Arc,
    Circle,
}

/// A form's sketch as a solve reads it for a type (F7b): where its vertices are in its host's axes (the ends of its lines
/// that meet within [`TOL`] one vertex, an axis line's ends and each circle's centre their own), each line's start and end
/// vertex (a circle's centre twice), what each line is, each circle's radius and each arc's turn (degrees), and the points
/// at each vertex.
#[derive(Clone, Debug, Default)]
struct Layout {
    at: Vec<P2>,
    ends: Vec<(usize, usize)>,
    shape: Vec<Shape>,
    radius: Vec<f64>,
    turn: Vec<f64>,
    points: Vec<Vec<(usize, Spot)>>,
}

/// The sketch of `form` read for `values` ([`Layout`]).
fn layout<'a>(
    form: &'a Form,
    values: &BTreeMap<String, Val>,
    read: &mut HashMap<&'a str, Result<Node, String>>,
    meter: &mut Meter,
) -> Result<Layout, String> {
    let count = form.plane_lines().count();
    if count > 2 * sketch::MAX_SKETCH {
        return Err(format!("its sketch has {count} lines, more than {}", 2 * sketch::MAX_SKETCH));
    }
    meter.spend(16 * count as u64 + 1)?;
    let mut out = Layout::default();
    // A Blend's base and top (F6) are two groups: a vertex of one never joins one of the other.
    let mut grid: HashMap<(usize, i64, i64), Vec<usize>> = HashMap::new();
    let cell = |p: P2| ((p.0 / TOL).floor() as i64, (p.1 / TOL).floor() as i64);
    let mut join = |out: &mut Layout, p: P2, own: bool, k: usize, spot: Spot| -> usize {
        let group = form.plane_group(k);
        if !own {
            let (cx, cy) = cell(p);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for &v in grid.get(&(group, cx + dx, cy + dy)).map(Vec::as_slice).unwrap_or_default() {
                        if out.at[v].dist(p) <= TOL {
                            out.points[v].push((k, spot));
                            return v;
                        }
                    }
                }
            }
        }
        out.at.push(p);
        out.points.push(vec![(k, spot)]);
        let v = out.at.len() - 1;
        if !own {
            let (cx, cy) = cell(p);
            grid.entry((group, cx, cy)).or_default().push(v);
        }
        v
    };
    for (k, l) in form.plane_lines().enumerate() {
        let mut num = |e: &'a Expr| number(e, values, read);
        match &l.curve {
            FormCurve::Line { start, end, turn } => {
                let a = P2(num(&start[0])?, num(&start[1])?);
                let b = P2(num(&end[0])?, num(&end[1])?);
                let t = match turn {
                    Some(t) => num(t)?,
                    None => 0.0,
                };
                let va = join(&mut out, a, l.axis, k, Spot::Start);
                let vb = join(&mut out, b, l.axis, k, Spot::End);
                out.ends.push((va, vb));
                out.shape.push(if t == 0.0 { Shape::Straight } else { Shape::Arc });
                out.radius.push(if t == 0.0 { 0.0 } else { arc_radius(a, b, t.to_radians()) });
                out.turn.push(t);
            }
            FormCurve::Circle { centre, radius } => {
                let c = P2(num(&centre[0])?, num(&centre[1])?);
                let r = num(radius)?;
                if r <= 0.0 {
                    return Err(format!("line {}: a circle's radius is {r}; it needs one above zero", k + 1));
                }
                let v = join(&mut out, c, true, k, Spot::Centre);
                out.ends.push((v, v));
                out.shape.push(Shape::Circle);
                out.radius.push(r);
                out.turn.push(0.0);
            }
        }
    }
    Ok(out)
}

/// An arc's radius, from its ends and its turn (radians).
fn arc_radius(a: P2, b: P2, t: f64) -> f64 {
    a.dist(b) / (2.0 * (t / 2.0).sin().abs())
}

/// An arc's centre, from its ends and its turn (radians): left of the way from `a` to `b` for a turn counter-clockwise.
fn arc_centre(a: P2, b: P2, t: f64) -> P2 {
    let k = 0.5 / (t / 2.0).tan();
    P2((a.0 + b.0) / 2.0 - (b.1 - a.1) * k, (a.1 + b.1) / 2.0 + (b.0 - a.0) * k)
}

/// Form `g` a sketch reference names: its place, the form, what it is drawn on (a plane, or a reference line's face, F7c)
/// and the axes of that as drawn.
fn sketched(f: &Family, g: Guid) -> Result<(usize, &Form, PlaneRef, Frame), String> {
    let i = f.forms.iter().position(|x| x.guid == g).ok_or_else(|| format!("form {g} isn't one of its forms"))?;
    let form = &f.forms[i];
    if !form.mesh.is_empty() {
        return Err(format!("form {}: a mesh from a file has no sketch to measure", i + 1));
    }
    Ok((i, form, form.host, f.frame(&form.host)?))
}

/// Line `l` of form `i`'s sketch: its place and the line.
fn sketch_line(form: &Form, i: usize, l: Guid) -> Result<(usize, &FormLine), String> {
    form.plane_lines()
        .enumerate()
        .find(|(_, x)| x.guid == l)
        .ok_or_else(|| format!("form {}: line {l} isn't one of its sketch's lines", i + 1))
}

/// The plane reference line `g` is drawn on, followed through the lines it is drawn on: what moves it.
fn line_root(f: &Family, g: Guid) -> Option<Guid> {
    let mut at = g;
    for _ in 0..=f.lines.len() {
        match f.lines.iter().find(|l| l.guid == at)?.host {
            PlaneRef::Plane(h) => return Some(h),
            PlaneRef::Line(h, _) => at = h,
        }
    }
    None
}

/// A line of a sketch a solve reads: its form's place, its own, the form's host plane and its axes, its end vertices.
struct Seen {
    form: usize,
    line: usize,
    host: PlaneRef,
    frame: Frame,
    ends: (usize, usize),
    shape: Shape,
}

/// The working of one solve: what may move, where it is drawn, where the solve has got to (`at`, what non-linear rows are
/// linearised about), and the sketches its references name, read for the type.
struct Problem<'f> {
    family: &'f Family,
    unknowns: Vec<Unknown>,
    x0: Vec<f64>,
    at: Vec<f64>,
    layouts: BTreeMap<usize, Layout>,
}

/// How deep reference lines drawn on reference lines are followed.
const DEPTH: usize = planes::MAX_LINES + 1;

/// What an angular dimension measures between: reference lines, reference planes and straight sketch lines.
const ANGULAR_REFS: &str =
    "an angular dimension measures between reference lines, reference planes and straight sketch lines, each seen as a line in its view";

impl<'f> Problem<'f> {
    /// Nothing to move yet, and nothing read: what only reads where things are drawn works from it.
    fn bare(family: &'f Family) -> Problem<'f> {
        Problem { family, unknowns: Vec::new(), x0: Vec::new(), at: Vec::new(), layouts: BTreeMap::new() }
    }

    /// Nothing to move yet; the sketches `refs` name read for `values`.
    fn new<'a>(
        family: &'f Family,
        refs: impl IntoIterator<Item = DimRef>,
        values: &BTreeMap<String, Val>,
        read: &mut HashMap<&'a str, Result<Node, String>>,
        meter: &mut Meter,
    ) -> Result<Problem<'f>, String>
    where
        'f: 'a,
    {
        let mut layouts = BTreeMap::new();
        for r in refs {
            if let DimRef::Sketch(g, _) | DimRef::Point(g, _, _) = r
                && let Some(i) = family.forms.iter().position(|f| f.guid == g)
                && !layouts.contains_key(&i)
            {
                let form: &'f Form = &family.forms[i];
                layouts.insert(i, layout(form, values, read, meter).map_err(|e| format!("form {}: {e}", i + 1))?);
            }
        }
        Ok(Problem { layouts, ..Problem::bare(family) })
    }

    fn unknown(&self, u: Unknown) -> Option<usize> {
        self.unknowns.iter().position(|v| *v == u)
    }

    /// Unknown `u` (by place) where the solve has got to.
    fn var(&self, u: usize) -> Ad {
        Ad::var(u, self.at.get(u).copied().unwrap_or(0.0))
    }

    /// Unknown `u` where the solve has got to, or `drawn` where it isn't one.
    fn value(&self, u: Unknown, drawn: f64) -> Ad {
        self.unknown(u).map_or(Ad::k(drawn), |k| self.var(k))
    }

    /// Plane `g`'s place in `planes`, and its normal and offset as drawn (a datum's are its own, whether listed or not).
    fn plane(&self, g: Guid) -> Result<(Option<usize>, P3, f64), String> {
        match self.family.planes.iter().position(|p| p.guid == g) {
            Some(i) => {
                let p = &self.family.planes[i];
                let z = p.frame().z;
                Ok((Some(i), z, if p.datum() { 0.0 } else { dot(p.start, z) }))
            }
            None => datums()
                .into_iter()
                .find(|d| d.guid == g)
                .map(|d| (None, d.frame().z, 0.0))
                .ok_or_else(|| format!("plane {g} isn't one of its planes")),
        }
    }

    /// Plane `g`'s position along its normal: an unknown, or where it is drawn.
    fn plane_pos(&self, g: Guid) -> Result<(P3, Lin), String> {
        let (i, z, s) = self.plane(g)?;
        Ok(match i.and_then(|i| self.unknown(Unknown::Plane(i))) {
            Some(u) => (z, Lin::of(u, 1.0)),
            None => (z, Lin::number(s)),
        })
    }

    /// The axes of what `host` names, moving and turning with the unknowns: a plane along its normal, a reference line's
    /// face with the line (F7c).
    fn host_ad(&self, host: &PlaneRef, depth: usize) -> Result<AdFrame, String> {
        match *host {
            PlaneRef::Plane(g) => {
                if let Some(i) = self.turning_plane(g) {
                    return self.plane_frame_ad(i, depth);
                }
                let fr = self.family.frame(host)?;
                let (z, lin) = self.plane_pos(g)?;
                Ok(AdFrame { origin: vmul(&v3(z), &lin.ad(&self.at)), x: v3(fr.x), y: v3(fr.y), z: v3(fr.z) })
            }
            PlaneRef::Line(g, face) => self.line_face(g, face, depth),
        }
    }

    /// Reference line `i` as drawn in the axes of what it is drawn on: its start (x, y and off the plane), its end off the
    /// plane, its angle and its length.
    fn line_drawn(&self, i: usize) -> Result<[f64; 6], String> {
        let l = self.family.lines.get(i).ok_or("no such line")?;
        let fr = self.family.frame(&l.host)?;
        let ((a, h0), (b, h1)) = (fr.local(l.start), fr.local(l.end));
        Ok([a.0, a.1, h0, h1, (b.1 - a.1).atan2(b.0 - a.0), a.dist(b)])
    }

    /// Reference line `g`'s place, its start and end, and the normal of what it is drawn on, moving with the unknowns.
    fn line_ends(&self, g: Guid, depth: usize) -> Result<(usize, V3, V3, V3), String> {
        let i = self.family.lines.iter().position(|l| l.guid == g).ok_or_else(|| format!("line {g} isn't one of its lines"))?;
        if depth == 0 {
            return Err(format!("line {}: it is drawn on lines too deep to follow", i + 1));
        }
        let l = &self.family.lines[i];
        let h = self.host_ad(&l.host, depth - 1)?;
        let [su, sv, h0, h1, th, len] = self.line_drawn(i)?;
        let (su, sv) = (self.value(Unknown::LineStart(i, 0), su), self.value(Unknown::LineStart(i, 1), sv));
        let (th, len) = (self.value(Unknown::LineAngle(i), th), self.value(Unknown::LineLength(i), len));
        let start = h.point(&su, &sv, &Ad::k(h0));
        let end = vadd(&vadd(&start, &vmul(&h.dir(&th.cos(), &th.sin()), &len)), &vmul(&h.z, &Ad::k(h1 - h0)));
        Ok((i, start, end, h.z))
    }

    /// Face `face` of reference line `g`, its axes as T18 has them, moving and turning with the line.
    fn line_face(&self, g: Guid, face: Face, depth: usize) -> Result<AdFrame, String> {
        let (_, start, end, n) = self.line_ends(g, depth)?;
        let d = vunit(&vsub(&end, &start));
        let side = vcross(&n, &d);
        Ok(match face {
            Face::Horizontal => AdFrame { origin: start, x: d, y: side, z: n },
            Face::Vertical => {
                let z = vcross(&d, &n);
                AdFrame { origin: start, x: d, y: n, z }
            }
            Face::Start => AdFrame { origin: start, x: side, y: n, z: d },
            Face::End => AdFrame { origin: end, x: side, y: n, z: d },
        })
    }

    /// Whether reference line `g`, or one it is drawn on, may turn or move: one of the unknowns is its.
    fn line_moves(&self, g: Guid) -> bool {
        let mut at = g;
        for _ in 0..=self.family.lines.len() {
            let Some(i) = self.family.lines.iter().position(|l| l.guid == at) else { return false };
            if self
                .unknowns
                .iter()
                .any(|u| matches!(u, Unknown::LineStart(k, _) | Unknown::LineAngle(k) | Unknown::LineLength(k) if *k == i))
            {
                return true;
            }
            match self.family.lines[i].host {
                PlaneRef::Plane(h) => return self.turning_plane(h).is_some(),
                PlaneRef::Line(h, _) => at = h,
            }
        }
        false
    }

    /// Line `l` of form `g`'s sketch, as the solve reads it.
    fn seen(&self, g: Guid, l: Guid) -> Result<Seen, String> {
        let (i, form, host, frame) = sketched(self.family, g)?;
        let (k, _) = sketch_line(form, i, l)?;
        let lay = self.layouts.get(&i).ok_or_else(|| format!("form {}: its sketch isn't read", i + 1))?;
        let (Some(ends), Some(shape)) = (lay.ends.get(k).copied(), lay.shape.get(k).copied()) else {
            return Err(format!("form {}: its sketch isn't read", i + 1));
        };
        Ok(Seen { form: i, line: k, host, frame, ends, shape })
    }

    /// Where vertex `v` of form `i`'s sketch is in its host's axes, as drawn for the type.
    fn drawn(&self, i: usize, v: usize) -> P2 {
        self.layouts.get(&i).and_then(|l| l.at.get(v)).copied().unwrap_or(P2(0.0, 0.0))
    }

    /// Vertex `v` of form `i`'s sketch along x (0) or y (1) of its host: an unknown, or where it is drawn.
    fn coord(&self, i: usize, v: usize, axis: usize) -> Lin {
        match self.unknown(Unknown::Vertex(i, v, axis)) {
            Some(u) => Lin::of(u, 1.0),
            None => {
                let p = self.drawn(i, v);
                Lin::number(if axis == 0 { p.0 } else { p.1 })
            }
        }
    }

    /// Vertex `v` of form `i`'s sketch in its host's axes, where the solve has got to.
    fn vertex_ad(&self, i: usize, v: usize) -> (Ad, Ad) {
        (self.coord(i, v, 0).ad(&self.at), self.coord(i, v, 1).ad(&self.at))
    }

    /// The turn (radians) of arc `s`, where the solve has got to.
    fn turn_ad(&self, s: &Seen) -> Ad {
        let drawn = self.layouts.get(&s.form).and_then(|l| l.turn.get(s.line)).copied().unwrap_or(0.0).to_radians();
        self.value(Unknown::Turn(s.form, s.line), drawn)
    }

    /// The centre of arc `s` and its radius, in its host's axes, where the solve has got to.
    fn arc_ad(&self, s: &Seen) -> ((Ad, Ad), Ad) {
        let (a, b) = (self.vertex_ad(s.form, s.ends.0), self.vertex_ad(s.form, s.ends.1));
        let t = self.turn_ad(s);
        let half = t.scale(0.5);
        let (dx, dy) = (b.0.sub(&a.0), b.1.sub(&a.1));
        let chord = dx.mul(&dx).add(&dy.mul(&dy)).sqrt();
        let r = chord.div(&half.sin().abs().scale(2.0));
        let k = half.cos().div(&half.sin().scale(2.0));
        let c = (a.0.add(&b.0).scale(0.5).sub(&dy.mul(&k)), a.1.add(&b.1).scale(0.5).add(&dx.mul(&k)));
        (c, r)
    }

    /// A point of sketch `s` at `p` (its host's axes) in the family's axes, moving with the unknowns.
    fn sketch_point(&self, s: &Seen, p: &(Ad, Ad)) -> Result<V3, String> {
        Ok(self.host_ad(&s.host, DEPTH)?.point(&p.0, &p.1, &Ad::k(0.0)))
    }

    /// Vertex `v` of the sketch `s` is a line of, along `m`: its host's offset as much as `m` runs along the host's normal,
    /// and its place on the host; on a reference line's face, through the face's axes as the line turns (F7c).
    fn vertex_along(&self, s: &Seen, v: usize, m: P3) -> Result<Lin, String> {
        let PlaneRef::Plane(h) = s.host else {
            return Ok(vdotk(&self.sketch_point(s, &self.vertex_ad(s.form, v))?, m).lin(&self.at));
        };
        if self.turning_plane(h).is_some() {
            return Ok(vdotk(&self.sketch_point(s, &self.vertex_ad(s.form, v))?, m).lin(&self.at));
        }
        let (z, host) = self.plane_pos(h)?;
        Ok(host
            .scaled(dot(m, z))
            .plus(self.coord(s.form, v, 0).scaled(dot(m, s.frame.x)))
            .plus(self.coord(s.form, v, 1).scaled(dot(m, s.frame.y)))
            .tidy())
    }

    /// The way across straight line `s`, in the family's axes, as drawn for the type.
    fn across(&self, s: &Seen) -> Result<P3, String> {
        let who = || format!("form {}: line {}", s.form + 1, s.line + 1);
        match s.shape {
            Shape::Straight => {}
            Shape::Arc => {
                return Err(format!(
                    "{}: an arc; a dimension measures to a straight line, an arc by its centre or a radial dimension",
                    who()
                ));
            }
            Shape::Circle => return Err(format!("{}: a circle; it is measured by its centre or by a radial dimension", who())),
        }
        let (a, b) = (self.drawn(s.form, s.ends.0), self.drawn(s.form, s.ends.1));
        if a.dist(b) < 1e-9 {
            return Err(format!("{}: it has no length", who()));
        }
        unit3(s.frame.dir((b - a).unit().perp())).ok_or_else(|| format!("{}: it has no length", who()))
    }

    /// The way reference `r` faces (its normal), or none for a point, which has no way of its own.
    fn direction(&self, r: &DimRef) -> Result<Option<P3>, String> {
        Ok(match *r {
            DimRef::Plane(g) => Some(self.plane(g)?.1),
            DimRef::Form(g, _) => {
                let (_, form) = end_form(self.family, g)?;
                Some(self.family.frame(&form.host)?.z)
            }
            DimRef::Sketch(g, l) => Some(self.across(&self.seen(g, l)?)?),
            DimRef::Point(..) | DimRef::LinePoint(..) => None,
            DimRef::Line(g, face) => Some(self.family.frame(&PlaneRef::Line(g, face))?.z),
        })
    }

    /// The vertex point `spot` of line `l` of form `g` is at (an arc's centre is no vertex: [`Problem::position`] works it
    /// out).
    fn spot(&self, g: Guid, l: Guid, spot: Spot) -> Result<(Seen, usize), String> {
        let s = self.seen(g, l)?;
        let who = format!("form {}: line {}", s.form + 1, s.line + 1);
        let v = match (s.shape, spot) {
            (Shape::Circle, Spot::Centre) => s.ends.0,
            (Shape::Circle, _) => return Err(format!("{who}: a circle has a centre, no start or end")),
            (Shape::Arc, Spot::Centre) => return Err(format!("{who}: an arc's centre is no vertex")),
            (Shape::Straight, Spot::Centre) => return Err(format!("{who}: a straight line has no centre")),
            (_, Spot::Start) => s.ends.0,
            (_, Spot::End) => s.ends.1,
        };
        Ok((s, v))
    }

    /// Reference `r`'s place along `m`, worked out over `values`: linearised where the solve has got to, where it turns.
    fn position<'a>(
        &self,
        r: &DimRef,
        m: P3,
        values: &BTreeMap<String, Val>,
        read: &mut HashMap<&'a str, Result<Node, String>>,
    ) -> Result<Lin, String>
    where
        'f: 'a,
    {
        let signed = |n: P3, lin: Lin| if dot(n, m) < 0.0 { lin.scaled(-1.0) } else { lin };
        match *r {
            DimRef::Plane(g) if self.turning_plane(g).is_some() => {
                Ok(vdotk(&self.host_ad(&PlaneRef::Plane(g), DEPTH)?.origin, m).lin(&self.at))
            }
            DimRef::Plane(g) => {
                let (z, lin) = self.plane_pos(g)?;
                Ok(signed(z, lin))
            }
            DimRef::Form(g, end) => {
                let (i, form) = end_form(self.family, g)?;
                let value = match self.unknown(Unknown::End(i, end)) {
                    Some(u) => Lin::of(u, 1.0),
                    None => Lin::number(number(end_expr(form, end), values, read)?),
                };
                let turning = matches!(form.host, PlaneRef::Plane(h) if self.turning_plane(h).is_some());
                let (PlaneRef::Plane(h), false) = (form.host, turning) else {
                    // On a reference line's face (F7c), or a plane a pivot turns (F7d): along the face's normal as it turns.
                    let f = self.host_ad(&form.host, DEPTH)?;
                    return Ok(vdotk(&vadd(&f.origin, &vmul(&f.z, &value.ad(&self.at))), m).lin(&self.at));
                };
                let (z, host) = self.plane_pos(h)?;
                Ok(signed(z, host.plus(value)))
            }
            DimRef::Sketch(g, l) => {
                let s = self.seen(g, l)?;
                self.across(&s)?;
                self.vertex_along(&s, s.ends.0, m)
            }
            DimRef::Point(g, l, Spot::Centre) if self.seen(g, l).is_ok_and(|s| s.shape == Shape::Arc) => {
                let s = self.seen(g, l)?;
                let (c, _) = self.arc_ad(&s);
                Ok(vdotk(&self.sketch_point(&s, &c)?, m).lin(&self.at))
            }
            DimRef::Point(g, l, spot) => {
                let (s, v) = self.spot(g, l, spot)?;
                self.vertex_along(&s, v, m)
            }
            DimRef::Line(g, face) if self.line_moves(g) => Ok(vdotk(&self.line_face(g, face, DEPTH)?.origin, m).lin(&self.at)),
            DimRef::Line(g, face) => {
                let fr = self.family.frame(&PlaneRef::Line(g, face))?;
                let mut lin = Lin::number(dot(fr.origin, fr.z));
                // It moves with the plane it is drawn on, as far as that plane moves along its normal.
                if let Some(root) = line_root(self.family, g)
                    && let (Some(i), z, drawn) = self.plane(root)?
                    && let Some(u) = self.unknown(Unknown::Plane(i))
                {
                    let k = dot(fr.z, z);
                    lin = lin.plus(Lin { terms: vec![(u, k)], c: -k * drawn }).tidy();
                }
                Ok(signed(fr.z, lin))
            }
            DimRef::LinePoint(g, spot) => {
                let (_, start, end, _) = self.line_ends(g, DEPTH)?;
                let p = if spot == Spot::End { end } else { start };
                Ok(vdotk(&p, m).lin(&self.at))
            }
        }
    }

    /// What may move when `r` is held, a dimension of `kind` measuring to it: its plane, its end, its line's vertices (and
    /// a point's host), its circle's radius or its arc's turn, the plane its reference line is drawn on, the reference line
    /// itself (F7c).
    fn unknowns_of(&self, r: &DimRef, kind: DimKind, add: &mut dyn FnMut(Unknown)) {
        let f = self.family;
        let own = |g: Guid| f.planes.iter().position(|p| p.guid == g && !p.datum() && !p.defines_origin && p.pivot.is_none());
        // A plane a pivot turns: its turn, whatever measures it (F7d).
        let pivoted = |g: Guid| f.planes.iter().position(|p| p.guid == g && p.pivot.is_some() && !p.datum());
        let line = |g: Guid, add: &mut dyn FnMut(Unknown)| {
            if let Some(i) = f.lines.iter().position(|l| l.guid == g) {
                add(Unknown::LineStart(i, 0));
                add(Unknown::LineStart(i, 1));
                add(Unknown::LineAngle(i));
                add(Unknown::LineLength(i));
            }
        };
        match *r {
            DimRef::Plane(g) => {
                if kind != DimKind::Angular {
                    own(g).into_iter().for_each(|i| add(Unknown::Plane(i)));
                }
                pivoted(g).into_iter().for_each(|i| add(Unknown::PlaneTurn(i)));
            }
            DimRef::Form(g, end) => {
                if let Some(i) = f.forms.iter().position(|x| x.guid == g) {
                    add(Unknown::End(i, end));
                }
            }
            DimRef::Sketch(g, l) | DimRef::Point(g, l, _) => {
                let Ok(s) = self.seen(g, l) else { return };
                let both = |add: &mut dyn FnMut(Unknown)| {
                    for v in [s.ends.0, s.ends.1] {
                        add(Unknown::Vertex(s.form, v, 0));
                        add(Unknown::Vertex(s.form, v, 1));
                    }
                };
                let arc = s.shape == Shape::Arc && (kind.is_round() || matches!(r, DimRef::Point(_, _, Spot::Centre)));
                if arc {
                    both(add);
                    add(Unknown::Turn(s.form, s.line));
                } else if kind.is_round() {
                    add(Unknown::Radius(s.form, s.line));
                    return;
                } else {
                    let vs = match r {
                        DimRef::Point(_, _, Spot::End) => vec![s.ends.1],
                        DimRef::Point(..) => vec![s.ends.0],
                        _ => vec![s.ends.0, s.ends.1],
                    };
                    for v in vs {
                        add(Unknown::Vertex(s.form, v, 0));
                        add(Unknown::Vertex(s.form, v, 1));
                    }
                }
                if r.is_point()
                    && let PlaneRef::Plane(h) = s.host
                {
                    own(h).into_iter().for_each(|i| add(Unknown::Plane(i)));
                    pivoted(h).into_iter().for_each(|i| add(Unknown::PlaneTurn(i)));
                }
            }
            DimRef::Line(g, _) | DimRef::LinePoint(g, _) => {
                if kind != DimKind::Angular {
                    line_root(f, g).and_then(own).into_iter().for_each(|i| add(Unknown::Plane(i)));
                }
                line_root(f, g).and_then(pivoted).into_iter().for_each(|i| add(Unknown::PlaneTurn(i)));
                line(g, add);
            }
        }
    }

    /// The way aligned dimension `d` measures along: its first reference that faces a way.
    fn way(&self, d: &FamilyDimension) -> Result<P3, String> {
        for r in &d.refs {
            if let Some(n) = self.direction(r)? {
                return Ok(n);
            }
        }
        Err("it measures between points alone: give it a plane or a line to measure along".into())
    }

    /// Where each reference of aligned dimension `d` is along the way it measures, as the family stands: the way, and each
    /// one's place along it.
    fn stations<'a>(
        &self,
        d: &FamilyDimension,
        values: &BTreeMap<String, Val>,
        read: &mut HashMap<&'a str, Result<Node, String>>,
    ) -> Result<(P3, Vec<f64>), String>
    where
        'f: 'a,
    {
        let m = self.way(d)?;
        let mut out = Vec::with_capacity(d.refs.len());
        for r in &d.refs {
            out.push(self.position(r, m, values, read)?.at(&self.at));
        }
        Ok((m, out))
    }

    /// The circle or arc radial or diameter dimension `d` measures: its centre (the family's axes), its radius and its host's
    /// axes.
    fn circle(&self, d: &FamilyDimension) -> Result<(P3, f64, Frame), String> {
        let Some(DimRef::Sketch(g, l)) = d.refs.first() else {
            return Err("a radial or diameter dimension measures a circle or an arc".into());
        };
        let s = self.seen(*g, *l)?;
        match s.shape {
            Shape::Circle => {
                let r = self.layouts.get(&s.form).and_then(|x| x.radius.get(s.line)).copied().unwrap_or(0.0);
                Ok((s.frame.point(self.drawn(s.form, s.ends.0), 0.0), r, s.frame))
            }
            Shape::Arc => {
                let ((cx, cy), r) = self.arc_ad(&s);
                Ok((s.frame.point(P2(cx.v, cy.v), 0.0), r.v, s.frame))
            }
            Shape::Straight => Err(format!(
                "form {}: line {} is straight; a radial or diameter dimension measures a circle or an arc",
                s.form + 1,
                s.line + 1
            )),
        }
    }

    /// Reference `r` of an angular dimension drawn in `view`, in the view's own axes ([`view_axes`]), where the solve has got
    /// to: a point it passes and the way it runs (a reference line from its start to its end, a plane's trace, a sketch line
    /// from its start to its end).
    fn in_view(&self, r: &DimRef, view: DimView) -> Result<(Ad2, Ad2), String> {
        let (a, b) = view_axes(view);
        let eye = view.eye();
        let flat = |p: &V3| (vdotk(p, a), vdotk(p, b));
        let (point, way): (V3, V3) = match *r {
            DimRef::Plane(g) => {
                let (_, z, _) = self.plane(g)?;
                if dot(z, eye).abs() > PARALLEL {
                    return Err(ANGULAR_REFS.into());
                }
                let f = self.host_ad(&PlaneRef::Plane(g), DEPTH)?;
                let way = vcross(&v3(eye), &f.z);
                (f.origin, way)
            }
            DimRef::Line(g, _) => {
                let (_, start, end, _) = self.line_ends(g, DEPTH)?;
                let way = vsub(&end, &start);
                (start, way)
            }
            DimRef::Sketch(g, l) => {
                let s = self.seen(g, l)?;
                if s.shape != Shape::Straight {
                    return Err(ANGULAR_REFS.into());
                }
                let pa = self.sketch_point(&s, &self.vertex_ad(s.form, s.ends.0))?;
                let pb = self.sketch_point(&s, &self.vertex_ad(s.form, s.ends.1))?;
                let way = vsub(&pb, &pa);
                (pa, way)
            }
            _ => return Err(ANGULAR_REFS.into()),
        };
        let w = [way[0].v, way[1].v, way[2].v];
        match unit3(w) {
            Some(u) if dot(u, eye).abs() <= PARALLEL => Ok((flat(&point), flat(&way))),
            Some(_) => Err(ANGULAR_REFS.into()),
            None => Err("a line it measures has no length".into()),
        }
    }

    /// The angles of angular dimension `d` where the solve has got to, each signed so that it is the angle as drawn when
    /// `signs` (each reference's ray and each segment's turn, from [`ray_signs`] on the family as drawn) are the drawn ones.
    fn angles(&self, d: &FamilyDimension, signs: &AngleSigns) -> Result<Vec<Ad>, String> {
        let mut ways: Vec<(Ad, Ad)> = Vec::with_capacity(d.refs.len());
        for (r, s) in d.refs.iter().zip(&signs.rays) {
            let (_, w) = self.in_view(r, d.view)?;
            ways.push((w.0.scale(*s), w.1.scale(*s)));
        }
        Ok(ways
            .windows(2)
            .zip(&signs.turns)
            .map(|(w, s)| {
                let (a, b) = (&w[0], &w[1]);
                let cross = a.0.mul(&b.1).sub(&a.1.mul(&b.0));
                let along = a.0.mul(&b.0).add(&a.1.mul(&b.1));
                cross.atan2(&along).scale(*s)
            })
            .collect())
    }

    /// The rays and turns of angular dimension `d` as the family stands: the sector each angle is, the one `at` lies in.
    fn angle_signs(&self, d: &FamilyDimension) -> Result<AngleSigns, String> {
        let mut seen = Vec::with_capacity(d.refs.len());
        for r in &d.refs {
            let (p, w) = self.in_view(r, d.view)?;
            seen.push((P2(p.0.v, p.1.v), P2(w.0.v, w.1.v)));
        }
        let (a, b) = view_axes(d.view);
        ray_signs(&seen, P2(dot(d.at, a), dot(d.at, b)))
    }
}

/// Which half of each line an angular dimension measures to (+1 its own way, -1 the other) and which way each of its angles
/// turns (+1 counter-clockwise in the view's axes), and where its first two lines cross (the view's axes).
#[derive(Clone, Debug, Default, PartialEq)]
struct AngleSigns {
    rays: Vec<f64>,
    turns: Vec<f64>,
    vertex: P2,
}

/// An angle (radians) as it is measured: in degrees, unsigned, to a billionth of a degree (so 30° drawn by its cosine and
/// sine reads 30).
fn degrees(a: f64) -> f64 {
    (a.abs().to_degrees() * 1e9).round() / 1e9
}

/// Two axes across the way view `view` looks, which an angular dimension's angles are measured in.
fn view_axes(view: DimView) -> (P3, P3) {
    match view.which() {
        views::Which::Plan | views::Which::Ceiling => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        views::Which::Elevation(_) => {
            let e = view.eye();
            ([-e[1], e[0], 0.0], [0.0, 0.0, 1.0])
        }
    }
}

/// The rays and turns of an angular dimension between lines (each a point and a way in the view's axes), its sector the one
/// `at` lies in: each line's half on `at`'s side of where the first two cross. Lines next to each other that are parallel
/// make no angle.
fn ray_signs(lines: &[(P2, P2)], at: P2) -> Result<AngleSigns, String> {
    let cross2 = |a: P2, b: P2| a.0 * b.1 - a.1 * b.0;
    let (Some(&(p0, d0)), Some(&(p1, d1))) = (lines.first(), lines.get(1)) else {
        return Err("it measures between 2 lines or more".into());
    };
    let parallel = |a: P2, b: P2| cross2(a, b).abs() <= PARALLEL * a.length() * b.length();
    if lines.windows(2).any(|w| parallel(w[0].1, w[1].1)) {
        return Err("two lines next to each other are parallel; an angular dimension measures between lines that cross".into());
    }
    let t = cross2(p1 - p0, d1) / cross2(d0, d1);
    let vertex = p0 + d0 * t;
    let toward = at - vertex;
    let rays: Vec<f64> = lines.iter().map(|(_, d)| if d.dot(toward) < 0.0 { -1.0 } else { 1.0 }).collect();
    let turns =
        lines.windows(2).zip(rays.windows(2)).map(|(w, s)| if cross2(w[0].1 * s[0], w[1].1 * s[1]) < 0.0 { -1.0 } else { 1.0 }).collect();
    if !(vertex.0.is_finite() && vertex.1.is_finite()) {
        return Err("its lines cross too far away to measure".into());
    }
    Ok(AngleSigns { rays, turns, vertex })
}

/// The form of GUID `g` an end of which is measured: an extrusion of a profile or a circle, on a plane or a reference
/// line's face.
fn end_form(f: &Family, g: Guid) -> Result<(usize, &Form), String> {
    let i = f.forms.iter().position(|x| x.guid == g).ok_or_else(|| format!("form {g} isn't one of its forms"))?;
    let form = &f.forms[i];
    if !form.mesh.is_empty() || form.axis().is_some() || form.kind().swept() {
        return Err(format!("form {}: only an extrusion's or a blend's start and end are measured", i + 1));
    }
    Ok((i, form))
}

fn end_expr(form: &Form, end: Extent) -> &Expr {
    match end {
        Extent::Start => &form.from,
        Extent::End => &form.to,
    }
}

/// Unit-length `a`, or none for a zero one.
fn unit3(a: P3) -> Option<P3> {
    let l = dot(a, a).sqrt();
    (l > 1e-12 && l.is_finite()).then(|| a.map(|x| x / l))
}

fn parallel(a: P3, b: P3) -> bool {
    let c = cross(a, b);
    dot(c, c).sqrt() <= PARALLEL
}

impl Family {
    /// Whether parameter `name` is a reporting one.
    fn reporting(&self, name: &str) -> bool {
        self.parameters.iter().any(|p| p.name == name && p.reporting)
    }

    /// Whether its dimensions are sound, whatever the values: no more than it may hold, each measuring between two or more of
    /// its references (each once; a radial or diameter one, one circle), parallel and lying across its view; a label naming a
    /// length parameter on a dimension of one segment; locks on segments it has; Align's lock of two references, locked at 0;
    /// and no label worked out from a reporting parameter another dimension measures, which would be measured from itself.
    /// Where a sketch line faces is a type's ([`Family::check_dimensions_for`]).
    pub(super) fn check_dimensions(&self) -> Result<(), String> {
        if self.dimensions.len() > MAX_DIMENSIONS {
            return Err(format!("it has more than {MAX_DIMENSIONS} dimensions"));
        }
        let mut guids = BTreeSet::new();
        for (i, d) in self.dimensions.iter().enumerate() {
            let who = format!("dimension {}", i + 1);
            if !d.guid.is_nil() && !guids.insert(d.guid) {
                return Err(format!("{who}: another dimension holds its GUID"));
            }
            if d.refs.len() < 2 && !d.kind.is_round() || d.refs.len() > MAX_DIM_REFS {
                return Err(format!("{who}: it measures between 2 to {MAX_DIM_REFS} references, not {}", d.refs.len()));
            }
            if d.kind.is_round() && d.refs.len() != 1 {
                return Err(format!("{who}: a radial or diameter dimension measures one circle, not {} references", d.refs.len()));
            }
            if !d.at.iter().all(|x| x.is_finite() && x.abs() <= MAX_COORD) {
                return Err(format!("{who}: its `at` is a point within {MAX_COORD} mm"));
            }
            let mut seen = BTreeSet::new();
            for r in &d.refs {
                if !seen.insert(format!("{r}")) {
                    return Err(format!("{who}: it measures to {r} twice"));
                }
                self.reference_ok(r, d.kind).map_err(|e| format!("{who}: {e}"))?;
            }
            if d.kind.is_aligned() && d.refs.iter().all(DimRef::is_point) {
                return Err(format!("{who}: it measures between points alone: give it a plane or a line to measure along"));
            }
            let problem = Problem::bare(self);
            if d.kind == DimKind::Angular {
                // Lines in its view, whatever the values: planes and reference lines (sketch lines are a type's).
                if d.equal && d.refs.len() < 3 {
                    return Err(format!("{who}: EQ keeps the angles of three lines or more equal"));
                }
                let mut seen = Vec::new();
                for r in &d.refs {
                    match r {
                        DimRef::Plane(_) | DimRef::Line(..) => {
                            let (p, w) = problem.in_view(r, d.view).map_err(|e| format!("{who}: {e}"))?;
                            seen.push((P2(p.0.v, p.1.v), P2(w.0.v, w.1.v)));
                        }
                        DimRef::Sketch(..) => {}
                        _ => return Err(format!("{who}: {ANGULAR_REFS}")),
                    }
                }
                if seen.len() == d.refs.len() {
                    let (a, b) = view_axes(d.view);
                    ray_signs(&seen, P2(dot(d.at, a), dot(d.at, b))).map_err(|e| format!("{who}: {e}"))?;
                }
            }
            // The ways what is drawn faces, whatever the values: planes, extrusion ends and reference lines.
            let mut normals = Vec::new();
            for r in d.refs.iter().filter(|r| d.kind.is_aligned() && matches!(r, DimRef::Plane(_) | DimRef::Form(..) | DimRef::Line(..))) {
                if let Some(n) = problem.direction(r).map_err(|e| format!("{who}: {e}"))? {
                    normals.push(n);
                }
            }
            if let Some(&m) = normals.first() {
                if normals.iter().any(|n| !parallel(*n, m)) {
                    return Err(format!("{who}: its references aren't parallel; a dimension measures between parallel ones"));
                }
                if dot(m, d.view.eye()).abs() > PARALLEL {
                    return Err(format!("{who}: it measures the way its view looks; draw it in a view its references are seen edge on in"));
                }
            }
            let segments = d.segments();
            if let Some(l) = &d.label {
                let p = self
                    .parameters
                    .iter()
                    .find(|p| &p.name == l)
                    .ok_or_else(|| format!("{who}: its label: {}", not_a_parameter(l, &self.parameters)))?;
                if d.kind == DimKind::Angular && p.kind != Kind::Angle {
                    return Err(format!("{who}: its label `{l}` is {}; an angular dimension's label is an angle", p.kind.said()));
                }
                if d.kind != DimKind::Angular && p.kind != Kind::Length {
                    return Err(format!("{who}: its label `{l}` is {}; a dimension's label is a length", p.kind.said()));
                }
                if segments != 1 {
                    return Err(format!("{who}: it has {segments} segments; only a dimension of one segment takes a label"));
                }
                if d.alignment {
                    return Err(format!("{who}: an Align lock takes no label"));
                }
            }
            for l in &d.locked {
                if l.segment >= segments {
                    return Err(format!("{who}: segment {} is locked; it has {segments}", l.segment));
                }
                if d.kind == DimKind::Angular && !(l.length.is_finite() && (0.0..MAX_ANGLE).contains(&l.length)) {
                    return Err(format!("{who}: a locked angle is from 0 up to {MAX_ANGLE}°, not {}", l.length));
                }
                if !l.length.is_finite() || l.length < 0.0 || l.length > MAX_COORD {
                    return Err(format!("{who}: a locked length is from 0 to {MAX_COORD} mm, not {}", l.length));
                }
            }
            if d.kind.is_round() && (d.equal || d.alignment) {
                return Err(format!("{who}: a radial or diameter dimension takes a label or a lock, no EQ and no Align"));
            }
            if d.kind == DimKind::Angular && d.alignment {
                return Err(format!("{who}: an angular dimension is no Align lock"));
            }
            if d.alignment && (segments != 1 || d.equal || !matches!(d.locked.as_slice(), [Lock { segment: 0, length }] if *length == 0.0))
            {
                return Err(format!("{who}: an Align lock holds two references, its one segment locked at 0"));
            }
        }
        // A label worked out from a reporting parameter measured by a dimension: the solve would read its own result.
        let measured: BTreeSet<&str> = self.dimensions.iter().filter_map(|d| d.label.as_deref()).filter(|l| self.reporting(l)).collect();
        if !measured.is_empty() {
            for (i, d) in self.dimensions.iter().enumerate() {
                let Some(l) = d.label.as_deref().filter(|l| !self.reporting(l)) else { continue };
                if let Some(r) = self.reads_from(l, &measured) {
                    return Err(format!(
                        "dimension {}: its label `{l}` is worked out from `{r}`, which a dimension measures: a label can't read what the dimensions come to",
                        i + 1
                    ));
                }
            }
        }
        Ok(())
    }

    /// Whether `r` is something of its own a dimension of `kind` measures to, whatever the values: a plane it has, an
    /// extrusion's end on a plane, a straight line of a sketch drawn on a plane (a circle, for a radial or diameter one), a
    /// line's end or a circle's centre, a reference line's face.
    fn reference_ok(&self, r: &DimRef, kind: DimKind) -> Result<(), String> {
        let line = |g: Guid, l: Guid| -> Result<(usize, usize, &FormLine), String> {
            let (i, form, _, _) = sketched(self, g)?;
            let (k, line) = sketch_line(form, i, l)?;
            Ok((i, k, line))
        };
        let who = |i: usize, k: usize| format!("form {}: line {}", i + 1, k + 1);
        let straight = |l: &FormLine| matches!(&l.curve, FormCurve::Line { turn: None | Some(Expr::Number(0.0)), .. });
        if kind.is_round() {
            let DimRef::Sketch(g, l) = *r else {
                return Err("a radial or diameter dimension measures a circle or an arc of a sketch".into());
            };
            let (i, k, l) = line(g, l)?;
            if straight(l) {
                return Err(format!("{}: a radial or diameter dimension measures a circle or an arc, not a straight line", who(i, k)));
            }
            return Ok(());
        }
        match *r {
            DimRef::Plane(_) | DimRef::Form(..) => Problem::bare(self).direction(r).map(|_| ()),
            DimRef::Sketch(g, l) => {
                let (i, k, l) = line(g, l)?;
                if straight(l) {
                    Ok(())
                } else if matches!(l.curve, FormCurve::Circle { .. }) {
                    Err(format!("{}: a circle; it is measured by its centre or by a radial dimension", who(i, k)))
                } else {
                    Err(format!(
                        "{}: an arc; a dimension measures to a straight line, an arc by its centre or a radial dimension",
                        who(i, k)
                    ))
                }
            }
            DimRef::Point(g, l, spot) => {
                let (i, k, l) = line(g, l)?;
                match (&l.curve, spot) {
                    (FormCurve::Circle { .. }, Spot::Centre) => Ok(()),
                    (FormCurve::Circle { .. }, _) => Err(format!("{}: a circle has a centre, no start or end", who(i, k))),
                    (_, Spot::Centre) if straight(l) => Err(format!("{}: a straight line has no centre", who(i, k))),
                    _ => Ok(()),
                }
            }
            DimRef::Line(g, face) => self.frame(&PlaneRef::Line(g, face)).map(|_| ()),
            DimRef::LinePoint(g, spot) => {
                self.frame(&PlaneRef::Line(g, Face::Horizontal))?;
                if spot == Spot::Centre {
                    return Err(format!("line {g}: a reference line has a start and an end, no centre"));
                }
                Ok(())
            }
        }
    }

    /// Whether its dimensions to sketches are sound for `values` (a type's): each sketch line they measure to has a length,
    /// faces the way the dimension's other references do and is seen edge on in its view; a radial or diameter one's circle
    /// is seen flat in its view.
    pub(super) fn check_dimensions_for(&self, values: &BTreeMap<String, Val>) -> Result<(), String> {
        let sketchy =
            |d: &&FamilyDimension| d.refs.iter().any(|r| matches!(r, DimRef::Sketch(..) | DimRef::Point(..) | DimRef::LinePoint(..)));
        if !self.dimensions.iter().any(|d| sketchy(&d)) {
            return Ok(());
        }
        let refs: Vec<DimRef> = self.dimensions.iter().filter(sketchy).flat_map(|d| d.refs.iter().copied()).collect();
        let mut read = HashMap::new();
        let problem = Problem::new(self, refs, values, &mut read, &mut Meter::default())?;
        for (i, d) in self.dimensions.iter().enumerate().filter(|(_, d)| sketchy(d)) {
            let who = format!("dimension {}", i + 1);
            if d.kind == DimKind::Angular {
                problem.angle_signs(d).map_err(|e| format!("{who}: {e}"))?;
                continue;
            }
            if d.kind.is_round() {
                let (_, _, frame) = problem.circle(d).map_err(|e| format!("{who}: {e}"))?;
                if !parallel(frame.z, d.view.eye()) {
                    return Err(format!("{who}: its circle isn't seen flat in its view; draw it in a view that looks square on to it"));
                }
                continue;
            }
            let m = problem.way(d).map_err(|e| format!("{who}: {e}"))?;
            for r in &d.refs {
                if let Some(n) = problem.direction(r).map_err(|e| format!("{who}: {e}"))?
                    && !parallel(n, m)
                {
                    return Err(format!("{who}: its references aren't parallel; a dimension measures between parallel ones"));
                }
            }
            if dot(m, d.view.eye()).abs() > PARALLEL {
                return Err(format!("{who}: it measures the way its view looks; draw it in a view its references are seen edge on in"));
            }
        }
        Ok(())
    }

    /// Of `among`, one that parameter `name`'s formula reads, at any depth.
    fn reads_from<'a>(&self, name: &str, among: &BTreeSet<&'a str>) -> Option<&'a str> {
        let mut todo = vec![name.to_string()];
        let mut seen = BTreeSet::new();
        while let Some(n) = todo.pop() {
            if !seen.insert(n.clone()) || seen.len() > MAX_PARAMETERS + 1 {
                continue;
            }
            let Some(f) = self.parameters.iter().find(|p| p.name == n).and_then(|p| p.formula.as_deref()) else { continue };
            for said in names(f).unwrap_or_default() {
                if let Some(hit) = among.iter().find(|m| **m == said) {
                    return Some(hit);
                }
                todo.push(said);
            }
        }
        None
    }

    /// Whether it has a dimension that constrains anything: else flexing has nothing to do.
    pub fn constrained(&self) -> bool {
        let reporting = |l: &str| self.reporting(l);
        self.dimensions.iter().any(|d| d.constrains(&reporting)) || !self.pivots().is_empty()
    }

    /// Its dimensions solved for `values`: where the planes, extrusion ends and sketch points they hold go, the least change
    /// from where they are drawn that holds them all.
    pub fn solve(&self, values: &BTreeMap<String, Val>) -> Result<Solution, Unsat> {
        self.solve_with(values, &[], &[], &mut Meter::default())
    }

    /// [`Family::solve`], `pins` held where they are said to be as well, the labels of the dimensions `free` (by place) not
    /// held, and the work counted on `meter`.
    fn solve_with(&self, values: &BTreeMap<String, Val>, pins: &[Pin], free: &[usize], meter: &mut Meter) -> Result<Solution, Unsat> {
        let reporting = |l: &str| self.reporting(l);
        if !self.constrained() && pins.is_empty() {
            return Ok(Solution::default());
        }
        let constraining: Vec<(usize, &FamilyDimension)> =
            self.dimensions.iter().enumerate().filter(|(_, d)| d.constrains(&reporting)).collect();
        let mut read = HashMap::new();
        // The sketches pivots are pinned to are read too (F7d).
        let pivot_refs: Vec<DimRef> = self
            .pivots()
            .into_iter()
            .filter_map(|(_, p)| match p.to {
                PivotPoint::Sketch(f, l, s) => Some(DimRef::Point(f, l, s)),
                _ => None,
            })
            .collect();
        let refs = constraining
            .iter()
            .flat_map(|(_, d)| d.refs.iter().copied())
            .chain(pins.iter().filter_map(|p| match p {
                Pin::Ref(r, ..) => Some(*r),
                Pin::Plane(..) => None,
            }))
            .chain(pivot_refs);
        let mut problem = Problem::new(self, refs, values, &mut read, meter).map_err(Unsat::from)?;
        // What may move: what a constraint or a pin holds, in the dimensions' order.
        let mut unknowns = Vec::new();
        {
            let mut add = |u: Unknown| {
                if !unknowns.contains(&u) {
                    unknowns.push(u);
                }
            };
            for (_, d) in &constraining {
                for r in &d.refs {
                    problem.unknowns_of(r, d.kind, &mut add);
                }
            }
            for p in pins {
                match p {
                    Pin::Plane(g, _) => problem.unknowns_of(&DimRef::Plane(*g), DimKind::Aligned, &mut add),
                    Pin::Ref(r, ..) => problem.unknowns_of(r, DimKind::Aligned, &mut add),
                }
            }
            // What pivots turn: a line's start, angle and length, a plane's turn (F7d).
            for (e, _) in self.pivots() {
                match e {
                    Pivoted::Line(i) => {
                        add(Unknown::LineStart(i, 0));
                        add(Unknown::LineStart(i, 1));
                        add(Unknown::LineAngle(i));
                        add(Unknown::LineLength(i));
                    }
                    Pivoted::Plane(i) => add(Unknown::PlaneTurn(i)),
                }
            }
        }
        let mut x0 = Vec::with_capacity(unknowns.len());
        for u in &unknowns {
            x0.push(match *u {
                Unknown::Plane(i) => self.planes.get(i).map_or(0.0, |p| dot(p.start, p.frame().z)),
                Unknown::End(i, end) => match self.forms.get(i) {
                    Some(f) => number(end_expr(f, end), values, &mut read).map_err(|e| format!("form {}: {e}", i + 1))?,
                    None => 0.0,
                },
                Unknown::Vertex(i, v, axis) => {
                    let p = problem.drawn(i, v);
                    if axis == 0 { p.0 } else { p.1 }
                }
                Unknown::Radius(i, k) => problem.layouts.get(&i).and_then(|l| l.radius.get(k)).copied().unwrap_or(0.0),
                Unknown::LineStart(i, axis) => problem.line_drawn(i).map_err(Unsat::from)?[axis],
                Unknown::LineAngle(i) => problem.line_drawn(i).map_err(Unsat::from)?[4],
                Unknown::LineLength(i) => problem.line_drawn(i).map_err(Unsat::from)?[5],
                Unknown::Turn(i, k) => problem.layouts.get(&i).and_then(|l| l.turn.get(k)).copied().unwrap_or(0.0).to_radians(),
                Unknown::PlaneTurn(_) => 0.0,
            });
        }
        (problem.unknowns, problem.x0) = (unknowns, x0);
        problem.at = problem.x0.clone();
        // Lines and arcs read through sines, and angles between sketch lines through their vertices: non-linear (F7c).
        let turns = problem.unknowns.iter().any(Unknown::turns)
            || constraining.iter().any(|(_, d)| d.kind == DimKind::Angular && d.refs.iter().any(|r| matches!(r, DimRef::Sketch(..))));
        let mut memo = Memo { record: true, ..Memo::default() };
        let mut rows = self.rows(&problem, &constraining, pins, free, values, &mut read, &mut memo)?;
        // An extrusion's end, a sketch's vertex, a circle's radius and an arc's turn give way before a plane or a reference
        // line does: what a plane or a line holds them to, they follow. A line's angle gives way as its end moves.
        let give: Vec<f64> = problem
            .unknowns
            .iter()
            .map(|u| match u {
                Unknown::Plane(_) | Unknown::LineStart(..) | Unknown::LineLength(_) => 1.0,
                Unknown::LineAngle(i) => 1.0 / problem.line_drawn(*i).map_or(1.0, |d| d[5]).max(1e-3),
                Unknown::PlaneTurn(i) => {
                    let p = self.planes.get(*i);
                    1.0 / p.map_or(1.0, |p| dot(sub(p.end, p.start), sub(p.end, p.start)).sqrt()).max(1e-3)
                }
                Unknown::Turn(i, k) => {
                    let r = problem.layouts.get(i).and_then(|l| l.radius.get(*k)).copied().unwrap_or(1.0);
                    END_GIVES / r.max(1e-3)
                }
                _ => END_GIVES,
            })
            .collect();
        let x = if turns {
            // Gauss-Newton on the least change: each step holds the rows linearised where it has got to, with the least
            // change from where they are drawn; a step that doesn't bring them nearer is halved back.
            let x0 = problem.x0.clone();
            let (mut x, dropped) = solve_rows(&x0, &give, &rows, meter, &[], false)?;
            let size = x0.iter().fold(0.0f64, |a, v| a.max(v.abs()));
            let tol = 1e-9 * (size / 1000.0).max(1.0);
            let off = |rows: &[Row], x: &[f64]| -> f64 {
                let s: f64 = rows.iter().enumerate().filter(|(k, _)| !dropped.contains(k)).map(|(_, r)| r.lin.at(x).powi(2)).sum();
                if s.is_finite() { s.sqrt() } else { f64::INFINITY }
            };
            let mut again = |x: &[f64], meter: &mut Meter| -> Option<Vec<Row>> {
                if !x.iter().all(|v| v.is_finite() && v.abs() <= 4.0 * MAX_COORD) || meter.spend(64 * x.len() as u64 + 64).is_err() {
                    return None;
                }
                problem.at = x.to_vec();
                memo.replay();
                self.rows(&problem, &constraining, pins, free, values, &mut read, &mut memo).ok()
            };
            let mut gone = None;
            match again(&x, meter) {
                Some(r) => rows = r,
                None => gone = Some(0.0),
            }
            // Until the rows hold and the step comes to nothing: held is not yet the least change (a step from a far
            // linearisation may have moved what the rows no longer read), so it goes on to the point that is its own step.
            let mut now = off(&rows, &x);
            let mut steps = 0;
            while gone.is_none() {
                steps += 1;
                if steps > MAX_STEPS {
                    if now > tol {
                        gone = Some(now);
                    }
                    break;
                }
                let (next, _) = solve_rows(&x0, &give, &rows, meter, &dropped, true)?;
                let step = x.iter().zip(&next).fold(0.0f64, |a, (p, q)| a.max((p - q).abs()));
                if now <= tol && step <= tol {
                    break;
                }
                let mut alpha = 1.0;
                let mut took = None;
                for _ in 0..=MAX_HALVINGS {
                    let cand: Vec<f64> = x.iter().zip(&next).map(|(a, b)| a + alpha * (b - a)).collect();
                    if let Some(r) = again(&cand, meter) {
                        let o = off(&r, &cand);
                        if o < now || o <= tol {
                            took = Some((cand, r, o));
                            break;
                        }
                    }
                    alpha *= 0.5;
                }
                match took {
                    Some((c, r, o)) => (x, rows, now) = (c, r, o),
                    None if now <= tol => break,
                    None => gone = Some(now),
                }
            }
            if gone.is_some() {
                let mut dims: Vec<usize> = rows
                    .iter()
                    .filter(|r| r.lin.at(&x).abs().partial_cmp(&tol) != Some(std::cmp::Ordering::Less))
                    .filter_map(|r| r.dim)
                    .collect();
                if dims.is_empty() {
                    dims = rows.iter().filter_map(|r| r.dim).collect();
                }
                dims.sort_unstable();
                dims.dedup();
                let named = dims.iter().map(|d| (d + 1).to_string()).collect::<Vec<_>>().join(", ");
                return Err(Unsat {
                    message: format!("{NOT_SATISFIED}: they can't all hold for these values (dimension {named})"),
                    dims,
                    over: false,
                });
            }
            x
        } else {
            solve_rows(&problem.x0, &give, &rows, meter, &[], false)?.0
        };
        // To a millionth of a millimetre, so what the arithmetic leaves over (500.00000000000006) doesn't show; angles as
        // they come.
        let x: Vec<f64> =
            x.into_iter()
                .zip(&problem.unknowns)
                .map(|(v, u)| {
                    if matches!(u, Unknown::LineAngle(_) | Unknown::Turn(..) | Unknown::PlaneTurn(_)) { v } else { (v * 1e6).round() / 1e6 }
                })
                .collect();
        if x.iter().any(|v| !v.is_finite() || v.abs() > MAX_COORD) {
            return Err(Unsat {
                message: format!("{NOT_SATISFIED}: they would put a plane, a line, a form's end or a sketch past {MAX_COORD} mm"),
                dims: Vec::new(),
                over: false,
            });
        }
        let refuse = |message: String| Err(Unsat { message: format!("{NOT_SATISFIED}: {message}"), dims: Vec::new(), over: false });
        for (u, v) in problem.unknowns.iter().zip(&x) {
            match *u {
                Unknown::Radius(i, _) if *v <= 0.0 => {
                    return refuse(format!("they would give a circle of form {} a radius of {v} mm", i + 1));
                }
                Unknown::LineLength(i) if *v < 1e-3 => {
                    return refuse(format!("they would give reference line {} a length of {v} mm", i + 1));
                }
                Unknown::Turn(i, _) if !(v.abs() > 1e-9 && v.abs() < std::f64::consts::TAU) => {
                    return refuse(format!("they would make an arc of form {} straight or a whole circle", i + 1));
                }
                _ => {}
            }
        }
        let mut out = Solution::default();
        let mut vertices: BTreeMap<(usize, usize), P2> = BTreeMap::new();
        let mut lines: BTreeMap<usize, [f64; 4]> = BTreeMap::new();
        for (u, v) in problem.unknowns.iter().zip(&x) {
            match *u {
                Unknown::Plane(i) => out.planes.push((i, *v)),
                Unknown::End(i, end) => out.ends.push((i, end, *v)),
                Unknown::Vertex(i, vx, axis) => {
                    let p = vertices.entry((i, vx)).or_insert_with(|| problem.drawn(i, vx));
                    if axis == 0 {
                        p.0 = *v;
                    } else {
                        p.1 = *v;
                    }
                }
                Unknown::Radius(i, k) => out.radii.push((i, k, *v)),
                Unknown::LineStart(i, axis) => lines.entry(i).or_default()[axis] = *v,
                Unknown::LineAngle(i) => lines.entry(i).or_default()[2] = *v,
                Unknown::LineLength(i) => lines.entry(i).or_default()[3] = *v,
                Unknown::Turn(i, k) => out.turns.push((i, k, v.to_degrees())),
                Unknown::PlaneTurn(_) => {}
            }
        }
        out.lines = lines.into_iter().collect();
        // Each plane a pivot turned, its axes where it now stands (F7d).
        problem.at = x.clone();
        for (u, v) in problem.unknowns.iter().zip(&x) {
            if let Unknown::PlaneTurn(i) = *u {
                let f = problem.plane_frame_ad(i, DEPTH).map_err(Unsat::from)?;
                let val = |a: &V3| [a[0].v, a[1].v, a[2].v];
                let frame = Frame { origin: val(&f.origin), x: val(&f.x), y: val(&f.y), z: val(&f.z) };
                if !frame.origin.iter().chain(&frame.x).chain(&frame.z).all(|c| c.is_finite() && c.abs() <= MAX_COORD) {
                    return refuse(format!("they would put {} past {MAX_COORD} mm", self.pivoted_said(Pivoted::Plane(i))));
                }
                out.turned_planes.push((i, frame, *v));
            }
        }
        for ((i, vx), p) in vertices {
            let at = problem.layouts.get(&i).and_then(|l| l.points.get(vx));
            for (k, spot) in at.into_iter().flatten() {
                out.points.push((i, *k, *spot, p));
            }
        }
        Ok(out)
    }

    /// The rows of a solve, linearised where `problem` has got to: dimension after dimension, then the pins, then the soft
    /// rows keeping sketch lines and reference lines measured across parallel. What depends on where things are drawn (the
    /// way a segment runs, the sector an angle is) is read the first time (`memo` recording, at the drawn point) and kept.
    #[allow(clippy::too_many_arguments)]
    fn rows<'a>(
        &'a self,
        problem: &Problem<'a>,
        constraining: &[(usize, &FamilyDimension)],
        pins: &[Pin],
        free: &[usize],
        values: &BTreeMap<String, Val>,
        read: &mut HashMap<&'a str, Result<Node, String>>,
        memo: &mut Memo,
    ) -> Result<Vec<Row>, Unsat> {
        let reporting = |l: &str| self.reporting(l);
        let mut rows: Vec<Row> = Vec::new();
        let mut soft: Vec<(Guid, Guid)> = Vec::new();
        let mut keep: Vec<usize> = Vec::new();
        let mut turned_soft: Vec<Row> = Vec::new();
        for &(k, d) in constraining {
            let who = format!("dimension {}", k + 1);
            let at = |e: String| Unsat::from(format!("{who}: {e}"));
            let angular = d.kind == DimKind::Angular;
            // Each segment, the way it runs as drawn: a label or a lock is a length (an angle) along it.
            let turning = d.kind.is_aligned().then(|| {
                d.refs.iter().find_map(|r| match r {
                    DimRef::Plane(g) => problem.turning_plane(*g).map(|_| *g),
                    _ => None,
                })
            });
            let segments: Vec<Lin> = if let Some(Some(g)) = turning {
                // An aligned dimension to a plane a pivot turns measures along its normal as it turns (F7d).
                let stations = problem.turned_stations(d, g, values, read, &mut turned_soft).map_err(at)?;
                let flips = memo
                    .take(|| {
                        Ok(stations
                            .windows(2)
                            .map(|w| if w[1].clone().minus(&w[0]).at(&problem.at) < -1e-9 { -1.0 } else { 1.0 })
                            .collect())
                    })
                    .map_err(at)?;
                stations.windows(2).zip(flips).map(|(w, f)| w[1].clone().minus(&w[0]).scaled(f)).collect()
            } else if d.kind.is_aligned() {
                let m = problem.way(d).map_err(at)?;
                let mut stations: Vec<Lin> = Vec::new();
                for r in &d.refs {
                    stations.push(problem.position(r, m, values, read).map_err(at)?);
                    if let DimRef::Sketch(g, l) = *r
                        && !soft.contains(&(g, l))
                    {
                        soft.push((g, l));
                    }
                    if let DimRef::Line(g, _) = *r
                        && let Some(i) = self.lines.iter().position(|l| l.guid == g)
                        && problem.unknown(Unknown::LineAngle(i)).is_some()
                        && !keep.contains(&i)
                    {
                        keep.push(i);
                    }
                }
                let flips = memo
                    .take(|| {
                        Ok(stations
                            .windows(2)
                            .map(|w| if w[1].clone().minus(&w[0]).at(&problem.at) < -1e-9 { -1.0 } else { 1.0 })
                            .collect())
                    })
                    .map_err(at)?;
                stations.windows(2).zip(flips).map(|(w, f)| w[1].clone().minus(&w[0]).scaled(f)).collect()
            } else if angular {
                let n = d.refs.len();
                let saved = memo
                    .take(|| {
                        let s = problem.angle_signs(d)?;
                        Ok(s.rays.into_iter().chain(s.turns).collect())
                    })
                    .map_err(at)?;
                let signs = AngleSigns {
                    rays: saved.iter().take(n).copied().collect(),
                    turns: saved.iter().skip(n).copied().collect(),
                    ..AngleSigns::default()
                };
                problem.angles(d, &signs).map_err(at)?.into_iter().map(|a| a.lin(&problem.at)).collect()
            } else {
                problem.circle(d).map_err(at)?;
                let Some(DimRef::Sketch(g, l)) = d.refs.first() else { continue };
                let s = problem.seen(*g, *l).map_err(at)?;
                if s.shape == Shape::Arc {
                    vec![problem.arc_ad(&s).1.scale(d.kind.times()).lin(&problem.at)]
                } else {
                    match problem.unknown(Unknown::Radius(s.form, s.line)) {
                        Some(u) => vec![Lin::of(u, d.kind.times())],
                        None => continue,
                    }
                }
            };
            let labelled = d.label.as_deref().filter(|l| !reporting(l) && !free.contains(&k));
            if let (Some(l), Some(s)) = (labelled, segments.first()) {
                let v = match values.get(l) {
                    Some(Val::Number(v)) => *v,
                    _ if angular => return Err(format!("{who}: its label `{l}` has no angle").into()),
                    _ => return Err(format!("{who}: its label `{l}` has no length").into()),
                };
                if angular && !(v.is_finite() && (0.0..MAX_ANGLE).contains(&v)) {
                    return Err(Unsat {
                        message: format!(
                            "{NOT_SATISFIED}: dimension {}'s label `{l}` is {v}°; a labelled angle is from 0 up to {MAX_ANGLE}°",
                            k + 1
                        ),
                        dims: vec![k],
                        over: false,
                    });
                }
                if !angular && (!v.is_finite() || v <= 0.0 || v > MAX_COORD) {
                    return Err(Unsat {
                        message: format!(
                            "{NOT_SATISFIED}: dimension {}'s label `{l}` is {v} mm; a labelled length is above 0 and within {MAX_COORD} mm",
                            k + 1
                        ),
                        dims: vec![k],
                        over: false,
                    });
                }
                let mut lin = s.clone();
                lin.c -= if angular { v.to_radians() } else { v };
                rows.push(Row::hard(lin, Some(k)));
            }
            for lock in &d.locked {
                if labelled.is_some() && lock.segment == 0 {
                    continue;
                }
                if let Some(s) = segments.get(lock.segment) {
                    let mut lin = s.clone();
                    lin.c -= if angular { lock.length.to_radians() } else { lock.length };
                    rows.push(Row::hard(lin, Some(k)));
                }
            }
            if d.equal
                && let Some(first) = segments.first()
            {
                for s in segments.iter().skip(1) {
                    rows.push(Row::hard(s.clone().minus(first), Some(k)));
                }
            }
        }
        for pin in pins {
            match *pin {
                Pin::Plane(g, at) => {
                    let (_, mut lin) = problem.plane_pos(g).map_err(Unsat::from)?;
                    lin.c -= at;
                    rows.push(Row::hard(lin, None));
                }
                Pin::Ref(r, m, at) => {
                    let mut lin = problem.position(&r, m, values, read).map_err(Unsat::from)?;
                    lin.c -= at;
                    rows.push(Row::hard(lin, None));
                    // A line both its ends.
                    if let DimRef::Sketch(g, l) = r {
                        let s = problem.seen(g, l).map_err(Unsat::from)?;
                        let mut lin = problem.vertex_along(&s, s.ends.1, m).map_err(Unsat::from)?;
                        lin.c -= at;
                        rows.push(Row::hard(lin, None));
                    }
                }
            }
        }
        // The pivots (F7d): their hard rows, then their own angles and the sketch lines kept parallel to turning planes, soft,
        // before F7b's and F7c's soft rows.
        let mut pivot_soft = Vec::new();
        problem.pivot_rows(values, &mut rows, &mut pivot_soft)?;
        rows.extend(pivot_soft);
        let parallel_to_turned: Vec<(Guid, Guid)> = constraining
            .iter()
            .filter(|(_, d)| {
                d.kind.is_aligned() && d.refs.iter().any(|r| matches!(r, DimRef::Plane(g) if problem.turning_plane(*g).is_some()))
            })
            .flat_map(|(_, d)| d.refs.iter())
            .filter_map(|r| match r {
                DimRef::Sketch(g, l) => Some((*g, *l)),
                _ => None,
            })
            .collect();
        rows.extend(turned_soft);
        for (g, l) in soft.into_iter().filter(|x| !parallel_to_turned.contains(x)) {
            let s = problem.seen(g, l).map_err(Unsat::from)?;
            let n = problem.across(&s).map_err(Unsat::from)?;
            let a = problem.vertex_along(&s, s.ends.0, n).map_err(Unsat::from)?;
            let b = problem.vertex_along(&s, s.ends.1, n).map_err(Unsat::from)?;
            rows.push(Row { lin: b.minus(&a), dim: None, soft: true, pivot: None });
        }
        // A reference line measured across keeps its angle on what it is drawn on, unless the rows before turn it (F7c).
        for i in keep {
            if let Some(u) = problem.unknown(Unknown::LineAngle(i)) {
                let drawn = problem.x0.get(u).copied().unwrap_or(0.0);
                rows.push(Row { lin: Lin { terms: vec![(u, 1.0)], c: -drawn }, dim: None, soft: true, pivot: None });
            }
        }
        Ok(rows)
    }

    /// The family flexed for `values`: its planes where its dimensions put them (and the lines and forms drawn on them with
    /// them), its extrusions' ends and its sketches' points the constraints set given as numbers. Itself, borrowed, when it
    /// holds no constraint.
    pub fn flexed(&self, values: &BTreeMap<String, Val>) -> Result<Cow<'_, Family>, Unsat> {
        self.flexed_with(values, &mut Meter::default())
    }

    pub(super) fn flexed_with(&self, values: &BTreeMap<String, Val>, meter: &mut Meter) -> Result<Cow<'_, Family>, Unsat> {
        if !self.constrained() {
            return Ok(Cow::Borrowed(self));
        }
        let s = self.solve_with(values, &[], &[], meter)?;
        Ok(Cow::Owned(self.moved(&s)))
    }

    /// A copy with solution `s` applied.
    fn moved(&self, s: &Solution) -> Family {
        let mut f = self.clone();
        // Each plane that went: by how much, or turned (none).
        let mut shift: Vec<(Guid, Option<P3>)> = Vec::new();
        for (i, v) in &s.planes {
            let Some(p) = f.planes.get_mut(*i) else { continue };
            let z = p.frame().z;
            let d = v - dot(p.start, z);
            if d.abs() > 0.0 {
                let by = mul(z, d);
                p.start = add(p.start, by);
                p.end = add(p.end, by);
                shift.push((p.guid, Some(by)));
            }
        }
        // A plane its pivot turned takes its axes as they now stand, its line moved with them (F7d).
        for (i, frame, phi) in &s.turned_planes {
            let Some(p) = f.planes.get_mut(*i) else { continue };
            let old = p.frame();
            let carry = |q: P3| {
                let (l, t) = old.local(q);
                frame.point(l, t)
            };
            p.start = carry(p.start);
            p.end = carry(p.end);
            p.turned = Some(*frame);
            if let Some(piv) = p.pivot.as_mut() {
                piv.turned += phi;
            }
            shift.push((p.guid, None));
        }
        // A reference line lies on what it is drawn on: moved with it, and what is drawn on it with it. One the constraints
        // set goes where they put it, in the axes of what it is drawn on as that now stands (F7c); one drawn on a line that
        // turned turns with it.
        if !shift.is_empty() || !s.lines.is_empty() {
            // Each line that went: by how much, or turned (none).
            let mut lines: Vec<(Guid, Option<P3>)> = Vec::new();
            for k in 0..f.lines.len() {
                let l = f.lines[k].clone();
                let host = match l.host {
                    PlaneRef::Plane(g) => shift.iter().find(|(h, _)| *h == g).map(|(_, b)| *b),
                    PlaneRef::Line(g, _) => lines.iter().find(|(h, _)| *h == g).map(|(_, b)| *b),
                };
                let solved = s.lines.iter().find(|(i, _)| *i == k).map(|(_, v)| *v);
                let (Ok(old), Ok(new)) = (self.frame(&l.host), f.frame(&l.host)) else { continue };
                let (start, end, how) = match (solved, host) {
                    (Some([su, sv, th, len]), _) => {
                        let ((a, h0), (b, h1)) = (old.local(l.start), old.local(l.end));
                        // How far it turned on what it is drawn on, for its pivot (F7d).
                        if let Some(piv) = f.lines[k].pivot.as_mut() {
                            let was = (b.1 - a.1).atan2(b.0 - a.0);
                            let d = th - was;
                            piv.turned += d - (d / std::f64::consts::TAU).round() * std::f64::consts::TAU;
                        }
                        let start = new.point(P2(su, sv), h0);
                        (start, new.point(P2(su + len * th.cos(), sv + len * th.sin()), h1), None)
                    }
                    (None, Some(Some(by))) => (add(l.start, by), add(l.end, by), Some(by)),
                    (None, Some(None)) => {
                        let move_ = |p: P3| {
                            let (q, t) = old.local(p);
                            new.point(q, t)
                        };
                        (move_(l.start), move_(l.end), None)
                    }
                    (None, None) => continue,
                };
                // To a billionth of a millimetre, so a quarter turn's cosine (6e-17) doesn't show.
                let tidy = |p: P3| p.map(|v| (v * 1e9).round() / 1e9);
                f.lines[k].start = if how.is_some() { start } else { tidy(start) };
                f.lines[k].end = if how.is_some() { end } else { tidy(end) };
                lines.push((l.guid, how));
            }
        }
        for (i, end, v) in &s.ends {
            if let Some(form) = f.forms.get_mut(*i) {
                let e = match end {
                    Extent::Start => &mut form.from,
                    Extent::End => &mut form.to,
                };
                *e = Expr::Number(*v);
            }
        }
        let n = |p: &P2| [Expr::Number(p.0), Expr::Number(p.1)];
        for (i, k, spot, p) in &s.points {
            let Some(line) = f.forms.get_mut(*i).and_then(|x| x.plane_line_mut(*k)) else { continue };
            match (&mut line.curve, spot) {
                (FormCurve::Line { start, .. }, Spot::Start) => *start = n(p),
                (FormCurve::Line { end, .. }, Spot::End) => *end = n(p),
                (FormCurve::Circle { centre, .. }, Spot::Centre) => *centre = n(p),
                _ => {}
            }
        }
        for (i, k, r) in &s.radii {
            if let Some(FormCurve::Circle { radius, .. }) = f.forms.get_mut(*i).and_then(|x| x.plane_line_mut(*k)).map(|l| &mut l.curve) {
                *radius = Expr::Number(*r);
            }
        }
        for (i, k, t) in &s.turns {
            if let Some(FormCurve::Line { turn, .. }) = f.forms.get_mut(*i).and_then(|x| x.plane_line_mut(*k)).map(|l| &mut l.curve) {
                *turn = Some(Expr::Number(*t));
            }
        }
        f
    }

    /// Where each reference of aligned dimension `d` is along the way it measures, for `values`, as the family stands
    /// (flexed or not): the direction (that of its first reference with one) and each one's place along it.
    pub fn stations(&self, d: &FamilyDimension, values: &BTreeMap<String, Val>) -> Result<(P3, Vec<f64>), String> {
        let mut read = HashMap::new();
        let problem = Problem::new(self, d.refs.iter().copied(), values, &mut read, &mut Meter::default())?;
        problem.stations(d, values, &mut read)
    }

    /// Each segment's length of each of its dimensions (an angular one's angles, in degrees), flexed for `values` (empty for
    /// one that can't be measured).
    pub fn measured(&self, values: &BTreeMap<String, Val>) -> Result<Vec<Vec<f64>>, Unsat> {
        let f = self.flexed(values)?;
        let mut read = HashMap::new();
        let all = f.dimensions.iter().flat_map(|d| d.refs.iter().copied());
        let Ok(problem) = Problem::new(&f, all, values, &mut read, &mut Meter::default()) else {
            return Ok(vec![Vec::new(); f.dimensions.len()]);
        };
        let mut drawn_read = HashMap::new();
        let all = self.dimensions.iter().filter(|d| d.kind == DimKind::Angular).flat_map(|d| d.refs.iter().copied());
        let drawn = Problem::new(self, all, values, &mut drawn_read, &mut Meter::default()).ok();
        Ok(f.dimensions
            .iter()
            .enumerate()
            .map(|(k, d)| {
                if d.kind == DimKind::Angular {
                    let signs = drawn.as_ref().zip(self.dimensions.get(k)).and_then(|(p, d)| p.angle_signs(d).ok());
                    signs.and_then(|s| problem.angles(d, &s).ok()).map(|a| a.iter().map(|x| degrees(x.v)).collect()).unwrap_or_default()
                } else if d.kind.is_aligned() {
                    problem
                        .stations(d, values, &mut read)
                        .map(|(_, p)| p.windows(2).map(|w| (w[1] - w[0]).abs()).collect())
                        .unwrap_or_default()
                } else {
                    problem.circle(d).map(|(_, r, _)| vec![r * d.kind.times()]).unwrap_or_default()
                }
            })
            .collect())
    }

    /// The values its reporting parameters measured by dimensions come to, for `values`: each labelled dimension's length.
    pub(super) fn reported(&self, values: &BTreeMap<String, Val>) -> Vec<(String, f64)> {
        if !self.dimensions.iter().any(|d| d.label.as_deref().is_some_and(|l| self.reporting(l))) {
            return Vec::new();
        }
        let Ok(lengths) = self.measured(values) else { return Vec::new() };
        self.dimensions
            .iter()
            .zip(lengths)
            .filter_map(|(d, l)| Some((d.label.clone().filter(|l| self.reporting(l))?, *l.first()?)))
            .collect()
    }

    /// Plane `g` dragged to `to` along its normal, for `values`: what changes, as Revit does it. A labelled dimension that
    /// measures to it (unless it is locked, or its parameter is worked out by a formula) takes the length the drag gives it,
    /// its parameter changed for the type; the plane and what the constraints move with it go where the solve puts them;
    /// one the constraints hold where it is can't move, and says which hold it.
    pub fn drag_plane(&self, values: &BTreeMap<String, Val>, g: Guid, to: f64) -> Result<Vec<Edit>, Unsat> {
        // From where the type has them, not where they are drawn: what the drag doesn't move stays where it is seen.
        self.flexed(values)?.drag_from(values, g, to)
    }

    fn drag_from(&self, values: &BTreeMap<String, Val>, g: Guid, to: f64) -> Result<Vec<Edit>, Unsat> {
        let Some(i) = self.planes.iter().position(|p| p.guid == g) else { return Err(format!("plane {g} isn't one of its planes").into()) };
        if self.planes[i].pivot.is_some() {
            return Err(Unsat {
                message: format!("{NOT_SATISFIED}: the plane pivots: turn it by its rotate grip"),
                dims: Vec::new(),
                over: true,
            });
        }
        if self.planes[i].datum() || self.planes[i].defines_origin {
            return Err(Unsat {
                message: format!("{NOT_SATISFIED}: the plane defines the origin, which stays"),
                dims: Vec::new(),
                over: true,
            });
        }
        let mut meter = Meter::default();
        // The labels it may change: of the dimensions measuring to it, free to take another length.
        let free: Vec<usize> = self
            .dimensions
            .iter()
            .enumerate()
            .filter(|(_, d)| d.kind.is_aligned() && d.refs.contains(&DimRef::Plane(g)) && !d.is_locked(0))
            .filter(|(_, d)| {
                d.label.as_deref().is_some_and(|l| self.parameters.iter().any(|p| p.name == l && p.formula.is_none() && !p.reporting))
            })
            .map(|(k, _)| k)
            .collect();
        let pin = [Pin::Plane(g, to)];
        if !free.is_empty()
            && let Ok(s) = self.solve_with(values, &pin, &free, &mut meter)
        {
            let moved = self.moved(&s);
            let mut out = Vec::new();
            for &k in &free {
                let Some(d) = self.dimensions.get(k) else { continue };
                let Some(l) = d.label.as_deref() else { continue };
                let Some(length) = moved.stations(d, values).ok().and_then(|(_, p)| Some((p.get(1)? - p.first()?).abs())) else { continue };
                if length <= 0.0 || length > MAX_COORD {
                    return Err(Unsat {
                        message: format!("{NOT_SATISFIED}: dimension {}'s `{l}` would be {length} mm", k + 1),
                        dims: vec![k],
                        over: false,
                    });
                }
                let was = match values.get(l) {
                    Some(Val::Number(v)) => *v,
                    _ => f64::NAN,
                };
                if (length - was).abs() > 1e-9 {
                    out.push(Edit::Parameter(l.to_string(), length));
                }
            }
            out.extend(self.edits(&s, values));
            return Ok(out);
        }
        match self.solve_with(values, &pin, &[], &mut meter) {
            Ok(s) => Ok(self.edits(&s, values)),
            Err(e) if e.over => Err(Unsat { message: format!("{NOT_SATISFIED}: {}", e.message), ..e }),
            Err(e) => Err(e),
        }
    }

    /// The edits that make the draft what solution `s` of this family (flexed for the type) says: every plane it solved, and
    /// the sketch points it moved.
    fn edits(&self, s: &Solution, values: &BTreeMap<String, Val>) -> Vec<Edit> {
        let mut out: Vec<Edit> = s.planes.iter().filter_map(|(i, v)| Some(Edit::Plane(self.planes.get(*i)?.guid, *v))).collect();
        // Reference lines the constraints moved or turned, and those drawn on them (F7c).
        if !s.lines.is_empty() {
            let moved = self.moved(s);
            for (was, now) in self.lines.iter().zip(&moved.lines) {
                if was.start != now.start || was.end != now.end {
                    // A pivoted line is drawn unturned: its pivot turns it again as it flexes (F7d).
                    let (start, end) = match (&now.pivot, moved.frame(&now.host)) {
                        (Some(piv), Ok(h)) if piv.turned != 0.0 => {
                            let ((a, ha), (b, hb)) = (h.local(now.start), h.local(now.end));
                            let len = a.dist(b);
                            let way = (b - a) * (1.0 / len.max(1e-12));
                            let at = a + way * piv.at.distance(len);
                            let (c, sn) = ((-piv.turned).cos(), (-piv.turned).sin());
                            let back = |q: P2| {
                                let d = q - at;
                                at + P2(d.0 * c - d.1 * sn, d.0 * sn + d.1 * c)
                            };
                            (h.point(back(a), ha), h.point(back(b), hb))
                        }
                        _ => (now.start, now.end),
                    };
                    out.push(Edit::LineEnds(was.guid, start, end));
                }
            }
        }
        for (i, k, t) in &s.turns {
            out.push(Edit::Turn(*i, *k, *t));
        }
        let mut read = HashMap::new();
        for (i, k, spot, p) in &s.points {
            let Some(line) = self.forms.get(*i).and_then(|f| f.plane_line(*k)) else { continue };
            let was = match (&line.curve, spot) {
                (FormCurve::Line { start, .. }, Spot::Start) => start,
                (FormCurve::Line { end, .. }, Spot::End) => end,
                (FormCurve::Circle { centre, .. }, Spot::Centre) => centre,
                _ => continue,
            };
            let (x, y) = (number(&was[0], values, &mut read), number(&was[1], values, &mut read));
            let unmoved = matches!((x, y), (Ok(x), Ok(y)) if P2(x, y).dist(*p) <= 1e-9);
            if !unmoved {
                out.push(Edit::Point(*i, *k, *spot, [p.0, p.1]));
            }
        }
        out
    }

    /// The constraints of dimensions `dims` (by place) taken off, as Revit's Remove Constraints does: their labels, locks and
    /// equality; an Align lock goes altogether.
    pub fn remove_constraints(&mut self, dims: &[usize]) {
        for &k in dims {
            if let Some(d) = self.dimensions.get_mut(k) {
                d.label = None;
                d.locked.clear();
                d.equal = false;
            }
        }
        let mut k = 0;
        self.dimensions.retain(|d| {
            let gone = dims.contains(&k) && d.alignment;
            k += 1;
            !gone
        });
    }

    /// What its dimensions measure to that isn't there: each dimension (by place) with a reference it has no plane, form,
    /// sketch line or reference line for.
    pub fn dangling_dimensions(&self) -> Vec<usize> {
        let line = |g: Guid, l: Guid| self.forms.iter().any(|f| f.guid == g && f.plane_lines().any(|x| x.guid == l));
        self.dimensions
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                d.refs.iter().any(|r| match *r {
                    DimRef::Plane(g) => !self.planes.iter().any(|p| p.guid == g) && !DATUMS.contains(&g),
                    DimRef::Form(g, _) => !self.forms.iter().any(|f| f.guid == g),
                    DimRef::Sketch(g, l) | DimRef::Point(g, l, _) => !line(g, l),
                    DimRef::Line(g, _) | DimRef::LinePoint(g, _) => !self.lines.iter().any(|x| x.guid == g),
                })
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Reference `gone` taken from its dimensions, as deleting the plane, form or sketch line does in Revit: each dimension
    /// measuring to it loses it (its locks renumbered, its label and equality kept while it still has segments), and one left
    /// with too few references goes. How many went.
    pub fn forget_reference(&mut self, gone: Guid) -> usize {
        let before = self.dimensions.len();
        for d in &mut self.dimensions {
            while let Some(at) = d.refs.iter().position(|r| r.names(gone)) {
                d.refs.remove(at);
                // The segments either side of it become one: a lock on either goes, those after move down.
                d.locked.retain(|l| l.segment + 1 != at && l.segment != at);
                d.locked.iter_mut().filter(|l| l.segment > at).for_each(|l| l.segment -= 1);
                if !d.kind.is_round() && d.refs.len() != 2 {
                    d.label = None;
                }
            }
        }
        self.dimensions.retain(|d| d.refs.len() >= d.kind.least());
        before - self.dimensions.len()
    }

    /// The sketch lines `before` (its forms as they were) had that its forms no longer have, taken from its dimensions
    /// ([`Family::forget_reference`]): a form drawn again keeps the dimensions of the lines that keep their GUIDs. How many
    /// dimensions went.
    pub fn forget_lines_gone(&mut self, before: &[Form]) -> usize {
        let now: BTreeSet<Guid> = self.forms.iter().flat_map(|f| f.all_lines().map(|l| l.guid)).collect();
        let gone: Vec<Guid> =
            before.iter().flat_map(|f| f.all_lines().map(|l| l.guid)).filter(|g| !g.is_nil() && !now.contains(g)).collect();
        gone.into_iter()
            .map(|g| {
                self.forget_pivots(g);
                self.forget_reference(g)
            })
            .sum()
    }

    /// Each dimension's reference given as an id (as a command may) given as the GUID of the plane, form, sketch line or
    /// reference line of that id; a part given another GUID (`moved`: from, to) followed.
    pub(crate) fn resolve_dimension_refs(&mut self, moved: &[(Guid, Guid)]) {
        let ids = |it: &mut dyn Iterator<Item = (Id, Guid)>| -> Vec<(Id, Guid)> { it.collect() };
        let plane_ids = ids(&mut self.planes.iter().map(|p| (p.id, p.guid)));
        let form_ids = ids(&mut self.forms.iter().map(|f| (f.id, f.guid)));
        let line_ids = ids(&mut self.lines.iter().map(|l| (l.id, l.guid)));
        let sketch_ids: Vec<(Guid, Vec<(Id, Guid)>)> =
            self.forms.iter().map(|f| (f.guid, f.plane_lines().map(|l| (l.id, l.guid)).collect())).collect();
        let pick = |among: &[(Id, Guid)], g: &mut Guid| {
            if let Some(id) = g.pending()
                && let Some((_, found)) = among.iter().find(|(i, found)| *i == id && !crate::ids::unset(i) && !found.is_nil())
            {
                *g = *found;
            }
        };
        let follow = |g: &mut Guid| {
            if let Some((_, to)) = moved.iter().find(|(from, _)| from == g) {
                *g = *to;
            }
        };
        // A pivot's point and reference (F7d), as a dimension's references.
        let pivots = self.lines.iter_mut().filter_map(|l| l.pivot.as_mut()).chain(self.planes.iter_mut().filter_map(|p| p.pivot.as_mut()));
        for piv in pivots {
            match &mut piv.to {
                PivotPoint::Crossing(a, b) => {
                    for g in [a, b] {
                        pick(&plane_ids, g);
                        follow(g);
                    }
                }
                PivotPoint::LineEnd(g, _) => {
                    pick(&line_ids, g);
                    follow(g);
                }
                PivotPoint::Sketch(g, l, _) => {
                    pick(&form_ids, g);
                    if let Some((_, lines)) = sketch_ids.iter().find(|(f, _)| f == g) {
                        pick(lines, l);
                    }
                    follow(g);
                    follow(l);
                }
                PivotPoint::Fixed(_) => {}
            }
            match &mut piv.from {
                PivotFrom::Plane(g) => {
                    pick(&plane_ids, g);
                    follow(g);
                }
                PivotFrom::Line(g) => {
                    pick(&line_ids, g);
                    follow(g);
                }
                PivotFrom::Drawn => {}
            }
        }
        for r in self.dimensions.iter_mut().flat_map(|d| d.refs.iter_mut()) {
            match r {
                DimRef::Plane(g) => pick(&plane_ids, g),
                DimRef::Form(g, _) => pick(&form_ids, g),
                DimRef::Line(g, _) | DimRef::LinePoint(g, _) => pick(&line_ids, g),
                DimRef::Sketch(g, l) | DimRef::Point(g, l, _) => {
                    pick(&form_ids, g);
                    if let Some((_, lines)) = sketch_ids.iter().find(|(f, _)| f == g) {
                        pick(lines, l);
                    }
                }
            }
            match r {
                DimRef::Plane(g) | DimRef::Form(g, _) | DimRef::Line(g, _) | DimRef::LinePoint(g, _) => follow(g),
                DimRef::Sketch(g, l) | DimRef::Point(g, l, _) => {
                    follow(g);
                    follow(l);
                }
            }
        }
    }
}

/// How much more readily an extrusion's end, a sketch's vertex or a circle's radius moves than a plane: the least change
/// weighs their moves this much less.
const END_GIVES: f64 = 1e6;

/// The least change `dx` from `x0` with every row held (`lin(x0 + dx) = 0`), each connected group of unknowns on its own, an
/// unknown's change weighed down by its `give` (the change of `give · y`, `y` the least). A row that adds nothing to those
/// before it in its group is refused, naming the dimensions it comes from; a soft one is left out (and listed with the
/// point). The rows `skip` (by place) are left out; `lenient` leaves out a hard row that adds nothing too, as a step of a
/// non-linear solve does away from the drawn point, where its residual decides.
fn solve_rows(
    x0: &[f64],
    give: &[f64],
    rows: &[Row],
    meter: &mut Meter,
    skip: &[usize],
    lenient: bool,
) -> Result<(Vec<f64>, Vec<usize>), Unsat> {
    let n = x0.len();
    // Groups: unknowns joined by a row.
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for r in rows {
        let mut us = r.lin.terms.iter().map(|t| t.0).filter(|u| *u < n);
        if let Some(first) = us.next() {
            for u in us {
                let (a, b) = (root(&mut parent, first), root(&mut parent, u));
                if a != b {
                    parent[a.max(b)] = a.min(b);
                }
            }
        }
    }
    let mut x = x0.to_vec();
    let mut groups: BTreeMap<usize, (Vec<usize>, Vec<usize>)> = BTreeMap::new();
    for u in 0..n {
        let r = root(&mut parent, u);
        groups.entry(r).or_default().0.push(u);
    }
    let mut dropped = Vec::new();
    for (k, row) in rows.iter().enumerate() {
        if skip.contains(&k) {
            continue;
        }
        match row.lin.terms.iter().map(|t| t.0).find(|u| *u < n) {
            Some(u) => {
                let r = root(&mut parent, u);
                groups.entry(r).or_default().1.push(k);
            }
            None if row.soft || lenient => dropped.push(k),
            // Nothing in it may move: it adds nothing.
            None => return Err(over(rows, k, &[])),
        }
    }
    for (_, (us, ks)) in groups {
        if ks.is_empty() {
            continue;
        }
        if us.len() > MAX_UNKNOWNS {
            return Err(
                format!("its constraints hold more than {MAX_UNKNOWNS} planes, ends and points together: hold fewer of them").into()
            );
        }
        let dim = us.len();
        let col = |u: usize| us.iter().position(|v| *v == u);
        // Modified Gram-Schmidt over the group's rows in order, each orthonormal row kept with what it is of the rows.
        let mut qs: Vec<Vec<f64>> = Vec::new();
        let mut ts: Vec<Vec<f64>> = Vec::new();
        for (j, &k) in ks.iter().enumerate() {
            let mut a = vec![0.0; dim];
            for (u, c) in &rows[k].lin.terms {
                if let Some(i) = col(*u) {
                    a[i] += c * give.get(*u).copied().unwrap_or(1.0);
                }
            }
            let size = dot_n(&a, &a).sqrt();
            let mut v = a;
            let mut t = vec![0.0; ks.len()];
            t[j] = 1.0;
            meter.spend(2 * (qs.len() as u64 + 1) * (dim + ks.len()) as u64).map_err(Unsat::from)?;
            for _ in 0..2 {
                for (q, tq) in qs.iter().zip(&ts) {
                    let p = dot_n(&v, q);
                    v.iter_mut().zip(q).for_each(|(a, b)| *a -= p * b);
                    t.iter_mut().zip(tq).for_each(|(a, b)| *a -= p * b);
                }
            }
            let left = dot_n(&v, &v).sqrt();
            if size == 0.0 || left <= DEPENDENT * size.max(1.0) {
                if rows[k].soft || lenient {
                    dropped.push(k);
                    continue;
                }
                let with: Vec<usize> = t.iter().enumerate().filter(|(i, w)| *i != j && w.abs() > 1e-9).map(|(i, _)| ks[i]).collect();
                return Err(over(rows, k, &with));
            }
            qs.push(v.into_iter().map(|a| a / left).collect());
            ts.push(t.into_iter().map(|a| a / left).collect());
        }
        // What is left to do in each row, at x0.
        let r: Vec<f64> = ks.iter().map(|&k| -rows[k].lin.at(x0)).collect();
        // dx = Σ β_k q_k with β = T r: the least change that does it.
        for (q, t) in qs.iter().zip(&ts) {
            let beta: f64 = t.iter().zip(&r).map(|(a, b)| a * b).sum();
            for (i, u) in us.iter().enumerate() {
                x[*u] += beta * q[i] * give.get(*u).copied().unwrap_or(1.0);
            }
        }
    }
    Ok((x, dropped))
}

fn dot_n(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Row `k` refused: it adds nothing to rows `with` (by place in `rows`).
fn over(rows: &[Row], k: usize, with: &[usize]) -> Unsat {
    let own = rows.get(k).and_then(|r| r.dim);
    let own_pivot = rows.get(k).and_then(|r| r.pivot.clone());
    let mut dims: Vec<usize> = with.iter().filter_map(|i| rows.get(*i).and_then(|r| r.dim)).filter(|d| Some(*d) != own).collect();
    dims.sort_unstable();
    dims.dedup();
    // The pivots among them (F7d), named as they are.
    let mut pivots: Vec<String> =
        with.iter().filter_map(|i| rows.get(*i).and_then(|r| r.pivot.clone())).filter(|p| Some(p) != own_pivot.as_ref()).collect();
    pivots.sort();
    pivots.dedup();
    let named = |ds: &[usize]| ds.iter().map(|d| (d + 1).to_string()).collect::<Vec<_>>().join(", ");
    let mut others: Vec<String> = Vec::new();
    if !dims.is_empty() {
        others.push(format!("dimension {}", named(&dims)));
    }
    others.extend(pivots);
    let others = others.join(" and ");
    let whose = own.map(|d| format!("dimension {}", d + 1)).or(own_pivot);
    let message = match (whose, others.is_empty()) {
        (Some(d), true) => format!("{d} is over-constrained: what it holds can't move"),
        (Some(d), false) => format!("{d} is over-constrained: {others} hold what it holds already"),
        (None, true) => "what is moved is fixed".to_string(),
        (None, false) => format!("{others} hold it where it is"),
    };
    let mut all = dims;
    if let Some(d) = own {
        all.insert(0, d);
    }
    Unsat { message, dims: all, over: true }
}

/// A dimension as a family's view draws it: its references' places on its line (the view's mm), the way along it and
/// across it there, each segment's length, its label and which segments are locked. A radial one's points are the
/// circle's centre and a point of it, a diameter one's two points of it across the centre. An angular one's are where its
/// lines cross, then a point on each line's ray as far from there as `at` is (its arc's radius), its lengths the angles in
/// degrees.
#[derive(Clone, Debug, PartialEq)]
pub struct DimMark {
    /// Its place in the family's dimensions.
    pub index: usize,
    pub points: Vec<P2>,
    pub along: P2,
    pub across: P2,
    pub lengths: Vec<f64>,
    pub label: Option<String>,
    pub locked: Vec<bool>,
    pub equal: bool,
    pub alignment: bool,
    pub kind: DimKind,
}

/// The dimensions of `flexed` (`drawn`, the family as drawn, flexed for `values`) drawn in view `which` of `st`. An angular
/// one measures the sector it was drawn in, whichever way its lines have turned since.
pub fn marks(drawn: &Family, flexed: &Family, values: &BTreeMap<String, Val>, st: &views::Staged, which: views::Which) -> Vec<DimMark> {
    let view = DimView::of(which);
    let mut out = Vec::new();
    let mut read = HashMap::new();
    let shown = flexed.dimensions.iter().filter(|d| d.view == view).flat_map(|d| d.refs.iter().copied());
    let Ok(problem) = Problem::new(flexed, shown, values, &mut read, &mut Meter::default()) else { return out };
    let mut drawn_read = HashMap::new();
    let angular = drawn.dimensions.iter().filter(|d| d.view == view && d.kind == DimKind::Angular).flat_map(|d| d.refs.iter().copied());
    let as_drawn = Problem::new(drawn, angular, values, &mut drawn_read, &mut Meter::default()).ok();
    for (index, d) in flexed.dimensions.iter().enumerate().filter(|(_, d)| d.view == view) {
        let (points, lengths) = if d.kind == DimKind::Angular {
            // The sector as drawn (a dimension being placed, which the drawn family hasn't, as it is now).
            let signs = match drawn.dimensions.get(index).filter(|x| x.guid == d.guid && x.refs == d.refs) {
                Some(x) => as_drawn.as_ref().and_then(|p| p.angle_signs(x).ok()),
                None => problem.angle_signs(d).ok(),
            };
            let Some(signs) = signs else { continue };
            let Ok(angles) = problem.angles(d, &signs) else { continue };
            let mut seen = Vec::new();
            for r in &d.refs {
                let Ok((p, w)) = problem.in_view(r, view) else { break };
                seen.push((P2(p.0.v, p.1.v), P2(w.0.v, w.1.v)));
            }
            let Ok(now) = ray_signs(&seen, P2(0.0, 0.0)) else { continue };
            let (a, b) = view_axes(view);
            let eye = view.eye();
            let depth = mul(eye, dot(d.at, eye));
            let to3 = |p: P2| add(add(mul(a, p.0), mul(b, p.1)), depth);
            let radius = (P2(dot(d.at, a), dot(d.at, b)) - now.vertex).length();
            if !(radius > 1e-6 && radius.is_finite()) {
                continue;
            }
            let mut points = vec![st.to_view(which, to3(now.vertex))];
            for ((_, w), s) in seen.iter().zip(&signs.rays) {
                points.push(st.to_view(which, to3(now.vertex + (*w * *s).unit() * radius)));
            }
            (points, angles.iter().map(|x| degrees(x.v)).collect::<Vec<f64>>())
        } else if d.kind.is_aligned() {
            let Ok((m, p)) = problem.stations(d, values, &mut read) else { continue };
            let base = dot(d.at, m);
            let points: Vec<P2> = p.iter().map(|s| st.to_view(which, add(d.at, mul(m, s - base)))).collect();
            (points, p.windows(2).map(|w| (w[1] - w[0]).abs()).collect::<Vec<f64>>())
        } else {
            let Ok((c, r, frame)) = problem.circle(d) else { continue };
            // Toward where it is placed, in the circle's plane.
            let (centre, _) = frame.local(c);
            let (toward, _) = frame.local(d.at);
            let way = (toward - centre).unit();
            let way = if way.length() > 0.5 { way } else { P2(1.0, 0.0) };
            let rim = |k: f64| st.to_view(which, frame.point(centre + way * (r * k), 0.0));
            let points = if d.kind == DimKind::Radial { vec![st.to_view(which, c), rim(1.0)] } else { vec![rim(-1.0), rim(1.0)] };
            (points, vec![r * d.kind.times()])
        };
        let (Some(&a), Some(&b)) = (points.first(), points.last()) else { continue };
        let along = if d.kind.is_aligned() {
            let m = problem.way(d).unwrap_or([1.0, 0.0, 0.0]);
            st.to_view(which, add(d.at, m)) - st.to_view(which, d.at)
        } else if d.kind == DimKind::Angular {
            points.get(1).map_or(b - a, |p| *p - a)
        } else {
            b - a
        };
        let len = along.dot(along).sqrt();
        if len < 1e-9 || !len.is_finite() {
            continue;
        }
        let along = along * (1.0 / len);
        let across = P2(-along.1, along.0);
        let locked = (0..lengths.len()).map(|i| d.is_locked(i)).collect();
        out.push(DimMark {
            index,
            points,
            along,
            across,
            lengths,
            label: d.label.clone(),
            locked,
            equal: d.equal,
            alignment: d.alignment,
            kind: d.kind,
        });
    }
    out
}

/// The extrusion ends of `flexed` view `which` sees edge on, each as the segment it is drawn as (the view's mm): what a
/// dimension or Align may pick besides the planes.
pub fn end_faces(flexed: &Family, values: &BTreeMap<String, Val>, st: &views::Staged, which: views::Which) -> Vec<(DimRef, P2, P2)> {
    let eye = DimView::of(which).eye();
    let mut out = Vec::new();
    let mut read = HashMap::new();
    for (i, form) in flexed.forms.iter().enumerate() {
        if !form.mesh.is_empty() || form.axis().is_some() || form.kind().swept() || form.guid.is_nil() {
            continue;
        }
        let Ok(frame) = flexed.frame(&form.host) else { continue };
        let z = frame.z;
        if dot(z, eye).abs() > PARALLEL {
            continue;
        }
        let Some(dir) = unit3(cross(z, eye)) else { continue };
        let ring = views::outline(flexed, values, st, which, i);
        if ring.is_empty() {
            continue;
        }
        for end in [Extent::Start, Extent::End] {
            let Ok(t) = number(end_expr(form, end), values, &mut read) else { continue };
            let base = add(frame.origin, mul(z, t));
            let (o, step) = (st.to_view(which, base), st.to_view(which, add(base, dir)));
            let dv = step - o;
            let l2 = dv.dot(dv);
            if l2 < 1e-12 {
                continue;
            }
            let ts: Vec<f64> = ring.iter().map(|q| (*q - o).dot(dv) / l2).collect();
            let (lo, hi) = ts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), t| (a.min(*t), b.max(*t)));
            if lo.is_finite() && hi.is_finite() {
                out.push((DimRef::Form(form.guid, end), o + dv * lo, o + dv * hi));
            }
        }
    }
    out
}

/// The straight lines of `flexed`'s sketches view `which` sees edge on (F7b), each as the segment it is drawn as (the view's
/// mm): a line of a sketch on a plane the view sees flat, where it is drawn; and an extrusion's side where its line runs the
/// way the view looks, from its start to its end (Revit's face of the extrusion). What a dimension or Align picks of a form.
pub fn sketch_faces(flexed: &Family, values: &BTreeMap<String, Val>, st: &views::Staged, which: views::Which) -> Vec<(DimRef, P2, P2)> {
    let eye = DimView::of(which).eye();
    let mut out = Vec::new();
    let mut read = HashMap::new();
    for form in &flexed.forms {
        if !form.mesh.is_empty() || form.guid.is_nil() {
            continue;
        }
        let Ok(frame) = flexed.frame(&form.host) else { continue };
        let flat = parallel(frame.z, eye);
        // A side face seen edge on is an extrusion's alone: a blend's sides lean, a sweep's follow its path.
        let edge = dot(frame.z, eye).abs() <= PARALLEL && form.kind() == super::forms::FormKind::Extrusion;
        if !flat && !edge {
            continue;
        }
        let ends = if edge {
            match (number(&form.from, values, &mut read), number(&form.to, values, &mut read)) {
                (Ok(a), Ok(b)) => (a, b),
                _ => continue,
            }
        } else {
            (0.0, 0.0)
        };
        for l in form.plane_lines().filter(|l| !l.guid.is_nil()) {
            let FormCurve::Line { start, end, turn } = &l.curve else { continue };
            let mut num = |e| number(e, values, &mut read);
            let (Ok(ax), Ok(ay), Ok(bx), Ok(by)) = (num(&start[0]), num(&start[1]), num(&end[0]), num(&end[1])) else { continue };
            if turn.as_ref().is_some_and(|t| num(t) != Ok(0.0)) {
                continue;
            }
            let (a, b) = (P2(ax, ay), P2(bx, by));
            if a.dist(b) < 1e-6 {
                continue;
            }
            let r = DimRef::Sketch(form.guid, l.guid);
            if flat {
                out.push((r, st.to_view(which, frame.point(a, 0.0)), st.to_view(which, frame.point(b, 0.0))));
            } else if parallel(frame.dir((b - a).unit()), eye) {
                out.push((r, st.to_view(which, frame.point(a, ends.0)), st.to_view(which, frame.point(a, ends.1))));
            }
        }
    }
    out
}

/// A circle or an arc of a sketch as a view sees it (the view's mm): its reference (its line), its centre and radius, and
/// an arc's start, middle and end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Round {
    pub r: DimRef,
    pub centre: P2,
    pub radius: f64,
    pub arc: Option<[P2; 3]>,
}

impl Round {
    /// Whether `p` (the view's mm) lies within its sweep, seen from its centre: anywhere for a circle.
    pub fn sweeps(&self, p: P2) -> bool {
        let Some([a, m, b]) = self.arc else { return true };
        let angle = |q: P2| (q.1 - self.centre.1).atan2(q.0 - self.centre.0);
        let tau = std::f64::consts::TAU;
        let from = |q: P2| (angle(q) - angle(a)).rem_euclid(tau);
        // Counter-clockwise from its start (in the view) when its middle comes before its end that way, else clockwise.
        let (to_m, to_b, to_p) = (from(m), from(b), from(p));
        if to_m <= to_b { to_p <= to_b } else { to_p >= to_b }
    }
}

/// The circles and arcs of `flexed`'s sketches on planes view `which` sees flat (F7b, arcs F7c): what a radial dimension
/// picks, and whose centre a dimension or Align picks.
pub fn sketch_circles(flexed: &Family, values: &BTreeMap<String, Val>, st: &views::Staged, which: views::Which) -> Vec<Round> {
    let eye = DimView::of(which).eye();
    let mut out = Vec::new();
    let mut read = HashMap::new();
    for form in &flexed.forms {
        if !form.mesh.is_empty() || form.guid.is_nil() {
            continue;
        }
        let Ok(frame) = flexed.frame(&form.host) else { continue };
        if !parallel(frame.z, eye) {
            continue;
        }
        let at = |p: P2| st.to_view(which, frame.point(p, 0.0));
        for l in form.plane_lines().filter(|l| !l.guid.is_nil()) {
            let r = DimRef::Sketch(form.guid, l.guid);
            let mut num = |e| number(e, values, &mut read);
            match &l.curve {
                FormCurve::Circle { centre, radius } => {
                    let (Ok(x), Ok(y), Ok(radius)) = (num(&centre[0]), num(&centre[1]), num(radius)) else { continue };
                    out.push(Round { r, centre: at(P2(x, y)), radius, arc: None });
                }
                FormCurve::Line { start, end, turn: Some(t) } => {
                    let (Ok(ax), Ok(ay), Ok(bx), Ok(by), Ok(t)) = (num(&start[0]), num(&start[1]), num(&end[0]), num(&end[1]), num(t))
                    else {
                        continue;
                    };
                    let (a, b, t) = (P2(ax, ay), P2(bx, by), t.to_radians());
                    if t == 0.0 || a.dist(b) < 1e-9 || !t.is_finite() {
                        continue;
                    }
                    let c = arc_centre(a, b, t);
                    let from = (a.1 - c.1).atan2(a.0 - c.0) + t / 2.0;
                    let radius = arc_radius(a, b, t);
                    let m = c + P2(from.cos(), from.sin()) * radius;
                    out.push(Round { r, centre: at(c), radius, arc: Some([at(a), at(m), at(b)]) });
                }
                FormCurve::Line { .. } => {}
            }
        }
    }
    out
}

/// The reference lines of `flexed` view `which` sees a face of edge on, each by that face and the segment the line is drawn
/// as (the view's mm): what a dimension or Align picks of a reference line (its vertical face in a plan of a line drawn on
/// a level, its horizontal one in an elevation that sees it along).
pub fn line_faces(flexed: &Family, st: &views::Staged, which: views::Which) -> Vec<(DimRef, P2, P2)> {
    let eye = DimView::of(which).eye();
    let mut out = Vec::new();
    for l in flexed.lines.iter().filter(|l| !l.guid.is_nil()) {
        let (a, b) = (st.to_view(which, l.start), st.to_view(which, l.end));
        if a.dist(b) < 1e-6 {
            continue;
        }
        for face in [Face::Vertical, Face::Horizontal] {
            if let Ok(fr) = flexed.frame(&PlaneRef::Line(l.guid, face))
                && dot(fr.z, eye).abs() <= PARALLEL
            {
                out.push((DimRef::Line(l.guid, face), a, b));
                break;
            }
        }
    }
    out
}

/// The way reference `r` of `flexed` faces, for `values`; none for a point, which has none of its own.
pub fn direction(flexed: &Family, r: &DimRef, values: &BTreeMap<String, Val>) -> Result<Option<P3>, String> {
    let mut read = HashMap::new();
    Problem::new(flexed, [*r], values, &mut read, &mut Meter::default())?.direction(r)
}

/// Where reference `r` of `flexed` is along `m`, for `values`.
pub fn place(flexed: &Family, r: &DimRef, values: &BTreeMap<String, Val>, m: P3) -> Result<f64, String> {
    let mut read = HashMap::new();
    let problem = Problem::new(flexed, [*r], values, &mut read, &mut Meter::default())?;
    Ok(problem.position(r, m, values, &mut read)?.c)
}

/// Where reference `r` of `flexed` is along its own normal, for `values`: what Align and a new dimension work from. A point
/// has no normal of its own.
pub fn place_along(flexed: &Family, r: &DimRef, values: &BTreeMap<String, Val>) -> Result<(P3, f64), String> {
    let n = direction(flexed, r, values)?.ok_or("a point faces no way of its own: measure it along a plane or a line")?;
    Ok((n, place(flexed, r, values, n)?))
}

/// The way reference `r` of `flexed` runs in view `view` (in the view's own axes), for `values`: a line an angular dimension
/// may measure to, seen as a line there (F7c); else why not.
pub fn view_way(flexed: &Family, r: &DimRef, values: &BTreeMap<String, Val>, view: DimView) -> Result<P2, String> {
    let mut read = HashMap::new();
    let problem = Problem::new(flexed, [*r], values, &mut read, &mut Meter::default())?;
    let (_, w) = problem.in_view(r, view)?;
    Ok(P2(w.0.v, w.1.v))
}

/// Align (AL): `target` moved onto `reference` (parallel to it), as Revit's Align moves it, for `values`: the edits that
/// do it. A plane is dragged ([`Family::drag_plane`]); an extrusion end the constraints don't set takes the new value; a
/// sketch's line or circle centre moves there and the lines joined to it follow, through the constraints (refused where they
/// hold it); a reference line moves, and what is drawn on it with it.
pub fn align(f: &Family, values: &BTreeMap<String, Val>, reference: &DimRef, target: &DimRef) -> Result<Vec<Edit>, Unsat> {
    let flexed = f.flexed(values)?;
    let (a, b) = (direction(&flexed, reference, values).map_err(Unsat::from)?, direction(&flexed, target, values).map_err(Unsat::from)?);
    let m = match (a, b) {
        (Some(n), Some(m)) if !parallel(n, m) => return Err("Align takes a reference parallel to the first".to_string().into()),
        (Some(n), _) | (None, Some(n)) => n,
        (None, None) => return Err("Align takes a plane or a line: two points face no way to align along".to_string().into()),
    };
    let at = place(&flexed, reference, values, m).map_err(Unsat::from)?;
    let was = place(&flexed, target, values, m).map_err(Unsat::from)?;
    if (at - was).abs() <= 1e-9 {
        return Ok(Vec::new());
    }
    // Along the target's own normal, where it has one.
    let own = |n: Option<P3>| if n.is_some_and(|n| dot(n, m) < 0.0) { -1.0 } else { 1.0 };
    match *target {
        DimRef::Plane(g) => {
            let (_, drawn) = place_along(&flexed, target, values).map_err(Unsat::from)?;
            f.drag_plane(values, g, drawn + (at - was) * own(b))
        }
        DimRef::Form(g, end) => {
            let (i, form) = end_form(f, g).map_err(Unsat::from)?;
            let reporting = |l: &str| f.reporting(l);
            if let Some(k) = f.dimensions.iter().position(|d| d.constrains(&reporting) && d.refs.contains(&DimRef::Form(g, end))) {
                return Err(Unsat {
                    message: format!("{NOT_SATISFIED}: dimension {} holds the form's end", k + 1),
                    dims: vec![k],
                    over: true,
                });
            }
            let value = number(end_expr(form, end), values, &mut HashMap::new()).map_err(Unsat::from)?;
            Ok(vec![Edit::End(i, end, value + (at - was) * own(b))])
        }
        DimRef::Sketch(..) | DimRef::Point(..) => {
            match flexed.solve_with(values, &[Pin::Ref(*target, m, at)], &[], &mut Meter::default()) {
                Ok(s) => Ok(flexed.edits(&s, values)),
                Err(e) if e.over => Err(Unsat { message: format!("{NOT_SATISFIED}: {}", e.message), ..e }),
                Err(e) => Err(e),
            }
        }
        DimRef::Line(g, _) | DimRef::LinePoint(g, _) => Ok(vec![Edit::Line(g, mul(m, at - was))]),
    }
}
