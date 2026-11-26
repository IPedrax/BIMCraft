//! View graphics in the family document. Each of its views ({3D}, the two plans and the four elevations) keeps its own
//! Visibility/Graphics (the project's dialog, its rows the categories a family's views draw), visual style and Graphic
//! Display Options, {3D}'s alone as in the project, and View Range, a plan's alone; Thin Lines is the window's, shared with
//! the project's tabs. Each view's are laid over the scratch project it is drawn of (`Views::staged`, `staged_3d`), so the
//! project's drawing does what they say.
//!
//! What they set is the view's for the session, as Preview Visibility and Temporary Hide/Isolate are: no part of the draft, so
//! neither saved, nor a step of its history, nor an unsaved change; it goes when the family closes.
use super::tabs::DocId;
use super::*;
use bimcraft_core::family::views::Which;
use bimcraft_core::graphics::{Graphics, Override, VisualStyle};
use bimcraft_core::ids::Guid;
use bimcraft_core::model::ViewRange;
use egui::{Button, Grid, TextEdit};
use std::collections::BTreeSet;

/// What the family document's views keep of their graphics for the session.
#[derive(Default)]
pub(crate) struct Kept {
    /// Each view's Visibility/Graphics and visual style, by view ({3D}'s under none); none set, the project's defaults.
    graphics: BTreeMap<Option<Which>, Graphics>,
    /// Each plan's View Range; none set, a project plan's default.
    ranges: BTreeMap<Which, ViewRange>,
    /// {3D}'s Graphic Display Options.
    pub(crate) display: bimcraft_core::view3d::DisplayOptions,
    /// The box {3D} is cut to, its least and greatest corners in the family's axes, while Selection Box keeps one.
    pub(crate) section_box: Option<([f64; 3], [f64; 3])>,
    /// The View Range dialog, open on a plan: its four fields as typed, and why OK was refused.
    range_dialog: Option<RangeDialog>,
    /// The views that reveal what they hide (Reveal Hidden Elements): drawn in magenta, so it can be picked and unhidden.
    reveal: BTreeSet<Option<Which>>,
    /// View-Specific Element Graphics (Override Graphics by Element), open: the view, the forms it overrides (by GUID), the override it opened on and
    /// the override as edited.
    override_dialog: Option<(Option<Which>, Vec<Guid>, Override, Override)>,
}

/// Reveal Hidden Elements' colour, the project's.
const REVEALED: [u8; 3] = [255, 0, 255];

/// What Reveal Hidden Elements draws a hidden element with: magenta, cut and seen.
fn revealed() -> Override {
    let magenta = bimcraft_core::graphics::Style { color: Some(REVEALED), ..Default::default() };
    Override { projection: magenta.clone(), cut: magenta, ..Default::default() }
}

/// The View Range dialog, open.
struct RangeDialog {
    which: Which,
    fields: [String; 4],
    error: Option<String>,
}

/// The family document's view the Visibility/Graphics dialog is open on, and the categories it lists.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FamilyView {
    pub(crate) doc: DocId,
    pub(crate) which: Option<Which>,
    /// The Model Categories tab's rows, then the Annotation Categories tab's.
    pub(crate) model: Vec<&'static str>,
    pub(crate) annotation: Vec<&'static str>,
}

impl Kept {
    /// View `which`'s Visibility/Graphics and visual style.
    pub(crate) fn graphics_of(&self, which: Option<Which>) -> Graphics {
        self.graphics.get(&which).cloned().unwrap_or_default()
    }

    /// Plan `which`'s View Range.
    pub(crate) fn range_of(&self, which: Which) -> ViewRange {
        self.ranges.get(&which).copied().unwrap_or_default()
    }

    /// The heights plan `which` sees once its View Range is set, lowest and highest, mm over Ref. Level; none until then, every
    /// form whatever its height (`Staged::seen`).
    pub(crate) fn seen(&self, which: Which) -> Option<(f64, f64)> {
        let r = self.ranges.get(&which)?;
        Some((r.bottom.min(r.depth), r.top.unwrap_or(f64::INFINITY)))
    }

    /// Whether view `which` reveals what it hides.
    pub(crate) fn revealing(&self, which: Option<Which>) -> bool {
        self.reveal.contains(&which)
    }

