//! What is selected in a family's plan or elevation rotated, mirrored or scaled, as the Modify panel's tools do it. The
//! planes, reference lines and forms taken are drawn again where the transform puts them, in one go: a plane's ends and its
//! up (mirrored, its ends swapped, so its normal is the mirror image of what it was and what is drawn on it keeps its side),
//! a reference line's ends, a form's sketch and top in its host's axes. What is drawn on what is taken goes with it, as on a
//! move. A form's numbers the transform changes are written as numbers; one that is a formula refuses it, as a move's
//! does. A mirrored arc turns the other way, and a mirrored revolve turns the other way round its axis.
//!
//! What stays, refusing it before anything is changed, is what stays on a move ([`Stays`]): a datum, a plane that defines the
//! origin, what a pivot turns, a form of a file's triangles, and what would leave the plane it is drawn on that isn't taken
//! too. Scale takes planes and reference lines alone (a form is sized by its dimensions and parameters, so Scale
//! leaves it), and Mirror leaves a sweep, whose profile would face the wrong way ([`TransformRefused`]).
//!
//! A dimension whose references are all taken goes with them. One that holds a constraint (a label, a lock, EQ, Align's lock)
//! between what is taken and what isn't would no longer hold, nor would a label or a lock of a length Scale changes: the
//! transform is refused as "Constraints are not satisfied", naming them ([`Unsat`]), which the front end asks about,
//! Remove Constraints then doing it again.

use super::*;
use crate::family::forms::FormKind;
use crate::view3d::{cross, sub};

/// How far a point may be off the plane it is drawn on and still be on it, mm.
const ON: f64 = 1e-6;

/// The most a Scale may make bigger or smaller.
pub const MAX_FACTOR: f64 = 1000.0;

/// A rotation, a mirror or a scale of the family's space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Transform {
    /// About the line through `centre` along `axis`, `degrees` right-handed about it.
    Rotate { centre: P3, axis: P3, degrees: f64 },
    /// Across the plane through `point` square to `normal`.
    Mirror { point: P3, normal: P3 },
    /// About `centre`, by `factor`, across the plane square to `normal` alone: what lies along `normal` (the depth the view
    /// looks along) is held: Scale works in the view's plane.
    Scale { centre: P3, factor: f64, normal: P3 },
}

/// What a transform takes: the family's own reference planes and its reference lines by GUID, its forms by place. A
/// dimension goes with its references where they are all taken; one that measures to what isn't taken stays and measures
/// again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transforming {
    pub planes: Vec<Guid>,
    pub lines: Vec<Guid>,
    pub forms: Vec<usize>,
}

/// Why a transform is refused.
#[derive(Clone, Debug, PartialEq)]
pub enum TransformRefused {
    /// What stays on a move stays here too.
    Stays(Stays),
    /// The constraints of these dimensions would no longer hold.
    Unsat(Unsat),
    /// Scale takes planes and reference lines alone: a form is sized by its dimensions and parameters.
    NotScaled(Named),
    /// A sweep or a swept blend isn't mirrored: its profile would face the wrong way along the path.
    NotMirrored(Named),
    /// A nested instance placed on what is taken would be left behind: by place.
    Nested(usize),
    /// The transform itself can't be: no turn, no mirror line, a factor out of range.
    Bad(String),
}

/// The family as the transform leaves it, and what it took: what was asked and what is drawn on it.
#[derive(Clone, Debug)]
pub struct Transformed {
    pub family: Family,
    pub planes: Vec<Guid>,
    pub lines: Vec<Guid>,
    pub forms: Vec<usize>,
    pub dimensions: Vec<Guid>,
    /// The dimensions deleted: each measured between what is taken and what isn't, and no longer could.
    pub dropped: Vec<Guid>,
}

fn unit3(a: P3) -> Option<P3> {
    let l = dot(a, a).sqrt();
    (l.is_finite() && l > 1e-12).then(|| [a[0] / l, a[1] / l, a[2] / l])
}

fn finite(p: P3) -> bool {
    p.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD)
}

/// To a micron's millionth, so a number comes back as typed after a whole turn.
fn tidy(v: f64) -> f64 {
    let r = (v * 1e6).round() / 1e6;
    if r == 0.0 { 0.0 } else { r }
}

impl Transform {
    /// What it does to a point of the family.
    pub fn point(&self, p: P3) -> P3 {
        match *self {
            Transform::Rotate { centre, .. } => {
                let v = self.vector(sub(p, centre));
                std::array::from_fn(|k| centre[k] + v[k])
            }
            Transform::Mirror { point, .. } => {
                let v = self.vector(sub(p, point));
                std::array::from_fn(|k| point[k] + v[k])
            }
            Transform::Scale { centre, .. } => {
                let v = self.vector(sub(p, centre));
                std::array::from_fn(|k| centre[k] + v[k])
            }
        }
    }

