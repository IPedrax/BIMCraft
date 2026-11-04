//! Modelling an adaptive family as in Revit's family editor: reference points placed on a work plane or hosted on a line, made
//! adaptive in the order they are made so, lines drawn through points, forms made from profiles, points moved, elements
//! deleted with what depends on them, and what the pointer is over in a picture of the evaluation. Each edit changes the
//! canonical [`Definition`]; nothing here keeps state of its own.
use super::frame::norm;
use super::{
    Appearance, CurveLocation, Datum, Definition, Evaluated, Evaluation, Frame, Host, Measurement, Node, Operation, OrientsTo, Placement,
    Plane, PointType, Transform, WorkPlane, plane_of,
};
use crate::family::Expr;
use crate::view3d::{P3, add, dot, mul, sub};
use std::collections::BTreeSet;

/// A segment as it shows on screen, its two ends' pixels.
type OnScreen = ((f64, f64), (f64, f64));

/// What the pointer is over in a picture of an evaluation.
#[derive(Clone, Debug, PartialEq)]
pub enum Picked {
    Point(String),
    /// A curve, and the normalized parameter (as [`Measurement::NormalizedCurveParameter`] reads it) of the place on it nearest the pointer.
    Curve(String, f64),
    Form(String),
}

impl Picked {
    pub fn id(&self) -> &str {
        match self {
            Picked::Point(id) | Picked::Curve(id, _) | Picked::Form(id) => id,
        }
    }
}

/// How far, px, the pointer may be from a point or a line and still be over it.
pub const REACH: f64 = 8.0;

/// The nearest point on segment `a`–`b` to `p`, as the fraction of the way from `a`, and how far `p` is from it.
fn on_segment(a: (f64, f64), b: (f64, f64), p: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let f = if length < 1e-12 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length).clamp(0.0, 1.0) };
    let (x, y) = (a.0 + dx * f, a.1 + dy * f);
    (f, (p.0 - x).hypot(p.1 - y))
}

impl Evaluation {
    /// What shows at pixel `at` when `screen` gives where a model point shows and how near the camera it is (larger is
    /// nearer; `None` behind the eye): a point within [`REACH`] first, then a line, then the nearest form under it.
    pub fn pick(&self, screen: &dyn Fn(P3) -> Option<(f64, f64, f64)>, at: (f64, f64)) -> Option<Picked> {
        let xy = |p: P3| screen(p).map(|(x, y, _)| (x, y));
        let mut best: Option<(f64, Picked)> = None;
        for id in &self.order {
            if let Some(Evaluated::Point { frame }) = self.values.get(id)
                && let Some(s) = xy(frame.origin)
            {
                let d = (s.0 - at.0).hypot(s.1 - at.1);
                if d <= REACH && best.as_ref().is_none_or(|(b, _)| d < *b) {
                    best = Some((d, Picked::Point(id.clone())));
                }
            }
        }
        if best.is_some() {
            return best.map(|(_, p)| p);
        }
        for id in &self.order {
            if let Some(Evaluated::Curve { curve }) = self.values.get(id) {
                let mut samples = curve.samples();
                if curve.closed
                    && let Some(first) = samples.first().copied()
                {
                    samples.push(first);
                }
                let n = samples.len().saturating_sub(1);
                for (i, pair) in samples.windows(2).enumerate() {
                    let (Some(a), Some(b)) = (xy(pair[0]), xy(pair[1])) else { continue };
                    let (f, d) = on_segment(a, b, at);
                    if d <= REACH && best.as_ref().is_none_or(|(b, _)| d < *b) {
                        best = Some((d, Picked::Curve(id.clone(), ((i as f64 + f) / n.max(1) as f64).clamp(0.0, 1.0))));
                    }
                }
            }
        }
        if best.is_some() {
            return best.map(|(_, p)| p);
        }
        // A form under the pointer: the triangle nearest the camera there.
        let mut nearest: Option<(f64, String)> = None;
        for id in &self.order {
            if let Some(Evaluated::Form { mesh, .. }) = self.values.get(id) {
                for t in mesh.triangles() {
                    let (Some(a), Some(b), Some(c)) = (screen(t[0]), screen(t[1]), screen(t[2])) else { continue };
                    let side = |p: (f64, f64, f64), q: (f64, f64, f64)| (q.0 - p.0) * (at.1 - p.1) - (q.1 - p.1) * (at.0 - p.0);
                    let (s1, s2, s3) = (side(a, b), side(b, c), side(c, a));
                    let inside = (s1 >= 0. && s2 >= 0. && s3 >= 0.) || (s1 <= 0. && s2 <= 0. && s3 <= 0.);
                    let z = (a.2 + b.2 + c.2) / 3.;
                    if inside && nearest.as_ref().is_none_or(|(n, _)| z > *n) {
                        nearest = Some((z, id.clone()));
                    }
                }
            }
        }
        nearest.map(|(_, id)| Picked::Form(id))
    }

