//! The Dimension tool while a form is sketched. In a plan or an elevation that sees the
//! sketch's plane flat, Aligned (DI, DD), Angular (AN), Radial and Diameter dimensions pick the sketch's own lines and circles
//! (unfinished: each line has its GUID as it is drawn), reference planes, reference lines and other forms' lines, as
//! they do outside sketch mode (`dimensions.rs`). What is made, changed or taken off while sketching is held with the sketch
//! (`FormSketch::dims`, `changed`, `unlocked`, as Align's `locks`) and drawn with it, with the draft's own dimensions that
//! measure the form's lines; a label, a lock, EQ or a value typed in drives the sketch's lines at once through the solve
//! Finish checks (`Family::flexed`, for the type previewed), or is refused saying why. Finish writes them with the form, one
//! step of the history, and refuses (sketch mode staying open) what a type can't hold; Cancel drops them all.
use super::dimensions::{self as dims, Clicked, DimMark};
use super::sketch::{Part, flat, reader};
use super::*;
use bimcraft_core::curves::Curve;
use bimcraft_core::family::constraints::Unsat;
use bimcraft_core::family::views::{Staged, Which};
use bimcraft_core::family::{DimKind, DimRef, FamilyDimension, Val};
use bimcraft_core::geom::P2;
use bimcraft_core::ids::Guid;
use bimcraft_core::model::Lock;
use egui::Pos2;
use std::collections::BTreeSet;

/// The sketch's dimensions as a view shows them: the family as Finish would make it now (`FamilyEditor::sketch_family`), as
/// the type previewed flexes it and its values, and the marks of the dimensions the sketch shows (and of the one being
/// placed, past the family's last).
pub(super) struct SketchDims {
    pub(super) flexed: Family,
    pub(super) values: BTreeMap<String, Val>,
    pub(super) marks: Vec<DimMark>,
}

impl SketchDims {
    /// The marks' lines and ticks on screen: what Align's padlocks keep off.
    pub(super) fn lines(&self, screen: &dyn Fn(P2) -> Pos2) -> Vec<[Pos2; 2]> {
        let mut out = Vec::new();
        for m in &self.marks {
            if let (Some(a), Some(b)) = (m.points.first(), m.points.last()) {
                out.push([screen(*a), screen(*b)]);
            }
        }
        out
    }
}

/// Whether curves `a` and `b` are the same line, to a millionth of a millimetre.
pub(super) fn same(a: &Curve, b: &Curve) -> bool {
    let (p, q) = (a.handles(), b.handles());
    matches!(a, Curve::Circle { .. }) == matches!(b, Curve::Circle { .. })
        && p.len() == q.len()
        && p.iter().zip(&q).all(|(x, y)| x.dist(*y) <= 1e-6)
        && (a.length() - b.length()).abs() <= 1e-6
}

impl FamilyEditor {
    /// The dimensions as Finish would write them now: the draft's, less those taken off while sketching and as they were
    /// changed then, with the locks and the dimensions made while sketching; a line of the form gone from the sketch (deleted,
    /// or taken back by Ctrl+Z) taken from those that measure to it, as `Family::forget_lines_gone` takes a line gone at
    /// Finish (one left with too few references goes).
    pub(crate) fn sketch_dimensions(&self) -> Vec<FamilyDimension> {
        let Some(s) = self.sketch.as_ref() else { return self.dimensions.clone() };
        let out = self.sketch_dimensions_whole();
        let now: BTreeSet<Guid> = s.sketch.lines.iter().chain(s.parts.iter().flatten()).map(|l| l.guid).collect();
        let gone: BTreeSet<Guid> = out
            .iter()
            .flat_map(|d| d.refs.iter())
            .filter_map(|r| match r {
                DimRef::Sketch(f, l) | DimRef::Point(f, l, _) if *f == s.guid && !now.contains(l) => Some(*l),
                _ => None,
            })
            .collect();
        let mut f = Family { dimensions: out, ..Family::default() };
        for g in gone {
            f.forget_reference(g);
        }
        f.dimensions
    }

