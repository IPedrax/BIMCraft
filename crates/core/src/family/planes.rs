//! A family's reference planes and reference lines, as Revit's Family Editor has them (F2, T1, T18): the three datums every
//! family holds (Ref. Level, Center (Front/Back), Center (Left/Right)), the planes and lines a family adds, and the frame
//! of whichever of them a form is drawn on (`Form::host`): its profile in the frame's x and y, swept along its z.

use super::*;
use crate::ids::{Guid, Id};
use crate::view3d::{dot, sub};
use serde_json::json;

/// Ref. Level's GUID, the same in every family: the family's xy, where its base stands.
pub const REF_LEVEL: Guid = Guid::from_u128(0x9e3c_0a10_1f0b_4d8c_8a5e_0000_0000_00a1);
/// Center (Front/Back)'s GUID, the same in every family: the family's xz, its normal toward -y.
pub const CENTER_FRONT_BACK: Guid = Guid::from_u128(0x9e3c_0a10_1f0b_4d8c_8a5e_0000_0000_00a2);
/// Center (Left/Right)'s GUID, the same in every family: the family's yz, its normal toward +x.
pub const CENTER_LEFT_RIGHT: Guid = Guid::from_u128(0x9e3c_0a10_1f0b_4d8c_8a5e_0000_0000_00a3);
/// The datums, in the order a family's `planes` starts with them.
pub const DATUMS: [Guid; 3] = [REF_LEVEL, CENTER_FRONT_BACK, CENTER_LEFT_RIGHT];
/// Reference planes and reference lines one family may have (its datums among the planes).
pub const MAX_PLANES: usize = 256;
pub const MAX_LINES: usize = 256;
/// How far a reference line's end may be off its host plane, mm, before it is said not to lie on it.
const ON_PLANE: f64 = 1e-3;

/// What a reference plane or line is to the component placed from the family (Revit's Is Reference): a dimension or an
/// alignment may pick a reference, a strong one first; the named ones (Left through Top) are each at most one plane or line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reference {
    NotAReference,
    Strong,
    #[default]
    Weak,
    Left,
    CenterLeftRight,
    Right,
    Front,
    CenterFrontBack,
    Back,
    Bottom,
    CenterElevation,
    Top,
}

impl Reference {
    /// Whether it names one side or middle of the family (Left through Top), which only one plane or line may.
    pub fn named(self) -> bool {
        !matches!(self, Reference::NotAReference | Reference::Strong | Reference::Weak)
    }
}

/// A reference plane of a family, as it was drawn: the line from `start` to `end` (the family's axes, mm) and `up`, the
/// plane's other direction. Its own axes: x along `end - start`, y `up` made square to x, z (its normal) `x × y`; its
/// origin is the point of the plane nearest the family's origin, so a profile drawn on a plane parallel to a datum sits
/// where it would on the datum, moved along the normal.
///
/// `defines_origin` is stored and checked (at most one plane for each of x, y and z, and only a plane square to one of
/// them), but moving the family's origin to a plane that takes it over, with the placed components' `at` corrected so they
/// stay where they are, is not done yet: the datums are the origin whatever the flag says.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefPlane {
    /// Its id and GUID, as a form's: given when it's made, kept while it stays where it is in its list. A datum has its
    /// well-known GUID and no id of its own.
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: Id,
    #[serde(default, skip_serializing_if = "Guid::is_nil")]
    pub guid: Guid,
    /// Unique in the family where it has one; Revit's planes may have none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub up: [f64; 3],
    #[serde(default)]
    pub reference: Reference,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub defines_origin: bool,
    /// A Reference Planes subcategory, by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subcategory: Option<String>,
    /// Whether it is a level, not a reference plane: only Ref. Level is.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub level: bool,
}

/// One of a reference line's four planes: `horizontal`, the plane it is drawn on, through it; `vertical`, square to that
/// through it; `start` and `end`, square to it at each end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Face {
    #[default]
    Horizontal,
    Vertical,
    Start,
    End,
}

/// The plane something is drawn on: a reference plane of the family (a datum or not), or one of a reference line's four.
/// In JSON `{"plane": guid}` or `{"line": guid, "face": horizontal|vertical|start|end}`; an id reads as the one it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawRef", into = "RawRef")]
pub enum PlaneRef {
    Plane(Guid),
    Line(Guid, Face),
}