    /// The work plane under the pointer, for Placement Plane's Pick: of each point's three planes (squares `half` mm from it
    /// each way), the one nearest the eye where the pointer's ray meets it; with none under it, of the datums (as far as box
    /// `room` reaches), the nearest. A point's planes come first, as in Revit: the datums fill the room, so one in front of a
    /// point would hide them. `hit` gives where the ray meets the plane through a point square to a direction, `depth` how
    /// near the eye a point is (larger is nearer).
    pub fn pick_plane(
        &self,
        hit: &dyn Fn(P3, P3) -> Option<P3>,
        depth: &dyn Fn(P3) -> f64,
        half: f64,
        room: (P3, P3),
    ) -> Option<WorkPlane> {
        let mut best: Option<(f64, WorkPlane)> = None;
        let consider = |best: &mut Option<(f64, WorkPlane)>, p: P3, plane: WorkPlane| {
            let z = depth(p);
            if z.is_finite() && best.as_ref().is_none_or(|(b, _)| z > *b + 1e-6) {
                *best = Some((z, plane));
            }
        };
        for id in &self.order {
            if let Some(Evaluated::Point { frame }) = self.values.get(id) {
                for plane in [Plane::Xy, Plane::Xz, Plane::Yz] {
                    let f = plane_of(*frame, plane);
                    if let Some(p) = hit(f.origin, f.z) {
                        let local = f.inverse_point(p);
                        if local[0].abs() <= half && local[1].abs() <= half {
                            consider(&mut best, p, WorkPlane::Point { point: id.clone(), plane });
                        }
                    }
                }
            }
        }
        if best.is_some() {
            return best.map(|(_, plane)| plane);
        }
        let (lo, hi) = room;
        for datum in Datum::ALL {
            let f = datum.frame();
            if let Some(p) = hit(f.origin, f.z)
                && (0..3).all(|i| p[i] >= lo[i] - 1e-6 && p[i] <= hi[i] + 1e-6)
            {
                consider(&mut best, p, WorkPlane::Datum(datum));
            }
        }
        best.map(|(_, plane)| plane)
    }

    /// What a selection box from pixel `a` to pixel `b` takes, as Revit's: dragged left to right (a window), what lies wholly
    /// inside it; right to left (a crossing), what it touches. In the order of the evaluation; `screen` as for [`Evaluation::pick`].
    pub fn in_box(&self, screen: &dyn Fn(P3) -> Option<(f64, f64, f64)>, a: (f64, f64), b: (f64, f64)) -> Vec<String> {
        let crossing = b.0 < a.0;
        let (x0, x1, y0, y1) = (a.0.min(b.0), a.0.max(b.0), a.1.min(b.1), a.1.max(b.1));
        let inside = |p: (f64, f64)| p.0 >= x0 && p.0 <= x1 && p.1 >= y0 && p.1 <= y1;
        // Whether segment p-q crosses the box: clipped to it, something is left (Liang-Barsky).
        let crosses = |p: (f64, f64), q: (f64, f64)| {
            let (dx, dy) = (q.0 - p.0, q.1 - p.1);
            let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
            for (den, num) in [(-dx, p.0 - x0), (dx, x1 - p.0), (-dy, p.1 - y0), (dy, y1 - p.1)] {
                if den.abs() < 1e-12 {
                    if num < 0. {
                        return false;
                    }
                } else {
                    let t = num / den;
                    if den < 0. { lo = lo.max(t) } else { hi = hi.min(t) }
                }
            }
            lo <= hi
        };
        let mut out = Vec::new();
        for id in &self.order {
            // Each element as the pieces it is drawn with: a point, a line's run of samples, a form's triangles' edges.
            let segments: Vec<(P3, P3)> = match self.values.get(id) {
                Some(Evaluated::Point { frame }) => vec![(frame.origin, frame.origin)],
                Some(Evaluated::Curve { curve }) => {
                    let mut samples = curve.samples();
                    if curve.closed
                        && let Some(first) = samples.first().copied()
                    {
                        samples.push(first);
                    }
                    samples.windows(2).map(|w| (w[0], w[1])).collect()
                }
                Some(Evaluated::Form { mesh, .. }) => {
                    mesh.triangles().iter().flat_map(|t| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])]).collect()
                }
                None => continue,
            };
            let on_screen: Option<Vec<OnScreen>> =
                segments.iter().map(|(p, q)| Some((screen(*p).map(|s| (s.0, s.1))?, screen(*q).map(|s| (s.0, s.1))?))).collect();
            let Some(on_screen) = on_screen.filter(|v| !v.is_empty()) else { continue };
            let taken = if crossing {
                on_screen.iter().any(|(p, q)| inside(*p) || inside(*q) || crosses(*p, *q))
            } else {
                on_screen.iter().all(|(p, q)| inside(*p) && inside(*q))
            };
            if taken {
                out.push(id.clone());
            }
        }
        out
    }

    /// Where point `id` is, evaluated.
    pub fn origin(&self, id: &str) -> Option<P3> {
        match self.values.get(id) {
            Some(Evaluated::Point { frame }) => Some(frame.origin),
            _ => None,
        }
    }
}

