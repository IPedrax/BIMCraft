//! The Options Bar, as data and pure functions: what a tool in hand lets a person set before the click (a wall's height and location
//! line, a beam's justification and offset, a ceiling's height, a roof's slope, a stair's width, Chain), the type the Type Selector picked
//! for it, and how those settings join the params the tool's machine (`tooling.rs`) made. So a tool no longer runs on the command's
//! defaults or the first type there is. It is egui's bar (`crates/ui-egui/src/lib.rs`, `options`; `canvas.rs`, `wall_params`, `run_params`),
//! with its values kept here and its fields typed in the project's units; nothing here meets a window, and the model changes only
//! through the commands these params go to.

use std::collections::BTreeMap;

use bimcraft_core::ids::Id;
use bimcraft_core::model::{CEILING_HEIGHT, DEFAULT_HEIGHT, Justify, Location, MAX_COORD, Model, STAIR_WIDTH, StairLine};
use bimcraft_core::units::Units;
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::props::kind::number;
use crate::tooling::{self, Kind};

/// What a typed field holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// A length in the project's units.
    Length,
    /// A number of degrees.
    Degrees,
}

/// A list of fixed choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// A wall's location line.
    Location,
    /// Which line of a run's section its height is.
    Justify,
    /// The line of a stair its clicks follow.
    StairLine,
}

/// A check box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flag {
    /// Each click goes on from the one before.
    Chain,
    /// A roof's ends stand upright.
    Gable,
}

/// One control of the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// Height: up to a level above, or Unconnected at a typed height. The key is the tool's (`wall`, `column`, `stair`).
    Height(&'static str),
    /// A typed field: its key, its label.
    Field(&'static str, &'static str, Unit),
    List(Choice, &'static str),
    Check(Flag, &'static str),
}

const WALL: &[Control] = &[Control::Height("wall"), Control::List(Choice::Location, "Location Line"), Control::Check(Flag::Chain, "Chain")];
const RUN: &[Control] = &[
    Control::List(Choice::Justify, "Justification"),
    Control::Field("run.offset", "Offset", Unit::Length),
    Control::Check(Flag::Chain, "Chain"),
];

/// What the Options Bar of tool `id` (of `tools::TOOLS`) holds; none for a tool with nothing to set.
pub fn controls(id: &str) -> &'static [Control] {
    match id {
        "wall" => WALL,
        "curtain_wall" => &[Control::Height("wall"), Control::Check(Flag::Chain, "Chain")],
        "column" => &[Control::Height("column")],
        "structural_column" => &[Control::Height("structural_column")],
        "beam" | "pipe" | "conduit" | "cable_tray" => RUN,
        "strip_footing" => &[Control::Field("strip_footing.offset", "Top Offset", Unit::Length), Control::Check(Flag::Chain, "Chain")],
        "ceiling" => &[Control::Field("ceiling.height", "Height Offset", Unit::Length)],
        "roof" => &[
            Control::Field("roof.slope", "Slope", Unit::Degrees),
            Control::Field("roof.overhang", "Overhang", Unit::Length),
            Control::Check(Flag::Gable, "Gable"),
        ],
        "stair" => &[
            Control::Height("stair"),
            Control::List(Choice::StairLine, "Location Line"),
            Control::Field("stair.offset", "Offset", Unit::Length),
            Control::Field("stair.width", "Width", Unit::Length),
        ],
        _ => &[],
    }
}

/// The category of the types tool `id` places, which the Type Selector lists while it is in hand; none for a tool with no type.
pub fn category_of(id: &str) -> Option<&'static str> {
    Some(match id {
        "wall" => "wall",
        "door" => "door",
        "window" => "window",
        "column" => "column",
        "structural_column" => "structuralcolumn",
        "footing" => "footing",
        "strip_footing" => "stripfooting",
        "floor" => "floor",
        "roof" => "roof",
        "ceiling" => "ceiling",
        "stair" => "stair",
        "ramp" => "ramp",
        "railing" => "railing",
        "room" => "room",
        "beam" => "beam",
        "pipe" => "pipe",
        "conduit" => "conduit",
        "cable_tray" => "cabletray",
        "component" | "plumbing_fixture" | "drain" | "outlet" | "switch" | "lighting" => "component",
        _ => return None,
    })
}

/// The type the Type Selector picked for a category: its name, and the family of a type of a family.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypePick {
    pub name: String,
    pub family: Option<String>,
}

