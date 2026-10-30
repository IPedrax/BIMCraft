//! Visibility/Graphics, as Revit's dialog has them: what the active view hides and how it draws
//! the rest, by category (the model's, then the annotation's), by the project's filters and by
//! element; the Filters dialog that makes those filters; the View Templates dialog; and Hide in
//! View. The Visibility/Graphics dialog edits a draft of the view's graphics and OK or Apply sends
//! the difference as one `graphics` command; the other two run their commands as they go, as
//! Project Parameters does.

use crate::popup::Modal;

use bimcraft_i18n::msg;

use std::collections::BTreeSet;
use std::sync::Arc;

use bimcraft_core::graphics::{
    ANNOTATION, Applied, Dash, Detail, Graphics, Group, IMPORTED, MODEL, Override, PatternFill, SCANS, Style, VisualStyle,
};
use bimcraft_core::model::{Element, Guid, Id, Model};
use bimcraft_core::pen::{MAX_PEN, Pen};
use bimcraft_core::schedule;
use bimcraft_core::styles::ObjectStyle;
use bimcraft_core::units::Units;
use egui::{
    Align, Align2, Button, Checkbox, Color32, ComboBox, Context, DragValue, FontId, Grid, Id as UiId, Layout, Painter, Rect, RichText,
    ScrollArea, Sense, Shape, Slider, Stroke, StrokeKind, TextEdit, TextStyle, TextWrapMode, Ui, UiBuilder, WidgetInfo, WidgetType, pos2,
    vec2,
};
use serde_json::{Value, json};

use crate::lang::{self, t, tf};
use crate::style::pal;
use crate::{BimApp, Sheet, Status};

/// Visibility/Graphics' tabs.
const TABS: [&str; 6] =
    [msg!("Model Categories"), msg!("Annotation Categories"), msg!("Imported Categories"), "Filters", "Elements", "Design Options"];

/// The Elements tab's place in [`TABS`], the Filters' and the Design Options'.
pub(crate) const ELEMENTS_TAB: usize = 4;
pub(crate) const FILTERS_TAB: usize = 3;
pub(crate) const OPTIONS_TAB: usize = 5;

/// What a list of the dialog has picked, to edit its override.
#[derive(Clone, Debug, PartialEq)]
enum Pick {
    Category(String),
    Filter(usize),
    Element(Id),
}

/// The Visibility/Graphics dialog: the view (as commands take it) and its name, its graphics as the
/// project has them and as the dialog has edited them, the tab showing, the row picked, the
/// elements the Elements tab lists (those with an override, and those added to give one), the cell
/// being edited, and the search for a category's name. Opened on a view template (`template`), it edits the View Templates
/// dialog's draft of it: no Elements tab, and OK hands the draft back rather than running a command.
pub(crate) struct VgDialog {
    view: Value,
    template: Option<String>,
    title: String,
    own: Graphics,
    draft: Graphics,
    tab: usize,
    pick: Option<Pick>,
    listed: BTreeSet<Id>,
    cell: Option<CellEdit>,
    search: String,
}

/// What the Filters dialog has picked: a filter, and the name typed for it.
#[derive(Default)]
pub(crate) struct FiltersDialog {
    picked: String,
    name: String,
}

/// The JSON Merge Patch that turns `old` into `new`: what's new or changed, and null for what's gone.
pub(crate) fn merge_patch(old: &Value, new: &Value) -> Value {
    match (old, new) {
        (Value::Object(a), Value::Object(b)) => {
            let mut out = serde_json::Map::new();
            for (k, v) in b {
                match a.get(k) {
                    Some(w) if w == v => {}
                    Some(w) => {
                        out.insert(k.clone(), merge_patch(w, v));
                    }
                    None => {
                        out.insert(k.clone(), v.clone());
                    }
                }
            }
            for k in a.keys().filter(|k| !b.contains_key(*k)) {
                out.insert(k.clone(), Value::Null);
            }
            Value::Object(out)
        }
        _ => new.clone(),
    }
}

/// An element as a list says it: `Wall 12`.
fn element_name(m: &Model, id: Id) -> String {
    let category = m.elements.get(id).map_or("element", Element::category);
    format!("{} {id}", lang::category(category, false))
}

/// What view template `name` controls of the views following it, if there is one.
pub(crate) fn controls(m: &Model, name: Option<&String>) -> BTreeSet<Group> {
    name.and_then(|n| m.view_templates.get(n)).map(|t| t.controls.clone()).unwrap_or_default()
}

/// A group of a template's settings, as the dialogs name it.
pub(crate) fn group_name(g: Group) -> &'static str {
    match g {
        Group::Categories => "Categories",
        Group::Filters => "Filters",
        Group::Range => msg!("View Range"),
        Group::Phase => msg!("Phase and Phase Filter"),
        Group::Options => "Design Options",
        Group::Scale => "View Scale",
        Group::Detail => "Detail Level",
        Group::Style => "Visual Style",
        Group::BackgroundGrid => msg!("Background Grids"),
    }
}

/// The scales a drop-down offers, each as the interface names it: none first, then 1 : n.
/// A drop-down for a scale, named by `label`, of the scales a project in `units` offers, named as it names them: the
/// scale picked.
pub(crate) fn scale_combo(ui: &mut Ui, salt: &'static str, label: UiId, now: f64, units: Units) -> Option<f64> {
    let options: Vec<String> = units.scales().iter().map(|n| units.scale(*n)).collect();
    let picked = crate::properties::combo(ui, (0, salt), label, &units.scale(now), &options)?;
    units.scales().iter().copied().find(|n| units.scale(*n) == picked)
}

/// A drop-down for a detail level, named by `label`: the level picked.
pub(crate) fn detail_combo(ui: &mut Ui, salt: &'static str, label: UiId, now: Detail) -> Option<Detail> {
    let options: Vec<String> = Detail::ALL.iter().map(|d| t(d.name()).to_string()).collect();
    let picked = crate::properties::combo(ui, (0, salt), label, t(now.name()), &options)?;
    Detail::ALL.into_iter().find(|d| t(d.name()) == picked)
}

/// A drop-down for a visual style, named by `label`: the style picked.
pub(crate) fn style_combo(ui: &mut Ui, salt: &'static str, label: UiId, now: VisualStyle) -> Option<VisualStyle> {
    let options: Vec<String> = VisualStyle::ALL.iter().map(|v| t(v.name()).to_string()).collect();
    let picked = crate::properties::combo(ui, (0, salt), label, t(now.name()), &options)?;
    VisualStyle::ALL.into_iter().find(|v| t(v.name()) == picked)
}

/// Why a setting a view template controls can't be changed in the view following it, for a tooltip.
pub(crate) fn controlled_note(template: &str) -> String {
    tf("Set by view template {}: change the template, or stop following it.", &[template])
}

impl VgDialog {
    /// The dialog on template `name`'s graphics as the View Templates dialog has them, on a tab.
    pub(crate) fn for_template(name: String, graphics: Graphics, tab: usize) -> VgDialog {
        VgDialog {
            view: Value::Null,
            title: tf("View Template: {}", &[&name]),
            template: Some(name),
            own: graphics.clone(),
            draft: graphics,
            tab,
            pick: None,
            listed: BTreeSet::new(),
            cell: None,
            search: String::new(),
        }
    }

    /// Whether it is open on a view template.
    pub(crate) fn of_template(&self) -> bool {
        self.template.is_some()
    }
}