    /// [`FamilyEditor::sketch_dimensions`] before the lines gone are taken from them: each with every reference it holds.
    fn sketch_dimensions_whole(&self) -> Vec<FamilyDimension> {
        let Some(s) = self.sketch.as_ref() else { return self.dimensions.clone() };
        let mut out: Vec<FamilyDimension> = self
            .dimensions
            .iter()
            .filter(|d| !s.unlocked.contains(&d.guid))
            .map(|d| s.changed.iter().find(|c| c.guid == d.guid).unwrap_or(d).clone())
            .collect();
        out.extend(s.locks.iter().cloned());
        out.extend(s.dims.iter().cloned());
        out
    }

    /// The dimension selected while sketching let go once it is gone (taken back, deleted, its line gone), so the Options Bar
    /// and Delete never act on one that isn't there.
    pub(crate) fn prune_selected_dim(&mut self) {
        let Some(g) = self.sketch.as_ref().and_then(|s| s.selected_dim) else { return };
        if !self.sketch_dimensions().iter().any(|d| d.guid == g)
            && let Some(s) = self.sketch.as_mut()
        {
            s.selected_dim = None;
        }
    }

    /// The dimensions in effect: the sketch's while a form is sketched ([`FamilyEditor::sketch_dimensions`]), else the draft's.
    pub(crate) fn dimensions_now(&self) -> Vec<FamilyDimension> {
        self.sketch_dimensions()
    }

    /// The family as Finish would make it now, unchecked: the draft with the form being sketched made of its lines as they
    /// are, the dimensions in effect and the parameters made while sketching.
    pub(crate) fn sketch_family(&self) -> Result<Family, String> {
        let (index, form, _) = self.sketched_form()?;
        self.family_with_form(index, form)
    }

    /// The draft with `form` at `index` (a new one last), the dimensions in effect and the parameters made while sketching.
    pub(crate) fn family_with_form(&self, index: Option<usize>, form: Form) -> Result<Family, String> {
        let s = self.sketch.as_ref().ok_or_else(|| t("Nothing is being sketched.").to_string())?;
        let mut f = self.family()?;
        for p in &s.params {
            f.parameters.push(p.parameter()?);
        }
        match index {
            Some(i) if i < f.forms.len() => f.forms[i] = form,
            _ => f.forms.push(form),
        }
        f.dimensions = self.sketch_dimensions();
        Ok(f)
    }

    /// The values of the type previewed for `f` (the sketch's family, with the parameters made while sketching).
    pub(crate) fn sketch_values(&self, f: &Family) -> BTreeMap<String, Val> {
        self.types.get(self.picked).and_then(|(name, _)| f.values(name, &self.preview_params).ok()).unwrap_or_default()
    }

    /// The parameters made while sketching, for a label: none outside sketch mode.
    pub(crate) fn sketch_params(&self) -> Vec<(String, Holds)> {
        self.sketch.as_ref().map(|s| s.params.iter().map(|p| (p.name.trim().to_string(), p.kind)).collect()).unwrap_or_default()
    }

    /// Where a parameter made for a label goes: the sketch's while a form is sketched, else the draft's.
    pub(super) fn params_held(&mut self) -> &mut Vec<ParamText> {
        match self.sketch.as_mut() {
            Some(s) => &mut s.params,
            None => &mut self.params,
        }
    }

