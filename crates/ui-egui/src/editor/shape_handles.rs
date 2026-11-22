//! Shape handles on a selected form's profile. With one extrusion selected (solid or void) or a Blend (its
//! base), a plan or an elevation that sees its plane flat shows a handle at the middle of each edge of its profile, pointing out
//! of the solid; an elevation that sees an extrusion's plane edge-on shows one at mid-depth on each side it sees edge-on. A drag
//! offsets that edge along its normal, the lines at its ends following (`family::handles`), or changes an arc's radius: live,
//! one step of the document's history. That side alone moves: one whose numbers are formulas keeps them, the move
//! added. A drag that would leave no profile, or that the constraints would undo, is refused saying why, and the form is left
//! as it was.
//!
//! A side or an end dragged to within [`SNAP`] points of a reference plane, or of another form's side or end that
//! faces its way, lines up with it (`Family::handle_targets`); let go there in a plan or an elevation, Align's padlock is
//! offered open beside it, and a click on it locks the two (a drag through a parameter offers none: its label holds it).
use super::handle_params::Target;
use super::sketch::{edge_on, flat, looking, reader};
use super::*;
use bimcraft_core::family::forms::FormKind;
use bimcraft_core::family::handles::{self as hs, Held, Refused};
use bimcraft_core::family::sketch::FormLine;
use bimcraft_core::family::views::{Staged, Which};
use bimcraft_core::family::{DimRef, Extent};
use bimcraft_core::geom::P2;

/// A handle on an edge of the selected form's profile, as a view shows it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShapeHandle {
    /// Where it shows, in the view's millimetres (in {3D}, where it is on the profile's plane, its own axes).
    pub(crate) at: P2,
    /// The way out of the solid, in the family's axes: what a drag moves it along.
    pub(crate) way: [f64; 3],
    /// The line's place in the form's sketch, and the way out in the plane's own axes.
    pub(crate) line: usize,
    pub(crate) out: P2,
    /// Why it can't be dragged, if it can't: it is drawn greyed.
    pub(crate) blocked: Option<String>,
    /// The parameters its drag goes through, where its place is theirs: the first that moves it.
    pub(crate) param: Option<Vec<String>>,
}

/// A profile edge being dragged by its handle: the form, its line, the way out (the plane's axes and the family's), where the
/// button went down (the view's millimetres), the form as it was, its sketch as the type previewed shows it, and how far it
/// has gone.
#[derive(Clone)]
pub(crate) struct EdgeDrag {
    /// The view it is dragged in ({3D} none), and the last pass that view was drawn in: shown no longer, the drag is given up.
    pub(super) which: Option<Which>,
    pub(super) seen: u64,
    line: usize,
    out: P2,
    way: [f64; 3],
    /// In a plan or an elevation, where the button went down and the view as the drag began: the pointer is read through it
    /// while the drag goes on. {3D} reads it itself (`handles3d.rs`).
    pub(super) on: Option<(P2, bimcraft_core::render::View)>,
    before: FormText,
    shown: Vec<FormLine>,
    last: f64,
}

impl EdgeDrag {
    /// The way out of the solid it is dragged along, in the family's axes.
    pub(super) fn way(&self) -> [f64; 3] {
        self.way
    }

    /// The line of the sketch it drags.
    pub(super) fn line(&self) -> usize {
        self.line
    }
}

/// How near, in points, a dragged side or end comes to what it can line up with before it is taken onto it.
pub(crate) const SNAP: f32 = 8.0;

/// What the side or end being dragged by its handle can line up with: itself as a reference, where it stood along
/// its way as the drag began, how near (mm) takes it, the references that face its way with where each is, how far the hand
/// has taken it (an End's drag in a plan or an elevation comes a step at a time) and how far it has been put, what it is
/// lined up with now, and whether a padlock may be offered (a drag through a parameter offers none).
#[derive(Clone, Debug)]
pub(crate) struct HandleSnap {
    own: DimRef,
    way: [f64; 3],
    from: f64,
    reach: f64,
    targets: Vec<(DimRef, f64)>,
    raw: f64,
    done: f64,
    /// What it is lined up with now, and where that is along its way.
    on: Option<(DimRef, f64)>,
    free: bool,
}

