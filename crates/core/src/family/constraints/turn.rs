//! Align onto a reference that isn't parallel (FE-10). What can turn turns onto it: a reference plane of the family's own and
//! a reference line, about where it crosses the reference (or, where it doesn't reach that far, about its end nearer to it,
//! then slid onto it), as sketch mode's Align turns a line of the sketch (`align::Placement::onto`). A pivoted plane or line
//! turns about its pivot instead, its swing (its parameter, or its own angle) set so it lies on the reference. What a
//! dimension measures as drawn is named and refused; a form's face doesn't turn.

use super::*;
use crate::align::Placement;
use crate::view3d::{add, cross, mul, sub};

/// A turn of `angle` radians about the line through `about` along `axis` (right-handed), then a move by `by`.
#[derive(Clone, Copy, Debug)]
struct Turn {
    about: P3,
    axis: P3,
    angle: f64,
    by: P3,
}

impl Turn {
    fn vector(&self, v: P3) -> P3 {
        let (s, c) = self.angle.sin_cos();
        let k = self.axis;
        add(add(mul(v, c), mul(cross(k, v), s)), mul(k, dot(k, v) * (1.0 - c)))
    }

    fn point(&self, p: P3) -> P3 {
        add(add(self.about, self.vector(sub(p, self.about))), self.by)
    }
}

/// The turn about `axis` that lays the segment `a`–`b` (the family's axes) on the plane of points `p` with `n`·`p` = `at`.
fn onto(n: P3, at: f64, axis: P3, a: P3, b: P3) -> Result<Turn, String> {
    // The section square to the axis: e1 along the reference's trace there, e2 (its normal) across it; e1 × e2 = -axis.
    let e1 = unit3(cross(axis, n)).ok_or("the reference holds the axis it would turn about")?;
    let to2 = |p: P3| P2(dot(p, e1), dot(p, n));
    let p = Placement::onto((P2(0.0, at), P2(1000.0, at)), (to2(a), to2(b)))?;
    let along = mul(axis, dot(a, axis));
    let about = add(add(mul(e1, p.about.0), mul(n, p.about.1)), along);
    Ok(Turn { about, axis: mul(axis, -1.0), angle: p.angle.to_radians(), by: add(mul(e1, p.by.0), mul(n, p.by.1)) })
}

/// What dimension `d` (the `k`th) does, for a refusal that names it.
fn what_it_does(k: usize, d: &FamilyDimension) -> String {
    let how = if let Some(l) = &d.label {
        if d.kind == DimKind::Angular { format!(", the angle labelled `{l}`") } else { format!(", labelled `{l}`") }
    } else if d.alignment {
        ", an Align lock".to_string()
    } else if !d.locked.is_empty() {
        ", locked".to_string()
    } else if d.equal {
        ", EQ".to_string()
    } else if d.kind == DimKind::Angular {
        ", an angle".to_string()
    } else {
        String::new()
    };
    format!("dimension {}{how}", k + 1)
}

/// The refusal for `who`, held by `what`.
fn held(who: &str, what: &[String]) -> Unsat {
    let verb = if what.len() == 1 { "measures" } else { "measure" };
    Unsat::from(format!(
        "Align can't turn {who}: {} {verb} to it as it is drawn. Delete or unlock that first, or align it to a parallel reference",
        what.join(", ")
    ))
}

impl Family {
    /// Align of `target` onto `reference`, which isn't parallel to it: `n`, the reference's normal, `at` its place along it.
    pub(super) fn turn_onto(&self, values: &BTreeMap<String, Val>, target: &DimRef, n: P3, at: f64, t: P3) -> Result<Vec<Edit>, Unsat> {
        let axis = unit3(cross(t, n)).ok_or_else(|| Unsat::from("Align takes a reference parallel to the first".to_string()))?;
        match *target {
            DimRef::Plane(g) => self.turn_plane(values, g, n, at, axis),
            DimRef::Line(g, _) => self.turn_line(values, g, n, at, axis),
            DimRef::LinePoint(..) | DimRef::Point(..) => {
                Err(Unsat::from("Align takes a plane or a line: a point faces no way to turn onto the reference".to_string()))
            }
            DimRef::Sketch(..) | DimRef::Form(..) => {
                Err(Unsat::from("Align turns a plane or a line onto the reference; a form's face must be parallel to it".to_string()))
            }
        }
    }

    /// The dimensions measuring what `measures` says, named for a refusal; those `skip` takes over left out.
    fn holding(&self, measures: &dyn Fn(&DimRef) -> bool, skip: &dyn Fn(&FamilyDimension) -> bool) -> Vec<String> {
        self.dimensions
            .iter()
            .enumerate()
            .filter(|(_, d)| d.refs.iter().any(measures) && !skip(d))
            .map(|(k, d)| what_it_does(k, d))
            .collect()
    }