    /// The sketch's dimensions with `change` made to them, if the constraints still solve for the type previewed: the sketch's
    /// lines moved to where they put them (a label of another length, EQ, a lock), what changed held with the sketch. Else
    /// why not, nothing changed.
    pub(super) fn sketch_try(&mut self, change: impl FnOnce(&mut Vec<FamilyDimension>)) -> Result<(), Unsat> {
        let mut f = self.sketch_family().map_err(Unsat::from)?;
        let before = f.dimensions.clone();
        change(&mut f.dimensions);
        // One that has lost a line gone from the sketch keeps the references it holds until Finish: it isn't changed then.
        let whole = self.sketch_dimensions_whole();
        let lost = |d: &FamilyDimension| whole.iter().any(|w| w.guid == d.guid && w.refs != d.refs);
        if f.dimensions.iter().any(|d| !before.contains(d) && lost(d)) {
            return Err(Unsat::from(
                t("This dimension measures to a line that is gone: draw the line again (Ctrl+Z) or delete the dimension.").to_string(),
            ));
        }
        let values = self.sketch_values(&f);
        let flexed = f.flexed(&values)?.into_owned();
        self.keep_sketch_dimensions(&before, &f.dimensions);
        self.drive_sketch(&flexed, &values);
        Ok(())
    }

    /// The dimensions of `after` that aren't as they are in `before` held with the sketch: one made while sketching (or an
    /// Align lock) changed where it is, one of the draft's among those changed, any other a new one.
    fn keep_sketch_dimensions(&mut self, before: &[FamilyDimension], after: &[FamilyDimension]) {
        let draft: Vec<Guid> = self.dimensions.iter().map(|d| d.guid).collect();
        let Some(s) = self.sketch.as_mut() else { return };
        for d in after.iter().filter(|d| !before.contains(d)) {
            if let Some(x) = s.dims.iter_mut().find(|x| x.guid == d.guid) {
                *x = d.clone();
            } else if let Some(x) = s.locks.iter_mut().find(|x| x.guid == d.guid) {
                *x = d.clone();
            } else if draft.contains(&d.guid) {
                s.changed.retain(|x| x.guid != d.guid);
                s.changed.push(d.clone());
            } else {
                s.dims.push(d.clone());
            }
        }
    }

    /// The sketch's lines put where `flexed` (the sketch's family solved for `values`) has them: what a label, a lock or EQ
    /// moved, a step the sketch's Ctrl+Z takes back.
    fn drive_sketch(&mut self, flexed: &Family, values: &BTreeMap<String, Val>) {
        let Some(s) = self.sketch.as_ref() else { return };
        let Some(form) = flexed.forms.iter().find(|f| f.guid == s.guid) else { return };
        let mut num = reader(values);
        let moved: Vec<(Guid, Curve)> =
            form.plane_lines().filter(|l| !l.guid.is_nil()).filter_map(|l| Some((l.guid, l.curve(&mut num).ok()?.curve))).collect();
        let Some(s) = self.sketch.as_mut() else { return };
        let mut any = false;
        let mut driven = std::mem::take(&mut s.driven);
        for l in s.sketch.lines.iter_mut().chain(s.parts.iter_mut().flatten()) {
            if let Some((_, c)) = moved.iter().find(|(g, _)| *g == l.guid)
                && !same(&l.curve, c)
            {
                // Where it was drawn before the dimensions first moved it, kept for Finish.
                match driven.iter_mut().find(|(g, _, to)| *g == l.guid && same(&l.curve, to)) {
                    Some(entry) => entry.2 = c.clone(),
                    None => {
                        driven.retain(|(g, ..)| *g != l.guid);
                        driven.push((l.guid, l.curve.clone(), c.clone()));
                    }
                }
                l.curve = c.clone();
                any = true;
            }
        }
        s.driven = driven;
        if any {
            let picked = std::mem::take(&mut s.sketch.picked);
            s.sketch.changed();
            s.sketch.picked = picked;
        }
    }

