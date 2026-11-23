//! A plugin's palette: the widgets its manifest declares, drawn by the window, and the calls its buttons make. The
//! values are the call's `params`; the answer's `panel` fills the widgets, the table and the text. Nothing here is the
//! plugin's code: it declares, the window draws, and a call still goes through the session's transaction.

use std::collections::BTreeMap;

use bimcraft_core::units::Units;
use bimcraft_plugin::{Align, Column, PanelAnswer, PanelTable, Unit, Widget};
use egui::{ComboBox, Grid, Label, Layout, RichText, ScrollArea, TextEdit, Ui};
use serde_json::{Map, Value, json};

use crate::BimApp;
use crate::dock;
use crate::lang::{t, tf};
use crate::style::pal;

/// A widget's value while the panel is open: what the user typed, or what the answer gave.
#[derive(Clone, Debug, PartialEq)]
enum PanelValue {
    Text(String),
    Number(String),
    Bool(bool),
    Select(Value),
}

impl PanelValue {
    /// Its JSON, as a call's parameter or a button's condition reads it.
    fn json(&self) -> Value {
        match self {
            PanelValue::Text(s) => Value::String(s.clone()),
            PanelValue::Number(s) => match s.trim() {
                "" => Value::Null,
                // The window shows numbers as text; the call gets the number typed, in the project's units.
                text => text.parse::<f64>().map_or_else(|_| Value::String(text.to_string()), |n| json!(n)),
            },
            PanelValue::Bool(b) => Value::Bool(*b),
            PanelValue::Select(v) => v.clone(),
        }
    }

    fn text(&self) -> String {
        match self {
            PanelValue::Text(s) | PanelValue::Number(s) => s.clone(),
            PanelValue::Bool(b) => b.to_string(),
            PanelValue::Select(v) => v.as_str().map_or_else(|| v.to_string(), str::to_string),
        }
    }
}

/// What a plugin's panel holds while it is open: its fields' values, the text of its `text_panel`s and read-only
/// `text` widgets, its table, its badge, its error line. Kept in the window, never in the project or the settings file.
#[derive(Clone, Debug, Default)]
pub struct PluginPanelState {
    values: BTreeMap<String, PanelValue>,
    texts: BTreeMap<String, String>,
    table: Option<PanelTable>,
    badge: String,
    error: Option<String>,
    /// Its `on_open` has been called.
    opened: bool,
    /// A call is running: the panel's buttons wait. A call is synchronous, so this is set and cleared within one frame.
    running: bool,
}

/// A button pressed: the function it calls, and whether the panel's refresh runs after it.
struct Action {
    function: String,
    refresh: bool,
}

impl BimApp {
    /// The badge a plugin panel shows beside its title: its answer's, or nothing.
    pub(crate) fn plugin_panel_badge(&self, index: usize) -> String {
        let Some(panel) = dock::plugin_palette(index) else { return String::new() };
        self.plugin_panel_states.get(panel.name).map(|state| state.badge.clone()).unwrap_or_default()
    }

