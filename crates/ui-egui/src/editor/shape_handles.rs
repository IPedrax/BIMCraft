//! FE-5: shape handles on a selected form's profile. With one extrusion selected (solid or void) or a Blend (its
//! base), a plan or an elevation that sees its plane flat shows a handle at the middle of each edge of its profile, pointing out
//! of the solid; an elevation that sees an extrusion's plane edge-on shows one at mid-depth on each side it sees edge-on. A drag
//! offsets that edge along its normal, the lines at its ends following (`family::handles`), or changes an arc's radius: live,
//! one step of the document's history. An edge a lock, a labelled dimension or EQ holds, or whose numbers are formulas, is
//! greyed and refuses a drag, saying why; so does a drag that would leave no profile, or that the constraints would undo,
//! and the form is left as it was.
use super::sketch::{edge_on, flat, looking, reader};
use super::*;
use bimcraft_core::family::forms::FormKind;
use bimcraft_core::family::handles::{self as hs, Held, Refused};
use bimcraft_core::family::sketch::FormLine;
use bimcraft_core::family::views::{Staged, Which};
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
    /// The parameters its drag goes through, where its place is theirs (FE-25): the first that moves it.
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
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// What a refused drag says.
fn said(r: Refused) -> String {
    match r {
        Refused::Tied => t("This edge is tied to a parameter or worked out by a formula: change it in Family Types.").to_string(),
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

/// Why line `k`'s handle (out of the solid `out`) is greyed, if it is: a number a drag of it would move is a formula or tied
/// to a parameter, either way. Where a drag would take the profile (lines crossing, an arc below nothing) is each drag's own
/// question, refused then (`offset_edge`), never a reason to grey the handle.
pub(crate) fn greyed(lines: &[FormLine], k: usize, out: P2, num: &mut dyn FnMut(&Expr) -> Result<f64, String>) -> Option<String> {
    let probes = [1.0, -1.0].map(|d| hs::offset_lines(lines, k, out, d, num));
    if probes.iter().any(Result::is_ok) {
        return None;
    }
    probes.into_iter().find(|p| *p == Err(Refused::Tied)).and_then(Result::err).map(said)
}

impl FamilyEditor {
    /// Whether a view's handles show: no tool in hand, nothing being sketched or placed.
    pub(crate) fn handles_shown(&self) -> bool {
        self.sketch.is_none()
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
            // Tied to a parameter, its drag goes through it; a lock or EQ holding it, no handle (FE-25).
            let (blocked, param) = match self.drive(drawn, i, super::handle_params::Target::Edge(h.line, h.out)) {
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
        let Some(drag) = self.views.edge_drag.as_mut() else { return Ok(()) };
        let d = dot3(moved, drag.way);
        if self.views.param_drag.is_some() {
            return self.param_to(d);
        }
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
        }
        done
    }

    /// The Start or End drag, the edge drag and the selection's move given up, each form put back as it was before it began (so no step of the
    /// history): Escape, Undo and Redo, and the view it began in shown no longer. Whether there was one.
    pub(crate) fn cancel_drags(&mut self) -> bool {
        // The selection being moved too (FE-7).
        let mut any = self.cancel_move();
        // An End or a Start held in {3D} (FE-25); a side held there is the edge drag below.
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
        // A parameter dragged by a handle: the draft as the drag began (FE-25).
        any |= self.cancel_param();
        any
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