/// What the Options Bar has set, and the types picked: everything a tool reads besides its points.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// The types picked, by category.
    pub picks: BTreeMap<&'static str, TypePick>,
    /// What was typed into the fields, by key; a field not typed in has its default (`default_text`).
    pub texts: BTreeMap<&'static str, String>,
    /// The top a tool's height goes up to, by the tool's key: none is the level above (or Unconnected where there is none),
    /// `Some(None)` is Unconnected.
    pub tops: BTreeMap<&'static str, Option<Id>>,
    pub location: Location,
    pub justify: Justify,
    pub line: StairLine,
    pub chain: bool,
    pub gable: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            picks: BTreeMap::new(),
            texts: BTreeMap::new(),
            tops: BTreeMap::new(),
            location: Location::Centre,
            justify: Justify::Top,
            line: StairLine::Centre,
            chain: true,
            gable: false,
        }
    }
}

/// What a field holds until something is typed in it, in the project's units.
pub fn default_text(key: &str, units: Units) -> String {
    match key {
        "wall.height" | "column.height" | "structural_column.height" => units.length(3000.0),
        "stair.height" => units.length(DEFAULT_HEIGHT),
        "stair.width" => units.length(STAIR_WIDTH),
        "ceiling.height" => units.length(CEILING_HEIGHT),
        "roof.slope" => "30".to_string(),
        "roof.overhang" => units.length(500.0),
        _ => units.length(0.0),
    }
}

/// Each way a tool's height can go up: the levels above `level`, nearest first, then Unconnected (none).
fn tops(m: &Model, level: Id) -> Vec<Option<Id>> {
    let z = m.elevation(level);
    let mut above: Vec<(Id, f64)> = m.levels().into_iter().filter(|(_, l)| l.elevation > z).map(|(id, l)| (id, l.elevation)).collect();
    above.sort_by(|a, b| a.1.total_cmp(&b.1));
    above.into_iter().map(|(id, _)| Some(id)).chain([None]).collect()
}

/// The level above `level`, which a height goes up to unless another is picked.
fn level_above(m: &Model, level: Id) -> Option<Id> {
    m.level_above(level).and_then(|g| m.elements.id(g))
}

/// A length typed in `units`, in mm, finite and no farther than the model reaches; or `what` is not one.
fn length(text: &str, units: Units, what: &str) -> Result<f64, String> {
    match units.parse(text) {
        Some(x) if x.abs() <= MAX_COORD => Ok(x),
        _ => Err(tf("`{}` is not a {}", &[text.trim(), what])),
    }
}

impl Settings {
    /// What field `key` holds: what was typed, else its default.
    pub fn text(&self, key: &str, units: Units) -> String {
        self.texts.get(key).cloned().unwrap_or_else(|| default_text(key, units))
    }

    /// The top tool `key`'s height goes up to: the one picked if the project still has it, else the level above `level`; none is Unconnected.
    pub fn top(&self, key: &str, m: &Model, level: Id) -> Option<Id> {
        match self.tops.get(key) {
            Some(Some(id)) if m.level(*id).is_some() => Some(*id),
            Some(None) => None,
            _ => level_above(m, level),
        }
    }

    /// The pick for `category`, if the project still has that type.
    pub fn pick(&self, m: &Model, category: &str) -> Option<&TypePick> {
        self.picks.get(category).filter(|p| match &p.family {
            Some(f) => m.families.get(f).is_some_and(|x| x.types.contains_key(&p.name)),
            None => m.type_names(category).contains(&p.name.as_str()),
        })
    }

    /// The params a height of tool `key` makes: up to a level, or unconnected at the height typed.
    fn height(&self, key: &str, m: &Model, level: Id) -> Result<Value, String> {
        match self.top(key, m, level) {
            Some(id) => Ok(json!({"top_level": id})),
            None => length(&self.text(&format!("{key}.height"), m.units), m.units, "height").map(|h| json!({"height": h})),
        }
    }

