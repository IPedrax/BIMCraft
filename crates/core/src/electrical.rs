//! The electrical settings of a project, as Revit's Electrical Settings keep them: the voltages (each with the band a device may
//! read within), the distribution systems (phases and wires, and the voltages each supplies) and the wire types (a conductor's
//! material, insulation and working temperature, and the ampacity of each section). They are project data that circuits, panels
//! and connectors will refer to by name; nothing here draws, and none of it goes to IFC. A distribution board's own data, which
//! names them, is [`Panel`].
//!
//! A new project starts with the set in `data/electrical.json`, which is data and not code: voltages and systems of Europe,
//! Brazil and North America, and copper wires of the NBR 5410 tables (PVC 70 °C, XLPE or EPR 90 °C) and of the NEC (THHN and THW),
//! the American ones sized in AWG and kcmil and kept in mm². Every section is in mm², whatever the project's units. The ampacities are the
//! contributor's, from the tables each wire type names in its `note`, and are unchecked; a design is checked against the standard in
//! force.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::family::Category;
use crate::model::{Component, Element, Guid, Id, MAX_TEXT, Model};
use crate::units::Units;

/// The voltages, distribution systems and wire types a project holds, each.
pub const MAX_VOLTAGES: usize = 64;
pub const MAX_SYSTEMS: usize = 64;
pub const MAX_WIRE_TYPES: usize = 64;
/// The rows of a wire type's table.
pub const MAX_SIZES: usize = 64;
/// The most volts, mm², amperes and degrees a value may be.
pub const MAX_VOLTS: f64 = 100_000.0;
pub const MAX_SECTION: f64 = 10_000.0;
pub const MAX_AMPACITY: f64 = 100_000.0;
pub const MAX_TEMPERATURE: f64 = 250.0;
/// What a conductor may be made of.
pub const MATERIALS: [&str; 2] = ["copper", "aluminium"];
/// How a system is earthed, as IEC 60364-1 names the arrangements.
pub const EARTHINGS: [&str; 5] = ["TN-S", "TN-C", "TN-C-S", "TT", "IT"];
/// The most slots a board has, and the most amperes its mains carry.
pub const MAX_SLOTS: u32 = 1_000;
pub const MAX_MAINS: f64 = 100_000.0;
/// How far the voltage between lines of a three-phase system may be from root three times the one to neutral.
const THREE_PHASE_TOLERANCE: f64 = 0.03;

/// A voltage by name, with the band a device may read within, volts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Voltage {
    pub nominal: f64,
    pub min: f64,
    pub max: f64,
}

impl Voltage {
    /// A voltage of `nominal` volts, which may read 10 per cent under and over it.
    pub fn around(nominal: f64) -> Voltage {
        let round = |x: f64| (x * 10.0).round() / 10.0;
        Voltage { nominal, min: round(nominal * 0.9), max: round(nominal * 1.1) }
    }

    /// Whether it is one the project may hold, by `name`.
    pub fn check(&self, name: &str) -> Result<(), String> {
        let who = format!("voltage `{}`", shown(name));
        title(&who, name)?;
        let volts = |x: f64| x.is_finite() && x > 0.0 && x <= MAX_VOLTS;
        if !volts(self.nominal) {
            return Err(format!("{who}: `nominal` is volts above 0 and up to {MAX_VOLTS}, not {}", self.nominal));
        }
        if !volts(self.min) || self.min > self.nominal {
            return Err(format!("{who}: `min` is volts above 0 and not above the nominal {} V, not {}", self.nominal, self.min));
        }
        if !volts(self.max) || self.max < self.nominal {
            return Err(format!("{who}: `max` is volts up to {MAX_VOLTS} and not under the nominal {} V, not {}", self.nominal, self.max));
        }
        Ok(())
    }
}

/// A distribution system by name: how many phases and wires it has, the voltages it supplies, by name, and how it is earthed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DistributionSystem {
    /// 1, 2 or 3 line conductors.
    pub phases: u8,
    /// The line conductors and the neutral, not the protective one.
    pub wires: u8,
    /// The voltage between two lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l_l: Option<String>,
    /// The voltage between a line and the neutral.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l_n: Option<String>,
    /// The voltage between a line and earth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l_g: Option<String>,
    /// How it is earthed, one of [`EARTHINGS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground: Option<String>,
}

impl DistributionSystem {
    /// The voltages it names, line to line first.
    pub fn voltage_names(&self) -> impl Iterator<Item = &String> {
        [&self.l_l, &self.l_n, &self.l_g].into_iter().flatten()
    }

    pub(crate) fn voltages_mut(&mut self) -> impl Iterator<Item = &mut String> {
        [&mut self.l_l, &mut self.l_n, &mut self.l_g].into_iter().flatten()
    }

