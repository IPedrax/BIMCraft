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

use super::*;
use crate::ids::{Guid, Id};
use crate::model::Lock;
use crate::view3d::dot;

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

/// What a dimension measures to: a reference plane (a datum or one of the family's own), or an extrusion's start or end face.
/// In JSON `{"plane": guid}` or `{"form": guid, "end": "start" | "end"}`; an id reads as the one it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RawDimRef", into = "RawDimRef")]
pub enum DimRef {
    Plane(Guid),
    Form(Guid, Extent),
}

impl DimRef {
    pub fn guid(&self) -> Guid {
        match self {
            DimRef::Plane(g) | DimRef::Form(g, _) => *g,
        }
    }

    fn guid_mut(&mut self) -> &mut Guid {
        match self {
            DimRef::Plane(g) | DimRef::Form(g, _) => g,
        }
    }
}

impl std::fmt::Display for DimRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DimRef::Plane(g) => write!(f, "plane {g}"),
            DimRef::Form(g, Extent::Start) => write!(f, "form {g}'s start"),
            DimRef::Form(g, Extent::End) => write!(f, "form {g}'s end"),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDimRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plane: Option<Guid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    form: Option<Guid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    end: Option<Extent>,
}

impl TryFrom<RawDimRef> for DimRef {
    type Error = String;
    fn try_from(r: RawDimRef) -> Result<DimRef, String> {
        match (r.plane, r.form, r.end) {
            (Some(g), None, None) => Ok(DimRef::Plane(g)),
            (None, Some(g), Some(e)) => Ok(DimRef::Form(g, e)),
            _ => Err("a dimension's reference is {\"plane\": guid} or {\"form\": guid, \"end\": \"start\" | \"end\"}".into()),
        }
    }
}

impl From<DimRef> for RawDimRef {
    fn from(r: DimRef) -> RawDimRef {
        match r {
            DimRef::Plane(g) => RawDimRef { plane: Some(g), form: None, end: None },
            DimRef::Form(g, e) => RawDimRef { plane: None, form: Some(g), end: Some(e) },
        }
    }
}

/// A dimension of the family's, as Revit's aligned dimension in the family editor: between `refs` (parallel, in order along
/// the way it measures), drawn in `view` with its line through `at` (the family's axes, mm). `label` names the length
/// parameter its one segment is (a reporting parameter is measured, not imposed); `locked` keeps segments at a length (a
/// labelled one's lock keeps its parameter from being changed by dragging, Family Types' Lock); `equal` keeps every segment
/// one length (EQ); `alignment` marks the lock Align leaves, one segment locked at 0, drawn as a padlock and never printed.
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
}

impl FamilyDimension {
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

/// What can move: a plane of the family's own (its place in `planes`) along its normal, or an extrusion's end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Unknown {
    Plane(usize),
    End(usize, Extent),
}

/// A position along a normal: the sum of some unknowns, each times its factor, and a constant.
#[derive(Clone, Debug, Default)]
struct Lin {
    terms: Vec<(usize, f64)>,
    c: f64,
}

impl Lin {
    fn scaled(mut self, k: f64) -> Lin {
        self.terms.iter_mut().for_each(|t| t.1 *= k);
        self.c *= k;
        self
    }

    fn minus(mut self, other: &Lin) -> Lin {
        self.terms.extend(other.terms.iter().map(|(u, k)| (*u, -k)));
        self.c -= other.c;
        self
    }

    fn at(&self, x: &[f64]) -> f64 {
        self.terms.iter().map(|(u, k)| k * x.get(*u).copied().unwrap_or(0.0)).sum::<f64>() + self.c
    }
}

/// One constraint: `lin = 0`, held for dimension `dim` (none: a pin a drag adds).
struct Row {
    lin: Lin,
    dim: Option<usize>,
}

/// What one solve came to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Solution {
    /// Each plane of the family's own that moved: its place in `planes` and its new offset along its normal.
    pub planes: Vec<(usize, f64)>,
    /// Each extrusion end the constraints set: the form's place, which end, and its value along its host's normal.
    pub ends: Vec<(usize, Extent, f64)>,
}

