//! View Templates, as Revit's dialog: on the left the project's templates, a tab for each kind of view they are for (plans,
//! elevations and sections, 3D views), with New from This View, Duplicate, Rename and Delete under them; on the right the one
//! picked, its settings in a table of Parameter, Value and Include in the look Type Properties has. Its settings wait in a draft
//! until OK or Apply sends each template's as one `template` command; New, Duplicate, Rename and Delete run at once, as Type
//! Properties' Duplicate and Rename do. Visibility/Graphics and Background Grids open on a template's draft from here. Opened from a view's
//! Properties, the list starts with <None>, and OK also sets the template the view follows.

use std::collections::BTreeMap;
use std::sync::Arc;

use bimcraft_core::graphics::{Group, ViewKind, ViewTemplate};
use bimcraft_core::model::{Model, ViewRange};
use bimcraft_i18n::msg;
use egui::text::{CCursor, CCursorRange};
use egui::{
    Align, Button, Checkbox, ComboBox, Context, Frame, Id as UiId, Key, Label, Layout, Rect, RichText, ScrollArea, Sense, Stroke, TextEdit,
    Ui, UiBuilder, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use crate::graphics::{FILTERS_TAB, OPTIONS_TAB, VgDialog, detail_combo, group_name, merge_patch, scale_combo, style_combo};
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::{pal, semibold};
use crate::{BimApp, Sheet};

/// The dialog's width, and its list's.
const WIDTH: f32 = 860.0;
const LIST: f32 = 270.0;

/// How tall the list and the table are.
const BODY: f32 = 380.0;

/// How tall a row of the table is.
const ROW: f32 = 26.0;

/// Where the Value and Include columns start, as shares of the table's width.
const VALUE: f32 = 0.42;
const INCLUDE: f32 = 0.84;

/// What each tab lists, and what it is called.
fn kind_name(k: ViewKind) -> &'static str {
    match k {
        ViewKind::Plan => msg!("Plan"),
        ViewKind::Section => msg!("Elevation/Section"),
        ViewKind::ThreeD => msg!("3D"),
    }
}

/// A name being typed under the list: for a template made from the active view, for a copy of the one picked, or for it.
#[derive(Clone, Debug, PartialEq)]
enum Naming {
    New(String),
    Duplicate(String),
    Rename(String),
}

/// The View Templates dialog: the tab showing, the template picked (none: <None>, or nothing to pick), the name being typed,
/// each template's settings as the dialog has edited them, the view it assigns a template to when Properties opened it, the
/// View Range dialog's fields while it's open, and what stopped the last command.
pub(crate) struct TemplatesDialog {
    kind: ViewKind,
    picked: Option<String>,
    naming: Option<Naming>,
    drafts: BTreeMap<String, ViewTemplate>,
    assign: Option<Value>,
    range: Option<[String; 4]>,
    error: Option<String>,
}

/// Whether template `t` is listed on the tab for `kind`: one for any view is on every tab.
fn listed(t: &ViewTemplate, kind: ViewKind) -> bool {
    t.kind.is_none_or(|k| k == kind)
}

/// The settings of a template, Revit's order: how it draws, what it shows, then its range and phase.
const ORDER: [Group; 9] = [
    Group::Scale,
    Group::Detail,
    Group::Style,
    Group::Categories,
    Group::Filters,
    Group::Options,
    Group::Range,
    Group::Phase,
    Group::BackgroundGrid,
];

/// The `template` command's parameters that turn `was` into `now`: none for no difference.
fn patch(was: &ViewTemplate, now: &ViewTemplate) -> Option<serde_json::Map<String, Value>> {
    let (a, b) = (&was.graphics, &now.graphics);
    let mut p = serde_json::Map::new();
    if was.controls != now.controls {
        p.insert("controls".into(), json!(now.controls));
    }
    if a.scale != b.scale
        && let Some(n) = b.scale
    {
        p.insert("scale".into(), json!(n));
    }
    if a.detail != b.detail {
        p.insert("detail".into(), json!(b.detail.name()));
    }
    if a.style != b.style {
        p.insert("visual_style".into(), json!(b.style.name()));
    }
    if a.phase != b.phase {
        p.insert("phase".into(), json!(b.phase));
    }
    if a.phase_filter != b.phase_filter {
        p.insert("phase_filter".into(), json!(b.phase_filter));
    }
    if was.view_range != now.view_range
        && let Some(r) = now.view_range
    {
        p.insert("view_range".into(), json!(r));
    }
    if a.categories != b.categories {
        p.insert("categories".into(), merge_patch(&json!(a.categories), &json!(b.categories)));
    }
    if a.filters != b.filters {
        p.insert("filters".into(), json!(b.filters));
    }
    if a.options != b.options {
        p.insert("options".into(), merge_patch(&json!(a.options), &json!(b.options)));
    }
    if a.background_grid != b.background_grid {
        p.insert("background_grid".into(), json!(b.background_grid));
    }
    (!p.is_empty()).then_some(p)
}