impl BimApp {
    /// The view the active tab is, the way commands take it: none for a sheet or a schedule. A level's
    /// ceiling plan is its plan's view.
    pub(crate) fn graphic_view(&self) -> Option<Value> {
        match self.sheet {
            // A dependent view's graphics are its parent's: Visibility/Graphics is for that one.
            Sheet::Plan(level) | Sheet::Ceiling(level) => {
                self.session.model.plan_view(level).is_none_or(|v| v.parent.is_none()).then(|| json!(level))
            }
            Sheet::Section(id) => Some(json!(id)),
            Sheet::View3D => Some(json!("{3D}")),
            Sheet::Paper(_) | Sheet::Schedule(_) | Sheet::Table(_) => None,
        }
    }

    /// The graphics the active view holds now: the default 3D view has none until something changes it.
    pub(crate) fn graphics_now(&self) -> Graphics {
        let m = &self.session.model;
        let id = match self.sheet {
            Sheet::Plan(id) | Sheet::Ceiling(id) | Sheet::Section(id) => Some(id),
            Sheet::View3D => m.default_3d(),
            _ => None,
        };
        id.and_then(|id| bimcraft_core::graphics::graphics_of(m, id)).cloned().unwrap_or_default()
    }

    /// What the active view's graphics come to: its own, but for what its view template controls.
    pub(crate) fn graphics_shown(&self) -> Graphics {
        self.graphics_now().resolved(&self.session.model).into_owned()
    }

    /// The view template that sets `group` of the active view, if it follows one that does.
    pub(crate) fn controlled_by(&self, group: Group) -> Option<String> {
        self.graphics_now().controlled_by(&self.session.model, group).map(String::from)
    }

    /// Visibility/Graphics for the active view, on a tab.
    pub(crate) fn open_graphics(&mut self, tab: usize) {
        let Some(view) = self.graphic_view() else {
            self.status = Status { text: t("This view has no graphics to override.").to_string(), error: true };
            return;
        };
        let own = self.graphics_now();
        let listed = own.elements.keys().filter_map(|g| self.session.model.elements.id(g)).collect();
        self.vg_dialog = Some(VgDialog {
            view,
            template: None,
            title: self.view_title(self.sheet),
            draft: own.clone(),
            own,
            tab,
            pick: None,
            listed,
            cell: None,
            search: String::new(),
        });
    }

    /// Override Graphics in View, By Element: the Elements tab, with what's selected on it.
    pub(crate) fn override_selection(&mut self) {
        let m = Arc::clone(&self.session.model);
        let picked: Vec<Id> = self.drawn_selection(&m);
        self.open_graphics(ELEMENTS_TAB);
        if let Some(d) = &mut self.vg_dialog {
            d.pick = picked.first().map(|id| Pick::Element(*id));
            d.listed.extend(picked);
        }
    }

    /// The selected elements views draw.
    pub(crate) fn drawn_selection(&self, m: &Model) -> Vec<Id> {
        self.selection
            .iter()
            .copied()
            .filter(|id| m.elements.get(id).is_some_and(|e| bimcraft_core::graphics::drawn(e.category())))
            .collect()
    }

    /// Hide in View: the selected elements, or with `category`, all the selected elements' categories.
    pub(crate) fn hide_selection(&mut self, category: bool) {
        let m = Arc::clone(&self.session.model);
        let (Some(view), picked) = (self.graphic_view(), self.drawn_selection(&m)) else {
            self.status = Status { text: t("This view has no graphics to override.").to_string(), error: true };
            return;
        };
        if picked.is_empty() {
            return;
        }
        let mut p = json!({"view": view});
        if category {
            let cats: BTreeSet<&str> = picked.iter().filter_map(|id| m.elements.get(id)).map(Element::category).collect();
            p["hide"] = json!(cats);
        } else {
            p["hide_elements"] = json!(picked);
        }
        if self.run("graphics", p).is_some() {
            self.selection.clear();
        }
    }

    /// What of the selection the active view hides: the elements hidden one by one, and the categories hidden whole. Only
    /// Reveal Hidden Elements lets anything hidden be picked, so this is what Unhide Element and Unhide Category offer.
    pub(crate) fn hidden_selection(&self) -> (Vec<Id>, Vec<&'static str>) {
        let (m, g) = (&self.session.model, self.graphics_shown());
        let (mut elements, mut categories) = (Vec::new(), BTreeSet::new());
        for (id, e) in self.selection.iter().filter_map(|id| Some((*id, m.elements.get(id)?))) {
            if g.elements.get(&m.guid(id)).is_some_and(|o| o.hidden) {
                elements.push(id);
            }
            if g.hides(e.category()) {
                categories.insert(e.category());
            }
        }
        (elements, categories.into_iter().collect())
    }

    /// Unhide Element (EU) and Unhide Category (VU): what the selection hides in the view comes back. It stays selected.
    pub(crate) fn unhide_selection(&mut self, category: bool) {
        let Some(view) = self.graphic_view() else { return };
        let (elements, categories) = self.hidden_selection();
        if if category { categories.is_empty() } else { elements.is_empty() } {
            let text = t("Nothing selected is hidden in this view. Reveal Hidden Elements (RH) draws what is, to select it.");
            self.status = Status { text: text.to_string(), error: false };
            return;
        }
        let p = if category { json!({"view": view, "show": categories}) } else { json!({"view": view, "show_elements": elements}) };
        self.run("graphics", p);
    }