impl Definition {
    /// An id no node has: `stem` and the first free number.
    pub fn fresh_id(&self, stem: &str) -> String {
        let taken: BTreeSet<&str> = self.nodes.iter().map(|n| n.id.as_str()).collect();
        (1..).map(|n| format!("{stem}{n}")).find(|id| !taken.contains(id.as_str())).unwrap_or_else(|| stem.to_string())
    }

    fn push(&mut self, stem: &str, name: &str, operation: Operation) -> String {
        let id = self.fresh_id(stem);
        self.nodes.push(Node { id: id.clone(), name: name.into(), operation, appearance: Appearance::default() });
        id
    }

    /// A reference point at `at`, its axes the family's (Revit's Point Element on Ref. Level, its default work plane, for a
    /// point at height 0); its id.
    pub fn add_point(&mut self, at: P3) -> String {
        let datum = (at[2] == 0.).then_some(Datum::Level);
        let host = Host::World { frame: Frame::at(at), datum };
        self.push("point", "Reference point", Operation::Point { host, transform: Transform::default() })
    }

    /// A reference point drawn on work plane `plane` where `at` falls square onto it, as `evaluation` has the plane: on a
    /// datum, with the datum's axes; on a point's plane, hosted on it (so it follows that point), at an offset in it. Its id.
    pub fn add_point_on(&mut self, plane: &WorkPlane, at: P3, evaluation: &Evaluation) -> Result<String, String> {
        if !at.iter().all(|v| v.is_finite() && v.abs() <= 1e9) {
            return Err("The point would be out of range".into());
        }
        match plane {
            WorkPlane::Datum(datum) => {
                let f = datum.frame();
                let local = f.inverse_point(at);
                let frame = Frame { origin: f.point([local[0], local[1], 0.]), ..f };
                let host = Host::World { frame, datum: Some(*datum) };
                Ok(self.push("point", "Reference point", Operation::Point { host, transform: Transform::default() }))
            }
            WorkPlane::Point { point, plane } => {
                let Some(Evaluated::Point { frame }) = evaluation.values.get(point.as_str()) else {
                    return Err(format!("`{point}` is not a point: its planes are no work plane"));
                };
                let local = plane_of(*frame, *plane).inverse_point(at);
                let offset = [Expr::Number(local[0]), Expr::Number(local[1]), Expr::Number(0.)];
                let host = Host::Plane { point: point.clone(), plane: *plane };
                Ok(self.push("point", "Hosted point", Operation::Point { host, transform: Transform { offset, ..Transform::default() } }))
            }
        }
    }