    /// The graphics view `which` is drawn with: its own, but while it reveals what it hides, nothing hidden: an element it
    /// hides (a plane) in magenta, and a form, drawn apart in magenta (`apart_forms`).
    pub(crate) fn drawn_graphics(&self, which: Option<Which>) -> Graphics {
        let mut g = self.graphics_of(which);
        if self.revealing(which) {
            g.categories.values_mut().for_each(|o| o.hidden = false);
            g.elements.values_mut().filter(|o| o.hidden).for_each(|o| *o = revealed());
        }
        g
    }

    /// Whether view `which` hides element `g` (a form or a plane, by its GUID) by itself, and doesn't reveal it.
    pub(crate) fn hides(&self, which: Option<Which>, g: Guid) -> bool {
        !self.revealing(which) && self.hidden_by_element(which, g)
    }

    /// The colour view `which` draws plane `g`'s trace in while it reveals what it hides and hides the plane: magenta. None
    /// otherwise.
    pub(crate) fn revealed_plane(&self, which: Option<Which>, g: Guid) -> Option<bimcraft_core::render::Rgb> {
        let [r, g2, b] = REVEALED;
        (self.revealing(which) && self.hidden_by_element(which, g)).then_some(bimcraft_core::render::Rgb(r, g2, b))
    }

    /// Whether view `which` hides element `g` by itself, revealed or not.
    fn hidden_by_element(&self, which: Option<Which>, g: Guid) -> bool {
        self.graphics.get(&which).and_then(|x| x.elements.get(&g)).is_some_and(|o| o.hidden)
    }

    /// Whether view `which` hides category `cat`.
    fn hidden_category(&self, which: Option<Which>, cat: &str) -> bool {
        self.graphics.get(&which).and_then(|x| x.categories.get(cat)).is_some_and(|o| o.hidden)
    }

    /// Elements `gs` hidden in view `which`, their overrides kept.
    pub(crate) fn hide(&mut self, which: Option<Which>, gs: &[Guid]) {
        let mut g = self.graphics_of(which);
        for id in gs {
            g.elements.entry(*id).or_default().hidden = true;
        }
        self.set_graphics(which, g);
    }

    /// Elements `gs` shown in view `which` again: what else their overrides say stays.
    fn unhide(&mut self, which: Option<Which>, gs: &[Guid]) {
        let mut g = self.graphics_of(which);
        for id in gs {
            if let Some(o) = g.elements.get_mut(id) {
                o.hidden = false;
                if o.is_empty() {
                    g.elements.remove(id);
                }
            }
        }
        self.set_graphics(which, g);
    }

    /// Category `cat` hidden in view `which`, or shown again.
    pub(crate) fn hide_category(&mut self, which: Option<Which>, cat: &str, hidden: bool) {
        let mut g = self.graphics_of(which);
        let o = g.categories.entry(cat.to_string()).or_default();
        o.hidden = hidden;
        if o.is_empty() {
            g.categories.remove(cat);
        }
        self.set_graphics(which, g);
    }

    /// View `which`'s graphics set to `g`.
    pub(crate) fn set_graphics(&mut self, which: Option<Which>, g: Graphics) {
        if g == Graphics::default() {
            self.graphics.remove(&which);
        } else {
            self.graphics.insert(which, g);
        }
    }

    /// Whether a dialog of the views' is open, which has the keys.
    pub(crate) fn dialog_open(&self) -> bool {
        self.range_dialog.is_some() || self.override_dialog.is_some()
    }

    /// What each view keeps that changes how it is drawn, for the caches' keys.
    pub(crate) fn key(&self, which: Option<Which>) -> String {
        let range = which.and_then(|w| self.ranges.get(&w));
        let reveal = self.revealing(which);
        let boxed = if which.is_none() { self.section_box } else { None };
        format!("{}{range:?}{:?}{reveal}{boxed:?}", serde_json::to_string(&self.graphics_of(which)).unwrap_or_default(), self.display)
    }
}

/// The view a family document's view is drawn of has `own` laid over it: its categories' and elements' overrides and its
/// visual style, over what the document itself sets there (Temporary Hide's wall, the ghost's halftone), which win.
pub(crate) fn lay_over(graphics: &mut Graphics, own: &Graphics) {
    for (k, o) in &own.categories {
        graphics.categories.entry(k.clone()).or_insert_with(|| o.clone());
    }
    for (g, o) in &own.elements {
        graphics.elements.entry(*g).or_insert_with(|| o.clone());
    }
    graphics.style = own.style;
}

