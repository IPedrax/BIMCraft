//! The form a plugin function's `params` make: the window draws a field for each property of the schema, and the values
//! are the call's parameters. The schema is free JSON, so what the window doesn't understand is a text field that says
//! so: it never refuses a plugin whose parameters it can't read.

use bimcraft_core::model::Model;
use bimcraft_plugin::{Choice, Function};
use egui::{Align, ComboBox, Context, Id as UiId, Label, Layout, RichText, TextEdit, Ui};
use serde_json::{Map, Value, json};

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;

/// A field of a plugin function's form.
#[derive(Clone, Debug)]
struct PluginField {
    name: String,
    title: String,
    description: String,
    required: bool,
    kind: Kind,
    /// What the user typed, or the choice taken.
    text: String,
    on: bool,
    choice: Option<Value>,
    min: Option<f64>,
    max: Option<f64>,
}

/// What a field is: the schema's `type` and its `x-bimcraft` kinds.
#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Bool,
    Integer,
    Number,
    Text,
    Choice(Vec<Choice>),
    Level,
    Length,
    Library(String),
    Selection,
    Path,
    /// A schema the host doesn't read: a text field that says so, and a refusal when it is required.
    Unsupported(String),
}

/// The values of a plugin function's parameters while the window is open, and what a run said was wrong.
#[derive(Clone, Debug, Default)]
pub struct PluginForm {
    pub label: String,
    fields: Vec<PluginField>,
    pub error: Option<String>,
}

impl PluginForm {
    /// Whether the function's `params` name properties the window draws a form for. None: the function runs in one click,
    /// as every function of ABI 1 did.
    pub fn has_fields(function: &Function) -> bool {
        function.params.get("properties").and_then(Value::as_object).is_some_and(|p| !p.is_empty())
    }

    /// The form of `function`, its fields as they start: their defaults, the project's first level or library entry, or
    /// what the schema says.
    pub fn from_schema(function: &Function, model: &Model) -> PluginForm {
        let mut form = PluginForm { label: function.label.clone(), fields: Vec::new(), error: None };
        let Some(properties) = function.params.get("properties").and_then(Value::as_object) else { return form };
        let required: Vec<&str> = function
            .params
            .get("required")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let units = model.units;
        let levels: Vec<String> = model.levels().iter().map(|(_, level)| level.name.clone()).collect();
        for (name, property) in properties {
            let kind = kind_of(property);
            let title =
                property.get("title").and_then(Value::as_str).filter(|t| !t.is_empty()).map_or_else(|| name.clone(), str::to_string);
            let description = property.get("description").and_then(Value::as_str).unwrap_or_default().to_string();
            let default = property.get("default");
            let library = |kind: &str| -> Vec<String> {
                model.library.entries().filter(|(_, e)| e.kind.to_string() == kind).map(|(_, e)| e.name.clone()).collect()
            };
            let (text, choice) = match &kind {
                Kind::Bool => (String::new(), None),
                Kind::Choice(options) => {
                    let value = default.cloned().or_else(|| options.first().map(|c| c.value.clone()));
                    (value.as_ref().map_or_else(String::new, |v| v.as_str().map_or_else(|| v.to_string(), str::to_string)), value)
                }
                Kind::Level => {
                    let value = default.and_then(Value::as_str).map(str::to_string).or_else(|| levels.first().cloned());
                    (value.unwrap_or_default(), None)
                }
                Kind::Library(kind) => {
                    let names = library(kind);
                    let value = default.and_then(Value::as_str).map(str::to_string).or_else(|| names.first().cloned());
                    (value.unwrap_or_default(), None)
                }
                Kind::Length => {
                    let mm = default.and_then(Value::as_f64).unwrap_or(0.0);
                    (if default.is_some() { units.field(mm) } else { String::new() }, None)
                }
                Kind::Integer | Kind::Number => (default.map_or_else(String::new, |v| v.to_string()), None),
                Kind::Text | Kind::Path => (default.and_then(Value::as_str).map_or_else(String::new, str::to_string), None),
                Kind::Selection => (String::new(), None),
                Kind::Unsupported(_) => (String::new(), None),
            };
            let on = default.and_then(Value::as_bool).unwrap_or(false);
            form.fields.push(PluginField {
                name: name.clone(),
                title,
                description,
                required: required.contains(&name.as_str()),
                kind,
                text,
                on,
                choice,
                min: property.get("minimum").and_then(Value::as_f64),
                max: property.get("maximum").and_then(Value::as_f64),
            });
        }
        form
    }

