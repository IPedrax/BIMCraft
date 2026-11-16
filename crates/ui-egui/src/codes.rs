//! Manage > Building Codes: the project's building codes, one tab for each discipline (architectural, structural,
//! electrical, mechanical, plumbing) and an Overview with the region presets and the site's numbers. A tab picks its base
//! code and the amendments over it, lists the sections of rules, and shows each rule with where it comes from (the code, an
//! amendment, BIMCraft's library or the project), its source and its kind; any rule, tables included, can be changed for the
//! project, and a tab is imported, exported and saved as a code on its own. Nothing is ever refused for breaking a rule:
//! what is outside one is built and marked, and the window a Properties sign opens lists what to change.

use std::collections::BTreeMap;

use bimcraft_core::codes::{self, BuildingCode, Codes, Discipline, Layer, ORDER};
use bimcraft_core::model::{Element, Model};
use bimcraft_core::units::Units;
use bimcraft_i18n::msg;
use egui::{Align, Color32, ComboBox, Context, DragValue, Grid, Id as UiId, Layout, RichText, ScrollArea, Ui, WidgetInfo, WidgetType};
use serde_json::{Map, Value, json};

use crate::BimApp;
use crate::lang::{t, tf};
use crate::popup::{self, Modal};
use crate::style::pal;

/// The colour of what is outside a rule, or changed from the code's.
pub(crate) const OUTSIDE: Color32 = Color32::from_rgb(0xC8, 0x7A, 0x00);

/// What each stair rule is called, and the group it is listed under.
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

/// What the rules of the other sections are called, by section and rule, and how their numbers read: `mm` a length in the
/// project's units, `%`, `m²`, or a plain number.
const RULES: [(&str, &str, &str, &str); 20] = [
    ("ramps", "slope", "New Ramp Slope", "%"),
    ("ramps", "width", "New Ramp Width", "mm"),
    ("ramps", "slope_max", "Steepest Slope", "%"),
    ("ramps", "slope_by_rise", "Steepest Slope by a Run's Rise", ""),
    ("ramps", "run_rise_max", "Most Rise of a Run", "mm"),
    ("ramps", "width_min", "Minimum Width", "mm"),
    ("ramps", "landing_length_min", "Minimum Landing Length", "mm"),
    ("doors", "clear_width_min", "Minimum Clear Width", "mm"),
    ("doors", "clear_height_min", "Minimum Clear Height", "mm"),
    ("doors", "leaf_width_max", "Widest Leaf (0: none)", "mm"),
    ("doors", "frame_allowance", "Frame Allowance", "mm"),
    ("doors", "exempt", "Doors Not Held (by type name)", ""),
    ("rooms", "kinds", "Kinds of Room", ""),
    ("cold_water", "max_speed", "Most Speed (m/s)", ""),
    ("cold_water", "min_use_kpa", "Least Pressure at a Point of Use (kPa)", ""),
    ("drainage", "main_min_dn", "Least Building Drain DN", ""),
    ("storm", "default_rain", "Rainfall for a Small Roof (mm/h)", ""),
    ("storm", "min_downpipe", "Least Downpipe (mm inside)", ""),
    ("hydrants", "max_speed", "Most Speed (m/s)", ""),
    ("hydrants", "max_nozzle_head", "Most Head at a Nozzle (mca)", ""),
];

/// What to do about an element outside rule `rule` of its kind, as a person would say it.
fn advice(category: &str, rule: &str) -> Option<&'static str> {
    Some(match (category, rule) {
        ("ramp", "width_min") => msg!("The ramp is too narrow. Make it wider."),
        ("ramp", "slope_max") | ("ramp", "slope_by_rise") => {
            msg!("The ramp is too steep for how far it climbs. Make its slope gentler, or break it with landings.")
        }
        ("ramp", "run_rise_max") => msg!("A run of the ramp climbs too far without a rest. Add a landing."),
        ("ramp", "landing_length_min") => msg!("A landing of the ramp is too short. Make it longer."),
        ("door", "clear_width_min") => msg!("The door is too narrow to pass through. Use a wider door type."),
        ("door", "clear_height_min") => msg!("The door is too low. Use a taller door type."),
        ("door", "leaf_width_max") => msg!("The leaf is too wide. Use a pair of narrower leaves."),
        ("room", "area_min") => msg!("The room is too small for its kind. Move a wall to enlarge it, or rename it if it is another kind."),
        ("room", "side_min") => msg!("The room is too narrow for its kind. Move a wall to widen it."),
        ("room", "height_min") => msg!("The room is too low. Raise its upper limit, or what is over it."),
        (_, "riser_max") => msg!("The risers are too high. Add more risers so each one is lower, or lower the Riser in the Options Bar."),
        (_, "riser_min") => msg!("The risers are too low. Use fewer, higher risers."),
        (_, "going_min") => msg!("The treads are too shallow. Make them deeper."),
        (_, "going_max") => msg!("The treads are too deep. Make them shallower."),
        (_, "step_min") => msg!("The steps are too short for a comfortable stride. Deepen the treads, or use fewer risers."),
        (_, "step_max") => msg!("The steps are too long for a comfortable stride. Make the treads shallower, or use more risers."),
        (_, "width_min") => msg!("The stair is too narrow. Make it wider."),
        (_, "headroom") => {
            msg!("Something overhead is too low. Cut it open over the stair with Cut Opening below, or raise what is in the way.")
        }
        (_, "flight_rise_max") | (_, "flight_risers_max") => {
            msg!("This flight climbs too far without a rest. Add a landing to break it up; Add Landings Due in Properties does it for you.")
        }
        (_, "landing_length") => msg!("A landing is too short. Make it longer."),
        (_, "winders") => msg!("This code doesn't allow winders. Use a landing at the turn instead."),
        (_, "winder_going_min") => {
            msg!("The winders are too shallow on the walk line. Use fewer of them, or spread them into the flights.")
        }
        (_, "winder_narrow_min") => {
            msg!("The winders come too close to a point at the inside. Spread them into the flights so each keeps some depth.")
        }
        (_, "nosing_min") => {
            msg!("The treads don't stand out enough over the risers. Give the stair type a longer nosing or finish nosing.")
        }
        (_, "nosing_max") => msg!("The treads stand out too far over the risers. Give the stair type a shorter nosing."),
        (_, "open_risers") => msg!("This code doesn't allow open risers. Use a stair type with riser boards."),
        (_, "uniformity") => msg!("The risers aren't all the same height. Pick a riser height that divides each storey more evenly."),
        (_, "ratio_min" | "ratio_max" | "sum_min" | "sum_max" | "pitch_min" | "pitch_max") => {
            msg!("The stair's proportions are outside what its type allows. Change the riser or the tread, or the type's rules.")
        }
        _ => return None,
    })
}

/// What a rule is called, in the interface's language: a stair's by its name, another's as the window lists it, else its
/// key in words.
pub(crate) fn rule_label(rule: &str) -> &str {
    let of_type = TYPE_LABELS.iter().find(|(k, _)| *k == rule).map(|(_, l)| t(l));
    LABELS.iter().find(|(k, _, _)| *k == rule).map(|(_, _, l)| t(l)).or(of_type).unwrap_or(rule)
}