    /// Why the dimension tools can't be taken up while sketching, if they can't: in {3D}, or in a view that doesn't see the
    /// sketch's plane flat (a sweep's profile's too).
    pub(crate) fn sketch_undimensionable(&self) -> Option<&'static str> {
        self.sketch.as_ref()?;
        let Some(which) = self.views.shown.filter(|_| self.adaptive.is_none()) else {
            return Some(msg!("Dimensions are drawn in a plan or an elevation, not in {3D}."));
        };
        match self.sketch_frame() {
            Some(frame) if flat(which, &frame) => None,
            _ => Some(msg!("This view sees the sketch's plane edge-on: dimension it in a view that sees it flat.")),
        }
    }

    /// Aligned, Angular, Radial or Diameter Dimension (`kind`) while sketching, where it can be: the Draw panel's shape, Align
    /// and what is selected put down.
    pub(super) fn sketch_dimension_tool(&mut self, kind: DimKind) {
        if self.sketch_undimensionable().is_some() {
            return;
        }
        let Some(s) = self.sketch.as_mut() else { return };
        s.set_tool(None, false);
        s.align_from = None;
        s.offer = None;
        s.selected_dim = None;
        s.sketch.picked.clear();
        self.views.aligning = None;
        self.views.dimensioning = Some(Vec::new());
        self.views.dim_kind = kind;
    }

    /// Dimension `g` taken away while sketching: one made then dropped, one of the draft's taken off at Finish.
    pub(super) fn delete_sketch_dimension(&mut self, g: Guid) {
        let had = self.dimensions.iter().any(|d| d.guid == g);
        let Some(s) = self.sketch.as_mut() else { return };
        s.dims.retain(|d| d.guid != g);
        s.locks.retain(|d| d.guid != g);
        s.changed.retain(|d| d.guid != g);
        if had && !s.unlocked.contains(&g) {
            s.unlocked.push(g);
        }
        if s.selected_dim == Some(g) {
            s.selected_dim = None;
        }
    }

    /// Dimension `g`'s one segment made `v` (mm, or degrees for an angle) by a value typed while sketching: the sketch's lines
    /// moved to it by the solve, as a lock at `v` would hold them (a locked one stays locked, at `v`). Refused for a labelled
    /// one (its parameter's value says) and where the constraints can't hold it.
    pub(crate) fn set_dimension_value(&mut self, g: Guid, v: f64) -> Result<(), String> {
        let mut f = self.sketch_family()?;
        let Some(at) = f.dimensions.iter().position(|d| d.guid == g) else { return Ok(()) };
        let d = &f.dimensions[at];
        if d.label.is_some() {
            return Err(t("A labelled dimension is its parameter's value: change it in Family Types.").to_string());
        }
        if d.segments() != 1 || d.alignment {
            return Err(t("Only a dimension of one segment takes a value.").to_string());
        }
        if !(v.is_finite() && v > 0.0) {
            return Err(t("A dimension's value is more than 0.").to_string());
        }
        let locked = d.is_locked(0);
        let before = f.dimensions.clone();
        let values = self.sketch_values(&f);
        let base = f.flexed(&values).map(std::borrow::Cow::into_owned).unwrap_or_else(|_| f.clone());
        let d = &mut f.dimensions[at];
        d.locked.retain(|l| l.segment != 0);
        d.locked.push(Lock { segment: 0, length: v });
        d.locked.sort_by_key(|l| l.segment);
        let flexed = f.flexed(&values).map_err(dims::said)?.into_owned();
        // The value is the sketch's to reach: one the solve reaches by moving a plane or a reference line is refused.
        let off = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).any(|(x, y)| (x - y).abs() > 1e-6);
        let planes = base.planes.iter().zip(&flexed.planes).any(|(a, b)| off(a.start, b.start) || off(a.end, b.end));
        let lines = base.lines.iter().zip(&flexed.lines).any(|(a, b)| off(a.start, b.start) || off(a.end, b.end));
        if planes || lines {
            return Err(t(
                "This value would move a reference plane or a reference line, not the sketch: unlock what holds the line, or change the plane.",
            )
            .to_string());
        }
        if locked {
            self.keep_sketch_dimensions(&before, &f.dimensions);
        }
        self.drive_sketch(&flexed, &values);
        Ok(())
    }

    /// The reference a dimension tool of `kind` picks at `p` while sketching: as outside sketch mode; on a sweep's profile, its
    /// own lines alone (the constraints measure a profile's lines among themselves).
    #[allow(clippy::too_many_arguments)]
    fn sketch_pick(
        &self,
        f: &Family,
        values: &BTreeMap<String, Val>,
        st: &Staged,
        which: Which,
        kind: DimKind,
        p: Pos2,
        screen: &dyn Fn(P2) -> Pos2,
        plane_at: &dyn Fn(Pos2) -> Option<Guid>,
    ) -> Option<DimRef> {
        let s = self.sketch.as_ref()?;
        if !matches!(s.part, Part::Profile | Part::End) {
            return dims::pick_reference(f, values, st, which, kind, plane_at(p), p, screen);
        }
        let own = |r: &DimRef| match *r {
            DimRef::Sketch(g, l) | DimRef::Point(g, l, _) => g == s.guid && s.sketch.lines.iter().any(|x| x.guid == l),
            _ => false,
        };
        dims::pick_reference(f, values, st, which, kind, None, p, screen).filter(own)
    }

    /// What the sketch's dimensions are in view `which` (`st` its scratch model, `screen` its millimetres on screen): the
    /// sketch's family flexed for the type previewed, and the marks of the dimensions made while sketching and of the draft's
    /// that measure the form (Align's locks are the sketch's padlocks), with the one being placed toward `hover`. None while
    /// the family doesn't build.
    pub(super) fn sketch_dims(
        &self,
        st: &Staged,
        which: Which,
        screen: &dyn Fn(P2) -> Pos2,
        hover: Option<Pos2>,
        at_view: &dyn Fn(Pos2) -> P2,
        plane_at: &dyn Fn(Pos2) -> Option<Guid>,
    ) -> Option<SketchDims> {
        let s = self.sketch.as_ref()?;
        let family = self.sketch_family().ok()?;
        let values = self.sketch_values(&family);
        let flexed = family.flexed(&values).map(std::borrow::Cow::into_owned).unwrap_or_else(|_| family.clone());
        let kind = self.views.dim_kind;
        let placing = match (&self.views.dimensioning, hover) {
            (Some(refs), Some(h)) => {
                let under = self.sketch_pick(&flexed, &values, st, which, kind, h, screen, plane_at);
                dims::placing(&flexed, &values, st, which, kind, refs, under, at_view(h))
            }
            _ => None,
        };
        let n = flexed.dimensions.len();
        let mut marks = dims::marks_of(&family, &flexed, &values, st, which, placing);
        marks.retain(|m| {
            m.index >= n
                || flexed
                    .dimensions
                    .get(m.index)
                    .is_some_and(|d| !d.alignment && (s.dims.iter().any(|x| x.guid == d.guid) || d.refs.iter().any(|r| r.names(s.guid))))
        });
        Some(SketchDims { flexed, values, marks })
    }

    /// A click at `p` (`q` in the view's mm) on the sketch's dimensions in view `which`: with a dimension tool, a reference
    /// picked or the dimension placed (held with the sketch); with Modify, a dimension's padlock, its EQ toggle, or the
    /// dimension selected (a click on nothing lets it go). Whether the click was theirs, and what went wrong.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn sketch_dims_click(
        &mut self,
        sd: &SketchDims,
        st: &Staged,
        which: Which,
        p: Pos2,
        q: P2,
        screen: &dyn Fn(P2) -> Pos2,
        plane_at: &dyn Fn(Pos2) -> Option<Guid>,
    ) -> (bool, Option<String>) {
        let kind = self.views.dim_kind;
        if let Some(mut refs) = self.views.dimensioning.take() {
            let under = self.sketch_pick(&sd.flexed, &sd.values, st, which, kind, p, screen, plane_at);
            let done = dims::dimension_click(&sd.flexed, &sd.values, st, which, kind, &mut refs, under, q);
            self.views.dimensioning = Some(refs);
            return match done {
                Ok(Some(d)) => {
                    if let Some(s) = self.sketch.as_mut() {
                        s.dims.push(d);
                    }
                    (true, None)
                }
                Ok(None) => (true, None),
                Err(e) => (true, Some(e)),
            };
        }
        let Some(s) = self.sketch.as_ref() else { return (false, None) };
        if s.tool.is_some() || self.views.aligning.is_some() || s.trim.is_some() {
            return (false, None);
        }
        let selected = s.selected_dim;
        let hit = dims::clicked(&sd.marks, &sd.flexed.dimensions, selected, None, &[], p, screen);
        let done = match hit {
            Some(Clicked::Select(g)) => {
                if let Some(s) = self.sketch.as_mut() {
                    s.selected_dim = Some(g);
                }
                Ok(())
            }
            Some(Clicked::Lock(g, i)) => {
                let length = sd
                    .marks
                    .iter()
                    .find(|m| sd.flexed.dimensions.get(m.index).is_some_and(|d| d.guid == g))
                    .and_then(|m| m.lengths.get(i).copied())
                    .unwrap_or(0.0);
                self.toggle_lock(g, i, length)
            }
            Some(Clicked::Equal(g)) => self.toggle_equal(g),
            Some(Clicked::Unlock(_) | Clicked::AlignLock) => Ok(()),
            None => {
                if let Some(s) = self.sketch.as_mut() {
                    s.selected_dim = None;
                }
                return (false, None);
            }
        };
        (true, done.err())
    }

    /// The sketch's dimensions painted over it in view `which`: the selected one picked, with its padlocks and EQ; the
    /// references the dimension tool has picked so far, lit.
    pub(super) fn paint_sketch_dims(
        &self,
        painter: &egui::Painter,
        sd: &SketchDims,
        st: &Staged,
        which: Which,
        screen: &dyn Fn(P2) -> Pos2,
        units: bimcraft_core::units::Units,
    ) {
        let selected = self.sketch.as_ref().and_then(|s| s.selected_dim);
        for m in &sd.marks {
            let picked = selected.is_some() && sd.flexed.dimensions.get(m.index).map(|d| d.guid) == selected;
            dims::paint(painter, m, screen, picked, units, None);
        }
        for r in self.views.dimensioning.iter().flatten() {
            dims::paint_picked(painter, &sd.flexed, &sd.values, st, which, r, screen);
        }
    }
}