    /// The Visibility/Graphics dialog, while it's open.
    pub(crate) fn graphics_window(&mut self, ctx: &Context) {
        // Object Styles, opened from it, stands in its place until it closes; the draft waits.
        if self.object_styles_dialog.is_some() {
            return;
        }
        let Some(mut d) = self.vg_dialog.take() else { return };
        let model = Arc::clone(&self.session.model);
        let followed = controls(&model, d.draft.template.as_ref());
        let (mut ok, mut cancel, mut apply, mut styles) = (false, false, false, false);
        // A cell's dialog over it has its own OK and Cancel.
        let over = d.cell.is_some();
        let r = Modal::new(UiId::new("visibility-graphics"), tf("Visibility/Graphics Overrides for {}", &[&d.title]))
            .width(800.0)
            .min_height(500.0)
            .closable(!over)
            .show(ctx, |ui| {
                crate::popup::body(ui, |ui| {
                    // As large on every tab, so the dialog doesn't move about as the tabs change.
                    if let Some(name) = &d.draft.template {
                        let what: Vec<&str> = followed
                            .iter()
                            .filter(|g| {
                                !matches!(
                                    g,
                                    Group::Range | Group::Phase | Group::Scale | Group::Detail | Group::Style | Group::BackgroundGrid
                                )
                            })
                            .map(|g| t(group_name(*g)))
                            .collect();
                        if !what.is_empty() {
                            ui.label(
                                RichText::new(tf("{} follow view template {}: change them there.", &[&what.join(", "), name]))
                                    .color(pal().muted),
                            );
                        }
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        for (i, label) in TABS.iter().enumerate().filter(|(i, _)| d.template.is_none() || *i != ELEMENTS_TAB) {
                            if ui.selectable_label(d.tab == i, t(label)).clicked() {
                                d.tab = i;
                                d.pick = None;
                            }
                        }
                    });
                    ui.separator();
                    let shown = d.draft.resolved(&model).into_owned();
                    match d.tab {
                        0 | 1 => {
                            // A view's point clouds with its model's categories: Revit gives them a tab of their own.
                            let names: Vec<&str> =
                                if d.tab == 0 { MODEL.iter().chain(&SCANS).copied().collect() } else { ANNOTATION.to_vec() };
                            styles = categories(ui, &mut d, &model, &shown, &names, !followed.contains(&Group::Categories));
                        }
                        2 => {
                            styles = categories(ui, &mut d, &model, &shown, &IMPORTED, !followed.contains(&Group::Categories));
                            // A drawing's layers are a view's own.
                            if d.template.is_none() {
                                ui.add_space(6.0);
                                imported_layers(ui, &mut d, &model);
                            }
                        }
                        3 => filters(ui, &mut d, &model, &shown, !followed.contains(&Group::Filters)),
                        ELEMENTS_TAB => elements(ui, &mut d, &model, &self.selection),
                        _ => design_options(ui, &mut d, &model, &shown, !followed.contains(&Group::Options)),
                    }
                });
                crate::popup::footer(ui, |ui| {
                    if !over {
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            apply = ui.button(t("Apply")).clicked();
                            cancel = ui.button(t("Cancel")).clicked();
                            ok = ui.button(t("OK")).clicked();
                        });
                    } else {
                        ui.allocate_space(vec2(ui.available_width(), ui.spacing().interact_size.y));
                    }
                });
            });
        cell_window(ctx, &mut d, &model);
        if styles {
            self.object_styles_dialog = Some(crate::objectstyles::ObjectStylesDialog::new(&model));
        }
        // What's different from the project's, as one command; a refusal keeps the dialog open to fix it.
        let mut failed = false;
        if (ok || apply)
            && let Some(name) = &d.template
        {
            // A template's: back to the View Templates dialog's draft, sent with its OK.
            if let Some(slot) = self.templates_dialog.as_mut().and_then(|td| td.draft(&model, name)) {
                slot.graphics.categories = d.draft.categories.clone();
                slot.graphics.filters = d.draft.filters.clone();
                slot.graphics.options = d.draft.options.clone();
            }
            d.own = d.draft.clone();
        } else if ok || apply {
            if let Some(p) = patch(&d, &followed) {
                match self.run("graphics", p) {
                    Some(_) => d.own = d.draft.clone(),
                    None => failed = true,
                }
            }
            if apply {
                let now = self.graphics_now();
                d.own = now;
            }
        }
        let done = (ok && !failed) || cancel || (r.should_close() && !over);
        if !done {
            self.vg_dialog = Some(d);
        }
    }

    /// The Filters dialog, while it's open: the project's view filters, one picked to edit, its
    /// categories and its rules. Each change is a `filter` command.
    pub(crate) fn filters_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.filters_dialog.take() else { return };
        let model = Arc::clone(&self.session.model);
        let units = model.units;
        let (mut close, mut run, mut picked) = (false, None::<Value>, None::<String>);
        let r = Modal::new(UiId::new("view-filters"), t("Filters")).width(660.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let l = ui.label(t("Filter:"));
                    let c = ComboBox::from_id_salt("view-filter-name").selected_text(&d.picked).width(220.0).show_ui(ui, |ui| {
                        for name in model.view_filters.keys() {
                            if ui.selectable_label(*name == d.picked, name).clicked() {
                                picked = Some(name.clone());
                            }
                        }
                    });
                    c.response.labelled_by(l.id);
                    if ui.button(t("New")).clicked() {
                        let fresh =
                            (1..).map(|n| format!("{} {n}", t("Filter"))).find(|n| !model.view_filters.contains_key(n)).unwrap_or_default();
                        run = Some(json!({"name": fresh, "categories": ["wall"]}));
                        picked = Some(fresh);
                    }
                    if ui.add_enabled(model.view_filters.contains_key(&d.picked), Button::new(t("Delete"))).clicked() {
                        run = Some(json!({"name": d.picked, "delete": true}));
                        picked = model.view_filters.keys().find(|n| **n != d.picked).cloned().or_else(|| Some(String::new()));
                    }
                });
                match model.view_filters.get(&d.picked) {
                    Some(f) => {
                        ui.add_space(6.0);
                        ui.label(RichText::new(t("Categories")).strong());
                        Grid::new("view-filter-categories").num_columns(4).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
                            for (i, cat) in schedule::categories().into_iter().enumerate() {
                                let mut on = f.categories.iter().any(|c| c == cat);
                                if ui.checkbox(&mut on, lang::category(cat, true)).changed() {
                                    let mut list: Vec<&str> = f.categories.iter().map(String::as_str).filter(|c| *c != cat).collect();
                                    if on {
                                        list.push(cat);
                                    }
                                    run = Some(json!({"name": d.picked, "categories": list}));
                                }
                                if i % 4 == 3 {
                                    ui.end_row();
                                }
                            }
                        });
                        ui.add_space(6.0);
                        ui.label(RichText::new(t("Filter Rules")).strong());
                        // The fields every one of its categories has.
                        let kinds: Vec<Vec<(String, schedule::Kind)>> = f.categories.iter().map(|c| schedule::fields(&model, c)).collect();
                        let names: Vec<String> = kinds
                            .first()
                            .map(|first| {
                                first
                                    .iter()
                                    .map(|(n, _)| n.clone())
                                    .filter(|n| kinds.iter().all(|k| k.iter().any(|(m, _)| m == n)))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let kind_of =
                            |n: &str| kinds.first().and_then(|k| k.iter().find(|(m, _)| m == n)).map_or(schedule::Kind::Text, |(_, k)| *k);
                        let mut rules = f.rules.clone();
                        // ponytail: typed values commit as the schedule's filter rows do, by the filter's picked name.
                        if self.filter_rows(ui, 0, &mut rules, &names, &kind_of, units) {
                            run = Some(json!({"name": d.picked, "rules": rules}));
                        }
                    }
                    None => {
                        ui.add_space(6.0);
                        ui.label(RichText::new(t("No filters yet: New makes one.")).color(pal().muted));
                    }
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button(t("Close")).clicked());
            });
        });
        if let Some(p) = run
            && self.run("filter", p).is_none()
        {
            picked = None;
        }
        if let Some(n) = picked {
            (d.name, d.picked) = (n.clone(), n);
        } else if !self.session.model.view_filters.contains_key(&d.picked) {
            d.picked = self.session.model.view_filters.keys().next().cloned().unwrap_or_default();
        }
        if !(close || r.should_close()) {
            self.filters_dialog = Some(d);
        }
    }

    /// Properties' Graphics group for the active view: its view template, and Edit… for its
    /// Visibility/Graphics Overrides.
    pub(crate) fn graphics_properties(&mut self, ui: &mut Ui) {
        let Some(view) = self.graphic_view() else { return };
        let model = Arc::clone(&self.session.model);
        let own = self.graphics_now();
        let shown = self.graphics_shown();
        let group = msg!("Graphics");
        let open = !self.collapsed.contains(group);
        if crate::properties::group_header(ui, t(group), open) && !self.collapsed.remove(group) {
            self.collapsed.insert(group);
        }
        if !open {
            return;
        }
        let (mut edit, mut assign, mut set) = (false, false, None::<(&str, Value)>);
        // The id the scale's field edits under: the view's own, which the 3D view's has no field for.
        let own_id = match self.sheet {
            Sheet::Plan(id) | Sheet::Ceiling(id) | Sheet::Section(id) => Some(id),
            _ => None,
        };
        egui::Frame::new().inner_margin(egui::Margin { left: 8, right: 0, top: 2, bottom: 6 }).show(ui, |ui| {
            let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            Grid::new("graphics-properties").num_columns(2).min_col_width(half).max_col_width(half).show(ui, |ui| {
                let controlled = |g: Group| own.controlled_by(&model, g).map(controlled_note);
                if let Some(id) = own_id {
                    let note = controlled(Group::Scale);
                    let label = ui.label(t("View Scale"));
                    let label = if let Some(n) = &note { label.on_hover_text(n) } else { label };
                    ui.add_enabled_ui(note.is_none(), |ui| {
                        if let Some(n) = scale_combo(ui, "view-scale", label.id, shown.scale_n(model.units), model.units) {
                            set = Some(("scale", json!({"view": view, "scale": n})));
                        }
                    });
                    ui.end_row();
                    let label = ui.label(t("Scale Value 1:"));
                    let label = if let Some(n) = &note { label.on_hover_text(n) } else { label };
                    ui.add_enabled_ui(note.is_none(), |ui| {
                        if let Some(typed) = self.field(ui, id, "scale", shown.scale_n(model.units).to_string(), label.id, "") {
                            match (typed.trim().is_empty(), model.units.parse_scale(&typed)) {
                                (true, _) => self.status = Status { text: t("A view always has a scale").into(), error: true },
                                (_, Some(n)) => set = Some(("scale", json!({"view": view, "scale": n}))),
                                _ => self.status = Status { text: tf("`{}` is not a scale", &[typed.trim()]), error: true },
                            }
                        }
                    });
                    ui.end_row();
                    let note = controlled(Group::Detail);
                    let label = ui.label(t("Detail Level"));
                    let label = if let Some(n) = &note { label.on_hover_text(n) } else { label };
                    ui.add_enabled_ui(note.is_none(), |ui| {
                        if let Some(level) = detail_combo(ui, "detail-level", label.id, shown.detail) {
                            set = Some(("detail", json!({"view": view, "detail": level.name()})));
                        }
                    });
                    ui.end_row();
                } else if self.sheet == Sheet::View3D {
                    let note = controlled(Group::Style);
                    let label = ui.label(t("Visual Style"));
                    let label = if let Some(n) = &note { label.on_hover_text(n) } else { label };
                    ui.add_enabled_ui(note.is_none(), |ui| {
                        if let Some(v) = style_combo(ui, "visual-style", label.id, shown.style) {
                            set = Some(("visual_style", json!({"style": v.name()})));
                        }
                    });
                    ui.end_row();
                }
                if let Some(id) = own_id {
                    let note = self.grid_controlled(id);
                    ui.add_enabled(note.is_none(), egui::Label::new(t("Background Grids")));
                    let r = ui.add_enabled_ui(note.is_none(), |ui| crate::properties::wide_button(ui, t("Edit…"))).inner;
                    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, note.is_none(), t("Edit Background Grids")));
                    if r.clicked() {
                        self.open_background_grid();
                    }
                    if let Some(note) = note {
                        r.on_disabled_hover_text(note);
                    }
                    ui.end_row();
                }
                // A button naming the template, as Revit's: it opens the dialog to pick another, or none.
                ui.label(t("View Template"));
                let now = own.template.clone().unwrap_or_else(|| t("<None>").to_string());
                let r = crate::properties::wide_button(ui, &now);
                r.widget_info(|| {
                    let mut info = WidgetInfo::labeled(WidgetType::Button, true, t("View Template"));
                    info.current_text_value = Some(now.clone());
                    info
                });
                assign |= r.clicked();
                ui.end_row();
                ui.label(t("Visibility/Graphics Overrides"));
                let r = crate::properties::wide_button(ui, t("Edit…"));
                r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tf("Edit {}", &[t("Visibility/Graphics Overrides")])));
                edit |= r.clicked();
                ui.end_row();
            });
        });
        if let Some((command, p)) = set {
            self.run(command, p);
        }
        if edit {
            self.open_graphics(0);
        }
        if assign {
            self.assign_template();
        }
    }
}