    /// Whether it is one the project may hold, by `name`, with the voltages the project has.
    pub fn check(&self, name: &str, voltages: &BTreeMap<String, Voltage>) -> Result<(), String> {
        let who = format!("distribution system `{}`", shown(name));
        title(&who, name)?;
        if !(1..=3).contains(&self.phases) {
            return Err(format!("{who}: `phases` is 1, 2 or 3, not {}", self.phases));
        }
        let (least, most) = (self.phases.max(2), self.phases + 1);
        if self.wires < least || self.wires > most {
            return Err(format!(
                "{who}: `wires` counts the line conductors and the neutral, not the protective one: {least} to {most} for {} phase{}, not {}",
                self.phases,
                if self.phases == 1 { "" } else { "s" },
                self.wires
            ));
        }
        if self.voltage_names().next().is_none() {
            return Err(format!("{who}: it supplies no voltage; give `l_l`, `l_n` or `l_g`"));
        }
        if self.phases >= 2 && self.l_l.is_none() {
            return Err(format!("{who}: `l_l`, the voltage between lines, is required for {} phases", self.phases));
        }
        for (field, v) in [("l_l", &self.l_l), ("l_n", &self.l_n), ("l_g", &self.l_g)] {
            if let Some(v) = v
                && !voltages.contains_key(v)
            {
                let there: Vec<&str> = voltages.keys().map(String::as_str).collect();
                return Err(format!(
                    "{who}: `{field}` names voltage `{}`, which the project doesn't have (there are: {})",
                    shown(v),
                    there.join(", ")
                ));
            }
        }
        if let (3, Some(ll), Some(ln)) = (self.phases, &self.l_l, &self.l_n)
            && let (Some(a), Some(b)) = (voltages.get(ll), voltages.get(ln))
            && (a.nominal / b.nominal - 3f64.sqrt()).abs() > 3f64.sqrt() * THREE_PHASE_TOLERANCE
        {
            return Err(format!(
                "{who}: with 3 phases `l_l` `{ll}` ({} V) should be about 1.73 times `l_n` `{ln}` ({} V)",
                a.nominal, b.nominal
            ));
        }
        if let Some(g) = &self.ground
            && !EARTHINGS.contains(&g.as_str())
        {
            return Err(format!("{who}: `ground` is the earthing, one of {}, not `{}`", EARTHINGS.join(", "), shown(g)));
        }
        Ok(())
    }
}

/// One row of a wire type's table: a section and the current it carries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireSize {
    /// mm², always: a size the standard gives in AWG or kcmil is converted.
    pub section: f64,
    /// Amperes.
    pub ampacity: f64,
    /// What its standard calls the size, if not its section: `12 AWG`, `250 kcmil`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// A wire type by name: a conductor of one material and insulation, working at a temperature, and its table of sections.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireType {
    /// One of [`MATERIALS`].
    pub material: String,
    /// The conductor's highest working temperature, °C.
    pub temperature: f64,
    pub insulation: String,
    /// Where the ampacities come from, and for which way of installing the wire.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// The sections it comes in, smallest first.
    pub sizes: Vec<WireSize>,
}

impl WireType {
    /// Whether it is one the project may hold, by `name`.
    pub fn check(&self, name: &str) -> Result<(), String> {
        let who = format!("wire type `{}`", shown(name));
        title(&who, name)?;
        if !MATERIALS.contains(&self.material.as_str()) {
            return Err(format!("{who}: `material` is one of {}, not `{}`", MATERIALS.join(", "), shown(&self.material)));
        }
        if !(self.temperature.is_finite() && self.temperature > 0.0 && self.temperature <= MAX_TEMPERATURE) {
            return Err(format!("{who}: `temperature` is °C above 0 and up to {MAX_TEMPERATURE}, not {}", self.temperature));
        }
        if self.insulation.trim().is_empty() || self.insulation.chars().count() > MAX_TEXT {
            return Err(format!("{who}: `insulation` is 1 to {MAX_TEXT} characters"));
        }
        if self.note.chars().count() > MAX_TEXT {
            return Err(format!("{who}: `note` is up to {MAX_TEXT} characters"));
        }
        if self.sizes.is_empty() || self.sizes.len() > MAX_SIZES {
            return Err(format!("{who}: `sizes` holds 1 to {MAX_SIZES} rows, not {}", self.sizes.len()));
        }
        let mut before: Option<&WireSize> = None;
        for (i, s) in self.sizes.iter().enumerate() {
            let row = i + 1;
            if !(s.section.is_finite() && s.section > 0.0 && s.section <= MAX_SECTION) {
                return Err(format!("{who}: row {row}: `section` is mm² above 0 and up to {MAX_SECTION}, not {}", s.section));
            }
            if !(s.ampacity.is_finite() && s.ampacity > 0.0 && s.ampacity <= MAX_AMPACITY) {
                return Err(format!("{who}: row {row}: `ampacity` is amperes above 0 and up to {MAX_AMPACITY}, not {}", s.ampacity));
            }
            if s.label.as_ref().is_some_and(|l| l.chars().count() > MAX_TEXT) {
                return Err(format!("{who}: row {row}: `label` is up to {MAX_TEXT} characters"));
            }
            if let Some(b) = before {
                if s.section <= b.section {
                    return Err(format!(
                        "{who}: row {row}: `section` {} mm² must be larger than the row before's {} mm²",
                        s.section, b.section
                    ));
                }
                if s.ampacity < b.ampacity {
                    return Err(format!(
                        "{who}: row {row}: `ampacity` {} A is less than the row before's {} A: a larger section carries at least as much",
                        s.ampacity, b.ampacity
                    ));
                }
            }
            before = Some(s);
        }
        Ok(())
    }
}