/// `base`, or `base 2`, `base 3`…: the first the project hasn't.
fn fresh(m: &Model, base: &str) -> String {
    std::iter::once(base.to_string())
        .chain((2..).map(|n| format!("{base} {n}")))
        .find(|n| !m.view_templates.contains_key(n))
        .unwrap_or_default()
}

/// What the dialog asks of the project once it's drawn.
enum Ask {
    New(String),
    Duplicate(String),
    Rename(String),
    Delete,
    /// Visibility/Graphics for the template picked, on a tab.
    Graphics(usize),
    /// The View Range dialog for it.
    Range,
    /// Background Grids for it.
    Grid,
}

impl TemplatesDialog {
    /// The template picked as the dialog has it, if one is.
    fn current(&self, m: &Model) -> Option<ViewTemplate> {
        let name = self.picked.as_ref()?;
        self.drafts.get(name).or_else(|| m.view_templates.get(name)).cloned()
    }

    /// The draft of template `name`, to edit: the project's to start with.
    pub(crate) fn draft(&mut self, m: &Model, name: &str) -> Option<&mut ViewTemplate> {
        if !self.drafts.contains_key(name) {
            let t = m.view_templates.get(name)?.clone();
            self.drafts.insert(name.to_string(), t);
        }
        self.drafts.get_mut(name)
    }

    /// The first template on the tab showing, or none.
    fn first(&self, m: &Model) -> Option<String> {
        m.view_templates.iter().find(|(_, t)| listed(t, self.kind)).map(|(n, _)| n.clone())
    }
}

impl BimApp {
    /// The kind of view the active tab is, for a template; none for a sheet or a schedule.
    fn view_kind(&self) -> Option<ViewKind> {
        match self.sheet {
            Sheet::Plan(_) | Sheet::Ceiling(_) => Some(ViewKind::Plan),
            Sheet::Section(id) => self.session.model.elements.get(id).and_then(ViewKind::of),
            Sheet::View3D => Some(ViewKind::ThreeD),
            _ => None,
        }
    }

    /// View Templates (VT): the project's templates, on the tab for the active view's kind, the one it follows picked.
    pub(crate) fn open_templates(&mut self) {
        let kind = self.view_kind().unwrap_or(ViewKind::Plan);
        let followed = self.graphic_view().and_then(|_| self.graphics_now().template);
        let mut d =
            TemplatesDialog { kind, picked: followed, naming: None, drafts: BTreeMap::new(), assign: None, range: None, error: None };
        if d.picked.as_ref().is_none_or(|n| !self.session.model.view_templates.get(n).is_some_and(|t| listed(t, kind))) {
            d.picked = d.first(&self.session.model);
        }
        self.templates_dialog = Some(d);
    }

    /// Assign View Template, from a view's Properties: the same dialog, <None> first, and the view's template picked.
    pub(crate) fn assign_template(&mut self) {
        let Some(view) = self.graphic_view() else { return };
        let kind = self.view_kind().unwrap_or(ViewKind::Plan);
        self.templates_dialog = Some(TemplatesDialog {
            kind,
            picked: self.graphics_now().template,
            naming: None,
            drafts: BTreeMap::new(),
            assign: Some(view),
            range: None,
            error: None,
        });
    }

