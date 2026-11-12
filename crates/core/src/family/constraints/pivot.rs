//! F7d: pivots. A reference line or a reference plane may be pinned to a point of the family and turn about it, its angle
//! its one free rotation: a door's swing, a window's sash, anything that turns. The pivot is the element's own (a field of
//! the line or the plane, no dimension), its swing an Angle parameter on the pivot or a number of its own, measured from the
//! element as drawn, a reference plane or a reference line, between optional limits.
//!
//! In the solve a pivoted line is F7c's four unknowns and the pivot two rows (its point on the pivot point) and a swing row;
//! a pivoted plane is one unknown, its turn about its axis, and its axes are the drawn ones moved rigidly onto the pivot
//! point, so what is drawn on it turns with it. The pivot point is read where the solve has got to and never moved by the
//! pivot. The swing row is hard for a parameter and soft for the pivot's own angle, which a dimension may then set.

use super::*;
use crate::view3d::sub;

/// How far a swing may be, either way, in degrees.
pub const MAX_SWING: f64 = 360.0;
/// How near a snap (0, a limit, each 45°) the rotate grip lands on it, in degrees.
const SNAP_REACH: f64 = 5.0;

/// The point a pivot pins its element to: where two reference planes cross (each holding the pivot's axis, so they cross
/// in a line along it), a reference line's end, a point of a form's sketch (a line's end, a circle's or an arc's centre),
/// or a fixed point (the family's axes, mm). In JSON `{"planes": [g, g]}`, `{"line": g, "point": "start" | "end"}`,
/// `{"form": g, "line": g, "point": …}` or `{"fixed": [x, y, z]}`; an id reads as the one it names.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawPoint", into = "RawPoint")]
pub enum PivotPoint {
    Crossing(Guid, Guid),
    LineEnd(Guid, Spot),
    Sketch(Guid, Guid, Spot),
    Fixed(P3),
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPoint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    planes: Option<[Guid; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    form: Option<Guid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    line: Option<Guid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    point: Option<Spot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fixed: Option<P3>,
}

impl TryFrom<RawPoint> for PivotPoint {
    type Error = String;
    fn try_from(r: RawPoint) -> Result<PivotPoint, String> {
        match (r.planes, r.form, r.line, r.point, r.fixed) {
            (Some([a, b]), None, None, None, None) => Ok(PivotPoint::Crossing(a, b)),
            (None, None, Some(l), Some(s), None) => Ok(PivotPoint::LineEnd(l, s)),
            (None, Some(f), Some(l), Some(s), None) => Ok(PivotPoint::Sketch(f, l, s)),
            (None, None, None, None, Some(p)) => Ok(PivotPoint::Fixed(p)),
            _ => Err("a pivot's `to` is {\"planes\": [guid, guid]}, {\"line\": guid, \"point\": \"start\" | \"end\"}, {\"form\": guid, \
                      \"line\": guid, \"point\": \"start\" | \"end\" | \"centre\"} or {\"fixed\": [x, y, z]}"
                .into()),
        }
    }
}

impl From<PivotPoint> for RawPoint {
    fn from(p: PivotPoint) -> RawPoint {
        let none = RawPoint::default();
        match p {
            PivotPoint::Crossing(a, b) => RawPoint { planes: Some([a, b]), ..none },
            PivotPoint::LineEnd(l, s) => RawPoint { line: Some(l), point: Some(s), ..none },
            PivotPoint::Sketch(f, l, s) => RawPoint { form: Some(f), line: Some(l), point: Some(s), ..none },
            PivotPoint::Fixed(p) => RawPoint { fixed: Some(p), ..none },
        }
    }
}

impl PivotPoint {
    /// Whether it names `g`: a plane, a line, a form or a sketch line.
    pub fn names(&self, g: Guid) -> bool {
        match *self {
            PivotPoint::Crossing(a, b) => a == g || b == g,
            PivotPoint::LineEnd(l, _) => l == g,
            PivotPoint::Sketch(f, l, _) => f == g || l == g,
            PivotPoint::Fixed(_) => false,
        }
    }
}

/// Which point of its element a pivot holds: its start, its end, or a point `along` it (mm from its start); a plane's,
/// along the line it is drawn as. In JSON `"start"`, `"end"` or `{"along": mm}`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PivotAt {
    #[default]
    Start,
    End,
    Along(f64),
}

impl PivotAt {
    fn is_start(&self) -> bool {
        *self == PivotAt::Start
    }

    /// How far from the start of a line `len` long, as drawn.
    pub fn distance(&self, len: f64) -> f64 {
        match *self {
            PivotAt::Start => 0.0,
            PivotAt::End => len,
            PivotAt::Along(d) => d,
        }
    }
}

/// What a pivot's swing is measured from: its element as drawn, a reference plane's trace, or a reference line. In JSON
/// `"drawn"`, `{"plane": guid}` or `{"line": guid}`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PivotFrom {
    #[default]
    Drawn,
    Plane(Guid),
    Line(Guid),
}

impl PivotFrom {
    fn is_drawn(&self) -> bool {
        *self == PivotFrom::Drawn
    }
}

/// A reference line's or a reference plane's pivot (F7d): it keeps point `at` of its element on point `to`, and its swing,
/// counter-clockwise about its axis from `from`, is `parameter` (an Angle parameter, degrees), or `angle` where none labels
/// it; `min` and `max` bound it. The axis is the normal of what a line is drawn on, a plane's `up`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pivot {
    pub to: PivotPoint,
    #[serde(default, skip_serializing_if = "PivotAt::is_start")]
    pub at: PivotAt,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter: Option<String>,
    #[serde(default, skip_serializing_if = "zero")]
    pub angle: f64,
    #[serde(default, skip_serializing_if = "PivotFrom::is_drawn")]
    pub from: PivotFrom,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// How far (radians) a flexed copy has turned its element from as drawn, so a copy solved again reads "as drawn" as the
    /// family drew it. Never in a file.
    #[serde(skip)]
    pub turned: f64,
}