    fn turn_plane(&self, values: &BTreeMap<String, Val>, g: Guid, n: P3, at: f64, axis: P3) -> Result<Vec<Edit>, Unsat> {
        let datum = datums().into_iter().find(|d| d.guid == g);
        let (i, p) = match (self.planes.iter().position(|p| p.guid == g), &datum) {
            (Some(i), _) => (i, &self.planes[i]),
            (None, Some(d)) => (usize::MAX, d),
            (None, None) => return Err(Unsat::from(format!("{g} isn't one of its planes"))),
        };
        let who = if p.name.is_empty() { format!("reference plane {}", i.saturating_add(1)) } else { format!("`{}`", p.name) };
        if datum.is_some() || p.defines_origin || p.level {
            return Err(Unsat::from(format!(
                "{who} defines the family's origin and doesn't turn: Align turns a reference plane of the family's own or a reference line"
            )));
        }
        if p.pivot.is_some() {
            return self.turn_pivoted(values, g, &who, n, at, axis);
        }
        let others: Vec<String> = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.host.guid() == g)
            .map(|(k, _)| format!("reference line {}", k + 1))
            .chain(self.adaptive.iter().flat_map(|d| d.drawn_on(g)).map(|id| format!("adaptive point `{id}`")))
            .chain(self.nested_on(g))
            .collect();
        if !others.is_empty() {
            return Err(Unsat::from(format!("Align can't turn {who}: {} drawn on it", others.join(", "))));
        }
        let frame = p.frame();
        let way = sub(p.end, p.start);
        // Its trace across the axis: from its start to its end, or, where that runs along the axis, across it without end.
        let (a, b) = if parallel(way, axis) {
            let w = cross(axis, frame.z);
            (add(p.start, mul(w, -MAX_COORD)), add(p.start, mul(w, MAX_COORD)))
        } else {
            (p.start, p.end)
        };
        let turn = onto(n, at, axis, a, b).map_err(Unsat::from)?;
        // An Align lock to a straight line of a form whose sketch the turn sees flat carries the line with it.
        let carried = |d: &FamilyDimension| d.alignment && d.refs.len() == 2 && self.carried_line(d, g, axis).is_some();
        let holds = self.holding(&|r| *r == DimRef::Plane(g), &carried);
        if !holds.is_empty() {
            return Err(held(&who, &holds));
        }
        let turned = RefPlane { start: turn.point(p.start), end: turn.point(p.end), up: turn.vector(p.up), ..p.clone() };
        let mut out = vec![Edit::PlaneTurn(g, [turned.start, turned.end, turned.up])];
        // What is drawn on it keeps its place on it: its sketch slid by where its axes' origin goes.
        let (shift, _) = turned.frame().local(turn.point(frame.origin));
        for (k, form) in self.forms.iter().enumerate() {
            if form.host == PlaneRef::Plane(g) && shift.0.hypot(shift.1) > 1e-9 {
                out.push(Edit::FormShift(k, [shift.0, shift.1]));
            }
        }
        for d in self.dimensions.iter().filter(|d| d.alignment && d.refs.contains(&DimRef::Plane(g))) {
            if let Some((k, line)) = self.carried_line(d, g, axis) {
                out.extend(self.carry(values, k, line, &turn).map_err(Unsat::from)?);
            }
        }
        Ok(out)
    }

    /// The form and the straight line of its sketch an Align lock `d` holds to plane `g`, where the turn about `axis` sees
    /// that sketch flat and the form isn't drawn on `g` itself (which carries it whole).
    fn carried_line(&self, d: &FamilyDimension, g: Guid, axis: P3) -> Option<(usize, usize)> {
        let other = d.refs.iter().find(|r| **r != DimRef::Plane(g))?;
        let DimRef::Sketch(fg, lg) = *other else { return None };
        let (k, form) = self.forms.iter().enumerate().find(|(_, f)| f.guid == fg)?;
        let frame = self.frame(&form.host).ok()?;
        if form.host == PlaneRef::Plane(g) || !parallel(frame.z, axis) {
            return None;
        }
        let line = form.sketch.iter().position(|l| l.guid == lg)?;
        matches!(form.sketch[line].curve, FormCurve::Line { turn: None, .. }).then_some((k, line))
    }

    /// Line `line` of form `k`'s sketch turned with `turn`, the lines joined to its ends following them.
    fn carry(&self, values: &BTreeMap<String, Val>, k: usize, line: usize, turn: &Turn) -> Result<Vec<Edit>, String> {
        let form = self.forms.get(k).ok_or("no such form")?;
        let frame = self.frame(&form.host)?;
        let at = |e: &[Expr; 2]| -> Result<P2, String> {
            let mut read = HashMap::new();
            Ok(P2(number(&e[0], values, &mut read)?, number(&e[1], values, &mut read)?))
        };
        let ends: Vec<Option<(P2, P2)>> = form
            .sketch
            .iter()
            .map(|l| match &l.curve {
                FormCurve::Line { start, end, .. } => Ok(Some((at(start)?, at(end)?))),
                FormCurve::Circle { .. } => Ok(None),
            })
            .collect::<Result<_, String>>()?;
        let Some(Some((a, b))) = ends.get(line).copied() else { return Err("no such line".into()) };
        let moved = |q: P2| frame.local(turn.point(frame.point(q, 0.0))).0;
        let mut out = Vec::new();
        for (from, spot) in [(a, Spot::Start), (b, Spot::End)] {
            let to = moved(from);
            out.push(Edit::Point(k, line, spot, [to.0, to.1]));
            for (j, e) in ends.iter().enumerate().filter(|(j, _)| *j != line) {
                let Some((s, t)) = e else { continue };
                if s.dist(from) <= TOL {
                    out.push(Edit::Point(k, j, Spot::Start, [to.0, to.1]));
                }
                if t.dist(from) <= TOL {
                    out.push(Edit::Point(k, j, Spot::End, [to.0, to.1]));
                }
            }
        }
        Ok(out)
    }

    fn turn_line(&self, values: &BTreeMap<String, Val>, g: Guid, n: P3, at: f64, axis: P3) -> Result<Vec<Edit>, Unsat> {
        let i = self.lines.iter().position(|l| l.guid == g).ok_or_else(|| Unsat::from(format!("{g} isn't one of its lines")))?;
        let l = &self.lines[i];
        let who = format!("reference line {}", i + 1);
        if l.pivot.is_some() {
            return self.turn_pivoted(values, g, &who, n, at, axis);
        }
        let host = self.frame(&l.host).map_err(Unsat::from)?;
        if !parallel(host.z, axis) {
            return Err(Unsat::from(format!(
                "Align can't turn {who} onto that reference: it would leave the plane it is drawn on. Align it in a view that sees that plane flat"
            )));
        }
        let holds = self.holding(&|r| matches!(*r, DimRef::Line(h, _) | DimRef::LinePoint(h, _) if h == g), &|_| false);
        if !holds.is_empty() {
            return Err(held(&who, &holds));
        }
        let turn = onto(n, at, axis, l.start, l.end).map_err(Unsat::from)?;
        let ((a, _), (b, _)) = (host.local(turn.point(l.start)), host.local(turn.point(l.end)));
        let s = Solution { lines: vec![(i, [a.0, a.1, (b.1 - a.1).atan2(b.0 - a.0), a.dist(b)])], ..Solution::default() };
        Ok(self.edits(&s, values))
    }

    /// A pivoted plane or line `g` turned about its pivot onto the reference: its swing set (its parameter, or its own angle).
    fn turn_pivoted(&self, values: &BTreeMap<String, Val>, g: Guid, who: &str, n: P3, at: f64, axis: P3) -> Result<Vec<Edit>, Unsat> {
        let flexed = self.flexed(values)?;
        let (e, piv) = flexed.pivot_of(g).ok_or_else(|| Unsat::from(format!("{who} has no pivot")))?;
        let r = flexed.rotation(e).map_err(Unsat::from)?;
        if !parallel(r.frame.z, axis) {
            return Err(Unsat::from(format!("{who} turns about its pivot only, whose axis this view doesn't look along")));
        }
        let hinge = r.frame.origin;
        if (dot(n, hinge) - at).abs() > 0.5 {
            return Err(Unsat::from(format!("{who} turns about its pivot, which isn't on the reference: turned, it can't lie on it")));
        }
        let mut w = unit3(cross(n, r.frame.z)).ok_or_else(|| Unsat::from("the reference holds the pivot's axis".to_string()))?;
        if dot(w, r.arm) < 0.0 {
            w = mul(w, -1.0);
        }
        let (v, edits) = self.drag_pivot(values, g, add(hinge, mul(w, 1000.0)), false)?;
        // Within its limits, it lies on the reference.
        let mut after = self.clone();
        let mut values = values.clone();
        for edit in &edits {
            match edit {
                Edit::Parameter(name, v) => {
                    values.insert(name.clone(), Val::Number(*v));
                }
                Edit::PivotAngle(..) => {
                    let p = after.planes.iter_mut().find(|p| p.guid == g).and_then(|p| p.pivot.as_mut());
                    let p = p.or_else(|| after.lines.iter_mut().find(|l| l.guid == g).and_then(|l| l.pivot.as_mut()));
                    if let Some(p) = p {
                        p.angle = v;
                    }
                }
                _ => {}
            }
        }
        let lies = after.flexed(&values).ok().and_then(|f| f.rotation(e).ok()).is_some_and(|r2| parallel(r2.way, w));
        if !lies {
            let (lo, hi) = (piv.min.unwrap_or(-MAX_SWING), piv.max.unwrap_or(MAX_SWING));
            return Err(Unsat::from(format!("{who} turns about its pivot from {lo}° to {hi}° only: it can't lie on that reference")));
        }
        Ok(edits)
    }
}