impl HandleSnap {
    /// Whether it is lined up with something now.
    pub(super) fn on(&self) -> bool {
        self.on.is_some()
    }
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// What a refused drag says.
fn said(r: Refused) -> String {
    match r {
        Refused::Invalid => t("The profile can't take this: its lines would cross, overlap or no longer close.").to_string(),
    }
}

/// What a constraint holding an edge or an end says, `what` naming it.
pub(crate) fn held_said(h: &Held, end: bool) -> String {
    match (h, end) {
        (Held::Label(p), false) => tf("Dimension label {} sets this edge: change it in Family Types.", &[p]),
        (Held::Label(p), true) => tf("Dimension label {} sets this end: change it in Family Types.", &[p]),
        (Held::Locked, false) => t("This edge is locked: unlock it to drag it.").to_string(),
        (Held::Locked, true) => t("This end is locked: unlock it to drag it.").to_string(),
        (Held::Equal, false) => t("An equality (EQ) holds this edge: take it off to drag it.").to_string(),
        (Held::Equal, true) => t("An equality (EQ) holds this end: take it off to drag it.").to_string(),
    }
}

impl FamilyEditor {
    /// Whether a view's handles show: no tool in hand (a Modify tool's clicks are its own), nothing being sketched
    /// or placed.
    pub(crate) fn handles_shown(&self) -> bool {
        self.sketch.is_none()
            && self.views.modify.is_none()
            && self.adaptive.is_none()
            && self.placing.is_none()
            && self.views.drawing.is_none()
            && self.views.picking.is_none()
            && self.views.pivoting.is_none()
            && self.views.dimensioning.is_none()
            && self.views.aligning.is_none()
    }

    /// The selected extrusion's (or Blend's) profile handles in view `which`: `drawn` is the family as drawn, `shown` as the type
    /// previewed flexes it, which they sit on. In a view that sees its plane flat, one on each edge; in one that sees it
    /// edge-on (an extrusion's), one at mid-depth on each straight side seen edge-on and in sight.
    pub(crate) fn shape_handles(&self, drawn: &Family, shown: &Family, st: &Staged, which: Which) -> Vec<ShapeHandle> {
        if !self.handles_shown() {
            return Vec::new();
        }
        let Some(i) = self.selected_form() else { return Vec::new() };
        // A form the view hides (Hide in View, Temporary Hide/Isolate): no handles.
        if self.temp_hidden_form(i) {
            return Vec::new();
        }
        let Some(form) = drawn.forms.get(i) else { return Vec::new() };
        let kind = form.kind();
        if !matches!(kind, FormKind::Extrusion | FormKind::Blend) || !form.mesh.is_empty() {
            return Vec::new();
        }
        let flexed = if form.guid.is_nil() { shown.forms.get(i) } else { shown.forms.iter().find(|f| f.guid == form.guid) };
        let Some(flexed) = flexed else { return Vec::new() };
        let Ok(frame) = shown.frame(&flexed.host) else { return Vec::new() };
        let values = self.values();
        let mut num = reader(&values);
        let (sees_flat, sides) = (flat(which, &frame), kind == FormKind::Extrusion && edge_on(which, &frame));
        if !sees_flat && !sides {
            return Vec::new();
        }
        let (Ok(from), Ok(to)) = (num(&flexed.from), num(&flexed.to)) else { return Vec::new() };
        let look = looking(which);
        let mut out: Vec<ShapeHandle> = Vec::new();
        let key = self.drive_key(drawn);
        for h in hs::edge_handles(&flexed.sketch, &mut num) {
            let way: [f64; 3] = std::array::from_fn(|k| frame.x[k] * h.out.0 + frame.y[k] * h.out.1);
            let at = if sees_flat {
                st.to_view(which, frame.point(h.at, to.max(from)))
            } else {
                // A side seen edge-on: straight, its way out square to the way the view looks, and not behind another part
                // of the profile.
                let toward = P2(-dot3(look, frame.x), -dot3(look, frame.y)).unit();
                if h.round || dot3(way, look).abs() > 1e-6 || !hs::in_sight(&flexed.sketch, &h, toward, &mut num) {
                    continue;
                }
                st.to_view(which, frame.point(h.at, (from + to) / 2.0))
            };
            // Two sides one behind the other show one handle, the first's.
            if out.iter().any(|o| (o.at.0 - at.0).hypot(o.at.1 - at.1) < 1e-6) {
                continue;
            }
            // Tied to a parameter, its drag goes through it; a lock or EQ holding it, no handle.
            let (blocked, param) = match self.drive(key, drawn, i, super::handle_params::Target::Edge(h.line, h.out)) {
                super::handle_params::Drive::Free => (None, None),
                super::handle_params::Drive::Param(names) => (None, Some(names)),
                super::handle_params::Drive::Refused(why) => (Some(why), None),
                super::handle_params::Drive::Hidden => continue,
            };
            out.push(ShapeHandle { at, way, line: h.line, out: h.out, blocked, param });
        }
        out
    }