    /// The parameters a call gets: each field's value, in the units commands take (lengths in millimetres). A field that
    /// can't be read, or a required one left empty, is an error that names it.
    fn resolve(&self, model: &Model) -> Result<Map<String, Value>, String> {
        let units = model.units;
        let mut params = Map::new();
        for field in &self.fields {
            let what = &field.title;
            let value = match &field.kind {
                Kind::Bool => json!(field.on),
                Kind::Integer => {
                    let text = field.text.trim();
                    if text.is_empty() && !field.required {
                        continue;
                    }
                    let n = text.parse::<i64>().map_err(|_| tf("{}: A number is expected", &[what]))?;
                    json!(n)
                }
                Kind::Number => {
                    let text = field.text.trim();
                    if text.is_empty() && !field.required {
                        continue;
                    }
                    let n = text.parse::<f64>().map_err(|_| tf("{}: A number is expected", &[what]))?;
                    range(field, n)?;
                    json!(n)
                }
                Kind::Length => {
                    let text = field.text.trim();
                    if text.is_empty() && !field.required {
                        continue;
                    }
                    let mm = units.parse(text).ok_or_else(|| tf("{}: A number is expected", &[what]))?;
                    range(field, mm)?;
                    json!(mm)
                }
                Kind::Choice(options) => field
                    .choice
                    .clone()
                    .or_else(|| options.iter().find(|c| c.label == field.text).map(|c| c.value.clone()))
                    .ok_or_else(|| tf("{}: This field is required", &[what]))?,
                Kind::Level | Kind::Library(_) | Kind::Text | Kind::Path => {
                    let text = field.text.trim();
                    if text.is_empty() {
                        if field.required {
                            return Err(tf("{}: This field is required", &[what]));
                        }
                        continue;
                    }
                    json!(text)
                }
                Kind::Selection => continue,
                Kind::Unsupported(schema) => {
                    if field.required {
                        return Err(tf("Unsupported field: {}", &[&format!("{} ({schema})", field.name)]));
                    }
                    continue;
                }
            };
            params.insert(field.name.clone(), value);
        }
        Ok(params)
    }
}

fn kind_of(property: &Value) -> Kind {
    if let Some(kind) = property.get("x-bimcraft").and_then(|x| x.get("kind")).and_then(Value::as_str) {
        return match kind {
            "level" => Kind::Level,
            "length" => Kind::Length,
            "selection" => Kind::Selection,
            "path" => Kind::Path,
            other if other.starts_with("library:") => Kind::Library(other["library:".len()..].to_string()),
            other => Kind::Unsupported(format!("x-bimcraft kind `{other}`")),
        };
    }
    match property.get("enum").and_then(Value::as_array) {
        Some(values) if matches!(property.get("type").and_then(Value::as_str), Some("string" | "integer")) => {
            let mut options = Vec::new();
            for value in values {
                let label = value.as_str().map_or_else(|| value.to_string(), str::to_string);
                options.push(Choice { value: value.clone(), label });
            }
            if !options.is_empty() {
                return Kind::Choice(options);
            }
        }
        Some(values) => return Kind::Unsupported(format!("enum {values:?}")),
        None => {}
    }
    match property.get("type").and_then(Value::as_str) {
        Some("boolean") => Kind::Bool,
        Some("integer") => Kind::Integer,
        Some("number") => Kind::Number,
        Some("string") => Kind::Text,
        Some(other) => Kind::Unsupported(format!("type `{other}`")),
        None => Kind::Unsupported(property.to_string()),
    }
}

fn range(field: &PluginField, value: f64) -> Result<(), String> {
    if field.min.is_some_and(|min| value < min) || field.max.is_some_and(|max| value > max) {
        return Err(tf("{}: Out of range", &[&field.title]));
    }
    Ok(())
}

/// What the form's fields are drawn against: the project's levels and library, and its units.
struct FormInfo {
    model_levels: Vec<String>,
    library: Vec<(String, String)>,
    units: bimcraft_core::units::Units,
}

impl FormInfo {
    fn of(model: &Model) -> FormInfo {
        FormInfo {
            model_levels: model.levels().iter().map(|(_, level)| level.name.clone()).collect(),
            library: model.library.entries().map(|(_, e)| (e.kind.to_string(), e.name.clone())).collect(),
            units: model.units,
        }
    }

    fn of_kind(&self, kind: &str) -> Vec<&str> {
        self.library.iter().filter(|(k, _)| k == kind).map(|(_, name)| name.as_str()).collect()
    }
}