impl Default for PlaneRef {
    fn default() -> PlaneRef {
        PlaneRef::Plane(REF_LEVEL)
    }
}

impl PlaneRef {
    /// The plane or line it names.
    pub fn guid(&self) -> Guid {
        match self {
            PlaneRef::Plane(g) | PlaneRef::Line(g, _) => *g,
        }
    }

    fn guid_mut(&mut self) -> &mut Guid {
        match self {
            PlaneRef::Plane(g) | PlaneRef::Line(g, _) => g,
        }
    }
}

impl std::fmt::Display for PlaneRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlaneRef::Plane(g) => write!(f, "plane {g}"),
            PlaneRef::Line(g, face) => write!(f, "line {g}'s {} plane", json!(face).as_str().unwrap_or_default()),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plane: Option<Guid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    line: Option<Guid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    face: Option<Face>,
}

impl TryFrom<RawRef> for PlaneRef {
    type Error = String;
    fn try_from(r: RawRef) -> Result<PlaneRef, String> {
        match (r.plane, r.line, r.face) {
            (Some(g), None, None) => Ok(PlaneRef::Plane(g)),
            (None, Some(g), face) => Ok(PlaneRef::Line(g, face.unwrap_or_default())),
            _ => Err("a host is {\"plane\": guid} or {\"line\": guid, \"face\": horizontal|vertical|start|end}".into()),
        }
    }
}

impl From<PlaneRef> for RawRef {
    fn from(r: PlaneRef) -> RawRef {
        match r {
            PlaneRef::Plane(g) => RawRef { plane: Some(g), line: None, face: None },
            PlaneRef::Line(g, face) => RawRef { plane: None, line: Some(g), face: Some(face) },
        }
    }
}

/// A reference line of a family: from `start` to `end` (the family's axes, mm), drawn on `host`, which it lies on. Its four
/// planes are [`Face`]'s; each has its origin at the line's start (`end`'s at its end), so what is drawn on one turns with
/// the line about its start. A line hosted on another line's plane names a line listed before it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefLine {
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: Id,
    #[serde(default, skip_serializing_if = "Guid::is_nil")]
    pub guid: Guid,
    pub start: [f64; 3],
    pub end: [f64; 3],
    #[serde(default)]
    pub host: PlaneRef,
    #[serde(default)]
    pub reference: Reference,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subcategory: Option<String>,
}

/// A plane's own axes in the family's: a point `p` of a profile drawn on it, `t` along its normal, is
/// `origin + x p.0 + y p.1 + z t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: P3,
    pub x: P3,
    pub y: P3,
    pub z: P3,
}

impl Frame {
    /// The family's xy, as Ref. Level is.
    pub const LEVEL: Frame = Frame { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0] };
    /// The family's xz, its normal -y, as Center (Front/Back) is.
    pub const FRONT_BACK: Frame = Frame { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 0.0, 1.0], z: [0.0, -1.0, 0.0] };
    /// The family's yz, its normal +x, as Center (Left/Right) is.
    pub const LEFT_RIGHT: Frame = Frame { origin: [0.0; 3], x: [0.0, 1.0, 0.0], y: [0.0, 0.0, 1.0], z: [1.0, 0.0, 0.0] };

    /// The point of the profile at `p`, `t` along the normal.
    pub fn point(&self, p: P2, t: f64) -> P3 {
        std::array::from_fn(|k| self.origin[k] + self.x[k] * p.0 + self.y[k] * p.1 + self.z[k] * t)
    }

    /// The direction `p` of the profile's plane, in the family's axes.
    pub fn dir(&self, p: P2) -> P3 {
        std::array::from_fn(|k| self.x[k] * p.0 + self.y[k] * p.1)
    }

    /// Whether x, y and the normal turn as the family's x, y and z do: a mirrored frame's triangles turn the other way.
    pub fn right_handed(&self) -> bool {
        dot(cross(self.x, self.y), self.z) > 0.0
    }
}

/// `a` made a unit long, each part divided (so an axis' own direction comes out exact).
fn unit(a: P3) -> P3 {
    let l = dot(a, a).sqrt();
    if l > 0.0 { a.map(|x| x / l) } else { [0.0; 3] }
}

