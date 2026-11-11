//! Manage > Building Codes: the code the project's stairs and railings are held to (apart from its technical standard,
//! which is about drafting), and the rules themselves, the code's until the project changes one. A rule changed is
//! marked, with the code's own number beside it; nothing is ever refused for breaking one.

use bimcraft_core::codes::{self, GENERIC, ORDER};
use bimcraft_core::model::Model;
use egui::{Align, Color32, ComboBox, Context, DragValue, Grid, Id as UiId, Layout, RichText, ScrollArea, Ui, WidgetInfo, WidgetType};
use serde_json::{Map, Value, json};

use crate::BimApp;
use crate::lang::{t, tf};
use crate::popup::{self, Modal};
use crate::style::pal;

const TABS: [&str; 2] = ["Overview", "Stairs & Railings"];

/// The colour of what is outside a rule, or changed from the code's.
pub(crate) const OUTSIDE: Color32 = Color32::from_rgb(0xC8, 0x7A, 0x00);

/// What each rule is called, and the group it is listed under.
const LABELS: [(&str, &str, &str); 25] = [
    ("riser", "New stairs", "Riser Height"),
    ("going", "New stairs", "Tread Depth (going)"),
    ("riser_min", "Risers and treads", "Minimum Riser Height"),
    ("riser_max", "Risers and treads", "Maximum Riser Height"),
    ("going_min", "Risers and treads", "Minimum Tread Depth"),
    ("going_max", "Risers and treads", "Maximum Tread Depth"),
    ("step_min", "Risers and treads", "Minimum Step (2 risers + 1 tread)"),
    ("step_max", "Risers and treads", "Maximum Step (2 risers + 1 tread)"),
    ("width_min", "Risers and treads", "Minimum Width"),
    ("headroom", "Risers and treads", "Minimum Headroom"),
    ("flight_rise_max", "Landings", "Landing Due After a Rise of"),
    ("flight_risers_max", "Landings", "Landing Due After Risers"),
    ("landing_rule", "Landings", "Landing Length By"),
    ("landing_length", "Landings", "Landing Length"),
    ("winders", "Winders", "Winders Allowed"),
    ("walk_line", "Winders", "Walk Line Measured"),
    ("walk_line_offset", "Winders", "Walk Line Offset From Narrow Side"),
    ("winder_going_min", "Winders", "Minimum Tread Depth at Walk Line"),
    ("winder_narrow_min", "Winders", "Minimum Tread Depth at Narrow End"),
    ("nosing_max", "Treads", "Maximum Nosing"),
    ("open_risers", "Treads", "Open Risers Allowed"),
    ("uniformity", "Treads", "Most Difference Between Steps"),
    ("handrail_min", "Railings", "Minimum Handrail Height"),
    ("handrail_max", "Railings", "Maximum Handrail Height"),
    ("guard_min", "Railings", "Minimum Guard Height"),
];

/// What a rule that is a choice may be, as the core names each and as it is shown.
fn choices(key: &str) -> &'static [(&'static str, &'static str)] {
    match key {
        "landing_rule" => &[("width", "The stair's width"), ("fixed", "A fixed length"), ("step", "A tread and one step")],
        "walk_line" => &[("centre", "Along the middle"), ("narrow_side", "From the narrow side")],
        _ => &[],
    }
}

fn rules_of(m: &Model, code: &str) -> Map<String, Value> {
    let rules = &m.code(code).unwrap_or(codes::generic()).stairs;
    match serde_json::to_value(rules) {
        Ok(Value::Object(o)) => o,
        _ => Map::new(),
    }
}

/// The Building Codes dialog: the code picked and the rules as they would be applied.
pub(crate) struct CodesDialog {
    pub(crate) selected: String,
    pub(crate) rules: Map<String, Value>,
    pub(crate) tab: usize,
    /// The name the rules would be saved under, as a code of the project's own.
    pub(crate) name: String,
    /// What went wrong, said inside the dialog.
    pub(crate) error: String,
}