    /// The View Templates dialog, while it's open. Visibility/Graphics, opened on one of its templates, stands in its place
    /// until it closes.
    pub(crate) fn templates_window(&mut self, ctx: &Context) {
        if self.vg_dialog.as_ref().is_some_and(VgDialog::of_template) || self.template_grid_open() {
            return;
        }
        let Some(mut d) = self.templates_dialog.take() else { return };
        let model = Arc::clone(&self.session.model);
        let here = self.view_kind();
        let view = self.graphic_view();
        let title = if d.assign.is_some() { t("Assign View Template") } else { t("View Templates") };
        let (mut ok, mut cancel, mut apply, mut ask) = (false, false, false, None::<Ask>);
        let over = d.range.is_some();
        let r = Modal::new(UiId::new("view-templates"), title).width(WIDTH).closable(!over).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(LIST);
                        ask = list(ui, &mut d, &model, here, view.is_some());
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        if let Some(a) = settings(ui, &mut d, &model) {
                            ask = Some(a);
                        }
                    });
                });
                crate::typeprops::error(ui, d.error.as_deref());
            });
            crate::popup::footer(ui, |ui| {
                if over {
                    ui.allocate_space(vec2(ui.available_width(), ui.spacing().interact_size.y));
                } else {
                    (ok, cancel, apply) = crate::typeprops::foot(ui);
                }
            });
        });
        range_window(ctx, &mut d, &model);
        match ask {
            Some(Ask::New(name)) => {
                if let Some(from) = view.clone()
                    && self.run_for_type("template", json!({"name": name, "from": from}), &mut d.error)
                {
                    d.picked = Some(name);
                }
            }
            Some(Ask::Duplicate(name)) => {
                if let Some(like) = d.picked.clone()
                    && self.run_for_type("template", json!({"name": name, "like": like}), &mut d.error)
                {
                    // What the copy's source holds as the dialog has it, unsaved edits and all.
                    if let Some(t) = d.drafts.get(&like).cloned() {
                        d.drafts.insert(name.clone(), t);
                    }
                    d.picked = Some(name);
                }
            }
            Some(Ask::Rename(to)) => {
                if let Some(from) = d.picked.clone()
                    && self.run_for_type("template", json!({"name": from, "rename": to}), &mut d.error)
                {
                    if let Some(t) = d.drafts.remove(&from) {
                        d.drafts.insert(to.clone(), t);
                    }
                    d.picked = Some(to);
                }
            }
            Some(Ask::Delete) => {
                if let Some(name) = d.picked.clone()
                    && self.run_for_type("template", json!({"name": name, "delete": true}), &mut d.error)
                {
                    d.drafts.remove(&name);
                    d.picked = if d.assign.is_some() { None } else { d.first(&self.session.model) };
                }
            }
            Some(Ask::Range) => {}
            Some(Ask::Grid) => {
                if let Some(name) = d.picked.clone()
                    && let Some(t) = d.current(&model)
                {
                    self.open_template_background_grid(name, t.graphics.background_grid);
                }
            }
            Some(Ask::Graphics(tab)) => {
                if let Some(name) = d.picked.clone()
                    && let Some(t) = d.current(&model)
                {
                    self.vg_dialog = Some(VgDialog::for_template(name, t.graphics, tab));
                }
            }
            None => {}
        }
        // OK and Apply: each template's edits as one command, then the view's template; a refusal keeps the dialog open.
        let mut failed = false;
        if ok || apply {
            let drafts = std::mem::take(&mut d.drafts);
            for (name, now) in drafts {
                let Some(p) = self.session.model.view_templates.get(&name).and_then(|was| patch(was, &now)) else { continue };
                let mut p = p;
                p.insert("name".into(), json!(name));
                if !failed && self.run_for_type("template", Value::Object(p), &mut d.error) {
                    continue;
                }
                failed = true;
                d.drafts.insert(name, now);
            }
            if !failed && let Some(view) = &d.assign {
                let p = json!({"view": view, "template": d.picked});
                failed = !self.run_for_type("graphics", p, &mut d.error);
            }
        }
        if d.picked.as_ref().is_some_and(|n| !self.session.model.view_templates.contains_key(n)) {
            d.picked = None;
        }
        let done = (ok && !failed) || cancel || (r.should_close() && !over);
        if !done {
            self.templates_dialog = Some(d);
        }
    }
}

