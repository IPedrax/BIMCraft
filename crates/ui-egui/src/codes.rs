//! Manage > Building Codes: the code the project's stairs and railings are held to (apart from its technical standard,
//! which is about drafting), and the rules themselves, the code's until the project changes one. A rule changed is
//! marked, with the code's own number beside it; nothing is ever refused for breaking one.

use bimcraft_core::codes::{self, ORDER};
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

fn rules_of(code: &str) -> Map<String, Value> {
    let rules = &codes::code(code).unwrap_or_else(codes::generic).stairs;
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
}

impl CodesDialog {
    pub(crate) fn new(m: &Model) -> CodesDialog {
        let rules = match serde_json::to_value(m.stair_rules()) {
            Ok(Value::Object(o)) => o,
            _ => Map::new(),
        };
        CodesDialog { selected: m.building_code().code.clone(), rules, tab: 1 }
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

/// A stair's findings, as its Properties' popup lists them: the code, then each rule broken with what the stair
/// has, what the rule asks, and what would bring it inside.
pub(crate) fn findings(ui: &mut Ui, m: &Model, list: &Value) {
    ui.set_max_width(360.0);
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

fn stairs_tab(ui: &mut Ui, d: &mut CodesDialog) {
    let own = rules_of(&d.selected);
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
    /// The Building Codes modal dialog.
    pub(crate) fn codes_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.codes_dialog.take() else { return };
        let (mut ok, mut apply, mut cancel) = (false, false, false);

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
                    let now = codes::code(&d.selected).map_or_else(String::new, name);
                    let c = ComboBox::from_id_salt("building-code-selector").selected_text(now).width(320.0).show_ui(ui, |ui| {
                        for c in codes::codes() {
                            ui.selectable_value(&mut d.selected, c.code.clone(), name(c));
                        }
                    });
                    c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, t("Building Code")));
                    // Another code brings its own rules.
                    if d.selected != was {
                        d.rules = rules_of(&d.selected);
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    for (i, label) in TABS.iter().enumerate() {
                        if ui.selectable_label(d.tab == i, t(label)).clicked() {
                            d.tab = i;
                        }
                    }
                });
                ui.separator();
                ScrollArea::vertical().max_height(380.0).show(ui, |ui| match d.tab {
                    0 => overview_tab(ui, codes::code(&d.selected).unwrap_or_else(codes::generic)),
                    _ => stairs_tab(ui, &mut d),
                });
            });
            popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(t("Reset to the Code's Rules")).clicked() {
                        d.rules = rules_of(&d.selected);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        apply = ui.button(t("Apply")).clicked();
                        cancel = ui.button(t("Cancel")).clicked();
                        ok = ui.button(t("OK")).clicked();
                    });
                });
            });
        });

        // What can't be rules (a range upside down) is refused with what to fix, and the dialog stays.
        let done = if ok || apply { self.run("building_code", json!({"set": d.selected, "stairs": d.rules})).is_some() } else { true };
        if !((ok && done) || cancel || r.should_close()) {
            self.codes_dialog = Some(d);
        }
    }
}