    /// What it does to a direction (its linear part).
    pub fn vector(&self, v: P3) -> P3 {
        match *self {
            Transform::Rotate { axis, degrees, .. } => {
                let Some(k) = unit3(axis) else { return v };
                // Rodrigues: v cos + (k × v) sin + k (k·v)(1 − cos).
                let (s, c) = degrees.to_radians().sin_cos();
                let kv = cross(k, v);
                let along = dot(k, v) * (1.0 - c);
                std::array::from_fn(|i| v[i] * c + kv[i] * s + k[i] * along)
            }
            Transform::Mirror { normal, .. } => {
                let Some(n) = unit3(normal) else { return v };
                let d = 2.0 * dot(v, n);
                std::array::from_fn(|i| v[i] - n[i] * d)
            }
            Transform::Scale { factor, normal, .. } => {
                let Some(n) = unit3(normal) else { return v.map(|x| x * factor) };
                let along = dot(v, n) * (1.0 - factor);
                std::array::from_fn(|i| v[i] * factor + n[i] * along)
            }
        }
    }

    /// Whether it turns things the other way round: a mirror.
    pub fn reflects(&self) -> bool {
        matches!(self, Transform::Mirror { .. })
    }

    /// What it does, for a refusal: "rotated", "mirrored", "scaled".
    pub fn done(&self) -> &'static str {
        match self {
            Transform::Rotate { .. } => "rotated",
            Transform::Mirror { .. } => "mirrored",
            Transform::Scale { .. } => "scaled",
        }
    }

    /// Whether it can be done: finite, a turn about an axis, a mirror across a plane, a factor within [`MAX_FACTOR`] either
    /// way.
    fn check(&self) -> Result<(), String> {
        match *self {
            Transform::Rotate { centre, axis, degrees } => {
                if !finite(centre) || unit3(axis).is_none() || !degrees.is_finite() || degrees.abs() > 3600.0 {
                    return Err(format!("a rotation of {degrees}° about {centre:?} along {axis:?} can't be made"));
                }
            }
            Transform::Mirror { point, normal } => {
                if !finite(point) || unit3(normal).is_none() {
                    return Err("the mirror's axis is one point: pick or draw a line".into());
                }
            }
            Transform::Scale { centre, factor, normal } => {
                if unit3(normal).is_none() {
                    return Err("a scale needs the view it is seen in: the way it holds".into());
                }
                if !finite(centre) || !factor.is_finite() || factor <= 0.0 || !(1.0 / MAX_FACTOR..=MAX_FACTOR).contains(&factor) {
                    return Err(format!("a scale of {factor} can't be made: give a factor above 0, from 1/{MAX_FACTOR} to {MAX_FACTOR}"));
                }
            }
        }
        Ok(())
    }
}

/// `e`, a number worth `was`, written `now`; a formula stays as it is where it is still worth it, else refuses (false).
fn put(e: &mut Expr, was: f64, now: f64) -> bool {
    if (now - was).abs() <= 1e-6 {
        return true;
    }
    match e {
        Expr::Number(v) => {
            *v = tidy(now);
            true
        }
        Expr::Formula(_) => false,
    }
}

