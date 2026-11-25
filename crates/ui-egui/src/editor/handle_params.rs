//! A shape handle tied to a parameter drags it. A side or an end whose place a labelled dimension sets, or an end whose
//! number is a formula of parameters (an End associated with `Height`), is dragged by changing that parameter for the type
//! previewed (an instance parameter's own value, its default in the family): the value that puts the side or end where the
//! hand is, found on the flexed family (the dimension's length after the drag, the End's new number) and written as a
//! dimension's drag through its label writes it (`Edit::Parameter`), one step of the history. A side whose numbers are
//! formulas and that no label sets has no parameter dragged: it moves alone, its formulas kept with the move added (
//! `handles::shifted`; the starter box's `Width / 2` side dragged 40 out is `(Width / 2) + 40`).
//!
//! Which parameter is found by moving each (`drive`): of those the side's or end's formulas name (or the label on it), the ones
//! that move it, a label's first, then a size (one that leaves the far side where it is or moves it the other way) before a
//! position (one that moves both one way), then as the family lists them. None that moves it, or no type to set it for, and
//! the handle is greyed saying why; so is one whose parameter is worked out by a formula, is a reporting one or is set by the
//! instance previewed. The value is searched for (`solve`): secant steps from where the last frame left it, then, where they
//! don't land, a bracket widened outward from the value it began at and halved, so a nonlinear formula, an `if()` and one flat
//! where it starts are reached; an Integer takes the nearest whole number, the rest a hundredth of a millimetre.
//!
//! Refused, the draft as it was and the status bar saying why: a value that turns the profile inside out, leaves it nothing or
//! the form no depth (whatever the parameter's kind), one the drag can't reach, and one a type that built can't build with. A
//! lock, an Align lock or EQ holding a side or an end with no label is no parameter: its handle isn't drawn.
use super::document::Draft;
use super::sketch::reader;
use super::*;
use bimcraft_core::family::constraints::Edit;
use bimcraft_core::family::handles::{self as hs, Held};
use bimcraft_core::family::sketch::{FormCurve, FormLine};
use bimcraft_core::family::{Form, Kind, Val};
use bimcraft_core::geom::P2;
use bimcraft_core::ids::Guid;

/// What a handle's drag moves: a side of the profile (its line, the way out in the plane's axes), or the End (true) or the Start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Target {
    Edge(usize, P2),
    End(bool),
}

/// How a handle's drag works: on the numbers themselves, through one of these parameters (the first it tries), or refused, why;
/// or the handle isn't drawn.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Drive {
    Free,
    Param(Vec<String>),
    Refused(String),
    Hidden,
}

/// The drives worked out for the draft as it was (`key`), by form and side (a line, or the End and Start as -1 and -2).
#[derive(Default)]
pub(crate) struct Drives {
    key: u64,
    by: std::collections::HashMap<(usize, i64), Drive>,
}

/// Where a side or an end stands along its handle's way (`m`), the far side of the form that way (`far`), and the profile's
/// signed area or the form's depth (`body`): what turns inside out changes its sign.
#[derive(Clone, Copy, Debug)]
struct Probe {
    m: f64,
    far: f64,
    body: f64,
}

/// A parameter being dragged by a handle: the draft and the family as the drag began (what Escape and a refusal put back),
/// the type, which types built then, the form's GUID and place, what moves, the parameter, its kind, its value and where the
/// side or end stood, a value it moves it at, the last value found, how far the drag has gone, whether it is written as it
/// goes (a plan or an elevation) or once the button is up ({3D}, the value found waiting).
#[derive(Clone)]
pub(crate) struct ParamDrag {
    kept: Draft,
    family: Family,
    ty: String,
    built: Vec<(String, bool)>,
    form: (Guid, usize),
    target: Target,
    name: String,
    integer: bool,
    v0: f64,
    start: Probe,
    moved: (f64, f64),
    last: Option<(f64, f64)>,
    total: f64,
    live: bool,
    pending: Option<f64>,
}