impl CodesDialog {
    pub(crate) fn new(m: &Model) -> CodesDialog {
        let rules = match serde_json::to_value(m.stair_rules()) {
            Ok(Value::Object(o)) => o,
            _ => Map::new(),
        };
        CodesDialog { selected: m.building_code().code.clone(), rules, tab: 1, name: String::new(), error: String::new() }
    }
}

/// How a rule's value reads.
fn said(key: &str, v: &Value) -> String {
    match v {
        Value::Bool(b) => t(if *b { "Yes" } else { "No" }).to_string(),
        Value::String(s) => choices(key).iter().find(|(k, _)| k == s).map_or_else(|| s.clone(), |(_, label)| t(label).to_string()),
        Value::Number(n) if key == "flight_risers_max" => n.to_string(),
        Value::Number(n) => format!("{} mm", n.as_f64().unwrap_or(0.0)),
        _ => String::new(),
    }
}

/// A stair's findings, as the window its Properties' sign opens lists them: the code, then each rule broken with what
/// the stair has, what the rule asks, and what would bring it inside.
pub(crate) fn findings(ui: &mut Ui, m: &Model, list: &Value) {
    ui.label(RichText::new(t(&m.building_code().name)).strong());
    ui.label(RichText::new(t("The stair is built as drawn. These rules it is outside of:")).color(pal().muted));
    for f in list.as_array().into_iter().flatten() {
        ui.separator();
        let rule = f["rule"].as_str().unwrap_or("");
        let label = LABELS.iter().find(|(k, _, _)| *k == rule).map_or(rule, |(_, _, l)| l);
        ui.label(RichText::new(t(label)).color(OUTSIDE).strong());
        // What the stair has and what the rule asks; a yes or no rule has no numbers.
        if rule != "winders" {
            let unit = if rule == "flight_risers_max" { "" } else { " mm" };
            let (has, asks) = (f["value"].as_f64().unwrap_or(0.0), f["limit"].as_f64().unwrap_or(0.0));
            Grid::new(("finding", rule)).num_columns(2).show(ui, |ui| {
                ui.label(RichText::new(t("The stair:")).color(pal().muted));
                ui.label(format!("{has}{unit}"));
                ui.end_row();
                ui.label(RichText::new(t("The rule:")).color(pal().muted));
                ui.label(format!("{asks}{unit}"));
                ui.end_row();
            });
        }
        // What the core says of it, its fix after the colon, on a line of its own.
        let said = f["message"].as_str().unwrap_or("");
        let fix = said.split_once(": ").map_or(said, |(_, fix)| fix);
        ui.add(egui::Label::new(RichText::new(fix).small()).wrap());
    }
    ui.separator();
    ui.label(RichText::new(t("Manage > Building Codes changes a rule for this project.")).color(pal().muted).small());
}

fn overview_tab(ui: &mut Ui, code: &codes::BuildingCode) {
    ui.label(RichText::new(t(&code.name)).strong());
    if !code.region.is_empty() {
        ui.label(RichText::new(&code.region).color(pal().muted));
    }
    ui.add_space(6.0);
    ui.label(t(&code.note));
    ui.add_space(8.0);
    if !code.references.is_empty() {
        ui.label(RichText::new(t("Documents this code's numbers are from:")).strong());
        for r in &code.references {
            ui.horizontal(|ui| {
                ui.label("•");
                ui.label(r);
            });
        }
        ui.add_space(8.0);
    }
    ui.label(
        RichText::new(t(
            "The numbers were typed from memory of these documents and are unchecked: check one against a copy before relying on it.",
        ))
        .color(OUTSIDE),
    );
}

/// Whether the rules in the dialog are no longer its code's: a code of the user's own making, until saved.
fn custom(m: &Model, d: &CodesDialog) -> bool {
    let own = rules_of(m, &d.selected);
    ORDER.iter().any(|k| match (d.rules.get(*k), own.get(*k)) {
        (Some(a), Some(b)) => a != b && (a.as_f64().is_none() || a.as_f64() != b.as_f64()),
        (a, b) => a != b,
    })
}