/// The patch to the view's graphics the dialog's edits come to: its categories, its elements and its
/// filters, but for what a template it follows controls. None without a difference.
fn patch(d: &VgDialog, followed: &BTreeSet<Group>) -> Option<Value> {
    let mut p = serde_json::Map::new();
    let mut put = |key: &str, old: Value, new: Value, follows: bool| {
        let diff = merge_patch(&old, &new);
        if !follows && diff.as_object().is_some_and(|o| !o.is_empty()) {
            p.insert(key.into(), diff);
        }
    };
    put("categories", json!(d.own.categories), json!(d.draft.categories), followed.contains(&Group::Categories));
    put("elements", json!(d.own.elements), json!(d.draft.elements), false);
    put("options", json!(d.own.options), json!(d.draft.options), followed.contains(&Group::Options));
    put("layers", json!(d.own.layers), json!(d.draft.layers), false);
    if !followed.contains(&Group::Filters) && d.own.filters != d.draft.filters {
        p.insert("filters".into(), json!(d.draft.filters));
    }
    if p.is_empty() {
        return None;
    }
    p.insert("view".into(), d.view.clone());
    Some(Value::Object(p))
}

/// A category's override as the dialog's draft has it, to edit; the project's template's, shown.
fn category_override(shown: &Graphics, cat: &str) -> Override {
    shown.categories.get(cat).cloned().unwrap_or_default()
}

/// Put `o` back as category `cat`'s override in the draft: none when it has nothing.
fn put_category(d: &mut VgDialog, cat: &str, o: Override) {
    if o.is_empty() {
        d.draft.categories.remove(cat);
    } else {
        d.draft.categories.insert(cat.to_string(), o);
    }
}

/// Put `o` back as the override of the row `pick` is: a category's, a filter's or an element's.
fn put(d: &mut VgDialog, model: &Model, pick: &Pick, o: Override) {
    match pick {
        Pick::Category(cat) => put_category(d, cat, o),
        Pick::Filter(i) => {
            if let Some(x) = d.draft.filters.get_mut(*i) {
                x.over = o;
            }
        }
        Pick::Element(id) => put_element(d, model.guid(*id), o),
    }
}

/// A column of the table that opens an editor, as Revit's: a style's lines or its patterns, in projection or where the
/// view cuts, or the surface's transparency.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Cell {
    ProjectionLines,
    ProjectionPatterns,
    Transparency,
    CutLines,
    CutPatterns,
}

impl Cell {
    /// Every column, the projection's then the cut's.
    const ALL: [Cell; 5] = [Cell::ProjectionLines, Cell::ProjectionPatterns, Cell::Transparency, Cell::CutLines, Cell::CutPatterns];

    fn cut(self) -> bool {
        matches!(self, Cell::CutLines | Cell::CutPatterns)
    }

    /// The column's heading, under its group's.
    fn heading(self) -> &'static str {
        match self {
            Cell::ProjectionLines | Cell::CutLines => msg!("Lines"),
            Cell::ProjectionPatterns | Cell::CutPatterns => msg!("Patterns"),
            Cell::Transparency => "Transparency",
        }
    }

    /// The column's name after a row's, for screen readers: "Walls: Cut Lines".
    fn name(self) -> &'static str {
        match self {
            Cell::ProjectionLines => msg!("Projection Lines"),
            Cell::ProjectionPatterns => msg!("Projection Patterns"),
            Cell::Transparency => "Transparency",
            Cell::CutLines => msg!("Cut Lines"),
            Cell::CutPatterns => msg!("Cut Patterns"),
        }
    }

    /// The small dialog that edits it.
    fn title(self) -> &'static str {
        match self {
            Cell::ProjectionLines | Cell::CutLines => msg!("Line Graphics"),
            Cell::ProjectionPatterns | Cell::CutPatterns => msg!("Pattern Graphics"),
            Cell::Transparency => msg!("Surface Transparency"),
        }
    }
}