/// The left of the dialog: a tab for each kind of view, the templates of the one showing, and the buttons that make, copy,
/// rename and delete them. `here` is the active view's kind; `has_view`, whether there is a view to make one from.
fn list(ui: &mut Ui, d: &mut TemplatesDialog, m: &Model, here: Option<ViewKind>, has_view: bool) -> Option<Ask> {
    let mut ask = None;
    ui.horizontal(|ui| {
        for k in ViewKind::ALL {
            if ui.selectable_label(d.kind == k, t(kind_name(k))).clicked() && d.kind != k {
                d.kind = k;
                d.naming = None;
                if d.picked.as_ref().and_then(|n| m.view_templates.get(n)).is_none_or(|t| !listed(t, k)) {
                    d.picked = if d.assign.is_some() { None } else { d.first(m) };
                }
            }
        }
    });
    ui.add_space(2.0);
    Frame::new().fill(pal().surface).stroke(Stroke::new(1.0, pal().border)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ScrollArea::vertical().id_salt("view-template-names").max_height(BODY).min_scrolled_height(BODY).auto_shrink([false, false]).show(
            ui,
            |ui| {
                ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.spacing_mut().button_padding = vec2(8.0, 4.0);
                    if d.assign.is_some() && ui.selectable_label(d.picked.is_none(), t("<None>")).clicked() {
                        d.picked = None;
                        d.naming = None;
                    }
                    let names: Vec<&String> = m.view_templates.iter().filter(|(_, t)| listed(t, d.kind)).map(|(n, _)| n).collect();
                    for name in &names {
                        if ui.selectable_label(d.picked.as_ref() == Some(*name), name.as_str()).clicked() {
                            d.picked = Some((*name).clone());
                            d.naming = None;
                        }
                    }
                    if names.is_empty() {
                        ui.add_space(6.0);
                        let what =
                            tf("No {} templates yet: New from This View makes one from a view of the kind.", &[t(kind_name(d.kind))]);
                        ui.add(Label::new(RichText::new(what).color(pal().muted)).wrap());
                    }
                });
            },
        );
    });
    ui.add_space(6.0);
    let picked = d.picked.clone().filter(|n| m.view_templates.contains_key(n));
    let width = (LIST - 2.0 * ui.spacing().item_spacing.x) / 3.0;
    let size = vec2(width, 0.0);
    ui.horizontal(|ui| {
        let can = picked.is_some();
        let on = |n: &Option<Naming>, f: fn(&Naming) -> bool| n.as_ref().is_some_and(f);
        let duplicating = on(&d.naming, |n| matches!(n, Naming::Duplicate(_)));
        if ui.add_enabled(can, Button::new(t("Duplicate…")).selected(duplicating).min_size(size)).clicked() {
            d.naming = (!duplicating).then(|| Naming::Duplicate(fresh(m, picked.as_deref().unwrap_or_default())));
            ui.data_mut(|w| w.insert_temp(UiId::new("view-template-focus"), true));
        }
        let renaming = on(&d.naming, |n| matches!(n, Naming::Rename(_)));
        if ui.add_enabled(can, Button::new(t("Rename…")).selected(renaming).min_size(size)).clicked() {
            d.naming = (!renaming).then(|| Naming::Rename(picked.clone().unwrap_or_default()));
            ui.data_mut(|w| w.insert_temp(UiId::new("view-template-focus"), true));
        }
        if ui.add_enabled(can, Button::new(t("Delete")).min_size(size)).clicked() {
            d.naming = None;
            ask = Some(Ask::Delete);
        }
    });
    // A template is made from a view of the kind the tab lists.
    let can = has_view && here == Some(d.kind);
    let making = matches!(d.naming, Some(Naming::New(_)));
    let r = ui.add_enabled(can, Button::new(t("New from This View…")).selected(making).min_size(vec2(LIST, 0.0)));
    let r = if can {
        r
    } else {
        r.on_disabled_hover_text(tf("Open a view of the kind ({}) to make a template from it.", &[t(kind_name(d.kind))]))
    };
    if r.clicked() {
        d.naming = (!making).then(|| Naming::New(fresh(m, &tf("{} Template", &[t(kind_name(d.kind))]))));
        ui.data_mut(|w| w.insert_temp(UiId::new("view-template-focus"), true));
    }
    if let Some(naming) = &mut d.naming {
        let (verb, typed) = match naming {
            Naming::New(s) | Naming::Duplicate(s) => (t("Create"), s),
            Naming::Rename(s) => (t("Rename"), s),
        };
        let mut done = None;
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let l = ui.label(t("Name:"));
            let id = UiId::new("view-template-naming");
            let r = ui.add(TextEdit::singleline(typed).id(id).desired_width(LIST - 130.0)).labelled_by(l.id);
            if ui.data_mut(|w| w.remove_temp::<bool>(UiId::new("view-template-focus"))).is_some() {
                r.request_focus();
                if let Some(mut state) = TextEdit::load_state(ui.ctx(), id) {
                    let all = CCursorRange::two(CCursor::new(0), CCursor::new(typed.chars().count()));
                    state.cursor.set_char_range(Some(all));
                    state.store(ui.ctx(), id);
                }
            }
            let entered = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let clicked = ui.add(Button::new(verb).min_size(vec2(70.0, 0.0))).clicked();
            if (entered || clicked) && !typed.trim().is_empty() {
                done = Some(typed.trim().to_string());
            }
        });
        if let Some(name) = done {
            ask = Some(match naming {
                Naming::New(_) => Ask::New(name),
                Naming::Duplicate(_) => Ask::Duplicate(name),
                Naming::Rename(_) => Ask::Rename(name),
            });
            d.naming = None;
        }
    }
    ask
}