    /// The body of the palette [`crate::dock::Palette::Plugin`] `index`: its label, its fields, its buttons, its table
    /// and its text, as its manifest declares them, and the answer's error line at the foot. The first frame it is open
    /// calls the panel's `on_open`.
    pub(crate) fn plugin_panel_body(&mut self, ui: &mut Ui, index: usize) {
        let Some(panel) = dock::plugin_palette(index) else {
            ui.label(RichText::new(t("The plugin's panel couldn't be shown")).color(pal().error));
            return;
        };
        let name = panel.name;
        let widgets = panel.widgets.clone();
        let (on_open, refresh) = (panel.on_open.map(str::to_string), panel.refresh.map(str::to_string));
        let mut state = self.plugin_panel_states.get(name).cloned().unwrap_or_default();
        if !state.opened {
            state.opened = true;
            self.plugin_panel_states.insert(name.to_string(), state.clone());
            if let Some(function) = &on_open {
                self.run_panel_function(name, function, false);
                state = self.plugin_panel_states.get(name).cloned().unwrap_or_default();
            }
        }
        let info = PanelInfo::of(self);
        let mut actions = Vec::new();
        egui::Frame::new().inner_margin(8.0).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if let Some(function) = &refresh
                && !state.running
                && ui.button(t("Refresh")).clicked()
            {
                actions.push(Action { function: function.clone(), refresh: false });
            }
            draw_widgets(ui, &widgets, &mut state, &info, &mut actions);
            if let Some(error) = &state.error {
                ui.add_space(4.0);
                ui.add(Label::new(RichText::new(error).color(pal().error)).selectable(true));
            }
            if state.running {
                ui.add_space(4.0);
                ui.label(RichText::new(t("Running…")).color(pal().muted));
            }
        });
        self.plugin_panel_states.insert(name.to_string(), state);
        for action in actions {
            self.run_panel_function(name, &action.function, action.refresh);
        }
    }

    /// Runs a panel's function with the panel's current values, and shows its answer: one call, one step of undo, as
    /// any plugin's function. `refresh` runs the panel's own refresh after it, as a button does.
    pub(crate) fn run_panel_function(&mut self, name: &str, function: &str, refresh: bool) {
        let Some(panel) = dock::plugin_palettes().into_iter().find(|panel| panel.name == name) else { return };
        let info = PanelInfo::of(self);
        let mut state = self.plugin_panel_states.get(name).cloned().unwrap_or_default();
        let params = match resolve_params(&panel.widgets, &mut state, &info) {
            Ok(params) => params,
            Err(why) => {
                state.error = Some(why);
                self.plugin_panel_states.insert(name.to_string(), state);
                return;
            }
        };
        let id = format!("plugin:{}:{function}", panel.plugin);
        state.running = true;
        state.error = None;
        self.plugin_panel_states.insert(name.to_string(), state);
        let answer = self.run(&id, Value::Object(params));
        if let Some(state) = self.plugin_panel_states.get_mut(name) {
            state.running = false;
            // A call that failed — a trap, a limit, a refused change — says so in the panel too, and leaves what it showed.
            if answer.is_none() && self.status.error {
                state.error = Some(self.status.text.clone());
            }
        }
        if let Some(value) = answer {
            if let Some(panel_answer) = value.get("panel") {
                match PanelAnswer::from_json(panel_answer) {
                    Ok(answer) => self.apply_panel_answer(name, &panel.widgets, answer),
                    Err(why) => {
                        self.plugins.note(panel.plugin, bimcraft_plugin::Level::Warning, why.clone());
                        self.status = crate::Status { text: tf("The plugin's panel couldn't be shown: {}", &[&why]), error: true };
                    }
                }
            }
            if let Some(report) = value.get("report") {
                match crate::plugins::report_parts(report) {
                    Ok((text, format)) => self.open_plugin_report(panel.title.to_string(), &text, format),
                    Err(why) => self.plugins.note(panel.plugin, bimcraft_plugin::Level::Warning, why),
                }
            }
        }
        if refresh && let Some(function) = panel.refresh {
            self.run_panel_function(name, function, false);
        }
    }

    /// Puts an answer's values into the panel's widgets, its table, text, badge and error line. A value for a widget the
    /// panel hasn't is one line in the plugin log.
    fn apply_panel_answer(&mut self, name: &str, widgets: &[Widget], answer: PanelAnswer) {
        let units = self.session.model.units;
        let mut state = self.plugin_panel_states.get(name).cloned().unwrap_or_default();
        let mut known = Vec::new();
        collect_ids(widgets, &mut known);
        let plugin = dock::plugin_palettes().into_iter().find(|panel| panel.name == name).map_or("plugins", |panel| panel.plugin);
        for (id, value) in answer.values {
            if !known.iter().any(|k| k == &id) {
                self.plugins.note(plugin, bimcraft_plugin::Level::Info, format!("the panel's answer names `{id}`, which the panel hasn't"));
                continue;
            }
            let Some(widget) = find_widget(widgets, &id) else { continue };
            if let Some(value) = value_of(widget, &value, units) {
                state.values.insert(id, value);
            }
        }
        if let Some(text) = answer.text {
            for widget in text_widgets(widgets) {
                if let Widget::TextPanel { id, .. } | Widget::Text { id, readonly: true, .. } = widget {
                    state.texts.insert(id.clone(), text.clone());
                }
            }
        }
        if let Some(table) = answer.table {
            state.table = Some(table);
        }
        if let Some(badge) = answer.badge {
            state.badge = badge;
        }
        state.error = answer.error;
        self.plugin_panel_states.insert(name.to_string(), state);
    }
}

