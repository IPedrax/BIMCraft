//! Manage > Building Codes: the code the project's stairs and railings are held to (apart from its technical standard,
//! which is about drafting), and the rules themselves, the code's until the project changes one. A rule changed is
//! marked, with the code's own number beside it; nothing is ever refused for breaking one.

use bimcraft_core::codes::{self, GENERIC, ORDER};
use bimcraft_core::model::Model;
use bimcraft_core::units::Units;
use bimcraft_i18n::msg;
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
const LABELS: [(&str, &str, &str); 26] = [
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
    ("nosing_min", "Treads", "Minimum Nosing"),
    ("nosing_max", "Treads", "Maximum Nosing"),
    ("open_risers", "Treads", "Open Risers Allowed"),
    ("uniformity", "Treads", "Most Difference Between Steps"),
    ("handrail_min", "Railings", "Minimum Handrail Height"),
    ("handrail_max", "Railings", "Maximum Handrail Height"),
    ("guard_min", "Railings", "Minimum Guard Height"),
];

/// What the rules only a stair type has are called.
const TYPE_LABELS: [(&str, &str); 6] = [
    ("ratio_min", "Minimum Tread / Riser Ratio"),
    ("ratio_max", "Maximum Tread / Riser Ratio"),
    ("sum_min", "Minimum Tread + Riser"),
    ("sum_max", "Maximum Tread + Riser"),
    ("pitch_min", "Minimum Stair Pitch"),
    ("pitch_max", "Maximum Stair Pitch"),
];

/// What to do about a stair outside rule `rule`, as a person would say it.
fn advice(rule: &str) -> Option<&'static str> {
    Some(match rule {
        "riser_max" => msg!("The risers are too high. Add more risers so each one is lower, or lower the Riser in the Options Bar."),
        "riser_min" => msg!("The risers are too low. Use fewer, higher risers."),
        "going_min" => msg!("The treads are too shallow. Make them deeper."),
        "going_max" => msg!("The treads are too deep. Make them shallower."),
        "step_min" => msg!("The steps are too short for a comfortable stride. Deepen the treads, or use fewer risers."),
        "step_max" => msg!("The steps are too long for a comfortable stride. Make the treads shallower, or use more risers."),
        "width_min" => msg!("The stair is too narrow. Make it wider."),
        "headroom" => {
            msg!("Something overhead is too low. Cut it open over the stair with Cut Opening below, or raise what is in the way.")
        }
        "flight_rise_max" | "flight_risers_max" => {
            msg!("This flight climbs too far without a rest. Add a landing to break it up; Add Landings Due in Properties does it for you.")
        }
        "landing_length" => msg!("A landing is too short. Make it longer."),
        "winders" => msg!("This code doesn't allow winders. Use a landing at the turn instead."),
        "winder_going_min" => msg!("The winders are too shallow on the walk line. Use fewer of them, or spread them into the flights."),
        "winder_narrow_min" => {
            msg!("The winders come too close to a point at the inside. Spread them into the flights so each keeps some depth.")
        }
        "nosing_min" => msg!("The treads don't stand out enough over the risers. Give the stair type a longer nosing or finish nosing."),
        "nosing_max" => msg!("The treads stand out too far over the risers. Give the stair type a shorter nosing."),
        "open_risers" => msg!("This code doesn't allow open risers. Use a stair type with riser boards."),
        "uniformity" => msg!("The risers aren't all the same height. Pick a riser height that divides each storey more evenly."),
        "ratio_min" | "ratio_max" | "sum_min" | "sum_max" | "pitch_min" | "pitch_max" => {
            msg!("The stair's proportions are outside what its type allows. Change the riser or the tread, or the type's rules.")
        }
        _ => return None,
    })
}

/// What a rule is called, in the interface's language.
pub(crate) fn rule_label(rule: &str) -> &str {
    let of_type = TYPE_LABELS.iter().find(|(k, _)| *k == rule).map(|(_, l)| t(l));
    LABELS.iter().find(|(k, _, _)| *k == rule).map(|(_, _, l)| t(l)).or(of_type).unwrap_or(rule)
}

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

/// A length as the project writes its lengths; in millimetres, to the hundredth, since a code's inch is 25.4.
fn length(units: Units, mm: f64) -> String {
    if units == Units::Millimetres { format!("{} mm", (mm * 100.0).round() / 100.0) } else { units.with_unit(mm) }
}