impl FamilyEditor {
    /// The category its forms are of, as Visibility/Graphics lists it.
    pub(crate) fn forms_category(&self) -> &'static str {
        self.view_categories().0.first().copied().unwrap_or(bimcraft_core::graphics::COMPONENT)
    }

    /// Form `i`'s GUID: what a view's graphics name it by.
    pub(crate) fn form_guid(&self, i: usize) -> Option<Guid> {
        self.forms.get(i).map(|f| f.original.guid)
    }

    /// Whether view `which` hides form `i` for good (by itself, or as its category), and doesn't reveal it.
    pub(crate) fn kept_hides_form(&self, which: Option<Which>, i: usize) -> bool {
        !self.views.kept.revealing(which) && self.kept_hidden_form(which, i)
    }

    /// Whether view `which` hides form `i` for good, revealed or not.
    fn kept_hidden_form(&self, which: Option<Which>, i: usize) -> bool {
        let k = &self.views.kept;
        self.form_guid(i).is_some_and(|g| k.hidden_by_element(which, g)) || k.hidden_category(which, self.forms_category())
    }

    /// The solid forms the view shown leaves out, hidden in it for good or for now, by GUID: what no Dimension or Align picks
    /// there (a reference that names one).
    pub(crate) fn forms_out_of_view(&self) -> Vec<Guid> {
        (0..self.forms.len()).filter(|i| self.temp_hidden_form(*i)).filter_map(|i| self.form_guid(i)).collect()
    }

    /// Whether view `which` hides plane `g` for good, and doesn't reveal it.
    pub(crate) fn kept_hides(&self, which: Option<Which>, g: Guid) -> bool {
        self.views.kept.hides(which, g)
    }

    /// The solid forms view `which` draws apart, each with graphics of its own: those it overrides, and, while it reveals what it
    /// hides, those it hides, in magenta. Not those Temporary Hide/Isolate hides, being sketched, or left out by their type or
    /// their Visibility Settings in it (`by_view`), which the view draws as it did.
    pub(crate) fn apart_forms(
        &self,
        which: Option<Which>,
        f: &bimcraft_core::family::Family,
        values: &BTreeMap<String, bimcraft_core::family::Val>,
        by_view: &[bool],
    ) -> Vec<(usize, Override)> {
        let own = self.views.kept.graphics_of(which);
        let editing = self.sketch.as_ref().and_then(|s| s.index);
        let mut out = Vec::new();
        for (i, form) in f.forms.iter().enumerate() {
            let in_type = form.visible.as_ref().is_some_and(|v| crate::adaptive_editor::worth(v, values) == Some(0.0));
            if form.void || in_type || by_view.get(i).copied().unwrap_or(false) || self.temp_only_form(i) || editing == Some(i) {
                continue;
            }
            if self.kept_hidden_form(which, i) {
                // Hidden for good, by itself or with its category, whatever else it overrides: drawn only when revealed (a door's
                // forms drawn apart are a generic family, which its category's hide wouldn't reach).
                if self.views.kept.revealing(which) {
                    out.push((i, revealed()));
                }
            } else if let Some(o) = own.elements.get(&form.guid).filter(|o| !o.hidden && !o.is_empty()) {
                out.push((i, o.clone()));
            }
        }
        out
    }

    /// The categories the family's views draw, as Visibility/Graphics lists them: its own category (a door's or a window's as
    /// the project's Doors or Windows) and, for one that stands in a wall, Walls; and the levels its elevations draw Ref. Level
    /// as.
    pub(crate) fn view_categories(&self) -> (Vec<&'static str>, Vec<&'static str>) {
        let own = match self.category {
            Category::Door => "door",
            Category::Window => "window",
            c => bimcraft_core::graphics::component_category(c).unwrap_or(bimcraft_core::graphics::COMPONENT),
        };
        let mut model = vec![own];
        if self.category.hosted() {
            model.push("wall");
        }
        (model, vec!["level"])
    }
}

impl BimApp {
    /// Visibility/Graphics for the family document's view shown.
    pub(crate) fn family_vg(&mut self) {
        let (Some(doc), Some(ed)) = (self.active_doc(), self.active_family()) else { return };
        self.vg_dialog = Some(vg_for(doc, ed));
    }

    /// The Visibility/Graphics dialog's OK or Apply on a family document's view: `g` its graphics now.
    pub(crate) fn family_vg_done(&mut self, view: &FamilyView, g: &Graphics) {
        if let Some(ed) = self.family_of_mut(view.doc) {
            let mut own = ed.views.kept.graphics_of(view.which);
            own.categories = g.categories.clone();
            own.elements = g.elements.clone();
            ed.views.kept.set_graphics(view.which, own);
        }
    }

