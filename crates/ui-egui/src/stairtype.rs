//! A stair type's own page in Type Properties: its dimension rules, each a least and a most that is on or off (off, the
//! project's building code decides, and its number shows greyed); how its stairs show in plan on the level below, their
//! own level and the level above; and the line patterns and the fill they are drawn with.

use bimcraft_core::model::{Model, PlanPart, PlanShow};
use bimcraft_core::units::Units;
use egui::{Grid, RichText, TextEdit, Ui};
use serde_json::{Value, json};

use bimcraft_i18n::msg;

use crate::lang::{t, tf};
use crate::properties::number;
use crate::style::pal;
use crate::typeprops::Table;

/// What a rule's numbers are: a length, a plain number, or degrees.
#[derive(Clone, Copy, PartialEq)]
enum Holds {
    Length,
    Number,
    Degrees,
}

/// A rule as typed: its name, the keys of its least and its most (a rule may have one alone), whether each is on, and
/// each as typed.
#[derive(Clone, PartialEq)]
struct Rule {
    label: &'static str,
    keys: [Option<&'static str>; 2],
    on: [bool; 2],
    typed: [String; 2],
    holds: Holds,
}

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

/// Each way a stair shows in plan, as the core has it and as it reads.
pub(crate) fn plan_ways() -> Vec<(PlanShow, &'static str)> {
    PlanShow::ALL.into_iter().zip(WAYS.iter().map(|(_, l)| *l)).collect()
}

/// A stair type's rules and plan display, as typed.
#[derive(Clone, PartialEq)]
pub(crate) struct StairDraft {
    rules: Vec<Rule>,
    /// How it shows on the level below, its own level and the level above, as they read.
    plan: [String; 3],
    line_visible: String,
    line_hidden: String,
    fill: String,
    /// The structure's material and the finishes', "" for none.
    material: String,
    cover_material: String,
    /// Whether its plans number its treads.
    numbers: bool,
    /// A monolithic stair's step profile, as it reads: square, or the riser slanted back by the nosing.
    profile: String,
    /// Whether its stairs begin and end with a riser (else with a tread level with the floor).
    begin_riser: bool,
    end_riser: bool,
    /// What carries its treads, as the core names it.
    construction: &'static str,
    /// Whether a stringed stair's stringers go round its landings too.
    landing_stringers: bool,
    /// The front edge of its tread finish, the head and the foot of its walk line in plan, and its break mark, as the core
    /// names each.
    finish_profile: &'static str,
    arrow: &'static str,
    walk_start: &'static str,
    cut_mark: &'static str,
    /// Whether the walk line rounds its corners at a landing.
    walk_rounded: bool,
}

/// What carries a stair's treads: as the core names each, as it reads, and what it means.
const CONSTRUCTIONS: [(&str, &str, &str); 4] = [
    ("monolithic", "Monolithic (Cast Concrete)", "One slab under the steps and the landings, cast in place."),
    ("stringers", "Stringers on Both Sides", "Treads held between two stringers, one along each side."),
    ("carriage", "Centre Stringer", "Treads resting on a single stringer down the middle."),
    ("treads", "Treads Fixed in a Wall", "Each tread fixed into the wall beside it, with nothing underneath."),
];

/// The front edges a tread finish may have.
const FINISH_PROFILES: [(&str, &str); 4] =
    [("square", "Square"), ("rounded", "Rounded Top Edge"), ("chamfered", "Chamfered Top Edge"), ("bullnose", "Bullnose (Half Round)")];

/// The heads of the walk line, the marks at its foot, and the break marks.
const ARROWS: [(&str, &str); 5] =
    [("filled", "Filled Arrow"), ("open", "Open Arrow"), ("hollow", "Outlined Arrow"), ("tick", "Tick"), ("none", "None")];
const WALK_STARTS: [(&str, &str); 3] = [("none", "None"), ("dot", "Dot"), ("circle", "Circle")];
const CUT_MARKS: [(&str, &str); 4] = [("zigzag", "Zigzag"), ("double", "Double Line"), ("straight", "Straight Line"), ("wave", "Wave")];

/// What `v` serializes to, as a name: the core's word for a choice.
fn word(v: serde_json::Result<Value>) -> String {
    v.ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

/// The core's word in `list` that `v` is, the first where it is none of them.
fn key_of(list: &[(&'static str, &'static str)], v: serde_json::Result<Value>) -> &'static str {
    let w = word(v);
    list.iter().find(|(k, _)| *k == w).or(list.first()).map_or("", |(k, _)| k)
}

/// A drop-down row of `list`, each shown as it reads; `key` the core's word picked.
fn choose(tb: &mut Table, label: &str, key: &mut &'static str, list: &[(&'static str, &'static str)]) {
    let options: Vec<String> = list.iter().map(|(_, l)| t(l).to_string()).collect();
    let mut now = list.iter().find(|(k, _)| k == key).map_or(String::new(), |(_, l)| t(l).to_string());
    let was = now.clone();
    tb.pick(label, &mut now, &options);
    if now != was
        && let Some((k, _)) = list.iter().find(|(_, l)| t(l) == now)
    {
        *key = k;
    }
}

/// The step profiles, as the core names each and as it reads.
const PROFILES: [(&str, &str); 2] = [("square", "Square"), ("slanted", "Slanted Riser")];

impl StairDraft {
    pub(crate) fn new(m: &Model, name: &str) -> StairDraft {
        let kind = m.types.stair.get(name).cloned().unwrap_or_default();
        let built = kind.built();
        let own = serde_json::to_value(kind.rules).unwrap_or_default();
        let code = m.stair_rules();
        let said = |holds: Holds, x: f64| match holds {
            Holds::Length => m.units.length(x),
            _ => ((x * 1000.0).round() / 1000.0).to_string(),
        };
        // What the building code has for a rule that is off: shown greyed, so it is plain what applies.
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
        let rule = |label: &'static str, keys: [Option<&'static str>; 2], holds: Holds| {
            let side = |key: Option<&'static str>| match key
                .and_then(|k| own[k].as_f64().map(|x| (true, x)).or_else(|| from_code(k).map(|x| (false, x))))
            {
                Some((on, x)) => (on, said(holds, x)),
                None => (false, String::new()),
            };
            let (low, high) = (side(keys[0]), side(keys[1]));
            Rule { label, keys, on: [low.0, high.0], typed: [low.1, high.1], holds }
        };
        let both = |a: &'static str, b: &'static str| [Some(a), Some(b)];
        let way = |w: bimcraft_core::model::PlanShow| {
            let key = serde_json::to_value(w).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
            WAYS.iter().find(|(k, _)| *k == key).map_or(String::new(), |(_, l)| t(l).to_string())
        };
        StairDraft {
            rules: vec![
                rule("Riser Height", both("riser_min", "riser_max"), Holds::Length),
                rule("Tread Depth", both("going_min", "going_max"), Holds::Length),
                rule("2 Risers + 1 Tread", both("step_min", "step_max"), Holds::Length),
                rule("Tread / Riser Ratio", both("ratio_min", "ratio_max"), Holds::Number),
                rule("Tread + Riser", both("sum_min", "sum_max"), Holds::Length),
                rule("Stair Pitch", both("pitch_min", "pitch_max"), Holds::Degrees),
                rule("Landing Length", [Some("landing_min"), None], Holds::Length),
                rule("Walk Line Offset", [Some("walk_offset_min"), None], Holds::Length),
            ],
            plan: [way(kind.plan.below), way(kind.plan.same), way(kind.plan.above)],
            // A style it names, else the one its plan is drawn in: shown, so it is plain which.
            line_visible: usual_style(m, kind.line_visible, bimcraft_core::lines::MEDIUM),
            line_hidden: usual_style(m, kind.line_hidden, bimcraft_core::lines::HIDDEN),
            fill: kind.fill,
            material: kind.material,
            cover_material: kind.cover_material,
            numbers: kind.numbers,
            begin_riser: !kind.start_tread,
            end_riser: !kind.end_tread,
            construction: {
                let w = word(serde_json::to_value(built));
                CONSTRUCTIONS.iter().find(|(k, ..)| *k == w).map_or("monolithic", |(k, ..)| k)
            },
            landing_stringers: kind.landing_stringers,
            finish_profile: key_of(&FINISH_PROFILES, serde_json::to_value(kind.finish_profile)),
            arrow: key_of(&ARROWS, serde_json::to_value(kind.arrow)),
            walk_start: key_of(&WALK_STARTS, serde_json::to_value(kind.walk_start)),
            cut_mark: key_of(&CUT_MARKS, serde_json::to_value(kind.cut_mark)),
            walk_rounded: kind.walk_rounded,
            profile: {
                let key = serde_json::to_value(kind.profile).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
                PROFILES.iter().find(|(k, _)| *k == key).map_or(String::new(), |(_, l)| t(l).to_string())
            },
        }
    }