/// A cell's override being edited in its own small dialog, as Revit's Line Graphics: the row, its name, the column, the
/// row's override as edited, and the pens a weight starts at, the row's object style.
pub(crate) struct CellEdit {
    pick: Pick,
    name: String,
    cell: Cell,
    over: Override,
    start: ObjectStyle,
}

/// A row of the table: what it is, its name, its override and the pens its object style gives.
struct Row {
    pick: Pick,
    name: String,
    over: Override,
    start: ObjectStyle,
}

/// The table's columns, after Visibility: the cells that open an editor, Halftone, and Detail Level when it has one.
struct Columns {
    cells: &'static [Cell],
    detail: bool,
}

/// The widths of the table's columns, points: the name, a cell, Halftone, Detail Level; and its rows' height.
const NAME_W: f32 = 230.0;
const CELL_W: f32 = 84.0;
const HALFTONE_W: f32 = 64.0;
const DETAIL_W: f32 = 80.0;
const ROW_H: f32 = 22.0;

/// A child of `ui` in `r`, laid out by `layout`, taking no room of `ui`'s.
fn child(ui: &mut Ui, r: Rect, layout: Layout) -> Ui {
    let mut c = ui.new_child(UiBuilder::new().max_rect(r).layout(layout));
    c.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
    c
}

/// The table of Visibility/Graphics, as Revit's: a row for each of `rows`, its visibility, a cell for each of the
/// columns that opens its editor and shows what it overrides, Halftone, and Detail Level. A click on a row picks it,
/// and its cells read Override…. The rows whose visibility or halftone changed, with their override as it is now.
fn table(ui: &mut Ui, d: &mut VgDialog, model: &Model, rows: &[Row], cols: &Columns, enabled: bool) -> Vec<(Pick, Override)> {
    let total = NAME_W + cols.cells.len() as f32 * CELL_W + HALFTONE_W + if cols.detail { DETAIL_W } else { 0.0 };
    let line = ui.visuals().widgets.noninteractive.bg_stroke;
    let ink = ui.visuals().text_color();
    let small = TextStyle::Small.resolve(ui.style());
    let body = TextStyle::Body.resolve(ui.style());
    // The heading: Visibility, the projection's columns and the cut's under their groups, Halftone and Detail Level.
    let (head, _) = ui.allocate_exact_size(vec2(total, ROW_H * 2.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(head, 0.0, ui.visuals().faint_bg_color);
    let text = |r: Rect, s: &str, font: &FontId| {
        p.text(r.center(), Align2::CENTER_CENTER, s, font.clone(), ink);
    };
    let (top, mid) = (head.top(), head.top() + ROW_H);
    text(Rect::from_min_size(pos2(head.left(), top), vec2(NAME_W, ROW_H * 2.0)), t("Visibility"), &body);
    let mut x = head.left() + NAME_W;
    for (group, cut) in [(msg!("Projection/Surface"), false), ("Cut", true)] {
        let cells: Vec<Cell> = cols.cells.iter().copied().filter(|c| c.cut() == cut).collect();
        if cells.is_empty() {
            continue;
        }
        let w = cells.len() as f32 * CELL_W;
        p.vline(x, head.y_range(), line);
        text(Rect::from_min_size(pos2(x, top), vec2(w, ROW_H)), t(group), &body);
        p.hline(x..=x + w, mid, line);
        for (i, c) in cells.iter().enumerate() {
            let cx = x + i as f32 * CELL_W;
            if i > 0 {
                p.vline(cx, mid..=head.bottom(), line);
            }
            text(Rect::from_min_size(pos2(cx, mid), vec2(CELL_W, ROW_H)), t(c.heading()), &small);
        }
        x += w;
    }
    p.vline(x, head.y_range(), line);
    text(Rect::from_min_size(pos2(x, top), vec2(HALFTONE_W, ROW_H * 2.0)), t("Halftone"), &small);
    x += HALFTONE_W;
    if cols.detail {
        p.vline(x, head.y_range(), line);
        text(Rect::from_min_size(pos2(x, top), vec2(DETAIL_W, ROW_H * 2.0)), t("Detail Level"), &small);
    }
    p.hline(head.x_range(), head.bottom(), line);
    let mut edits = Vec::new();
    ScrollArea::vertical().max_height(300.0).auto_shrink([false, true]).id_salt(("vg-table", d.tab)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (n, row) in rows.iter().enumerate() {
            let (r, clicked) = ui.allocate_exact_size(vec2(total, ROW_H), Sense::click());
            if clicked.clicked() {
                d.pick = Some(row.pick.clone());
            }
            let picked = d.pick.as_ref() == Some(&row.pick);
            let p = ui.painter().clone();
            if picked {
                p.rect_filled(r, 0.0, pal().select_fill);
            } else if n % 2 == 1 {
                p.rect_filled(r, 0.0, ui.visuals().faint_bg_color);
            }
            p.hline(r.x_range(), r.bottom(), line);
            let cell = |x: f32, w: f32| Rect::from_min_size(pos2(x, r.top()), vec2(w, ROW_H));
            // Visibility, ticked to show it.
            let mut visible = !row.over.hidden;
            let mut c = child(ui, cell(r.left(), NAME_W).shrink2(vec2(6.0, 0.0)), Layout::left_to_right(Align::Center));
            if c.add_enabled(enabled, Checkbox::new(&mut visible, &row.name)).changed() {
                edits.push((row.pick.clone(), Override { hidden: !visible, ..row.over.clone() }));
            }
            // The cells: Override… on the row picked, what each overrides on the rest.
            let mut x = r.left() + NAME_W;
            for k in cols.cells {
                let at = cell(x, CELL_W);
                p.vline(x, r.y_range(), line);
                let name = format!("{}: {}", row.name, t(k.name()));
                let mut c = child(ui, at.shrink(2.0), Layout::centered_and_justified(egui::Direction::LeftToRight));
                let b = c.add_enabled(enabled, Button::new(if picked { t("Override…") } else { "" }).frame(picked));
                b.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, name.clone()));
                if b.clicked() {
                    d.pick = Some(row.pick.clone());
                    d.cell = Some(CellEdit {
                        pick: row.pick.clone(),
                        name: row.name.clone(),
                        cell: *k,
                        over: row.over.clone(),
                        start: row.start,
                    });
                }
                if !picked {
                    paint_cell(ui, &p, at, *k, &row.over, ink, model);
                }
                x += CELL_W;
            }
            // Halftone, a box of its own.
            p.vline(x, r.y_range(), line);
            let mut half = row.over.halftone;
            let mut c = child(ui, cell(x, HALFTONE_W), Layout::centered_and_justified(egui::Direction::LeftToRight));
            let name = format!("{}: {}", row.name, t("Halftone"));
            let h = c.add_enabled(enabled, Checkbox::without_text(&mut half));
            h.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, half, name.clone()));
            if h.changed() {
                edits.push((row.pick.clone(), Override { halftone: half, ..row.over.clone() }));
            }
            x += HALFTONE_W;
            // Detail Level: each category drawn at the view's, until a category can have its own.
            if cols.detail {
                p.vline(x, r.y_range(), line);
                p.text(cell(x, DETAIL_W).center(), Align2::CENTER_CENTER, t("By View"), small.clone(), pal().muted);
            }
        }
    });
    edits
}

