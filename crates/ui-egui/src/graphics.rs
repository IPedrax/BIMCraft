//! Visibility/Graphics, as Revit's dialog has them: what the active view hides and how it draws
//! the rest, by category (the model's, then the annotation's), by the project's filters and by
//! element; the Filters dialog that makes those filters; the View Templates dialog; and Hide in
//! View. The Visibility/Graphics dialog edits a draft of the view's graphics and OK or Apply sends
//! the difference as one `graphics` command; the other two run their commands as they go, as
//! Project Parameters does.

use std::collections::BTreeSet;
use std::sync::Arc;

use bimcraft_core::graphics::{ANNOTATION, Applied, Dash, Detail, Graphics, Group, MODEL, Override, Style, VisualStyle};
use bimcraft_core::model::{Element, Guid, Id, Model};
use bimcraft_core::pen::{MAX_PEN, Pen};
use bimcraft_core::schedule;
use bimcraft_core::units::Units;
use egui::{
    Align, Button, Checkbox, ComboBox, Context, DragValue, Grid, Id as UiId, Layout, Modal, RichText, ScrollArea, Ui, WidgetInfo,
    WidgetType, vec2,
};
use serde_json::{Value, json};

use crate::lang::{self, t, tf};
use crate::style::pal;
use crate::{BimApp, Sheet, Status};

/// Visibility/Graphics' tabs.
const TABS: [&str; 5] = ["Model Categories", "Annotation Categories", "Filters", "Elements", "Design Options"];

/// What a list of the dialog has picked, to edit its override.
#[derive(Clone, Debug, PartialEq)]
enum Pick {
    Category(String),
    Filter(usize),
    Element(Id),
}

/// The Visibility/Graphics dialog: the view (as commands take it) and its name, its graphics as the
/// project has them and as the dialog has edited them, the tab showing, what's picked to edit, and
/// the elements the Elements tab lists (those with an override, and those added to give one).
pub(crate) struct VgDialog {
    view: Value,
    title: String,
    own: Graphics,
    draft: Graphics,
    tab: usize,
    pick: Option<Pick>,
    listed: BTreeSet<Id>,
}

/// How wide the names in a list are at least, points.
const LIST_NAME: f32 = 270.0;

/// What the Filters dialog has picked: a filter, and the name typed for it.
#[derive(Default)]
pub(crate) struct FiltersDialog {
    picked: String,
    name: String,
}

/// The View Templates dialog: the template picked, and the name typed for a new one.
#[derive(Default)]
pub(crate) struct TemplatesDialog {
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
fn group_name(g: Group) -> &'static str {
    match g {
        Group::Categories => "Categories",
        Group::Filters => "Filters",
        Group::Range => "View Range",
        Group::Phase => "Phase and Phase Filter",
        Group::Options => "Design Options",
        Group::Scale => "View Scale",
        Group::Detail => "Detail Level",
        Group::Style => "Visual Style",
    }
}

/// The scales a drop-down offers, each as the interface names it: none first, then 1 : n.
/// A drop-down for a scale, named by `label`, of the scales a project in `units` offers, named as it names them: the
/// scale picked.
fn scale_combo(ui: &mut Ui, salt: &'static str, label: UiId, now: f64, units: Units) -> Option<f64> {
    let options: Vec<String> = units.scales().iter().map(|n| units.scale(*n)).collect();
    let picked = crate::properties::combo(ui, (0, salt), label, &units.scale(now), &options)?;
    units.scales().iter().copied().find(|n| units.scale(*n) == picked)
}

/// A drop-down for a detail level, named by `label`: the level picked.
fn detail_combo(ui: &mut Ui, salt: &'static str, label: UiId, now: Detail) -> Option<Detail> {
    let options: Vec<String> = Detail::ALL.iter().map(|d| t(d.name()).to_string()).collect();
    let picked = crate::properties::combo(ui, (0, salt), label, t(now.name()), &options)?;
    Detail::ALL.into_iter().find(|d| t(d.name()) == picked)
}

/// A drop-down for a visual style, named by `label`: the style picked.
fn style_combo(ui: &mut Ui, salt: &'static str, label: UiId, now: VisualStyle) -> Option<VisualStyle> {
    let options: Vec<String> = VisualStyle::ALL.iter().map(|v| t(v.name()).to_string()).collect();
    let picked = crate::properties::combo(ui, (0, salt), label, t(now.name()), &options)?;
    VisualStyle::ALL.into_iter().find(|v| t(v.name()) == picked)
}