    /// A drag on handle `h` begun at `on` (the view's millimetres and the view as it was then; none in {3D}) in view `which`,
    /// as the type previewed shows the form (`shown`): what the views keep while it goes on, or why it can't be dragged.
    pub(crate) fn edge_drag(
        &self,
        h: &ShapeHandle,
        (on, which, seen): (Option<(P2, bimcraft_core::render::View)>, Option<Which>, u64),
        shown: &Family,
    ) -> Result<EdgeDrag, String> {
        if let Some(why) = &h.blocked {
            return Err(why.clone());
        }
        let i = self.selected_form().ok_or_else(String::new)?;
        let before = self.forms.get(i).cloned().ok_or_else(String::new)?;
        let guid = before.original.guid;
        let flexed = if guid.is_nil() { shown.forms.get(i) } else { shown.forms.iter().find(|f| f.guid == guid) };
        let shown = flexed.map(|f| f.sketch.clone()).unwrap_or_default();
        Ok(EdgeDrag { which, seen, line: h.line, out: h.out, way: h.way, on, before, shown, last: 0.0 })
    }

    /// The edge being dragged, the pointer `moved` (the family's mm) from where the button went down: the form made again from
    /// what it was with the edge that far out along its normal, where the profile and every type take it; else left as it
    /// was, saying why.
    pub(crate) fn drag_edge_to(&mut self, moved: [f64; 3]) -> Result<(), String> {
        let Some(way) = self.views.edge_drag.as_ref().map(|d| d.way) else { return Ok(()) };
        // Up to something that faces its way, it lines up with it; refused there, it lines up with nothing.
        let d = self.snap_to(dot3(moved, way));
        if self.views.param_drag.is_some() {
            let done = self.param_to(d);
            if done.is_err() {
                self.snap_refused();
            }
            return done;
        }
        let Some(drag) = self.views.edge_drag.as_mut() else { return Ok(()) };
        if !d.is_finite() || (d - drag.last).abs() < 1e-9 {
            return Ok(());
        }
        drag.last = d;
        let drag = drag.clone();
        // The form by what it is known by while the document is open, not by its place, which an undo may give another.
        let Some(i) = self.forms.iter().position(|f| f.tag == drag.before.tag) else {
            self.views.edge_drag = None;
            return Ok(());
        };
        // Back where it began: the form exactly as it was typed, so the drag leaves nothing to save.
        if d.abs() < 1e-9 {
            self.forms[i] = drag.before.clone();
            return Ok(());
        }
        let done = self.offset_edge(&drag, i, d);
        if done.is_err() {
            self.forms[i] = drag.before.clone();
            self.snap_refused();
        }
        done
    }

    /// The Start or End drag, the edge drag and the selection's move given up, each form put back as it was before it began (so no step of the
    /// history): Escape, Undo and Redo, and the view it began in shown no longer. Whether there was one.
    pub(crate) fn cancel_drags(&mut self) -> bool {
        self.views.handle_snap = None;
        // The selection being moved too.
        let mut any = self.cancel_move();
        // An End or a Start held in {3D}; a side held there is the edge drag below.
        any |= self.cancel_3d();
        if let Some(d) = self.views.edge_drag.take() {
            if let Some(i) = self.forms.iter().position(|f| f.tag == d.before.tag) {
                self.forms[i] = d.before;
            }
            any = true;
        }
        if let Some(d) = self.views.end_drag.take() {
            if let Some(f) = self.forms.iter_mut().find(|f| f.tag == d.tag) {
                *(if d.end { &mut f.to } else { &mut f.from }) = d.before;
            }
            any = true;
        }
        // A parameter dragged by a handle: the draft as the drag began.
        any |= self.cancel_param();
        any
    }

    /// A drag of the selected form's `target` along `way` (the family's axes) begun: what it can line up with, within
    /// `reach` mm; `free` where no parameter is dragged, so a padlock may be offered. None for a form or a line with no GUID
    /// yet, which nothing can refer to.
    pub(crate) fn begin_snap(&mut self, target: Target, way: [f64; 3], reach: f64, free: bool) {
        self.views.handle_snap = self.snap_for(target, way, reach, free);
    }