/// A rule of section `section` as the window lists it, and how its numbers read.
fn named(section: &str, rule: &str) -> (String, &'static str) {
    if section == "stairs" {
        let unit = if rule == "flight_risers_max" { "" } else { "mm" };
        return (rule_label(rule).to_string(), unit);
    }
    if let Some((_, _, label, unit)) = RULES.iter().find(|(s, r, _, _)| *s == section && *r == rule) {
        return (t(label).to_string(), unit);
    }
    // Its key in words: `clear_width_min` is "Clear width min".
    let mut words = rule.replace('_', " ");
    if let Some(first) = words.get(..1) {
        words = format!("{}{}", first.to_uppercase(), words.get(1..).unwrap_or(""));
    }
    (words, "")
}

/// What a stair rule that is a choice may be, as the core names each and as it is shown.
fn choices(key: &str) -> &'static [(&'static str, &'static str)] {
    match key {
        "landing_rule" => &[("width", "The stair's width"), ("fixed", "A fixed length"), ("step", "A tread and one step")],
        "walk_line" => &[("centre", "Along the middle"), ("narrow_side", "From the narrow side")],
        "flow_method" => &[("weights", "Weights (√ΣP)"), ("sum", "Every fixture at once")],
        _ => &[],
    }
}

/// A length as the project writes its lengths; in millimetres, to the hundredth, since a code's inch is 25.4.
fn length(units: Units, mm: f64) -> String {
    if units == Units::Millimetres { format!("{} mm", (mm * 100.0).round() / 100.0) } else { units.with_unit(mm) }
}

/// How a rule's value reads.
fn said(units: Units, key: &str, unit: &str, v: &Value) -> String {
    match v {
        Value::Bool(b) => t(if *b { "Yes" } else { "No" }).to_string(),
        Value::String(s) => choices(key).iter().find(|(k, _)| k == s).map_or_else(|| s.clone(), |(_, label)| t(label).to_string()),
        Value::Number(n) if unit == "mm" => length(units, n.as_f64().unwrap_or(0.0)),
        Value::Number(n) if unit.is_empty() => n.to_string(),
        Value::Number(n) => format!("{n} {unit}"),
        Value::Array(a) => tf("a table of {} rows", &[&a.len().to_string()]),
        Value::Object(o) => tf("{} entries", &[&o.len().to_string()]),
        Value::Null => "—".to_string(),
    }
}

/// Whether a rule the dialog has is no longer the layers' own number.
fn changed(now: &Value, code: &Value) -> bool {
    now != code && now.as_f64() != code.as_f64().or(Some(f64::NAN))
}

/// Where a rule comes from, as the From column says it.
fn from_said(layer: Option<&Layer>) -> String {
    match layer {
        Some(Layer::Base(_)) => t("code").to_string(),
        Some(Layer::Amendment(c)) => c.clone(),
        Some(Layer::Library(c)) => tf("library: {}", &[c]),
        Some(Layer::Project) => t("project").to_string(),
        None => String::new(),
    }
}

/// One discipline's tab: the base code and amendments chosen in the dialog, and the rules as they would be applied, by
/// section.
#[derive(Clone, Debug)]
pub(crate) struct Tab {
    pub(crate) discipline: Discipline,
    pub(crate) code: String,
    pub(crate) amendments: Vec<String>,
    pub(crate) rules: BTreeMap<String, Map<String, Value>>,
    /// The section shown.
    pub(crate) section: String,
}

/// The project's codes as they would be with tab `tab`'s choice: what the layers say before any change of the project's.
fn with_choice(m: &Model, tab: &Tab) -> Codes {
    let mut c = (*m.codes).clone();
    let choice = c.choice_mut(tab.discipline);
    choice.code = Some(tab.code.clone());
    choice.amendments = tab.amendments.clone();
    choice.changes.clear();
    c
}

impl Tab {
    fn new(m: &Model, d: Discipline) -> Tab {
        let base = m.codes.base(d).code.clone();
        let amendments = m.codes.layers(d).iter().skip(1).map(|c| c.code.clone()).collect();
        let mut rules = BTreeMap::new();
        for s in codes::sections().iter().filter(|s| s.discipline == d) {
            rules.insert(s.key.to_string(), m.resolve_code(d, s.key).value);
        }
        let section = codes::sections().iter().find(|s| s.discipline == d).map_or_else(String::new, |s| s.key.to_string());
        Tab { discipline: d, code: base, amendments, rules, section }
    }

    /// The rules of section `key` as the chosen layers have them, before the dialog's changes.
    fn layered(&self, m: &Model, key: &str) -> codes::Resolved {
        codes::layered(&with_choice(m, self), self.discipline, key)
    }

    /// Picks `code` as the base: the amendments that no longer stand over it go, and the rules are the code's.
    pub(crate) fn set(&mut self, m: &Model, code: &str) {
        self.code = code.to_string();
        let c = with_choice(m, self);
        let held: Vec<String> = c.layers(self.discipline).iter().skip(1).map(|k| k.code.clone()).collect();
        self.amendments.retain(|a| held.contains(a));
        self.reload(m);
    }

    /// The rules the chosen layers have, with nothing changed.
    fn reload(&mut self, m: &Model) {
        let keys: Vec<String> = self.rules.keys().cloned().collect();
        for key in keys {
            let v = self.layered(m, &key).value;
            self.rules.insert(key, v);
        }
    }

    /// The rules of each section that are no longer the layers' own, by section.
    fn changes(&self, m: &Model) -> BTreeMap<String, Vec<String>> {
        let mut out = BTreeMap::new();
        for (key, rules) in &self.rules {
            let own = self.layered(m, key).value;
            let moved: Vec<String> =
                rules.iter().filter(|(k, v)| own.get(*k).is_none_or(|o| changed(v, o))).map(|(k, _)| k.clone()).collect();
            if !moved.is_empty() {
                out.insert(key.clone(), moved);
            }
        }
        out
    }

    /// Whether the tab is not what the project has now.
    fn differs(&self, m: &Model) -> bool {
        let fresh = Tab::new(m, self.discipline);
        fresh.code != self.code || fresh.amendments != self.amendments || fresh.rules != self.rules
    }

    /// The `building_code` params that make the project's choice this tab's: its code, its amendments and its rules.
    fn params(&self, m: &Model) -> Value {
        let mut rules = Map::new();
        for (key, moved) in self.changes(m) {
            let Some(all) = self.rules.get(&key) else { continue };
            let only: Map<String, Value> = moved.iter().filter_map(|k| all.get(k).map(|v| (k.clone(), v.clone()))).collect();
            rules.insert(key, Value::Object(only));
        }
        json!({"discipline": self.discipline.key(), "set": self.code, "amendments": self.amendments, "rules": rules})
    }
}

/// The Building Codes dialog: the tab shown, the five disciplines' tabs, the site's numbers, and what is being typed.
pub(crate) struct CodesDialog {
    /// 0 the Overview, then the disciplines in `Discipline::ALL`'s order.
    pub(crate) tab: usize,
    pub(crate) tabs: Vec<Tab>,
    pub(crate) site: Map<String, Value>,
    pub(crate) search: String,
    /// The name the shown tab's rules would be saved under, as a code of the project's own.
    pub(crate) name: String,
    /// What went wrong, said inside the dialog.
    pub(crate) error: String,
    /// The rule the card shows, and the table being edited: (section, rule).
    pub(crate) card: Option<(String, String)>,
    pub(crate) table: Option<(String, String)>,
    /// Rows pasted as tab-separated text, waiting to be taken into the table.
    pub(crate) pasted: String,
    /// How many elements are outside a rule, by discipline, counted when the dialog opened.
    pub(crate) outside: BTreeMap<Discipline, usize>,
}