/// Why a setting a view template controls can't be changed in the view following it, for a tooltip.
pub(crate) fn controlled_note(template: &str) -> String {
    tf("Set by view template {}: change the template, or stop following it.", &[template])
}

impl BimApp {
    /// The view the active tab is, the way commands take it: none for a sheet or a schedule. A level's
    /// ceiling plan is its plan's view.
    pub(crate) fn graphic_view(&self) -> Option<Value> {
        match self.sheet {
            Sheet::Plan(level) | Sheet::Ceiling(level) => Some(json!(level)),
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
        self.vg_dialog = Some(VgDialog { view, title: self.view_title(self.sheet), draft: own.clone(), own, tab, pick: None, listed });
    }

    /// Override Graphics in View, By Element: the Elements tab, with what's selected on it.
    pub(crate) fn override_selection(&mut self) {
        let m = Arc::clone(&self.session.model);
        let picked: Vec<Id> = self.drawn_selection(&m);
        self.open_graphics(3);
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
        let Some(mut d) = self.vg_dialog.take() else { return };
        let model = Arc::clone(&self.session.model);
        let followed = controls(&model, d.draft.template.as_ref());
        let (mut ok, mut cancel, mut apply) = (false, false, false);
        let r = Modal::new(UiId::new("visibility-graphics")).show(ctx, |ui| {
            // As large on every tab, so the dialog doesn't move about as the tabs change.
            ui.set_width(700.0);
            ui.set_min_height(500.0);
            ui.label(RichText::new(tf("Visibility/Graphics Overrides for {}", &[&d.title])).heading());
            if let Some(name) = &d.draft.template {
                let what: Vec<&str> = followed
                    .iter()
                    .filter(|g| !matches!(g, Group::Range | Group::Phase | Group::Scale | Group::Detail | Group::Style))
                    .map(|g| t(group_name(*g)))
                    .collect();
                if !what.is_empty() {
                    ui.label(
                        RichText::new(tf("{} follow view template {}: change them there.", &[&what.join(", "), name])).color(pal().muted),
                    );
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (i, label) in TABS.iter().enumerate() {
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
                    let names: &[&str] = if d.tab == 0 { &MODEL } else { &ANNOTATION };
                    categories(ui, &mut d, &shown, names, !followed.contains(&Group::Categories));
                }
                2 => filters(ui, &mut d, &model, &shown, !followed.contains(&Group::Filters)),
                3 => elements(ui, &mut d, &model, &self.selection),
                _ => design_options(ui, &mut d, &model, &shown, !followed.contains(&Group::Options)),
            }
            ui.add_space(6.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                apply = ui.button(t("Apply")).clicked();
                cancel = ui.button(t("Cancel")).clicked();
                ok = ui.button(t("OK")).clicked();
            });
        });
        // What's different from the project's, as one command; a refusal keeps the dialog open to fix it.
        let mut failed = false;
        if ok || apply {
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
        let done = (ok && !failed) || cancel || r.should_close();
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
        let r = Modal::new(UiId::new("view-filters")).show(ctx, |ui| {
            ui.set_width(660.0);
            ui.label(RichText::new(t("Filters")).heading());
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
                            first.iter().map(|(n, _)| n.clone()).filter(|n| kinds.iter().all(|k| k.iter().any(|(m, _)| m == n))).collect()
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
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button(t("Close")).clicked());
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

    /// The View Templates dialog, while it's open: the project's templates, what each controls, one
    /// made from the active view, and the active view's template.
    pub(crate) fn templates_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.templates_dialog.take() else { return };
        let model = Arc::clone(&self.session.model);
        let view = self.graphic_view();
        let own = self.graphics_now();
        let (mut close, mut run, mut assign, mut picked) = (false, None::<Value>, None::<Value>, None::<String>);
        let r = Modal::new(UiId::new("view-templates")).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.label(RichText::new(t("View Templates")).heading());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let l = ui.label(t("Template:"));
                let c = ComboBox::from_id_salt("view-template-name").selected_text(&d.picked).width(220.0).show_ui(ui, |ui| {
                    for name in model.view_templates.keys() {
                        if ui.selectable_label(*name == d.picked, name).clicked() {
                            picked = Some(name.clone());
                        }
                    }
                });
                c.response.labelled_by(l.id);
                if ui.add_enabled(model.view_templates.contains_key(&d.picked), Button::new(t("Delete"))).clicked() {
                    run = Some(json!({"name": d.picked, "delete": true}));
                    picked = model.view_templates.keys().find(|n| **n != d.picked).cloned().or_else(|| Some(String::new()));
                }
            });
            if let Some(tpl) = model.view_templates.get(&d.picked) {
                ui.add_space(6.0);
                ui.label(RichText::new(t("The views following it get from it:")).strong());
                for g in Group::ALL {
                    let mut on = tpl.controls.contains(&g);
                    if ui.checkbox(&mut on, t(group_name(g))).changed() {
                        // This one as ticked, the others as they were.
                        let list: Vec<Group> =
                            Group::ALL.into_iter().filter(|h| if *h == g { on } else { tpl.controls.contains(h) }).collect();
                        run = Some(json!({"name": d.picked, "controls": list}));
                    }
                }
                // What it holds of scale, detail level and visual style, to set.
                ui.add_space(4.0);
                Grid::new("template-settings").num_columns(2).show(ui, |ui| {
                    let l = ui.label(t("View Scale:"));
                    if let Some(n) = scale_combo(ui, "template-scale", l.id, tpl.graphics.scale_n(model.units), model.units) {
                        run = Some(json!({"name": d.picked, "scale": n}));
                    }
                    ui.end_row();
                    let l = ui.label(t("Detail Level:"));
                    if let Some(level) = detail_combo(ui, "template-detail", l.id, tpl.graphics.detail) {
                        run = Some(json!({"name": d.picked, "detail": level.name()}));
                    }
                    ui.end_row();
                    let l = ui.label(t("Visual Style:"));
                    if let Some(v) = style_combo(ui, "template-style", l.id, tpl.graphics.style) {
                        run = Some(json!({"name": d.picked, "visual_style": v.name()}));
                    }
                    ui.end_row();
                });
                if let Some(view) = &view {
                    let now = own.template.as_ref() == Some(&d.picked);
                    ui.add_space(4.0);
                    let label = if now { t("Stop Following in This View") } else { t("Follow in This View") };
                    if ui.button(label).clicked() {
                        assign = Some(json!({"view": view, "template": if now { Value::Null } else { json!(d.picked) }}));
                    }
                }
            } else {
                ui.add_space(6.0);
                ui.label(RichText::new(t("No view templates yet: Create from This View makes one.")).color(pal().muted));
            }
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let l = ui.label(t("New template:"));
                ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(180.0)).labelled_by(l.id);
                let from = view.clone().filter(|_| !d.name.trim().is_empty());
                let r = ui.add_enabled(from.is_some(), Button::new(t("Create from This View")));
                if r.clicked()
                    && let Some(from) = from
                {
                    run = Some(json!({"name": d.name.trim(), "from": from}));
                    picked = Some(d.name.trim().to_string());
                }
            });
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button(t("Close")).clicked());
        });
        if let Some(p) = run
            && self.run("template", p).is_none()
        {
            picked = None;
        }
        if let Some(p) = assign {
            self.run("graphics", p);
        }
        if let Some(n) = picked {
            d.picked = n;
        } else if !self.session.model.view_templates.contains_key(&d.picked) {
            d.picked = self.session.model.view_templates.keys().next().cloned().unwrap_or_default();
        }
        if !(close || r.should_close()) {
            self.templates_dialog = Some(d);
        }
    }

    /// Properties' Graphics group for the active view: its view template, and Edit… for its
    /// Visibility/Graphics Overrides.
    pub(crate) fn graphics_properties(&mut self, ui: &mut Ui) {
        let Some(view) = self.graphic_view() else { return };
        let model = Arc::clone(&self.session.model);
        let own = self.graphics_now();
        let shown = self.graphics_shown();
        let group = "Graphics";
        let open = !self.collapsed.contains(group);
        if crate::properties::group_header(ui, t(group), open) && !self.collapsed.remove(group) {
            self.collapsed.insert(group);
        }
        if !open {
            return;
        }
        let (mut edit, mut set) = (false, None::<(&str, Value)>);
        // The id the scale's field edits under: the view's own, which the 3D view's has no field for.
        let own_id = match self.sheet {
            Sheet::Plan(id) | Sheet::Ceiling(id) | Sheet::Section(id) => Some(id),
            _ => None,
        };
        egui::Frame::new().inner_margin(egui::Margin { left: 8, right: 0, top: 2, bottom: 6 }).show(ui, |ui| {
            let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            Grid::new("graphics-properties").num_columns(2).striped(true).min_col_width(half).max_col_width(half).show(ui, |ui| {
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
                let label = ui.label(t("View Template"));
                let none = t("<None>").to_string();
                let now = own.template.clone().unwrap_or_else(|| none.clone());
                let options: Vec<String> = std::iter::once(none.clone()).chain(model.view_templates.keys().cloned()).collect();
                if let Some(picked) = crate::properties::combo(ui, (0, "view-template"), label.id, &now, &options) {
                    set = Some(("graphics", json!({"view": view, "template": if picked == none { Value::Null } else { json!(picked) }})));
                }
                ui.end_row();
                ui.label(t("Visibility/Graphics Overrides"));
                let r = ui.button(t("Edit…"));
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

/// The Model Categories and Annotation Categories tabs: each category visible or not, and
/// Override… to edit how it draws; the override of the one picked below the list.
fn categories(ui: &mut Ui, d: &mut VgDialog, shown: &Graphics, names: &[&str], enabled: bool) {
    ScrollArea::vertical().max_height(240.0).auto_shrink([false, true]).id_salt(("vg-categories", d.tab)).show(ui, |ui| {
        Grid::new(("vg-grid", d.tab)).num_columns(3).striped(true).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
            for cat in names {
                let o = category_override(shown, cat);
                let mut visible = !o.hidden;
                let r = ui
                    .scope(|ui| {
                        ui.set_min_width(LIST_NAME);
                        ui.add_enabled(enabled, Checkbox::new(&mut visible, lang::category(cat, true)))
                    })
                    .inner;
                if r.changed() {
                    let mut o = o.clone();
                    o.hidden = !visible;
                    put_category(d, cat, o);
                }
                let picked = d.pick == Some(Pick::Category((*cat).to_string()));
                let name = tf("Override {}", &[&lang::category(cat, true)]);
                let b = ui.add_enabled(enabled || picked, Button::new(t("Override…")).selected(picked));
                b.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name.clone()));
                if b.clicked() {
                    d.pick = Some(Pick::Category((*cat).to_string()));
                }
                ui.label(RichText::new(if o.is_empty() { "" } else { t("Overridden") }).color(pal().muted));
                ui.end_row();
            }
        });
    });
    if let Some(Pick::Category(cat)) = d.pick.clone().filter(|p| matches!(p, Pick::Category(c) if names.contains(&c.as_str()))) {
        let mut o = category_override(shown, &cat);
        ui.separator();
        ui.label(RichText::new(tf("Overrides for {}", &[&lang::category(&cat, true)])).strong());
        if editor(ui, &mut o, enabled, true) {
            put_category(d, &cat, o);
        }
    }
}

/// The Filters tab: the filters the view applies in order, each visible or not, with Override… to
/// edit how it draws what it picks out, and Add, Remove, Up and Down.
fn filters(ui: &mut Ui, d: &mut VgDialog, model: &Model, shown: &Graphics, enabled: bool) {
    if d.pick.as_ref().is_some_and(|p| !matches!(p, Pick::Filter(i) if *i < shown.filters.len())) {
        d.pick = None;
    }
    // A template's filters are only shown: editing them would go to the draft, which doesn't hold them.
    let list: Vec<Applied> = shown.filters.clone();
    Grid::new("vg-filters").num_columns(3).striped(true).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
        for (i, a) in list.iter().enumerate() {
            let mut visible = !a.over.hidden;
            let r = ui
                .scope(|ui| {
                    ui.set_min_width(LIST_NAME);
                    ui.add_enabled(enabled, Checkbox::new(&mut visible, &a.filter))
                })
                .inner;
            if r.changed()
                && let Some(x) = d.draft.filters.get_mut(i)
            {
                x.over.hidden = !visible;
            }
            let picked = d.pick == Some(Pick::Filter(i));
            let name = tf("Override {}", &[&a.filter]);
            let b = ui.add(Button::new(t("Override…")).selected(picked));
            b.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name.clone()));
            if b.clicked() {
                d.pick = Some(Pick::Filter(i));
            }
            ui.label(RichText::new(if a.over.is_empty() { "" } else { t("Overridden") }).color(pal().muted));
            ui.end_row();
        }
    });
    if list.is_empty() {
        ui.label(RichText::new(t("No filters applied.")).color(pal().muted));
    }
    ui.add_space(4.0);
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
    if let Some(Pick::Filter(i)) = d.pick.clone()
        && let Some(a) = shown.filters.get(i)
    {
        let mut o = a.over.clone();
        ui.separator();
        ui.label(RichText::new(tf("Overrides for {}", &[&a.filter])).strong());
        if editor(ui, &mut o, enabled, true)
            && let Some(x) = d.draft.filters.get_mut(i)
        {
            x.over = o;
        }
    }
}