fn zero(v: &f64) -> bool {
    *v == 0.0
}

impl Pivot {
    /// A pivot to `to` at `at`, its swing 0 from as drawn, unlimited.
    pub fn new(to: PivotPoint, at: PivotAt) -> Pivot {
        Pivot { to, at, parameter: None, angle: 0.0, from: PivotFrom::Drawn, min: None, max: None, turned: 0.0 }
    }

    /// The range its swing may take, degrees.
    pub fn range(&self) -> (f64, f64) {
        (self.min.unwrap_or(-MAX_SWING), self.max.unwrap_or(MAX_SWING))
    }

    /// Whether `v` (degrees) is a swing it may take; else what to say, naming `what` and `who`.
    fn allows(&self, v: f64, what: &str, who: &str) -> Result<(), String> {
        let (lo, hi) = self.range();
        if v.is_finite() && v >= lo - 1e-9 && v <= hi + 1e-9 {
            return Ok(());
        }
        Err(if self.min.is_some() || self.max.is_some() {
            format!("{what} is {}°; {who}'s pivot turns from {}° to {}°", shown(v), shown(lo), shown(hi))
        } else {
            format!("{what} is {}°; a pivot turns within ±{MAX_SWING}°", shown(v))
        })
    }

    /// `v` (degrees) brought within its range.
    pub fn clamp(&self, v: f64) -> f64 {
        let (lo, hi) = self.range();
        v.clamp(lo, hi)
    }

    /// `v` (degrees) snapped as the rotate grip snaps it: to whole degrees, and within [`SNAP_REACH`] of 0, a limit or a
    /// multiple of 45° to it; then within its range.
    pub fn snap(&self, v: f64) -> f64 {
        let mut snaps: Vec<f64> = (-8..=8).map(|k| f64::from(k) * 45.0).collect();
        snaps.extend(self.min);
        snaps.extend(self.max);
        let near = snaps.iter().copied().filter(|s| (s - v).abs() <= SNAP_REACH).min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs()));
        self.clamp(near.unwrap_or_else(|| v.round()))
    }
}

/// A number as a message shows it: whole where it is, else to a thousandth.
fn shown(v: f64) -> String {
    let r = (v * 1000.0).round() / 1000.0;
    if r == r.trunc() && r.abs() < 1e15 { format!("{}", r as i64) } else { format!("{r}") }
}

/// What carries a pivot: a reference line or a reference plane, by its place in its list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pivoted {
    Line(usize),
    Plane(usize),
}

/// A pivot's turn about its axis: its axes (`origin` the point of its element it holds as drawn, `x` and `y` in the plane
/// of rotation, `z` the axis), its element's way as drawn, and the arm's (the way from the pivot to its far end) and length.
#[derive(Clone, Copy, Debug)]
pub struct Rotation {
    pub frame: Frame,
    pub way: P3,
    pub arm: P3,
    pub reach: f64,
}

fn unit(a: P3) -> Option<P3> {
    let l = dot(a, a).sqrt();
    (l > 1e-12 && l.is_finite()).then(|| a.map(|x| x / l))
}

impl Family {
    /// Its pivots: its lines', then its planes'.
    pub fn pivots(&self) -> Vec<(Pivoted, &Pivot)> {
        let lines = self.lines.iter().enumerate().filter_map(|(i, l)| Some((Pivoted::Line(i), l.pivot.as_ref()?)));
        let planes = self.planes.iter().enumerate().filter_map(|(i, p)| Some((Pivoted::Plane(i), p.pivot.as_ref()?)));
        lines.chain(planes).collect()
    }

    /// The pivot of the line or plane `g`, and which it is.
    pub fn pivot_of(&self, g: Guid) -> Option<(Pivoted, &Pivot)> {
        self.pivots().into_iter().find(|(e, _)| self.pivoted_guid(*e) == Some(g))
    }

    /// The GUID of what carries a pivot.
    pub fn pivoted_guid(&self, e: Pivoted) -> Option<Guid> {
        match e {
            Pivoted::Line(i) => self.lines.get(i).map(|l| l.guid),
            Pivoted::Plane(i) => self.planes.get(i).map(|p| p.guid),
        }
    }

    /// What a message calls it: `reference line 2`, ``reference plane `Hinge` ``, `reference plane 4`.
    pub fn pivoted_said(&self, e: Pivoted) -> String {
        match e {
            Pivoted::Line(i) => format!("reference line {}", i + 1),
            Pivoted::Plane(i) => match self.planes.get(i) {
                Some(p) if !p.name.is_empty() => format!("reference plane `{}`", p.name),
                _ => format!("reference plane {}", i + 1),
            },
        }
    }

    /// The pivot carried by `e`.
    fn pivot_at(&self, e: Pivoted) -> Option<&Pivot> {
        match e {
            Pivoted::Line(i) => self.lines.get(i)?.pivot.as_ref(),
            Pivoted::Plane(i) => self.planes.get(i)?.pivot.as_ref(),
        }
    }