    /// What it is built as, by the core's word for it.
    #[cfg(test)]
    pub(crate) fn set_construction(&mut self, to: &str) {
        if let Some((k, ..)) = CONSTRUCTIONS.iter().find(|(k, ..)| *k == to) {
            self.construction = k;
        }
    }

    /// Whether its profile is the slanted riser's.
    pub(crate) fn slanted(&self) -> bool {
        PROFILES.iter().find(|(_, l)| t(l) == self.profile).is_some_and(|(k, _)| *k == "slanted")
    }

    /// What the `type` command takes for what is typed: each rule that is on its number, each that is off null; or why
    /// one is no number.
    pub(crate) fn params(&self, p: &mut Value, units: Units) -> Result<(), String> {
        let mut rules = serde_json::Map::new();
        for rule in &self.rules {
            for side in 0..2 {
                let Some(key) = rule.keys[side] else { continue };
                let typed = rule.typed[side].trim();
                let v = if !rule.on[side] {
                    Value::Null
                } else {
                    let read = match rule.holds {
                        Holds::Length => units.parse(typed),
                        _ => number(typed.trim_end_matches('°')),
                    };
                    json!(read.ok_or_else(|| tf("{}: `{}` is not a number", &[t(rule.label), typed]))?)
                };
                rules.insert(key.to_string(), v);
            }
        }
        let way = |shown: &String| WAYS.iter().find(|(_, l)| t(l) == shown).map_or("break_visible_hidden", |(k, _)| k);
        p["rules"] = Value::Object(rules);
        p["plan"] = json!({"below": way(&self.plan[0]), "same": way(&self.plan[1]), "above": way(&self.plan[2])});
        p["line_visible"] = json!(self.line_visible);
        p["line_hidden"] = json!(self.line_hidden);
        p["fill"] = json!(self.fill);
        p["material"] = json!(self.material);
        p["cover_material"] = json!(self.cover_material);
        p["numbers"] = json!(self.numbers);
        p["start_tread"] = json!(!self.begin_riser);
        p["end_tread"] = json!(!self.end_riser);
        p["profile"] = json!(if self.construction == "monolithic" {
            PROFILES.iter().find(|(_, l)| t(l) == self.profile).map_or("square", |(k, _)| k)
        } else {
            "square"
        });
        p["construction"] = json!(self.construction);
        p["landing_stringers"] = json!(self.landing_stringers && matches!(self.construction, "stringers" | "carriage"));
        p["finish_profile"] = json!(self.finish_profile);
        p["arrow"] = json!(self.arrow);
        p["walk_start"] = json!(self.walk_start);
        p["cut_mark"] = json!(self.cut_mark);
        p["walk_rounded"] = json!(self.walk_rounded);
        // A monolithic type needs its slab; treads standing free, their thickness.
        Ok(())
    }