    /// The work plane point `id` was drawn on, Revit's Work Plane, if it has one.
    pub fn work_plane(&self, id: &str) -> Option<WorkPlane> {
        match &self.nodes.iter().find(|n| n.id == id)?.operation {
            Operation::Point { host: Host::World { datum: Some(d), .. }, .. } => Some(WorkPlane::Datum(*d)),
            Operation::Point { host: Host::Plane { point, plane }, .. } => Some(WorkPlane::Point { point: point.clone(), plane: *plane }),
            _ => None,
        }
    }

    /// A reference point hosted on curve `curve` at normalized parameter `t`, oriented along it; its id.
    pub fn add_hosted_point(&mut self, curve: &str, t: f64) -> Result<String, String> {
        if !self
            .nodes
            .iter()
            .any(|n| n.id == curve && matches!(n.operation, Operation::Line { .. } | Operation::Polyline { .. } | Operation::Bezier { .. }))
        {
            return Err(format!("`{curve}` is not a line"));
        }
        let location =
            CurveLocation { measurement: Measurement::NormalizedCurveParameter, value: Expr::Number(t.clamp(0., 1.)), from_end: false };
        let host = Host::Curve { curve: curve.into(), location, orientation: Default::default() };
        Ok(self.push("point", "Hosted point", Operation::Point { host, transform: Transform::default() }))
    }

    /// The next adaptive point's number: one more than the highest.
    pub fn next_number(&self) -> u32 {
        self.nodes
            .iter()
            .filter_map(|n| match &n.operation {
                Operation::Point { host: Host::Adaptive { number, .. }, .. } => Some(*number),
                _ => None,
            })
            .max()
            .unwrap_or(0)
            + 1
    }

    /// Point `id` made a placement point, numbered next as Revit numbers them: in the order they are made adaptive, and
    /// oriented to the instance, Revit's default. A point on the work plane or a shape handle keeps where it is; one already a
    /// placement point keeps its number. Its number.
    pub fn make_adaptive(&mut self, id: &str) -> Result<u32, String> {
        let number = self.next_number();
        let node = self.nodes.iter_mut().find(|n| n.id == id).ok_or_else(|| format!("No element `{id}`"))?;
        let Operation::Point { host, .. } = &mut node.operation else { return Err(format!("`{id}` is not a point")) };
        match *host {
            Host::Adaptive { number, .. } => Ok(number),
            Host::World { frame, .. } | Host::Handle { frame, .. } => {
                let orients = match *host {
                    Host::Handle { orients, .. } => orients,
                    _ => OrientsTo::Instance,
                };
                *host = Host::Adaptive { number, frame, orients };
                node.name = format!("Adaptive point {number}");
                Ok(number)
            }
            _ => Err(format!("`{id}` is hosted: only a point on the work plane can be made adaptive")),
        }
    }

    /// An adaptive point (placement or shape handle) made a reference point on the work plane again, where it is; the points
    /// numbered after it close up.
    pub fn make_reference(&mut self, id: &str) -> Result<(), String> {
        let number = self.unnumber(id)?;
        let node = self.nodes.iter_mut().find(|n| n.id == id).ok_or_else(|| format!("No element `{id}`"))?;
        let Operation::Point { host, .. } = &mut node.operation else { return Err(format!("`{id}` is not a point")) };
        if let Host::Adaptive { frame, .. } | Host::Handle { frame, .. } = *host {
            *host = Host::World { frame, datum: None };
            node.name = "Reference point".into();
        }
        if number.is_none() {
            self.preview.handles.remove(id);
        }
        Ok(())
    }

    /// Point `id` made a shape handle (Revit's Shape Handle Point): no number, the placement points after it closing up.
    fn make_handle(&mut self, id: &str) -> Result<(), String> {
        self.unnumber(id)?;
        let node = self.nodes.iter_mut().find(|n| n.id == id).ok_or_else(|| format!("No element `{id}`"))?;
        let Operation::Point { host, .. } = &mut node.operation else { return Err(format!("`{id}` is not a point")) };
        match *host {
            Host::Handle { .. } => Ok(()),
            Host::World { frame, .. } | Host::Adaptive { frame, .. } => {
                let orients = match *host {
                    Host::Adaptive { orients, .. } => orients,
                    _ => OrientsTo::Instance,
                };
                *host = Host::Handle { frame, orients, constrained: Default::default() };
                node.name = "Shape handle point".into();
                Ok(())
            }
            _ => Err(format!("`{id}` is hosted: only a point on the work plane can be made adaptive")),
        }
    }