impl crate::BimApp {
    /// The form of the function a click asked for, while it is open. Run takes its values to the call, which is one step
    /// of undo as a click always was; Cancel closes it, and what was typed stays for the next click.
    pub(crate) fn plugin_form_window(&mut self, ctx: &Context) {
        let Some(id) = self.plugin_form_open.clone() else { return };
        let Some(mut form) = self.plugin_forms.get(&id).cloned() else {
            self.plugin_form_open = None;
            return;
        };
        let info = FormInfo::of(&self.session.model);
        let (mut run, mut cancel) = (false, false);
        let r = Modal::new(UiId::new("plugin-form"), form.label.clone()).width(420.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(RichText::new(t("Tick the fields and run the plugin's command")).color(pal().muted));
                ui.add_space(4.0);
                for field in &mut form.fields {
                    field_ui(ui, field, &info);
                    ui.add_space(2.0);
                }
                if let Some(error) = &form.error {
                    ui.add_space(4.0);
                    ui.add(Label::new(RichText::new(error).color(pal().error)).selectable(true));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button(t("Cancel")).clicked();
                    run = ui.add_enabled(!form.fields.is_empty(), egui::Button::new(t("Run"))).clicked();
                });
            });
        });
        self.plugin_forms.insert(id.clone(), form.clone());
        if cancel || r.should_close() {
            self.plugin_form_open = None;
            return;
        }
        if !run {
            return;
        }
        match form.resolve(&self.session.model) {
            Ok(params) => {
                self.plugin_form_open = None;
                self.finish_plugin_function(&id, form.label.clone(), params);
            }
            Err(why) => {
                form.error = Some(why);
                self.plugin_forms.insert(id, form);
            }
        }
    }
}

fn field_ui(ui: &mut Ui, field: &mut PluginField, info: &FormInfo) {
    match &field.kind {
        Kind::Bool => {
            let r = ui.checkbox(&mut field.on, &field.title);
            if !field.description.is_empty() {
                r.on_hover_text(&field.description);
            }
        }
        Kind::Selection => {
            caption(ui, field);
            let text = if let Ok(n) = field.text.parse::<usize>() {
                tf("{} selected", &[&n.to_string()])
            } else {
                t("Nothing selected").to_string()
            };
            ui.label(RichText::new(text).color(pal().muted));
        }
        Kind::Unsupported(schema) => {
            caption(ui, field);
            ui.add(Label::new(RichText::new(tf("Unsupported field: {}", &[schema])).color(pal().muted)));
        }
        Kind::Choice(options) => {
            let named = caption(ui, field);
            let shown = field
                .choice
                .as_ref()
                .and_then(|value| options.iter().find(|c| &c.value == value))
                .map_or_else(|| field.text.clone(), |c| c.label.clone());
            ComboBox::from_id_salt(("plugin-form-choice", &field.name))
                .selected_text(shown)
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for choice in options {
                        if ui.selectable_label(field.choice.as_ref() == Some(&choice.value), &choice.label).clicked() {
                            field.choice = Some(choice.value.clone());
                            field.text = choice.label.clone();
                        }
                    }
                })
                .response
                .labelled_by(named);
        }
        Kind::Level | Kind::Library(_) => {
            let named = caption(ui, field);
            let names: Vec<String> = match &field.kind {
                Kind::Level => info.model_levels.clone(),
                Kind::Library(kind) => info.of_kind(kind).into_iter().map(str::to_string).collect(),
                _ => Vec::new(),
            };
            ComboBox::from_id_salt(("plugin-form-list", &field.name))
                .selected_text(field.text.clone())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for name in &names {
                        ui.selectable_value(&mut field.text, name.clone(), name).labelled_by(named);
                    }
                })
                .response
                .labelled_by(named);
        }
        Kind::Integer | Kind::Number | Kind::Length | Kind::Text | Kind::Path => {
            let named = caption(ui, field);
            let hint = match field.kind {
                Kind::Length => info.units.length(0.0),
                _ => String::new(),
            };
            let edit = TextEdit::singleline(&mut field.text).hint_text(hint).desired_width(ui.available_width());
            let r = ui.add(edit).labelled_by(named);
            if !field.description.is_empty() {
                r.on_hover_text(&field.description);
            }
        }
    }
}

fn caption(ui: &mut Ui, field: &PluginField) -> egui::Id {
    let label = if field.required { format!("{} *", field.title) } else { field.title.clone() };
    let r = ui.label(RichText::new(label).small().color(pal().muted));
    let id = r.id;
    if !field.description.is_empty() {
        r.on_hover_text(&field.description);
    }
    id
}
