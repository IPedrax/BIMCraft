//! A stair type's own dialog (egui's `stairtype.rs`, the stair's page of Type Properties): what carries its treads, the sizes that apply to
//! that, the steps, the finishes and materials, the dimension rules each a least and a most that is on or off (off, the project's building code
//! decides, and its number shows), how its stairs show in plan on the level below, their own and the level above, and the lines, fill and
//! symbols they are drawn with. The type is the selected stair's. What is typed goes to the core as one `type` command, which changes the type
//! everywhere it is used.

use bimcraft_core::ids::Id;
use bimcraft_core::model::Model;
use bimcraft_core::units::Units;
use rune::TextEdit;
use serde_json::{Map, Value, json};

use super::{Cell, Choice, Discipline, Foot, Host, Outcome, Section, edits, typed};
use crate::lang::{t, tf};

/// What a rule's numbers are: a length, a plain number, or degrees.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Holds {
    Length,
    Number,
    Degrees,
}

/// A rule: its name, the keys of its least and its most (a rule may have one alone), and what its numbers are.
struct Rule {
    label: &'static str,
    keys: [Option<&'static str>; 2],
    holds: Holds,
}

const RULES: [Rule; 8] = [
    Rule { label: "Riser Height", keys: [Some("riser_min"), Some("riser_max")], holds: Holds::Length },
    Rule { label: "Tread Depth", keys: [Some("going_min"), Some("going_max")], holds: Holds::Length },
    Rule { label: "2 Risers + 1 Tread", keys: [Some("step_min"), Some("step_max")], holds: Holds::Length },
    Rule { label: "Tread / Riser Ratio", keys: [Some("ratio_min"), Some("ratio_max")], holds: Holds::Number },
    Rule { label: "Tread + Riser", keys: [Some("sum_min"), Some("sum_max")], holds: Holds::Length },
    Rule { label: "Stair Pitch", keys: [Some("pitch_min"), Some("pitch_max")], holds: Holds::Degrees },
    Rule { label: "Landing Length", keys: [Some("landing_min"), None], holds: Holds::Length },
    Rule { label: "Walk Line Offset", keys: [Some("walk_offset_min"), None], holds: Holds::Length },
];

/// The sizes of the type, as the core names each: the label it has and which constructions it applies to.
const SIZES: [(&str, &str); 13] = [
    ("waist", "Slab Thickness"),
    ("nosing", "Nosing"),
    ("tread_thickness", "Tread Thickness"),
    ("riser_thickness", "Riser Thickness"),
    ("stringer_width", "Stringer Width"),
    ("stringer_depth", "Stringer Depth"),
    ("carriage_width", "Centre Stringer Width"),
    ("carriage_depth", "Centre Stringer Depth"),
    ("post", "Centre Post Diameter"),
    ("tread_cover", "Tread Finish Thickness"),
    ("riser_cover", "Riser Finish Thickness"),
    ("cover_nosing", "Finish Nosing"),
    ("cover_side", "Finish Side Overhang"),
];

/// What carries a stair's treads: as the core names each, as it reads, and what it means.
const CONSTRUCTIONS: [(&str, &str, &str); 4] = [
    ("monolithic", "Monolithic (Cast Concrete)", "One slab under the steps and the landings, cast in place."),
    ("stringers", "Stringers on Both Sides", "Treads held between two stringers, one along each side."),
    ("carriage", "Centre Stringer", "Treads resting on a single stringer down the middle."),
    ("treads", "Treads Fixed in a Wall", "Each tread fixed into the wall beside it, with nothing underneath."),
];

const PROFILES: [(&str, &str); 2] = [("square", "Square"), ("slanted", "Slanted Riser")];
const FINISH_PROFILES: [(&str, &str); 4] =
    [("square", "Square"), ("rounded", "Rounded Top Edge"), ("chamfered", "Chamfered Top Edge"), ("bullnose", "Bullnose (Half Round)")];
const ARROWS: [(&str, &str); 5] =
    [("filled", "Filled Arrow"), ("open", "Open Arrow"), ("hollow", "Outlined Arrow"), ("tick", "Tick"), ("none", "None")];
const WALK_STARTS: [(&str, &str); 3] = [("none", "None"), ("dot", "Dot"), ("circle", "Circle")];
const CUT_MARKS: [(&str, &str); 4] = [("zigzag", "Zigzag"), ("double", "Double Line"), ("straight", "Straight Line"), ("wave", "Wave")];
/// The ways a stair shows in plan, as the core names each and as it reads.
const WAYS: [(&str, &str); 10] = [
    ("break_visible_hidden", "With Break Mark: Visible - Hidden"),
    ("no_break_visible", "No Break Mark: Visible"),
    ("below_visible", "Below Break Mark: Visible"),
    ("above_visible", "Above Break Mark: Visible"),
    ("no_break_hidden", "No Break Mark: Hidden"),
    ("above_hidden", "Above Break Mark: Hidden"),
    ("below_hidden", "Below Break Mark: Hidden"),
    ("break_all_visible", "With Break Mark: All Visible"),
    ("break_hidden_visible", "With Break Mark: Hidden - Visible"),
    ("not_shown", "Not Shown"),
];

/// The lists, by place: what each chooses.
const CONSTRUCTION: usize = 0;
const PROFILE: usize = 1;
const FINISH_PROFILE: usize = 2;
const ARROW: usize = 3;
const WALK_START: usize = 4;
const CUT_MARK: usize = 5;
const MATERIAL: usize = 6;
const COVER_MATERIAL: usize = 7;
const FILL: usize = 8;
const LINE_VISIBLE: usize = 9;
const LINE_HIDDEN: usize = 10;
/// The three ways it shows in plan: this and the two after it.
const PLAN: usize = 11;

/// The checks, by the place `press("check", ..)` is given.
const BEGIN_RISER: usize = 0;
const END_RISER: usize = 1;
const LANDING_STRINGERS: usize = 2;
const WALK_ROUNDED: usize = 3;
const NUMBERS: usize = 4;
/// The rules' ticks: this and two for each rule after it.
const TICK: usize = 5;

/// The edits: the sizes, then the numbers of the rules (two each).
const RULE_EDITS: usize = SIZES.len();

fn word(v: serde_json::Result<Value>) -> String {
    v.ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

/// The core's word in `list` that `word` is, the first where it is none of them.
fn key_of(list: &[(&'static str, &'static str)], word: &str) -> &'static str {
    list.iter().find(|(k, _)| *k == word).or(list.first()).map_or("", |(k, _)| k)
}

/// The line style `name`, where the project has it; else its style named `usual` (in its language, else in English).
fn usual_style(m: &Model, name: String, usual: &str) -> String {
    if m.line_styles.contains_key(&name) {
        return name;
    }
    [m.language.word(usual), usual].into_iter().find(|n| m.line_styles.contains_key(*n)).map_or(name, str::to_string)
}

/// A number typed without a unit: a ratio or a pitch, a point or a comma for the decimals.
fn plain(text: &str) -> Option<f64> {
    text.trim().trim_end_matches('\u{b0}').trim().replace(',', ".").parse::<f64>().ok().filter(|x| x.is_finite())
}

/// The dialog: the type, what is typed, and the words chosen.
pub struct StairType {
    name: String,
    units: Units,
    edits: Vec<TextEdit>,
    /// Each rule's two ticks: whether the rule is the type's own.
    on: Vec<bool>,
    checks: [bool; TICK],
    /// What each list has chosen, as the core's word (a name for the lines, the fill and the materials).
    picked: Vec<String>,
    /// The names the lists offer besides the core's own words.
    lines: Vec<String>,
    fills: Vec<String>,
    materials: Vec<String>,
    code: String,
    error: Option<String>,
}

impl StairType {
    /// Opens on the type of the stair selected. Why not, in words.
    pub fn open(m: &Model, selection: &[Id]) -> Result<StairType, String> {
        if let [id] = selection
            && matches!(m.elements.get(*id), Some(bimcraft_core::model::Element::Stair(st)) if st.type_name.is_empty())
        {
            return Err(t("That stair has no type: it is solid down to its level.").to_string());
        }
        let name = match selection {
            [id] => crate::typeprops::of_element(m, *id).filter(|k| k.category == "stair").map(|k| k.name),
            _ => None,
        };
        let Some(name) = name else { return Err(t("Select one stair to edit its type first.").to_string()) };
        let kind = m.types.stair.get(&name).cloned().unwrap_or_default();
        let own = serde_json::to_value(&kind).unwrap_or_default();
        let code = m.stair_rules();
        let from_code = |key: &str| match key {
            "riser_min" => Some(code.riser_min),
            "riser_max" => Some(code.riser_max),
            "going_min" => Some(code.going_min),
            "going_max" => Some(code.going_max),
            "step_min" => Some(code.step_min),
            "step_max" => Some(code.step_max),
            "landing_min" => Some(code.landing_length),
            "walk_offset_min" => Some(code.walk_line_offset),
            _ => None,
        };
        let said = |holds: Holds, x: f64| if holds == Holds::Length { m.field(x) } else { ((x * 1000.0).round() / 1000.0).to_string() };
        let mut texts: Vec<String> = SIZES.iter().map(|(k, _)| own[*k].as_f64().map_or_else(String::new, |x| m.field(x))).collect();
        let mut on = Vec::new();
        for rule in &RULES {
            for key in rule.keys {
                let side = key.and_then(|k| own["rules"][k].as_f64().map(|x| (true, x)).or_else(|| from_code(k).map(|x| (false, x))));
                let (ticked, text) = side.map_or((false, String::new()), |(ticked, x)| (ticked, said(rule.holds, x)));
                on.push(ticked);
                texts.push(text);
            }
        }
        let plan = |w: &str| key_of(&WAYS, &word(serde_json::to_value(kind.plan).map(|p| p[w].clone()))).to_string();
        let construction = key_of(&CONSTRUCTIONS.map(|(k, l, _)| (k, l)), &word(serde_json::to_value(kind.built()))).to_string();
        let mut checks = [false; TICK];
        checks[BEGIN_RISER] = !kind.start_tread;
        checks[END_RISER] = !kind.end_tread;
        checks[LANDING_STRINGERS] = kind.landing_stringers;
        checks[WALK_ROUNDED] = kind.walk_rounded;
        checks[NUMBERS] = kind.numbers;
        let mut picked = vec![String::new(); PLAN + 3];
        picked[CONSTRUCTION] = construction;
        picked[PROFILE] = key_of(&PROFILES, &word(serde_json::to_value(kind.profile))).to_string();
        picked[FINISH_PROFILE] = key_of(&FINISH_PROFILES, &word(serde_json::to_value(kind.finish_profile))).to_string();
        picked[ARROW] = key_of(&ARROWS, &word(serde_json::to_value(kind.arrow))).to_string();
        picked[WALK_START] = key_of(&WALK_STARTS, &word(serde_json::to_value(kind.walk_start))).to_string();
        picked[CUT_MARK] = key_of(&CUT_MARKS, &word(serde_json::to_value(kind.cut_mark))).to_string();
        picked[MATERIAL] = kind.material.clone();
        picked[COVER_MATERIAL] = kind.cover_material.clone();
        picked[FILL] = kind.fill.clone();
        picked[LINE_VISIBLE] = usual_style(m, kind.line_visible.clone(), bimcraft_core::lines::MEDIUM);
        picked[LINE_HIDDEN] = usual_style(m, kind.line_hidden.clone(), bimcraft_core::lines::HIDDEN);
        for (i, w) in ["below", "same", "above"].into_iter().enumerate() {
            picked[PLAN + i] = plan(w);
        }
        Ok(StairType {
            name,
            units: m.units,
            edits: edits(texts),
            on,
            checks,
            picked,
            lines: m.line_styles.keys().cloned().collect(),
            fills: m.fill_patterns.keys().cloned().collect(),
            materials: m.materials.keys().cloned().collect(),
            code: t(&m.building_code().name).to_string(),
            error: None,
        })
    }