    fn snap_for(&self, target: Target, way: [f64; 3], reach: f64, free: bool) -> Option<HandleSnap> {
        let i = self.selected_form()?;
        let f = self.family().ok()?;
        let form = f.forms.get(i).filter(|x| !x.guid.is_nil())?;
        let own = match target {
            Target::Edge(k, _) => DimRef::Sketch(form.guid, form.sketch.get(k).map(|l| l.guid).filter(|g| !g.is_nil())?),
            Target::End(end) => DimRef::Form(form.guid, if end { Extent::End } else { Extent::Start }),
        };
        let values = self.values();
        let flexed = f.flexed(&values).ok()?;
        let from = bimcraft_core::family::constraints::place(&flexed, &own, &values, way).ok().filter(|x| x.is_finite())?;
        // What the view hides is nothing to line up with.
        let hidden: Vec<bimcraft_core::ids::Guid> =
            f.forms.iter().enumerate().filter(|(j, _)| self.temp_hidden_form(*j)).map(|(_, x)| x.guid).collect();
        let shown = |r: &DimRef| match *r {
            DimRef::Plane(g) => !self.temp_hidden_plane(g),
            DimRef::Form(g, _) | DimRef::Sketch(g, _) => !hidden.contains(&g),
            _ => true,
        };
        let targets = f.handle_targets(&values, way, form.guid, &shown);
        let snap = HandleSnap { own, way, from, reach, targets, raw: 0.0, done: 0.0, on: None, free };
        (reach.is_finite() && reach > 0.0).then_some(snap)
    }

    /// The side or end dragged `d` mm along its way: as far as takes it onto what it lines up with there, else `d`.
    pub(crate) fn snap_to(&mut self, d: f64) -> f64 {
        let Some(s) = self.views.handle_snap.as_mut() else { return d };
        s.on = hs::snapped(&s.targets, s.from + d, s.reach);
        s.on.map_or(d, |(_, at)| at - s.from)
    }

    /// The drag refused where it would have lined up: lined up with nothing, so no padlock is offered and nothing says so.
    pub(crate) fn snap_refused(&mut self) {
        if let Some(s) = self.views.handle_snap.as_mut() {
            s.on = None;
        }
    }

    /// What a padlock would lock, the reference first: what the side or end is lined up with, where no parameter is dragged
    /// and the form, as the type previewed builds it, stands there now.
    pub(crate) fn snap_lock(&self) -> Option<(DimRef, DimRef)> {
        let s = self.views.handle_snap.as_ref().filter(|s| s.free)?;
        let (on, at) = s.on?;
        let f = self.family().ok()?;
        let values = self.values();
        let flexed = f.flexed(&values).ok()?;
        let now = bimcraft_core::family::constraints::place(&flexed, &s.own, &values, s.way).ok()?;
        ((now - at).abs() <= 1e-3).then_some((on, s.own))
    }

    /// The side or end dragged a step `d` further (an End's drag in a plan or an elevation comes a step at a time): the step
    /// that puts it where the whole drag, lined up, has it.
    pub(crate) fn snap_step(&mut self, d: f64) -> f64 {
        let Some(s) = self.views.handle_snap.as_mut() else { return d };
        s.raw += d;
        let (raw, done) = (s.raw, s.done);
        let to = self.snap_to(raw);
        if let Some(s) = self.views.handle_snap.as_mut() {
            s.done = to;
        }
        to - done
    }

    /// Whether the side or end being dragged is lined up with something now.
    pub(crate) fn lined_up(&self) -> bool {
        self.views.handle_snap.as_ref().is_some_and(HandleSnap::on)
    }

    fn offset_edge(&mut self, drag: &EdgeDrag, i: usize, d: f64) -> Result<(), String> {
        let values = self.values();
        let mut num = reader(&values);
        let before = drag.before.form()?;
        let lines = hs::offset_edge(&before.sketch, drag.line, drag.out, d, &mut num).map_err(said)?;
        let form = Form { sketch: lines, ..before };
        self.forms[i] = FormText { tag: drag.before.tag, ..FormText::of(&form) };
        let f = self.family()?;
        // Constraints flex it: the type previewed shows the edge where the drag put it, or a lock or a dimension holds it.
        if f.constrained() {
            let expected = hs::offset_edge(&drag.shown, drag.line, drag.out, d, &mut num).map_err(said)?;
            let flexed = f.flexed(&values).map_err(|e| e.to_string())?;
            let now = flexed.forms.iter().find(|x| x.guid == form.guid && !form.guid.is_nil()).or_else(|| flexed.forms.get(i));
            if !now.is_some_and(|x| hs::same_place(&x.sketch, &expected, 1e-3, &mut num)) {
                return Err(t("Constraints are not satisfied: a lock or a dimension holds the profile where it is.").to_string());
            }
        }
        // Each type built with it, as Finish builds a form.
        let mine = format!("form {}:", i + 1);
        for name in f.types.keys() {
            let values = f.values(name, &BTreeMap::new())?;
            if let Err(e) = f.solve(&values) {
                return Err(format!("{name}: {e}"));
            }
            if let Err(e) = f.pieces(&values)
                && e.contains(&mine)
            {
                return Err(format!("{name}: {e}"));
            }
        }
        Ok(())
    }
}