    /// Wireframe, Hidden Line or Shaded for {3D}; in a plan or an elevation, says they are {3D}'s.
    pub(crate) fn family_style(&mut self, style: VisualStyle) {
        let Some(ed) = self.active_family_mut() else { return };
        if ed.views.shown.is_some() {
            self.status = Status { text: t("visual styles are for {3D}: open it from the Project Browser").into(), error: true };
            return;
        }
        let mut g = ed.views.kept.graphics_of(None);
        g.style = style;
        ed.views.kept.set_graphics(None, g);
    }

    /// Graphic Display Options for {3D}; in a plan or an elevation, says they are {3D}'s.
    pub(crate) fn family_display(&mut self) {
        let Some(ed) = self.active_family() else { return };
        if ed.views.shown.is_some() {
            let text = t("graphic display options are for {3D}: open it from the Project Browser");
            self.status = Status { text: text.into(), error: true };
            return;
        }
        let options = ed.views.kept.display;
        let Some(doc) = self.active_doc() else { return };
        self.display_dialog = Some(crate::display::DisplayDialog::for_family(options, doc));
    }

    /// The Graphic Display Options dialog's OK or Apply on a family document's {3D}.
    pub(crate) fn family_display_done(&mut self, doc: DocId, options: bimcraft_core::view3d::DisplayOptions) {
        if let Some(ed) = self.family_of_mut(doc) {
            ed.views.kept.display = options;
        }
    }

    /// View Range for the floor plan; in another view, says it is the floor plan's. A ceiling plan looks up from its own
    /// cut, as a project's does, and keeps none.
    pub(crate) fn family_view_range(&mut self) {
        let units = self.session.model.units;
        let Some(ed) = self.active_family_mut() else { return };
        let Some(which) = ed.views.shown.filter(|w| *w == Which::Plan) else {
            self.status = Status { text: t("a view range is the floor plan's: open Floor Plans > Ref. Level").into(), error: true };
            return;
        };
        let r = ed.views.kept.range_of(which);
        let fields =
            [r.top.map(|v| units.length(v)).unwrap_or_default(), units.length(r.cut), units.length(r.bottom), units.length(r.depth)];
        ed.views.kept.range_dialog = Some(RangeDialog { which, fields, error: None });
    }

    /// The family document's View Range dialog, while it is open: the plan's range typed in the project's units; OK sets it.
    pub(crate) fn family_range_window(&mut self, ctx: &Context) {
        let units = self.session.model.units;
        let Some(ed) = self.active_family_mut() else { return };
        let Some(mut d) = ed.views.kept.range_dialog.take() else { return };
        let (mut ok, mut cancel) = (false, false);
        let r = crate::popup::Modal::new(UiId::new("family-view-range"), t("View Range")).width(340.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                Grid::new("family-view-range-fields").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                    for (i, (label, hint)) in
                        [(t("Top"), t("Unlimited")), (t("Cut Plane"), ""), (t("Bottom"), ""), (t("View Depth"), "")].into_iter().enumerate()
                    {
                        let l = ui.label(label);
                        ui.add_sized([160.0, ui.spacing().interact_size.y], TextEdit::singleline(&mut d.fields[i]).hint_text(hint))
                            .labelled_by(l.id);
                        ui.end_row();
                    }
                });
                ui.label(RichText::new(t("Offsets from Ref. Level.")).color(pal().muted));
                if let Some(e) = &d.error {
                    ui.label(RichText::new(e).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    let size = vec2(76.0, 0.0);
                    cancel = ui.add(Button::new(t("Cancel")).min_size(size)).clicked();
                    ok = ui.add(Button::new(t("OK")).min_size(size)).clicked();
                });
            });
        });
        let mut keep = !(ok || cancel || r.should_close());
        if ok {
            let parse = |s: &str| units.parse(s);
            let top = if d.fields[0].trim().is_empty() { Some(None) } else { parse(&d.fields[0]).map(Some) };
            match (top, parse(&d.fields[1]), parse(&d.fields[2]), parse(&d.fields[3])) {
                (Some(top), Some(cut), Some(bottom), Some(depth)) => {
                    // As the project checks a plan's: within reach, and top ≥ cut plane ≥ bottom ≥ view depth.
                    let range = ViewRange { top, cut, bottom, depth, ..ViewRange::default() };
                    match bimcraft_core::model::range_ok(t("Ref. Level"), &range) {
                        // OK on what the dialog showed of a plan that had none sets none: it still sees every form.
                        Ok(()) if range == ViewRange::default() && !ed.views.kept.ranges.contains_key(&d.which) => {}
                        Ok(()) => {
                            ed.views.kept.ranges.insert(d.which, range);
                        }
                        Err(e) => {
                            d.error = Some(e);
                            keep = true;
                        }
                    }
                }
                _ => {
                    d.error = Some(t("Each is a length, and Top may be left empty for the level above.").to_string());
                    keep = true;
                }
            }
        }
        if keep {
            ed.views.kept.range_dialog = Some(d);
        }
    }
}