    fn construction(&self) -> &str {
        &self.picked[CONSTRUCTION]
    }

    fn slanted(&self) -> bool {
        self.picked[PROFILE] == "slanted"
    }

    /// Whether a size applies to what carries the treads: those that don't are left as they are, so switching back finds them again.
    fn used(&self, key: &str) -> bool {
        let c = self.construction();
        match key {
            "waist" => c == "monolithic",
            "tread_thickness" | "riser_thickness" => c != "monolithic",
            "nosing" => c != "monolithic" || self.slanted(),
            "stringer_width" | "stringer_depth" => c == "stringers",
            "carriage_width" | "carriage_depth" => matches!(c, "stringers" | "carriage"),
            "cover_side" => c != "stringers",
            _ => true,
        }
    }

    fn label(&self, key: &str, english: &str) -> String {
        let c = self.construction();
        let label = match key {
            "nosing" if c == "monolithic" => "Riser Slant",
            "carriage_width" if c == "stringers" => "Centre Stringer Width (Optional)",
            _ => english,
        };
        t(label).to_string()
    }

    /// What the `type` command takes for what is typed: the sizes that apply, each rule that is on its number and each that is off null, the
    /// ways it shows in plan, its lines, fill, materials, steps and symbols; or why one is no number.
    pub fn params(&self) -> Result<Value, String> {
        let mut p = json!({"category": "stair", "name": self.name});
        for (i, (key, label)) in SIZES.iter().enumerate() {
            if !self.used(key) {
                continue;
            }
            let text = typed(&self.edits[i]);
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            let value = self.units.parse(text).filter(|x| x.is_finite());
            p[*key] = json!(value.ok_or_else(|| tf("{}: `{}` is not a length", &[&self.label(key, label), text]))?);
        }
        let mut rules = Map::new();
        for (r, rule) in RULES.iter().enumerate() {
            for (side, key) in rule.keys.iter().enumerate() {
                let Some(key) = key else { continue };
                let (ticked, text) = (self.on[r * 2 + side], typed(&self.edits[RULE_EDITS + r * 2 + side]));
                let value = if !ticked {
                    Value::Null
                } else {
                    let read = if rule.holds == Holds::Length { self.units.parse(text.trim()) } else { plain(&text) };
                    json!(read.filter(|x| x.is_finite()).ok_or_else(|| tf("{}: `{}` is not a number", &[t(rule.label), text.trim()]))?)
                };
                rules.insert(key.to_string(), value);
            }
        }
        p["rules"] = Value::Object(rules);
        p["plan"] = json!({"below": self.picked[PLAN], "same": self.picked[PLAN + 1], "above": self.picked[PLAN + 2]});
        p["line_visible"] = json!(self.picked[LINE_VISIBLE]);
        p["line_hidden"] = json!(self.picked[LINE_HIDDEN]);
        p["fill"] = json!(self.picked[FILL]);
        p["material"] = json!(self.picked[MATERIAL]);
        p["cover_material"] = json!(self.picked[COVER_MATERIAL]);
        p["numbers"] = json!(self.checks[NUMBERS]);
        p["start_tread"] = json!(!self.checks[BEGIN_RISER]);
        p["end_tread"] = json!(!self.checks[END_RISER]);
        let mono = self.construction() == "monolithic";
        p["profile"] = json!(if mono { self.picked[PROFILE].as_str() } else { "square" });
        p["construction"] = json!(self.construction());
        p["landing_stringers"] = json!(self.checks[LANDING_STRINGERS] && matches!(self.construction(), "stringers" | "carriage"));
        p["finish_profile"] = json!(self.picked[FINISH_PROFILE]);
        p["arrow"] = json!(self.picked[ARROW]);
        p["walk_start"] = json!(self.picked[WALK_START]);
        p["cut_mark"] = json!(self.picked[CUT_MARK]);
        p["walk_rounded"] = json!(self.checks[WALK_ROUNDED]);
        Ok(p)
    }