/// The parameters `formulas` read, as `f` lists them: each formula parsed (`family::names`), so a name is matched whole, never
/// inside another name or a function's (`a` isn't read by `abs(b)`). A formula that doesn't parse names none.
fn named(f: &Family, formulas: &[&str]) -> Vec<String> {
    let read: Vec<String> = formulas.iter().filter_map(|text| bimcraft_core::family::names(text).ok()).flatten().collect();
    f.parameters.iter().filter(|p| !p.name.is_empty() && read.contains(&p.name)).map(|p| p.name.clone()).collect()
}

fn formulas<'a>(exprs: impl IntoIterator<Item = &'a Expr>) -> Vec<&'a str> {
    exprs
        .into_iter()
        .filter_map(|e| match e {
            Expr::Formula(s) => Some(s.as_str()),
            Expr::Number(_) => None,
        })
        .collect()
}

/// `f` with parameter `name` at `v` for type `ty`: a type parameter in the type, an instance one its own value.
fn with_param(f: &Family, ty: &str, name: &str, v: f64) -> Family {
    let mut g = f.clone();
    let instance = g.parameters.iter().any(|p| p.name == name && p.instance);
    if instance {
        if let Some(p) = g.parameters.iter_mut().find(|p| p.name == name) {
            p.value = serde_json::json!(v);
        }
    } else {
        g.types.entry(ty.to_string()).or_default().insert(name.to_string(), serde_json::json!(v));
    }
    g
}

/// The form of `f` known by `guid` (else at `i`).
fn form_of(f: &Family, (guid, i): (Guid, usize)) -> Option<&Form> {
    if guid.is_nil() { f.forms.get(i) } else { f.forms.iter().find(|x| x.guid == guid) }
}

/// The points of `lines` (their ends, a circle's centre) but line `skip`'s, read by `num`.
fn points(lines: &[FormLine], skip: Option<usize>, num: &mut dyn FnMut(&Expr) -> Result<f64, String>) -> Option<Vec<P2>> {
    let mut out = Vec::new();
    for (_, l) in lines.iter().enumerate().filter(|(k, l)| Some(*k) != skip && !l.axis) {
        match &l.curve {
            FormCurve::Line { start, end, .. } => {
                out.push(P2(num(&start[0]).ok()?, num(&start[1]).ok()?));
                out.push(P2(num(&end[0]).ok()?, num(&end[1]).ok()?));
            }
            FormCurve::Circle { centre, .. } => out.push(P2(num(&centre[0]).ok()?, num(&centre[1]).ok()?)),
        }
    }
    Some(out)
}

/// The signed area the starts of `lines` go round, in order: what a profile turned inside out changes the sign of.
fn swept(lines: &[FormLine], num: &mut dyn FnMut(&Expr) -> Result<f64, String>) -> Option<f64> {
    let mut a = 0.0;
    for l in lines.iter().filter(|l| !l.axis) {
        match &l.curve {
            FormCurve::Line { start, end, .. } => {
                let (p, q) = (P2(num(&start[0]).ok()?, num(&start[1]).ok()?), P2(num(&end[0]).ok()?, num(&end[1]).ok()?));
                a += p.0 * q.1 - q.0 * p.1;
            }
            FormCurve::Circle { radius, .. } => a += num(radius).ok()?.powi(2),
        }
    }
    Some(a / 2.0)
}

/// Why no parameter can move a handle's side or end where the drag takes it.
fn said_unreachable(names: &[String]) -> String {
    tf("Constraints are not satisfied: {} can't put it where the drag takes it.", &[&names.join(", ")])
}

fn turned() -> String {
    t("The profile can't take this: its lines would cross, overlap or no longer close.").to_string()
}

impl FamilyEditor {
    /// The name of the type previewed.
    fn type_previewed(&self) -> Option<String> {
        self.types.get(self.picked).map(|(n, _)| n.clone())
    }

    /// Why parameter `name` of `f` can't be set by a drag, if it can't.
    fn unsettable(&self, f: &Family, name: &str) -> Option<String> {
        let Some(p) = f.parameters.iter().find(|p| p.name == name) else {
            return Some(tf("{} isn't a parameter of the family: no drag sets it.", &[name]));
        };
        if p.formula.is_some() {
            return Some(tf("{} is worked out by a formula: change the formula in Family Types.", &[name]));
        }
        if p.reporting {
            return Some(tf("{} is a reporting parameter: it reads a dimension and never drives one.", &[name]));
        }
        if p.instance && self.preview_params.contains_key(name) {
            return Some(tf("{} is set by the instance previewed: change it there.", &[name]));
        }
        if !p.instance && self.types.is_empty() {
            return Some(tf("{} is set by the family's types, and it has none: make one in Family Types.", &[name]));
        }
        None
    }