/// A name for a message, cut short.
fn shown(s: &str) -> String {
    s.chars().take(40).collect()
}

/// The name of an entry is 1 to [`MAX_TEXT`] characters.
fn title(who: &str, name: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.chars().count() > MAX_TEXT {
        return Err(format!("{who}: its name is 1 to {MAX_TEXT} characters"));
    }
    Ok(())
}

/// What a new project starts with.
#[derive(Default, Deserialize)]
struct Starter {
    voltages: BTreeMap<String, Voltage>,
    distribution_systems: BTreeMap<String, DistributionSystem>,
    wire_types: BTreeMap<String, WireType>,
}

fn starter() -> &'static Starter {
    static STARTER: OnceLock<Starter> = OnceLock::new();
    STARTER.get_or_init(|| serde_json::from_str(include_str!("../data/electrical.json")).unwrap_or_default())
}

/// The voltages a new project starts with.
pub fn voltages() -> BTreeMap<String, Voltage> {
    starter().voltages.clone()
}

/// The distribution systems a new project starts with.
pub fn distribution_systems() -> BTreeMap<String, DistributionSystem> {
    starter().distribution_systems.clone()
}

/// The wire types a new project starts with.
pub fn wire_types() -> BTreeMap<String, WireType> {
    starter().wire_types.clone()
}

/// The distribution system a new panel starts in, for a project in `units`: Europe's in metric, North America's in feet and inches.
pub fn start_system(units: Units) -> &'static str {
    if units.imperial() { "277/480 V" } else { "230/400 V" }
}

/// The wire type a new circuit starts with, for a project in `units`.
pub fn start_wire(units: Units) -> &'static str {
    if units.imperial() { "Cu THHN 90 °C" } else { "Cu PVC 70 °C" }
}

/// The settings are within what the project may hold, and a system names voltages it has.
pub(crate) fn check(m: &Model) -> Result<(), String> {
    for (what, n, most) in [
        ("voltages", m.voltages.len(), MAX_VOLTAGES),
        ("distribution systems", m.distribution_systems.len(), MAX_SYSTEMS),
        ("wire types", m.wire_types.len(), MAX_WIRE_TYPES),
    ] {
        if n > most {
            return Err(format!("the project has {n} {what}; it holds up to {most}"));
        }
    }
    for (n, v) in &m.voltages {
        v.check(n)?;
    }
    for (n, d) in &m.distribution_systems {
        d.check(n, &m.voltages)?;
    }
    for (n, w) in &m.wire_types {
        w.check(n)?;
    }
    check_panels(m)
}

impl Model {
    /// The lowest and highest volts the voltage called `name` may read.
    pub fn voltage_range(&self, name: &str) -> Option<(f64, f64)> {
        self.voltages.get(name).map(|v| (v.min, v.max))
    }

    /// The nominal volts the distribution system called `name` supplies, smallest first, each once: the ones a circuit on it may
    /// have. None if the project has no such system.
    pub fn system_voltages(&self, name: &str) -> Vec<f64> {
        let Some(d) = self.distribution_systems.get(name) else { return Vec::new() };
        let mut volts: Vec<f64> = d.voltage_names().filter_map(|v| self.voltages.get(v)).map(|v| v.nominal).collect();
        volts.sort_by(f64::total_cmp);
        volts.dedup();
        volts
    }

    /// The boards `panel` supplies, by id, smallest first.
    pub fn panel_feeds(&self, panel: Id) -> Vec<Id> {
        let g = self.guid(panel);
        self.elements
            .iter()
            .filter(|(_, e)| matches!(e, Element::Component(Component { panel: Some(p), .. }) if p.supply_from == Some(g)))
            .map(|(id, _)| *id)
            .collect()
    }