    /// The `type` command's parameters for the sizes that apply to what carries it: those that don't are left as they
    /// are, so switching back finds them again.
    pub(crate) fn used(&self, key: &str) -> bool {
        let c = self.construction;
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

    /// The whole of the type table for a stair type, in two parts: the Model (what carries it, its sizes, its finishes, its
    /// rules) and the Graphics (how its plans draw it). Only the sizes that apply to what carries it are shown.
    pub(crate) fn show(&mut self, tb: &mut Table, m: &Model, fields: &mut [crate::properties::TypeField]) {
        let hint = m.units.name();
        let mut size = |tb: &mut Table, key: &str, label: &str, about: &str| {
            if let Some((.., typed)) = fields.iter_mut().find(|f| f.1 == key) {
                tb.text_about(t(label), typed, hint, t(about));
            }
        };
        tb.section(msg!("Model"), msg!("What the stair is built of, its sizes and the rules it is checked against."));
        tb.group(msg!("Construction"));
        choose_construction(tb, &mut self.construction);
        let about = CONSTRUCTIONS.iter().find(|(k, ..)| *k == self.construction).map_or("", |(.., a)| a);
        tb.note(t(about));
        match self.construction {
            "monolithic" => {
                size(tb, "waist", msg!("Slab Thickness"), msg!("How thick the slab under the steps is, measured square to its slope."));
                let profiles: Vec<String> = PROFILES.iter().map(|(_, l)| t(l).to_string()).collect();
                tb.pick(t("Step Profile"), &mut self.profile, &profiles);
                if self.slanted() {
                    size(
                        tb,
                        "nosing",
                        msg!("Riser Slant"),
                        msg!("How far each riser leans back from the front edge of the tread above it."),
                    );
                }
            }
            c => {
                size(tb, "tread_thickness", msg!("Tread Thickness"), msg!("How thick each tread is."));
                size(tb, "riser_thickness", msg!("Riser Thickness"), msg!("How thick each riser board is. Leave it at 0 for open risers."));
                size(tb, "nosing", msg!("Nosing"), msg!("How far each tread reaches out over the riser below it."));
                if c == "stringers" {
                    size(tb, "stringer_width", msg!("Stringer Width"), msg!("How thick each side stringer is."));
                    size(
                        tb,
                        "stringer_depth",
                        msg!("Stringer Depth"),
                        msg!("How deep each side stringer is, measured square to the slope."),
                    );
                }
                if matches!(c, "stringers" | "carriage") {
                    let optional = if c == "stringers" { msg!("Centre Stringer Width (Optional)") } else { msg!("Centre Stringer Width") };
                    size(tb, "carriage_width", optional, msg!("How wide the stringer down the middle is. Leave it at 0 for none."));
                    size(tb, "carriage_depth", msg!("Centre Stringer Depth"), msg!("How deep the stringer down the middle is."));
                    tb.check_about(
                        t("Stringers Round Landings"),
                        &mut self.landing_stringers,
                        t("Carry the stringers on round the free edges of each landing, so a landing is framed like its flights."),
                    );
                }
            }
        }
        size(tb, "post", msg!("Centre Post Diameter"), msg!("For spiral stairs: the post at the centre. Leave it at 0 for none."));
        tb.group(msg!("Steps"));
        tb.check_about(
            t("Begin with Riser"),
            &mut self.begin_riser,
            t("Start with a riser at the bottom. Off, the stair starts with a tread level with the floor."),
        );
        tb.check_about(
            t("End with Riser"),
            &mut self.end_riser,
            t("Finish with a riser onto the floor above. Off, the last tread is level with that floor."),
        );
        tb.group(msg!("Finishes"));
        tb.note(t("A finish laid over the treads and risers, such as stone or timber. Risers are measured from finish to finish."));
        size(tb, "tread_cover", msg!("Tread Finish Thickness"), msg!("How thick the finish on each tread and landing is. 0 for none."));
        size(tb, "riser_cover", msg!("Riser Finish Thickness"), msg!("How thick the finish on the face of each riser is. 0 for none."));
        size(tb, "cover_nosing", msg!("Finish Nosing"), msg!("How far the tread finish stands out past the riser finish below it."));
        if self.construction != "stringers" {
            size(tb, "cover_side", msg!("Finish Side Overhang"), msg!("How far the tread finish stands out past an open side."));
        }
        choose(tb, t("Finish Edge Profile"), &mut self.finish_profile, &FINISH_PROFILES);
        let materials: Vec<String> = std::iter::once(String::new()).chain(m.materials.keys().cloned()).collect();
        tb.pick(t("Finish Material"), &mut self.cover_material, &materials);
        tb.group(msg!("Materials"));
        tb.pick(t("Structure Material"), &mut self.material, &materials);
        tb.group(msg!("Rules and Standards"));
        let units = m.units;
        let code = t(&m.building_code().name).to_string();
        tb.full(|ui| rules(ui, &mut self.rules, units, &code));

        tb.section(msg!("Graphics"), msg!("How plans draw stairs of this type. A view can override this in Visibility/Graphics."));
        tb.group(msg!("Floor Plan Display"));
        let ways: Vec<String> = WAYS.iter().map(|(_, l)| t(l).to_string()).collect();
        for (i, label) in ["On the Level Below", "On Its Own Level", "On the Level Above"].into_iter().enumerate() {
            let label = t(label);
            tb.row(label, |ui, l| {
                if let Some(picked) = way_pick(ui, label, l, &self.plan[i], &ways) {
                    self.plan[i] = picked;
                }
            });
        }
        tb.group(msg!("Lines and Fill"));
        let lines: Vec<String> = m.line_styles.keys().cloned().collect();
        tb.pick(t("Visible Line Style"), &mut self.line_visible, &lines);
        tb.pick(t("Hidden Line Style"), &mut self.line_hidden, &lines);
        let fills: Vec<String> = std::iter::once(String::new()).chain(m.fill_patterns.keys().cloned()).collect();
        tb.pick(t("Fill of the Visible Part"), &mut self.fill, &fills);
        tb.group(msg!("Symbols"));
        choose(tb, t("Walk Line Arrow"), &mut self.arrow, &ARROWS);
        choose(tb, t("Walk Line Start"), &mut self.walk_start, &WALK_STARTS);
        tb.check_about(
            t("Rounded Walk Line"),
            &mut self.walk_rounded,
            t("Round the walk line's corners where it turns on a landing, instead of turning them square."),
        );
        choose(tb, t("Cut Mark"), &mut self.cut_mark, &CUT_MARKS);
        tb.check_about(t("Tread Numbers"), &mut self.numbers, t("Number each tread in plan, from the bottom up."));
    }
}

/// The Construction row: a drop-down of what carries the treads, each with a little side view of it.
fn choose_construction(tb: &mut Table, key: &mut &'static str) {
    let label = t("Construction");
    tb.row(label, |ui, l| {
        construction_icon(ui, key);
        let now = CONSTRUCTIONS.iter().find(|(k, ..)| k == key).map_or("", |(_, l, _)| l);
        egui::ComboBox::from_id_salt("stair-construction")
            .selected_text(t(now))
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for (k, l, about) in CONSTRUCTIONS {
                    ui.horizontal(|ui| {
                        construction_icon(ui, k);
                        if ui.selectable_label(*key == k, t(l)).on_hover_text(t(about)).clicked() {
                            *key = k;
                        }
                    });
                }
            })
            .response
            .labelled_by(l);
    });
}