impl CodesDialog {
    pub(crate) fn new(m: &Model) -> CodesDialog {
        let tabs = Discipline::ALL.iter().map(|d| Tab::new(m, *d)).collect();
        let site = match serde_json::to_value(&m.codes.site) {
            Ok(Value::Object(o)) => o,
            _ => Map::new(),
        };
        // What is outside a rule, counted once: the architectural sections check elements today.
        let mut outside = BTreeMap::new();
        let n: usize = m.elements.iter().map(|(id, e)| codes::findings(m, *id, e).len()).sum();
        outside.insert(Discipline::Architectural, n);
        CodesDialog {
            tab: 1,
            tabs,
            site,
            search: String::new(),
            name: String::new(),
            error: String::new(),
            card: None,
            table: None,
            pasted: String::new(),
            outside,
        }
    }

    /// The tab of discipline `d`.
    pub(crate) fn tab_mut(&mut self, d: Discipline) -> &mut Tab {
        let i = Discipline::ALL.iter().position(|x| *x == d).unwrap_or(0);
        let last = self.tabs.len().saturating_sub(1);
        &mut self.tabs[i.min(last)]
    }

    /// The architectural tab's stair rules, as #386's dialog had them.
    #[cfg(test)]
    pub(crate) fn stairs_mut(&mut self) -> &mut Map<String, Value> {
        self.tab_mut(Discipline::Architectural).rules.entry("stairs".into()).or_default()
    }

    fn shown(&self) -> Option<&Tab> {
        self.tab.checked_sub(1).and_then(|i| self.tabs.get(i))
    }
}

/// An element's findings, as the window its Properties' sign opens lists them: the code, then each rule broken with what
/// the element has, what the rule asks, and what would bring it inside.
pub(crate) fn findings(ui: &mut Ui, m: &Model, category: &str, list: &Value) {
    ui.label(RichText::new(t(&m.building_code().name)).strong());
    ui.label(RichText::new(t("It is built as drawn. These rules it is outside of:")).color(pal().muted));
    for f in list.as_array().into_iter().flatten() {
        ui.separator();
        let rule = f["rule"].as_str().unwrap_or("");
        let section = match category {
            "ramp" => "ramps",
            "door" => "doors",
            "room" => "rooms",
            _ => "stairs",
        };
        let (label, unit) = named(section, rule);
        ui.label(RichText::new(label).color(OUTSIDE).strong());
        // What it has and what the rule asks; a yes or no rule has no numbers.
        if rule != "winders" {
            let (has, asks) = (f["value"].as_f64().unwrap_or(0.0), f["limit"].as_f64().unwrap_or(0.0));
            let unit = if category == "room" && rule == "area_min" { "m²" } else { unit };
            let shown = |x: f64| said(m.units, rule, unit, &json!(x));
            Grid::new(("finding", rule)).num_columns(2).show(ui, |ui| {
                ui.label(RichText::new(t("It has:")).color(pal().muted));
                ui.label(shown(has));
                ui.end_row();
                ui.label(RichText::new(t("The rule:")).color(pal().muted));
                ui.label(shown(asks));
                ui.end_row();
            });
        }
        // What to do about it, in plain words; the core's own sentence where there are none here.
        let fix = match advice(category, rule) {
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

/// The field of one rule's value: a tick, a choice, a count, a length or a number; a table opens the table editor.
fn rule_field(ui: &mut Ui, units: Units, key: &str, unit: &str, label: &str, v: &mut Value) -> bool {
    let mut open = false;
    match v {
        Value::Bool(b) => {
            let r = ui.checkbox(b, "");
            r.widget_info(|| WidgetInfo::labeled(WidgetType::Checkbox, true, label));
        }
        Value::String(s) if !choices(key).is_empty() => {
            let now = choices(key).iter().find(|(k, _)| k == s).map_or(s.as_str(), |(_, l)| l);
            let c = ComboBox::from_id_salt(("building-code-rule", key)).selected_text(t(now)).width(170.0).show_ui(ui, |ui| {
                for (k, l) in choices(key) {
                    ui.selectable_value(s, (*k).to_string(), t(l));
                }
            });
            c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, label));
        }
        Value::String(s) => {
            let r = ui.add(egui::TextEdit::singleline(s).desired_width(220.0));
            r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, label));
        }
        Value::Number(n) if (n.is_u64() || n.is_i64()) && unit.is_empty() => {
            let mut x = n.as_i64().unwrap_or(0);
            let r = ui.add(DragValue::new(&mut x).range(0..=1_000_000));
            r.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, label));
            *v = json!(x);
        }
        Value::Number(n) => {
            let mut x = n.as_f64().unwrap_or(0.0);
            let r = if unit == "mm" {
                // In the project's units, typed as its lengths are.
                ui.add(
                    DragValue::new(&mut x)
                        .range(0.0..=100_000.0)
                        .speed(1.0)
                        .custom_formatter(|v, _| length(units, v))
                        .custom_parser(|typed| units.parse(typed.trim().trim_end_matches("mm").trim())),
                )
            } else {
                ui.add(DragValue::new(&mut x).range(-1.0e9..=1.0e9).speed(0.01).max_decimals(6).suffix(if unit.is_empty() {
                    String::new()
                } else {
                    format!(" {unit}")
                }))
            };
            r.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, label));
            *v = json!(x);
        }
        Value::Array(_) | Value::Object(_) | Value::Null => {
            let r = ui.button(said(units, key, unit, v)).on_hover_text(t("Edit the table below"));
            r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
            open = r.clicked();
        }
    }
    open
}

/// The stair rules shown as one row with a least and a most side by side: the least's key, the most's, and the row's name.
const PAIRS: [(&str, &str, &str); 5] = [
    ("riser_min", "riser_max", "Riser Height"),
    ("going_min", "going_max", "Tread Depth"),
    ("step_min", "step_max", "Step (2 risers + 1 tread)"),
    ("nosing_min", "nosing_max", "Nosing"),
    ("handrail_min", "handrail_max", "Handrail Height"),
];

/// The rows of section `key`: groups of rules, each row one rule or a pair, in the order the window lists them.
fn rows(key: &str, rules: &Map<String, Value>) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    if key == "stairs" {
        let mut group = "";
        for (k, of, _) in LABELS {
            if PAIRS.iter().any(|(_, high, _)| *high == k) || !rules.contains_key(k) {
                continue;
            }
            let keys = match PAIRS.iter().find(|(low, _, _)| *low == k) {
                Some((low, high, _)) => vec![low.to_string(), high.to_string()],
                None => vec![k.to_string()],
            };
            if of != group {
                group = of;
                out.push((t(of).to_string(), Vec::new()));
            }
            out.push((String::new(), keys));
        }
        // Anything else the stairs hold, below.
        for k in rules.keys().filter(|k| !ORDER.contains(&k.as_str())) {
            out.push((String::new(), vec![k.clone()]));
        }
        return out;
    }
    // The section's rules as the window names them first, the rest after, numbers before tables.
    let mut keys: Vec<&String> = rules.keys().collect();
    let known = |k: &str| RULES.iter().position(|(s, r, _, _)| *s == key && *r == k).unwrap_or(usize::MAX);
    keys.sort_by_key(|k| (known(k), rules.get(*k).is_some_and(|v| v.is_array() || v.is_object()), k.to_string()));
    keys.into_iter().map(|k| (String::new(), vec![k.clone()])).collect()
}