/// The Options Bar while a dimension is selected in a sketch: its Label (as a finished one's, `dimension_options`), and for one
/// of one segment with no label its Value, typed and taken on Enter: the sketch's lines move to it. What went wrong.
pub(crate) fn dimension_bar(ui: &mut Ui, ed: &mut FamilyEditor, g: Guid, units: bimcraft_core::units::Units) -> Option<String> {
    ui.label(t("Dimensions"));
    ui.separator();
    let mut problem = ed.dimension_options(ui, g);
    let d = ed.dimensions_now().into_iter().find(|d| d.guid == g)?;
    if d.segments() != 1 || d.alignment || d.label.is_some() {
        return problem;
    }
    let angular = d.kind == DimKind::Angular;
    let now = ed.dimension_length(g);
    ui.separator();
    let l = ui.label(t("Value"));
    let shown = now.map(|v| if angular { format!("{}", (v * 100.0).round() / 100.0) } else { units.length(v) }).unwrap_or_default();
    let key = UiId::new(("family-sketch-dimension-value", g));
    let mut text = ui.data(|m| m.get_temp::<String>(key)).unwrap_or(shown);
    let r = ui.add(TextEdit::singleline(&mut text).desired_width(90.0)).labelled_by(l.id);
    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        let v = if angular { text.trim().trim_end_matches('°').trim().parse::<f64>().ok() } else { units.parse(&text) };
        match v {
            Some(v) => problem = ed.set_dimension_value(g, v).err().or(problem),
            None => problem = Some(tf("`{}` isn't a value", &[text.trim()])),
        }
        ui.data_mut(|m| m.remove::<String>(key));
    } else if r.has_focus() {
        ui.data_mut(|m| m.insert_temp(key, text));
    } else {
        ui.data_mut(|m| m.remove::<String>(key));
    }
    problem
}