/// What a cell of `o` overrides, drawn in it: a line as it would draw, a fill's colour, the transparency's percent; nothing
/// where it overrides nothing.
fn paint_cell(ui: &Ui, p: &Painter, r: Rect, cell: Cell, o: &Override, ink: Color32, model: &Model) {
    let r = r.shrink2(vec2(12.0, 0.0));
    let style = if cell.cut() { &o.cut } else { &o.projection };
    let rgb = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
    match cell {
        Cell::ProjectionLines | Cell::CutLines => {
            if style.color.is_none() && style.weight.is_none() && style.dash.is_none() {
                return;
            }
            let colour = style.color.map_or(ink, rgb);
            let width = style.weight.map_or(1.0, |w| (w.mm() as f32 * 4.0).clamp(1.0, 6.0));
            let ends = [r.left_center(), r.right_center()];
            match style.dash {
                Some(Dash::Dashed) => p.extend(Shape::dashed_line(&ends, Stroke::new(width, colour), 6.0, 3.0)),
                Some(Dash::Dotted) => p.extend(Shape::dotted_line(&ends, colour, 4.0, width / 2.0 + 0.5)),
                _ => {
                    p.line_segment(ends, Stroke::new(width, colour));
                }
            }
        }
        Cell::ProjectionPatterns | Cell::CutPatterns => {
            if style.fill.is_some() || style.foreground.is_some() || style.background.is_some() {
                pattern_preview(ui, r.shrink2(vec2(0.0, 4.0)), style, model);
            }
        }
        Cell::Transparency => {
            if o.transparency > 0 {
                p.text(r.center(), Align2::CENTER_CENTER, format!("{}%", o.transparency), FontId::proportional(12.0), ink);
            }
        }
    }
}

/// The small dialog a cell opens, over Visibility/Graphics: its lines' colour, weight and dash, its patterns' colour, or
/// its transparency, each with a box to override it. OK puts the row's override in the draft; Cancel drops it.
fn cell_window(ctx: &Context, d: &mut VgDialog, model: &Model) {
    let Some(mut e) = d.cell.take() else { return };
    let (mut ok, mut cancel) = (false, false);
    let r = Modal::new(UiId::new("vg-cell"), t(e.cell.title()))
        .width(if matches!(e.cell, Cell::ProjectionPatterns | Cell::CutPatterns) { 440.0 } else { 360.0 })
        .show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(RichText::new(&e.name).color(pal().muted));
                ui.add_space(6.0);
                let (section, start) = if e.cell.cut() { ("Cut", e.start.cut) } else { ("Projection", e.start.projection) };
                let style = if e.cell.cut() { &mut e.over.cut } else { &mut e.over.projection };
                Grid::new("vg-cell-grid").num_columns(2).spacing(vec2(16.0, 6.0)).show(ui, |ui| match e.cell {
                    Cell::ProjectionLines | Cell::CutLines => line_rows(ui, section, style, start),
                    Cell::ProjectionPatterns | Cell::CutPatterns => {
                        fill_row(ui, section, style);
                        pattern_rows(ui, section, style, model);
                    }
                    Cell::Transparency => {
                        let l = ui.label(t("Transparency"));
                        let mut pct = f64::from(e.over.transparency);
                        ui.add(Slider::new(&mut pct, 0.0..=100.0).suffix("%")).labelled_by(l.id);
                        e.over.transparency = pct.round().clamp(0.0, 100.0) as u8;
                        ui.end_row();
                    }
                });
                if matches!(e.cell, Cell::ProjectionPatterns | Cell::CutPatterns) {
                    ui.label(RichText::new(t("Unchecked layers inherit; <None> hides a layer.")).small().color(pal().muted));
                    ui.label(
                        RichText::new(t("Patterns appear in plans, ceiling plans, section cuts and filled regions."))
                            .small()
                            .color(pal().muted),
                    );
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(t("Clear Overrides")).clicked() {
                        match e.cell {
                            Cell::ProjectionLines | Cell::CutLines => {
                                let s = if e.cell.cut() { &mut e.over.cut } else { &mut e.over.projection };
                                (s.color, s.weight, s.dash) = (None, None, None);
                            }
                            Cell::ProjectionPatterns | Cell::CutPatterns => {
                                let s = if e.cell.cut() { &mut e.over.cut } else { &mut e.over.projection };
                                (s.fill, s.foreground, s.background) = (None, None, None);
                            }
                            Cell::Transparency => e.over.transparency = 0,
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        cancel = ui.button(t("Cancel")).clicked();
                        ok = ui.button(t("OK")).clicked();
                    });
                });
            });
        });
    if ok {
        let pick = e.pick.clone();
        put(d, model, &pick, e.over);
    } else if !(cancel || r.should_close()) {
        d.cell = Some(e);
    }
}

/// All, None and Invert: every row of `rows` shown, hidden, or the other way about.
fn visibility_buttons(ui: &mut Ui, d: &mut VgDialog, model: &Model, rows: &[Row], enabled: bool) {
    for label in ["All", "None", msg!("Invert")] {
        if ui.add_enabled(enabled && !rows.is_empty(), Button::new(t(label)).min_size(vec2(64.0, 0.0))).clicked() {
            for row in rows {
                let hidden = match label {
                    "All" => false,
                    "None" => true,
                    _ => !row.over.hidden,
                };
                put(d, model, &row.pick, Override { hidden, ..row.over.clone() });
            }
        }
    }
}

/// The Model Categories and Annotation Categories tabs: a row for each category the search finds, by its name. A model
/// category's row has every column; an annotation's, its lines and halftone. Whether Object Styles… was pressed.
fn categories(ui: &mut Ui, d: &mut VgDialog, model: &Model, shown: &Graphics, names: &[&str], enabled: bool) -> bool {
    ui.horizontal(|ui| {
        let l = ui.label(t("Category name search:"));
        ui.add(TextEdit::singleline(&mut d.search).desired_width(200.0)).labelled_by(l.id);
    });
    ui.add_space(4.0);
    let needle = d.search.trim().to_lowercase();
    let mut rows: Vec<Row> = names
        .iter()
        .map(|cat| (cat, lang::category(cat, true)))
        .filter(|(_, name)| name.to_lowercase().contains(&needle))
        .map(|(cat, name)| Row {
            pick: Pick::Category((*cat).to_string()),
            name,
            over: category_override(shown, cat),
            start: model.object_style(cat),
        })
        .collect();
    rows.sort_by_cached_key(|r| r.name.to_lowercase());
    let model_tab = d.tab == 0;
    let cols = if model_tab {
        Columns { cells: &Cell::ALL, detail: true }
    } else {
        Columns { cells: &[Cell::ProjectionLines, Cell::ProjectionPatterns], detail: false }
    };
    for (pick, o) in table(ui, d, model, &rows, &cols, enabled) {
        put(d, model, &pick, o);
    }
    if rows.is_empty() {
        ui.label(RichText::new(t("No category has that name.")).color(pal().muted));
    }
    ui.add_space(6.0);
    let mut styles = false;
    ui.horizontal(|ui| {
        visibility_buttons(ui, d, model, &rows, enabled);
        if model_tab {
            ui.add_space(16.0);
            ui.label(RichText::new(t("Categories not overridden are drawn as Object Styles sets them.")).color(pal().muted));
            styles = ui.button(t("Object Styles…")).clicked();
        }
    });
    styles
}