/// A cell of a table as typed: a number, a yes or no, JSON, else text.
fn cell_value(typed: &str) -> Value {
    let typed = typed.trim();
    match typed {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        "" | "—" => Value::Null,
        _ => serde_json::from_str::<Value>(typed).unwrap_or_else(|_| Value::String(typed.to_string())),
    }
}

/// A cell as the table shows it.
fn cell_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "—".to_string(),
        other => other.to_string(),
    }
}

/// The columns of a table: an array of objects has a column for each key any row has; an array of arrays one for each
/// place; else none, and each row is one cell.
fn columns(rows: &[Value]) -> Vec<String> {
    if rows.iter().all(Value::is_object) {
        let mut cols: Vec<String> = Vec::new();
        for r in rows {
            for k in r.as_object().into_iter().flat_map(|o| o.keys()) {
                if !cols.contains(k) {
                    cols.push(k.clone());
                }
            }
        }
        return cols;
    }
    let widest = rows.iter().filter_map(Value::as_array).map(Vec::len).max().unwrap_or(0);
    if rows.iter().all(Value::is_array) { (1..=widest).map(|i| i.to_string()).collect() } else { Vec::new() }
}

/// The table `v` as tab-separated text, a header line first where its rows are objects.
pub(crate) fn as_tsv(v: &Value) -> String {
    let rows: Vec<Value> = match v {
        Value::Array(a) => a.clone(),
        Value::Object(o) => o.iter().map(|(k, v)| json!([k, v])).collect(),
        other => vec![other.clone()],
    };
    let cols = columns(&rows);
    let mut out = String::new();
    let objects = rows.iter().all(Value::is_object) && !rows.is_empty();
    if objects {
        out.push_str(&cols.join("\t"));
        out.push('\n');
    }
    for r in &rows {
        let cells: Vec<String> = match r {
            Value::Object(o) => cols.iter().map(|c| o.get(c).map_or_else(String::new, cell_text)).collect(),
            Value::Array(a) => a.iter().map(cell_text).collect(),
            other => vec![cell_text(other)],
        };
        out.push_str(&cells.join("\t"));
        out.push('\n');
    }
    out
}

/// Rows pasted as tab-separated text, read as the table `like` is: objects by its header line, else arrays.
pub(crate) fn from_tsv(text: &str, like: &Value) -> Option<Value> {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() || lines.len() > 5000 {
        return None;
    }
    let objects = like.as_array().is_some_and(|a| !a.is_empty() && a.iter().all(Value::is_object));
    if objects {
        let head: Vec<&str> = lines.first()?.split('\t').map(str::trim).collect();
        let rows = lines.iter().skip(1).map(|l| {
            let o: Map<String, Value> = head.iter().zip(l.split('\t')).map(|(k, c)| ((*k).to_string(), cell_value(c))).collect();
            Value::Object(o)
        });
        return Some(Value::Array(rows.collect()));
    }
    let rows = lines.iter().map(|l| {
        let cells: Vec<Value> = l.split('\t').map(cell_value).collect();
        if cells.len() == 1 { cells.into_iter().next().unwrap_or(Value::Null) } else { Value::Array(cells) }
    });
    Some(Value::Array(rows.collect()))
}

/// The table editor of rule `rule`: its cells edited in place, rows added and taken away, copied and pasted as text.
fn table_editor(ui: &mut Ui, label: &str, v: &mut Value, pasted: &mut String) -> bool {
    let mut close = false;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                close = ui.small_button(t("Close Table")).clicked();
                if ui.small_button(t("Copy")).on_hover_text(t("Copy the table as tab-separated text, for a spreadsheet")).clicked() {
                    ui.ctx().copy_text(as_tsv(v));
                }
            });
        });
        // A map is edited as rows of its key and value.
        if let Value::Object(o) = v {
            let mut entries: Vec<(String, String)> = o.iter().map(|(k, x)| (k.clone(), cell_text(x))).collect();
            let mut edited = false;
            Grid::new(("code-table-map", label)).striped(true).num_columns(2).show(ui, |ui| {
                for (k, x) in entries.iter_mut() {
                    ui.label(k.as_str());
                    edited |= ui.add(egui::TextEdit::singleline(x).desired_width(360.0)).changed();
                    ui.end_row();
                }
            });
            if edited {
                *o = entries.into_iter().map(|(k, x)| (k, cell_value(&x))).collect();
            }
            return;
        }
        let Value::Array(rows) = v else { return };
        let cols = columns(rows);
        let mut gone = None;
        ScrollArea::horizontal().id_salt(("code-table", label)).show(ui, |ui| {
            Grid::new(("code-table-grid", label)).striped(true).show(ui, |ui| {
                for c in &cols {
                    ui.label(RichText::new(c).color(pal().muted).small());
                }
                ui.label("");
                ui.end_row();
                for (i, row) in rows.iter_mut().enumerate() {
                    match row {
                        Value::Object(o) => {
                            for c in &cols {
                                let mut text = o.get(c).map_or_else(String::new, cell_text);
                                if ui.add(egui::TextEdit::singleline(&mut text).desired_width(90.0)).changed() {
                                    o.insert(c.clone(), cell_value(&text));
                                }
                            }
                        }
                        Value::Array(a) => {
                            for j in 0..cols.len() {
                                let mut text = a.get(j).map_or_else(String::new, cell_text);
                                if ui.add(egui::TextEdit::singleline(&mut text).desired_width(70.0)).changed() {
                                    while a.len() <= j {
                                        a.push(Value::Null);
                                    }
                                    a[j] = cell_value(&text);
                                }
                            }
                        }
                        other => {
                            let mut text = cell_text(other);
                            if ui.add(egui::TextEdit::singleline(&mut text).desired_width(200.0)).changed() {
                                *other = cell_value(&text);
                            }
                        }
                    }
                    if ui.small_button("×").on_hover_text(t("Take this row away")).clicked() {
                        gone = Some(i);
                    }
                    ui.end_row();
                }
            });
        });
        if let Some(i) = gone {
            rows.remove(i);
        }
        ui.horizontal(|ui| {
            if ui.small_button(t("Add Row")).clicked() {
                let blank = match rows.last() {
                    Some(Value::Object(o)) => Value::Object(o.keys().map(|k| (k.clone(), Value::Null)).collect()),
                    Some(Value::Array(a)) => Value::Array(vec![Value::Null; a.len()]),
                    _ => Value::Null,
                };
                rows.push(blank);
            }
            ui.label(RichText::new(t("Paste rows from a spreadsheet:")).color(pal().muted).small());
        });
        let mut take = false;
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::multiline(pasted).desired_rows(2).desired_width(420.0).hint_text(t("Tab-separated rows")));
            take = ui.add_enabled(!pasted.trim().is_empty(), egui::Button::new(t("Take Pasted Rows"))).clicked();
        });
        if take && let Some(new) = from_tsv(pasted, &Value::Array(rows.clone())) {
            *v = new;
            pasted.clear();
        }
    });
    close
}