/// What the panel's widgets are drawn against: the project's levels and library, and the selection's size.
struct PanelInfo {
    levels: Vec<String>,
    library: Vec<(String, String)>,
    selected: usize,
    units: Units,
}

impl PanelInfo {
    fn of(app: &BimApp) -> PanelInfo {
        let model = &app.session.model;
        PanelInfo {
            levels: model.levels().iter().map(|(_, level)| level.name.clone()).collect(),
            library: model.library.entries().map(|(_, e)| (e.kind.to_string(), e.name.clone())).collect(),
            selected: app.selection.len(),
            units: model.units,
        }
    }

    fn of_kind(&self, kind: &str) -> Vec<&str> {
        self.library.iter().filter(|(k, _)| k == kind).map(|(_, name)| name.as_str()).collect()
    }
}

fn collect_ids(widgets: &[Widget], ids: &mut Vec<String>) {
    for widget in widgets {
        if let Some(id) = widget.id() {
            ids.push(id.to_string());
        }
        collect_ids(widget.children(), ids);
    }
}

fn find_widget<'a>(widgets: &'a [Widget], id: &str) -> Option<&'a Widget> {
    for widget in widgets {
        if widget.id() == Some(id) {
            return Some(widget);
        }
        if let Some(found) = find_widget(widget.children(), id) {
            return Some(found);
        }
    }
    None
}

fn text_widgets(widgets: &[Widget]) -> Vec<&Widget> {
    let mut out = Vec::new();
    for widget in widgets {
        match widget {
            Widget::TextPanel { .. } | Widget::Text { .. } => out.push(widget),
            _ => out.extend(text_widgets(widget.children())),
        }
    }
    out
}

/// A value of an answer as the widget that takes it reads it: a number in the project's units, a text, a box's state
/// or a choice of its list. None: the widget doesn't take one.
fn value_of(widget: &Widget, value: &Value, units: Units) -> Option<PanelValue> {
    match widget {
        Widget::Text { .. } | Widget::Level { .. } | Widget::Library { .. } => value.as_str().map(|s| PanelValue::Text(s.to_string())),
        Widget::Number { unit, .. } => value.as_f64().map(|mm| PanelValue::Number(show_number(units, *unit, mm))),
        Widget::Checkbox { .. } => value.as_bool().map(PanelValue::Bool),
        Widget::Select { options, .. } => {
            options.iter().find(|c| values_equal(&c.value, value)).map(|c| PanelValue::Select(c.value.clone()))
        }
        _ => None,
    }
}

/// A number as a field shows it: a length in the project's units, an area or a volume likewise, an angle in degrees.
fn show_number(units: Units, unit: Unit, mm: f64) -> String {
    match unit {
        Unit::Length => units.field(mm),
        Unit::Area => format!("{:.2}", mm * units.area(1.0).0),
        Unit::Volume => format!("{:.3}", mm * units.area(1.0).0 / if units.imperial() { 304.8 } else { 1000.0 }),
        Unit::Angle | Unit::None => format!("{mm}"),
    }
}