/// Whether every part is a number within [`MAX_COORD`].
fn usable(p: &P3) -> bool {
    p.iter().all(|x| x.is_finite() && x.abs() <= MAX_COORD)
}

/// The three datums every family holds, first in its `planes`, as their GUIDs say: pinned, never moved or taken away.
pub fn datums() -> Vec<RefPlane> {
    let datum = |guid, name: &str, end: P3, up: P3, reference, level| RefPlane {
        id: 0,
        guid,
        name: name.to_string(),
        start: end.map(|x| -x),
        end,
        up,
        reference,
        defines_origin: true,
        subcategory: None,
        level,
    };
    vec![
        datum(REF_LEVEL, "Ref. Level", [1000.0, 0.0, 0.0], [0.0, 1000.0, 0.0], Reference::Bottom, true),
        datum(CENTER_FRONT_BACK, "Center (Front/Back)", [1000.0, 0.0, 0.0], [0.0, 0.0, 1000.0], Reference::CenterFrontBack, false),
        datum(CENTER_LEFT_RIGHT, "Center (Left/Right)", [0.0, 1000.0, 0.0], [0.0, 0.0, 1000.0], Reference::CenterLeftRight, false),
    ]
}

/// A family's planes as a file or a command gives them, its datums put first as they are (what the file says of them is not
/// its to say), and which plane defines the origin in each direction settled ([`Family::settle_planes`]).
pub(super) fn with_datums<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<RefPlane>, D::Error> {
    let given = Vec::<RefPlane>::deserialize(d)?;
    let mut planes = datums();
    planes.extend(given.into_iter().filter(|p| !p.datum()));
    settle_origin(&mut planes);
    Ok(planes)
}

/// Each datum defines the origin in its direction unless another plane of that direction does.
fn settle_origin(planes: &mut [RefPlane]) {
    let (datums, others) = planes.split_at_mut(3.min(planes.len()));
    for d in datums {
        let axis = d.direction();
        d.defines_origin = !others.iter().any(|p| p.defines_origin && p.direction() == axis);
    }
}

impl RefPlane {
    /// Whether it is one of the three datums every family holds.
    pub fn datum(&self) -> bool {
        DATUMS.contains(&self.guid)
    }

    /// Whether it is the datum its GUID names, as every family holds it (whichever plane defines the origin).
    pub fn as_datum(&self) -> bool {
        datums().into_iter().any(|d| d.guid == self.guid && RefPlane { defines_origin: d.defines_origin, id: 0, ..self.clone() } == d)
    }

    /// Its own axes in the family's.
    pub fn frame(&self) -> Frame {
        match self.guid {
            REF_LEVEL => return Frame::LEVEL,
            CENTER_FRONT_BACK => return Frame::FRONT_BACK,
            CENTER_LEFT_RIGHT => return Frame::LEFT_RIGHT,
            _ => {}
        }
        let x = unit(sub(self.end, self.start));
        let y = unit(sub(self.up, mul(x, dot(self.up, x))));
        let z = cross(x, y);
        Frame { origin: mul(z, dot(self.start, z)), x, y, z }
    }

    /// Which of the family's axes its normal lies along (0, 1, 2 for x, y, z), if one.
    fn direction(&self) -> Option<usize> {
        let z = self.frame().z;
        (0..3).find(|&k| z[k].abs() > 1.0 - 1e-9)
    }

    /// Whether it can be drawn: numbers within [`MAX_COORD`], a line with two ends, and `up` across it.
    fn check(&self, who: &str) -> Result<(), String> {
        if ![self.start, self.end, self.up].iter().all(usable) {
            return Err(format!("{who}: its start, end and up are numbers within {MAX_COORD} mm"));
        }
        let d = sub(self.end, self.start);
        if dot(d, d).sqrt() < ON_PLANE {
            return Err(format!("{who}: its start and end are one point; it needs a line to be drawn along"));
        }
        let across = cross(unit(d), unit(self.up));
        if dot(across, across).sqrt() < 1e-6 {
            return Err(format!("{who}: its `up` lies along its line; it needs a direction across it"));
        }
        if self.name.chars().count() > MAX_TEXT || self.name.contains(['\n', '\r']) {
            return Err(format!("{who}: a name is one line of up to {MAX_TEXT} characters"));
        }
        subcategory_ok(who, &self.subcategory)
    }
}