/// A discipline's tab: its header (code, amendments, actions), the section list, the rules grid and the card.
fn discipline_tab(ui: &mut Ui, m: &Model, d: &mut CodesDialog, actions: &mut Actions) {
    let Some(i) = d.tab.checked_sub(1) else { return };
    let units = m.units;
    let Some(tab) = d.tabs.get(i).cloned() else { return };
    let disc = tab.discipline;
    let mut tab = tab;
    // The code: its base, and the amendments laid over it.
    let all: Vec<(BuildingCode, bool)> = m.codes.all(disc).map(|(c, own)| (c.clone(), own)).collect();
    let base = all.iter().find(|(c, _)| c.code == tab.code).map(|(c, _)| c.clone()).unwrap_or_default();
    let moved = tab.changes(m);
    let custom = !moved.is_empty();
    ui.horizontal(|ui| {
        ui.label(RichText::new(t("Code:")).strong());
        let now = if custom { tf("Custom (based on {})", &[t(&base.name)]) } else { base.title() };
        let c = ComboBox::from_id_salt(("building-code-selector", disc.key())).selected_text(now).width(340.0).show_ui(ui, |ui| {
            for (c, _) in all.iter().filter(|(c, _)| c.amends.is_empty()) {
                let label = if c.edition.is_empty() { c.title() } else { format!("{} · {}", c.title(), c.edition) };
                if ui.selectable_label(!custom && tab.code == c.code, label).clicked() {
                    tab.set(m, &c.code);
                }
            }
        });
        c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, t("Building Code")));
        if base.checked.is_empty() {
            ui.label(RichText::new(t("unchecked numbers")).color(OUTSIDE).small()).on_hover_text(t(
                "These numbers haven't been checked against the documents yet. Check one against a copy before you rely on it.",
            ));
        } else {
            ui.label(RichText::new(t("✔ checked")).color(pal().muted).small()).on_hover_text(&base.checked);
        }
        if custom {
            ui.label(RichText::new(t("Custom")).color(OUTSIDE).strong());
        }
    });
    // Amendments: a chip each, taken away by its cross; one more added from those that stand over the layers.
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(t("Amendments:")).strong());
        let mut gone = None;
        for (k, a) in tab.amendments.iter().enumerate() {
            let name = all.iter().find(|(c, _)| c.code == *a).map_or(a.clone(), |(c, _)| c.name.clone());
            egui::Frame::group(ui.style()).inner_margin(egui::Margin::symmetric(6, 2)).show(ui, |ui| {
                ui.label(name);
                if ui.small_button("×").on_hover_text(t("Take this amendment away")).clicked() {
                    gone = Some(k);
                }
            });
        }
        if let Some(k) = gone {
            tab.amendments.remove(k);
            tab.reload(m);
        }
        let held: Vec<String> = std::iter::once(tab.code.clone()).chain(tab.amendments.iter().cloned()).collect();
        let open: Vec<&BuildingCode> =
            all.iter().map(|(c, _)| c).filter(|c| !c.amends.is_empty() && held.contains(&c.amends) && !held.contains(&c.code)).collect();
        if open.is_empty() {
            if tab.amendments.is_empty() {
                ui.label(RichText::new(t("none for this code")).color(pal().muted).small());
            }
        } else {
            ComboBox::from_id_salt(("building-code-amend", disc.key())).selected_text(t("+ Add…")).width(220.0).show_ui(ui, |ui| {
                for c in open {
                    if ui.selectable_label(false, c.title()).clicked() {
                        tab.amendments.push(c.code.clone());
                        tab.reload(m);
                    }
                }
            });
        }
    });
    ui.horizontal(|ui| {
        actions.import =
            ui.button(t("Import…")).on_hover_text(t("Read a building code from an XML file, as one of the project's own")).clicked();
        actions.export = ui
            .button(t("Export…"))
            .on_hover_text(t("Write this tab's rules as an XML file, to hand to another project or office"))
            .clicked();
        if all.iter().any(|(c, own)| *own && c.code == tab.code) && !custom {
            actions.delete = ui
                .button(t("Delete"))
                .on_hover_text(t("Take this code of the project's own away; a project held to it keeps its rules"))
                .clicked();
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.add_enabled(custom, egui::Button::new(t("Reset Tab to the Code's Rules"))).clicked() {
                tab.reload(m);
            }
        });
    });
    // A custom code is saved under a name of the user's own, and is then a code like the others.
    if custom {
        ui.horizontal(|ui| {
            let l = ui.label(t("Save as:"));
            ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(240.0).hint_text(t("A name of your own"))).labelled_by(l.id);
            actions.save = ui.add_enabled(!d.name.trim().is_empty(), egui::Button::new(t("Save Code"))).clicked();
            ui.label(RichText::new(t("Unsaved, the rules are still the project's when applied.")).color(pal().muted).small());
        });
    }
    let gone: Vec<(String, String, String)> = m.codes.editions_gone(disc);
    for (code, pinned, now) in gone {
        ui.label(
            RichText::new(tf(
                "{} was chosen in its edition {}, which this BIMCraft no longer has: it is held to {}.",
                &[&code, &pinned, &now],
            ))
            .color(OUTSIDE)
            .small(),
        );
    }
    ui.separator();
    // The sections on the left, the rules of the one shown on the right.
    let sections: Vec<&codes::SectionDef> = codes::sections().iter().filter(|s| s.discipline == disc).collect();
    let search = d.search.trim().to_lowercase();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(230.0);
            let r = ui.add(egui::TextEdit::singleline(&mut d.search).hint_text(t("🔍 find a rule…")).desired_width(220.0));
            r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, t("Find a rule")));
            ui.add_space(4.0);
            for s in &sections {
                let held = tab.rules.get(s.key).is_some_and(|r| !r.is_empty());
                // A search shows the sections with a rule of that name, key or source.
                if !search.is_empty() {
                    let hits = tab.rules.get(s.key).is_some_and(|r| {
                        r.keys().any(|k| k.to_lowercase().contains(&search) || named(s.key, k).0.to_lowercase().contains(&search))
                    }) || t(s.label).to_lowercase().contains(&search);
                    if !hits {
                        continue;
                    }
                }
                let mut label = t(s.label).to_string();
                if let Some(n) = moved.get(s.key).map(Vec::len) {
                    label = format!("{label}  • {n}");
                }
                if s.read_by.is_empty() {
                    label = format!("{label}  {}", t("(ref.)"));
                }
                let text = if held { RichText::new(label) } else { RichText::new(label).color(pal().muted) };
                if ui.selectable_label(tab.section == s.key, text).on_hover_text(t(s.about)).clicked() {
                    tab.section = s.key.to_string();
                    d.table = None;
                }
            }
            ui.add_space(6.0);
            ui.label(RichText::new(t("• changed for this project")).color(pal().muted).small());
            ui.label(RichText::new(t("(ref.) reference only: nothing checks these yet")).color(pal().muted).small());
        });
        ui.separator();
        ui.vertical(|ui| {
            let key = tab.section.clone();
            let Some(def) = codes::section_def(disc, &key) else { return };
            let layered = tab.layered(m, &key);
            ui.horizontal(|ui| {
                ui.label(RichText::new(t(def.label)).strong().size(15.0));
                if def.read_by.is_empty() {
                    ui.label(RichText::new(t("reference only")).color(pal().muted).small());
                }
            });
            ui.label(RichText::new(t(def.about)).color(pal().muted).small());
            if !def.read_by.is_empty() {
                ui.label(RichText::new(tf("Read by: {}", &[&def.read_by.join(", ")])).color(pal().muted).small());
            }
            ui.add_space(4.0);
            let Some(rules) = tab.rules.get_mut(&key) else { return };
            if rules.is_empty() {
                ui.label(RichText::new(t("None of the codes chosen holds this section yet.")).color(pal().muted));
                return;
            }
            let mut open = None;
            Grid::new(("building-code-rules", disc.key(), key.as_str())).striped(true).num_columns(4).min_col_width(110.0).show(ui, |ui| {
                ui.label(RichText::new(t("Rule")).color(pal().muted).small());
                ui.label(RichText::new(t("Value")).color(pal().muted).small());
                ui.label(RichText::new(t("From")).color(pal().muted).small());
                ui.label(RichText::new(t("Source")).color(pal().muted).small());
                ui.end_row();
                for (group, keys) in rows(&key, rules) {
                    if keys.is_empty() {
                        ui.label(RichText::new(group).strong());
                        ui.end_row();
                        continue;
                    }
                    if !search.is_empty()
                        && !keys.iter().any(|k| k.to_lowercase().contains(&search) || named(&key, k).0.to_lowercase().contains(&search))
                    {
                        continue;
                    }
                    let pair = (key == "stairs").then(|| PAIRS.iter().find(|(low, _, _)| keys.first().is_some_and(|k| k == low))).flatten();
                    let row_label = pair.map_or_else(|| named(&key, &keys[0]).0, |(_, _, l)| t(l).to_string());
                    if ui.selectable_label(d.card.as_ref().is_some_and(|(s, r)| *s == key && keys.contains(r)), &row_label).clicked() {
                        d.card = Some((key.clone(), keys[0].clone()));
                    }
                    ui.horizontal(|ui| {
                        for (n, k) in keys.iter().enumerate() {
                            if pair.is_some() {
                                ui.label(RichText::new(t(if n == 0 { "min." } else { "max." })).color(pal().muted));
                            }
                            let (label, unit) = named(&key, k);
                            if let Some(v) = rules.get_mut(k)
                                && rule_field(ui, units, k, unit, &label, v)
                            {
                                open = Some(k.clone());
                            }
                        }
                    });
                    // A rule the project changes: the layers' own beside it, and the way back; else where it comes from.
                    let moved: Vec<(String, Value)> = keys
                        .iter()
                        .filter_map(|k| match (rules.get(k), layered.value.get(k)) {
                            (Some(now), Some(code)) if changed(now, code) => Some((k.clone(), code.clone())),
                            (Some(_), None) => Some((k.clone(), Value::Null)),
                            _ => None,
                        })
                        .collect();
                    if moved.is_empty() {
                        ui.label(RichText::new(from_said(layered.from.get(&keys[0]))).color(pal().muted).small());
                        let from: Vec<String> = keys.iter().filter_map(|k| layered.sources.get(k).cloned()).collect();
                        let mut from = from;
                        from.dedup();
                        if from.is_empty() {
                            ui.label("");
                        } else {
                            let said = from.join("; ");
                            ui.add(egui::Label::new(RichText::new(&said).color(pal().muted).small()).truncate()).on_hover_text(&said);
                        }
                    } else {
                        ui.label(RichText::new(t("project")).color(OUTSIDE).small());
                        ui.horizontal(|ui| {
                            let was: Vec<String> = moved.iter().map(|(k, code)| said(units, k, named(&key, k).1, code)).collect();
                            let was = if pair.is_some() && moved.len() == 1 {
                                let side = if moved.first().is_some_and(|(k, _)| k.ends_with("_min")) { t("min.") } else { t("max.") };
                                format!("{side} {}", was.join(""))
                            } else {
                                was.join(" – ")
                            };
                            ui.label(RichText::new(tf("Changed: the code says {}", &[&was])).color(OUTSIDE));
                            if ui.small_button(t("Reset")).clicked() {
                                for (k, code) in moved {
                                    if code.is_null() {
                                        rules.remove(&k);
                                    } else {
                                        rules.insert(k, code);
                                    }
                                }
                            }
                        });
                    }
                    ui.end_row();
                }
            });
            if let Some(k) = open {
                d.table = Some((key.clone(), k.clone()));
                d.card = Some((key.clone(), k));
            }
            // The table being edited, under the grid.
            if let Some((s, r)) = d.table.clone()
                && s == key
                && let Some(v) = rules.get_mut(&r)
            {
                ui.add_space(6.0);
                if table_editor(ui, &named(&key, &r).0, v, &mut d.pasted) {
                    d.table = None;
                }
            }
            // The card: the rule picked, with its full source, kind and where it comes from.
            if let Some((s, r)) = d.card.clone()
                && s == key
            {
                ui.add_space(6.0);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(named(&key, &r).0).strong());
                    ui.label(RichText::new(format!("{key}.{r}")).color(pal().muted).small().monospace());
                    let kind = layered.kinds.get(&r).map_or("", |k| k.label());
                    let from = from_said(layered.from.get(&r));
                    ui.label(
                        RichText::new(format!("{} · {} · {}", tf("from {}", &[&from]), t(kind), base.named())).color(pal().muted).small(),
                    );
                    match layered.sources.get(&r) {
                        Some(src) => ui.add(egui::Label::new(src.as_str()).wrap()),
                        None => ui.label(RichText::new(t("No source given.")).color(OUTSIDE).small()),
                    };
                });
            }
        });
    });
    if let Some(slot) = d.tabs.get_mut(i) {
        *slot = tab;
    }
}

