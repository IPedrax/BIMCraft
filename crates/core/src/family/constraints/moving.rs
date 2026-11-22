//! What is selected in a family's plan or elevation moved together by a drag. The planes, reference
//! lines and forms moved are held where the move puts them (each unknown of theirs at where it is drawn plus the shift: a
//! plane along its normal, a line's start in the axes of what it is drawn on, a form's sketch in its host's axes and its
//! ends along its normal), those rows first; then every constraint of the family. What the constraints tie to what is moved
//! follows it where they let it (a form's line locked to a plane that moves goes with the plane); a constraint that would
//! have to change to let the move be (a lock, a label, EQ, an Align lock to what stays) holds it, and the move is refused,
//! naming it. Before the constraints are asked, what can't move at all is refused ([`Stays`]): a datum, a plane that defines
//! the origin, what a pivot turns, a number of a form's that the move would change and that is a formula, and what would
//! leave the plane it is drawn on (a revolve, a sweep or a reference line moved off it).

use super::*;

/// What a move takes along: the family's own reference planes and its reference lines by GUID, its forms by place.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Moving {
    pub planes: Vec<Guid>,
    pub lines: Vec<Guid>,
    pub forms: Vec<usize>,
}

/// What a refusal names: a plane or a reference line by GUID, a form by place; [`Family::said`] says it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Named {
    Plane(Guid),
    Line(Guid),
    Form(usize),
}

/// Why a move is refused before the constraints are asked, naming what stays.
#[derive(Clone, Debug, PartialEq)]
pub enum Stays {
    /// A datum: Ref. Level and the two Center planes stay where they are.
    Datum(Named),
    /// A plane that defines the origin.
    Origin(Named),
    /// What a pivot turns, or a form on a plane a pivot turns.
    Pivot(Named),
    /// A form's number the move would change is a formula.
    Formula(Named),
    /// It would leave what it is drawn on: a revolve's, a sweep's or a reference line's way off its plane.
    Hosted(Named),
    /// A form of a file's triangles, which has no sketch to move.
    Mesh(Named),
}

/// Why a move is refused: what stays, or the constraints.
#[derive(Clone, Debug, PartialEq)]
pub enum MoveRefused {
    Stays(Stays),
    Unsat(Unsat),
}

impl From<Unsat> for MoveRefused {
    fn from(e: Unsat) -> MoveRefused {
        MoveRefused::Unsat(e)
    }
}

/// How far from zero a shift is still none, mm.
const NONE: f64 = 1e-9;

impl Moving {
    pub fn is_empty(&self) -> bool {
        self.planes.is_empty() && self.lines.is_empty() && self.forms.is_empty()
    }

    /// How far what `host` names goes when everything moved goes by `d` (the family's axes): a plane moved along its normal
    /// (its trace sliding along itself moves nothing drawn on it), a reference line moved by all of `d`, one drawn on what
    /// moves with it; else nowhere.
    pub fn host_shift(&self, f: &Family, host: &PlaneRef, d: P3) -> P3 {
        let mut at = *host;
        for _ in 0..=f.lines.len() {
            match at {
                PlaneRef::Plane(g) => {
                    if !self.planes.contains(&g) {
                        return [0.0; 3];
                    }
                    let Ok(fr) = f.frame(&at) else { return [0.0; 3] };
                    return mul(fr.z, dot(d, fr.z));
                }
                PlaneRef::Line(g, _) => {
                    if self.lines.contains(&g) {
                        return d;
                    }
                    match f.lines.iter().find(|l| l.guid == g) {
                        Some(l) => at = l.host,
                        None => return [0.0; 3],
                    }
                }
            }
        }
        [0.0; 3]
    }

    /// What is drawn on `host` moves by, of its own, in `host`'s axes: `d` less what the host moves by.
    fn own(&self, f: &Family, host: &PlaneRef, d: P3) -> Result<(P2, f64), String> {
        let fr = f.frame(host)?;
        let o = sub(d, self.host_shift(f, host, d));
        let tidy = |v: f64| if v.abs() <= NONE { 0.0 } else { v };
        Ok((P2(tidy(dot(o, fr.x)), tidy(dot(o, fr.y))), tidy(dot(o, fr.z))))
    }
}

/// A plane's name as a refusal says it.
pub(super) fn plane_said(f: &Family, g: Guid) -> String {
    let named = f.planes.iter().position(|p| p.guid == g).map(|i| (i, f.planes[i].name.clone()));
    match named {
        Some((_, n)) if !n.is_empty() => n,
        Some((i, _)) => format!("reference plane {}", i + 1),
        None => datums().into_iter().find(|d| d.guid == g).map_or_else(|| g.to_string(), |d| d.name),
    }
}

/// Whether every number `pick` takes of a form's sketch and top (on its host; a sweep's profiles go with its path) is a
/// number, not a formula.
fn numbers(form: &Form, pick: impl Fn(&[Expr; 2]) -> &Expr) -> bool {
    form.sketch.iter().chain(&form.top).all(|l| match &l.curve {
        FormCurve::Line { start, end, .. } => [start, end].iter().all(|p| matches!(pick(p), Expr::Number(_))),
        FormCurve::Circle { centre, .. } => matches!(pick(centre), Expr::Number(_)),
    })
}