/// The right of the dialog: the template picked, its settings, what it controls ticked under Include.
fn settings(ui: &mut Ui, d: &mut TemplatesDialog, m: &Model) -> Option<Ask> {
    let Some(name) = d.picked.clone() else {
        ui.add_space(30.0);
        let what = if d.assign.is_some() {
            t("<None>: the view follows no template, and its settings are its own.")
        } else {
            t("No template picked.")
        };
        ui.label(RichText::new(what).color(pal().muted));
        return None;
    };
    let tpl = d.current(m)?;
    let views = m
        .elements
        .values()
        .filter(|e| bimcraft_core::graphics::graphics_of_element(e).is_some_and(|g| g.template.as_ref() == Some(&name)))
        .count();
    ui.horizontal(|ui| {
        ui.label(RichText::new(t("View Properties")).font(semibold(13.0)));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(tf("Views following it: {}", &[&views.to_string()])).color(pal().muted));
        });
    });
    ui.add_space(4.0);
    let groups = tpl.kind.map_or_else(|| Group::ALL.to_vec(), ViewKind::groups);
    let mut ask = None;
    let mut now = tpl.clone();
    Frame::new().fill(pal().surface).stroke(Stroke::new(1.0, pal().border)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let width = ui.available_width();
        let (value, include) = ((width * VALUE).round(), (width * INCLUDE).round());
        let (r, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
        let p = ui.painter();
        p.rect_filled(r, 0.0, pal().panel);
        for (x, heading) in [(0.0, t("Parameter")), (value, t("Value")), (include, t("Include"))] {
            p.text(pos2(r.left() + x + 8.0, r.center().y), egui::Align2::LEFT_CENTER, heading, semibold(12.5), pal().muted);
        }
        for x in [value, include] {
            p.line_segment([pos2(r.left() + x, r.top()), pos2(r.left() + x, r.bottom())], Stroke::new(1.0, pal().border));
        }
        p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, pal().border));
        ScrollArea::vertical()
            .id_salt("view-template-rows")
            .max_height(BODY - ROW)
            .min_scrolled_height(BODY - ROW)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let top = ui.cursor().top();
                let mut rows = Rows { ui, value, include };
                for g in ORDER.into_iter().filter(|g| groups.contains(g)) {
                    if let Some(a) = group_rows(&mut rows, g, &mut now, m) {
                        ask = Some(a);
                    }
                }
                let Rows { ui, .. } = rows;
                let used = ui.cursor().top() - top;
                let body = BODY - ROW;
                if used < body {
                    ui.allocate_exact_size(vec2(width, body - used), Sense::hover());
                }
                let left = ui.min_rect().left();
                for x in [value, include] {
                    ui.painter().line_segment([pos2(left + x, top), pos2(left + x, top + used.max(body))], Stroke::new(1.0, pal().border));
                }
            });
    });
    if now != tpl
        && let Some(slot) = d.draft(m, &name)
    {
        *slot = now;
    }
    if matches!(ask, Some(Ask::Range)) {
        // The View Range row: its own small dialog, on the draft's range.
        let r = d.current(m).and_then(|t| t.view_range).unwrap_or_default();
        let units = m.units;
        d.range =
            Some([r.top.map(|v| units.length(v)).unwrap_or_default(), units.length(r.cut), units.length(r.bottom), units.length(r.depth)]);
        return None;
    }
    ask
}