    /// The boards that are in distribution system `name`, by id.
    pub fn panels_in(&self, name: &str) -> Vec<Id> {
        self.elements
            .iter()
            .filter(|(_, e)| matches!(e, Element::Component(Component { panel: Some(p), .. }) if p.distribution.as_deref() == Some(name)))
            .map(|(id, _)| *id)
            .collect()
    }

    /// The amperes a conductor of `section` mm² carries in the wire type called `wire`, if its table has that section.
    pub fn wire_ampacity(&self, wire: &str, section: f64) -> Option<f64> {
        let sizes = &self.wire_types.get(wire)?.sizes;
        sizes.iter().find(|s| (s.section - section).abs() <= 1e-9 * section.abs().max(1.0)).map(|s| s.ampacity)
    }
}

/// The electrical data of a distribution board, a component of the equipment category: the distribution system it is on, its
/// mains (the main breaker's rating, amperes), the single-pole breakers it takes (`Max #1 Pole Breakers`) and the board that
/// supplies it. Each is what Revit calls the parameter; none is set until said. A board's totals (its installed and demand load,
/// and the current of each phase) are sums over its circuits, so they are worked out where circuits are, and never saved.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Panel {
    /// A distribution system of the project, by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distribution: Option<String>,
    /// Amperes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mains: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots: Option<u32>,
    /// Another board, by GUID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supply_from: Option<Guid>,
}

impl Panel {
    /// Whether it says nothing: such a board is no panel.
    pub fn is_empty(&self) -> bool {
        *self == Panel::default()
    }
}

/// Whether `c` is electrical equipment: a component of a family of the equipment category.
fn equipment(m: &Model, c: &Component) -> bool {
    m.families.get(&c.family).is_some_and(|f| f.category == Category::Equipment)
}

/// The id and component of `id` when it is electrical equipment.
pub fn board(m: &Model, id: Id) -> Option<(Id, &Component)> {
    match m.elements.get(id) {
        Some(Element::Component(c)) if equipment(m, c) => Some((id, c)),
        _ => None,
    }
}

/// The boards from `id` up its supply chain (`id` first), when the chain comes back to `id`; none when it ends, or loops among
/// others without it (that loop is another board's to report).
pub fn supply_loop(m: &Model, id: Id) -> Option<Vec<Id>> {
    let mut chain = vec![id];
    let mut now = id;
    loop {
        let next = board(m, now).and_then(|(_, c)| c.panel.as_ref()).and_then(|p| p.supply_from).and_then(|g| m.elements.id(g))?;
        if next == id {
            return Some(chain);
        }
        if chain.contains(&next) {
            return None;
        }
        chain.push(next);
        now = next;
    }
}

/// What each board's data must be: only equipment has it, it names a distribution system of the project, its mains and slots
/// are in range, and the board that supplies it is equipment too, never itself and never one it supplies, however far down.
fn check_panels(m: &Model) -> Result<(), String> {
    for (&id, e) in m.elements.iter() {
        let Element::Component(c) = e else { continue };
        let Some(p) = &c.panel else { continue };
        let who = format!("board {id} ({})", shown(&c.family));
        if !equipment(m, c) {
            return Err(format!("{who} is not electrical equipment: only a distribution board has panel data; take it off with `panel`"));
        }
        if let Some(d) = &p.distribution
            && !m.distribution_systems.contains_key(d)
        {
            let there: Vec<&str> = m.distribution_systems.keys().map(String::as_str).collect();
            return Err(format!("{who}: distribution system `{}` isn't one of the project's (there are: {})", shown(d), there.join(", ")));
        }
        if let Some(a) = p.mains
            && !(a.is_finite() && a > 0.0 && a <= MAX_MAINS)
        {
            return Err(format!("{who}: `mains` is amperes above 0 and up to {MAX_MAINS}, not {a}"));
        }
        if let Some(n) = p.slots
            && !(1..=MAX_SLOTS).contains(&n)
        {
            return Err(format!("{who}: `slots` is 1 to {MAX_SLOTS}, not {n}"));
        }
        let Some(up) = p.supply_from else { continue };
        let Some(up_id) = m.elements.id(up) else {
            return Err(format!("{who}: `supply_from` names an element the project doesn't have"));
        };
        if up_id == id {
            return Err(format!("{who} cannot be supplied from itself"));
        }
        if board(m, up_id).is_none() {
            return Err(format!("{who}: `supply_from` {up_id} is not electrical equipment, so it cannot supply a board"));
        }
        // Up the chain from the board that supplies it: it must not come back.
        if let Some(chain) = supply_loop(m, id) {
            let path: Vec<String> = chain.iter().map(Id::to_string).collect();
            return Err(format!(
                "{who} cannot be supplied from {up_id}: that makes a supply loop, {} and back to {id}; change what supplies one of them",
                path.join(" supplied from ")
            ));
        }
    }
    Ok(())
}