/// The Elements tab: the elements hidden or overridden in this view, each visible or not, with
/// Override…, and Add Selected for what's selected.
fn elements(ui: &mut Ui, d: &mut VgDialog, model: &Model, selection: &[Id]) {
    let ids: Vec<Id> = d.listed.iter().copied().filter(|id| model.elements.contains_key(id)).collect();
    ScrollArea::vertical().max_height(200.0).auto_shrink([false, true]).id_salt("vg-elements").show(ui, |ui| {
        Grid::new("vg-elements-grid").num_columns(3).striped(true).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
            for id in &ids {
                let o = d.draft.elements.get(&model.guid(*id)).cloned().unwrap_or_default();
                let mut visible = !o.hidden;
                let name = element_name(model, *id);
                let r = ui
                    .scope(|ui| {
                        ui.set_min_width(LIST_NAME);
                        ui.add(Checkbox::new(&mut visible, &name))
                    })
                    .inner;
                if r.changed() {
                    let mut o = o.clone();
                    o.hidden = !visible;
                    put_element(d, model.guid(*id), o);
                }
                let picked = d.pick == Some(Pick::Element(*id));
                let label = tf("Override {}", &[&name]);
                let b = ui.add(Button::new(t("Override…")).selected(picked));
                b.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label.clone()));
                if b.clicked() {
                    d.pick = Some(Pick::Element(*id));
                }
                ui.label(RichText::new(if o.is_empty() { "" } else { t("Overridden") }).color(pal().muted));
                ui.end_row();
            }
        });
    });
    if ids.is_empty() {
        ui.label(RichText::new(t("No element is hidden or overridden here: select some and Add Selected.")).color(pal().muted));
    }
    ui.add_space(4.0);
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
    if let Some(Pick::Element(id)) = d.pick.clone() {
        let mut o = d.draft.elements.get(&model.guid(id)).cloned().unwrap_or_default();
        ui.separator();
        ui.label(RichText::new(tf("Overrides for {}", &[&element_name(model, id)])).strong());
        if editor(ui, &mut o, true, true) {
            put_element(d, model.guid(id), o);
        }
    }
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
        let named = |what: &str| format!("{}: {}", t(section), t(what));
        // A colour: a box that overrides it, and the colour once it does.
        let colour = |ui: &mut Ui, what: &str, value: &mut Option<[u8; 3]>, start: [u8; 3]| {
            let mut on = value.is_some();
            let name = named(what);
            let r = ui.checkbox(&mut on, t(what));
            r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, name.clone()));
            if r.changed() {
                *value = on.then_some(start);
            }
            if let Some(rgb) = value {
                let c = ui.color_edit_button_srgb(rgb);
                c.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name.clone()));
            } else {
                ui.label("");
            }
            ui.end_row();
        };
        colour(ui, "Line color", &mut s.color, [40, 40, 40]);
        // The weight.
        let mut on = s.weight.is_some();
        let name = named("Line weight");
        let r = ui.checkbox(&mut on, t("Line weight"));
        r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, name.clone()));
        if r.changed() {
            s.weight = on.then_some(weight);
        }
        if let Some(w) = &mut s.weight {
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
        // The dash.
        let mut on = s.dash.is_some();
        let name = named("Line dash");
        let r = ui.checkbox(&mut on, t("Line dash"));
        r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, name.clone()));
        if r.changed() {
            s.dash = on.then_some(Dash::Dashed);
        }
        if let Some(k) = &mut s.dash {
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
        colour(ui, "Fill color", &mut s.fill, [200, 200, 200]);
    });
}

fn dash_name(d: Dash) -> &'static str {
    match d {
        Dash::Solid => "Solid",
        Dash::Dashed => "Dashed",
        Dash::Dotted => "Dotted",
    }
}