    /// Joins what tool `tool` has set to `params`, the ones its machine made: the type (unless the machine named one), the heights, the
    /// lines. Or what is wrong with a field, which the click is refused for.
    pub fn feed(&self, tool: &str, m: &Model, level: Id, params: &mut Value) -> Result<(), String> {
        let units = m.units;
        let Some(object) = params.as_object_mut() else { return Ok(()) };
        // The type picked for the tool's category; a family's type of a family tool (of one of its kinds) or a door or a window.
        let kinds = match tooling::spec_of(tool).map(|s| s.kind) {
            Some(Kind::Family { kinds }) => Some(kinds),
            _ => None,
        };
        let of_kind = |p: &TypePick| {
            kinds.is_none_or(|k| k.is_empty() || p.family.as_ref().and_then(|f| m.families.get(f)).is_some_and(|x| k.contains(&x.category)))
        };
        if let Some(pick) = category_of(tool).and_then(|c| self.pick(m, c)).filter(|p| of_kind(p)) {
            let family_tool = kinds.is_some();
            if family_tool || pick.family.is_some() || !object.contains_key("type") {
                object.insert("type".into(), json!(pick.name));
            }
            if let Some(f) = &pick.family {
                object.insert("family".into(), json!(f));
            }
        }
        let mut put = |more: Value| {
            if let Value::Object(more) = more {
                object.extend(more);
            }
        };
        match tool {
            "wall" | "curtain_wall" => {
                put(self.height("wall", m, level)?);
                if tool == "wall" {
                    put(json!({"location": self.location.name()}));
                }
            }
            "column" | "structural_column" => put(self.height(tool, m, level)?),
            "beam" | "pipe" | "conduit" | "cable_tray" => {
                let offset = length(&self.text("run.offset", units), units, "height")?;
                put(json!({"justify": self.justify, "offset": offset}));
            }
            "strip_footing" => {
                let offset = length(&self.text("strip_footing.offset", units), units, "height")?;
                put(json!({"offset": offset}));
            }
            "ceiling" => {
                let offset = length(&self.text("ceiling.height", units), units, "height")?;
                put(json!({"offset": offset}));
            }
            "roof" => {
                let slope = number(&self.text("roof.slope", units)).filter(|s| (0.0..90.0).contains(s));
                let slope = slope.ok_or_else(|| tf("`{}` is not a slope in degrees", &[self.text("roof.slope", units).trim()]))?;
                let overhang = length(&self.text("roof.overhang", units), units, "length")?;
                put(json!({"slope": slope, "overhang": overhang, "gable": self.gable}));
            }
            "stair" => {
                let width = length(&self.text("stair.width", units), units, "width")?;
                let offset = length(&self.text("stair.offset", units), units, "length")?;
                put(self.height("stair", m, level)?);
                put(json!({"width": width, "line": self.line, "offset": offset}));
            }
            _ => {}
        }
        Ok(())
    }

    /// Whether a click of tool `tool` goes on from the one before: not with Chain off, for the tools that have it.
    pub fn chains(&self, tool: &str) -> bool {
        self.chain || !controls(tool).iter().any(|c| matches!(c, Control::Check(Flag::Chain, _)))
    }
}