/// Three steps seen from the side, built as `key` says: on a slab, between stringers, on a centre stringer, or fixed in a
/// wall.
fn construction_icon(ui: &mut Ui, key: &str) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(28.0, 18.0), egui::Sense::hover());
    let p = ui.painter();
    let ink = egui::Stroke::new(1.2, pal().text);
    let (x0, y0) = (r.left() + 2.0, r.bottom() - 2.0);
    let (w, h) = (7.0, 4.5);
    let step = |i: f32| egui::pos2(x0 + i * w, y0 - i * h);
    let mut outline = vec![egui::pos2(x0, y0)];
    for i in 0..3 {
        let a = step(i as f32);
        outline.push(egui::pos2(a.x, a.y - h));
        outline.push(egui::pos2(a.x + w, a.y - h));
    }
    match key {
        "monolithic" => {
            outline.push(egui::pos2(x0 + 3.0 * w, y0 - 3.0 * h + 4.0));
            outline.push(egui::pos2(x0 + 4.0, y0));
            outline.push(egui::pos2(x0, y0));
            p.add(egui::Shape::line(outline, ink));
        }
        "stringers" | "carriage" => {
            for i in 0..3 {
                let a = step(i as f32);
                p.line_segment([egui::pos2(a.x, a.y - h), egui::pos2(a.x + w, a.y - h)], egui::Stroke::new(2.0, pal().text));
            }
            let stroke = if key == "stringers" { egui::Stroke::new(1.2, pal().text) } else { egui::Stroke::new(2.4, pal().accent) };
            p.line_segment([egui::pos2(x0 - 1.0, y0 + 1.0), egui::pos2(x0 + 3.0 * w, y0 - 3.0 * h + 1.0)], stroke);
            if key == "stringers" {
                p.line_segment([egui::pos2(x0 + 3.0, y0 + 1.0), egui::pos2(x0 + 3.0 * w + 3.0, y0 - 3.0 * h + 1.0)], stroke);
            }
        }
        _ => {
            for i in 0..3 {
                let a = step(i as f32);
                p.line_segment([egui::pos2(a.x, a.y - h), egui::pos2(a.x + w, a.y - h)], egui::Stroke::new(2.0, pal().text));
            }
            p.line_segment(
                [egui::pos2(r.right() - 1.0, r.top()), egui::pos2(r.right() - 1.0, r.bottom())],
                egui::Stroke::new(2.0, pal().muted),
            );
        }
    }
}