/// The rows of the settings table, as [`settings`] fills them.
struct Rows<'u> {
    ui: &'u mut Ui,
    value: f32,
    include: f32,
}

impl Rows<'_> {
    /// A row: its parameter's name, what `add` draws in the Value column given the name's id, and, for the first row of a
    /// group, its Include box, named for the group.
    fn row<R>(&mut self, label: &str, include: Option<(Group, &mut bool)>, add: impl FnOnce(&mut Ui, UiId) -> R) -> R {
        let r = Rect::from_min_size(self.ui.cursor().min, vec2(self.ui.available_width(), ROW));
        self.ui.advance_cursor_after_rect(r);
        self.ui.painter().line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, pal().border.gamma_multiply(0.6)));
        let (v, i) = (r.left() + self.value, r.left() + self.include);
        let name = Rect::from_x_y_ranges(r.left() + 8.0..=v - 6.0, r.y_range());
        let mut cell =
            self.ui.new_child(UiBuilder::new().id_salt(("name", label)).max_rect(name).layout(Layout::left_to_right(Align::Center)));
        let l = cell.add(Label::new(label).truncate());
        let value = Rect::from_x_y_ranges(v + 4.0..=i - 4.0, r.y_range().shrink(2.0));
        let mut cell =
            self.ui.new_child(UiBuilder::new().id_salt(("value", label)).max_rect(value).layout(Layout::left_to_right(Align::Center)));
        let out = add(&mut cell, l.id);
        if let Some((group, on)) = include {
            let rect = Rect::from_x_y_ranges(i + 4.0..=r.right() - 4.0, r.y_range());
            let mut cell = self.ui.new_child(
                UiBuilder::new()
                    .id_salt(("include", label))
                    .max_rect(rect)
                    .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
            );
            let was = *on;
            let r = cell.add(Checkbox::without_text(on));
            let name = t(group_name(group)).to_string();
            r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, was, name.clone()));
        }
        out
    }

    /// A row of a button across the Value column that opens an editor: what `edit` asks, if it was clicked.
    fn edit(&mut self, label: &str, include: Option<(Group, &mut bool)>, edit: Ask) -> Option<Ask> {
        let named = tf("Edit {}", &[label]);
        self.row(label, include, |ui, _| {
            let r = crate::properties::wide_button(ui, t("Edit…"));
            r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, named.clone()));
            r.clicked().then_some(edit)
        })
    }
}