/// The Filters tab: the filters the view applies in order, each a row of the table, and Add, Remove, Up and Down.
fn filters(ui: &mut Ui, d: &mut VgDialog, model: &Model, shown: &Graphics, enabled: bool) {
    if d.pick.as_ref().is_some_and(|p| !matches!(p, Pick::Filter(i) if *i < shown.filters.len())) {
        d.pick = None;
    }
    // A template's filters are only shown: editing them would go to the draft, which doesn't hold them.
    let start = bimcraft_core::styles::default_of("");
    let rows: Vec<Row> = (shown.filters.iter().enumerate())
        .map(|(i, a)| Row { pick: Pick::Filter(i), name: a.filter.clone(), over: a.over.clone(), start })
        .collect();
    for (pick, o) in table(ui, d, model, &rows, &Columns { cells: &Cell::ALL, detail: false }, enabled) {
        put(d, model, &pick, o);
    }
    if rows.is_empty() {
        ui.label(RichText::new(t("No filters applied.")).color(pal().muted));
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let applied: BTreeSet<&String> = d.draft.filters.iter().map(|a| &a.filter).collect();
        let others: Vec<String> = model.view_filters.keys().filter(|n| !applied.contains(n)).cloned().collect();
        let label = ui.label(t("Add filter:"));
        let mut add = None;
        let c = ComboBox::from_id_salt("vg-add-filter").selected_text(t("<Pick>")).width(180.0).show_ui(ui, |ui| {
            for n in &others {
                if ui.selectable_label(false, n).clicked() {
                    add = Some(n.clone());
                }
            }
        });
        c.response.labelled_by(label.id).on_hover_text(t("A filter is made in View > Filters"));
        if let Some(n) = add
            && enabled
        {
            d.draft.filters.push(Applied { filter: n, over: Override::default() });
            d.pick = Some(Pick::Filter(d.draft.filters.len() - 1));
        }
        let at = match d.pick {
            Some(Pick::Filter(i)) if i < d.draft.filters.len() => Some(i),
            _ => None,
        };
        if ui.add_enabled(enabled && at.is_some(), Button::new(t("Remove"))).clicked()
            && let Some(i) = at
        {
            d.draft.filters.remove(i);
            d.pick = None;
        }
        if ui.add_enabled(enabled && at.is_some_and(|i| i > 0), Button::new(t("Up"))).clicked()
            && let Some(i) = at
        {
            d.draft.filters.swap(i, i - 1);
            d.pick = Some(Pick::Filter(i - 1));
        }
        if ui.add_enabled(enabled && at.is_some_and(|i| i + 1 < d.draft.filters.len()), Button::new(t("Down"))).clicked()
            && let Some(i) = at
        {
            d.draft.filters.swap(i, i + 1);
            d.pick = Some(Pick::Filter(i + 1));
        }
    });
}

/// The Elements tab: the elements hidden or overridden in this view, each a row of the table, and Add Selected for
/// what's selected.
fn elements(ui: &mut Ui, d: &mut VgDialog, model: &Model, selection: &[Id]) {
    let rows: Vec<Row> = (d.listed.iter())
        .filter_map(|id| model.elements.get(id).map(|e| (*id, e.category())))
        .map(|(id, cat)| Row {
            pick: Pick::Element(id),
            name: element_name(model, id),
            over: d.draft.elements.get(&model.guid(id)).cloned().unwrap_or_default(),
            start: model.object_style(cat),
        })
        .collect();
    for (pick, o) in table(ui, d, model, &rows, &Columns { cells: &Cell::ALL, detail: false }, true) {
        put(d, model, &pick, o);
    }
    if rows.is_empty() {
        ui.label(RichText::new(t("No element is hidden or overridden here: select some and Add Selected.")).color(pal().muted));
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let drawn: Vec<Id> = selection
            .iter()
            .copied()
            .filter(|id| model.elements.get(id).is_some_and(|e| bimcraft_core::graphics::drawn(e.category())))
            .collect();
        if ui.add_enabled(!drawn.is_empty(), Button::new(t("Add Selected"))).clicked() {
            d.listed.extend(drawn);
        }
        let gone = match d.pick {
            Some(Pick::Element(id)) => Some(id),
            _ => None,
        };
        if ui.add_enabled(gone.is_some(), Button::new(t("Remove"))).clicked()
            && let Some(id) = gone
        {
            d.draft.elements.remove(&model.guid(id));
            d.listed.remove(&id);
            d.pick = None;
        }
    });
}

/// The Imported Categories tab's layers: for each CAD drawing of the project, a check box for each of its layers, cleared
/// for those this view hides (Revit's layers under each drawing's row).
fn imported_layers(ui: &mut Ui, d: &mut VgDialog, model: &Model) {
    if model.cads.is_empty() {
        ui.label(RichText::new(t("The project has no CAD drawings: Insert > Import CAD brings one in.")).color(pal().muted));
        return;
    }
    ui.label(RichText::new(t("Layers this view shows of each drawing")).strong());
    ScrollArea::vertical().max_height(150.0).auto_shrink([false, true]).id_salt("vg-layers").show(ui, |ui| {
        for (name, cad) in &model.cads {
            egui::CollapsingHeader::new(name).id_salt(("vg-cad", name)).default_open(model.cads.len() == 1).show(ui, |ui| {
                for layer in &cad.dxf().layers {
                    let mut on = !d.draft.layers.get(name).is_some_and(|l| l.contains(&layer.name));
                    if ui.checkbox(&mut on, &layer.name).changed() {
                        let hidden = d.draft.layers.entry(name.clone()).or_default();
                        if on {
                            hidden.remove(&layer.name);
                        } else {
                            hidden.insert(layer.name.clone());
                        }
                        if hidden.is_empty() {
                            d.draft.layers.remove(name);
                        }
                    }
                }
            });
        }
    });
}

/// The Design Options tab: for each set of options, the one this view shows, the primary unless
/// it chooses another.
fn design_options(ui: &mut Ui, d: &mut VgDialog, model: &Model, shown: &Graphics, enabled: bool) {
    if model.option_sets.is_empty() {
        ui.label(RichText::new(t("The project has no design options: Manage > Design Options makes them.")).color(pal().muted));
        return;
    }
    Grid::new("vg-options").num_columns(2).striped(true).spacing(vec2(16.0, 6.0)).show(ui, |ui| {
        for (set, s) in &model.option_sets {
            let label = ui.label(set);
            let primary = tf("Primary ({})", &[&s.primary]);
            let chosen = shown.options.get(set);
            let text = chosen.cloned().unwrap_or_else(|| primary.clone());
            let mut pick = None;
            ui.add_enabled_ui(enabled, |ui| {
                let c = ComboBox::from_id_salt(("vg-option", set)).selected_text(text).width(220.0).show_ui(ui, |ui| {
                    if ui.selectable_label(chosen.is_none(), &primary).clicked() {
                        pick = Some(None);
                    }
                    for o in &s.options {
                        if ui.selectable_label(chosen == Some(o), o).clicked() {
                            pick = Some(Some(o.clone()));
                        }
                    }
                });
                c.response.labelled_by(label.id);
            });
            match pick {
                Some(Some(o)) => {
                    d.draft.options.insert(set.clone(), o);
                }
                Some(None) => {
                    d.draft.options.remove(set);
                }
                None => {}
            }
            ui.end_row();
        }
    });
}

/// Put `o` back as element `id`'s override in the draft: none when it has nothing.
fn put_element(d: &mut VgDialog, id: Guid, o: Override) {
    if o.is_empty() {
        d.draft.elements.remove(&id);
    } else {
        d.draft.elements.insert(id, o);
    }
}