impl Family {
    /// What `taking` takes, and what is drawn on it, transformed by `t`, worked out from where they are drawn with the
    /// formulas worth what `values` make them: the family as it leaves it, or why not.
    pub fn transformed(
        &self,
        values: &BTreeMap<String, Val>,
        taking: &Transforming,
        t: &Transform,
    ) -> Result<Transformed, TransformRefused> {
        t.check().map_err(TransformRefused::Bad)?;
        let stays = |s: Stays| Err(TransformRefused::Stays(s));
        // The planes asked for: never a datum, the plane that defines the origin or one a pivot turns.
        for g in &taking.planes {
            let said = Named::Plane(*g);
            let Some(p) = self.planes.iter().find(|p| p.guid == *g && !p.datum()) else { return stays(Stays::Datum(said)) };
            if p.defines_origin {
                return stays(Stays::Origin(said));
            }
            if p.pivot.is_some() {
                return stays(Stays::Pivot(said));
            }
        }
        let planes: Vec<Guid> = taking.planes.clone();
        // The lines asked for and those drawn on what is taken, in the family's order (a line is drawn on one before it).
        let mut lines: Vec<Guid> = Vec::new();
        for l in &self.lines {
            let on = match l.host {
                PlaneRef::Plane(h) => planes.contains(&h),
                PlaneRef::Line(h, _) => lines.contains(&h),
            };
            if taking.lines.contains(&l.guid) || on {
                lines.push(l.guid);
            }
        }
        let taken = |host: &PlaneRef| match host {
            PlaneRef::Plane(h) => planes.contains(h),
            PlaneRef::Line(h, _) => lines.contains(h),
        };
        let turning = |host: &PlaneRef| match host {
            PlaneRef::Plane(h) => self.planes.iter().any(|p| p.guid == *h && p.pivot.is_some()),
            PlaneRef::Line(h, _) => self.lines.iter().any(|l| l.guid == *h && l.pivot.is_some()),
        };
        for l in self.lines.iter().filter(|l| lines.contains(&l.guid)) {
            if l.pivot.is_some() || turning(&l.host) {
                return stays(Stays::Pivot(Named::Line(l.guid)));
            }
        }
        let mut forms: Vec<usize> = (0..self.forms.len()).filter(|i| taking.forms.contains(i) || taken(&self.forms[*i].host)).collect();
        forms.dedup();
        for &i in &forms {
            let form = &self.forms[i];
            let said = Named::Form(i);
            if !form.mesh.is_empty() {
                return stays(Stays::Mesh(said));
            }
            if turning(&form.host) {
                return stays(Stays::Pivot(said));
            }
            if matches!(t, Transform::Scale { .. }) {
                return Err(TransformRefused::NotScaled(said));
            }
            if t.reflects() && form.kind().swept() {
                return Err(TransformRefused::NotMirrored(said));
            }
        }
        if let Some(k) = self.nested_instances.iter().position(|n| taken(&n.host)) {
            return Err(TransformRefused::Nested(k));
        }
        let mut out = self.clone();
        for p in out.planes.iter_mut().filter(|p| planes.contains(&p.guid)) {
            let (a, b) = (t.point(p.start), t.point(p.end));
            // Mirrored, its ends swapped: its normal the mirror image of its own, so what is drawn on it keeps its side.
            (p.start, p.end) = if t.reflects() { (b, a) } else { (a, b) };
            // A way, not a length: a scale leaves it as long as it was.
            p.up = unit3(t.vector(p.up)).unwrap_or(p.up);
            if !finite(p.start) || !finite(p.end) {
                return Err(TransformRefused::Bad(format!("{} would go past {MAX_COORD} mm", self.said(Named::Plane(p.guid)))));
            }
        }
        for l in out.lines.iter_mut().filter(|l| lines.contains(&l.guid)) {
            (l.start, l.end) = (t.point(l.start), t.point(l.end));
            if !finite(l.start) || !finite(l.end) {
                return Err(TransformRefused::Bad(format!("{} would go past {MAX_COORD} mm", self.said(Named::Line(l.guid)))));
            }
        }
        // A line whose host stays must stay on it: a turn about an axis across the plane, a mirror across one, takes it off.
        for (k, l) in out.lines.iter().enumerate().filter(|(_, l)| lines.contains(&l.guid) && !taken(&l.host)) {
            let fr = out.frame(&l.host).map_err(|e| TransformRefused::Unsat(e.into()))?;
            if [l.start, l.end].iter().any(|p| fr.local(*p).1.abs() > ON * (1.0 + dot(*p, *p).sqrt())) {
                return stays(Stays::Hosted(Named::Line(self.lines[k].guid)));
            }
        }
        // The forms: each point of a sketch on its host, taken through the family's space onto the host as it is left.
        for &i in &forms {
            let said = Named::Form(i);
            let host = self.forms[i].host;
            let was = self.frame(&host).map_err(|e| TransformRefused::Unsat(e.into()))?;
            let now = out.frame(&host).map_err(|e| TransformRefused::Unsat(e.into()))?;
            let onto = |q: P2| -> Result<P2, TransformRefused> {
                let (p, off) = now.local(t.point(was.point(q, 0.0)));
                if off.abs() > ON * (1.0 + q.0.abs() + q.1.abs()) {
                    return Err(TransformRefused::Stays(Stays::Hosted(said)));
                }
                Ok(p)
            };
            // How the host's axes come back: turned the other way (a mirror seen on it), an arc turns the other way.
            let dir = |v: P2| {
                let w = t.vector(was.dir(v));
                P2(dot(w, now.x), dot(w, now.y))
            };
            let flipped = dir(P2(1.0, 0.0)).cross(dir(P2(0.0, 1.0))) < 0.0;
            // Whether the host's normal comes back the other way: what was along it is then against it, so an extrusion's
            // Start and End, and a Blend's base and top, go to the other side.
            let over = dot(t.vector(was.z), now.z) < 0.0;
            if over && self.forms[i].kind().swept() {
                return Err(TransformRefused::NotMirrored(said));
            }
            let form = &mut out.forms[i];
            let refused = || TransformRefused::Stays(Stays::Formula(said));
            let mut ok = true;
            let drawn = self.forms[i].clone();
            let keep = drawn.sketch.iter().chain(&drawn.top);
            for (line, before) in form.sketch.iter_mut().chain(form.top.iter_mut()).zip(keep) {
                let pt = |at: &mut [Expr; 2], old: &[Expr; 2]| -> Result<bool, TransformRefused> {
                    let x = worth(&old[0], values).map_err(|e| TransformRefused::Unsat(e.into()))?;
                    let y = worth(&old[1], values).map_err(|e| TransformRefused::Unsat(e.into()))?;
                    let q = onto(P2(x, y))?;
                    Ok(put(&mut at[0], x, q.0) && put(&mut at[1], y, q.1))
                };
                match (&mut line.curve, &before.curve) {
                    (FormCurve::Line { start, end, turn }, FormCurve::Line { start: s0, end: e0, .. }) => {
                        ok &= pt(start, s0)?;
                        ok &= pt(end, e0)?;
                        if flipped && let Some(turn) = turn.as_mut() {
                            let v = worth(turn, values).map_err(|e| TransformRefused::Unsat(e.into()))?;
                            ok &= put(turn, v, -v);
                        }
                    }
                    (FormCurve::Circle { centre, .. }, FormCurve::Circle { centre: c0, .. }) => ok &= pt(centre, c0)?,
                    _ => {}
                }
            }
            let a = worth(&drawn.from, values).map_err(|e| TransformRefused::Unsat(e.into()))?;
            let b = worth(&drawn.to, values).map_err(|e| TransformRefused::Unsat(e.into()))?;
            match drawn.kind() {
                // A revolve mirrored turns the other way round its axis: from −End to −Start. A turn keeps its angles,
                // whichever way it leaves the plane.
                FormKind::Revolve if t.reflects() => ok &= put(&mut form.from, a, -b) && put(&mut form.to, b, -a),
                // An extrusion turned over spans the other side: from −End to −Start.
                FormKind::Extrusion if over => ok &= put(&mut form.from, a, -b) && put(&mut form.to, b, -a),
                // A Blend's base stays its base and its top its top, each on the other side.
                FormKind::Blend if over => ok &= put(&mut form.from, a, -a) && put(&mut form.to, b, -b),
                _ => {}
            }
            if !ok {
                return Err(refused());
            }
        }
        // The dimensions: those whose references are all taken go with them; a constraint across the edge of what is taken,
        // or a length Scale changes held by a label or a lock, refuses it.
        let reporting = |l: &str| self.reporting(l);
        let form_guids: Vec<Guid> = forms.iter().filter_map(|i| self.forms.get(*i)).map(|f| f.guid).collect();
        let inside = |r: &DimRef| match *r {
            DimRef::Plane(g) => planes.contains(&g),
            DimRef::Form(g, _) | DimRef::Sketch(g, _) | DimRef::Point(g, ..) => form_guids.contains(&g),
            DimRef::Line(g, _) | DimRef::LinePoint(g, _) => lines.contains(&g),
        };
        let scaling = matches!(t, Transform::Scale { factor, .. } if (factor - 1.0).abs() > 1e-12);
        let mut held: Vec<usize> = Vec::new();
        let mut dimensions = Vec::new();
        for (k, d) in out.dimensions.iter_mut().enumerate() {
            let all = d.refs.iter().all(inside);
            let some = d.refs.iter().any(inside);
            let ties = d.constrains(&reporting);
            let sized = scaling && (d.label.as_deref().is_some_and(|l| !reporting(l)) || !d.locked.is_empty());
            if (some && !all && ties) || (all && sized) {
                held.push(k);
            } else if all && !d.refs.is_empty() {
                d.at = t.point(d.at);
                dimensions.push(d.guid);
            }
        }
        if !held.is_empty() {
            let named = match held.as_slice() {
                [k] => format!("dimension {}", k + 1),
                ks => format!("dimensions {}", ks.iter().map(|k| (k + 1).to_string()).collect::<Vec<_>>().join(", ")),
            };
            let message = if scaling {
                format!("{NOT_SATISFIED}: {named} would no longer hold what is {}", t.done())
            } else {
                format!("{NOT_SATISFIED}: {named} ties what is {} to what isn't", t.done())
            };
            return Err(TransformRefused::Unsat(Unsat { message, dims: held, over: true }));
        }
        // A dimension between what is taken and what isn't whose references no longer face one way is deleted: it no
        // longer measures anything.
        let mut dropped = Vec::new();
        for d in out.dimensions.iter().filter(|d| d.kind == DimKind::Aligned && d.refs.iter().any(inside) && !d.refs.iter().all(inside)) {
            let ways: Vec<P3> = d.refs.iter().filter_map(|r| direction(&out, r, values).ok().flatten()).collect();
            if ways.windows(2).any(|w| !parallel(w[0], w[1])) {
                dropped.push(d.guid);
            }
        }
        out.dimensions.retain(|d| !dropped.contains(&d.guid));
        Ok(Transformed { family: out, planes, lines, forms, dimensions, dropped })
    }
}