/// What a control of the bar shows and takes.
#[derive(Clone, Debug, PartialEq)]
pub enum Editor {
    /// A typed field; `hint` is its unit.
    Field {
        hint: String,
    },
    /// A list: what is in force and every option, by their labels.
    List {
        now: String,
        options: Vec<String>,
    },
    Check(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// What a handler is given: `wall.top`, `wall.location`, `run.offset`, `chain`.
    pub key: &'static str,
    pub label: String,
    pub editor: Editor,
}

const JUSTIFY: [(Justify, &str); 3] = [(Justify::Top, "Top"), (Justify::Centre, "Centre"), (Justify::Bottom, "Bottom")];
const LINES: [(StairLine, &str); 3] =
    [(StairLine::Left, "Run: Left"), (StairLine::Centre, "Run: Center"), (StairLine::Right, "Run: Right")];

fn list(now: &str, options: Vec<String>) -> Editor {
    Editor::List { now: now.to_string(), options }
}

/// The Options Bar for tool `tool` on `level`: each control as what it shows. A height that is Unconnected is a list and a field.
pub fn items(tool: &str, m: &Model, level: Id, s: &Settings) -> Vec<Item> {
    let mut out = Vec::new();
    let hint = |u: Unit| match u {
        Unit::Length => m.units.name().to_string(),
        Unit::Degrees => "°".to_string(),
    };
    for control in controls(tool) {
        match *control {
            Control::Height(key) => {
                let ways = tops(m, level);
                let name = |w: &Option<Id>| w.and_then(|id| m.level(id)).map_or_else(|| t("Unconnected").to_string(), |l| l.name.clone());
                let now = s.top(key, m, level);
                let options: Vec<String> = ways.iter().map(name).collect();
                out.push(Item { key: height_key(key), label: t("Height").to_string(), editor: list(&name(&now), options) });
                if now.is_none() {
                    out.push(Item { key: field_key(key), label: String::new(), editor: Editor::Field { hint: hint(Unit::Length) } });
                }
            }
            Control::Field(key, label, unit) => {
                out.push(Item { key, label: t(label).to_string(), editor: Editor::Field { hint: hint(unit) } });
            }
            Control::List(which, label) => {
                let (now, key, options): (String, &'static str, Vec<String>) = match which {
                    Choice::Location => (
                        t(s.location.label()).to_string(),
                        "wall.location",
                        Location::ALL.iter().map(|l| t(l.label()).to_string()).collect(),
                    ),
                    Choice::Justify => (
                        JUSTIFY.iter().find(|(j, _)| *j == s.justify).map_or("", |(_, n)| t(n)).to_string(),
                        "run.justify",
                        JUSTIFY.iter().map(|(_, n)| t(n).to_string()).collect(),
                    ),
                    Choice::StairLine => (
                        LINES.iter().find(|(l, _)| *l == s.line).map_or("", |(_, n)| t(n)).to_string(),
                        "stair.line",
                        LINES.iter().map(|(_, n)| t(n).to_string()).collect(),
                    ),
                };
                out.push(Item { key, label: t(label).to_string(), editor: list(&now, options) });
            }
            Control::Check(Flag::Chain, label) => {
                out.push(Item { key: "chain", label: t(label).to_string(), editor: Editor::Check(s.chain) })
            }
            Control::Check(Flag::Gable, label) => {
                out.push(Item { key: "gable", label: t(label).to_string(), editor: Editor::Check(s.gable) })
            }
        }
    }
    out
}

/// The key of a tool's height list, and of its field.
fn height_key(tool: &str) -> &'static str {
    match tool {
        "column" => "column.top",
        "structural_column" => "structural_column.top",
        "stair" => "stair.top",
        _ => "wall.top",
    }
}

fn field_key(tool: &str) -> &'static str {
    match tool {
        "column" => "column.height",
        "structural_column" => "structural_column.height",
        "stair" => "stair.height",
        _ => "wall.height",
    }
}

/// The typed fields of tool `tool`'s bar, by key: what a text edit is made for. Unconnected heights' fields are among them whether or not
/// they show, so typing one and changing the top does not lose it.
pub fn field_keys(tool: &str) -> Vec<&'static str> {
    let mut keys = Vec::new();
    for control in controls(tool) {
        match *control {
            Control::Height(key) => keys.push(field_key(key)),
            Control::Field(key, ..) => keys.push(key),
            _ => {}
        }
    }
    keys
}

impl Settings {
    /// Option `i` of the list `key` was picked on `level`'s bar. False for a list or an option there is not.
    pub fn choose(&mut self, m: &Model, level: Id, key: &str, i: usize) -> bool {
        match key {
            "wall.location" => match Location::ALL.get(i) {
                Some(l) => {
                    self.location = *l;
                    true
                }
                None => false,
            },
            "run.justify" => match JUSTIFY.get(i) {
                Some((j, _)) => {
                    self.justify = *j;
                    true
                }
                None => false,
            },
            "stair.line" => match LINES.get(i) {
                Some((l, _)) => {
                    self.line = *l;
                    true
                }
                None => false,
            },
            "wall.top" => self.set_top("wall", m, level, i),
            "column.top" => self.set_top("column", m, level, i),
            "structural_column.top" => self.set_top("structural_column", m, level, i),
            "stair.top" => self.set_top("stair", m, level, i),
            _ => false,
        }
    }

    /// Height way `i` of tool `tool`.
    fn set_top(&mut self, tool: &'static str, m: &Model, level: Id, i: usize) -> bool {
        match tops(m, level).get(i) {
            Some(way) => {
                self.tops.insert(tool, *way);
                true
            }
            None => false,
        }
    }

    /// A check box was set.
    pub fn check(&mut self, key: &str, on: bool) -> bool {
        match key {
            "chain" => self.chain = on,
            "gable" => self.gable = on,
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests;