    /// What the drives worked out for `drawn` (as typed) are kept by: the draft, the instance previewed and the type shown.
    /// Worked out once by each caller for all its handles, as it reads the whole family.
    pub(crate) fn drive_key(&self, drawn: &Family) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (serde_json::to_string(drawn).unwrap_or_default(), serde_json::to_string(&self.preview_params).unwrap_or_default()).hash(&mut h);
        self.picked.hash(&mut h);
        h.finish()
    }

    /// How a drag of form `i`'s handle on `target` works, in the family `drawn` (as typed, `key` its [`drive_key`](Self::drive_key)):
    /// worked out once for the draft as it is, and kept while something is being dragged.
    pub(crate) fn drive(&self, key: u64, drawn: &Family, i: usize, target: Target) -> Drive {
        let side = (
            i,
            match target {
                Target::Edge(k, _) => k as i64,
                Target::End(true) => -1,
                Target::End(false) => -2,
            },
        );
        let busy = self.views.param_drag.is_some()
            || self.views.edge_drag.is_some()
            || self.views.end_drag.is_some()
            || self.views.drag3d.is_some();
        {
            let kept = self.views.drives.borrow();
            if (kept.key == key || busy)
                && let Some(d) = kept.by.get(&side)
            {
                return d.clone();
            }
        }
        let d = self.drive_now(drawn, i, target);
        let mut kept = self.views.drives.borrow_mut();
        if kept.key != key && !busy {
            *kept = Drives { key, by: Default::default() };
        }
        kept.by.insert(side, d.clone());
        d
    }

    fn drive_now(&self, drawn: &Family, i: usize, target: Target) -> Drive {
        let Some(form) = drawn.forms.get(i) else { return Drive::Hidden };
        let (held, exprs): (Option<Held>, Vec<&Expr>) = match target {
            Target::Edge(k, _) => {
                let Some(line) = form.sketch.get(k) else { return Drive::Hidden };
                // No label, lock or EQ on it: the side moves alone, a formula of its own kept with the move added.
                let held = if form.guid.is_nil() { None } else { drawn.edge_held(form.guid, line.guid) };
                if held.is_none() {
                    return Drive::Free;
                }
                (held, Vec::new())
            }
            Target::End(end) => {
                let extent = if end { bimcraft_core::family::Extent::End } else { bimcraft_core::family::Extent::Start };
                let held = if form.guid.is_nil() { None } else { drawn.end_held(form.guid, extent) };
                let e = if end { &form.to } else { &form.from };
                if held.is_none() && matches!(e, Expr::Number(_)) {
                    return Drive::Free;
                }
                (held, vec![e])
            }
        };
        let (names, label) = match held {
            Some(Held::Label(p)) => (vec![p.clone()], Some(p)),
            Some(Held::Locked | Held::Equal) => return Drive::Hidden,
            None => (named(drawn, &formulas(exprs)), None),
        };
        // Named and none settable: greyed, the first's reason.
        if !names.is_empty() && names.iter().all(|n| self.unsettable(drawn, n).is_some()) {
            return Drive::Refused(names.iter().find_map(|n| self.unsettable(drawn, n)).unwrap_or_default());
        }
        // Each settable one moved: those that move it, a label's first, then sizes, then positions, as the family lists them.
        let Some(ty) = self.type_previewed() else {
            return Drive::Refused(t("The family has no type for a drag to set a parameter of: make one in Family Types.").to_string());
        };
        let form_id = (form.guid, i);
        let Some(start) = self.probe(drawn, &ty, form_id, target) else { return Drive::Hidden };
        let order = |n: &str| drawn.parameters.iter().position(|p| p.name == n).unwrap_or(usize::MAX);
        let mut moving: Vec<(u8, usize, String)> = Vec::new();
        for n in names.iter().filter(|n| self.unsettable(drawn, n).is_none()) {
            let Some((_, p)) = self.moves(drawn, &ty, form_id, target, n, start) else { continue };
            let (dm, dfar) = (p.m - start.m, p.far - start.far);
            let rank = if label.as_deref() == Some(n.as_str()) {
                0
            } else if dfar.abs() < 1e-6 || dfar * dm < 0.0 {
                1
            } else {
                2
            };
            moving.push((rank, order(n), n.clone()));
        }
        if moving.is_empty() {
            let named = if names.is_empty() { t("its formulas").to_string() } else { names.join(", ") };
            return Drive::Refused(tf("Moving {} leaves this where it is: nothing the family sets places it.", &[&named]));
        }
        moving.sort();
        Drive::Param(moving.into_iter().map(|(_, _, n)| n).collect())
    }

    /// Where `target` of form `form` stands in `f` as type `ty` flexes it (`Probe`): none where it can't be built or read.
    fn probe(&self, f: &Family, ty: &str, form: (Guid, usize), target: Target) -> Option<Probe> {
        let values = f.values(ty, &self.preview_params).ok()?;
        let flexed = f.flexed(&values).ok()?;
        let x = form_of(&flexed, form)?;
        let mut num = reader(&values);
        let p = match target {
            Target::Edge(k, out) => {
                x.loops(&mut num).ok()?;
                let h = hs::edge_handles(&x.sketch, &mut num).into_iter().find(|h| h.line == k)?;
                let along = |q: P2| q.0 * out.0 + q.1 * out.1;
                let far = points(&x.sketch, Some(k), &mut num)?.into_iter().map(along).fold(f64::INFINITY, f64::min);
                Probe { m: along(h.at), far: if far.is_finite() { far } else { 0.0 }, body: swept(&x.sketch, &mut num)? }
            }
            Target::End(end) => {
                let (from, to) = (num(&x.from).ok()?, num(&x.to).ok()?);
                if end { Probe { m: to, far: from, body: to - from } } else { Probe { m: -from, far: -to, body: to - from } }
            }
        };
        (p.m.is_finite() && p.far.is_finite() && p.body.is_finite()).then_some(p)
    }

    /// The first value of `name`, from its own outward over a widening range, that moves `target` from `start`: it and where it
    /// puts it. None where none does.
    fn moves(&self, f: &Family, ty: &str, form: (Guid, usize), target: Target, name: &str, start: Probe) -> Option<(f64, Probe)> {
        let v0 = self.value_in(f, ty, name)?;
        let base = v0.abs().max(100.0);
        for k in [0.01, 0.1, 0.5, 1.0, 2.0, 5.0] {
            for s in [1.0, -1.0] {
                let v = v0 + s * k * base;
                if let Some(p) = self.probe(&with_param(f, ty, name, v), ty, form, target)
                    && (p.m - start.m).abs() > 1e-6
                {
                    return Some((v, p));
                }
            }
        }
        None
    }

    /// Parameter `name`'s number in `f` for type `ty`.
    fn value_in(&self, f: &Family, ty: &str, name: &str) -> Option<f64> {
        match f.values(ty, &self.preview_params).ok()?.get(name)? {
            Val::Number(x) => Some(*x),
            _ => None,
        }
    }

    /// A drag of form `i`'s `target` through `names` begun (the first that moves it), written as it goes when `live`: why
    /// none can be, where none can.
    pub(crate) fn start_param_drag(&mut self, i: usize, target: Target, names: &[String], live: bool) -> Result<(), String> {
        let f = self.family()?;
        let Some(ty) = self.type_previewed() else {
            return Err(t("The family has no type for a drag to set a parameter of: make one in Family Types.").to_string());
        };
        let form = (f.forms.get(i).map(|x| x.guid).unwrap_or_default(), i);
        let start = self.probe(&f, &ty, form, target).ok_or_else(|| said_unreachable(names))?;
        let built = |f: &Family, name: &str| {
            f.values(name, &BTreeMap::new())
                .and_then(|values| {
                    f.solve(&values).map_err(|e| e.to_string())?;
                    f.pieces(&values).map(|_| ())
                })
                .is_ok()
        };
        for name in names {
            let Some(v0) = self.value_in(&f, &ty, name) else { continue };
            let Some((v, p)) = self.moves(&f, &ty, form, target, name, start) else { continue };
            let integer = f.parameters.iter().any(|x| x.name == *name && x.kind == Kind::Integer);
            let built = f.types.keys().map(|n| (n.clone(), built(&f, n))).collect();
            self.views.param_drag = Some(ParamDrag {
                kept: self.draft(),
                family: f,
                ty,
                built,
                form,
                target,
                name: name.clone(),
                integer,
                v0,
                start,
                moved: (v, p.m),
                last: None,
                total: 0.0,
                live,
                pending: None,
            });
            return Ok(());
        }
        Err(said_unreachable(names))
    }

    /// Whether a parameter is being dragged by a handle.
    pub(crate) fn dragging_param(&self) -> bool {
        self.views.param_drag.is_some()
    }

    /// The parameter dragged a step `d` further.
    pub(crate) fn param_by(&mut self, d: f64) -> Result<(), String> {
        let total = self.views.param_drag.as_ref().map_or(0.0, |p| p.total) + d;
        self.param_to(total)
    }

    /// The value of the dragged parameter that puts its side or end at `want`: secant steps from the last two places known,
    /// then a bracket widened outward from where the drag began and halved. None where it can't be reached.
    fn solve(&self, p: &ParamDrag, want: f64) -> Option<f64> {
        // A whole number's parameter is tried at whole numbers only.
        let whole = |v: f64| if p.integer { v.round() } else { v };
        let m = |v: f64| self.probe(&with_param(&p.family, &p.ty, &p.name, whole(v)), &p.ty, p.form, p.target).map(|x| x.m - want);
        const TOL: f64 = 1e-4;
        let g0 = p.start.m - want;
        if g0.abs() <= TOL {
            return Some(p.v0);
        }
        let mut known: Vec<(f64, f64)> = vec![(p.v0, g0), (p.moved.0, p.moved.1 - want)];
        if let Some((v, at)) = p.last {
            known.push((v, at - want));
        }
        // Secant, from the two nearest.
        known.sort_by(|a, b| a.1.abs().total_cmp(&b.1.abs()));
        let (mut a, mut b) = (known[1], known[0]);
        for _ in 0..8 {
            let den = b.1 - a.1;
            if den.abs() < 1e-12 {
                break;
            }
            let v = whole(b.0 - b.1 * (b.0 - a.0) / den);
            if known.iter().any(|k| k.0 == v) {
                break;
            }
            let Some(g) = v.is_finite().then(|| m(v)).flatten() else { break };
            known.push((v, g));
            if g.abs() <= TOL {
                return Some(v);
            }
            (a, b) = (b, (v, g));
        }
        // A bracket: two values either side of it, else widened outward from the start, both ways, then halved.
        let mut bracket = None;
        for x in &known {
            if let Some(y) = known.iter().find(|y| y.1.signum() != x.1.signum()) {
                bracket = Some((*x, *y));
                break;
            }
        }
        if bracket.is_none() {
            let base = p.v0.abs().max(100.0);
            let mut step = base * 0.01;
            'widen: for _ in 0..24 {
                for s in [1.0, -1.0] {
                    let v = p.v0 + s * step;
                    if v.abs() > 1e7 {
                        continue;
                    }
                    if let Some(g) = m(v)
                        && g.signum() != g0.signum()
                    {
                        bracket = Some(((p.v0, g0), (v, g)));
                        break 'widen;
                    }
                }
                step *= 2.0;
            }
        }
        let ((mut lo, mut glo), (mut hi, mut ghi)) = bracket?;
        for _ in 0..60 {
            // Whole numbers a step apart: the nearer.
            if p.integer && (hi - lo).abs() <= 1.0 {
                return Some(if ghi.abs() < glo.abs() { hi } else { lo });
            }
            let mid = whole((lo + hi) / 2.0);
            let g = m(mid)?;
            if g.abs() <= TOL || (hi - lo).abs() < 1e-6 {
                return Some(mid);
            }
            if g.signum() == glo.signum() {
                (lo, glo) = (mid, g);
            } else {
                (hi, ghi) = (mid, g);
            }
        }
        None
    }

    /// The parameter dragged so its side or end stands `d` along the handle's way from where it began: the value found (written
    /// as the drag goes in a plan or an elevation, waiting for the button in {3D}), said in the status bar; refused, the draft
    /// as the drag began, saying why.
    pub(crate) fn param_to(&mut self, d: f64) -> Result<(), String> {
        let Some(p) = self.views.param_drag.as_mut() else { return Ok(()) };
        if !d.is_finite() {
            return Ok(());
        }
        p.total = d;
        p.pending = None;
        let p = p.clone();
        if p.live {
            self.put_back(p.kept.clone());
        }
        if d.abs() < 1e-9 {
            return Ok(());
        }
        let refuse = |me: &mut FamilyEditor, why: String| {
            if p.live {
                me.put_back(p.kept.clone());
            }
            Err(why)
        };
        let want = p.start.m + d;
        let Some(v) = self.solve(&p, want) else { return refuse(self, said_unreachable(std::slice::from_ref(&p.name))) };
        // A whole number where the parameter is one (the nearer of the two either side), else to a hundredth.
        let v = if p.integer {
            let near = |v: f64| self.probe(&with_param(&p.family, &p.ty, &p.name, v), &p.ty, p.form, p.target).map(|x| (x.m - want).abs());
            let (lo, hi) = (v.floor(), v.ceil());
            match (near(lo), near(hi)) {
                (Some(a), Some(b)) if b < a => hi,
                (Some(_), _) => lo,
                (None, Some(_)) => hi,
                (None, None) => return refuse(self, said_unreachable(std::slice::from_ref(&p.name))),
            }
        } else {
            (v * 100.0).round() / 100.0
        };
        let g = with_param(&p.family, &p.ty, &p.name, v);
        // The profile not turned inside out nor left nothing, the form not left without depth.
        let at = match self.probe(&g, &p.ty, p.form, p.target) {
            Some(x) if x.body.signum() == p.start.body.signum() && x.body.abs() > 1e-6 => x.m,
            _ => return refuse(self, turned()),
        };
        // Every type that built as the drag began still builds with it, as Finish builds a form.
        for (name, before) in &p.built {
            let after = g.values(name, &BTreeMap::new()).and_then(|values| {
                g.solve(&values).map_err(|e| e.to_string())?;
                g.pieces(&values).map(|_| ())
            });
            if let (true, Err(e)) = (before, after) {
                return refuse(self, format!("{name}: {e}"));
            }
        }
        if let Some(q) = self.views.param_drag.as_mut() {
            q.last = Some((v, at));
            q.pending = Some(v);
        }
        if p.live {
            self.write_param(&p.name, v);
        }
        self.views.said = Some(tf("{} = {}", &[&p.name, &number_text(v)]));
        Ok(())
    }

    fn write_param(&mut self, name: &str, v: f64) {
        self.apply_edits(vec![Edit::Parameter(name.to_string(), v)]);
    }

    /// The parameter's drag let go: written as it was found ({3D}'s, now), a step of the history once the button is up.
    pub(crate) fn let_param_go(&mut self) {
        if let Some(p) = self.views.param_drag.take()
            && !p.live
            && let Some(v) = p.pending
        {
            self.write_param(&p.name, v);
            self.views.said = Some(tf("{} = {}", &[&p.name, &number_text(v)]));
        }
    }

    /// The parameter's drag given up: the draft as it began. Whether there was one.
    pub(super) fn cancel_param(&mut self) -> bool {
        let Some(p) = self.views.param_drag.take() else { return false };
        if p.live {
            self.put_back(p.kept);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A parameter is named by a formula as a whole name: `a` isn't read by `abs(b)` nor by `ab`, and a name that holds
    /// another (`Width` and `Width2`) is told apart.
    #[test]
    fn a_formula_names_its_parameters_whole() {
        let f: Family = serde_json::from_value(serde_json::json!({
            "types": {"T": {}},
            "parameters": [{"name": "a", "value": 1}, {"name": "b", "value": 2}, {"name": "ab", "value": 3},
                {"name": "Width", "value": 4}, {"name": "Width2", "value": 5}]
        }))
        .unwrap();
        assert_eq!(named(&f, &["abs(b)"]), ["b"]);
        assert_eq!(named(&f, &["ab + 1"]), ["ab"]);
        assert_eq!(named(&f, &["Width2 / 2", "a * 2"]), ["a", "Width2"]);
        assert!(named(&f, &["(("]).is_empty(), "a formula that doesn't parse names none");
    }
}
