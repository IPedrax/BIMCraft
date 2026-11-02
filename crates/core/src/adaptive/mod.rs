//! Adaptive family definitions and instance placements. No graph UI: every relationship is explicit data.
//! Frames are right-handed; coordinates are millimetres, rotations degrees. Failed nodes retain valid upstream results.
mod edit;
#[cfg(test)]
mod edit_tests;
pub use edit::{Picked, REACH, gizmo_end};
pub mod frame;
mod sample;
pub use sample::four_point_panel;
mod geometry;
#[cfg(test)]
mod revit_tests;
#[cfg(test)]
mod tests;

use crate::family::{Expr, Piece, Val};
use crate::view3d::{P3, add, dot, mul, sub};
pub use frame::Frame;
use frame::{EPS, norm, unit};
pub use geometry::Mesh;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_NODES: usize = 1024;
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    #[serde(default)]
    pub offset: [Expr; 3],
    #[serde(default)]
    pub rotation: [Expr; 3],
}
impl Transform {
    fn apply(&self, frame: Frame, values: &BTreeMap<String, Val>) -> Result<Frame, String> {
        frame.transformed(read3(&self.offset, values)?, read3(&self.rotation, values)?)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
    #[default]
    Z,
}
impl Axis {
    fn of(self, frame: Frame) -> P3 {
        match self {
            Self::X => frame.x,
            Self::Y => frame.y,
            Self::Z => frame.z,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Plane {
    #[default]
    Xy,
    Xz,
    Yz,
}
/// The datums a Generic Model Adaptive family starts with, as Revit's template has them: the level Ref. Level and the reference
/// planes Center (Front/Back) and Center (Left/Right), all through the family's origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Datum {
    Level,
    FrontBack,
    LeftRight,
}
impl Datum {
    pub const ALL: [Datum; 3] = [Datum::Level, Datum::FrontBack, Datum::LeftRight];
    /// The plane's axes at the origin, its Z square to it: Ref. Level the family's own; Center (Front/Back) seen from the front,
    /// Y up; Center (Left/Right) seen from the right, Y up.
    pub fn frame(self) -> Frame {
        match self {
            Datum::Level => Frame::default(),
            Datum::FrontBack => Frame { origin: [0.; 3], x: [1., 0., 0.], y: [0., 0., 1.], z: [0., -1., 0.] },
            Datum::LeftRight => Frame { origin: [0.; 3], x: [0., 1., 0.], y: [0., 0., 1.], z: [1., 0., 0.] },
        }
    }
    /// What Revit's Work Plane and Placement Plane call it.
    pub fn label(self) -> &'static str {
        match self {
            Datum::Level => "Level : Ref. Level",
            Datum::FrontBack => "Reference Plane : Center (Front/Back)",
            Datum::LeftRight => "Reference Plane : Center (Left/Right)",
        }
    }
}
/// Where a point or a line is drawn: a datum, or one of a point's three planes (Revit's work plane, its Placement Plane).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkPlane {
    Datum(Datum),
    Point { point: String, plane: Plane },
}
impl Default for WorkPlane {
    fn default() -> Self {
        WorkPlane::Datum(Datum::Level)
    }
}
impl WorkPlane {
    /// Its axes, as `evaluation` has them, its Z square to it; none for a point that did not evaluate.
    pub fn frame(&self, evaluation: &Evaluation) -> Option<Frame> {
        match self {
            WorkPlane::Datum(d) => Some(d.frame()),
            WorkPlane::Point { point, plane } => match evaluation.values.get(point.as_str()) {
                Some(Evaluated::Point { frame }) => Some(plane_of(*frame, *plane)),
                _ => None,
            },
        }
    }
}
/// Plane `plane` of a point at `f`, as a frame whose XY it is: XZ is (X, Z, -Y), YZ is (Y, Z, X).
pub fn plane_of(f: Frame, plane: Plane) -> Frame {
    match plane {
        Plane::Xy => f,
        Plane::Xz => Frame { x: f.x, y: f.z, z: mul(f.y, -1.), ..f },
        Plane::Yz => Frame { x: f.y, y: f.z, z: f.x, ..f },
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    InterpolateNormals,
    #[default]
    ProjectInterpolatedNormals,
    WorldVertical,
    ProjectWorldZ,
}
/// Where a point on a line is, as Revit's Measurement Type gives it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Measurement {
    /// The line's own parameter: mm along a straight line, 0 to the number of segments along a polyline, 0..1 on a Bezier.
    NonNormalizedCurveParameter,
    /// That parameter as a fraction, 0..1.
    #[default]
    #[serde(alias = "parameter")]
    NormalizedCurveParameter,
    /// mm along the line.
    #[serde(alias = "distance")]
    SegmentLength,
    /// The fraction of the line's length, 0..1.
    #[serde(alias = "normalized_length")]
    NormalizedSegmentLength,
    /// mm in a straight line from the end it is measured from.
    ChordLength,
    /// Degrees round an arc or a circle; none of this family's lines is one, so it is refused.
    Angle,
}
/// How an adaptive point's axes are taken once the family is placed: Revit's Orients to, as it was measured in Revit 2025
/// (docs/adaptive-revit-parity.md §1.4). The host is what the point was placed on; with none, the project.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrientsTo {
    /// The instance's axes: one direction for the whole instance. Revit's default.
    #[default]
    Instance,
    /// Z the instance's, X from the host's.
    InstanceZThenHost,
    /// The host's axes at the point.
    Host,
    /// The host's axes, flipped when the points are placed winding against the family's.
    HostAndLoopSystem,
    /// The project's axes.
    Global,
    /// Z straight up, X from the host's.
    GlobalZThenHost,
}
/// A point made adaptive in a file from before Orients to kept the frame it was placed on: the host's.
fn host_orients() -> OrientsTo {
    OrientsTo::Host
}
/// What a shape handle may be dragged along: Revit's Constrained, a plane or an axis of the point's own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Constrained {
    #[default]
    None,
    PlaneYz,
    PlaneZx,
    PlaneXy,
    AxisX,
    AxisY,
    AxisZ,
}
/// When a point's number or planes show: Revit's Never, When Selected, Always.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Show {
    Never,
    WhenSelected,
    Always,
}
/// Revit's Point: a reference point, or an adaptive one clicked when placing (placement) or dragged after (shape handle).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointType {
    Reference,
    Placement,
    ShapeHandle,
}
/// What a point shows, its own settings or Revit's defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointLook {
    /// Show Placement Number: Always by default on a placement point, Never on any other.
    pub number: Show,
    /// Show Reference Planes: Always by default on an adaptive point, Never on a reference point.
    pub planes: Show,
    /// Visible: whether the point shows when the family is in a project; No by default.
    pub visible: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurveLocation {
    #[serde(default)]
    pub measurement: Measurement,
    pub value: Expr,
    #[serde(default)]
    pub from_end: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Host {
    World {
        #[serde(default)]
        frame: Frame,
        /// The datum it was drawn on, its Work Plane; none from before work planes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        datum: Option<Datum>,
    },
    /// A placement point: clicked `number`th when the family is placed; `frame` is where the family has it.
    Adaptive {
        number: u32,
        #[serde(default)]
        frame: Frame,
        #[serde(default = "host_orients")]
        orients: OrientsTo,
    },
    /// A shape handle point: not clicked when placing, where the family has it in the instance's axes until it is dragged.
    Handle {
        #[serde(default)]
        frame: Frame,
        #[serde(default)]
        orients: OrientsTo,
        #[serde(default)]
        constrained: Constrained,
    },
    Frame {
        point: String,
    },
    Plane {
        point: String,
        #[serde(default)]
        plane: Plane,
    },
    Curve {
        curve: String,
        location: CurveLocation,
        #[serde(default)]
        orientation: Orientation,
    },
    Face {
        form: String,
        topology: String,
        face: usize,
        barycentric: [Expr; 3],
    },
    Edge {
        form: String,
        topology: String,
        edge: usize,
        parameter: Expr,
    },
    Vertex {
        form: String,
        topology: String,
        vertex: usize,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Point {
        host: Host,
        #[serde(default)]
        transform: Transform,
    },
    Line {
        a: String,
        b: String,
    },
    Polyline {
        points: Vec<String>,
        #[serde(default)]
        closed: bool,
    },
    Bezier {
        points: [String; 4],
    },
    Intersection {
        a: String,
        #[serde(default)]
        axis_a: Axis,
        b: String,
        #[serde(default)]
        axis_b: Axis,
        #[serde(default)]
        forward_only: bool,
    },
    Patch {
        chain: String,
    },
    Extrude {
        chain: String,
        distance: Expr,
        #[serde(default)]
        direction: Option<P3>,
    },
    Loft {
        profiles: Vec<String>,
    },
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Appearance {
    #[serde(default)]
    pub void: bool,
    #[serde(default)]
    pub material: String,
    /// A form: whether it shows (a formula; none, it shows). A point: Revit's Visible, whether it shows in a project.
    #[serde(default)]
    pub visible: Option<Expr>,
    /// A point's Show Placement Number; none, Revit's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_number: Option<Show>,
    /// A point's Show Reference Planes; none, Revit's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_planes: Option<Show>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub operation: Operation,
    #[serde(default)]
    pub appearance: Appearance,
}
impl Node {
    pub fn dependencies(&self) -> Vec<&str> {
        match &self.operation {
            Operation::Point { host, .. } => match host {
                Host::World { .. } | Host::Adaptive { .. } | Host::Handle { .. } => vec![],
                Host::Frame { point } | Host::Plane { point, .. } => vec![point],
                Host::Curve { curve, .. } => vec![curve],
                Host::Face { form, .. } | Host::Edge { form, .. } | Host::Vertex { form, .. } => vec![form],
            },
            Operation::Line { a, b } | Operation::Intersection { a, b, .. } => vec![a, b],
            Operation::Polyline { points, .. } => points.iter().map(String::as_str).collect(),
            Operation::Bezier { points } => points.iter().map(String::as_str).collect(),
            Operation::Patch { chain } | Operation::Extrude { chain, .. } => vec![chain],
            Operation::Loft { profiles } => profiles.iter().map(String::as_str).collect(),
        }
    }
    pub fn is_form(&self) -> bool {
        matches!(self.operation, Operation::Patch { .. } | Operation::Extrude { .. } | Operation::Loft { .. })
    }
    /// What this point shows, its own settings or Revit's defaults for its type.
    pub fn look(&self) -> PointLook {
        let (placement, adaptive) = match &self.operation {
            Operation::Point { host: Host::Adaptive { .. }, .. } => (true, true),
            Operation::Point { host: Host::Handle { .. }, .. } => (false, true),
            _ => (false, false),
        };
        let a = &self.appearance;
        PointLook {
            number: a.show_number.unwrap_or(if placement { Show::Always } else { Show::Never }),
            planes: a.show_planes.unwrap_or(if adaptive { Show::Always } else { Show::Never }),
            visible: matches!(a.visible, Some(Expr::Number(n)) if n != 0.),
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    #[serde(default, deserialize_with = "numbered")]
    pub frames: BTreeMap<u32, Frame>,
    /// Human-readable context provenance only; never substitutes for the captured frames.
    #[serde(default)]
    pub context: String,
    /// Persistent source references of a captured context snapshot (not live constraints).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", deserialize_with = "numbered")]
    pub bindings: BTreeMap<u32, ContextHost>,
    /// The shape handles dragged in this instance, by the point's id; the others stay where the family has them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub handles: BTreeMap<String, Frame>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextHost {
    pub component: crate::model::Guid,
    pub host: Host,
}
// Element's internally tagged serde representation buffers map keys as strings.
// Read those explicitly so modify/open and direct JSON deserialization agree.
fn numbered<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<BTreeMap<u32, T>, D::Error> {
    let values = BTreeMap::<String, T>::deserialize(deserializer)?;
    values.into_iter().map(|(key, value)| key.parse::<u32>().map(|n| (n, value)).map_err(serde::de::Error::custom)).collect()
}
impl Placement {
    /// Its frames, given in the project, in the axes of a component standing on `root` (mirrored or not); the rest kept.
    pub fn into_local(&self, root: Frame, mirrored: bool) -> Result<Self, String> {
        let frames = self.frames.iter().map(|(n, f)| f.into_local(root, mirrored).map(|f| (*n, f))).collect::<Result<_, String>>()?;
        let handles =
            self.handles.iter().map(|(id, f)| f.into_local(root, mirrored).map(|f| (id.clone(), f))).collect::<Result<_, String>>()?;
        Ok(Self { frames, handles, ..self.clone() })
    }
    pub fn check(&self) -> Result<(), String> {
        for (number, frame) in &self.frames {
            if *number == 0 {
                return Err("Placement numbers start at 1".into());
            }
            frame.checked().map_err(|e| format!("placement {number}: {e}"))?;
        }
        if self.handles.len() > MAX_NODES {
            return Err(format!("A placement has at most {MAX_NODES} shape handles"));
        }
        for (id, frame) in &self.handles {
            frame.checked().map_err(|e| format!("shape handle `{id}`: {e}"))?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    #[serde(default)]
    pub nodes: Vec<Node>,
    /// Editable test placement, independent from every placed component.
    #[serde(default)]
    pub preview: Placement,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub node: String,
    pub code: String,
    pub message: String,
    pub path: Vec<String>,
    pub warning: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub controls: Vec<Frame>,
    pub closed: bool,
    pub bezier: bool,
}
impl Curve {
    pub fn point(&self, t: f64) -> P3 {
        if self.bezier {
            let p = &self.controls;
            let u = 1. - t;
            return add(
                add(mul(p[0].origin, u * u * u), mul(p[1].origin, 3. * u * u * t)),
                add(mul(p[2].origin, 3. * u * t * t), mul(p[3].origin, t * t * t)),
            );
        }
        let segments = if self.closed { self.controls.len() } else { self.controls.len() - 1 };
        let q = (t.clamp(0., 1.) * segments as f64).min(segments as f64);
        let index = (q.floor() as usize).min(segments - 1);
        let f = q - index as f64;
        add(mul(self.controls[index].origin, 1. - f), mul(self.controls[(index + 1) % self.controls.len()].origin, f))
    }
    fn tangent(&self, t: f64) -> Result<P3, String> {
        if self.bezier {
            let p = &self.controls;
            let u = 1. - t;
            return unit(add(
                add(mul(sub(p[1].origin, p[0].origin), 3. * u * u), mul(sub(p[2].origin, p[1].origin), 6. * u * t)),
                mul(sub(p[3].origin, p[2].origin), 3. * t * t),
            ));
        }
        let segments = if self.closed { self.controls.len() } else { self.controls.len() - 1 };
        let i = ((t * segments as f64).floor() as usize).min(segments - 1);
        unit(sub(self.controls[(i + 1) % self.controls.len()].origin, self.controls[i].origin))
    }
    fn normals(&self, t: f64) -> Result<(P3, P3, bool), String> {
        let (a, b, f) = if self.bezier {
            (self.controls[0], self.controls[3], t)
        } else {
            let n = if self.closed { self.controls.len() } else { self.controls.len() - 1 };
            let q = t * n as f64;
            let i = (q.floor() as usize).min(n - 1);
            (self.controls[i], self.controls[(i + 1) % self.controls.len()], q - i as f64)
        };
        let (z, fallback) = frame::interpolate_normal(a.z, b.z, f)?;
        Ok((z, add(mul(a.x, 1. - f), mul(b.x, f)), fallback))
    }
    pub fn samples(&self) -> Vec<P3> {
        if self.bezier { (0..=64).map(|i| self.point(i as f64 / 64.)).collect() } else { self.controls.iter().map(|f| f.origin).collect() }
    }
    fn parameter(&self, location: &CurveLocation, values: &BTreeMap<String, Val>) -> Result<f64, String> {
        let value = read(&location.value, values)?;
        let flip = |t: f64| if location.from_end { 1. - t } else { t };
        match location.measurement {
            Measurement::NormalizedCurveParameter => {
                if !(0. ..=1.).contains(&value) {
                    return Err("Normalized curve parameter must be between 0 and 1".into());
                }
                return Ok(flip(value));
            }
            Measurement::NonNormalizedCurveParameter => {
                // A straight line's parameter runs along it in mm, a polyline's over its segments, a Bezier's 0..1.
                let domain = if self.bezier {
                    1.
                } else if self.controls.len() == 2 && !self.closed {
                    norm(sub(self.controls[1].origin, self.controls[0].origin))
                } else if self.closed {
                    self.controls.len() as f64
                } else {
                    (self.controls.len() - 1) as f64
                };
                if !(0. ..=domain).contains(&value) || domain < EPS {
                    return Err(format!("Curve parameter must be between 0 and {domain}"));
                }
                return Ok(flip(value / domain));
            }
            Measurement::ChordLength => {
                if value < 0. {
                    return Err("A chord length is not negative".into());
                }
                // The first place, from the end measured from, that far away in a straight line.
                let start = self.point(flip(0.));
                let chord = |t: f64| norm(sub(self.point(flip(t)), start));
                const STEPS: usize = 1024;
                let mut previous = 0.;
                for i in 1..=STEPS {
                    let t = i as f64 / STEPS as f64;
                    if chord(t) >= value - 1e-9 {
                        let (mut lo, mut hi) = (previous, t);
                        for _ in 0..60 {
                            let mid = 0.5 * (lo + hi);
                            if chord(mid) >= value { hi = mid } else { lo = mid }
                        }
                        return Ok(flip(hi));
                    }
                    previous = t;
                }
                return Err(format!("No place on the line is {value} mm from its end in a straight line"));
            }
            Measurement::Angle => return Err("An angle places a point on an arc or a circle, and this line is neither".into()),
            Measurement::SegmentLength | Measurement::NormalizedSegmentLength => {}
        }
        let n = if self.bezier {
            256
        } else if self.closed {
            self.controls.len()
        } else {
            self.controls.len() - 1
        };
        let mut lengths = vec![0.];
        for i in 1..=n {
            lengths.push(lengths[i - 1] + norm(sub(self.point(i as f64 / n as f64), self.point((i - 1) as f64 / n as f64))));
        }
        let total = lengths[n];
        if total < EPS {
            return Err("Curve length is zero".into());
        }
        let distance = if location.measurement == Measurement::NormalizedSegmentLength {
            if !(0. ..=1.).contains(&value) {
                return Err("Normalized arc length must be between 0 and 1".into());
            }
            value * total
        } else {
            value
        };
        if !(0. ..=total).contains(&distance) {
            return Err(format!("Distance {distance} mm is outside curve length {total} mm"));
        }
        let distance = if location.from_end { total - distance } else { distance };
        let i = (1..=n).find(|i| lengths[*i] >= distance).unwrap_or(n);
        let span = lengths[i] - lengths[i - 1];
        Ok(((i - 1) as f64 + if span < EPS { 0. } else { (distance - lengths[i - 1]) / span }) / n as f64)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Evaluated {
    Point { frame: Frame },
    Curve { curve: Curve },
    Form { mesh: Mesh, appearance: Appearance },
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Evaluation {
    /// Fully resolved type and instance parameters, sufficient for exact standalone replay.
    pub parameters: BTreeMap<String, Val>,
    pub values: BTreeMap<String, Evaluated>,
    pub order: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub placement: Placement,
    /// The instance's own axes: the rotation that best takes the family's placement points onto the placed ones, and where
    /// that puts the family's origin. Revit's "instance" in Orients to.
    #[serde(default)]
    pub instance: Frame,
    /// Which way the family's placement points wind and which way the placed ones do (each a Newell normal); none when there
    /// are fewer than three or they are in a line.
    #[serde(skip)]
    winding: Option<(P3, P3)>,
}
impl Evaluation {
    pub fn replay(&self, definition: &Definition) -> Evaluation {
        definition.evaluate(&self.parameters, &self.placement)
    }
    pub fn host_frame(&self, host: &Host) -> Result<Frame, String> {
        match host {
            Host::Frame { point: id } => point(self, id),
            Host::Face { form, topology, face, barycentric } => {
                mesh(self, form, topology)?.face_frame(*face, read3(barycentric, &self.parameters)?)
            }
            Host::Edge { form, topology, edge, parameter } => {
                mesh(self, form, topology)?.edge_frame(*edge, read(parameter, &self.parameters)?)
            }
            Host::Vertex { form, topology, vertex } => mesh(self, form, topology)?.vertex_frame(*vertex),
            _ => Err("Context capture requires an evaluated point or mesh subelement reference".into()),
        }
    }
    pub fn failed(&self) -> bool {
        self.diagnostics.iter().any(|d| !d.warning)
    }
    pub fn pieces(&self) -> Vec<Piece> {
        self.compose().0
    }
    fn compose(&self) -> (Vec<Piece>, Vec<Diagnostic>) {
        let mut out = Vec::<Piece>::new();
        let mut diagnostics = Vec::new();
        let mut meter = crate::csg::Meter::default();
        for id in &self.order {
            if let Some(Evaluated::Form { mesh, appearance }) = self.values.get(id) {
                if appearance.visible == Some(Expr::Number(0.)) {
                    continue;
                }
                if appearance.void {
                    if !mesh.closed {
                        diagnostics.push(Diagnostic {
                            node: id.clone(),
                            code: "composition".into(),
                            message: "A void requires a closed solid".into(),
                            path: vec![id.clone()],
                            warning: false,
                        });
                        continue;
                    }
                    // Commit each void atomically: preserve all preceding solids when subtraction fails.
                    let cuts = out
                        .iter()
                        .map(|piece| crate::csg::subtract(&piece.tris, &mesh.triangles(), &mut meter))
                        .collect::<Result<Vec<_>, _>>();
                    match cuts {
                        Ok(cuts) => {
                            for (piece, cut) in out.iter_mut().zip(cuts) {
                                piece.tris = cut;
                            }
                        }
                        Err(message) => diagnostics.push(Diagnostic {
                            node: id.clone(),
                            code: "composition".into(),
                            message,
                            path: vec![id.clone()],
                            warning: false,
                        }),
                    }
                } else {
                    out.push(mesh.piece(appearance.material.clone()));
                }
            }
        }
        (out, diagnostics)
    }
}
pub fn read(expr: &Expr, values: &BTreeMap<String, Val>) -> Result<f64, String> {
    let n = match expr {
        Expr::Number(n) => *n,
        Expr::Formula(s) if s.chars().count() > crate::family::MAX_FORMULA => {
            return Err("Adaptive formula exceeds the formula length budget".into());
        }
        Expr::Formula(s) => crate::family::eval(s, &mut |key| match values.get(key) {
            Some(Val::Number(n)) => Ok(*n),
            _ => Err(format!("Unknown numeric parameter `{key}`")),
        })?,
    };
    if !n.is_finite() || n.abs() > 1e9 { Err("Expression result is non-finite or out of range".into()) } else { Ok(n) }
}
fn read3(expr: &[Expr; 3], values: &BTreeMap<String, Val>) -> Result<P3, String> {
    Ok([read(&expr[0], values)?, read(&expr[1], values)?, read(&expr[2], values)?])
}
impl Definition {
    /// `placement` with a frame for every adaptive point: its own, else the one the point has in this definition.
    pub fn completed(&self, placement: &Placement) -> Placement {
        let mut out = placement.clone();
        for node in &self.nodes {
            if let Operation::Point { host: Host::Adaptive { number, frame, .. }, .. } = &node.operation {
                out.frames.entry(*number).or_insert(*frame);
            }
        }
        out
    }
    /// The placement points, in number order: where the family has each, and where `placement` puts it.
    fn placed(&self, placement: &Placement) -> Vec<(Frame, Frame)> {
        let mut points: Vec<(u32, Frame)> = self
            .nodes
            .iter()
            .filter_map(|n| match &n.operation {
                Operation::Point { host: Host::Adaptive { number, frame, .. }, .. } => Some((*number, *frame)),
                _ => None,
            })
            .collect();
        points.sort_by_key(|p| p.0);
        points.into_iter().map(|(n, f)| (f, placement.frames.get(&n).copied().unwrap_or(f))).collect()
    }
    pub fn check(&self) -> Result<(), String> {
        if self.nodes.len() > MAX_NODES {
            return Err(format!("Adaptive family exceeds {MAX_NODES} nodes"));
        }
        self.preview.check()?;
        let mut ids = BTreeSet::new();
        let mut numbers = BTreeSet::new();
        for node in &self.nodes {
            if node.id.is_empty() || node.id.len() > 128 || !ids.insert(node.id.as_str()) {
                return Err(format!("Adaptive node ID `{}` is empty, too long or duplicated", node.id));
            }
            match &node.operation {
                Operation::Point { host: Host::Adaptive { number, frame, .. }, .. } => {
                    if *number == 0 || !numbers.insert(*number) {
                        return Err("Adaptive numbers must be unique and start at 1".into());
                    }
                    frame.checked()?;
                }
                Operation::Point { host: Host::Handle { frame, .. }, .. } => {
                    frame.checked()?;
                }
                _ => {}
            }
        }
        for node in &self.nodes {
            for dependency in node.dependencies() {
                if !ids.contains(dependency) {
                    return Err(format!("Node `{}` references missing `{dependency}`", node.id));
                }
            }
        }
        fn visit<'a>(
            id: &'a str,
            nodes: &BTreeMap<&str, &'a Node>,
            path: &mut Vec<&'a str>,
            done: &mut BTreeSet<&'a str>,
        ) -> Result<(), String> {
            if done.contains(id) {
                return Ok(());
            }
            if path.len() >= 128 {
                return Err("Adaptive dependency depth exceeds 128".into());
            }
            if path.contains(&id) {
                return Err(format!("Adaptive dependency cycle: {} -> {id}", path.join(" -> ")));
            }
            path.push(id);
            if let Some(node) = nodes.get(id) {
                for dependency in node.dependencies() {
                    visit(dependency, nodes, path, done)?;
                }
            }
            path.pop();
            done.insert(id);
            Ok(())
        }
        let nodes = self.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let mut done = BTreeSet::new();
        for node in &self.nodes {
            visit(&node.id, &nodes, &mut Vec::new(), &mut done)?;
        }
        Ok(())
    }
    pub fn evaluate(&self, values: &BTreeMap<String, Val>, placement: &Placement) -> Evaluation {
        let mut report = Evaluation { placement: placement.clone(), parameters: values.clone(), ..Evaluation::default() };
        if let Err(message) =
            placement.check().and_then(|_| if self.nodes.len() > MAX_NODES { Err("Adaptive node budget exceeded".into()) } else { Ok(()) })
        {
            report.diagnostics.push(Diagnostic { node: String::new(), code: "definition".into(), message, path: vec![], warning: false });
            return report;
        }
        let placed = self.placed(placement);
        let from: Vec<P3> = placed.iter().map(|p| p.0.origin).collect();
        let to: Vec<P3> = placed.iter().map(|p| p.1.origin).collect();
        report.instance = frame::best_fit(&from, &to);
        report.winding = match (frame::newell(&from), frame::newell(&to)) {
            (Some(a), Some(b)) => Some((a, b)),
            _ => None,
        };
        let nodes = self.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let mut failed = BTreeSet::new();
        for node in &self.nodes {
            evaluate_node(&node.id, &nodes, values, placement, &mut report, &mut failed, &mut Vec::new());
        }
        report.diagnostics.extend(report.compose().1);
        report
    }
}
fn evaluate_node<'a>(
    id: &'a str,
    nodes: &BTreeMap<&str, &'a Node>,
    parameters: &BTreeMap<String, Val>,
    placement: &Placement,
    report: &mut Evaluation,
    failed: &mut BTreeSet<&'a str>,
    path: &mut Vec<String>,
) {
    if report.values.contains_key(id) || failed.contains(id) {
        return;
    }
    if path.len() >= 128 || path.iter().any(|p| p == id) || !nodes.contains_key(id) {
        failed.insert(id);
        report.diagnostics.push(Diagnostic {
            node: id.into(),
            code: "dependency".into(),
            message: if path.len() >= 128 {
                "Adaptive dependency depth exceeds 128".into()
            } else if nodes.contains_key(id) {
                "Dependency cycle".into()
            } else {
                format!("Missing dependency `{id}`")
            },
            path: path.clone(),
            warning: false,
        });
        return;
    }
    let Some(node) = nodes.get(id) else { return };
    path.push(id.to_string());
    for dependency in node.dependencies() {
        evaluate_node(dependency, nodes, parameters, placement, report, failed, path);
    }
    if let Some(blocker) = node.dependencies().into_iter().find(|id| failed.contains(id)) {
        failed.insert(id);
        report.diagnostics.push(Diagnostic {
            node: id.into(),
            code: "blocked".into(),
            message: format!("Blocked by `{blocker}`"),
            path: path.clone(),
            warning: false,
        });
        path.pop();
        return;
    }
    let warning_start = report.diagnostics.len();
    match evaluate_operation(node, parameters, placement, report) {
        Ok(value) => {
            report.values.insert(id.into(), value);
            report.order.push(id.into());
        }
        Err(message) => {
            failed.insert(id);
            report.diagnostics.truncate(warning_start);
            report.diagnostics.push(Diagnostic { node: id.into(), code: "evaluation".into(), message, path: path.clone(), warning: false });
        }
    }
    path.pop();
}
fn point(report: &Evaluation, id: &str) -> Result<Frame, String> {
    match report.values.get(id) {
        Some(Evaluated::Point { frame }) => Ok(*frame),
        _ => Err(format!("`{id}` is not an evaluated point")),
    }
}
fn curve(report: &Evaluation, id: &str) -> Result<Curve, String> {
    match report.values.get(id) {
        Some(Evaluated::Curve { curve }) => Ok(curve.clone()),
        _ => Err(format!("`{id}` is not an evaluated curve")),
    }
}
fn mesh<'a>(report: &'a Evaluation, id: &str, topology: &str) -> Result<&'a Mesh, String> {
    match report.values.get(id) {
        Some(Evaluated::Form { mesh, .. }) if mesh.topology() == topology => Ok(mesh),
        Some(Evaluated::Form { .. }) => Err(format!("`{id}` topology changed; select its subelement again")),
        _ => Err(format!("`{id}` is not an evaluated form")),
    }
}
fn warn(report: &mut Evaluation, node: &Node, message: &str) {
    report.diagnostics.push(Diagnostic {
        node: node.id.clone(),
        code: "orientation_fallback".into(),
        message: message.into(),
        path: vec![node.id.clone()],
        warning: true,
    });
}
/// An adaptive point's frame at `at` (where it was placed, with its host's axes), as `orients` takes its axes. `family` is
/// where the family has it, with its own axes there.
fn oriented(orients: OrientsTo, family: Frame, at: Frame, report: &Evaluation) -> Result<Frame, String> {
    let i = report.instance;
    let up = [0., 0., 1.];
    let x_of = |z: P3| if norm(frame::project(at.x, z)) < EPS { at.y } else { at.x };
    match orients {
        OrientsTo::Instance => Ok(Frame { origin: at.origin, x: i.vector(family.x), y: i.vector(family.y), z: i.vector(family.z) }),
        OrientsTo::InstanceZThenHost => {
            let z = i.vector(family.z);
            Frame::from_z(at.origin, z, x_of(z))
        }
        OrientsTo::Host => Ok(at),
        OrientsTo::HostAndLoopSystem => {
            // Flipped (Z reversed, X and Y swapped, still right-handed) when the placed points wind, seen along the host's Z,
            // the other way from the family's seen along the point's own.
            let against = report.winding.is_some_and(|(f, p)| dot(f, family.z) * dot(p, at.z) < 0.);
            Ok(if against { Frame { x: at.y, y: at.x, z: mul(at.z, -1.), ..at } } else { at })
        }
        OrientsTo::Global => Ok(Frame::at(at.origin)),
        OrientsTo::GlobalZThenHost => Frame::from_z(at.origin, up, x_of(up)),
    }
}
fn evaluate_operation(
    node: &Node,
    values: &BTreeMap<String, Val>,
    placement: &Placement,
    report: &mut Evaluation,
) -> Result<Evaluated, String> {
    let mut form = None;
    let result = match &node.operation {
        Operation::Point { host, transform } => {
            let base = match host {
                Host::World { frame, .. } => frame.checked()?,
                Host::Adaptive { number, frame, orients } => {
                    let at = placement.frames.get(number).copied().unwrap_or(*frame).checked()?;
                    oriented(*orients, *frame, at, report)?
                }
                Host::Handle { frame, orients, .. } => {
                    let i = report.instance;
                    let at = match placement.handles.get(&node.id) {
                        Some(f) => *f,
                        None => Frame { origin: i.point(frame.origin), x: i.vector(frame.x), y: i.vector(frame.y), z: i.vector(frame.z) },
                    };
                    oriented(*orients, *frame, at.checked()?, report)?
                }
                Host::Frame { point: id } => point(report, id)?,
                Host::Plane { point: id, plane } => plane_of(point(report, id)?, *plane),
                Host::Curve { curve: id, location, orientation } => {
                    let c = curve(report, id)?;
                    let t = c.parameter(location, values)?;
                    let origin = c.point(t);
                    let tangent = c.tangent(t)?;
                    let (normal, x_hint, antipodes) = c.normals(t)?;
                    if antipodes {
                        warn(report, node, "Opposite endpoint normals use a deterministic great-circle interpolation");
                    }
                    match orientation {
                        Orientation::InterpolateNormals => {
                            if norm(frame::project(x_hint, normal)) < EPS {
                                warn(
                                    report,
                                    node,
                                    "Endpoint X hints degenerate after projection; deterministic perpendicular fallback used",
                                );
                            }
                            Frame::from_z(origin, normal, x_hint)?
                        }
                        Orientation::ProjectInterpolatedNormals | Orientation::ProjectWorldZ => {
                            let (f, used) = Frame::from_tangent(
                                origin,
                                tangent,
                                if *orientation == Orientation::ProjectWorldZ { [0., 0., 1.] } else { normal },
                            )?;
                            if used {
                                warn(report, node, "Normal parallel to curve tangent; deterministic perpendicular fallback used");
                            }
                            f
                        }
                        Orientation::WorldVertical => {
                            if norm(frame::project(tangent, [0., 0., 1.])) < EPS {
                                warn(
                                    report,
                                    node,
                                    "Vertical tangent has no horizontal direction; deterministic perpendicular fallback used",
                                );
                            }
                            Frame::from_z(origin, [0., 0., 1.], tangent)?
                        }
                    }
                }
                Host::Face { form, topology, face, barycentric } => {
                    mesh(report, form, topology)?.face_frame(*face, read3(barycentric, values)?)?
                }
                Host::Edge { form, topology, edge, parameter } => {
                    mesh(report, form, topology)?.edge_frame(*edge, read(parameter, values)?)?
                }
                Host::Vertex { form, topology, vertex } => mesh(report, form, topology)?.vertex_frame(*vertex)?,
            };
            Evaluated::Point { frame: transform.apply(base, values)? }
        }
        Operation::Line { a, b } => {
            let controls = vec![point(report, a)?, point(report, b)?];
            if norm(sub(controls[1].origin, controls[0].origin)) < EPS {
                return Err("Line endpoints coincide".into());
            }
            Evaluated::Curve { curve: Curve { controls, closed: false, bezier: false } }
        }
        Operation::Polyline { points, closed } => {
            if points.len() < 2 || points.len() > 256 {
                return Err("Polyline needs 2 to 256 point references".into());
            }
            let controls = points.iter().map(|id| point(report, id)).collect::<Result<Vec<_>, _>>()?;
            for pair in controls.windows(2) {
                if norm(sub(pair[1].origin, pair[0].origin)) < EPS {
                    return Err("Polyline has a zero-length segment".into());
                }
            }
            if *closed && controls.len() < 3 {
                return Err("A closed polyline needs at least three points".into());
            }
            let closed = *closed;
            if closed && norm(sub(controls[0].origin, controls[controls.len() - 1].origin)) < EPS {
                return Err("Closed polyline repeats its first point".into());
            }
            Evaluated::Curve { curve: Curve { controls, closed, bezier: false } }
        }
        Operation::Bezier { points } => Evaluated::Curve {
            curve: Curve {
                controls: points.iter().map(|id| point(report, id)).collect::<Result<Vec<_>, _>>()?,
                closed: false,
                bezier: true,
            },
        },
        Operation::Intersection { a, axis_a, b, axis_b, forward_only } => {
            let a = point(report, a)?;
            let b = point(report, b)?;
            let u = axis_a.of(a);
            let v = axis_b.of(b);
            let w = sub(a.origin, b.origin);
            let uv = dot(u, v);
            let denominator = 1. - uv * uv;
            if denominator.abs() < EPS {
                return Err("Directions are parallel or coincident; no unique intersection".into());
            }
            let s = (uv * dot(v, w) - dot(u, w)) / denominator;
            let t = (dot(v, w) - uv * dot(u, w)) / denominator;
            let pa = add(a.origin, mul(u, s));
            let pb = add(b.origin, mul(v, t));
            if norm(sub(pa, pb)) > 1e-5 {
                return Err(format!("Directions are skew (separation {} mm)", norm(sub(pa, pb))));
            }
            if *forward_only && (s < 0. || t < 0.) {
                return Err("Intersection is behind a ray origin".into());
            }
            Evaluated::Point { frame: Frame { origin: mul(add(pa, pb), 0.5), ..a } }
        }
        Operation::Patch { chain } => {
            form = Some(geometry::patch(&curve(report, chain)?)?);
            Evaluated::Point { frame: Frame::default() }
        }
        Operation::Extrude { chain, distance, direction } => {
            form = Some(geometry::extrude(&curve(report, chain)?, read(distance, values)?, *direction)?);
            Evaluated::Point { frame: Frame::default() }
        }
        Operation::Loft { profiles } => {
            let curves = profiles.iter().map(|id| curve(report, id)).collect::<Result<Vec<_>, _>>()?;
            form = Some(geometry::loft(&curves)?);
            Evaluated::Point { frame: Frame::default() }
        }
    };
    if let Some(mesh) = form {
        let existing: usize = report
            .values
            .values()
            .map(|v| match v {
                Evaluated::Form { mesh, .. } => mesh.faces.len(),
                _ => 0,
            })
            .sum();
        if existing + mesh.faces.len() > crate::family::MAX_BUILT {
            return Err("Adaptive evaluation exceeds the triangle budget".into());
        }

        let mut appearance = node.appearance.clone();
        if let Some(expr) = &appearance.visible {
            appearance.visible = Some(Expr::Number(read(expr, values)?));
        }
        if let Some(Val::Text(material)) = values.get(&appearance.material) {
            appearance.material = material.clone();
        }
        return Ok(Evaluated::Form { mesh, appearance });
    }
    Ok(result)
}