    /// Takes point `id`'s number away, if it is a placement point, closing up the numbers after it and the test placement's
    /// frames; the number it had.
    fn unnumber(&mut self, id: &str) -> Result<Option<u32>, String> {
        let node = self.nodes.iter().find(|n| n.id == id).ok_or_else(|| format!("No element `{id}`"))?;
        let Operation::Point { host: Host::Adaptive { number, .. }, .. } = node.operation else { return Ok(None) };
        for n in &mut self.nodes {
            if let Operation::Point { host: Host::Adaptive { number: k, .. }, .. } = &mut n.operation
                && *k > number
            {
                *k -= 1;
            }
        }
        self.preview.frames = std::mem::take(&mut self.preview.frames)
            .into_iter()
            .filter(|(k, _)| *k != number)
            .map(|(k, f)| (if k > number { k - 1 } else { k }, f))
            .collect();
        Ok(Some(number))
    }

    /// What kind of point `id` is, Revit's Point; none if it is not a point.
    pub fn point_type(&self, id: &str) -> Option<PointType> {
        match &self.nodes.iter().find(|n| n.id == id)?.operation {
            Operation::Point { host: Host::Adaptive { .. }, .. } => Some(PointType::Placement),
            Operation::Point { host: Host::Handle { .. }, .. } => Some(PointType::ShapeHandle),
            Operation::Point { .. } => Some(PointType::Reference),
            _ => None,
        }
    }

    /// Point `id`'s number, if it is a placement point.
    pub fn number(&self, id: &str) -> Option<u32> {
        match &self.nodes.iter().find(|n| n.id == id)?.operation {
            Operation::Point { host: Host::Adaptive { number, .. }, .. } => Some(*number),
            _ => None,
        }
    }

    /// Point `id` made a reference point, a placement point or a shape handle, as Revit's Point does.
    pub fn set_point_type(&mut self, id: &str, kind: PointType) -> Result<(), String> {
        match kind {
            PointType::Reference => self.make_reference(id),
            PointType::Placement => self.make_adaptive(id).map(|_| ()),
            PointType::ShapeHandle => self.make_handle(id),
        }
    }

    /// Placement point `id` numbered `number`, as typing it in Revit does: the point that had it takes `id`'s, the two
    /// swapping (with their frames in the test placement). Numbers run 1 to the number of placement points.
    pub fn set_number(&mut self, id: &str, number: u32) -> Result<(), String> {
        let from = self.number(id).ok_or_else(|| format!("`{id}` is not a placement point"))?;
        let count = self.next_number() - 1;
        if !(1..=count).contains(&number) {
            return Err(format!("Placement numbers run 1 to {count}"));
        }
        for n in &mut self.nodes {
            if let Operation::Point { host: Host::Adaptive { number: k, .. }, .. } = &mut n.operation {
                if n.id == id {
                    *k = number;
                } else if *k == number {
                    *k = from;
                }
            }
        }
        let (a, b) = (self.preview.frames.remove(&from), self.preview.frames.remove(&number));
        if let Some(f) = a {
            self.preview.frames.insert(number, f);
        }
        if let Some(f) = b {
            self.preview.frames.insert(from, f);
        }
        Ok(())
    }

    /// A line through `points` in order (two make a line, more a polyline, `closed` back to the first); its id.
    pub fn add_chain(&mut self, points: &[String], closed: bool) -> Result<String, String> {
        for p in points {
            if !self.nodes.iter().any(|n| &n.id == p && matches!(n.operation, Operation::Point { .. })) {
                return Err(format!("`{p}` is not a point"));
            }
        }
        match (points, closed) {
            ([a, b], false) => Ok(self.push("line", "Reference line", Operation::Line { a: a.clone(), b: b.clone() })),
            (_, _) if points.len() >= 2 && (!closed || points.len() >= 3) => {
                let name = if closed { "Closed profile" } else { "Reference line" };
                Ok(self.push("line", name, Operation::Polyline { points: points.to_vec(), closed }))
            }
            _ => Err("A line needs two points, a closed profile three".into()),
        }
    }