/// A move's solve's rows, where it came to (`x`): each that isn't soft holds, or the move is refused, naming the dimensions
/// and pivots of those that don't.
pub(super) fn held(rows: &[Row], x: &[f64]) -> Result<(), Unsat> {
    let size = x.iter().fold(0.0f64, |a, v| a.max(v.abs()));
    let tol = 1e-6 * (size / 1000.0).max(1.0);
    let off: Vec<&Row> = rows.iter().filter(|r| !r.soft && r.lin.at(x).abs().partial_cmp(&tol) != Some(std::cmp::Ordering::Less)).collect();
    if off.is_empty() {
        return Ok(());
    }
    let mut dims: Vec<usize> = off.iter().filter_map(|r| r.dim).collect();
    dims.sort_unstable();
    dims.dedup();
    let mut pivots: Vec<String> = off.iter().filter_map(|r| r.pivot.clone()).collect();
    pivots.sort();
    pivots.dedup();
    let mut who: Vec<String> = Vec::new();
    match dims.as_slice() {
        [] => {}
        [d] => who.push(format!("dimension {}", d + 1)),
        ds => who.push(format!("dimensions {}", ds.iter().map(|d| (d + 1).to_string()).collect::<Vec<_>>().join(", "))),
    }
    who.extend(pivots);
    let message = if who.is_empty() {
        format!("{NOT_SATISFIED}: what is moved is held where it is")
    } else {
        format!("{NOT_SATISFIED}: {} holds what is moved where it is", who.join(" and "))
    };
    Err(Unsat { message, dims, over: true })
}

impl Family {
    /// What `n` is called in a refusal: a plane by its name, else `reference plane 2`; `reference line 1`; `form 3`.
    pub fn said(&self, n: Named) -> String {
        match n {
            Named::Plane(g) => plane_said(self, g),
            Named::Line(g) => {
                self.lines.iter().position(|l| l.guid == g).map_or_else(|| g.to_string(), |i| format!("reference line {}", i + 1))
            }
            Named::Form(i) => format!("form {}", i + 1),
        }
    }

    /// What `moving` takes along moved by `d` (the family's axes, mm) for `values`: the edits that do it, what the
    /// constraints tie to it following; or why not. Worked out from where the type has them, as a drag of a plane is.
    pub fn move_by(&self, values: &BTreeMap<String, Val>, moving: &Moving, d: P3) -> Result<Vec<Edit>, MoveRefused> {
        if !d.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD) {
            return Err(MoveRefused::Unsat(format!("a move of {d:?} mm is past {MAX_COORD} mm").into()));
        }
        let flexed = self.flexed(values)?;
        let stays = |s: Stays| Err(MoveRefused::Stays(s));
        let mut pins: Vec<Pin> = Vec::new();
        for g in &moving.planes {
            let said = Named::Plane(*g);
            let Some(i) = flexed.planes.iter().position(|p| p.guid == *g && !p.datum()) else { return stays(Stays::Datum(said)) };
            let p = &flexed.planes[i];
            if p.defines_origin {
                return stays(Stays::Origin(said));
            }
            if p.pivot.is_some() {
                return stays(Stays::Pivot(said));
            }
            pins.push(Pin::Shifted(Unknown::Plane(i), dot(d, p.frame().z)));
        }
        let turning =
            |host: &PlaneRef| matches!(host, PlaneRef::Plane(h) if flexed.planes.iter().any(|p| p.guid == *h && p.pivot.is_some()));
        for g in &moving.lines {
            let Some(i) = flexed.lines.iter().position(|l| l.guid == *g) else { continue };
            let l = &flexed.lines[i];
            let said = Named::Line(*g);
            if l.pivot.is_some() || turning(&l.host) {
                return stays(Stays::Pivot(said));
            }
            let (by, off) = moving.own(&flexed, &l.host, d).map_err(Unsat::from)?;
            if off != 0.0 {
                return stays(Stays::Hosted(said));
            }
            pins.push(Pin::Shifted(Unknown::LineStart(i, 0), by.0));
            pins.push(Pin::Shifted(Unknown::LineStart(i, 1), by.1));
            pins.push(Pin::Shifted(Unknown::LineAngle(i), 0.0));
            pins.push(Pin::Shifted(Unknown::LineLength(i), 0.0));
        }
        for &i in &moving.forms {
            let (Some(form), Some(drawn)) = (flexed.forms.get(i), self.forms.get(i)) else { continue };
            let said = Named::Form(i);
            if !form.mesh.is_empty() {
                return stays(Stays::Mesh(said));
            }
            if turning(&form.host) {
                return stays(Stays::Pivot(said));
            }
            let (by, along) = moving.own(&flexed, &form.host, d).map_err(Unsat::from)?;
            let ends = form.axis().is_none() && !form.kind().swept();
            if along != 0.0 && !ends {
                return stays(Stays::Hosted(said));
            }
            // Only the numbers it moves are written: one of them a formula, it stays (as a shape handle's drag).
            let tied = (by.0 != 0.0 && !numbers(drawn, |p| &p[0]))
                || (by.1 != 0.0 && !numbers(drawn, |p| &p[1]))
                || (along != 0.0 && !(matches!(drawn.from, Expr::Number(_)) && matches!(drawn.to, Expr::Number(_))));
            if tied {
                return stays(Stays::Formula(said));
            }
            pins.push(Pin::Form(i, by, along));
        }
        if pins.is_empty() {
            return Ok(Vec::new());
        }
        let s = flexed.solve_with(values, &pins, &[], &mut Meter::default())?;
        let mut out = flexed.edits(&s, values);
        // Its ends: an extrusion's or a blend's moved along its normal, as numbers.
        let mut read = HashMap::new();
        for (i, end, v) in &s.ends {
            let Some(form) = flexed.forms.get(*i).filter(|_| moving.forms.contains(i)) else { continue };
            let was = number(end_expr(form, *end), values, &mut read).map_err(Unsat::from)?;
            if (was - v).abs() > NONE {
                out.push(Edit::End(*i, *end, *v));
            }
        }
        Ok(out)
    }
}