    /// How pivot `e` turns, as it stands: its axes, its element's way and its arm.
    pub fn rotation(&self, e: Pivoted) -> Result<Rotation, String> {
        let piv = self.pivot_at(e).ok_or("it has no pivot")?;
        let (start, end, x, y, z) = match e {
            Pivoted::Line(i) => {
                let l = self.lines.get(i).ok_or("no such line")?;
                let h = self.frame(&l.host)?;
                (l.start, l.end, h.x, h.y, h.z)
            }
            Pivoted::Plane(i) => {
                let p = self.planes.get(i).ok_or("no such plane")?;
                let f = p.frame();
                (p.start, p.end, f.x, cross(f.y, f.x), f.y)
            }
        };
        let way = unit(sub(end, start)).ok_or("it has no length")?;
        let len = dot(sub(end, start), sub(end, start)).sqrt();
        let s = piv.at.distance(len);
        let origin = add(start, mul(way, s));
        // The arm runs to the farther end.
        let (arm, reach) = if len - s >= s { (way, len - s) } else { (mul(way, -1.0), s) };
        Ok(Rotation { frame: Frame { origin, x, y, z }, way, arm, reach })
    }

    /// Whether its pivots are sound, whatever the values: on a plane of its own that doesn't define the origin, pinned to
    /// what it has (never the element itself) with planes that hold the axis and cross, its parameter an Angle one that isn't
    /// reporting, its numbers and limits in range, and no pivot reading what reads it back.
    pub fn check_pivots(&self) -> Result<(), String> {
        let pivots = self.pivots();
        for &(e, piv) in &pivots {
            let who = self.pivoted_said(e);
            let own = self.pivoted_guid(e).unwrap_or_default();
            if let Pivoted::Plane(i) = e
                && let Some(p) = self.planes.get(i)
            {
                if p.datum() {
                    return Err(format!("{who}: a datum stays: it can't pivot"));
                }
                if p.defines_origin {
                    return Err(format!("{who}: it defines the origin, which stays: it can't pivot"));
                }
            }
            let r = self.rotation(e).map_err(|x| format!("{who}: {x}"))?;
            let axis = r.frame.z;
            if piv.to.names(own) {
                return Err(format!("{who}: its pivot pins it to itself; pin it to another plane, line or point"));
            }
            match piv.to {
                PivotPoint::Crossing(a, b) => {
                    if a == b {
                        return Err(format!("{who}: its pivot is where two planes cross: name two planes"));
                    }
                    let mut normals = Vec::new();
                    for g in [a, b] {
                        let f = self.frame(&PlaneRef::Plane(g)).map_err(|x| format!("{who}: its pivot: {x}"))?;
                        if dot(f.z, axis).abs() > 1e-6 {
                            return Err(format!(
                                "{who}: its pivot: plane {g} doesn't hold its axis, so it doesn't cross the other along it; pick planes the axis lies in"
                            ));
                        }
                        normals.push(f.z);
                    }
                    if parallel(normals[0], normals[1]) {
                        return Err(format!("{who}: its pivot: the two planes are parallel and never cross"));
                    }
                }
                PivotPoint::LineEnd(g, s) => {
                    if !self.lines.iter().any(|l| l.guid == g) {
                        return Err(format!("{who}: its pivot: line {g} isn't one of its lines"));
                    }
                    if s == Spot::Centre {
                        return Err(format!("{who}: its pivot: a reference line has a start and an end, no centre"));
                    }
                }
                PivotPoint::Sketch(f, l, s) => {
                    self.reference_ok(&DimRef::Point(f, l, s), DimKind::Aligned).map_err(|x| format!("{who}: its pivot: {x}"))?;
                }
                PivotPoint::Fixed(p) => {
                    if !p.iter().all(|x| x.is_finite() && x.abs() <= MAX_COORD) {
                        return Err(format!("{who}: its pivot's fixed point is a point within {MAX_COORD} mm"));
                    }
                }
            }
            if let PivotAt::Along(d) = piv.at
                && !(d.is_finite() && d.abs() <= MAX_COORD)
            {
                return Err(format!("{who}: its pivot is a point along it within {MAX_COORD} mm, not {d}"));
            }
            for (what, v) in [("minimum", piv.min), ("maximum", piv.max)] {
                if let Some(v) = v
                    && !(v.is_finite() && v.abs() <= MAX_SWING)
                {
                    return Err(format!("{who}: its pivot's {what} is an angle within ±{MAX_SWING}°, not {v}"));
                }
            }
            if let (Some(lo), Some(hi)) = (piv.min, piv.max)
                && lo > hi
            {
                return Err(format!("{who}: its pivot's minimum, {}°, is past its maximum, {}°", shown(lo), shown(hi)));
            }
            match &piv.parameter {
                Some(name) => {
                    let p = self
                        .parameters
                        .iter()
                        .find(|p| &p.name == name)
                        .ok_or_else(|| format!("{who}: its pivot's angle: {}", not_a_parameter(name, &self.parameters)))?;
                    if p.kind != Kind::Angle {
                        return Err(format!("{who}: its pivot's angle `{name}` is {}; a swing is an angle", p.kind.said()));
                    }
                    if p.reporting {
                        return Err(format!("{who}: its pivot's angle `{name}` is a reporting parameter; a pivot's angle drives it"));
                    }
                }
                None => piv.allows(piv.angle, "its angle", &who).map_err(|x| format!("{who}: {x}"))?,
            }
            match piv.from {
                PivotFrom::Drawn => {}
                PivotFrom::Plane(g) | PivotFrom::Line(g) if g == own => {
                    return Err(format!("{who}: its pivot's swing is measured from itself; measure it from another plane or line"));
                }
                PivotFrom::Plane(g) => {
                    let f = self.frame(&PlaneRef::Plane(g)).map_err(|x| format!("{who}: its pivot's reference: {x}"))?;
                    let across = |v: P3| dot(v, r.frame.x).hypot(dot(v, r.frame.y));
                    if across(f.x).max(across(f.y)) < 1e-6 {
                        return Err(format!("{who}: its pivot's reference plane lies across its axis: it has no way to measure from"));
                    }
                }
                PivotFrom::Line(g) => {
                    let l = self
                        .lines
                        .iter()
                        .find(|l| l.guid == g)
                        .ok_or_else(|| format!("{who}: its pivot's reference: line {g} isn't one of its lines"))?;
                    let d = sub(l.end, l.start);
                    if dot(d, r.frame.x).hypot(dot(d, r.frame.y)) < 1e-6 {
                        return Err(format!("{who}: its pivot's reference line runs along its axis: it has no way to measure from"));
                    }
                }
            }
        }
        // No pivot reads, through its point, its reference or what its line is drawn on, what reads it back.
        let reads = |e: Pivoted| -> Vec<Guid> {
            let Some(piv) = self.pivot_at(e) else { return Vec::new() };
            let mut out = match piv.to {
                PivotPoint::Crossing(a, b) => vec![a, b],
                PivotPoint::LineEnd(l, _) => vec![l],
                PivotPoint::Sketch(f, _, _) => self.forms.iter().filter(|x| x.guid == f).map(|x| x.host.guid()).collect(),
                PivotPoint::Fixed(_) => Vec::new(),
            };
            match piv.from {
                PivotFrom::Plane(g) | PivotFrom::Line(g) => out.push(g),
                PivotFrom::Drawn => {}
            }
            if let Pivoted::Line(i) = e
                && let Some(l) = self.lines.get(i)
            {
                out.push(l.host.guid());
            }
            out
        };
        // What each element reads: a pivot's, and a line's host.
        let mut edges: BTreeMap<Guid, Vec<Guid>> = BTreeMap::new();
        for &(e, _) in &pivots {
            if let Some(g) = self.pivoted_guid(e) {
                edges.entry(g).or_default().extend(reads(e));
            }
        }
        for l in &self.lines {
            edges.entry(l.guid).or_default().push(l.host.guid());
        }
        for &(e, _) in &pivots {
            let Some(start) = self.pivoted_guid(e) else { continue };
            let mut todo: Vec<Guid> = edges.get(&start).cloned().unwrap_or_default();
            let mut seen = BTreeSet::new();
            while let Some(g) = todo.pop() {
                if g == start {
                    return Err(format!(
                        "{}: its pivot reads what turns with it (a cycle of pivots); pin it to something that doesn't",
                        self.pivoted_said(e)
                    ));
                }
                if seen.insert(g) && seen.len() <= MAX_PLANES + MAX_LINES {
                    todo.extend(edges.get(&g).cloned().unwrap_or_default());
                }
            }
        }
        Ok(())
    }