    /// Revit's Create Form from the profiles selected: one closed profile extruded `depth` mm along its normal, two or more
    /// lofted in the order selected. Its id.
    pub fn create_form(&mut self, profiles: &[String], depth: f64, void: bool) -> Result<String, String> {
        let closed = |id: &String| {
            self.nodes.iter().find(|n| &n.id == id).map(|n| match &n.operation {
                Operation::Polyline { closed, .. } => Some(*closed),
                Operation::Line { .. } | Operation::Bezier { .. } => Some(false),
                _ => None,
            })
        };
        let kinds =
            profiles.iter().map(|p| closed(p).flatten()).collect::<Option<Vec<bool>>>().ok_or("Select lines or profiles to make a form")?;
        let operation = match kinds.as_slice() {
            [true] => Operation::Extrude { chain: profiles[0].clone(), distance: Expr::Number(depth), direction: None },
            [false] => return Err("One open line makes no form: select a closed profile, or two or more profiles".into()),
            [] => return Err("Select a closed profile, or two or more profiles".into()),
            _ if kinds.iter().all(|k| *k == kinds[0]) => Operation::Loft { profiles: profiles.to_vec() },
            _ => return Err("Profiles of a loft are all closed or all open".into()),
        };
        let id = self.push("form", if void { "Void form" } else { "Solid form" }, operation);
        if let Some(n) = self.nodes.last_mut() {
            n.appearance.void = void;
        }
        Ok(id)
    }

    /// Point `id` moved so it evaluates at `to`, as `evaluation` found it: on the work plane or adaptive, its frame (the test
    /// placement's, when that has one of its own, for an adaptive point); hosted on a point, its offset in the host's axes;
    /// on a line, the parameter of the place on it nearest `to`.
    pub fn move_point(&mut self, id: &str, evaluation: &Evaluation, to: P3) -> Result<(), String> {
        let from = evaluation.origin(id).ok_or_else(|| format!("`{id}` is not an evaluated point"))?;
        let by = sub(to, from);
        if !to.iter().all(|v| v.is_finite() && v.abs() <= 1e9) {
            return Err("The point would be out of range".into());
        }
        let node = self.nodes.iter_mut().find(|n| n.id == id).ok_or_else(|| format!("No element `{id}`"))?;
        let Operation::Point { host, transform } = &mut node.operation else { return Err(format!("`{id}` is not a point")) };
        match host {
            Host::World { frame, .. } => frame.origin = add(frame.origin, by),
            Host::Adaptive { number, frame, .. } => match self.preview.frames.get_mut(number) {
                Some(f) => f.origin = add(f.origin, by),
                None => frame.origin = add(frame.origin, by),
            },
            Host::Handle { frame, .. } => match self.preview.handles.get_mut(&node.id) {
                Some(f) => f.origin = add(f.origin, by),
                None => frame.origin = add(frame.origin, by),
            },
            Host::Frame { point } | Host::Plane { point, .. } => {
                let host = evaluation.values.get(point.as_str()).and_then(|v| match v {
                    Evaluated::Point { frame } => Some(*frame),
                    _ => None,
                });
                let host = host.ok_or("The host point did not evaluate")?;
                let local = [dot(by, host.x), dot(by, host.y), dot(by, host.z)];
                for (o, d) in transform.offset.iter_mut().zip(local) {
                    match o {
                        Expr::Number(n) => *n += d,
                        Expr::Formula(_) => return Err("An offset driven by a formula moves with its parameter".into()),
                    }
                }
            }
            Host::Curve { curve, location, .. } => {
                let Some(Evaluated::Curve { curve: c }) = evaluation.values.get(curve.as_str()) else {
                    return Err("The host line did not evaluate".into());
                };
                // The nearest of a fine run of places along it.
                let t = (0..=512)
                    .map(|i| i as f64 / 512.)
                    .min_by(|a, b| norm(sub(c.point(*a), to)).total_cmp(&norm(sub(c.point(*b), to))))
                    .unwrap_or(0.);
                *location = CurveLocation { measurement: Measurement::NormalizedCurveParameter, value: Expr::Number(t), from_end: false };
            }
            _ => return Err("A point on a form's face, edge or vertex moves with the form".into()),
        }
        Ok(())
    }