/// Visibility/Graphics on the view document `doc` (`ed`) shows.
pub(crate) fn vg_for(doc: DocId, ed: &FamilyEditor) -> crate::graphics::VgDialog {
    let which = ed.views.shown;
    let (model, annotation) = ed.view_categories();
    let title = format!("{} - {}", ed.name.trim(), view_name(which));
    let graphics = ed.views.kept.graphics_of(which);
    crate::graphics::VgDialog::for_family(FamilyView { doc, which, model, annotation }, title, graphics)
}

impl FamilyEditor {
    /// What is selected that a view hides or overrides by element: the forms (by GUID) and the planes.
    fn picked_elements(&self) -> (Vec<Guid>, Vec<Guid>) {
        let forms = self.picked_forms().filter_map(|i| self.form_guid(i)).collect();
        (forms, self.picked_planes().collect())
    }

    /// Hide Element in the view, or with `category` Hide Category, for what is selected; the selection goes, as the project's does.
    /// Why not, when nothing of it can be.
    pub(crate) fn hide_in_view(&mut self, category: bool) -> Result<(), &'static str> {
        if self.adaptive.is_some() {
            return Err(msg!("an adaptive family's views hide for now only: use Temporary Hide/Isolate"));
        }
        let which = self.views.shown;
        let (forms, planes) = self.picked_elements();
        if forms.is_empty() && planes.is_empty() {
            return Err(msg!("select what to hide in the view first"));
        }
        if category {
            if forms.is_empty() {
                return Err(msg!("reference planes have no category to hide in a family's view: Hide Element hides each"));
            }
            let cat = self.forms_category();
            self.views.kept.hide_category(which, cat, true);
        } else {
            self.views.kept.hide(which, &[forms, planes].concat());
        }
        self.let_everything_go();
        Ok(())
    }

    /// Unhide Elements, or with `category` Unhide Category, for what is selected while the view reveals what it hides.
    pub(crate) fn unhide_in_view(&mut self, category: bool) -> Result<(), &'static str> {
        let which = self.views.shown;
        let (forms, planes) = self.picked_elements();
        if category {
            let cat = self.forms_category();
            if forms.is_empty() || !self.views.kept.hidden_category(which, cat) {
                return Err(msg!("select a form the view hides with its category first: Reveal Hidden Elements shows them"));
            }
            self.views.kept.hide_category(which, cat, false);
        } else {
            let hidden: Vec<Guid> = forms.into_iter().chain(planes).filter(|g| self.views.kept.hidden_by_element(which, *g)).collect();
            if hidden.is_empty() {
                return Err(msg!("select what the view hides first: Reveal Hidden Elements shows it"));
            }
            self.views.kept.unhide(which, &hidden);
        }
        Ok(())
    }

    /// Reveal Hidden Elements in the view shown, on or off.
    pub(crate) fn toggle_reveal(&mut self) {
        let which = self.views.shown;
        if !self.views.kept.reveal.remove(&which) {
            self.views.kept.reveal.insert(which);
        }
    }

    /// Override Graphics by Element: View-Specific Element Graphics on the forms selected, as the first has it.
    pub(crate) fn open_override(&mut self) -> Result<(), &'static str> {
        let (forms, _) = self.picked_elements();
        let Some(first) = forms.first() else {
            return Err(msg!("select a form to override its graphics in the view first"));
        };
        let which = self.views.shown;
        let mut o = self.views.kept.graphics_of(which).elements.get(first).cloned().unwrap_or_default();
        o.hidden = false;
        self.views.kept.override_dialog = Some((which, forms, o.clone(), o));
        Ok(())
    }
}