    /// The pairs a list offers: the core's word and what it reads.
    fn pairs(&self, pick: usize) -> Vec<(String, String)> {
        let of = |list: &[(&str, &str)]| list.iter().map(|(k, l)| (k.to_string(), t(l).to_string())).collect();
        let names = |list: &[String], none: bool| -> Vec<(String, String)> {
            none.then(|| (String::new(), String::new())).into_iter().chain(list.iter().map(|n| (n.clone(), n.clone()))).collect()
        };
        match pick {
            CONSTRUCTION => CONSTRUCTIONS.iter().map(|(k, l, _)| (k.to_string(), t(l).to_string())).collect(),
            PROFILE => of(&PROFILES),
            FINISH_PROFILE => of(&FINISH_PROFILES),
            ARROW => of(&ARROWS),
            WALK_START => of(&WALK_STARTS),
            CUT_MARK => of(&CUT_MARKS),
            MATERIAL | COVER_MATERIAL => names(&self.materials, true),
            FILL => names(&self.fills, true),
            LINE_VISIBLE | LINE_HIDDEN => names(&self.lines, false),
            p if (PLAN..PLAN + 3).contains(&p) => of(&WAYS),
            _ => Vec::new(),
        }
    }

    fn apply(&mut self, host: &mut dyn Host) -> bool {
        let params = match self.params() {
            Ok(p) => p,
            Err(why) => {
                self.error = Some(why);
                return false;
            }
        };
        match host.execute("type", &params) {
            Ok(_) => {
                self.error = None;
                true
            }
            Err(why) => {
                self.error = Some(why);
                false
            }
        }
    }
}

impl Discipline for StairType {
    fn title(&self) -> String {
        tf("Stair Type: {}", &[&self.name])
    }