    /// `ids` and everything they are built on (a line's points, a form's profile and its points, not a point's host): what Move,
    /// Copy and Rotate take.
    fn closure(&self, ids: &[String]) -> BTreeSet<String> {
        let mut out: BTreeSet<String> = BTreeSet::new();
        let mut todo: Vec<String> = ids.to_vec();
        while let Some(id) = todo.pop() {
            if let Some(n) = self.nodes.iter().find(|n| n.id == id)
                && out.insert(id)
                // A point is not built on its host: moved alone, it moves within it.
                && !matches!(n.operation, Operation::Point { .. })
            {
                todo.extend(n.dependencies().into_iter().map(String::from));
            }
        }
        out
    }

    /// The points of `taken` that move themselves when it moves: those whose host is not taken too (a point hosted on a taken
    /// point comes with it) and that are not on a line or a form (they come with those).
    fn free_points<'a>(&'a self, taken: &'a BTreeSet<String>) -> impl Iterator<Item = &'a Node> + 'a {
        self.nodes.iter().filter(move |n| {
            taken.contains(&n.id)
                && match &n.operation {
                    Operation::Point { host, .. } => {
                        !matches!(host, Host::Curve { .. } | Host::Face { .. } | Host::Edge { .. } | Host::Vertex { .. })
                            && !n.dependencies().iter().any(|d| taken.contains(*d))
                    }
                    _ => false,
                }
        })
    }

    /// Revit's Move: `ids`, with what they are built on, moved `by` mm, as `evaluation` has them. A point hosted on another moves
    /// within its host (its offset changes); one on a line or a form moves with it.
    pub fn move_elements(&mut self, ids: &[String], by: P3, evaluation: &Evaluation) -> Result<(), String> {
        let taken = self.closure(ids);
        let moving: Vec<String> = self.free_points(&taken).map(|n| n.id.clone()).collect();
        for id in moving {
            let from = evaluation.origin(&id).ok_or_else(|| format!("`{id}` did not evaluate"))?;
            self.move_point(&id, evaluation, add(from, by))?;
        }
        Ok(())
    }

    /// Revit's Rotate: `ids`, with what they are built on, turned `degrees` about the axis through `centre` along `axis` (the
    /// work plane's normal), counterclockwise seen from where the axis points.
    pub fn rotate_elements(&mut self, ids: &[String], centre: P3, axis: P3, degrees: f64, evaluation: &Evaluation) -> Result<(), String> {
        let k = super::frame::unit(axis)?;
        let (sin, cos) = degrees.to_radians().sin_cos();
        let taken = self.closure(ids);
        let moving: Vec<String> = self.free_points(&taken).map(|n| n.id.clone()).collect();
        for id in moving {
            let from = evaluation.origin(&id).ok_or_else(|| format!("`{id}` did not evaluate"))?;
            // Rodrigues: v cos + (k x v) sin + k (k . v)(1 - cos), about the centre.
            let v = sub(from, centre);
            let kv = crate::view3d::cross(k, v);
            let turned = add(add(mul(v, cos), mul(kv, sin)), mul(k, dot(k, v) * (1. - cos)));
            self.move_point(&id, evaluation, add(centre, turned))?;
        }
        Ok(())
    }

    /// Revit's Copy: `ids`, with what they are built on, copied and the copies moved `by` mm; an adaptive point copied is a new
    /// placement point, numbered next. The copies' ids, in the order of the originals.
    pub fn copy_elements(&mut self, ids: &[String], by: P3, evaluation: &Evaluation) -> Result<Vec<String>, String> {
        if self.nodes.len() + ids.len() > super::MAX_NODES {
            return Err(format!("Adaptive family exceeds {} nodes", super::MAX_NODES));
        }
        let taken = self.closure(ids);
        let originals: Vec<Node> = self.nodes.iter().filter(|n| taken.contains(&n.id)).cloned().collect();
        if self.nodes.len() + originals.len() > super::MAX_NODES {
            return Err(format!("Adaptive family exceeds {} nodes", super::MAX_NODES));
        }
        // New ids first, so references between the copies point at the copies.
        let mut names = std::collections::BTreeMap::new();
        for n in &originals {
            let stem: String = n.id.trim_end_matches(|c: char| c.is_ascii_digit()).to_string();
            let fresh = self.fresh_id(if stem.is_empty() { "copy" } else { &stem });
            self.nodes.push(Node {
                id: fresh.clone(),
                name: String::new(),
                operation: Operation::Line { a: String::new(), b: String::new() },
                appearance: Appearance::default(),
            });
            names.insert(n.id.clone(), fresh);
        }
        let rename = |id: &mut String| {
            if let Some(new) = names.get(id) {
                *id = new.clone();
            }
        };
        let mut made = Vec::new();
        for n in originals {
            let mut copy = n.clone();
            copy.id = names[&n.id].clone();
            match &mut copy.operation {
                Operation::Point { host, .. } => match host {
                    Host::Frame { point } | Host::Plane { point, .. } => rename(point),
                    Host::Curve { curve, .. } => rename(curve),
                    Host::Face { form, .. } | Host::Edge { form, .. } | Host::Vertex { form, .. } => rename(form),
                    Host::World { .. } | Host::Adaptive { .. } | Host::Handle { .. } => {}
                },
                Operation::Line { a, b } | Operation::Intersection { a, b, .. } => {
                    rename(a);
                    rename(b);
                }
                Operation::Polyline { points, .. } => points.iter_mut().for_each(rename),
                Operation::Bezier { points } => points.iter_mut().for_each(rename),
                Operation::Patch { chain } | Operation::Extrude { chain, .. } => rename(chain),
                Operation::Loft { profiles } => profiles.iter_mut().for_each(rename),
            }
            if let Operation::Point { host: Host::Adaptive { number, .. }, .. } = &mut copy.operation {
                *number = 0;
            }
            if let Some(slot) = self.nodes.iter_mut().find(|m| m.id == copy.id) {
                *slot = copy;
            }
            made.push(names[&n.id].clone());
        }
        // A copied placement point takes the next number, in the order of the originals'.
        for id in &made {
            let next = self.next_number();
            if let Some(Node { operation: Operation::Point { host: Host::Adaptive { number, .. }, .. }, name, .. }) =
                self.nodes.iter_mut().find(|n| &n.id == id)
                && *number == 0
            {
                *number = next;
                *name = format!("Adaptive point {next}");
            }
        }
        let moved = self.evaluate(&evaluation.parameters, &self.preview);
        self.move_elements(&made, by, &moved)?;
        Ok(made)
    }

    /// `ids` deleted, with every element that depends on them (as Revit deletes a line with its points); what was deleted.
    pub fn remove(&mut self, ids: &[String]) -> Vec<String> {
        let mut gone: BTreeSet<String> = ids.iter().filter(|id| self.nodes.iter().any(|n| &n.id == *id)).cloned().collect();
        loop {
            let more: Vec<String> = self
                .nodes
                .iter()
                .filter(|n| !gone.contains(&n.id) && n.dependencies().iter().any(|d| gone.contains(*d)))
                .map(|n| n.id.clone())
                .collect();
            if more.is_empty() {
                break;
            }
            gone.extend(more);
        }
        let adaptive: Vec<String> = self
            .nodes
            .iter()
            .filter(|n| gone.contains(&n.id) && matches!(n.operation, Operation::Point { host: Host::Adaptive { .. }, .. }))
            .map(|n| n.id.clone())
            .collect();
        // Highest number first, so each closing-up leaves the others' numbers right.
        let mut adaptive: Vec<(u32, String)> = adaptive
            .into_iter()
            .filter_map(|id| match self.nodes.iter().find(|n| n.id == id).map(|n| &n.operation) {
                Some(Operation::Point { host: Host::Adaptive { number, .. }, .. }) => Some((*number, id)),
                _ => None,
            })
            .collect();
        adaptive.sort_by_key(|a| std::cmp::Reverse(a.0));
        for (_, id) in adaptive {
            let _ = self.make_reference(&id);
        }
        self.nodes.retain(|n| !gone.contains(&n.id));
        gone.into_iter().collect()
    }
}

impl Placement {
    /// Frame `number` moved so its origin is `to`, its axes kept.
    pub fn moved(&self, number: u32, to: P3) -> Result<Placement, String> {
        let mut out = self.clone();
        let frame = out.frames.get_mut(&number).ok_or_else(|| format!("No adaptive point {number}"))?;
        frame.origin = to;
        frame.checked()?;
        Ok(out)
    }
}

/// Where the arrow of axis `axis` (0 x, 1 y, 2 z) of a move gizmo at `at` ends, `length` mm long.
pub fn gizmo_end(at: P3, axis: usize, length: f64) -> P3 {
    add(at, mul(std::array::from_fn(|i| if i == axis { 1. } else { 0. }), length))
}