/// The line style `name`, where the project has it; else its style named `usual` (in its language, else in English).
fn usual_style(m: &Model, name: String, usual: &str) -> String {
    if m.line_styles.contains_key(&name) {
        return name;
    }
    [m.language.word(usual), usual].into_iter().find(|n| m.line_styles.contains_key(*n)).map_or(name, str::to_string)
}

/// A drop-down of the ways a stair shows in plan, each with a little picture of a flight drawn that way, as Archicad's
/// list has: the one picked, if it changed.
fn way_pick(ui: &mut Ui, label: &str, named: egui::Id, current: &str, ways: &[String]) -> Option<String> {
    let mut picked = None;
    let look_of = |shown: &str| ways.iter().position(|w| w == shown).and_then(|i| PlanShow::ALL.get(i)).and_then(|w| w.look());
    ui.horizontal(|ui| {
        way_icon(ui, look_of(current));
        let c = egui::ComboBox::from_id_salt(("stair-plan-way", label))
            .selected_text(current)
            .width(ui.available_width())
            .truncate()
            .show_ui(ui, |ui| {
                for (w, way) in ways.iter().zip(PlanShow::ALL) {
                    ui.horizontal(|ui| {
                        way_icon(ui, way.look());
                        if ui.selectable_label(w == current, w).clicked() && w != current {
                            picked = Some(w.clone());
                        }
                    });
                }
            });
        c.response.labelled_by(named);
    });
    picked
}