/// How a rule's value reads.
fn said(units: Units, key: &str, v: &Value) -> String {
    match v {
        Value::Bool(b) => t(if *b { "Yes" } else { "No" }).to_string(),
        Value::String(s) => choices(key).iter().find(|(k, _)| k == s).map_or_else(|| s.clone(), |(_, label)| t(label).to_string()),
        Value::Number(n) if key == "flight_risers_max" => n.to_string(),
        Value::Number(n) => length(units, n.as_f64().unwrap_or(0.0)),
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
            let (has, asks) = (f["value"].as_f64().unwrap_or(0.0), f["limit"].as_f64().unwrap_or(0.0));
            let shown = |x: f64| if rule == "flight_risers_max" { x.to_string() } else { length(m.units, x) };
            Grid::new(("finding", rule)).num_columns(2).show(ui, |ui| {
                ui.label(RichText::new(t("The stair:")).color(pal().muted));
                ui.label(shown(has));
                ui.end_row();
                ui.label(RichText::new(t("The rule:")).color(pal().muted));
                ui.label(shown(asks));
                ui.end_row();
            });
        }
        // What to do about it, in plain words; the core's own sentence where there are none here.
        let fix = match advice(rule) {
            Some(a) => t(a).to_string(),
            None => {
                let said = f["message"].as_str().unwrap_or("");
                said.split_once(": ").map_or(said, |(_, fix)| fix).replace('`', "")
            }
        };
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
    if code.checked.is_empty() {
        ui.label(
            RichText::new(t(
                "These numbers haven't been checked against the documents yet. Check one against a copy before you rely on it.",
            ))
            .color(OUTSIDE),
        );
    } else {
        ui.label(RichText::new(&code.checked).color(pal().muted));
    }
    ui.label(RichText::new(t("Each rule shows where its number comes from on the Stairs tab.")).color(pal().muted).small());
}

/// Whether the rules in the dialog are no longer its code's: a code of the user's own making, until saved.
fn custom(m: &Model, d: &CodesDialog) -> bool {
    let own = rules_of(m, &d.selected);
    ORDER.iter().any(|k| match (d.rules.get(*k), own.get(*k)) {
        (Some(a), Some(b)) => a != b && (a.as_f64().is_none() || a.as_f64() != b.as_f64()),
        (a, b) => a != b,
    })
}

/// The rules shown as one row with a least and a most side by side, as Revit's Maximum Riser Height gets a second box:
/// the least's key, the most's, and the row's name.
const PAIRS: [(&str, &str, &str); 5] = [
    ("riser_min", "riser_max", "Riser Height"),
    ("going_min", "going_max", "Tread Depth"),
    ("step_min", "step_max", "Step (2 risers + 1 tread)"),
    ("nosing_min", "nosing_max", "Nosing"),
    ("handrail_min", "handrail_max", "Handrail Height"),
];

/// The field of one rule's value: a tick, a choice, a count or a length.
fn rule_field(ui: &mut Ui, units: Units, key: &str, label: &str, v: &mut Value) {
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
            // In the project's units, typed as its lengths are.
            let r = ui.add(
                DragValue::new(&mut x)
                    .range(0.0..=100_000.0)
                    .speed(1.0)
                    .custom_formatter(|v, _| length(units, v))
                    .custom_parser(|typed| units.parse(typed.trim().trim_end_matches("mm").trim())),
            );
            r.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, t(label)));
            *v = json!(x);
        }
        _ => {}
    }
}

/// Whether a rule the dialog has is no longer the code's own number.
fn changed(now: &Value, code: &Value) -> bool {
    now != code && now.as_f64() != code.as_f64().or(Some(f64::NAN))
}