/// The rows of group `g` of template `now`, editing it; what one asks of the dialog.
fn group_rows(rows: &mut Rows, g: Group, now: &mut ViewTemplate, m: &Model) -> Option<Ask> {
    let mut on = now.controls.contains(&g);
    let was = on;
    let include = Some((g, &mut on));
    let gr = &mut now.graphics;
    let ask = match g {
        Group::Scale => {
            rows.row(t("View Scale"), include, |ui, l| {
                if let Some(n) = scale_combo(ui, "template-scale", l, gr.scale_n(m.units), m.units) {
                    gr.scale = Some(n);
                }
            });
            None
        }
        Group::Detail => {
            rows.row(t("Detail Level"), include, |ui, l| {
                if let Some(level) = detail_combo(ui, "template-detail", l, gr.detail) {
                    gr.detail = level;
                }
            });
            None
        }
        Group::Style => {
            rows.row(t("Visual Style"), include, |ui, l| {
                if let Some(v) = style_combo(ui, "template-style", l, gr.style) {
                    gr.style = v;
                }
            });
            None
        }
        Group::Categories => rows.edit(t("V/G Overrides"), include, Ask::Graphics(0)),
        Group::Filters => rows.edit(t("V/G Filters"), include, Ask::Graphics(FILTERS_TAB)),
        Group::Options => rows.edit(t("Design Options"), include, Ask::Graphics(OPTIONS_TAB)),
        Group::Range => rows.edit(t("View Range"), include, Ask::Range),
        Group::BackgroundGrid => rows.edit(t("Background Grids"), include, Ask::Grid),
        Group::Phase => {
            let none = t("<None>").to_string();
            rows.row(t("Phase Filter"), include, |ui, l| {
                let now = gr.phase_filter.clone().unwrap_or_else(|| none.clone());
                let options: Vec<String> = std::iter::once(none.clone()).chain(m.phase_filters.keys().cloned()).collect();
                if let Some(picked) = crate::properties::combo(ui, (0, "template-phase-filter"), l, &now, &options) {
                    gr.phase_filter = (picked != none).then_some(picked);
                }
            });
            rows.row(t("Phase"), None, |ui, l| {
                let shown = gr.phase.clone().or_else(|| m.phases.last().cloned()).unwrap_or_default();
                let c = ComboBox::from_id_salt((0, "template-phase")).selected_text(&shown).width(ui.available_width()).show_ui(ui, |ui| {
                    for p in &m.phases {
                        if ui.selectable_label(*p == shown, p).clicked() {
                            gr.phase = Some(p.clone());
                        }
                    }
                });
                c.response.labelled_by(l);
            });
            None
        }
    };
    if on != was {
        if on {
            now.controls.insert(g);
        } else {
            now.controls.remove(&g);
        }
    }
    ask
}

/// The View Range dialog over the templates', while it's open: the template's range, typed in the project's units; OK puts it
/// in the draft.
fn range_window(ctx: &Context, d: &mut TemplatesDialog, m: &Model) {
    let Some(mut fields) = d.range.take() else { return };
    let (mut ok, mut cancel) = (false, false);
    let r = Modal::new(UiId::new("view-template-range"), t("View Range")).width(340.0).show(ctx, |ui| {
        crate::popup::body(ui, |ui| {
            egui::Grid::new("view-template-range-fields").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                for (i, (label, hint)) in
                    [(t("Top"), t("Level Above")), (t("Cut Plane"), ""), (t("Bottom"), ""), (t("View Depth"), "")].into_iter().enumerate()
                {
                    let l = ui.label(label);
                    ui.add_sized([160.0, ui.spacing().interact_size.y], TextEdit::singleline(&mut fields[i]).hint_text(hint))
                        .labelled_by(l.id);
                    ui.end_row();
                }
            });
            ui.label(RichText::new(t("Offsets from the plan's level.")).color(pal().muted));
        });
        crate::popup::footer(ui, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                let size = vec2(76.0, 0.0);
                cancel = ui.add(Button::new(t("Cancel")).min_size(size)).clicked();
                ok = ui.add(Button::new(t("OK")).min_size(size)).clicked();
            });
        });
    });
    let mut keep = !(ok || cancel || r.should_close());
    if ok {
        let units = m.units;
        let parse = |s: &str| units.parse(s);
        let top = if fields[0].trim().is_empty() { Some(None) } else { parse(&fields[0]).map(Some) };
        match (top, parse(&fields[1]), parse(&fields[2]), parse(&fields[3])) {
            (Some(top), Some(cut), Some(bottom), Some(depth)) => {
                if let Some(name) = d.picked.clone()
                    && let Some(t) = d.draft(m, &name)
                {
                    t.view_range = Some(ViewRange { top, cut, bottom, depth });
                }
                d.error = None;
            }
            _ => {
                d.error = Some(t("Each is a length, and Top may be left empty for the level above.").to_string());
                keep = true;
            }
        }
    }
    if keep {
        d.range = Some(fields);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_draft_with_no_difference_is_no_command_and_one_with_some_is_their_keys_alone() {
        let was = ViewTemplate { controls: Group::ALL.into_iter().collect(), ..ViewTemplate::default() };
        assert!(patch(&was, &was).is_none());
        let mut now = was.clone();
        now.controls.remove(&Group::Range);
        now.graphics.detail = bimcraft_core::graphics::Detail::Coarse;
        let p = patch(&was, &now).unwrap();
        assert_eq!(p.keys().collect::<Vec<_>>(), ["controls", "detail"]);
        assert_eq!(p["detail"], bimcraft_core::graphics::Detail::Coarse.name());
    }
}