    /// Whether `values` (a type's) give each pivot's parameter a swing it may take.
    pub(crate) fn check_pivot_values(&self, values: &BTreeMap<String, Val>) -> Result<(), String> {
        for (e, piv) in self.pivots() {
            if let Some(name) = &piv.parameter
                && let Some(Val::Number(v)) = values.get(name)
            {
                piv.allows(*v, &format!("`{name}`"), &self.pivoted_said(e))?;
            }
        }
        Ok(())
    }

    /// Whether `v` is a value parameter `name` may take, as the pivots it drives allow (a component's instance value).
    pub fn pivot_value_ok(&self, name: &str, v: f64) -> Result<(), String> {
        for (e, piv) in self.pivots() {
            if piv.parameter.as_deref() == Some(name) {
                piv.allows(v, &format!("`{name}`"), &self.pivoted_said(e))?;
            }
        }
        Ok(())
    }

    /// The pivots that name `gone` (a plane, a line, a form or a sketch line deleted) taken off, as a dimension loses a
    /// reference. How many went.
    pub fn forget_pivots(&mut self, gone: Guid) -> usize {
        let names = |p: &Pivot| p.to.names(gone) || matches!(p.from, PivotFrom::Plane(g) | PivotFrom::Line(g) if g == gone);
        let mut n = 0;
        for slot in self.lines.iter_mut().map(|l| &mut l.pivot).chain(self.planes.iter_mut().map(|p| &mut p.pivot)) {
            if slot.as_ref().is_some_and(names) {
                *slot = None;
                n += 1;
            }
        }
        n
    }

    /// Its pivots that name what it hasn't: each element's GUID.
    pub fn dangling_pivots(&self) -> Vec<Guid> {
        let plane = |g: Guid| self.planes.iter().any(|p| p.guid == g) || DATUMS.contains(&g);
        let line = |g: Guid| self.lines.iter().any(|l| l.guid == g);
        let sketch = |f: Guid, l: Guid| self.forms.iter().any(|x| x.guid == f && x.sketch.iter().any(|y| y.guid == l));
        self.pivots()
            .into_iter()
            .filter(|(_, p)| {
                let to = match p.to {
                    PivotPoint::Crossing(a, b) => plane(a) && plane(b),
                    PivotPoint::LineEnd(l, _) => line(l),
                    PivotPoint::Sketch(f, l, _) => sketch(f, l),
                    PivotPoint::Fixed(_) => true,
                };
                let from = match p.from {
                    PivotFrom::Drawn => true,
                    PivotFrom::Plane(g) => plane(g),
                    PivotFrom::Line(g) => line(g),
                };
                !(to && from)
            })
            .filter_map(|(e, _)| self.pivoted_guid(e))
            .collect()
    }

    /// The swing of pivot `e` as the family stands (a flexed copy), in degrees, as near `near` as a whole turn allows.
    fn swing_now(&self, e: Pivoted, near: f64) -> Result<f64, String> {
        let piv = self.pivot_at(e).ok_or("it has no pivot")?;
        let r = self.rotation(e)?;
        let angle = |v: P3| dot(v, r.frame.y).atan2(dot(v, r.frame.x));
        let element = angle(r.way);
        let reference = match piv.from {
            PivotFrom::Drawn => element - piv.turned,
            PivotFrom::Plane(g) => {
                let f = self.frame(&PlaneRef::Plane(g))?;
                let across = |v: P3| dot(v, r.frame.x).hypot(dot(v, r.frame.y));
                angle(if across(f.x) >= across(f.y) { f.x } else { f.y })
            }
            PivotFrom::Line(g) => {
                let l = self.lines.iter().find(|l| l.guid == g).ok_or("no such line")?;
                angle(sub(l.end, l.start))
            }
        };
        let s = (element - reference).to_degrees();
        Ok(s + ((near - s) / 360.0).round() * 360.0)
    }