fn stairs_tab(ui: &mut Ui, m: &Model, d: &mut CodesDialog) {
    let own = rules_of(m, &d.selected);
    let sources = m.code(&d.selected).map(|c| c.sources.clone()).unwrap_or_default();
    let units = m.units;
    let mut group = "";
    Grid::new("building-code-stairs").striped(true).num_columns(3).min_col_width(150.0).show(ui, |ui| {
        for (key, of, label) in LABELS {
            debug_assert!(ORDER.contains(&key));
            // The most of a pair is on its least's row.
            if PAIRS.iter().any(|(_, high, _)| *high == key) {
                continue;
            }
            if of != group {
                group = of;
                ui.label(RichText::new(t(of)).strong());
                ui.end_row();
            }
            let pair = PAIRS.iter().find(|(low, _, _)| *low == key);
            let keys: Vec<(&str, &str)> = match pair {
                Some((low, high, _)) => {
                    let named = |k: &str| LABELS.iter().find(|(x, _, _)| *x == k).map_or("", |(_, _, l)| *l);
                    vec![(*low, named(low)), (*high, named(high))]
                }
                None => vec![(key, label)],
            };
            ui.label(t(pair.map_or(label, |(_, _, l)| l)));
            ui.horizontal(|ui| {
                for (i, (k, named)) in keys.iter().enumerate() {
                    if pair.is_some() {
                        ui.label(RichText::new(t(if i == 0 { "min." } else { "max." })).color(pal().muted));
                    }
                    if let Some(v) = d.rules.get_mut(*k) {
                        rule_field(ui, units, k, named, v);
                    }
                }
            });
            // A rule the project changed: the code's own beside it, and the way back.
            let moved: Vec<(&str, Value)> = (keys.iter())
                .filter_map(|(k, _)| match (d.rules.get(*k), own.get(*k)) {
                    (Some(now), Some(code)) if changed(now, code) => Some((*k, code.clone())),
                    _ => None,
                })
                .collect();
            if moved.is_empty() {
                // Where the number comes from, so anyone can check it against its document.
                let from: Vec<&str> = keys.iter().filter_map(|(k, _)| sources.get(*k).map(String::as_str)).collect();
                let mut from = from;
                from.dedup();
                if from.is_empty() {
                    ui.label("");
                } else {
                    let said = from.join("; ");
                    ui.add(egui::Label::new(RichText::new(&said).color(pal().muted).small()).truncate()).on_hover_text(&said);
                }
            } else {
                ui.horizontal(|ui| {
                    let was: Vec<String> = moved.iter().map(|(k, code)| said(units, k, code)).collect();
                    let was = if pair.is_some() && moved.len() == 1 {
                        let side = if moved.first().is_some_and(|(k, _)| k.ends_with("_min")) { t("min.") } else { t("max.") };
                        format!("{side} {}", was.join(""))
                    } else {
                        was.join(" – ")
                    };
                    ui.label(RichText::new(tf("Changed: the code says {}", &[&was])).color(OUTSIDE));
                    if ui.small_button(t("Reset")).clicked() {
                        for (k, code) in moved {
                            d.rules.insert(k.to_string(), code);
                        }
                    }
                });
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
        // The stairs nearest it that fit: the solver's answers, each to apply.
        let proposals = match m.elements.get(id) {
            Some(bimcraft_core::model::Element::Stair(st)) => codes::solve(&m, st, codes::Locks::default()),
            _ => Vec::new(),
        };
        let mut close = false;
        let mut apply = None;
        let mut open_floor = false;
        let low = list.as_array().is_some_and(|l| l.iter().any(|f| f["rule"] == "headroom"));
        let r = Modal::new(UiId::new("stair-code-findings"), t("Building Code Check")).width(460.0).show(ctx, |ui| {
            popup::body(ui, |ui| {
                ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    findings(ui, &m, &list);
                    if proposals.is_empty() {
                        return;
                    }
                    ui.add_space(6.0);
                    let fits = proposals.iter().any(|p| p.findings.is_empty());
                    let head = if fits {
                        "Stairs that fit the rules, the nearest first:"
                    } else {
                        "No stair near it fits every rule. The nearest:"
                    };
                    ui.label(RichText::new(t(head)).strong());
                    for (i, p) in proposals.iter().enumerate() {
                        ui.separator();
                        ui.horizontal(|ui| {
                            let label = tf("Apply {}", &[&(i + 1).to_string()]);
                            if ui.button(label).on_hover_text(t("Make the stair this one, in one step that Undo takes back")).clicked() {
                                apply = Some(i + 1);
                            }
                            ui.vertical(|ui| {
                                for c in &p.changes {
                                    ui.add(egui::Label::new(c.replace('`', "")).wrap());
                                }
                                if !p.findings.is_empty() {
                                    let left: Vec<&str> = p
                                        .findings
                                        .iter()
                                        .map(|f| LABELS.iter().find(|(k, _, _)| *k == f.rule).map_or(f.rule, |(_, _, l)| t(l)))
                                        .collect();
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(tf("Still outside: {}", &[&left.join(", ")])).color(OUTSIDE).small(),
                                        )
                                        .wrap(),
                                    );
                                }
                            });
                        });
                    }
                });
            });
            popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    if low
                        && ui
                            .button(t("Cut Opening"))
                            .on_hover_text(t("Cut the floor over the stair open where it leaves less than the headroom over its steps"))
                            .clicked()
                    {
                        open_floor = true;
                    }
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
        if let Some(n) = apply {
            self.run("stair_solve", json!({"id": id, "apply": n}));
        }
        if open_floor {
            self.run("stair_opening", json!({"id": id}));
        }
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
