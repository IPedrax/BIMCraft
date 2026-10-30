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

use crate::family::{Category, ConnectorKind, ConnectorValues};
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
    check_panels(m)?;
    check_circuits(m)?;
    for (class, factor) in &m.demand_factors {
        if class.trim().is_empty() || class.chars().count() > MAX_TEXT {
            return Err(format!("demand factors: a classification is 1 to {MAX_TEXT} characters, not `{}`", shown(class)));
        }
        if !(factor.is_finite() && *factor > 0.0 && *factor <= 1.0) {
            return Err(format!("demand factors: `{}` is above 0 and up to 1, not {factor}", shown(class)));
        }
    }
    Ok(())
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

// ---------- circuits ----------

/// The devices a circuit holds, at most, and the highest number a circuit has.
pub const MAX_DEVICES: usize = 1_000;
pub const MAX_NUMBER: u32 = 9_999;

fn unity() -> f64 {
    1.0
}

fn is_unity(x: &f64) -> bool {
    *x == 1.0
}

/// A conductor: a section (mm²) of a wire type of the project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireRef {
    pub wire_type: String,
    pub section: f64,
}

/// The conductors of a circuit: the hot ones, the neutral and the protective one, each a section of a wire type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wires {
    pub hot: WireRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neutral: Option<WireRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground: Option<WireRef>,
}

impl Wires {
    fn each(&self) -> [(&'static str, Option<&WireRef>); 3] {
        [("hot", Some(&self.hot)), ("neutral", self.neutral.as_ref()), ("ground", self.ground.as_ref())]
    }

    /// The names of the wire types it holds, for what renames them.
    pub(crate) fn wire_types_mut(&mut self) -> impl Iterator<Item = &mut String> {
        std::iter::once(&mut self.hot.wire_type)
            .chain(self.neutral.iter_mut().map(|w| &mut w.wire_type))
            .chain(self.ground.iter_mut().map(|w| &mut w.wire_type))
    }
}

/// An electrical circuit: devices assigned to a distribution board, behind one breaker. The devices' voltage, poles and load are
/// their families' connectors'; what it loads is worked out from them ([`circuit_totals`]) and never saved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Circuit {
    /// The board it is on, by GUID.
    pub panel: Guid,
    /// Its number on that board.
    pub number: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The breaker, in amperes.
    pub rating_a: f64,
    pub poles: u8,
    /// A voltage of the project that its board's system supplies; none, the one its poles imply there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage: Option<String>,
    /// What it is for: `lighting`, `receptacle`…, as its devices' connectors say.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub classification: String,
    /// The share of its load that counts (above 0 up to 1).
    #[serde(default = "unity", skip_serializing_if = "is_unity")]
    pub demand_factor: f64,
    /// Its devices, by GUID.
    #[serde(default)]
    pub devices: Vec<Guid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wires: Option<Wires>,
}

impl Circuit {
    /// The names of the library entries it holds, for what renames them: its voltage and its wire types.
    pub(crate) fn voltages_mut(&mut self) -> impl Iterator<Item = &mut String> {
        self.voltage.iter_mut()
    }
}

/// The distribution board `panel` names, and the distribution system it is on.
fn board_system(m: &Model, panel: Guid) -> Result<(Id, &DistributionSystem, &str), String> {
    let id = m.elements.id(panel).ok_or_else(|| "its panel is not an element of the project".to_string())?;
    let (_, b) = board(m, id).ok_or_else(|| format!("its panel, element {id}, is not a distribution board"))?;
    let name = b
        .panel
        .as_ref()
        .and_then(|p| p.distribution.as_deref())
        .ok_or_else(|| format!("board {id} has no distribution system: give it one with `panel`"))?;
    let system = m
        .distribution_systems
        .get(name)
        .ok_or_else(|| format!("board {id}: distribution system `{}` isn't one of the project's", shown(name)))?;
    Ok((id, system, name))
}