/// A flight going right in a little box: what is under the break mark (the left half) and over it (the right) in full
/// or dashed lines or left out, and the mark across the middle where there is one. An empty box for not shown.
fn way_icon(ui: &mut Ui, look: Option<(PlanPart, PlanPart, bool)>) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(52.0, 20.0), egui::Sense::hover());
    let p = ui.painter();
    let ink = egui::Stroke::new(1.0, pal().text);
    p.rect_stroke(r, 2.0, egui::Stroke::new(1.0, pal().muted), egui::StrokeKind::Inside);
    let Some((below, above, mark)) = look else { return };
    let (top, bottom) = (r.top() + 4.0, r.bottom() - 4.0);
    let (left, right, mid) = (r.left() + 4.0, r.right() - 4.0, r.center().x);
    let line = |a: egui::Pos2, b: egui::Pos2, part: PlanPart| match part {
        PlanPart::Full => {
            p.line_segment([a, b], ink);
        }
        PlanPart::Hidden => {
            p.extend(egui::Shape::dashed_line(&[a, b], ink, 2.0, 2.0));
        }
        PlanPart::Out => {}
    };
    for (from, to, part) in [(left, mid, below), (mid, right, above)] {
        line(egui::pos2(from, top), egui::pos2(to, top), part);
        line(egui::pos2(from, bottom), egui::pos2(to, bottom), part);
        let mut x = from;
        while x < to - 0.5 {
            line(egui::pos2(x, top), egui::pos2(x, bottom), part);
            x += 5.0;
        }
    }
    if mark {
        let (a, b) = (egui::pos2(mid - 3.0, bottom + 1.0), egui::pos2(mid + 3.0, top - 1.0));
        let m = a + (b - a) * 0.5;
        let jog = egui::vec2(2.0, 0.0);
        p.add(egui::Shape::line(vec![a, m - (b - a) * 0.06 + jog, m + (b - a) * 0.06 - jog, b], egui::Stroke::new(1.4, pal().text)));
    }
}

/// The rules, a row each: its name, then its least and its most, each with a tick that turns it on. One that is off is
/// the building code's, greyed.
fn rules(ui: &mut Ui, rows: &mut [Rule], units: Units, code: &str) {
    ui.label(
        RichText::new(tf("A rule ticked is this type's own. One not ticked is the building code's ({}), where it has one.", &[code]))
            .color(pal().muted)
            .small(),
    );
    Grid::new("stair-type-rules").num_columns(5).spacing([8.0, 4.0]).show(ui, |ui| {
        for rule in rows.iter_mut() {
            ui.label(t(rule.label));
            for side in 0..2 {
                if rule.keys[side].is_none() {
                    ui.label("");
                    ui.label("");
                    continue;
                }
                let word = if side == 0 { t("min.") } else { t("max.") };
                let tick = ui.checkbox(&mut rule.on[side], word);
                let name = format!("{} {}", t(rule.label), word);
                tick.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, rule.on[side], name.clone()));
                let hint = match rule.holds {
                    Holds::Length => units.name(),
                    Holds::Degrees => "°",
                    Holds::Number => "",
                };
                ui.add_enabled(rule.on[side], TextEdit::singleline(&mut rule.typed[side]).desired_width(64.0).hint_text(hint));
            }
            ui.end_row();
        }
    });
}