fn subcategory_ok(who: &str, s: &Option<String>) -> Result<(), String> {
    match s {
        Some(s) if s.trim().is_empty() || s.chars().count() > MAX_TEXT || s.contains(['\n', '\r']) => {
            Err(format!("{who}: a subcategory is a name of one line, up to {MAX_TEXT} characters"))
        }
        _ => Ok(()),
    }
}

impl RefLine {
    /// Its face `face`'s axes, the line drawn on `host`.
    fn frame(&self, face: Face, host: &Frame) -> Frame {
        let (d, n) = (unit(sub(self.end, self.start)), host.z);
        let side = cross(n, d);
        match face {
            Face::Horizontal => Frame { origin: self.start, x: d, y: side, z: n },
            Face::Vertical => Frame { origin: self.start, x: d, y: n, z: cross(d, n) },
            Face::Start => Frame { origin: self.start, x: side, y: n, z: d },
            Face::End => Frame { origin: self.end, x: side, y: n, z: d },
        }
    }
}

/// A family's reference planes and lines, borrowed: what the plane a [`PlaneRef`] names is worked out from, apart from the
/// family (the Family Editor keeps them beside the adaptive definition it edits). The datums are in every family, so planes
/// that list none still know them: `Planes::default()` has the three datums and nothing else.
#[derive(Clone, Copy, Debug, Default)]
pub struct Planes<'a> {
    pub planes: &'a [RefPlane],
    pub lines: &'a [RefLine],
}

/// What the Work Plane and Placement Plane lists call a datum, as Revit's do.
fn datum_label(guid: Guid) -> Option<&'static str> {
    match guid {
        REF_LEVEL => Some("Level : Ref. Level"),
        CENTER_FRONT_BACK => Some("Reference Plane : Center (Front/Back)"),
        CENTER_LEFT_RIGHT => Some("Reference Plane : Center (Left/Right)"),
        _ => None,
    }
}

impl<'a> Planes<'a> {
    /// The axes of the plane `host` names: a plane of the family's (a datum whether listed or not), or a face of one of its
    /// reference lines.
    pub fn frame(&self, host: &PlaneRef) -> Result<Frame, String> {
        match *host {
            PlaneRef::Plane(g) => match self.planes.iter().find(|p| p.guid == g) {
                Some(p) => Ok(p.frame()),
                None => {
                    datums().into_iter().find(|d| d.guid == g).map(|d| d.frame()).ok_or_else(|| format!("{host} isn't one of its planes"))
                }
            },
            PlaneRef::Line(g, face) => {
                let i = self.lines.iter().position(|l| l.guid == g).ok_or_else(|| format!("{host}: there's no such line of it"))?;
                self.line_frame(i, MAX_LINES).map(|(line, on)| line.frame(face, &on))
            }
        }
    }

    /// Line `i` and the axes of the plane it is drawn on, followed through the lines it is drawn on at most `depth` deep.
    fn line_frame(&self, i: usize, depth: usize) -> Result<(&'a RefLine, Frame), String> {
        let line = self.lines.get(i).ok_or("no such line")?;
        let on = match line.host {
            PlaneRef::Line(g, face) => {
                let j = self
                    .lines
                    .iter()
                    .position(|l| l.guid == g)
                    .ok_or_else(|| format!("line {}: {} isn't one of its lines", i + 1, line.host))?;
                if j >= i || depth == 0 {
                    return Err(format!("line {}: it is drawn on a line listed after it; a line is drawn on one before it", i + 1));
                }
                let (host, under) = self.line_frame(j, depth - 1)?;
                host.frame(face, &under)
            }
            PlaneRef::Plane(_) => self.frame(&line.host)?,
        };
        Ok((line, on))
    }