/// A change a drag or Align asks of the draft: a plane's offset along its normal, an extrusion end's value, a parameter's.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Plane(Guid, f64),
    End(usize, Extent, f64),
    Parameter(String, f64),
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

/// The working of one solve: what may move, and where it is drawn.
struct Problem<'f> {
    family: &'f Family,
    unknowns: Vec<Unknown>,
    x0: Vec<f64>,
}

impl<'f> Problem<'f> {
    fn unknown(&self, u: Unknown) -> Option<usize> {
        self.unknowns.iter().position(|v| *v == u)
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
            Some(u) => (z, Lin { terms: vec![(u, 1.0)], c: 0.0 }),
            None => (z, Lin { terms: Vec::new(), c: s }),
        })
    }

    /// Reference `r`'s normal and its position along it, worked out over `values`.
    fn position<'a>(
        &self,
        r: &DimRef,
        values: &BTreeMap<String, Val>,
        read: &mut HashMap<&'a str, Result<Node, String>>,
    ) -> Result<(P3, Lin), String>
    where
        'f: 'a,
    {
        match *r {
            DimRef::Plane(g) => self.plane_pos(g),
            DimRef::Form(g, end) => {
                let (i, form) = end_form(self.family, g)?;
                let PlaneRef::Plane(h) = form.host else {
                    return Err(format!(
                        "form {}: an extrusion drawn on a reference line's plane is dimensioned once lines turn (F7c)",
                        i + 1
                    ));
                };
                let (z, host) = self.plane_pos(h)?;
                let value = match self.unknown(Unknown::End(i, end)) {
                    Some(u) => Lin { terms: vec![(u, 1.0)], c: 0.0 },
                    None => Lin { terms: Vec::new(), c: number(end_expr(form, end), values, read)? },
                };
                let mut lin = host;
                lin.terms.extend(value.terms);
                lin.c += value.c;
                Ok((z, lin))
            }
        }
    }
}