    /// The rotate grip of the line or plane `g` dragged to `toward` (the family's axes), for `values`: the swing it comes to
    /// (snapped as [`Pivot::snap`] when `snap`, within its limits) and the edits that make it, the pivot's parameter or, where
    /// none labels it, the pivot's own angle. A parameter a formula works out refuses.
    pub fn drag_pivot(&self, values: &BTreeMap<String, Val>, g: Guid, toward: P3, snap: bool) -> Result<(f64, Vec<Edit>), Unsat> {
        let flexed = self.flexed(values)?;
        let (e, piv) = flexed.pivot_of(g).ok_or_else(|| Unsat::from(format!("{g} has no pivot")))?;
        let r = flexed.rotation(e).map_err(Unsat::from)?;
        let d = sub(toward, r.frame.origin);
        let (x, y) = (dot(d, r.frame.x), dot(d, r.frame.y));
        if x.hypot(y) < 1e-9 {
            return Ok((self.swing_value(piv, values), Vec::new()));
        }
        let angle = |v: P3| dot(v, r.frame.y).atan2(dot(v, r.frame.x));
        let now = flexed.swing_now(e, self.swing_value(piv, values)).map_err(Unsat::from)?;
        let turn = (y.atan2(x) - angle(r.arm)).to_degrees();
        let turn = turn - (turn / 360.0).round() * 360.0;
        let raw = now + turn;
        let v = if snap { piv.snap(raw) } else { piv.clamp(raw) };
        let edit = match &piv.parameter {
            Some(name) => {
                if let Some(p) = self.parameters.iter().find(|p| &p.name == name)
                    && let Some(f) = p.formula.as_deref()
                {
                    return Err(Unsat {
                        message: format!("`{name}` is worked out by its formula `{f}`: change it in Family Types"),
                        dims: Vec::new(),
                        over: true,
                    });
                }
                Edit::Parameter(name.clone(), v)
            }
            None => Edit::PivotAngle(g, v),
        };
        Ok((v, vec![edit]))
    }

    /// What pivot `piv`'s swing is for `values`: its parameter's, else its own angle.
    pub fn swing_value(&self, piv: &Pivot, values: &BTreeMap<String, Val>) -> f64 {
        match piv.parameter.as_ref().and_then(|n| values.get(n)) {
            Some(Val::Number(v)) => *v,
            _ => piv.angle,
        }
    }

    /// Reference line `g`'s start or end (`spot`) moved by `by` (the family's axes), as its grip drags it (T18): the edits that
    /// put it there, on what it is drawn on, with the lines drawn on it carried along.
    pub fn drag_line_end(&self, values: &BTreeMap<String, Val>, g: Guid, spot: Spot, by: P3) -> Result<Vec<Edit>, String> {
        let i = self.lines.iter().position(|l| l.guid == g).ok_or_else(|| format!("line {g} isn't one of its lines"))?;
        let l = &self.lines[i];
        let h = self.frame(&l.host)?;
        let ((mut a, _), (mut b, _)) = (h.local(l.start), h.local(l.end));
        let d = P2(dot(by, h.x), dot(by, h.y));
        if !(d.0.is_finite() && d.1.is_finite()) {
            return Err("the end can't go there".into());
        }
        if spot == Spot::End {
            b = b + d;
        } else {
            a = a + d;
        }
        if a.dist(b) < 1.0 || !b.0.hypot(b.1).max(a.0.hypot(a.1)).le(&MAX_COORD) {
            return Err("a reference line keeps a length within reach".into());
        }
        let s = Solution { lines: vec![(i, [a.0, a.1, (b.1 - a.1).atan2(b.0 - a.0), a.dist(b)])], ..Solution::default() };
        Ok(self.edits(&s, values))
    }

    /// Which point of the line or plane `g` (as `self` stands) is nearest `p`: its start or its end within `reach` mm, else
    /// the point along it; what Pivot takes for `at`.
    pub fn pivot_at_near(&self, g: Guid, p: P3, reach: f64) -> Option<PivotAt> {
        let (start, end) = match self.lines.iter().find(|l| l.guid == g) {
            Some(l) => (l.start, l.end),
            None => self.planes.iter().find(|q| q.guid == g).map(|q| (q.start, q.end))?,
        };
        let d = sub(end, start);
        let len = dot(d, d).sqrt();
        let way = unit(d)?;
        let s = dot(sub(p, start), way).clamp(0.0, len);
        Some(if s <= reach {
            PivotAt::Start
        } else if len - s <= reach {
            PivotAt::End
        } else {
            PivotAt::Along((s * 1000.0).round() / 1000.0)
        })
    }
}

/// A pivot as a view draws it (the view's mm): where it is, and in a view that looks along its axis the rotate grip at
/// its arm's end and, for a limited pivot, its swing range as a polyline (an arc from its minimum to its maximum).
#[derive(Clone, Debug, PartialEq)]
pub struct PivotMark {
    pub element: Guid,
    pub centre: P2,
    pub grip: Option<P2>,
    pub range: Vec<P2>,
    /// Its swing, degrees.
    pub swing: f64,
}