/// How many millimetres, square millimetres, cubic millimetres or degrees one of the project's units is.
fn model_unit(units: Units, unit: Unit) -> f64 {
    let area = units.area(1.0).0;
    match unit {
        Unit::Length => 1.0,
        Unit::Area => 1.0 / area,
        Unit::Volume => {
            let linear = if units.imperial() { 304.8 } else { 1000.0 };
            linear / area
        }
        Unit::Angle | Unit::None => 1.0,
    }
}

/// The parameters a call of the panel gets: each writable widget's id and its value. A number the user couldn't type is
/// an error that names the field; a read-only widget is not sent.
fn resolve_params(widgets: &[Widget], state: &mut PluginPanelState, info: &PanelInfo) -> Result<Map<String, Value>, String> {
    let units = info.units;
    let mut params = Map::new();
    for widget in widgets {
        match widget {
            Widget::Text { id, readonly: false, .. } => {
                let text = state.values.get(id).map(PanelValue::text).unwrap_or_default();
                params.insert(id.clone(), Value::String(text));
            }
            Widget::Level { id, .. } => {
                let first = info.levels.first().cloned().unwrap_or_default();
                let value = state.values.entry(id.clone()).or_insert_with(|| PanelValue::Text(first));
                params.insert(id.clone(), Value::String(value.text()));
            }
            Widget::Library { id, kind, .. } => {
                let first = info.of_kind(kind).first().map_or_else(String::new, |n| (*n).to_string());
                let value = state.values.entry(id.clone()).or_insert_with(|| PanelValue::Text(first));
                params.insert(id.clone(), Value::String(value.text()));
            }
            Widget::Number { id, label, unit, min, max, .. } => {
                let text = state.values.get(id).map(PanelValue::text).unwrap_or_default();
                let text = text.trim();
                if text.is_empty() {
                    continue;
                }
                let n = read_number(units, *unit, text).ok_or_else(|| tf("{}: A number is expected", &[label]))?;
                if min.is_some_and(|min| n < min) || max.is_some_and(|max| n > max) {
                    return Err(tf("{}: Out of range", &[label]));
                }
                params.insert(id.clone(), json!(n));
            }
            Widget::Checkbox { id, .. } => {
                let on = matches!(state.values.get(id), Some(PanelValue::Bool(true)));
                params.insert(id.clone(), json!(on));
            }
            Widget::Select { id, .. } => {
                let value = state.values.get(id).map(PanelValue::json).unwrap_or(Value::Null);
                params.insert(id.clone(), value);
            }
            Widget::Row { widgets } | Widget::Group { widgets, .. } => {
                params.extend(resolve_params(widgets, state, info)?);
            }
            _ => {}
        }
    }
    Ok(params)
}

/// A number typed, in the widget's unit: a length with its unit (as any length field reads), an area, a volume or an
/// angle as a number of the project's.
fn read_number(units: Units, unit: Unit, text: &str) -> Option<f64> {
    match unit {
        Unit::Length => units.parse(text),
        Unit::Area | Unit::Volume => {
            let n = plain_number(text)?;
            let value = n * model_unit(units, unit);
            value.is_finite().then_some(value)
        }
        Unit::Angle | Unit::None => plain_number(text),
    }
}

/// A number with a unit's sign after it (`42.5 m²`, `3 m³`, `90°`): read as the number.
fn plain_number(text: &str) -> Option<f64> {
    let text = text.trim();
    for suffix in ["m²", "m2", "m³", "m3", "ft²", "ft2", "ft³", "ft3", "°", "deg", "mm"] {
        if let Some(rest) = text.strip_suffix(suffix) {
            return rest.trim().parse().ok();
        }
    }
    text.parse().ok()
}

fn draw_widgets(ui: &mut Ui, widgets: &[Widget], state: &mut PluginPanelState, info: &PanelInfo, actions: &mut Vec<Action>) {
    for widget in widgets {
        draw_widget(ui, widget, state, info, actions);
    }
}