/// The voltage a circuit is at, by its name and as the project defines it: the one it names, which its board's system supplies,
/// else the one its poles imply there (one pole: line to neutral; two or three: line to line).
pub fn circuit_voltage(m: &Model, c: &Circuit) -> Result<(String, Voltage), String> {
    let (id, system, sname) = board_system(m, c.panel)?;
    let name = match &c.voltage {
        Some(v) => {
            if !system.voltage_names().any(|n| n == v) {
                let there: Vec<&str> = system.voltage_names().map(String::as_str).collect();
                return Err(format!(
                    "voltage `{}` isn't one system `{sname}` of board {id} supplies (it supplies {})",
                    shown(v),
                    there.join(", ")
                ));
            }
            v.clone()
        }
        None => {
            let pick = if c.poles <= 1 { system.l_n.as_ref().or(system.l_g.as_ref()) } else { system.l_l.as_ref() };
            pick.cloned().ok_or_else(|| {
                format!(
                    "system `{sname}` of board {id} has no {} voltage for a circuit of {} pole(s): name one with `voltage`",
                    if c.poles <= 1 { "line-to-neutral" } else { "line-to-line" },
                    c.poles
                )
            })?
        }
    };
    let v = m.voltages.get(&name).ok_or_else(|| format!("voltage `{}` isn't one of the project's", shown(&name)))?.clone();
    Ok((name, v))
}

/// What the device `id` draws, as its family's power connector says for its type: a component of a family that has one.
pub fn device_power(m: &Model, id: Id) -> Result<ConnectorValues, String> {
    let Some(Element::Component(c)) = m.elements.get(id) else {
        return Err(format!("element {id} is not a device: a circuit takes components of a family with a power connector"));
    };
    let none = || format!("device {id} ({}) has no power connector: give its family one with `family` `connectors`", shown(&c.family));
    let family = m.families.get(&c.family).ok_or_else(none)?;
    let values = family.connector_values(&c.type_name, &c.params).map_err(|e| format!("device {id}: {e}"))?;
    values.into_iter().find(|v| v.kind == ConnectorKind::Power).ok_or_else(none)
}

/// A circuit's load, worked out from its devices: all of it, what counts of it, the current and each pole's.
#[derive(Clone, Debug, PartialEq)]
pub struct Totals {
    pub installed_va: f64,
    pub demand_va: f64,
    pub current_a: f64,
    /// Each pole's load, VA.
    pub pole_va: Vec<f64>,
    /// The volts the current is at.
    pub volts: f64,
}

fn thousandth(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0 + 0.0
}

/// The line current of `va` volt-amperes on `poles` poles at `volts`: through one or two poles it is `va / volts` (the voltage
/// between the two), through three, balanced, `va / (√3 · volts)`.
fn line_current(va: f64, poles: u8, volts: f64) -> f64 {
    if volts <= 0.0 {
        0.0
    } else if poles >= 3 {
        va / (3f64.sqrt() * volts)
    } else {
        va / volts
    }
}

/// What circuit `c` loads: none when its board or its voltage is not there to say it.
pub fn circuit_totals(m: &Model, c: &Circuit) -> Option<Totals> {
    let (_, v) = circuit_voltage(m, c).ok()?;
    let mut pole_va = vec![0.0; usize::from(c.poles)];
    let mut installed = 0.0;
    for g in &c.devices {
        let Some(p) = m.elements.id(*g).and_then(|id| device_power(m, id).ok()) else { continue };
        installed += p.load_va;
        for (slot, va) in pole_va.iter_mut().zip(&p.load_va_by_phase) {
            *slot += va;
        }
    }
    let demand = installed * c.demand_factor;
    Some(Totals {
        installed_va: thousandth(installed),
        demand_va: thousandth(demand),
        current_a: thousandth(line_current(installed, c.poles, v.nominal)),
        pole_va: pole_va.into_iter().map(thousandth).collect(),
        volts: v.nominal,
    })
}

/// What a board's circuits load: all of it, what counts of it and the current of what counts, balanced over the system's phases
/// (an unbalanced one waits for each circuit to say its phase). The circuits it has, by id.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardTotals {
    pub installed_va: f64,
    pub demand_va: f64,
    pub current_a: Option<f64>,
    pub circuits: Vec<Id>,
}

/// The circuits of board `id`, by id.
pub fn circuits_of(m: &Model, id: Id) -> Vec<Id> {
    let g = m.guid(id);
    m.elements.iter().filter(|(_, e)| matches!(e, Element::Circuit(c) if c.panel == g)).map(|(i, _)| *i).collect()
}

/// The factor the project's table gives a classification, applied to the demand of the circuits of it (1 when it has none).
pub fn class_factor(m: &Model, class: &str) -> f64 {
    m.demand_factors.get(class).copied().unwrap_or(1.0)
}