/// The pivots of `flexed` (`values` the type's) as view `which` of `st` draws them.
pub fn pivot_marks(flexed: &Family, values: &BTreeMap<String, Val>, st: &views::Staged, which: views::Which) -> Vec<PivotMark> {
    let eye = DimView::of(which).eye();
    let mut out = Vec::new();
    for (e, piv) in flexed.pivots() {
        let (Some(element), Ok(r)) = (flexed.pivoted_guid(e), flexed.rotation(e)) else { continue };
        let swing = flexed.swing_value(piv, values);
        let swing = flexed.swing_now(e, swing).unwrap_or(swing);
        let centre = st.to_view(which, r.frame.origin);
        let along = parallel(r.frame.z, eye);
        let grip = along.then(|| st.to_view(which, add(r.frame.origin, mul(r.arm, r.reach))));
        let mut range = Vec::new();
        if along && (piv.min.is_some() || piv.max.is_some()) && r.reach > 1e-6 {
            let (lo, hi) = piv.range();
            let arm = dot(r.arm, r.frame.y).atan2(dot(r.arm, r.frame.x));
            let base = arm - swing.to_radians();
            let steps = (((hi - lo).abs() / 3.0).ceil() as usize).clamp(2, 240);
            for k in 0..=steps {
                let t = base + (lo + (hi - lo) * k as f64 / steps as f64).to_radians();
                let p = add(r.frame.origin, add(mul(r.frame.x, r.reach * t.cos()), mul(r.frame.y, r.reach * t.sin())));
                range.push(st.to_view(which, p));
            }
        }
        out.push(PivotMark { element, centre, grip, range, swing });
    }
    out
}

/// The points of `flexed` a pivot may be pinned to in view `which` (the view's mm): where the traces of two planes that
/// hold the way the view looks cross, each reference line's ends and each point of a sketch seen flat.
pub fn pivot_targets(flexed: &Family, values: &BTreeMap<String, Val>, st: &views::Staged, which: views::Which) -> Vec<(PivotPoint, P2)> {
    let eye = DimView::of(which).eye();
    let mut out = Vec::new();
    let traces: Vec<(Guid, Frame)> =
        flexed.planes.iter().map(|p| (p.guid, p.frame())).filter(|(_, f)| dot(f.z, eye).abs() <= 1e-6).collect();
    for (i, (a, fa)) in traces.iter().enumerate() {
        for (b, fb) in traces.iter().skip(i + 1) {
            if parallel(fa.z, fb.z) {
                continue;
            }
            // Where they cross: on both, in the view's plane through the family's origin.
            let (na, nb) = (fa.z, fb.z);
            let (ca, cb) = (dot(na, fa.origin), dot(nb, fb.origin));
            let d = cross(na, nb);
            let k = dot(d, d);
            if k < 1e-12 {
                continue;
            }
            let p = mul(add(mul(cross(nb, d), ca), mul(cross(d, na), cb)), 1.0 / k);
            if p.iter().all(|x| x.is_finite()) {
                out.push((PivotPoint::Crossing(*a, *b), st.to_view(which, p)));
            }
        }
    }
    for l in flexed.lines.iter().filter(|l| !l.guid.is_nil()) {
        out.push((PivotPoint::LineEnd(l.guid, Spot::Start), st.to_view(which, l.start)));
        out.push((PivotPoint::LineEnd(l.guid, Spot::End), st.to_view(which, l.end)));
    }
    let mut read = HashMap::new();
    for form in flexed.forms.iter().filter(|f| f.mesh.is_empty() && !f.guid.is_nil()) {
        let Ok(frame) = flexed.frame(&form.host) else { continue };
        if !parallel(frame.z, eye) {
            continue;
        }
        for l in form.sketch.iter().filter(|l| !l.guid.is_nil()) {
            let mut num = |e| number(e, values, &mut read);
            match &l.curve {
                FormCurve::Line { start, end, .. } => {
                    let (Ok(ax), Ok(ay), Ok(bx), Ok(by)) = (num(&start[0]), num(&start[1]), num(&end[0]), num(&end[1])) else { continue };
                    out.push((PivotPoint::Sketch(form.guid, l.guid, Spot::Start), st.to_view(which, frame.point(P2(ax, ay), 0.0))));
                    out.push((PivotPoint::Sketch(form.guid, l.guid, Spot::End), st.to_view(which, frame.point(P2(bx, by), 0.0))));
                }
                FormCurve::Circle { centre, .. } => {
                    let (Ok(x), Ok(y)) = (num(&centre[0]), num(&centre[1])) else { continue };
                    out.push((PivotPoint::Sketch(form.guid, l.guid, Spot::Centre), st.to_view(which, frame.point(P2(x, y), 0.0))));
                }
            }
        }
    }
    out
}

/// A plane's axes as numbers, made an [`AdFrame`].
fn frame_ad(f: &Frame) -> AdFrame {
    AdFrame { origin: v3(f.origin), x: v3(f.x), y: v3(f.y), z: v3(f.z) }
}

/// `v` turned `phi` (radians) about unit axis `a`.
fn turn(v: P3, a: P3, phi: &Ad) -> V3 {
    let (c, s) = (phi.cos(), phi.sin());
    let ax = cross(a, v);
    let k = dot(a, v);
    std::array::from_fn(|j| c.scale(v[j] - a[j] * k).add(&s.scale(ax[j])).add(&Ad::k(a[j] * k)))
}

/// `v` brought within half a turn of nothing, its derivatives kept.
fn wrapped(mut v: Ad) -> Ad {
    v.v -= (v.v / std::f64::consts::TAU).round() * std::f64::consts::TAU;
    v
}