fn draw_widget(ui: &mut Ui, widget: &Widget, state: &mut PluginPanelState, info: &PanelInfo, actions: &mut Vec<Action>) {
    let enabled = !state.running;
    match widget {
        Widget::Label { text } => {
            ui.add(Label::new(RichText::new(text)).wrap());
        }
        Widget::Row { widgets } => {
            // Equal columns: a field that takes `ui.available_width()` inside a plain horizontal row would take the
            // whole width and push its neighbours out of the palette.
            ui.columns(widgets.len().max(1), |cols| {
                for (col, widget) in cols.iter_mut().zip(widgets) {
                    draw_widget(col, widget, state, info, actions);
                }
            });
        }
        Widget::Group { title, widgets, collapsed } => {
            egui::CollapsingHeader::new(RichText::new(title).strong())
                .id_salt(("plugin-group", title))
                .default_open(!collapsed)
                .show(ui, |ui| draw_widgets(ui, widgets, state, info, actions));
        }
        Widget::Text { id, label, readonly, multiline, unit } => {
            let value = state.values.entry(id.clone()).or_insert_with(|| PanelValue::Text(String::new()));
            if *readonly {
                let text = state.texts.get(id).cloned().unwrap_or_default();
                caption(ui, label, *unit, info);
                ui.add(Label::new(RichText::new(text).monospace()).selectable(true));
            } else {
                let PanelValue::Text(text) = value else { return };
                let named = caption(ui, label, *unit, info);
                let edit = if *multiline { TextEdit::multiline(text).desired_rows(3) } else { TextEdit::singleline(text) };
                ui.add(edit.desired_width(ui.available_width())).labelled_by(named);
            }
        }
        Widget::Number { id, label, unit, .. } => {
            let value = state.values.entry(id.clone()).or_insert_with(|| PanelValue::Number(String::new()));
            let PanelValue::Number(text) = value else { return };
            let named = caption(ui, label, *unit, info);
            let hint = match unit {
                Unit::Length => info.units.length(0.0),
                _ => String::new(),
            };
            let edit = TextEdit::singleline(text).hint_text(hint).desired_width(ui.available_width());
            ui.add_enabled(enabled, edit).labelled_by(named);
        }
        Widget::Checkbox { id, label } => {
            let value = state.values.entry(id.clone()).or_insert(PanelValue::Bool(false));
            let PanelValue::Bool(on) = value else { return };
            ui.add_enabled(enabled, egui::Checkbox::new(on, label));
        }
        Widget::Select { id, label, options } => {
            let value = state.values.entry(id.clone()).or_insert_with(|| PanelValue::Select(options[0].value.clone()));
            let PanelValue::Select(now) = value else { return };
            let shown = options.iter().find(|c| &c.value == now).map_or_else(|| now.to_string(), |c| c.label.clone());
            let named = caption(ui, label, Unit::None, info);
            egui::ComboBox::from_id_salt(("plugin-select", id))
                .selected_text(shown)
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for choice in options {
                        let text = choice.label.clone();
                        ui.selectable_value(now, choice.value.clone(), text).labelled_by(named);
                    }
                })
                .response
                .labelled_by(named);
        }
        Widget::Level { id, label } => {
            let first = info.levels.first().cloned().unwrap_or_default();
            let value = state.values.entry(id.clone()).or_insert_with(|| PanelValue::Text(first));
            let PanelValue::Text(now) = value else { return };
            let named = caption(ui, label, Unit::None, info);
            ComboBox::from_id_salt(("plugin-level", id))
                .selected_text(now.clone())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for level in &info.levels {
                        ui.selectable_value(now, level.clone(), level).labelled_by(named);
                    }
                })
                .response
                .labelled_by(named);
        }
        Widget::Library { id, label, kind } => {
            let names = info.of_kind(kind);
            let first = names.first().map_or_else(String::new, |n| (*n).to_string());
            let value = state.values.entry(id.clone()).or_insert_with(|| PanelValue::Text(first));
            let PanelValue::Text(now) = value else { return };
            let named = caption(ui, label, Unit::None, info);
            ComboBox::from_id_salt(("plugin-library", id))
                .selected_text(now.clone())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for name in names {
                        ui.selectable_value(now, name.to_string(), name).labelled_by(named);
                    }
                })
                .response
                .labelled_by(named);
        }
        Widget::Selection { label, .. } => {
            let text =
                if info.selected == 0 { t("Nothing selected").to_string() } else { tf("{} selected", &[&info.selected.to_string()]) };
            caption(ui, label, Unit::None, info);
            ui.label(RichText::new(text).color(pal().muted));
        }
        Widget::Button { label, function, enabled_when, .. } => {
            let ready = enabled_when
                .as_ref()
                .is_none_or(|when| state.values.get(&when.widget).is_some_and(|value| values_equal(&value.json(), &when.equals)));
            if ui.add_enabled(enabled && ready, egui::Button::new(label)).on_hover_text(t("Runs the plugin's command")).clicked() {
                actions.push(Action { function: function.clone(), refresh: true });
            }
        }
        Widget::Table { id, columns } => {
            caption(ui, id, Unit::None, info);
            table_ui(ui, id, columns, state.table.as_ref());
        }
        Widget::TextPanel { id, height } => {
            let text = state.texts.get(id).cloned().unwrap_or_default();
            ScrollArea::vertical().id_salt(("plugin-text", id)).max_height(height.unwrap_or(160.0)).auto_shrink([false, true]).show(
                ui,
                |ui| {
                    ui.add(Label::new(RichText::new(text).monospace()).selectable(true));
                },
            );
        }
        Widget::Separator => {
            ui.separator();
        }
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        _ => a == b,
    }
}