/// An override's settings, to edit: its projection's and its cut's lines and fill, halftone, and
/// (with `see_through`) transparency. Whether any changed.
pub(crate) fn editor(ui: &mut Ui, o: &mut Override, enabled: bool, see_through: bool) -> bool {
    let before = o.clone();
    ui.add_enabled_ui(enabled, |ui| {
        // The projection's settings and the cut's side by side.
        ui.columns(2, |cols| {
            style_rows(&mut cols[0], "Projection", &mut o.projection, Pen::PROJECTION);
            style_rows(&mut cols[1], "Cut", &mut o.cut, Pen::CUT);
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut o.halftone, t("Halftone"));
            if see_through {
                let l = ui.label(t("Transparency"));
                let mut pct = f64::from(o.transparency);
                let r = ui.add(DragValue::new(&mut pct).range(0.0..=100.0).speed(1.0).suffix("%"));
                r.labelled_by(l.id);
                o.transparency = pct.round().clamp(0.0, 100.0) as u8;
            }
            if ui.button(t("Reset")).clicked() {
                *o = Override { hidden: o.hidden, ..Override::default() };
            }
        });
    });
    *o != before
}

/// A pen as the Line weight drop-down names it: its number, and its width on paper or that it is a hairline.
pub(crate) fn pen_name(p: Pen) -> String {
    match p.0 {
        0 => format!("0 · {}", t("Hairline")),
        n => format!("{n} · {:.2} mm", p.mm()),
    }
}

/// A style's four settings, each with a box to override it: the line's colour, weight and dash, and
/// the fill's colour. `section` names which, `weight` is the weight a new override starts at.
fn style_rows(ui: &mut Ui, section: &str, s: &mut Style, weight: Pen) {
    ui.label(RichText::new(t(section)).strong());
    Grid::new(("vg-style", section)).num_columns(2).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
        line_rows(ui, section, s, weight);
        fill_row(ui, section, s);
    });
}

/// A box that overrides one of a style's settings, named "Cut: Line weight" for screen readers; whether it's ticked.
fn override_box(ui: &mut Ui, section: &str, what: &str, on: &mut bool) -> bool {
    let name = format!("{}: {}", t(section), t(what));
    let r = ui.checkbox(on, t(what));
    let ticked = *on;
    r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, ticked, name.clone()));
    r.changed()
}

/// A colour, a grid row: its box, and the colour once it overrides.
fn colour_row(ui: &mut Ui, section: &str, what: &str, value: &mut Option<[u8; 3]>, start: [u8; 3]) {
    let mut on = value.is_some();
    if override_box(ui, section, what, &mut on) {
        *value = on.then_some(start);
    }
    if let Some(rgb) = value {
        let name = format!("{}: {}", t(section), t(what));
        let c = ui.color_edit_button_srgb(rgb);
        c.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name.clone()));
    } else {
        ui.label("");
    }
    ui.end_row();
}

/// A style's lines, grid rows: their colour, weight and dash. A weight starts at `weight`.
fn line_rows(ui: &mut Ui, section: &str, s: &mut Style, weight: Pen) {
    colour_row(ui, section, msg!("Line color"), &mut s.color, [40, 40, 40]);
    let mut on = s.weight.is_some();
    if override_box(ui, section, "Line weight", &mut on) {
        s.weight = on.then_some(weight);
    }
    if let Some(w) = &mut s.weight {
        let name = format!("{}: {}", t(section), t("Line weight"));
        let c = ComboBox::from_id_salt(("vg-weight", section)).selected_text(pen_name(*w)).width(110.0).show_ui(ui, |ui| {
            for each in (0..=MAX_PEN).map(Pen) {
                ui.selectable_value(w, each, pen_name(each));
            }
        });
        c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, name.clone()));
    } else {
        ui.label("");
    }
    ui.end_row();
    let mut on = s.dash.is_some();
    if override_box(ui, section, "Line dash", &mut on) {
        s.dash = on.then_some(Dash::Dashed);
    }
    if let Some(k) = &mut s.dash {
        let name = format!("{}: {}", t(section), t("Line dash"));
        let c = ComboBox::from_id_salt(("vg-dash", section)).selected_text(t(dash_name(*k))).width(110.0).show_ui(ui, |ui| {
            for each in [Dash::Solid, Dash::Dashed, Dash::Dotted] {
                ui.selectable_value(k, each, t(dash_name(each)));
            }
        });
        c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, name.clone()));
    } else {
        ui.label("");
    }
    ui.end_row();
}

/// A style's patterns, a grid row: the fill's colour.
fn fill_row(ui: &mut Ui, section: &str, s: &mut Style) {
    colour_row(ui, section, msg!("Fill color"), &mut s.fill, [200, 200, 200]);
}

/// Independent pattern layers: an unchecked box inherits, <None> explicitly hides that layer.
fn pattern_rows(ui: &mut Ui, section: &str, s: &mut Style, model: &Model) {
    for (heading, pattern_label, color_label, value, color) in [
        ("Foreground", "Foreground Pattern", "Foreground Color", &mut s.foreground, [40, 40, 40]),
        ("Background", "Background Pattern", "Background Color", &mut s.background, [255, 255, 255]),
    ] {
        let mut on = value.is_some();
        if override_box(ui, section, heading, &mut on) {
            *value =
                on.then(|| PatternFill { pattern: model.fill_patterns.iter().find(|(_, p)| p.is_solid()).map(|(n, _)| n.clone()), color });
        }
        let mut fallback = PatternFill::default();
        let layer = value.as_mut().unwrap_or(&mut fallback);
        ui.add_enabled_ui(on, |ui| {
            ui.horizontal(|ui| {
                let name = format!("{}: {}", t(section), t(pattern_label));
                let shown = layer.pattern.as_deref().unwrap_or(t("<None>"));
                let c = ComboBox::from_id_salt(&name).selected_text(shown).width(170.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut layer.pattern, None, t("<None>"));
                    for name in model.fill_patterns.keys() {
                        ui.selectable_value(&mut layer.pattern, Some(name.clone()), name);
                    }
                });
                c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, on, name.clone()));
                let name = format!("{}: {}", t(section), t(color_label));
                let c = ui.color_edit_button_srgb(&mut layer.color);
                c.widget_info(|| WidgetInfo::labeled(WidgetType::ColorButton, on, name.clone()));
            });
        });
        ui.end_row();
    }
    ui.label(t("Preview"));
    let (rect, _) = ui.allocate_exact_size(vec2(220.0, 48.0), Sense::hover());
    pattern_preview(ui, rect, s, model);
    ui.end_row();
}

/// The two layers over paper, also used by the table's pattern cells.
fn pattern_preview(ui: &Ui, rect: Rect, s: &Style, model: &Model) {
    let rgb = |[r, g, b]: [u8; 3]| Color32::from_rgb(r, g, b);
    ui.painter().rect_filled(rect, 0.0, s.fill.map_or(Color32::WHITE, rgb));
    for layer in [&s.background, &s.foreground].into_iter().flatten() {
        crate::fillpatterns::paint_pattern(ui, rect, layer.pattern.as_ref().and_then(|n| model.fill_patterns.get(n)), rgb(layer.color));
    }
    ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color), StrokeKind::Inside);
}

fn dash_name(d: Dash) -> &'static str {
    match d {
        Dash::Solid => msg!("Solid"),
        Dash::Dashed => msg!("Dashed"),
        Dash::Dotted => msg!("Dotted"),
    }
}