impl Problem<'_> {
    /// The place of plane `g` in its family's planes, where a pivot turns it in this solve.
    pub(super) fn turning_plane(&self, g: Guid) -> Option<usize> {
        let i = self.family.planes.iter().position(|p| p.guid == g)?;
        self.unknown(Unknown::PlaneTurn(i)).map(|_| i)
    }

    /// The axes of plane `i`, which its pivot turns, where the solve has got to: the drawn ones turned about its axis
    /// through the point it holds, and carried across the axis onto the pivot point.
    pub(super) fn plane_frame_ad(&self, i: usize, depth: usize) -> Result<AdFrame, String> {
        let who = || self.family.pivoted_said(Pivoted::Plane(i));
        if depth == 0 {
            return Err(format!("{}: its pivot reads pivots too deep to follow", who()));
        }
        let p = self.family.planes.get(i).ok_or("no such plane")?;
        let piv = p.pivot.as_ref().ok_or_else(|| format!("{}: it has no pivot", who()))?;
        let fr = p.frame();
        let r = self.family.rotation(Pivoted::Plane(i))?;
        let at0 = r.frame.origin;
        let axis = r.frame.z;
        let point = self.pivot_point(&piv.to, &frame_ad(&r.frame), depth - 1)?;
        let off = vsub(&point, &v3(at0));
        let shift = vsub(&off, &vmul(&v3(axis), &vdotk(&off, axis)));
        let phi = self.value(Unknown::PlaneTurn(i), 0.0);
        let origin = vadd(&vadd(&v3(at0), &shift), &turn(sub(fr.origin, at0), axis, &phi));
        Ok(AdFrame { origin, x: turn(fr.x, axis, &phi), y: turn(fr.y, axis, &phi), z: turn(fr.z, axis, &phi) })
    }

    /// Pivot point `to`, where the solve has got to; a crossing of two planes is the point of their crossing in the plane of
    /// rotation `rot` (its x and y; its z the axis).
    fn pivot_point(&self, to: &PivotPoint, rot: &AdFrame, depth: usize) -> Result<V3, String> {
        if depth == 0 {
            return Err("its pivot reads pivots too deep to follow".into());
        }
        Ok(match *to {
            PivotPoint::Fixed(p) => v3(p),
            PivotPoint::LineEnd(g, s) => {
                let (_, a, b, _) = self.line_ends(g, depth - 1)?;
                if s == Spot::End { b } else { a }
            }
            PivotPoint::Sketch(f, l, s) => {
                let seen = self.seen(f, l)?;
                let at = if seen.shape == Shape::Arc && s == Spot::Centre {
                    self.arc_ad(&seen).0
                } else {
                    let (seen, v) = self.spot(f, l, s)?;
                    self.vertex_ad(seen.form, v)
                };
                self.host_ad(&seen.host, depth - 1)?.point(&at.0, &at.1, &Ad::k(0.0))
            }
            PivotPoint::Crossing(a, b) => {
                let (fa, fb) = (self.host_ad(&PlaneRef::Plane(a), depth - 1)?, self.host_ad(&PlaneRef::Plane(b), depth - 1)?);
                let (a11, a12) = (vdot(&fa.z, &rot.x), vdot(&fa.z, &rot.y));
                let (a21, a22) = (vdot(&fb.z, &rot.x), vdot(&fb.z, &rot.y));
                let r1 = vdot(&fa.z, &vsub(&fa.origin, &rot.origin));
                let r2 = vdot(&fb.z, &vsub(&fb.origin, &rot.origin));
                let det = a11.mul(&a22).sub(&a12.mul(&a21));
                if det.v.abs() < 1e-9 {
                    return Err("its pivot's planes don't cross across its axis".into());
                }
                let u = r1.mul(&a22).sub(&a12.mul(&r2)).div(&det);
                let v = a11.mul(&r2).sub(&r1.mul(&a21)).div(&det);
                vadd(&rot.origin, &vadd(&vmul(&rot.x, &u), &vmul(&rot.y, &v)))
            }
        })
    }

    /// The angle (radians) of what a pivot measures from in the plane of rotation `rot`, where the solve has got to.
    fn reference_angle(&self, from: &PivotFrom, rot: &AdFrame) -> Result<Ad, String> {
        let dir = match *from {
            PivotFrom::Drawn => return Ok(Ad::k(0.0)),
            PivotFrom::Plane(g) => {
                let f = self.host_ad(&PlaneRef::Plane(g), DEPTH)?;
                let across = |v: &V3| vdot(v, &rot.x).v.hypot(vdot(v, &rot.y).v);
                if across(&f.x) >= across(&f.y) { f.x } else { f.y }
            }
            PivotFrom::Line(g) => {
                let (_, a, b, _) = self.line_ends(g, DEPTH)?;
                vsub(&b, &a)
            }
        };
        let (x, y) = (vdot(&dir, &rot.x), vdot(&dir, &rot.y));
        if x.v.hypot(y.v) < 1e-12 {
            return Err("its pivot's reference lies across its axis".into());
        }
        Ok(y.atan2(&x))
    }

    /// Each pivot's rows, where the solve has got to: a line's point on its pivot point (two hard rows), and every pivot's
    /// swing, hard for a parameter and into `soft` for its own angle. A value past its limits can't be solved for.
    pub(super) fn pivot_rows(&self, values: &BTreeMap<String, Val>, rows: &mut Vec<Row>, soft: &mut Vec<Row>) -> Result<(), Unsat> {
        let f = self.family;
        for (e, piv) in f.pivots() {
            let who = format!("{}'s pivot", f.pivoted_said(e));
            let at = |x: String| Unsat::from(format!("{who}: {x}"));
            let swing = match e {
                Pivoted::Line(k) => {
                    if self.unknown(Unknown::LineAngle(k)).is_none() {
                        continue;
                    }
                    let l = &f.lines[k];
                    let h = self.host_ad(&l.host, DEPTH).map_err(at)?;
                    let d = self.line_drawn(k).map_err(at)?;
                    let s = piv.at.distance(d[5]);
                    let su = self.value(Unknown::LineStart(k, 0), d[0]);
                    let sv = self.value(Unknown::LineStart(k, 1), d[1]);
                    let th = self.value(Unknown::LineAngle(k), d[4]);
                    let u = su.add(&th.cos().scale(s));
                    let v = sv.add(&th.sin().scale(s));
                    let p = self.pivot_point(&piv.to, &h, DEPTH).map_err(at)?;
                    let rel = vsub(&p, &h.origin);
                    for (mine, axis) in [(u, &h.x), (v, &h.y)] {
                        rows.push(Row { lin: mine.sub(&vdot(&rel, axis)).lin(&self.at), dim: None, soft: false, pivot: Some(who.clone()) });
                    }
                    let reference = match piv.from {
                        PivotFrom::Drawn => Ad::k(d[4] - piv.turned),
                        from => self.reference_angle(&from, &h).map_err(at)?,
                    };
                    th.sub(&reference)
                }
                Pivoted::Plane(k) => {
                    if self.unknown(Unknown::PlaneTurn(k)).is_none() {
                        continue;
                    }
                    let r = f.rotation(e).map_err(at)?;
                    let phi = self.value(Unknown::PlaneTurn(k), 0.0);
                    let reference = match piv.from {
                        PivotFrom::Drawn => Ad::k(-piv.turned),
                        from => self.reference_angle(&from, &frame_ad(&r.frame)).map_err(at)?,
                    };
                    phi.sub(&reference)
                }
            };
            let (v, hard) = match &piv.parameter {
                Some(name) => match values.get(name) {
                    Some(Val::Number(v)) => (*v, true),
                    _ => return Err(at(format!("its angle `{name}` has no value"))),
                },
                None => (piv.angle, false),
            };
            let label = piv.parameter.as_ref().map_or_else(|| "its angle".to_string(), |n| format!("`{n}`"));
            piv.allows(v, &label, &f.pivoted_said(e)).map_err(|m| Unsat {
                message: format!("{NOT_SATISFIED}: {m}"),
                dims: Vec::new(),
                over: false,
            })?;
            let row = Row { lin: wrapped(swing.sub(&Ad::k(v.to_radians()))).lin(&self.at), dim: None, soft: !hard, pivot: Some(who) };
            if hard {
                rows.push(row);
            } else {
                soft.push(row);
            }
        }
        Ok(())
    }

    /// A point of reference `r` in the family's axes where the solve has got to: what an aligned dimension to a turning
    /// plane measures the distance of (a plane's origin, an extrusion end's on its host's normal, a sketch line's start, a
    /// point, a reference line's face's origin or end).
    fn point_of<'a>(
        &self,
        r: &DimRef,
        values: &BTreeMap<String, Val>,
        read: &mut HashMap<&'a str, Result<Node, String>>,
    ) -> Result<V3, String>
    where
        Self: 'a,
    {
        Ok(match *r {
            DimRef::Plane(g) => self.host_ad(&PlaneRef::Plane(g), DEPTH)?.origin,
            DimRef::Form(g, end) => {
                let (i, form) = end_form(self.family, g)?;
                let value = match self.unknown(Unknown::End(i, end)) {
                    Some(u) => self.var(u),
                    None => Ad::k(number(end_expr(form, end), values, read)?),
                };
                let fr = self.host_ad(&form.host, DEPTH)?;
                vadd(&fr.origin, &vmul(&fr.z, &value))
            }
            DimRef::Sketch(g, l) => {
                let s = self.seen(g, l)?;
                self.sketch_point(&s, &self.vertex_ad(s.form, s.ends.0))?
            }
            DimRef::Point(g, l, Spot::Centre) if self.seen(g, l).is_ok_and(|s| s.shape == Shape::Arc) => {
                let s = self.seen(g, l)?;
                self.sketch_point(&s, &self.arc_ad(&s).0)?
            }
            DimRef::Point(g, l, spot) => {
                let (s, v) = self.spot(g, l, spot)?;
                self.sketch_point(&s, &self.vertex_ad(s.form, v))?
            }
            DimRef::Line(g, face) => self.line_face(g, face, DEPTH)?.origin,
            DimRef::LinePoint(g, spot) => {
                let (_, a, b, _) = self.line_ends(g, DEPTH)?;
                if spot == Spot::End { b } else { a }
            }
        })
    }

    /// The stations of aligned dimension `d`, one of whose references is plane `g` that a pivot turns: each reference's
    /// distance from that plane along its normal as it turns; a sketch line in it kept parallel to the plane by a soft row
    /// into `soft` (in place of F7b's).
    pub(super) fn turned_stations<'a>(
        &self,
        d: &FamilyDimension,
        g: Guid,
        values: &BTreeMap<String, Val>,
        read: &mut HashMap<&'a str, Result<Node, String>>,
        soft: &mut Vec<Row>,
    ) -> Result<Vec<Lin>, String>
    where
        Self: 'a,
    {
        let fa = self.host_ad(&PlaneRef::Plane(g), DEPTH)?;
        let mut out = Vec::with_capacity(d.refs.len());
        for r in &d.refs {
            let p = self.point_of(r, values, read)?;
            out.push(vdot(&vsub(&p, &fa.origin), &fa.z).lin(&self.at));
            if let DimRef::Sketch(f, l) = *r {
                let s = self.seen(f, l)?;
                let a = self.sketch_point(&s, &self.vertex_ad(s.form, s.ends.0))?;
                let b = self.sketch_point(&s, &self.vertex_ad(s.form, s.ends.1))?;
                soft.push(Row { lin: vdot(&vsub(&b, &a), &fa.z).lin(&self.at), dim: None, soft: true, pivot: None });
            }
        }
        Ok(out)
    }
}
