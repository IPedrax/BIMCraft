//! Manage > Project Information, as Revit's: the project's name, its fields (number, client, address, status, issue date,
//! author, organization, building) and its values of the project parameters for the project. OK runs `project_info` with
//! what changed; a refusal is said in the dialog, which stays open.

use bimcraft_core::model::{Model, ParameterKind, Scope};
use bimcraft_core::project_info::{CATEGORY, FIELDS};
use egui::{Align, Context, Grid, Id as UiId, Layout, RichText, TextEdit, WidgetInfo, WidgetType};
use serde_json::{Map, Value, json};

use crate::lang::t;
use crate::popup::Modal;
use crate::properties::{parameter_parsed, parameter_shown};
use crate::style::pal;
use crate::{BimApp, Status};

/// A project parameter as the dialog has it: its name, kind, and what's typed (or ticked).
struct Param {
    name: String,
    kind: ParameterKind,
    description: String,
    read_only: bool,
    typed: String,
    on: bool,
}

/// The dialog's fields as typed, and what they were when it opened, so OK sends only what changed.
pub(crate) struct ProjectInfoDialog {
    name: String,
    fields: Vec<(&'static str, &'static str, String)>,
    params: Vec<Param>,
    was: Value,
    error: Option<String>,
}

impl ProjectInfoDialog {
    pub(crate) fn new(m: &Model) -> Self {
        let info = m.project_info().map(|(_, i)| i.clone()).unwrap_or_default();
        let fields = FIELDS.iter().map(|(key, label)| (*key, *label, info.get(key).unwrap_or("").to_string())).collect();
        let params = m
            .parameters
            .iter()
            .filter(|(_, p)| p.scope == Scope::Instance && p.categories.iter().any(|c| c == CATEGORY))
            .map(|(n, p)| {
                let v = info.params.get(n).cloned().unwrap_or(Value::Null);
                Param {
                    name: n.clone(),
                    kind: p.kind,
                    description: p.description.clone(),
                    read_only: p.read_only,
                    typed: parameter_shown(p.kind, &v, m.units),
                    on: v.as_bool().unwrap_or(false),
                }
            })
            .collect();
        let mut d = ProjectInfoDialog { name: m.name.clone(), fields, params, was: Value::Null, error: None };
        d.was = d.request(m).unwrap_or(Value::Null);
        d
    }

    /// Everything the dialog holds, as `project_info` takes it.
    fn request(&self, m: &Model) -> Result<Value, String> {
        let mut p = Map::new();
        p.insert("name".into(), json!(self.name.trim()));
        for (key, _, v) in &self.fields {
            p.insert((*key).into(), json!(v.trim()));
        }
        let mut params = Map::new();
        for q in self.params.iter().filter(|q| !q.read_only) {
            let v = match q.kind {
                ParameterKind::YesNo => json!(q.on),
                kind => parameter_parsed(kind, &q.typed, m.units).map_err(|e| format!("{}: {e}", q.name))?,
            };
            params.insert(q.name.clone(), v);
        }
        p.insert("params".into(), Value::Object(params));
        Ok(Value::Object(p))
    }
}

/// What of `now` differs from `was`, each key's, and in `params` each parameter's.
fn changes(was: &Value, now: &Value) -> Value {
    let mut out = Map::new();
    for (k, v) in now.as_object().into_iter().flatten() {
        if k == "params" {
            let before = was.get("params");
            let params: Map<String, Value> = v
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(n, v)| before.and_then(|b| b.get(n.as_str())) != Some(*v))
                .map(|(n, v)| (n.clone(), v.clone()))
                .collect();
            if !params.is_empty() {
                out.insert(k.clone(), Value::Object(params));
            }
        } else if was.get(k) != Some(v) {
            out.insert(k.clone(), v.clone());
        }
    }
    Value::Object(out)
}

impl BimApp {
    pub(crate) fn open_project_info(&mut self) {
        self.project_info_dialog = Some(ProjectInfoDialog::new(&self.session.model));
    }

    pub(crate) fn project_info_window(&mut self, ctx: &Context) {
        let Some(d) = self.project_info_dialog.as_mut() else { return };
        let (mut ok, mut close) = (false, false);
        let r = Modal::new(UiId::new("project-information"), t("Project Information")).width(520.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(t("Identity Data")).strong());
                Grid::new("project-info-fields").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    let l = ui.label(t("Project Name"));
                    ui.add(TextEdit::singleline(&mut d.name).desired_width(320.0)).labelled_by(l.id);
                    ui.end_row();
                    for (key, label, v) in &mut d.fields {
                        let l = ui.label(t(label));
                        let edit = if *key == "address" { TextEdit::multiline(v).desired_rows(3) } else { TextEdit::singleline(v) };
                        ui.add(edit.desired_width(320.0)).labelled_by(l.id);
                        ui.end_row();
                    }
                });
                if !d.params.is_empty() {
                    ui.add_space(10.0);
                    ui.label(RichText::new(t("Project Parameters")).strong());
                    Grid::new("project-info-params").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        for q in &mut d.params {
                            let mut l = ui.label(q.name.as_str());
                            if !q.description.is_empty() {
                                l = l.on_hover_text(&q.description);
                            }
                            ui.add_enabled_ui(!q.read_only, |ui| match q.kind {
                                ParameterKind::YesNo => {
                                    let r = ui.checkbox(&mut q.on, "");
                                    let (on, name) = (q.on, q.name.clone());
                                    r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, name.as_str()));
                                    r.labelled_by(l.id);
                                }
                                _ => {
                                    ui.add(TextEdit::singleline(&mut q.typed).desired_width(320.0)).labelled_by(l.id);
                                }
                            });
                            ui.end_row();
                        }
                    });
                }
                if let Some(e) = &d.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
                });
            });
        });
        if ok {
            self.apply_project_info();
        } else if close || r.should_close() {
            self.project_info_dialog = None;
        }
    }

    /// Runs `project_info` with what the dialog changed, and closes it; a refusal stays in the dialog.
    fn apply_project_info(&mut self) {
        let Some(d) = self.project_info_dialog.as_ref() else { return };
        let p = match d.request(&self.session.model) {
            Ok(now) => changes(&d.was, &now),
            Err(e) => {
                if let Some(d) = self.project_info_dialog.as_mut() {
                    d.error = Some(e);
                }
                return;
            }
        };
        if p.as_object().is_none_or(Map::is_empty) || self.run("project_info", p).is_some() {
            self.project_info_dialog = None;
            return;
        }
        let e = std::mem::take(&mut self.status).text;
        if let Some(d) = self.project_info_dialog.as_mut() {
            d.error = Some(e);
        }
        self.status = Status::default();
    }
}