fn stairs_tab(ui: &mut Ui, m: &Model, d: &mut CodesDialog) {
    let own = rules_of(m, &d.selected);
    let mut group = "";
    Grid::new("building-code-stairs").striped(true).num_columns(3).min_col_width(150.0).show(ui, |ui| {
        for (key, of, label) in LABELS {
            debug_assert!(ORDER.contains(&key));
            if of != group {
                group = of;
                ui.label(RichText::new(t(of)).strong());
                ui.end_row();
            }
            ui.label(t(label));
            let Some(v) = d.rules.get_mut(key) else {
                ui.end_row();
                continue;
            };
            match v {
                Value::Bool(b) => {
                    let r = ui.checkbox(b, "");
                    r.widget_info(|| WidgetInfo::labeled(WidgetType::Checkbox, true, t(label)));
                }
                Value::String(s) => {
                    let now = choices(key).iter().find(|(k, _)| k == s).map_or(s.as_str(), |(_, l)| l);
                    let c = ComboBox::from_id_salt(("building-code-rule", key)).selected_text(t(now)).width(170.0).show_ui(ui, |ui| {
                        for (k, l) in choices(key) {
                            ui.selectable_value(s, (*k).to_string(), t(l));
                        }
                    });
                    c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, t(label)));
                }
                Value::Number(n) if key == "flight_risers_max" => {
                    let mut x = n.as_u64().unwrap_or(1);
                    let r = ui.add(DragValue::new(&mut x).range(1..=1000));
                    r.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, t(label)));
                    *v = json!(x);
                }
                Value::Number(n) => {
                    let mut x = n.as_f64().unwrap_or(0.0);
                    let r = ui.add(DragValue::new(&mut x).range(0.0..=100_000.0).speed(1.0).max_decimals(2).suffix(" mm"));
                    r.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, t(label)));
                    *v = json!(x);
                }
                _ => {}
            }
            // A rule the project changed: the code's own beside it, and the way back.
            match (d.rules.get(key), own.get(key)) {
                (Some(now), Some(code)) if now != code && now.as_f64() != code.as_f64().or(Some(f64::NAN)) => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tf("Changed: the code says {}", &[&said(key, code)])).color(OUTSIDE));
                        if ui.small_button(t("Reset")).clicked() {
                            d.rules.insert(key.to_string(), code.clone());
                        }
                    });
                }
                _ => {
                    ui.label("");
                }
            }
            ui.end_row();
        }
    });
}