/// The Overview: a region preset that sets every tab, the five tabs in short, and the site's numbers.
fn overview_tab(ui: &mut Ui, m: &Model, d: &mut CodesDialog, actions: &mut Actions) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(t("Region preset:")).strong());
        let c = ComboBox::from_id_salt("building-code-preset").selected_text(t("Pick a region…")).width(260.0).show_ui(ui, |ui| {
            for (i, p) in codes::presets().iter().enumerate() {
                if ui.selectable_label(false, t(&p.name)).on_hover_text(t(&p.note)).clicked() {
                    actions.preset = Some(i);
                }
            }
        });
        c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, t("Region Preset")));
        ui.label(RichText::new(t("sets all five tabs")).color(pal().muted).small());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            actions.export_all = ui.button(t("Export All…")).on_hover_text(t("Write all five tabs' rules as one XML file")).clicked();
            actions.import_all =
                ui.button(t("Import All…")).on_hover_text(t("Read building codes from an XML file and hold the project to them")).clicked();
        });
    });
    ui.add_space(8.0);
    Grid::new("building-code-cards").num_columns(3).spacing([10.0, 10.0]).show(ui, |ui| {
        for (i, tab) in d.tabs.clone().iter().enumerate() {
            let base = m.codes.find(tab.discipline, &tab.code).cloned().unwrap_or_default();
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(250.0);
                ui.vertical(|ui| {
                    if ui.selectable_label(false, RichText::new(t(tab.discipline.label())).strong()).clicked() {
                        d.tab = i + 1;
                    }
                    ui.label(base.named());
                    for a in &tab.amendments {
                        ui.label(
                            RichText::new(format!("+ {}", m.codes.find(tab.discipline, a).map_or(a.as_str(), |c| c.name.as_str()))).small(),
                        );
                    }
                    let changes: usize = tab.changes(m).values().map(Vec::len).sum();
                    let status = if base.checked.is_empty() { t("unchecked numbers").to_string() } else { t("✔ checked").to_string() };
                    let status = match changes {
                        0 => status,
                        1 => format!("{status} · {}", t("1 change")),
                        n => format!("{status} · {}", tf("{} changes", &[&n.to_string()])),
                    };
                    ui.label(RichText::new(status).color(pal().muted).small());
                    if let Some(n) = d.outside.get(&tab.discipline).filter(|n| **n > 0) {
                        ui.label(RichText::new(tf("{} findings in the project", &[&n.to_string()])).color(OUTSIDE).small());
                    }
                });
            });
            if i % 3 == 2 {
                ui.end_row();
            }
        }
    });
    ui.add_space(10.0);
    ui.label(RichText::new(t("Site and Climate")).strong());
    ui.label(
        RichText::new(t(
            "The numbers a code's maps give for the project's place, each with the map it was read from. Storm drains read the rain.",
        ))
        .color(pal().muted)
        .small(),
    );
    Grid::new("building-code-site").num_columns(3).striped(true).show(ui, |ui| {
        let numbers: [(&str, &str); 8] = [
            ("rain_intensity", msg!("Rainfall intensity")),
            ("rain_return_period", msg!("Return period")),
            ("wind_speed", msg!("Basic wind speed")),
            ("ground_snow", msg!("Ground snow load")),
            ("frost_depth", msg!("Frost depth")),
            ("altitude", msg!("Altitude")),
            ("cooling_design", msg!("Cooling design temperature")),
            ("heating_design", msg!("Heating design temperature")),
        ];
        for (key, label) in numbers {
            let unit = codes::SITE_NUMBERS.iter().find(|(k, ..)| *k == key).map_or("", |(_, u, ..)| *u);
            ui.label(t(label));
            ui.horizontal(|ui| {
                let mut on = d.site.contains_key(key);
                if ui.checkbox(&mut on, "").on_hover_text(t("Given for this project")).changed() {
                    if on {
                        d.site.insert(key.to_string(), json!(0.0));
                    } else {
                        d.site.remove(key);
                    }
                }
                if let Some(v) = d.site.get_mut(key) {
                    let mut x = v.as_f64().unwrap_or(0.0);
                    let r = ui.add(DragValue::new(&mut x).speed(0.1).suffix(format!(" {unit}")));
                    r.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, t(label)));
                    *v = json!(x);
                }
            });
            site_source(ui, d, key);
            ui.end_row();
        }
        let texts: [(&str, &str); 3] =
            [("thunderstorms", msg!("Thunderstorms (AQ)")), ("climate_zone", msg!("Climate zone")), ("seismic_zone", msg!("Seismic zone"))];
        for (key, label) in texts {
            ui.label(t(label));
            let mut text = d.site.get(key).and_then(Value::as_str).unwrap_or("").to_string();
            let allowed = codes::SITE_TEXTS.iter().find(|(k, _)| *k == key).map_or(&[][..], |(_, a)| *a);
            if allowed.is_empty() {
                if ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0)).changed() {
                    if text.trim().is_empty() {
                        d.site.remove(key);
                    } else {
                        d.site.insert(key.to_string(), json!(text));
                    }
                }
            } else {
                ComboBox::from_id_salt(("site", key)).selected_text(if text.is_empty() { "—".to_string() } else { text.clone() }).show_ui(
                    ui,
                    |ui| {
                        if ui.selectable_label(text.is_empty(), "—").clicked() {
                            d.site.remove(key);
                        }
                        for a in allowed {
                            if ui.selectable_label(text == *a, *a).clicked() {
                                d.site.insert(key.to_string(), json!(a));
                            }
                        }
                    },
                );
            }
            site_source(ui, d, key);
            ui.end_row();
        }
    });
}