    fn about(&self) -> String {
        t("What the stair is built of, its sizes and the rules it is checked against. How plans draw stairs of this type; a view can override this in Visibility/Graphics.").to_string()
    }

    fn width(&self) -> f32 {
        760.0
    }

    fn sections(&self) -> Vec<Section> {
        let hint = self.units.name();
        let pick = |label: &str, pick: usize| vec![Cell::text(t(label), 220.0), Cell::Pick { pick, width: 300.0 }];
        let check = |label: &str, at: usize, on: bool| {
            vec![Cell::Check { action: "check", arg: at, label: t(label).to_string(), on, enabled: true }]
        };
        let c = self.construction();
        let about = CONSTRUCTIONS.iter().find(|(k, ..)| *k == c).map_or("", |(.., a)| a);
        let mut model = vec![pick("Construction", CONSTRUCTION), vec![Cell::text(t(about), 520.0)]];
        if c == "monolithic" {
            model.push(pick("Step Profile", PROFILE));
        }
        for (i, (key, english)) in SIZES.iter().enumerate().filter(|(_, (key, _))| self.used(key)) {
            model.push(vec![Cell::text(self.label(key, english), 220.0), Cell::field(i, self.label(key, english), hint, 100.0)]);
        }
        if matches!(c, "stringers" | "carriage") {
            model.push(check("Stringers Round Landings", LANDING_STRINGERS, self.checks[LANDING_STRINGERS]));
        }
        model.push(check("Begin with Riser", BEGIN_RISER, self.checks[BEGIN_RISER]));
        model.push(check("End with Riser", END_RISER, self.checks[END_RISER]));
        model.push(pick("Finish Edge Profile", FINISH_PROFILE));
        model.push(pick("Finish Material", COVER_MATERIAL));
        model.push(pick("Structure Material", MATERIAL));
        let mut rules = vec![vec![Cell::text(
            tf("A rule ticked is this type's own. One not ticked is the building code's ({}), where it has one.", &[&self.code]),
            640.0,
        )]];
        for (r, rule) in RULES.iter().enumerate() {
            let mut row = vec![Cell::text(t(rule.label), 170.0)];
            for (side, key) in rule.keys.iter().enumerate() {
                if key.is_none() {
                    row.extend([Cell::text("", 100.0), Cell::text("", 80.0)]);
                    continue;
                }
                let word = if side == 0 { t("min.") } else { t("max.") };
                let unit = match rule.holds {
                    Holds::Length => hint,
                    Holds::Degrees => "\u{b0}",
                    Holds::Number => "",
                };
                row.push(Cell::Check {
                    action: "check",
                    arg: TICK + r * 2 + side,
                    label: word.to_string(),
                    on: self.on[r * 2 + side],
                    enabled: true,
                });
                row.push(Cell::field(RULE_EDITS + r * 2 + side, format!("{} {}", t(rule.label), word), unit, 80.0));
            }
            rules.push(row);
        }
        let mut graphics = Vec::new();
        for (i, label) in ["On the Level Below", "On Its Own Level", "On the Level Above"].into_iter().enumerate() {
            graphics.push(pick(label, PLAN + i));
        }
        for (label, at) in [
            ("Visible Line Style", LINE_VISIBLE),
            ("Hidden Line Style", LINE_HIDDEN),
            ("Fill of the Visible Part", FILL),
            ("Walk Line Arrow", ARROW),
            ("Walk Line Start", WALK_START),
        ] {
            graphics.push(pick(label, at));
        }
        graphics.push(check("Rounded Walk Line", WALK_ROUNDED, self.checks[WALK_ROUNDED]));
        graphics.push(pick("Cut Mark", CUT_MARK));
        graphics.push(check("Tread Numbers", NUMBERS, self.checks[NUMBERS]));
        vec![
            Section::new(t("Model"), model),
            Section::new(t("Rules and Standards"), rules),
            Section::new(t("Floor Plan Display and Symbols"), graphics),
        ]
    }