/// The form of GUID `g` an end of which is measured: an extrusion of a profile or a circle.
fn end_form(f: &Family, g: Guid) -> Result<(usize, &Form), String> {
    let i = f.forms.iter().position(|x| x.guid == g).ok_or_else(|| format!("form {g} isn't one of its forms"))?;
    let form = &f.forms[i];
    if !form.mesh.is_empty() || form.axis.is_some() {
        return Err(format!("form {}: only an extrusion's start and end are measured", i + 1));
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

impl Family {
    /// Whether parameter `name` is a reporting one.
    fn reporting(&self, name: &str) -> bool {
        self.parameters.iter().any(|p| p.name == name && p.reporting)
    }

    /// Whether its dimensions are sound, whatever the values: no more than it may hold, each measuring between two or more of
    /// its references (each once), parallel and lying across its view; a label naming a length parameter on a dimension of
    /// one segment; locks on segments it has; Align's lock of two references, locked at 0; and no label worked out from a
    /// reporting parameter another dimension measures, which would be measured from itself.
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
            if d.refs.len() < 2 || d.refs.len() > MAX_DIM_REFS {
                return Err(format!("{who}: it measures between 2 to {MAX_DIM_REFS} references, not {}", d.refs.len()));
            }
            if !d.at.iter().all(|x| x.is_finite() && x.abs() <= MAX_COORD) {
                return Err(format!("{who}: its `at` is a point within {MAX_COORD} mm"));
            }
            let mut seen = BTreeSet::new();
            for r in &d.refs {
                if !seen.insert((r.guid(), matches!(r, DimRef::Form(_, Extent::End)))) {
                    return Err(format!("{who}: it measures to {r} twice"));
                }
            }
            let problem = Problem { family: self, unknowns: Vec::new(), x0: Vec::new() };
            let mut normals = Vec::new();
            for r in &d.refs {
                let (z, _) = match r {
                    DimRef::Plane(g) => problem.plane_pos(*g),
                    DimRef::Form(g, _) => {
                        let (fi, form) = end_form(self, *g).map_err(|e| format!("{who}: {e}"))?;
                        match form.host {
                            PlaneRef::Plane(h) => problem.plane_pos(h),
                            PlaneRef::Line(..) => Err(format!(
                                "form {}: an extrusion drawn on a reference line's plane is dimensioned once lines turn (F7c)",
                                fi + 1
                            )),
                        }
                    }
                }
                .map_err(|e| format!("{who}: {e}"))?;
                normals.push(z);
            }
            let m = normals.first().copied().unwrap_or([0.0, 0.0, 1.0]);
            if normals.iter().any(|n| dot(cross(*n, m), cross(*n, m)).sqrt() > PARALLEL) {
                return Err(format!("{who}: its references aren't parallel; a dimension measures between parallel ones"));
            }
            if dot(m, d.view.eye()).abs() > PARALLEL {
                return Err(format!("{who}: it measures the way its view looks; draw it in a view its references are seen edge on in"));
            }
            let segments = d.refs.len() - 1;
            if let Some(l) = &d.label {
                let p = self
                    .parameters
                    .iter()
                    .find(|p| &p.name == l)
                    .ok_or_else(|| format!("{who}: its label: {}", not_a_parameter(l, &self.parameters)))?;
                if p.kind != Kind::Length {
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
                if !l.length.is_finite() || l.length < 0.0 || l.length > MAX_COORD {
                    return Err(format!("{who}: a locked length is from 0 to {MAX_COORD} mm, not {}", l.length));
                }
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
        self.dimensions.iter().any(|d| d.constrains(&reporting))
    }

    /// Its dimensions solved for `values`: where the planes and extrusion ends they hold go, the least change from where they
    /// are drawn that holds them all.
    pub fn solve(&self, values: &BTreeMap<String, Val>) -> Result<Solution, Unsat> {
        self.solve_with(values, &[], &[], &mut Meter::default())
    }

    /// [`Family::solve`], plane `pins` (GUID, offset along its normal) held where they are said to be as well, the labels of
    /// the dimensions `free` (by place) not held, and the work counted on `meter`.
    fn solve_with(
        &self,
        values: &BTreeMap<String, Val>,
        pins: &[(Guid, f64)],
        free: &[usize],
        meter: &mut Meter,
    ) -> Result<Solution, Unsat> {
        let reporting = |l: &str| self.reporting(l);
        if !self.constrained() && pins.is_empty() {
            return Ok(Solution::default());
        }
        // What may move: each plane of its own and each extrusion end a constraint measures to, in the dimensions' order.
        let mut unknowns = Vec::new();
        let mut add = |u: Unknown| {
            if !unknowns.contains(&u) {
                unknowns.push(u);
            }
        };
        for d in self.dimensions.iter().filter(|d| d.constrains(&reporting)) {
            for r in &d.refs {
                match *r {
                    DimRef::Plane(g) => {
                        if let Some(i) = self.planes.iter().position(|p| p.guid == g && !p.datum() && !p.defines_origin) {
                            add(Unknown::Plane(i));
                        }
                    }
                    DimRef::Form(g, end) => {
                        if let Some(i) = self.forms.iter().position(|f| f.guid == g) {
                            add(Unknown::End(i, end));
                        }
                    }
                }
            }
        }
        for (g, _) in pins {
            if let Some(i) = self.planes.iter().position(|p| p.guid == *g && !p.datum() && !p.defines_origin) {
                add(Unknown::Plane(i));
            }
        }
        let mut read = HashMap::new();
        let mut x0 = Vec::with_capacity(unknowns.len());
        for u in &unknowns {
            x0.push(match *u {
                Unknown::Plane(i) => self.planes.get(i).map_or(0.0, |p| dot(p.start, p.frame().z)),
                Unknown::End(i, end) => match self.forms.get(i) {
                    Some(f) => number(end_expr(f, end), values, &mut read).map_err(|e| format!("form {}: {e}", i + 1))?,
                    None => 0.0,
                },
            });
        }
        let problem = Problem { family: self, unknowns, x0 };
        // The rows, dimension after dimension, then the pins.
        let mut rows: Vec<Row> = Vec::new();
        for (k, d) in self.dimensions.iter().enumerate() {
            if !d.constrains(&reporting) {
                continue;
            }
            let who = format!("dimension {}", k + 1);
            let mut stations: Vec<Lin> = Vec::new();
            let mut m: Option<P3> = None;
            for r in &d.refs {
                let (n, lin) = problem.position(r, values, &mut read).map_err(|e| Unsat::from(format!("{who}: {e}")))?;
                let m = *m.get_or_insert(n);
                let sign = if dot(n, m) < 0.0 { -1.0 } else { 1.0 };
                stations.push(lin.scaled(sign));
            }
            // Each segment, the way it runs as drawn: a label or a lock is a length along it.
            let segments: Vec<Lin> = stations
                .windows(2)
                .map(|w| {
                    let s = w[1].clone().minus(&w[0]);
                    if s.at(&problem.x0) < -1e-9 { s.scaled(-1.0) } else { s }
                })
                .collect();
            let labelled = d.label.as_deref().filter(|l| !reporting(l) && !free.contains(&k));
            if let (Some(l), Some(s)) = (labelled, segments.first()) {
                let v = match values.get(l) {
                    Some(Val::Number(v)) => *v,
                    _ => return Err(format!("{who}: its label `{l}` has no length").into()),
                };
                if !v.is_finite() || v <= 0.0 || v > MAX_COORD {
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
                lin.c -= v;
                rows.push(Row { lin, dim: Some(k) });
            }
            for lock in &d.locked {
                if labelled.is_some() && lock.segment == 0 {
                    continue;
                }
                if let Some(s) = segments.get(lock.segment) {
                    let mut lin = s.clone();
                    lin.c -= lock.length;
                    rows.push(Row { lin, dim: Some(k) });
                }
            }
            if d.equal
                && let Some(first) = segments.first()
            {
                for s in segments.iter().skip(1) {
                    rows.push(Row { lin: s.clone().minus(first), dim: Some(k) });
                }
            }
        }
        for (g, at) in pins {
            let (_, lin) = problem.plane_pos(*g).map_err(Unsat::from)?;
            let mut lin = lin;
            lin.c -= at;
            rows.push(Row { lin, dim: None });
        }
        // An extrusion's end gives way before a plane does: what a plane holds it to, it follows.
        let give: Vec<f64> = problem.unknowns.iter().map(|u| if matches!(u, Unknown::End(..)) { END_GIVES } else { 1.0 }).collect();
        let x = solve_rows(&problem.x0, &give, &rows, meter)?;
        // To a millionth of a millimetre, so what the arithmetic leaves over (500.00000000000006) doesn't show.
        let x: Vec<f64> = x.into_iter().map(|v| (v * 1e6).round() / 1e6).collect();
        if x.iter().any(|v| !v.is_finite() || v.abs() > MAX_COORD) {
            return Err(Unsat {
                message: format!("{NOT_SATISFIED}: they would put a plane or a form's end past {MAX_COORD} mm"),
                dims: Vec::new(),
                over: false,
            });
        }
        let mut out = Solution::default();
        for (u, v) in problem.unknowns.iter().zip(&x) {
            match *u {
                Unknown::Plane(i) => out.planes.push((i, *v)),
                Unknown::End(i, end) => out.ends.push((i, end, *v)),
            }
        }
        Ok(out)
    }

    /// The family flexed for `values`: its planes where its dimensions put them (and the lines and forms drawn on them with
    /// them), its extrusions' ends the constraints set given as numbers. Itself, borrowed, when it holds no constraint.
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
        let mut shift: Vec<(Guid, P3)> = Vec::new();
        for (i, v) in &s.planes {
            let Some(p) = f.planes.get_mut(*i) else { continue };
            let z = p.frame().z;
            let d = v - dot(p.start, z);
            if d.abs() > 0.0 {
                let by = mul(z, d);
                p.start = add(p.start, by);
                p.end = add(p.end, by);
                shift.push((p.guid, by));
            }
        }
        // A reference line lies on what it is drawn on: moved with it, and what is drawn on it with it.
        if !shift.is_empty() {
            let mut lines: Vec<(Guid, P3)> = Vec::new();
            for l in &mut f.lines {
                let by = match l.host {
                    PlaneRef::Plane(g) => shift.iter().find(|(h, _)| *h == g).map(|(_, b)| *b),
                    PlaneRef::Line(g, _) => lines.iter().find(|(h, _)| *h == g).map(|(_, b)| *b),
                };
                if let Some(by) = by {
                    l.start = add(l.start, by);
                    l.end = add(l.end, by);
                    lines.push((l.guid, by));
                }
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
        f
    }

    /// Where each reference of dimension `d` is along the way it measures, for `values`, as the family stands (flexed or
    /// not): the direction (that of its first reference) and each one's place along it.
    pub fn stations(&self, d: &FamilyDimension, values: &BTreeMap<String, Val>) -> Result<(P3, Vec<f64>), String> {
        let problem = Problem { family: self, unknowns: Vec::new(), x0: Vec::new() };
        let mut read = HashMap::new();
        let mut m = None;
        let mut out = Vec::new();
        for r in &d.refs {
            let (n, lin) = problem.position(r, values, &mut read)?;
            let m = *m.get_or_insert(n);
            out.push(if dot(n, m) < 0.0 { -lin.c } else { lin.c });
        }
        Ok((m.unwrap_or([1.0, 0.0, 0.0]), out))
    }

    /// Each segment's length of each of its dimensions, flexed for `values` (empty for one that can't be measured).
    pub fn measured(&self, values: &BTreeMap<String, Val>) -> Result<Vec<Vec<f64>>, Unsat> {
        let f = self.flexed(values)?;
        Ok(f.dimensions
            .iter()
            .map(|d| f.stations(d, values).map(|(_, p)| p.windows(2).map(|w| (w[1] - w[0]).abs()).collect()).unwrap_or_default())
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
        if self.planes[i].datum() || self.planes[i].defines_origin {
            return Err(Unsat {
                message: format!("{NOT_SATISFIED}: the plane defines the origin, which stays"),
                dims: Vec::new(),
                over: true,
            });
        }
        let mut meter = Meter::default();
        let edits =
            |s: &Solution| -> Vec<Edit> { s.planes.iter().filter_map(|(i, v)| Some(Edit::Plane(self.planes.get(*i)?.guid, *v))).collect() };
        // The labels it may change: of the dimensions measuring to it, free to take another length.
        let free: Vec<usize> = self
            .dimensions
            .iter()
            .enumerate()
            .filter(|(_, d)| d.refs.contains(&DimRef::Plane(g)) && !d.is_locked(0))
            .filter(|(_, d)| {
                d.label.as_deref().is_some_and(|l| self.parameters.iter().any(|p| p.name == l && p.formula.is_none() && !p.reporting))
            })
            .map(|(k, _)| k)
            .collect();
        if !free.is_empty()
            && let Ok(s) = self.solve_with(values, &[(g, to)], &free, &mut meter)
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
            out.extend(edits(&s));
            return Ok(out);
        }
        match self.solve_with(values, &[(g, to)], &[], &mut meter) {
            Ok(s) => Ok(edits(&s)),
            Err(e) if e.over => Err(Unsat { message: format!("{NOT_SATISFIED}: {}", e.message), ..e }),
            Err(e) => Err(e),
        }
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

    /// What its dimensions measure to that isn't there: each dimension (by place) with a reference it has no plane or form
    /// for.
    pub fn dangling_dimensions(&self) -> Vec<usize> {
        self.dimensions
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                d.refs.iter().any(|r| match r {
                    DimRef::Plane(g) => !self.planes.iter().any(|p| p.guid == *g) && !DATUMS.contains(g),
                    DimRef::Form(g, _) => !self.forms.iter().any(|f| f.guid == *g),
                })
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Reference `gone` taken from its dimensions, as deleting the plane or form does in Revit: each dimension measuring to it
    /// loses it (its locks renumbered, its label and equality kept while it still has segments), and one left with fewer
    /// than two references goes. How many went.
    pub fn forget_reference(&mut self, gone: Guid) -> usize {
        let before = self.dimensions.len();
        for d in &mut self.dimensions {
            while let Some(at) = d.refs.iter().position(|r| r.guid() == gone) {
                d.refs.remove(at);
                // The segments either side of it become one: a lock on either goes, those after move down.
                d.locked.retain(|l| l.segment + 1 != at && l.segment != at);
                d.locked.iter_mut().filter(|l| l.segment > at).for_each(|l| l.segment -= 1);
                if d.refs.len() != 2 {
                    d.label = None;
                }
            }
        }
        self.dimensions.retain(|d| d.refs.len() >= 2);
        before - self.dimensions.len()
    }

    /// Each dimension's reference given as an id (as a command may) given as the GUID of the plane or form of that id; a
    /// plane or a form given another GUID (`planes`, `forms`: from, to) followed.
    pub(crate) fn resolve_dimension_refs(&mut self, planes: &[(Guid, Guid)], forms: &[(Guid, Guid)]) {
        let plane_ids: Vec<(Id, Guid)> = self.planes.iter().map(|p| (p.id, p.guid)).collect();
        let form_ids: Vec<(Id, Guid)> = self.forms.iter().map(|f| (f.id, f.guid)).collect();
        for r in self.dimensions.iter_mut().flat_map(|d| d.refs.iter_mut()) {
            let (ids, moved) = if matches!(r, DimRef::Plane(_)) { (&plane_ids, planes) } else { (&form_ids, forms) };
            let g = r.guid_mut();
            if let Some(id) = g.pending()
                && let Some((_, found)) = ids.iter().find(|(i, found)| *i == id && !crate::ids::unset(i) && !found.is_nil())
            {
                *g = *found;
            }
            if let Some((_, to)) = moved.iter().find(|(from, _)| from == g) {
                *g = *to;
            }
        }
    }
}

/// How much more readily an extrusion's end moves than a plane: the least change weighs its moves this much less.
const END_GIVES: f64 = 1e6;

/// The least change `dx` from `x0` with every row held (`lin(x0 + dx) = 0`), each connected group of unknowns on its own, an
/// unknown's change weighed down by its `give` (the change of `give · y`, `y` the least). A row that adds nothing to those
/// before it in its group is refused, naming the dimensions it comes from.
fn solve_rows(x0: &[f64], give: &[f64], rows: &[Row], meter: &mut Meter) -> Result<Vec<f64>, Unsat> {
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
    for (k, row) in rows.iter().enumerate() {
        match row.lin.terms.iter().map(|t| t.0).find(|u| *u < n) {
            Some(u) => {
                let r = root(&mut parent, u);
                groups.entry(r).or_default().1.push(k);
            }
            // Nothing in it may move: it adds nothing.
            None => return Err(over(rows, k, &[])),
        }
    }
    for (_, (us, ks)) in groups {
        if ks.is_empty() {
            continue;
        }
        if us.len() > MAX_UNKNOWNS {
            return Err(format!("its constraints hold more than {MAX_UNKNOWNS} planes and ends together: hold fewer of them").into());
        }
        let dim = us.len();
        let col = |u: usize| us.iter().position(|v| *v == u);
        // Modified Gram-Schmidt over the group's rows in order, each orthonormal row kept with what it is of the rows.
        let mut qs: Vec<Vec<f64>> = Vec::new();
        let mut ts: Vec<Vec<f64>> = Vec::new();
        let mut taken: Vec<usize> = Vec::new();
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
                let with: Vec<usize> = t.iter().enumerate().filter(|(i, w)| *i != j && w.abs() > 1e-9).map(|(i, _)| ks[i]).collect();
                return Err(over(rows, k, &with));
            }
            qs.push(v.into_iter().map(|a| a / left).collect());
            ts.push(t.into_iter().map(|a| a / left).collect());
            taken.push(j);
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
    Ok(x)
}

fn dot_n(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Row `k` refused: it adds nothing to rows `with` (by place in `rows`).
fn over(rows: &[Row], k: usize, with: &[usize]) -> Unsat {
    let own = rows.get(k).and_then(|r| r.dim);
    let mut dims: Vec<usize> = with.iter().filter_map(|i| rows.get(*i).and_then(|r| r.dim)).filter(|d| Some(*d) != own).collect();
    dims.sort_unstable();
    dims.dedup();
    let named = |ds: &[usize]| ds.iter().map(|d| (d + 1).to_string()).collect::<Vec<_>>().join(", ");
    let message = match (own, dims.is_empty()) {
        (Some(d), true) => format!("dimension {} is over-constrained: what it holds can't move", d + 1),
        (Some(d), false) => format!("dimension {} is over-constrained: dimension {} hold what it holds already", d + 1, named(&dims)),
        (None, true) => "the plane is fixed".to_string(),
        (None, false) => format!("dimension {} hold the plane where it is", named(&dims)),
    };
    let mut all = dims;
    if let Some(d) = own {
        all.insert(0, d);
    }
    Unsat { message, dims: all, over: true }
}

/// A dimension as a family's view draws it: its references' places on its line (the view's mm), the way along it and
/// across it there, each segment's length, its label and which segments are locked.
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
}

/// The dimensions of `flexed` (the family flexed for `values`) drawn in view `which` of `st`.
pub fn marks(flexed: &Family, values: &BTreeMap<String, Val>, st: &views::Staged, which: views::Which) -> Vec<DimMark> {
    let view = DimView::of(which);
    let mut out = Vec::new();
    for (index, d) in flexed.dimensions.iter().enumerate().filter(|(_, d)| d.view == view) {
        let Ok((m, p)) = flexed.stations(d, values) else { continue };
        let base = dot(d.at, m);
        let points: Vec<P2> = p.iter().map(|s| st.to_view(which, add(d.at, mul(m, s - base)))).collect();
        let (a, b) = (st.to_view(which, d.at), st.to_view(which, add(d.at, m)));
        let along = b - a;
        let len = along.dot(along).sqrt();
        if len < 1e-9 || !len.is_finite() {
            continue;
        }
        let along = along * (1.0 / len);
        let across = P2(-along.1, along.0);
        let lengths: Vec<f64> = p.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        let locked = (0..lengths.len()).map(|i| d.is_locked(i)).collect();
        out.push(DimMark { index, points, along, across, lengths, label: d.label.clone(), locked, equal: d.equal, alignment: d.alignment });
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
        if !form.mesh.is_empty() || form.axis.is_some() || form.guid.is_nil() {
            continue;
        }
        let PlaneRef::Plane(_) = form.host else { continue };
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

/// Where reference `r` of `flexed` is along the normal `m`, for `values`: what Align and a new dimension work from.
pub fn place_along(flexed: &Family, r: &DimRef, values: &BTreeMap<String, Val>) -> Result<(P3, f64), String> {
    let problem = Problem { family: flexed, unknowns: Vec::new(), x0: Vec::new() };
    let (n, lin) = problem.position(r, values, &mut HashMap::new())?;
    Ok((n, lin.c))
}

/// Align (AL): `target` moved onto `reference` (parallel to it), as Revit's Align moves it, for `values`: the edits that
/// do it. A plane is dragged ([`Family::drag_plane`]); an extrusion end the constraints don't set takes the new value.
pub fn align(f: &Family, values: &BTreeMap<String, Val>, reference: &DimRef, target: &DimRef) -> Result<Vec<Edit>, Unsat> {
    let flexed = f.flexed(values)?;
    let (n, at) = place_along(&flexed, reference, values).map_err(Unsat::from)?;
    let (m, was) = place_along(&flexed, target, values).map_err(Unsat::from)?;
    if dot(cross(n, m), cross(n, m)).sqrt() > PARALLEL {
        return Err("Align takes a reference parallel to the first".to_string().into());
    }
    let sign = if dot(n, m) < 0.0 { -1.0 } else { 1.0 };
    let want = at * sign;
    if (want - was).abs() <= 1e-9 {
        return Ok(Vec::new());
    }
    match *target {
        DimRef::Plane(g) => f.drag_plane(values, g, want),
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
            Ok(vec![Edit::End(i, end, value + (want - was))])
        }
    }
}