/// The source field of the site's number `key`.
fn site_source(ui: &mut Ui, d: &mut CodesDialog, key: &str) {
    if !d.site.contains_key(key) {
        ui.label("");
        return;
    }
    let sources = d.site.entry("sources").or_insert_with(|| json!({}));
    let Some(o) = sources.as_object_mut() else { return };
    let mut text = o.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    if ui.add(egui::TextEdit::singleline(&mut text).desired_width(260.0).hint_text(t("the map or table it is from"))).changed() {
        if text.trim().is_empty() {
            o.remove(key);
        } else {
            o.insert(key.to_string(), json!(text));
        }
    }
}

/// What the dialog was asked to do this frame.
#[derive(Default)]
struct Actions {
    ok: bool,
    apply: bool,
    cancel: bool,
    save: bool,
    delete: bool,
    import: bool,
    export: bool,
    import_all: bool,
    export_all: bool,
    preset: Option<usize>,
}

impl BimApp {
    /// The window an element's sign in Properties opens: the rules it is outside of. It closes when the element is inside
    /// them all, or gone.
    pub(crate) fn findings_window(&mut self, ctx: &Context) {
        let Some(id) = self.code_findings else { return };
        let m = self.session.model.clone();
        let Some(e) = m.elements.get(id) else {
            self.code_findings = None;
            return;
        };
        let category = e.category();
        let list = serde_json::to_value(codes::findings(&m, id, e)).unwrap_or(Value::Null);
        if list.as_array().is_none_or(Vec::is_empty) {
            self.code_findings = None;
            return;
        }
        // The stairs nearest it that fit: the solver's answers, each to apply.
        let proposals = match e {
            Element::Stair(st) => codes::solve(&m, st, codes::Locks::default()),
            _ => Vec::new(),
        };
        let mut close = false;
        let mut apply = None;
        let mut open_floor = false;
        let low = list.as_array().is_some_and(|l| l.iter().any(|f| f["rule"] == "headroom"));
        let r = Modal::new(UiId::new("stair-code-findings"), t("Building Code Check")).width(460.0).show(ctx, |ui| {
            popup::body(ui, |ui| {
                ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    findings(ui, &m, category, &list);
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
                                    let left: Vec<&str> = p.findings.iter().map(|f| rule_label(f.rule)).collect();
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

    /// Building codes read from the XML file at `path`, kept as the project's own; `all` holds the project to them, else
    /// the code read is shown in its tab.
    pub(crate) fn import_code(&mut self, path: &std::path::Path, all: bool) {
        let reply = self
            .run("building_code_import", json!({"path": path.to_string_lossy(), "set": all}))
            .ok_or_else(|| std::mem::take(&mut self.status.text));
        let m = self.session.model.clone();
        let mut d = self.codes_dialog.take().unwrap_or_else(|| CodesDialog::new(&m));
        match reply {
            Ok(v) => {
                if all {
                    let tab = d.tab;
                    d = CodesDialog::new(&m);
                    d.tab = tab;
                } else {
                    for c in v["codes"].as_array().into_iter().flatten() {
                        let (Some(name), Some(disc)) = (c["code"].as_str(), c["discipline"].as_str().and_then(Discipline::parse)) else {
                            continue;
                        };
                        let Some(code) = m.codes.find(disc, name).cloned() else { continue };
                        let tab = d.tab_mut(disc);
                        if code.amends.is_empty() {
                            tab.set(&m, name);
                        } else if !tab.amendments.iter().any(|a| a == name) {
                            tab.amendments.push(name.to_string());
                            tab.reload(&m);
                        }
                        d.tab = Discipline::ALL.iter().position(|x| *x == disc).map_or(d.tab, |i| i + 1);
                    }
                }
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

    /// The tabs the dialog shows written as an XML file at `path`, one discipline (`only`) or all five: worked out on a copy
    /// of the project, so that what is not applied yet is exported as shown and the project stays as it is.
    pub(crate) fn export_code(&mut self, path: &std::path::Path, tabs: &[Tab], only: Option<Discipline>, name: &str) {
        let mut scratch = bimcraft_core::engine::Session::new();
        *scratch.model_mut() = (*self.session.model).clone();
        let mut reply = Ok(Value::Null);
        for tab in tabs {
            if reply.is_err() {
                break;
            }
            let m = scratch.model.clone();
            reply = scratch.execute("building_code", &tab.params(&m));
        }
        let mut p = json!({"path": path.to_string_lossy()});
        match only {
            Some(disc) => p["discipline"] = json!(disc.key()),
            None => p["all"] = json!(true),
        }
        if !name.is_empty() {
            p["name"] = json!(name);
        }
        let reply = reply.and_then(|_| scratch.execute("building_code_export", &p));
        if let Some(d) = self.codes_dialog.as_mut() {
            d.error = reply.err().unwrap_or_default();
        }
    }

    /// The Building Codes modal dialog.
    pub(crate) fn codes_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.codes_dialog.take() else { return };
        let mut a = Actions::default();
        let m = self.session.model.clone();
        let m = &m;

        let r = Modal::new(UiId::new("building-codes-dialog"), t("Building Codes")).width(1000.0).min_height(560.0).show(ctx, |ui| {
            popup::body(ui, |ui| {
                ui.label(RichText::new(t(
                    "The building codes this project is held to, one tab for each discipline. Any rule can be changed, for a state's or a city's own; what is outside a rule is still built, and marked.",
                )).color(pal().muted));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.selectable_label(d.tab == 0, t("Overview")).clicked() {
                        d.tab = 0;
                    }
                    for (i, tab) in d.tabs.iter().enumerate() {
                        let moved = tab.changes(m).values().map(Vec::len).sum::<usize>();
                        let label = if moved > 0 { format!("{} •", t(tab.discipline.label())) } else { t(tab.discipline.label()).to_string() };
                        if ui.selectable_label(d.tab == i + 1, label).clicked() {
                            d.tab = i + 1;
                            d.card = None;
                            d.table = None;
                        }
                    }
                });
                ui.separator();
                if !d.error.is_empty() {
                    ui.label(RichText::new(&d.error).color(pal().error));
                }
                if d.tab == 0 {
                    overview_tab(ui, m, &mut d, &mut a);
                } else {
                    discipline_tab(ui, m, &mut d, &mut a);
                }
            });
            popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        a.apply = ui.button(t("Apply")).clicked();
                        a.cancel = ui.button(t("Cancel")).clicked();
                        a.ok = ui.button(t("OK")).clicked();
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
        let shown = d.shown().cloned();
        let mut done = true;
        if let Some(i) = a.preset
            && let Some(p) = codes::presets().get(i)
        {
            for tab in d.tabs.iter_mut() {
                let pick = p.choices.get(&tab.discipline);
                tab.code = pick.map_or_else(|| codes::default_of(tab.discipline).code.clone(), |k| k.code.clone());
                tab.amendments = pick.map(|k| k.amendments.clone()).unwrap_or_default();
                tab.reload(m);
            }
        }
        if a.save
            && let Some(tab) = &shown
        {
            let mut p = tab.params(m);
            p["save"] = json!(d.name.trim());
            if let Some(v) = tried(self, &mut d, "building_code", p) {
                let fresh = Tab::new(&self.session.model, tab.discipline);
                *d.tab_mut(tab.discipline) = Tab { section: tab.section.clone(), ..fresh };
                let _ = v;
                d.name.clear();
            }
        } else if a.delete
            && let Some(tab) = &shown
        {
            let p = json!({"discipline": tab.discipline.key(), "delete": tab.code});
            if tried(self, &mut d, "building_code", p).is_some() {
                let fresh = Tab::new(&self.session.model, tab.discipline);
                *d.tab_mut(tab.discipline) = Tab { section: tab.section.clone(), ..fresh };
            }
        } else if a.ok || a.apply {
            // Each tab the project doesn't have yet, then the site's numbers.
            for tab in d.tabs.clone() {
                let now = self.session.model.clone();
                if !tab.differs(&now) {
                    continue;
                }
                if tried(self, &mut d, "building_code", tab.params(&now)).is_none() {
                    done = false;
                    break;
                }
            }
            if done {
                let site = match serde_json::to_value(&self.session.model.codes.site) {
                    Ok(Value::Object(o)) => o,
                    _ => Map::new(),
                };
                if site != d.site {
                    // Every number of the site, those taken away as null.
                    let mut p = d.site.clone();
                    for (k, _) in site.iter().filter(|(k, _)| !d.site.contains_key(*k) && k.as_str() != "sources") {
                        p.insert(k.clone(), Value::Null);
                    }
                    done = tried(self, &mut d, "site_climate", Value::Object(p)).is_some();
                }
            }
            if done && a.apply {
                let tab = d.tab;
                let sections: Vec<String> = d.tabs.iter().map(|t| t.section.clone()).collect();
                d = CodesDialog::new(&self.session.model);
                d.tab = tab;
                for (t, s) in d.tabs.iter_mut().zip(sections) {
                    t.section = s;
                }
            }
        }
        if a.import || a.import_all {
            let all = a.import_all;
            let dialog = rfd::AsyncFileDialog::new().add_filter(t("Building codes"), &["xml", "XML"]);
            self.ask_file(ctx, dialog, false, move |app, path| app.import_code(&path, all));
        }
        if a.export
            && let Some(tab) = &shown
        {
            // What the tab shows, under the name typed, else its code's: from a copy of the project, which stays as it is.
            let (tabs, disc, name) = (vec![tab.clone()], tab.discipline, d.name.trim().to_string());
            let file = format!("{}.xml", if name.is_empty() { tab.code.as_str() } else { name.as_str() }.replace(['/', '\\'], "-"));
            let dialog = rfd::AsyncFileDialog::new().add_filter(t("Building codes"), &["xml"]).set_file_name(file);
            self.ask_file(ctx, dialog, true, move |app, path| app.export_code(&path, &tabs, Some(disc), &name));
        }
        if a.export_all {
            let tabs = d.tabs.clone();
            let dialog = rfd::AsyncFileDialog::new().add_filter(t("Building codes"), &["xml"]).set_file_name("building-codes.xml");
            self.ask_file(ctx, dialog, true, move |app, path| app.export_code(&path, &tabs, None, ""));
        }
        if !((a.ok && done) || a.cancel || r.should_close()) {
            self.codes_dialog = Some(d);
        }
    }
}