impl BimApp {
    /// The window a stair's sign in Properties opens: the rules it is outside of. It closes when the stair is inside them
    /// all, or gone.
    pub(crate) fn findings_window(&mut self, ctx: &Context) {
        let Some(id) = self.code_findings else { return };
        let m = self.session.model.clone();
        let list = match m.elements.get(id) {
            Some(bimcraft_core::model::Element::Stair(st)) => serde_json::to_value(codes::stair_findings(&m, st)).unwrap_or(Value::Null),
            _ => Value::Null,
        };
        if list.as_array().is_none_or(Vec::is_empty) {
            self.code_findings = None;
            return;
        }
        let mut close = false;
        let r = Modal::new(UiId::new("stair-code-findings"), t("Building Code Check")).width(420.0).show(ctx, |ui| {
            popup::body(ui, |ui| {
                ScrollArea::vertical().max_height(360.0).show(ui, |ui| findings(ui, &m, &list));
            });
            popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(t("Building Codes…")).on_hover_text(t("Open the project's building code and its rules")).clicked() {
                        self.codes_dialog = Some(CodesDialog::new(&m));
                        close = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        close |= ui.button(t("Close")).clicked();
                    });
                });
            });
        });
        if close || r.should_close() {
            self.code_findings = None;
        }
    }

    /// A building code read from the XML file at `path`, kept as one of the project's own and shown in the dialog.
    pub(crate) fn import_code(&mut self, path: &std::path::Path) {
        let reply =
            self.run("building_code_import", json!({"path": path.to_string_lossy()})).ok_or_else(|| std::mem::take(&mut self.status.text));
        let m = &self.session.model;
        let mut d = self.codes_dialog.take().unwrap_or_else(|| CodesDialog::new(m));
        match reply {
            Ok(v) => {
                d.selected = v["code"].as_str().unwrap_or(GENERIC).to_string();
                d.rules = rules_of(m, &d.selected);
                let left: Vec<&str> = v["left_out"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                d.error = if left.is_empty() {
                    String::new()
                } else {
                    tf("Left out of the file, unknown to this version: {}", &[&left.join(", ")])
                };
            }
            Err(e) => d.error = e,
        }
        self.codes_dialog = Some(d);
    }

    /// The rules the dialog shows, written as an XML file at `path`: worked out on a copy of the project, so that what is
    /// not applied yet is exported as shown and the project stays as it is.
    pub(crate) fn export_code(&mut self, path: &std::path::Path, code: &str, rules: &Map<String, Value>, name: &str) {
        let mut scratch = bimcraft_core::engine::Session::new();
        *scratch.model_mut() = (*self.session.model).clone();
        let mut p = json!({"path": path.to_string_lossy()});
        if !name.is_empty() {
            p["name"] = json!(name);
        }
        let reply = scratch
            .execute("building_code", &json!({"set": code, "stairs": rules}))
            .and_then(|_| scratch.execute("building_code_export", &p));
        if let Some(d) = self.codes_dialog.as_mut() {
            d.error = reply.err().unwrap_or_default();
        }
    }

    /// The Building Codes modal dialog.
    pub(crate) fn codes_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.codes_dialog.take() else { return };
        let (mut ok, mut apply, mut cancel) = (false, false, false);
        let (mut save, mut delete, mut import, mut export) = (false, false, false, false);
        let m = self.session.model.clone();
        let m = &m;

        let r = Modal::new(UiId::new("building-codes-dialog"), t("Building Codes")).width(760.0).show(ctx, |ui| {
            popup::body(ui, |ui| {
                ui.add_space(4.0);
                ui.label(t(
                    "The building code this project's stairs and railings are held to. A new stair starts with its numbers. Any rule can be changed, for a state's or a city's own; a stair outside the rules is still built, and marked.",
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t("Building Code:")).strong());
                    let was = d.selected.clone();
                    let name = |c: &codes::BuildingCode| if c.region.is_empty() { t(&c.name).to_string() } else { format!("{} ({})", t(&c.name), c.region) };
                    let changed = custom(m, &d);
                    let base = m.code(&d.selected).map_or_else(String::new, |c| t(&c.name).to_string());
                    // Rules changed are no longer the code's: Custom, until saved under a name.
                    let now = if changed { tf("Custom (based on {})", &[&base]) } else { m.code(&d.selected).map_or_else(String::new, name) };
                    let c = ComboBox::from_id_salt("building-code-selector").selected_text(now).width(320.0).show_ui(ui, |ui| {
                        for c in codes::codes().iter().chain(&m.building_codes) {
                            if ui.selectable_label(!changed && d.selected == c.code, name(c)).clicked() {
                                d.selected = c.code.clone();
                                d.rules = rules_of(m, &d.selected);
                            }
                        }
                    });
                    c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, t("Building Code")));
                    if d.selected != was {
                        d.error.clear();
                    }
                    if changed {
                        ui.label(RichText::new(t("Custom")).color(OUTSIDE).strong());
                    }
                });
                ui.horizontal(|ui| {
                    import = ui.button(t("Import…")).on_hover_text(t("Read a building code from an XML file, as one of the project's own")).clicked();
                    export = ui.button(t("Export…")).on_hover_text(t("Write these rules as an XML file, to hand to another project or office")).clicked();
                    if m.building_codes.iter().any(|c| c.code == d.selected) && !custom(m, &d) {
                        delete = ui.button(t("Delete")).on_hover_text(t("Take this code of the project's own away; a project held to it keeps its rules")).clicked();
                    }
                });
                // A custom code is saved under a name of the user's own, and is then a code like the others.
                if custom(m, &d) {
                    ui.horizontal(|ui| {
                        let l = ui.label(t("Save as:"));
                        ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(240.0).hint_text(t("A name of your own"))).labelled_by(l.id);
                        save = ui.add_enabled(!d.name.trim().is_empty(), egui::Button::new(t("Save Code"))).clicked();
                        ui.label(RichText::new(t("Unsaved, the rules are still the project's when applied.")).color(pal().muted).small());
                    });
                }
                if !d.error.is_empty() {
                    ui.label(RichText::new(&d.error).color(pal().error));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    for (i, label) in TABS.iter().enumerate() {
                        if ui.selectable_label(d.tab == i, t(label)).clicked() {
                            d.tab = i;
                        }
                    }
                });
                ui.separator();
                ScrollArea::vertical().max_height(320.0).show(ui, |ui| match d.tab {
                    0 => overview_tab(ui, m.code(&d.selected).unwrap_or(codes::generic())),
                    _ => stairs_tab(ui, m, &mut d),
                });
            });
            popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(t("Reset to the Code's Rules")).clicked() {
                        d.rules = rules_of(m, &d.selected);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        apply = ui.button(t("Apply")).clicked();
                        cancel = ui.button(t("Cancel")).clicked();
                        ok = ui.button(t("OK")).clicked();
                    });
                });
            });
        });

        // What can't be rules (a range upside down) is refused with what to fix, said in the dialog, which stays.
        let tried = |app: &mut BimApp, d: &mut CodesDialog, command: &str, p: Value| {
            let reply = app.run(command, p);
            d.error = if reply.is_none() { std::mem::take(&mut app.status.text) } else { String::new() };
            reply
        };
        let mut done = true;
        if save {
            let p = json!({"set": d.selected, "stairs": d.rules, "save": d.name.trim()});
            if let Some(v) = tried(self, &mut d, "building_code", p) {
                d.selected = v["code"].as_str().unwrap_or(GENERIC).to_string();
                d.rules = rules_of(&self.session.model, &d.selected);
                d.name.clear();
            }
        } else if delete {
            let p = json!({"delete": d.selected});
            if let Some(v) = tried(self, &mut d, "building_code", p) {
                d.selected = v["code"].as_str().unwrap_or(GENERIC).to_string();
                d.rules = v["stairs"].as_object().cloned().unwrap_or_default();
            }
        } else if ok || apply {
            let p = json!({"set": d.selected, "stairs": d.rules});
            done = tried(self, &mut d, "building_code", p).is_some();
        }
        if import {
            let dialog = rfd::AsyncFileDialog::new().add_filter(t("Building codes"), &["xml", "XML"]);
            self.ask_file(ctx, dialog, false, |app, path| app.import_code(&path));
        }
        if export {
            // What the dialog shows, under the name typed, else its code's: from a copy of the project, which stays as it is.
            let (code, rules, name) = (d.selected.clone(), d.rules.clone(), d.name.trim().to_string());
            let file = format!("{}.xml", if name.is_empty() { code.as_str() } else { name.as_str() }.replace(['/', '\\'], "-"));
            let dialog = rfd::AsyncFileDialog::new().add_filter(t("Building codes"), &["xml"]).set_file_name(file);
            self.ask_file(ctx, dialog, true, move |app, path| app.export_code(&path, &code, &rules, &name));
        }
        if !((ok && done) || cancel || r.should_close()) {
            self.codes_dialog = Some(d);
        }
    }
}