impl BimApp {
    /// Hide Element, Hide Category, Unhide Elements, Unhide Category, Reveal Hidden Elements and Override Graphics by Element
    /// in the family document's view shown; why not, in the status bar.
    pub(crate) fn family_view_element(&mut self, action: &str) {
        let Some(ed) = self.active_family_mut() else { return };
        let done = match action {
            "Hide Element" => ed.hide_in_view(false),
            "Hide Category" => ed.hide_in_view(true),
            "Unhide Elements" => ed.unhide_in_view(false),
            "Unhide Category" => ed.unhide_in_view(true),
            "Override Graphics by Element" => ed.open_override(),
            _ => {
                ed.toggle_reveal();
                Ok(())
            }
        };
        if let Err(why) = done {
            self.status = Status { text: t(why).into(), error: true };
        }
    }

    /// View-Specific Element Graphics, while it is open: the project's override editor; OK gives the forms it is open on that
    /// override in the view, whether they are hidden there staying as it is.
    pub(crate) fn family_override_window(&mut self, ctx: &Context) {
        let Some(ed) = self.active_family_mut() else { return };
        let Some((which, forms, start, mut o)) = ed.views.kept.override_dialog.take() else { return };
        let (mut ok, mut cancel) = (false, false);
        let r = crate::popup::Modal::new(UiId::new("family-element-graphics"), t("View-Specific Element Graphics")).width(520.0).show(
            ctx,
            |ui| {
                crate::popup::body(ui, |ui| {
                    crate::graphics::editor(ui, &mut o, true, which.is_none());
                });
                crate::popup::footer(ui, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        let size = vec2(76.0, 0.0);
                        cancel = ui.add(Button::new(t("Cancel")).min_size(size)).clicked();
                        ok = ui.add(Button::new(t("OK")).min_size(size)).clicked();
                    });
                });
            },
        );
        if ok && o != start {
            let mut g = ed.views.kept.graphics_of(which);
            for id in &forms {
                let was = g.elements.get(id).cloned().unwrap_or_default();
                let mut set = edited(&was, &start, &o);
                set.hidden = was.hidden;
                if set.is_empty() {
                    g.elements.remove(id);
                } else {
                    g.elements.insert(*id, set);
                }
            }
            ed.views.kept.set_graphics(which, g);
        } else if !ok && !cancel && !r.should_close() {
            ed.views.kept.override_dialog = Some((which, forms, start, o));
        }
    }
}

/// Override `was` with what View-Specific Element Graphics changed from `start` to `now`, field by field, and nothing else: on
/// several forms it opens on the first's, and what no one edited stays each form's own.
fn edited(was: &Override, start: &Override, now: &Override) -> Override {
    fn patch(was: &mut serde_json::Value, start: &serde_json::Value, now: &serde_json::Value) {
        let (Some(s), Some(n)) = (start.as_object(), now.as_object()) else {
            *was = now.clone();
            return;
        };
        let keys: BTreeSet<&String> = s.keys().chain(n.keys()).collect();
        for k in keys {
            let (a, b) = (s.get(k), n.get(k));
            if a == b {
                continue;
            }
            if !was.is_object() {
                *was = serde_json::Value::Object(Default::default());
            }
            let Some(w) = was.as_object_mut() else { return };
            match (a, b) {
                (Some(a), Some(b)) if a.is_object() && b.is_object() => {
                    patch(w.entry(k.clone()).or_insert_with(|| serde_json::Value::Object(Default::default())), a, b);
                }
                (_, Some(b)) => {
                    w.insert(k.clone(), b.clone());
                }
                (_, None) => {
                    w.remove(k);
                }
            }
        }
    }
    let (Ok(mut w), Ok(s), Ok(n)) = (serde_json::to_value(was), serde_json::to_value(start), serde_json::to_value(now)) else {
        return now.clone();
    };
    patch(&mut w, &s, &n);
    serde_json::from_value(w).unwrap_or_else(|_| now.clone())
}

/// View `which`'s name, as the Project Browser and its tab name it.
fn view_name(which: Option<Which>) -> String {
    match which {
        None => "{3D}".into(),
        Some(Which::Plan) => format!("{} {}", t("Floor Plan:"), t("Ref. Level")),
        Some(Which::Ceiling) => format!("{} {}", t("Ceiling Plan:"), t("Ref. Level")),
        Some(Which::Elevation(s)) => format!("{} {}", t("Elevation:"), t(s.name())),
    }
}
