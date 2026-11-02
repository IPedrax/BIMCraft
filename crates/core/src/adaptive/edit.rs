//! Modelling an adaptive family as in Revit's family editor: reference points placed on a work plane or hosted on a line, made
//! adaptive in the order they are made so, lines drawn through points, forms made from profiles, points moved, elements
//! deleted with what depends on them, and what the pointer is over in a picture of the evaluation. Each edit changes the
//! canonical [`Definition`]; nothing here keeps state of its own.
use super::frame::norm;
use super::{
    Appearance, CurveLocation, Definition, Evaluated, Evaluation, Frame, Host, Measurement, Node, Operation, Placement, Transform,
};
use crate::family::Expr;
use crate::view3d::{P3, add, dot, mul, sub};
use std::collections::BTreeSet;

/// What the pointer is over in a picture of an evaluation.
#[derive(Clone, Debug, PartialEq)]
pub enum Picked {
    Point(String),
    /// A curve, and the normalized parameter (as [`Measurement::Parameter`] reads it) of the place on it nearest the pointer.
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

    /// A reference point on the work plane at `at`, its axes the world's (Revit's Point Element); its id.
    pub fn add_point(&mut self, at: P3) -> String {
        let host = Host::World { frame: Frame::at(at) };
        self.push("point", "Reference point", Operation::Point { host, transform: Transform::default() })
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
        let location = CurveLocation { measurement: Measurement::Parameter, value: Expr::Number(t.clamp(0., 1.)), from_end: false };
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

    /// Point `id` made adaptive, numbered next as Revit numbers them: in the order they are made adaptive. A point on the
    /// work plane keeps where it is; one already adaptive keeps its number. Its number.
    pub fn make_adaptive(&mut self, id: &str) -> Result<u32, String> {
        let number = self.next_number();
        let node = self.nodes.iter_mut().find(|n| n.id == id).ok_or_else(|| format!("No element `{id}`"))?;
        let Operation::Point { host, .. } = &mut node.operation else { return Err(format!("`{id}` is not a point")) };
        match *host {
            Host::Adaptive { number, .. } => Ok(number),
            Host::World { frame } => {
                *host = Host::Adaptive { number, frame };
                node.name = format!("Adaptive point {number}");
                Ok(number)
            }
            _ => Err(format!("`{id}` is hosted: only a point on the work plane can be made adaptive")),
        }
    }

    /// An adaptive point made a reference point on the work plane again, where it is; the points numbered after it close up.
    pub fn make_reference(&mut self, id: &str) -> Result<(), String> {
        let node = self.nodes.iter_mut().find(|n| n.id == id).ok_or_else(|| format!("No element `{id}`"))?;
        let Operation::Point { host, .. } = &mut node.operation else { return Err(format!("`{id}` is not a point")) };
        let Host::Adaptive { number, frame } = *host else { return Ok(()) };
        *host = Host::World { frame };
        node.name = "Reference point".into();
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
            Host::World { frame } => frame.origin = add(frame.origin, by),
            Host::Adaptive { number, frame } => match self.preview.frames.get_mut(number) {
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
                *location = CurveLocation { measurement: Measurement::Parameter, value: Expr::Number(t), from_end: false };
            }
            _ => return Err("A point on a form's face, edge or vertex moves with the form".into()),
        }
        Ok(())
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