/// A widget's label, above its field and named by it for a screen reader (and a test); it says the unit where there is
/// one. The answer's id names the response's accessibility node.
fn caption(ui: &mut Ui, label: &str, unit: Unit, info: &PanelInfo) -> egui::Id {
    let text = match unit {
        Unit::Length => format!("{} ({})", label, info.units.name()),
        Unit::Area => format!("{} ({})", label, info.units.area(1.0).1),
        Unit::Volume => format!("{} ({})", label, if info.units.imperial() { "ft³" } else { "m³" }),
        Unit::Angle => format!("{} (°)", label),
        Unit::None => label.to_string(),
    };
    ui.label(RichText::new(text).small().color(pal().muted)).id
}

/// A table of an answer: its columns' headings, then its rows, in a horizontal scroll so a long row is never wrapped.
fn table_ui(ui: &mut Ui, id: &str, columns: &[Column], table: Option<&PanelTable>) {
    let empty = PanelTable::default();
    let table = table.unwrap_or(&empty);
    ScrollArea::horizontal().id_salt(("plugin-table", id)).max_width(ui.available_width()).auto_shrink([false, true]).show(ui, |ui| {
        Grid::new(("plugin-grid", id)).striped(true).min_col_width(12.0).show(ui, |ui| {
            for column in columns {
                let heading = RichText::new(&column.label).strong();
                if let Some(width) = column.width {
                    ui.set_min_width(width);
                }
                ui.label(heading);
            }
            ui.end_row();
            for row in &table.rows {
                for (i, column) in columns.iter().enumerate() {
                    let cell = row.get(i).cloned().unwrap_or(Value::Null);
                    let text = match cell {
                        Value::Null => String::new(),
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    let width = ui.available_width();
                    let layout = match column.align {
                        Align::Left => Layout::left_to_right(egui::Align::Center),
                        Align::Center => Layout::centered_and_justified(egui::Direction::LeftToRight),
                        Align::Right => Layout::right_to_left(egui::Align::Center),
                    };
                    ui.with_layout(layout, |ui| {
                        if let Some(width) = column.width {
                            ui.set_min_width(width);
                        } else {
                            ui.set_min_width(width.min(160.0));
                        }
                        ui.add(Label::new(RichText::new(text)).wrap_mode(egui::TextWrapMode::Extend));
                    });
                }
                ui.end_row();
            }
        });
    });
}