/// The slots a circuit of `poles` poles that starts at slot `number` takes: Revit's boards have odd slots on the left and even
/// on the right, so a second pole is the next slot down the same side.
pub fn slots_of(number: u32, poles: u8) -> impl Iterator<Item = u32> + Clone {
    (0..u32::from(poles)).map(move |k| number.saturating_add(2 * k))
}

/// The phase (0 for A, 1 for B, 2 for C) slot `slot` is on in a system of `phases` phases: a row of two slots is one phase.
pub fn slot_phase(slot: u32, phases: u8) -> u8 {
    u8::try_from((slot.saturating_sub(1) / 2) % u32::from(phases.clamp(1, 3))).unwrap_or(0)
}

/// A phase's letter.
pub fn phase_letter(phase: u8) -> char {
    char::from(b'A' + phase.min(25))
}

/// A board's phases: its system's, none when it has none.
pub fn board_phases(m: &Model, id: Id) -> Option<u8> {
    let (_, b) = board(m, id)?;
    let name = b.panel.as_ref()?.distribution.as_deref()?;
    m.distribution_systems.get(name).map(|s| s.phases)
}

/// Board `id`'s slots that circuits take, each by the circuit.
pub fn occupied(m: &Model, id: Id) -> BTreeMap<u32, Id> {
    let mut slots = BTreeMap::new();
    for cid in circuits_of(m, id) {
        if let Some(Element::Circuit(c)) = m.elements.get(cid) {
            for s in slots_of(c.number, c.poles) {
                slots.entry(s).or_insert(cid);
            }
        }
    }
    slots
}

/// How many slots board `id` shows: its `Max #1 Pole Breakers`, else up to the last slot a circuit takes, made even.
pub fn slot_count(m: &Model, id: Id) -> u32 {
    let set = board(m, id).and_then(|(_, b)| b.panel.as_ref()).and_then(|p| p.slots);
    set.unwrap_or_else(|| occupied(m, id).keys().next_back().map_or(0, |last| last + last % 2))
}

/// The first number a circuit of `poles` poles can have on board `id`: every slot it takes free (those of circuit `except`
/// count as free) and none past the board's slots, on a run that starts in phase `phase` when that is given.
pub fn free_number(m: &Model, id: Id, poles: u8, phase: Option<u8>, except: Option<Id>) -> Option<u32> {
    let taken: BTreeMap<u32, Id> = occupied(m, id).into_iter().filter(|(_, c)| Some(*c) != except).collect();
    let set = board(m, id).and_then(|(_, b)| b.panel.as_ref()).and_then(|p| p.slots);
    let phases = board_phases(m, id).unwrap_or(1);
    let last = set.unwrap_or(MAX_NUMBER).min(MAX_NUMBER);
    (1..=last)
        .find(|n| slots_of(*n, poles).all(|s| s <= last && !taken.contains_key(&s)) && phase.is_none_or(|p| slot_phase(*n, phases) == p))
}

/// What a board's phases carry, volt-amperes: all of the load on each, and what counts of it (the circuits' demand factors, then
/// their classifications'). Each circuit's pole loads fall on the phases of the slots they take.
#[derive(Clone, Debug, PartialEq)]
pub struct PhaseLoads {
    pub installed: Vec<f64>,
    pub demand: Vec<f64>,
}

/// A board's loads by phase; none for what is no board or has no system.
pub fn phase_loads(m: &Model, id: Id) -> Option<PhaseLoads> {
    let phases = board_phases(m, id)?;
    let (mut installed, mut demand) = (vec![0.0; usize::from(phases)], vec![0.0; usize::from(phases)]);
    for cid in circuits_of(m, id) {
        let Some(Element::Circuit(c)) = m.elements.get(cid) else { continue };
        let Some(t) = circuit_totals(m, c) else { continue };
        let factor = c.demand_factor * class_factor(m, &c.classification);
        for (k, va) in t.pole_va.iter().enumerate() {
            let p = usize::from(slot_phase(c.number.saturating_add(2 * u32::try_from(k).unwrap_or(0)), phases));
            installed[p] += va;
            demand[p] += va * factor;
        }
    }
    Some(PhaseLoads { installed: installed.into_iter().map(thousandth).collect(), demand: demand.into_iter().map(thousandth).collect() })
}