    /// The planes something may be drawn on, as Revit's Work Plane and Placement Plane list them: the datums first, named as
    /// Revit names them (`Level : Ref. Level`, `Reference Plane : Center (Front/Back)`, …), then each named plane of the
    /// family's own (`Reference Plane : name`; an unnamed one is not listed, as in Revit), then each reference line's four
    /// planes (`Reference Line 1 : Horizontal`). English: a front end translates them.
    pub fn listed(&self) -> Vec<(PlaneRef, String)> {
        let mut out: Vec<(PlaneRef, String)> =
            DATUMS.iter().filter_map(|g| datum_label(*g).map(|l| (PlaneRef::Plane(*g), l.to_string()))).collect();
        out.extend(
            self.planes
                .iter()
                .filter(|p| !p.datum() && !p.name.is_empty())
                .map(|p| (PlaneRef::Plane(p.guid), format!("Reference Plane : {}", p.name))),
        );
        for (i, l) in self.lines.iter().enumerate() {
            for (face, name) in [(Face::Horizontal, "Horizontal"), (Face::Vertical, "Vertical"), (Face::Start, "Start"), (Face::End, "End")]
            {
                out.push((PlaneRef::Line(l.guid, face), format!("Reference Line {} : {name}", i + 1)));
            }
        }
        out
    }
}