    fn edit(&self, i: usize) -> Option<&TextEdit> {
        self.edits.get(i)
    }

    fn edit_mut(&mut self, i: usize) -> Option<&mut TextEdit> {
        self.edits.get_mut(i)
    }

    fn choice(&self, pick: usize) -> Option<Choice> {
        let pairs = self.pairs(pick);
        if pairs.is_empty() {
            return None;
        }
        let now = self.picked.get(pick)?;
        let options: Vec<String> =
            pairs.iter().map(|(_, label)| if label.is_empty() { t("None").to_string() } else { label.clone() }).collect();
        Some(Choice {
            name: tf("Stair type list {}", &[&(pick + 1).to_string()]),
            current: pairs.iter().position(|(k, _)| k == now),
            options,
        })
    }

    fn choose(&mut self, pick: usize, option: usize, _host: &mut dyn Host) {
        let key = self.pairs(pick).get(option).map(|(k, _)| k.clone());
        if let (Some(key), Some(slot)) = (key, self.picked.get_mut(pick)) {
            *slot = key;
        }
    }

    fn press(&mut self, action: &str, arg: usize, host: &mut dyn Host) -> Outcome {
        match action {
            "check" if arg < TICK => {
                if let Some(c) = self.checks.get_mut(arg) {
                    *c = !*c;
                }
            }
            "check" => {
                if let Some(on) = self.on.get_mut(arg - TICK) {
                    *on = !*on;
                }
            }
            "apply" => {
                self.apply(host);
            }
            "ok" => {
                if self.apply(host) {
                    return Outcome::Close;
                }
            }
            "cancel" => return Outcome::Close,
            _ => {}
        }
        Outcome::Keep
    }

    fn foot(&self) -> Vec<Foot> {
        vec![Foot::new("cancel", t("Cancel"), false), Foot::new("apply", t("Apply"), false), Foot::new("ok", t("OK"), true)]
    }

    fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

#[cfg(test)]
mod tests;