/// A board's totals; none for what is no board.
pub fn board_totals(m: &Model, id: Id) -> Option<BoardTotals> {
    let (_, b) = board(m, id)?;
    let circuits = circuits_of(m, id);
    let (mut installed, mut demand) = (0.0, 0.0);
    for cid in &circuits {
        if let Some(Element::Circuit(c)) = m.elements.get(*cid)
            && let Some(t) = circuit_totals(m, c)
        {
            installed += t.installed_va;
            demand += t.demand_va * class_factor(m, &c.classification);
        }
    }
    let current = b.panel.as_ref().and_then(|p| p.distribution.as_deref()).and_then(|n| m.distribution_systems.get(n)).and_then(|sys| {
        let (volts, poles) = match sys.phases {
            3 => (sys.l_l.as_ref(), 3),
            2 => (sys.l_l.as_ref(), 2),
            _ => (sys.l_l.as_ref().or(sys.l_n.as_ref()).or(sys.l_g.as_ref()), 1),
        };
        Some(thousandth(line_current(demand, poles, m.voltages.get(volts?)?.nominal)))
    });
    Some(BoardTotals { installed_va: thousandth(installed), demand_va: thousandth(demand), current_a: current, circuits })
}

/// What a project's circuits must hold: each on a distribution board that has a system, its breaker and poles in range and
/// within what the board and its system have, its voltage one the system supplies, each device one that draws power at that
/// voltage on that many poles and in no other circuit, the slots of a board's circuits distinct and within the board's slots, and each
/// conductor a section its wire type has.
fn check_circuits(m: &Model) -> Result<(), String> {
    let mut owner: BTreeMap<Guid, Id> = BTreeMap::new();
    let mut numbers: BTreeMap<(Id, u32), Id> = BTreeMap::new();
    let mut poles_on: BTreeMap<Id, u32> = BTreeMap::new();
    for (&id, e) in m.elements.iter() {
        let Element::Circuit(c) = e else { continue };
        let who = format!("circuit {id}");
        if c.name.chars().count() > MAX_TEXT {
            return Err(format!("{who}: its name is longer than {MAX_TEXT} characters"));
        }
        if c.classification.chars().count() > MAX_TEXT {
            return Err(format!("{who}: its classification is longer than {MAX_TEXT} characters"));
        }
        if !(1..=MAX_NUMBER).contains(&c.number) {
            return Err(format!("{who}: `number` is 1 to {MAX_NUMBER}, not {}", c.number));
        }
        if !(c.rating_a.is_finite() && c.rating_a > 0.0 && c.rating_a <= MAX_MAINS) {
            return Err(format!("{who}: `rating`, its breaker, is amperes above 0 and up to {MAX_MAINS}, not {}", c.rating_a));
        }
        if !(1..=3).contains(&c.poles) {
            return Err(format!("{who}: `poles` is 1, 2 or 3, not {}", c.poles));
        }
        if !(c.demand_factor.is_finite() && c.demand_factor > 0.0 && c.demand_factor <= 1.0) {
            return Err(format!("{who}: `demand_factor` is above 0 and up to 1, not {}", c.demand_factor));
        }
        if c.devices.len() > MAX_DEVICES {
            return Err(format!("{who}: it holds {} devices; it holds up to {MAX_DEVICES}", c.devices.len()));
        }
        let (pid, system, sname) = board_system(m, c.panel).map_err(|e| format!("{who}: {e}"))?;
        if let Some(mains) = board(m, pid).and_then(|(_, b)| b.panel.as_ref()).and_then(|p| p.mains)
            && c.rating_a > mains
        {
            return Err(format!("{who}: its breaker, {} A, is more than the {mains} A mains of board {pid}", c.rating_a));
        }
        if c.poles > system.phases {
            return Err(format!(
                "{who}: it has {} pole(s), but system `{sname}` of board {pid} has {} phase{}",
                c.poles,
                system.phases,
                if system.phases == 1 { "" } else { "s" }
            ));
        }
        let (vname, volts) = circuit_voltage(m, c).map_err(|e| format!("{who}: {e}"))?;
        let mut here: Vec<Guid> = Vec::new();
        for g in &c.devices {
            let Some(did) = m.elements.id(*g) else {
                return Err(format!("{who}: a device it holds is not an element of the project"));
            };
            if here.contains(g) {
                return Err(format!("{who}: device {did} is in it twice"));
            }
            here.push(*g);
            if let Some(other) = owner.insert(*g, id) {
                return Err(format!("{who}: device {did} is in circuit {other} already: a device is in one circuit"));
            }
            let draws = device_power(m, did).map_err(|e| format!("{who}: {e}"))?;
            let family = match m.elements.get(did) {
                Some(Element::Component(d)) => d.family.as_str(),
                _ => "",
            };
            if draws.poles != c.poles {
                return Err(format!(
                    "{who}: device {did} ({}) has {} pole(s) and the circuit {}: they go on a circuit of as many",
                    shown(family),
                    draws.poles,
                    c.poles
                ));
            }
            if draws.voltage < volts.min || draws.voltage > volts.max {
                return Err(format!(
                    "{who}: device {did} ({}) draws {} V, but the circuit is at {vname} ({} V, {} to {} V) on system `{sname}` of board {pid}",
                    shown(family),
                    draws.voltage,
                    volts.nominal,
                    volts.min,
                    volts.max
                ));
            }
        }
        for slot in slots_of(c.number, c.poles) {
            if let Some(other) = numbers.insert((pid, slot), id) {
                return Err(format!(
                    "{who}: slot {slot} is circuit {other}'s on board {pid} already (a circuit of {} pole(s) at number {} takes slots {}; \
                     give it another `number`)",
                    c.poles,
                    c.number,
                    slots_of(c.number, c.poles).map(|s| s.to_string()).collect::<Vec<_>>().join(", ")
                ));
            }
        }
        if let Some(last) = slots_of(c.number, c.poles).last()
            && let Some(slots) = board(m, pid).and_then(|(_, b)| b.panel.as_ref()).and_then(|p| p.slots)
            && last > slots
        {
            return Err(format!(
                "{who}: a circuit of {} pole(s) at number {} takes slot {last}, but board {pid} has {slots} slot(s) (`Max #1 Pole Breakers`)",
                c.poles, c.number
            ));
        }
        let used = poles_on.entry(pid).or_default();
        *used += u32::from(c.poles);
        if let Some(slots) = board(m, pid).and_then(|(_, b)| b.panel.as_ref()).and_then(|p| p.slots)
            && *used > slots
        {
            return Err(format!("{who}: board {pid} takes {slots} single-pole breakers; its circuits need {used}"));
        }
        if let Some(w) = &c.wires {
            for (role, r) in w.each() {
                let Some(r) = r else { continue };
                let Some(t) = m.wire_types.get(&r.wire_type) else {
                    let there: Vec<&str> = m.wire_types.keys().map(String::as_str).collect();
                    return Err(format!(
                        "{who}: its {role} wire type `{}` isn't one of the project's (there are: {})",
                        shown(&r.wire_type),
                        there.join(", ")
                    ));
                };
                if m.wire_ampacity(&r.wire_type, r.section).is_none() {
                    let mut sections: Vec<String> = t.sizes.iter().take(12).map(|s| s.section.to_string()).collect();
                    if t.sizes.len() > 12 {
                        sections.push("…".into());
                    }
                    return Err(format!(
                        "{who}: its {role} wire: wire type `{}` has no {} mm² row; its sections are {} mm²",
                        shown(&r.wire_type),
                        r.section,
                        sections.join(", ")
                    ));
                }
            }
        }
    }
    Ok(())
}

/// What is amiss in the circuits without making the model wrong: a circuit with no device (a spare), and a breaker that its
/// hot wire cannot carry.
pub(crate) fn circuit_warnings(m: &Model) -> Vec<String> {
    let mut out = Vec::new();
    for (&id, e) in m.elements.iter() {
        let Element::Circuit(c) = e else { continue };
        let on = m.elements.id(c.panel).map(|p| format!(" of board {p}")).unwrap_or_default();
        if c.devices.is_empty() {
            out.push(format!("circuit {id} (no. {}){on} has no devices: it is a spare", c.number));
        }
        if let Some(w) = &c.wires
            && let Some(carries) = m.wire_ampacity(&w.hot.wire_type, w.hot.section)
            && c.rating_a > carries
        {
            out.push(format!(
                "circuit {id} (no. {}){on}: its {} A breaker is more than the {carries} A its {} mm² {} hot wire carries",
                c.number, c.rating_a, w.hot.section, w.hot.wire_type
            ));
        }
    }
    out
}