impl Family {
    /// Its planes and lines, borrowed, to work out the frame of a plane they name.
    pub fn plane_set(&self) -> Planes<'_> {
        Planes { planes: &self.planes, lines: &self.lines }
    }

    /// The axes of the plane `host` names: a plane of its own, or a face of one of its reference lines.
    pub fn frame(&self, host: &PlaneRef) -> Result<Frame, String> {
        self.plane_set().frame(host)
    }

    /// Line `i` and the axes of the plane it is drawn on, followed through the lines it is drawn on at most `depth` deep.
    fn line_frame(&self, i: usize, depth: usize) -> Result<(&RefLine, Frame), String> {
        self.plane_set().line_frame(i, depth)
    }

    /// Its datums first and as they are, and which plane defines the origin in each direction settled: a datum, unless a
    /// plane of its own takes it.
    pub fn settle_planes(&mut self) {
        let given = std::mem::take(&mut self.planes);
        self.planes = datums();
        self.planes.extend(given.into_iter().filter(|p| !p.datum()));
        settle_origin(&mut self.planes);
    }

    /// Its forms, lines and adaptive points drawn on the plane or line `guid`: `form 2`, `line 1`, ``adaptive point `p3` ``.
    pub fn hosted_on(&self, guid: Guid) -> Vec<String> {
        let forms = self.forms.iter().enumerate().filter(|(_, f)| f.host.guid() == guid).map(|(i, _)| format!("form {}", i + 1));
        let lines = self.lines.iter().enumerate().filter(|(_, l)| l.host.guid() == guid).map(|(i, _)| format!("line {}", i + 1));
        let points = self.adaptive.iter().flat_map(|d| d.drawn_on(guid)).map(|id| format!("adaptive point `{id}`"));
        forms.chain(lines).chain(points).collect()
    }

    /// What is drawn on a plane or a line given another GUID (`planes` and `lines`: from, to) drawn on it still.
    pub fn rehost(&mut self, planes: &[(Guid, Guid)], lines: &[(Guid, Guid)]) {
        if planes.is_empty() && lines.is_empty() {
            return;
        }
        let follow = |h: &mut PlaneRef| {
            let among = if matches!(h, PlaneRef::Plane(_)) { planes } else { lines };
            let g = h.guid_mut();
            if let Some((_, to)) = among.iter().find(|(from, _)| from == g) {
                *g = *to;
            }
        };
        self.forms.iter_mut().for_each(|f| follow(&mut f.host));
        self.lines.iter_mut().for_each(|l| follow(&mut l.host));
        self.adaptive.iter_mut().for_each(|d| d.work_planes_mut(&mut |h| follow(h)));
        self.resolve_dimension_refs(planes, &[]);
    }

    /// Each host given as an id (as a command may) given as the GUID of the plane or line of that id.
    pub fn resolve_hosts(&mut self) {
        let planes: Vec<(Id, Guid)> = self.planes.iter().map(|p| (p.id, p.guid)).collect();
        let lines: Vec<(Id, Guid)> = self.lines.iter().map(|l| (l.id, l.guid)).collect();
        let resolve = |h: &mut PlaneRef| {
            let among = if matches!(h, PlaneRef::Plane(_)) { &planes } else { &lines };
            let g = h.guid_mut();
            if let Some(id) = g.pending()
                && let Some((_, found)) = among.iter().find(|(i, found)| *i == id && !crate::ids::unset(i) && !found.is_nil())
            {
                *g = *found;
            }
        };
        self.forms.iter_mut().for_each(|f| resolve(&mut f.host));
        self.lines.iter_mut().for_each(|l| resolve(&mut l.host));
        self.adaptive.iter_mut().for_each(|d| d.work_planes_mut(&mut |h| resolve(h)));
        self.resolve_dimension_refs(&[], &[]);
    }

    /// Whether its planes and lines are sound: its datums first and pinned; each plane drawable, named once, a level only
    /// if Ref. Level; each named reference (Left through Top) taken once; the origin defined at most once in each direction,
    /// by a plane square to it; each line drawable and lying on its host.
    pub(super) fn check_planes(&self) -> Result<(), String> {
        for (what, n, most) in [("planes", self.planes.len(), MAX_PLANES), ("reference lines", self.lines.len(), MAX_LINES)] {
            if n > most {
                return Err(format!("it has more than {most} {what}"));
            }
        }
        let pinned = self.planes.len() >= 3 && self.planes[..3].iter().zip(DATUMS).all(|(p, g)| p.guid == g && p.as_datum());
        if !pinned {
            return Err("its first three planes are its datums, Ref. Level, Center (Front/Back) and Center (Left/Right), as they are: they are pinned".into());
        }
        let mut names = BTreeSet::new();
        let mut guids = BTreeSet::new();
        let mut taken: BTreeMap<Reference, String> = BTreeMap::new();
        let mut origin: [Option<String>; 3] = Default::default();
        for (i, p) in self.planes.iter().enumerate() {
            let who = if p.name.is_empty() { format!("plane {}", i + 1) } else { format!("plane `{}`", p.name) };
            if i >= 3 {
                if p.datum() {
                    return Err(format!("{who}: it holds a datum's GUID; give it its own"));
                }
                if p.level {
                    return Err(format!("{who}: only Ref. Level is a level; a plane of its own isn't"));
                }
                p.check(&who)?;
            }
            if !p.guid.is_nil() && !guids.insert(p.guid) {
                return Err(format!("{who}: another plane holds its GUID"));
            }
            if !p.name.is_empty() && !names.insert(p.name.as_str()) {
                return Err(format!("{who}: two planes are named `{}`; a name is one plane's", p.name));
            }
            if p.reference.named()
                && let Some(other) = taken.insert(p.reference, who.clone())
            {
                return Err(format!("{who}: {other} is {} already; each of Left through Top is one plane or line", json!(p.reference)));
            }
            if p.defines_origin {
                let Some(k) = p.direction() else {
                    return Err(format!("{who}: it defines the origin, which only a plane square to x, y or z can"));
                };
                if let Some(other) = origin[k].replace(who.clone()) {
                    return Err(format!("{who}: {other} defines the origin in its direction already; one plane does"));
                }
            }
        }
        for (i, l) in self.lines.iter().enumerate() {
            let who = format!("line {}", i + 1);
            if !usable(&l.start) || !usable(&l.end) {
                return Err(format!("{who}: its start and end are numbers within {MAX_COORD} mm"));
            }
            let d = sub(l.end, l.start);
            if dot(d, d).sqrt() < ON_PLANE {
                return Err(format!("{who}: its start and end are one point"));
            }
            subcategory_ok(&who, &l.subcategory)?;
            if !l.guid.is_nil() && !guids.insert(l.guid) {
                return Err(format!("{who}: another plane or line holds its GUID"));
            }
            if l.reference.named()
                && let Some(other) = taken.insert(l.reference, who.clone())
            {
                return Err(format!("{who}: {other} is {} already; each of Left through Top is one plane or line", json!(l.reference)));
            }
            let (_, on) = self.line_frame(i, MAX_LINES)?;
            let off = |p: P3| dot(sub(p, on.origin), on.z).abs() > ON_PLANE + 1e-9 * dot(p, p).sqrt();
            if off(l.start) || off(l.end) {
                return Err(format!("{who}: it doesn't lie on {}, which it is drawn on", l.host));
            }
        }
        // An adaptive point keeps the plane it was drawn on as its Work Plane: one of the family's.
        if let Some(d) = &self.adaptive {
            for (id, plane) in d.work_plane_refs() {
                self.frame(&plane)
                    .map_err(|e| format!("adaptive point `{id}`: its work plane, {e}; draw it on one of the family's planes"))?;
            }
        }
        Ok(())
    }
}
