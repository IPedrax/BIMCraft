//! Building codes: the rules a project is held to by law or by a standard of practice, in five disciplines
//! (architectural, structural, electrical, mechanical, plumbing), apart from its technical standard, which is about
//! drafting. Nothing is ever refused for breaking a rule: what is outside one is built and reported.
//!
//! - A **code** ([`BuildingCode`]) is an entry for one discipline: its documents, who checked it, and its **sections**,
//!   each a group of rules as JSON (`stairs`, `conductors`, `drainage`…). A rule is a number, a yes or no, a choice or a
//!   table, with its source (`sources`, by `section.rule`) and its kind (`kinds`: a requirement, a practice…).
//! - A section is read by a module as a typed struct ([`Section`]), registered in [`sections`] with what it is and what
//!   reads it. The numbers are data, never code: `data/codes/<discipline>/*.json`, one file a code (`data.rs`).
//! - A project ([`Codes`], `Model::codes`) picks a **base** code for each discipline, any **amendments** over it (a state's
//!   fire brigade, a city's own code, a utility), and may change any rule (`changes`). The rules in force are those
//!   layers, the last one winning, rule by rule ([`Model::resolve_code`], [`Model::rules`], kept until the codes change).
//!   A section the base hasn't is taken from the first code BIMCraft has that holds it (the library), under what the
//!   amendments change of it, so a command works whatever the project chose, and says where its numbers came from.
//!   Amendments go in the order chosen, each over the base or another amendment held; a change made with `amend` is
//!   that amendment's (one of BIMCraft's becomes the project's copy, under its name).
//! - Site and Climate ([`Site`]) are the numbers a code's maps give for the project's place, shared by every tab.
//!
//! The area, and the rule that every check is fast and spatial: `docs/areas/building-codes.md`; the plan:
//! `docs/building-code-plan.md`.

use std::any::{Any, TypeId};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

use bimcraft_i18n::ui_msg;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::model::Model;

pub mod checks;
pub(crate) mod command;
mod data;
pub mod stairs;
mod xml;

pub use stairs::*;

/// The code of a project that has chosen none, in the architectural discipline: wide ranges round common practice.
pub const GENERIC: &str = "generic";

/// The most building codes a project keeps of its own, the longest name of one, and the largest file one is read from.
pub const MAX_OWN: usize = 100;
pub const MAX_NAME: usize = 64;
pub const MAX_XML: usize = 16 << 20;
/// The most values (numbers, texts, entries) a project's own codes and changes may hold together: tables the size of a
/// utility's, many times over, and no more.
pub const MAX_VALUES: usize = 400_000;
/// The most amendments over one base.
pub const MAX_AMENDMENTS: usize = 16;

/// A discipline: a tab of Manage > Building Codes, and the codes for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Discipline {
    #[default]
    Architectural,
    Structural,
    Electrical,
    Mechanical,
    Plumbing,
}

impl Discipline {
    pub const ALL: [Discipline; 5] =
        [Discipline::Architectural, Discipline::Structural, Discipline::Electrical, Discipline::Mechanical, Discipline::Plumbing];

    /// As the commands and files name it.
    pub fn key(self) -> &'static str {
        match self {
            Discipline::Architectural => "architectural",
            Discipline::Structural => "structural",
            Discipline::Electrical => "electrical",
            Discipline::Mechanical => "mechanical",
            Discipline::Plumbing => "plumbing",
        }
    }

    /// What the window calls it.
    pub fn label(self) -> &'static str {
        match self {
            Discipline::Architectural => ui_msg!("Architectural"),
            Discipline::Structural => ui_msg!("Structural"),
            Discipline::Electrical => ui_msg!("Electrical"),
            Discipline::Mechanical => ui_msg!("Mechanical"),
            Discipline::Plumbing => ui_msg!("Plumbing"),
        }
    }

    pub fn parse(t: &str) -> Option<Discipline> {
        Discipline::ALL.into_iter().find(|d| d.key() == t)
    }
}

/// What a rule's number is: the text's own, or something else that a reader should know.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// What the document asks.
    Requirement,
    /// What the document recommends.
    Recommendation,
    /// A maker's table, a handbook's or a common practice: no requirement.
    Practice,
    /// The project's to choose and justify.
    Assumption,
    /// An older edition's, kept for reference.
    Historical,
    /// No document gives it: a number that asks nothing in practice.
    Forgiving,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Requirement => ui_msg!("requirement"),
            Kind::Recommendation => ui_msg!("recommendation"),
            Kind::Practice => ui_msg!("practice"),
            Kind::Assumption => ui_msg!("assumption"),
            Kind::Historical => ui_msg!("historical"),
            Kind::Forgiving => ui_msg!("forgiving"),
        }
    }
}

/// A section's rules as a module reads them. The data's section deserializes into it; `check` says what can't be rules.
pub trait Section: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static {
    const DISCIPLINE: Discipline;
    const KEY: &'static str;
    fn check(&self) -> Result<(), String> {
        Ok(())
    }
}

/// A section as the registry knows it: its discipline and key, what the window calls it, a line on what it holds, what
/// reads it (none: shown as a reference only), and the reading of a value of it: the value as the section types it
/// (`1100` as `1100.0` where a length is a number), or what can't be rules.
pub struct SectionDef {
    pub discipline: Discipline,
    pub key: &'static str,
    pub label: &'static str,
    pub about: &'static str,
    pub read_by: &'static [&'static str],
    pub read: fn(&Value) -> Result<Value, String>,
}

impl SectionDef {
    /// Whether `v` can be this section's rules.
    pub fn check(&self, v: &Value) -> Result<(), String> {
        (self.read)(v).map(|_| ())
    }
}

fn check_as<S: Section>(v: &Value) -> Result<Value, String> {
    let s: S = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    s.check()?;
    serde_json::to_value(&s).map_err(|e| e.to_string())
}

/// A section a module reads as a type of its own that has no form to write back: it can be the rules if it reads as one, and
/// it is kept as it is.
fn valid_as<T: DeserializeOwned>(v: &Value) -> Result<Value, String> {
    serde_json::from_value::<T>(v.clone()).map_err(|e| e.to_string())?;
    Ok(v.clone())
}

/// A table by steps, as codes print a factor by a count or a load: the value of the first row whose bound `x` is up to, else
/// `beyond`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Steps {
    /// (up to, value), the bounds rising.
    pub rows: Vec<(f64, f64)>,
    pub beyond: f64,
}

impl Steps {
    pub fn at(&self, x: f64) -> f64 {
        self.rows.iter().find(|r| x <= r.0).map_or(self.beyond, |r| r.1)
    }

    /// Finite numbers, and bounds that rise.
    pub fn valid(&self, what: &str) -> Result<(), String> {
        if self.rows.iter().flat_map(|r| [r.0, r.1]).chain([self.beyond]).any(|x| !x.is_finite()) {
            return Err(format!("`{what}` holds a number that isn't finite"));
        }
        if self.rows.windows(2).any(|w| w[1].0 <= w[0].0) {
            return Err(format!("`{what}`: each row's bound is above the one before"));
        }
        Ok(())
    }
}

/// What a typed section's rules must be beyond their types: numbers that are finite and in range, lists that aren't empty.
pub trait Valid {
    fn valid(&self) -> Result<(), String>;
}

/// A section a module reads as a type of its own, with no form to write back, that checks what its types can't.
fn valid_checked<T: DeserializeOwned + Valid>(v: &Value) -> Result<Value, String> {
    serde_json::from_value::<T>(v.clone()).map_err(|e| e.to_string())?.valid()?;
    Ok(v.clone())
}

/// A section nothing reads yet: any object of rules, shown as a reference.
fn reference(v: &Value) -> Result<Value, String> {
    if v.is_object() { Ok(v.clone()) } else { Err("a section is an object of rules, by name".into()) }
}

/// Every section there is, in the order the window lists them.
pub fn sections() -> &'static [SectionDef] {
    use Discipline::*;
    static ALL: OnceLock<Vec<SectionDef>> = OnceLock::new();
    ALL.get_or_init(|| {
        let s = |discipline, key, label, about, read_by, read| SectionDef { discipline, key, label, about, read_by, read };
        vec![
            s(
                Architectural,
                "stairs",
                ui_msg!("Stairs and Railings"),
                ui_msg!("Risers, treads, widths, landings, winders, nosings, handrails and guards of stairs."),
                &["stair", "stair_check", "stair_solve", "code_check"],
                check_as::<StairRules>,
            ),
            s(
                Architectural,
                "ramps",
                ui_msg!("Ramps"),
                ui_msg!("The slope and width of new ramps, the steepest slope allowed, the height between landings, widths and landings."),
                &["ramp", "code_check"],
                check_as::<checks::RampRules>,
            ),
            s(
                Architectural,
                "doors",
                ui_msg!("Doors and Openings"),
                ui_msg!("The minimum clear width and height of doors, the widest leaf, and the doors that are not checked."),
                &["code_check"],
                check_as::<checks::DoorRules>,
            ),
            s(
                Architectural,
                "rooms",
                ui_msg!("Rooms and Habitability"),
                ui_msg!("Minimum area, width, height and natural light of rooms, by the kind of room its name shows."),
                &["code_check"],
                check_as::<checks::RoomRules>,
            ),
            s(
                Architectural,
                "egress",
                ui_msg!("Egress"),
                ui_msg!("Occupant load factors, exit widths, travel distances and corridors."),
                &[],
                reference,
            ),
            s(
                Architectural,
                "accessibility",
                ui_msg!("Accessibility"),
                ui_msg!("Accessible routes, turning spaces, reach ranges and accessible toilets."),
                &[],
                reference,
            ),
            s(
                Architectural,
                "fire",
                ui_msg!("Fire Safety"),
                ui_msg!("Fire resistance by element, compartments and separation."),
                &[],
                reference,
            ),
            s(
                Architectural,
                "envelope",
                ui_msg!("Envelope and Energy"),
                ui_msg!("Thermal transmittance of walls, roofs, floors and windows by climate zone."),
                &[],
                reference,
            ),
            s(
                Structural,
                "loads",
                ui_msg!("Loads and Combinations"),
                ui_msg!("Partial factors, imposed loads by use and unit weights."),
                &[],
                reference,
            ),
            s(
                Structural,
                "concrete",
                ui_msg!("Concrete"),
                ui_msg!("Concrete cover by exposure class, steel ratios, bars, stirrups and skin bars, used by the reinforcement check."),
                &["reinforce"],
                check_as::<crate::reinforce::norm::ConcreteRules>,
            ),
            s(
                Electrical,
                "rcd",
                ui_msg!("Residual-Current Devices"),
                ui_msg!("Which circuits need a high-sensitivity residual-current device (RCD), by place and type of point, and the highest residual current allowed."),
                &["dr_check"],
                valid_as::<crate::dr::RuleSet>,
            ),
            s(
                Electrical,
                "bath_volumes",
                ui_msg!("Bathroom Volumes"),
                ui_msg!("The zones around a bathtub or shower, their size, the IP rating, and what may be installed in each."),
                &["bath_volumes"],
                valid_as::<crate::bath::RuleSet>,
            ),
            s(
                Electrical,
                "load_points",
                ui_msg!("Dwelling Lighting and Outlets"),
                ui_msg!("The minimum lighting and outlets for each room of a home, and the power of each point."),
                &["load_points"],
                crate::loads::valid,
            ),
            s(
                Electrical,
                "building_demand",
                ui_msg!("Building Demand"),
                ui_msg!("How a utility or a country calculates a building's demand: the demand of one home, the factor for the number of homes, and the shared loads."),
                &["building_demand"],
                valid_as::<crate::building_demand::Method>,
            ),
            s(
                Electrical,
                "entrance",
                ui_msg!("Service Categories"),
                ui_msg!("The utility's service categories by supply system and use: the load or demand of each, with its breaker and cables."),
                &["demand"],
                valid_as::<crate::entrance::Utility>,
            ),
            s(
                Electrical,
                "voltages",
                ui_msg!("Voltages"),
                ui_msg!("The voltages the project uses, each with the lowest and highest a device may get."),
                &["voltage", "panel", "circuit"],
                crate::electrical::voltages_section,
            ),
            s(
                Electrical,
                "distribution_systems",
                ui_msg!("Distribution Systems"),
                ui_msg!("The systems that feed a board: the phases and wires, the voltages, and the earthing."),
                &["distribution_system", "panel"],
                crate::electrical::systems_section,
            ),
            s(
                Electrical,
                "wire_types",
                ui_msg!("Wire Types"),
                ui_msg!("The kinds of wire, by material and insulation, and the current each size carries."),
                &["wire_type", "circuit"],
                crate::electrical::wires_section,
            ),
            s(
                Electrical,
                "unit_demand",
                ui_msg!("Unit Demand and Supply"),
                ui_msg!("How a utility works out one unit's demand, and the supply category it gives for the load or demand."),
                &["demand"],
                valid_checked::<crate::demand::Standard>,
            ),
            s(
                Electrical,
                "circuits",
                ui_msg!("Circuit Sizing"),
                ui_msg!("The current each cable carries, the corrections for heat, grouping and soil, voltage drop, cable sizes and breakers."),
                &["circuit"],
                valid_checked::<crate::nbr5410::Rules>,
            ),
            s(
                Electrical,
                "conduit_fill",
                ui_msg!("Conduit Fill"),
                ui_msg!("How full a conduit may be, the conduits makers sell, and the outside size of each cable."),
                &["conduit_fill"],
                valid_checked::<crate::conduit::Rules>,
            ),
            s(
                Electrical,
                "surge",
                ui_msg!("Surge Protection"),
                ui_msg!("When a surge protective device is needed, where it goes, how it is connected, and the least currents and voltages."),
                &["dps_check"],
                valid_checked::<crate::dps::Rules>,
            ),
            s(
                Electrical,
                "lighting",
                ui_msg!("Lighting Levels"),
                ui_msg!("Maintained illuminance by room or task, and emergency lighting."),
                &[],
                reference,
            ),
            s(
                Electrical,
                "working_space",
                ui_msg!("Equipment Working Space"),
                ui_msg!("The clear space in front of panels and switchboards."),
                &[],
                reference,
            ),
            s(
                Mechanical,
                "ventilation",
                ui_msg!("Ventilation"),
                ui_msg!("Outdoor air by use, per person and per area, default occupancies and exhaust rates."),
                &[],
                reference,
            ),
            s(
                Mechanical,
                "ducts",
                ui_msg!("Ducts and Piping"),
                ui_msg!("The maximum air and water speeds, and friction losses."),
                &[],
                reference,
            ),
            s(
                Mechanical,
                "design_conditions",
                ui_msg!("Design Conditions"),
                ui_msg!("Indoor temperatures and humidity for heating and cooling."),
                &[],
                reference,
            ),
            s(
                Plumbing,
                "fixtures",
                ui_msg!("Fixtures"),
                ui_msg!("Each fixture's design flows, weight, fixture units and minimum drain."),
                &["water_size", "drain_size"],
                check_as::<crate::plumbing::Fixtures>,
            ),
            s(
                Plumbing,
                "cold_water",
                ui_msg!("Cold Water"),
                ui_msg!(
                    "The flow method, the maximum speed, the minimum pressure at a fixture, and the equivalent lengths of pipes and fittings."
                ),
                &["water_size"],
                check_as::<crate::water::WaterRules>,
            ),
            s(
                Plumbing,
                "reservoir",
                ui_msg!("Reservation"),
                ui_msg!("Days of water stored, refill time, compartments and pumps."),
                &["reservoir_size"],
                check_as::<crate::reservoir::ReservoirRules>,
            ),
            s(
                Plumbing,
                "drainage",
                ui_msg!("Sanitary Drainage and Vents"),
                ui_msg!("How many fixture units branches, stacks and building drains can carry by size and slope, and vents and trap distances."),
                &["drain_size", "vent_size"],
                check_as::<crate::drainage::DrainageRules>,
            ),
            s(
                Plumbing,
                "storm",
                ui_msg!("Storm Drainage"),
                ui_msg!("Rainfall, slopes, roughness, the minimum downpipe and the downpipe charts."),
                &["storm_size"],
                check_as::<crate::storm::StormRules>,
            ),
            s(
                Plumbing,
                "hydrants",
                ui_msg!("Hydrants and Hose Reels"),
                ui_msg!("Fire hydrant systems by type, the fire water reserve, pipe roughness, and the limits on speed and pressure."),
                &["hydrant_size"],
                check_as::<crate::hydrant::HydrantRules>,
            ),
            s(
                Plumbing,
                "fixture_counts",
                ui_msg!("Minimum Fixtures by Use"),
                ui_msg!("Toilets, washbasins and other fixtures a building needs for the people in it."),
                &[],
                reference,
            ),
        ]
    })
}

/// The section `key` of discipline `d`.
pub fn section_def(d: Discipline, key: &str) -> Option<&'static SectionDef> {
    sections().iter().find(|s| s.discipline == d && s.key == key)
}

/// A building code: an entry of `data/codes/`, or one of a project's own.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, from = "RawCode")]
pub struct BuildingCode {
    /// What `building_code {set}` takes, and a project file holds. One of a project's own is called by its name.
    pub code: String,
    #[serde(default)]
    pub discipline: Discipline,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub region: String,
    /// The documents its numbers are from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<String>,
    /// What an entry leaves out of its code, and which of its numbers are forgiving.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// The code one of a project's own started from, by what it is called.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub based_on: String,
    /// Its edition, as its documents name it (`2004`, `46.0`): a project pins the edition it chose
    /// ([`Choice::editions`]), and a newer one in the library changes nothing until the project takes it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub edition: String,
    /// The edition that replaced this one, where the library keeps an older edition for the projects pinned to it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub superseded_by: String,
    /// The code this one changes: empty for a base, which a project picks first; else an amendment, laid over the base or
    /// over another amendment (a member of a group of utilities over the group's standard).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub amends: String,
    /// The documents its sources cite, by an id of the code's own: title, edition, where it was found and when, kept
    /// apart from the rules so a moved link changes one line.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub documents: BTreeMap<String, Document>,
    /// Whether a project that chose none of its discipline's codes is held to this one (BIMCraft's own, one for each).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default: bool,
    /// Where each rule's number comes from, by `section.rule` (or `section` for all of it): a document of `documents`
    /// with its item and page, or a text saying so, so anyone can check it against its text; or why there is none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, Source>,
    /// What each number is, by `section.rule` or `section`, where it isn't the document's requirement.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub kinds: BTreeMap<String, Kind>,
    /// Who checked its numbers against the documents' text, and where that is written; empty while unchecked.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub checked: String,
    /// Its sections' rules, by the section's key.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sections: BTreeMap<String, Value>,
}

/// A code as a file holds it: as now, or as #386 wrote one (the stairs' rules beside the rest, sources by rule alone).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCode {
    code: String,
    #[serde(default)]
    discipline: Discipline,
    name: String,
    #[serde(default)]
    region: String,
    #[serde(default)]
    references: Vec<String>,
    #[serde(default)]
    note: String,
    #[serde(default)]
    based_on: String,
    #[serde(default)]
    edition: String,
    #[serde(default)]
    superseded_by: String,
    #[serde(default)]
    amends: String,
    #[serde(default)]
    documents: BTreeMap<String, Document>,
    #[serde(default)]
    default: bool,
    #[serde(default)]
    sources: BTreeMap<String, Source>,
    #[serde(default)]
    kinds: BTreeMap<String, Kind>,
    #[serde(default)]
    checked: String,
    #[serde(default)]
    sections: BTreeMap<String, Value>,
    #[serde(default)]
    stairs: Option<Value>,
}

impl From<RawCode> for BuildingCode {
    fn from(r: RawCode) -> BuildingCode {
        let mut sections = r.sections;
        let old = r.stairs.is_some();
        if let Some(stairs) = r.stairs {
            sections.entry("stairs".into()).or_insert(stairs);
        }
        // #386's sources were by the stair rule's name alone.
        let sources = if old {
            r.sources.into_iter().map(|(k, v)| if k.contains('.') { (k, v) } else { (format!("stairs.{k}"), v) }).collect()
        } else {
            r.sources
        };
        BuildingCode {
            code: r.code,
            discipline: r.discipline,
            name: r.name,
            region: r.region,
            references: r.references,
            note: r.note,
            based_on: r.based_on,
            edition: r.edition,
            superseded_by: r.superseded_by,
            amends: r.amends,
            documents: r.documents,
            default: r.default,
            sources,
            kinds: r.kinds,
            checked: r.checked,
            sections,
        }
    }
}

/// A document a code's numbers are from.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Document {
    pub title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub edition: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub publisher: String,
    /// Where it was found, and the day it was read there (`2026-11-16`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub accessed: String,
}

impl Document {
    /// Its title and edition, as a source cites it.
    pub fn cited(&self) -> String {
        if self.edition.is_empty() { self.title.clone() } else { format!("{} ({})", self.title, self.edition) }
    }
}

/// Where a number comes from: a text, or a document of the code's `documents` with its item and page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Source {
    Text(String),
    Cited(Citation),
}

/// A document cited by its id, with the item, table and page the number is on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Citation {
    pub doc: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub item: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub page: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
}

impl From<&str> for Source {
    fn from(t: &str) -> Source {
        Source::Text(t.to_string())
    }
}

impl Source {
    /// As a person reads it, the document `documents` names in full.
    pub fn said(&self, documents: &BTreeMap<String, Document>) -> String {
        match self {
            Source::Text(t) => t.clone(),
            Source::Cited(c) => {
                let mut out = documents.get(&c.doc).map_or_else(|| c.doc.clone(), Document::cited);
                for (part, lead) in [(&c.item, ""), (&c.page, "p. "), (&c.note, "")] {
                    if !part.is_empty() {
                        out.push_str(&format!(", {lead}{part}"));
                    }
                }
                out
            }
        }
    }

    /// How long its texts are together.
    fn len(&self) -> usize {
        match self {
            Source::Text(t) => t.chars().count(),
            Source::Cited(c) => [&c.doc, &c.item, &c.page, &c.note].iter().map(|t| t.chars().count()).sum(),
        }
    }
}

/// What a name for a project's own code must be: something, 64 characters at most, on one line, and not what BIMCraft
/// calls one of its own. The name, trimmed.
pub fn own_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME || name.chars().any(char::is_control) {
        return Err(format!("a building code's name is 1 to {MAX_NAME} characters on one line"));
    }
    if codes().iter().any(|c| c.code.eq_ignore_ascii_case(name) || c.name.eq_ignore_ascii_case(name)) {
        return Err(format!("`{name}` is a building code BIMCraft has: give the project's own another name"));
    }
    Ok(name.to_string())
}

/// How many values `v` holds, counting into its arrays and objects, stopping once past `most`.
pub(crate) fn values_in(v: &Value, most: usize) -> usize {
    fn walk(v: &Value, n: &mut usize, most: usize) {
        *n += 1;
        if *n > most {
            return;
        }
        match v {
            Value::Array(a) => a.iter().for_each(|x| walk(x, n, most)),
            Value::Object(o) => o.values().for_each(|x| walk(x, n, most)),
            _ => {}
        }
    }
    let mut n = 0;
    walk(v, &mut n, most);
    n
}

impl BuildingCode {
    /// Its section `key`'s rules, if it has the section.
    pub fn section(&self, key: &str) -> Option<&Map<String, Value>> {
        self.sections.get(key).and_then(Value::as_object)
    }

    /// Where rule `rule` of section `key` comes from: its own source, else its section's, as a person reads it.
    pub fn source(&self, key: &str, rule: &str) -> Option<String> {
        self.sources.get(&format!("{key}.{rule}")).or_else(|| self.sources.get(key)).map(|s| s.said(&self.documents))
    }

    /// Its name, with its edition.
    pub fn named(&self) -> String {
        if self.edition.is_empty() { self.name.clone() } else { format!("{} ({})", self.name, self.edition) }
    }

    /// What rule `rule` of section `key` is: as said, else its section's, else the document's requirement.
    pub fn kind(&self, key: &str, rule: &str) -> Kind {
        self.kinds.get(&format!("{key}.{rule}")).or_else(|| self.kinds.get(key)).copied().unwrap_or(Kind::Requirement)
    }

    /// What it is called in a list: its name, with its region.
    pub fn title(&self) -> String {
        if self.region.is_empty() { self.name.clone() } else { format!("{} ({})", self.name, self.region) }
    }

    /// Whether it can be a code: texts of a size, each of its sections the registry knows a section that can be.
    pub fn check_rules(&self) -> Result<(), String> {
        let long = |t: &str| t.chars().count() > 2000;
        if long(&self.name)
            || long(&self.region)
            || long(&self.note)
            || long(&self.based_on)
            || long(&self.amends)
            || self.references.len() > 50
            || self.references.iter().any(|r| long(r))
            || long(&self.checked)
            || self.sources.len() > 10_000
            || self.sources.iter().any(|(k, v)| long(k) || v.len() > 2000)
            || self.documents.len() > 200
            || self
                .documents
                .iter()
                .any(|(k, d)| long(k) || long(&d.title) || long(&d.edition) || long(&d.publisher) || long(&d.url) || long(&d.accessed))
            || self.kinds.len() > 10_000
        {
            return Err(format!("the building code `{}` has a text over 2000 characters, or more than 50 references", self.code));
        }
        // An amendment's sections hold what it changes, and are checked as laid over what it amends.
        if !self.amends.is_empty() {
            return Ok(());
        }
        for (key, v) in &self.sections {
            let Some(def) = section_def(self.discipline, key) else { continue };
            def.check(v).map_err(|e| format!("the building code `{}`, its {key}: {e}", self.code))?;
        }
        Ok(())
    }

    /// One of a project's own: a name that can be, and rules that can be.
    pub fn check(&self) -> Result<(), String> {
        // The project's copy of one of BIMCraft's amendments, changed for it, keeps that one's name.
        let copy = self.based_on == self.code && !self.amends.is_empty() && find(self.discipline, &self.code).is_some();
        if copy {
            return self.check_rules();
        }
        if own_name(&self.code)? != self.code {
            return Err(format!("the building code `{}` has spaces round its name", self.code));
        }
        self.check_rules()
    }

    /// As a file to hand to another project: XML, a `rule` for each of its rules.
    pub fn to_xml(&self) -> String {
        xml::write(std::slice::from_ref(self))
    }

    /// A code read from such a file, and what the file holds that this version doesn't know (a stairs-only file of #386
    /// too). A rule the file leaves out is its base's, else its discipline's default code's; the code is called by its
    /// name, or by `name`. A file of several codes gives the first: [`read_xml`] gives them all.
    pub fn from_xml(text: &str, name: Option<&str>) -> Result<(BuildingCode, Vec<String>), String> {
        let (mut all, left_out) = read_xml(text, name)?;
        if all.is_empty() {
            return Err("the file holds no building code".into());
        }
        Ok((all.remove(0), left_out))
    }
}

/// The codes an XML file holds, each completed and checked as one of a project's own, and what it holds that this version
/// doesn't know. `name` names the first one.
pub fn read_xml(text: &str, name: Option<&str>) -> Result<(Vec<BuildingCode>, Vec<String>), String> {
    if text.len() > MAX_XML {
        return Err(format!("the file is larger than building codes' ({} MB)", MAX_XML >> 20));
    }
    let (read, left_out) = xml::read(text)?;
    let mut out: Vec<BuildingCode> = Vec::new();
    // The first code renamed: what the file lays over it (a utility's method kept as an amendment) follows it.
    let mut first: Option<(Discipline, String, String)> = None;
    for (i, mut code) in read.into_iter().enumerate() {
        let wanted = if i == 0 { name } else { None };
        let was = code.name.clone();
        code.code = own_name(wanted.unwrap_or(&code.name)).map_err(|e| format!("{e} (`name` gives it one)"))?;
        if i == 0 {
            first = Some((code.discipline, was, code.code.clone()));
        } else if let Some((d, was, now)) = &first
            && code.discipline == *d
            && code.amends == *was
        {
            code.amends = now.clone();
            // An amendment named after the first (`<first> · <layer>`, as export writes them) is named after it again.
            if let Some(rest) = code.code.strip_prefix(&format!("{was} · ")) {
                let renamed = format!("{now} · {rest}");
                code.code = own_name(&renamed).map_err(|e| format!("{e}: `name` is too long for the codes the file lays over it"))?;
                code.name = code.code.clone();
            }
        }
        code.name = code.code.clone();
        complete(&mut code);
        code.check()?;
        out.push(code);
    }
    Ok((out, left_out))
}

/// Code `code` with what it leaves out of the sections it has filled in from what it is based on, or its discipline's
/// default code: a rule a file leaves out is that code's.
pub fn complete(code: &mut BuildingCode) {
    let from = find(code.discipline, &code.based_on).or_else(|| find(code.discipline, &code.amends));
    let base = from.unwrap_or_else(|| default_of(code.discipline));
    for (key, v) in code.sections.iter_mut() {
        // A section of reference is the file's as it is: only what a module reads must be whole.
        if section_def(code.discipline, key).is_none_or(|d| d.read_by.is_empty()) {
            continue;
        }
        // A utility's or a country's method is the file's whole: never filled in from another utility's.
        if is_whole(code.discipline, key) {
            continue;
        }
        let (Some(have), Some(theirs)) =
            (v.as_object_mut(), base.section(key).or_else(|| library(code.discipline, key).and_then(|c| c.section(key))))
        else {
            continue;
        };
        for (k, x) in theirs {
            have.entry(k.clone()).or_insert_with(|| x.clone());
        }
    }
}

/// The codes BIMCraft has, every discipline's, read once from `data/codes/`.
pub fn codes() -> &'static [BuildingCode] {
    static CODES: OnceLock<Vec<BuildingCode>> = OnceLock::new();
    CODES.get_or_init(data::read)
}

/// The codes BIMCraft has for discipline `d` in their current editions, its default first.
pub fn of(d: Discipline) -> impl Iterator<Item = &'static BuildingCode> {
    let current = move |c: &&BuildingCode| c.discipline == d && c.superseded_by.is_empty();
    let first = codes().iter().filter(current).filter(|c| c.default);
    first.chain(codes().iter().filter(current).filter(|c| !c.default))
}

/// One of BIMCraft's codes of discipline `d`, by what it is called.
pub fn find(d: Discipline, name: &str) -> Option<&'static BuildingCode> {
    if name.is_empty() {
        return None;
    }
    // `nbr`, the one entry the two Brazilian standards had before they were told apart, is NBR 9050's.
    let name = if d == Discipline::Architectural && name == "nbr" { "nbr-9050" } else { name };
    codes().iter().find(|c| c.discipline == d && c.code == name && c.superseded_by.is_empty())
}

/// One of BIMCraft's codes of discipline `d` in edition `edition`, if the library keeps it.
pub fn find_edition(d: Discipline, name: &str, edition: &str) -> Option<&'static BuildingCode> {
    codes().iter().find(|c| c.discipline == d && c.code == name && c.edition == edition)
}

/// An architectural code by what it is called (`nbr-9050`, `ibc`…): the stairs' codes.
pub fn code(name: &str) -> Option<&'static BuildingCode> {
    find(Discipline::Architectural, name)
}

/// The code of discipline `d` a project that chose none is held to.
pub fn default_of(d: Discipline) -> &'static BuildingCode {
    static NONE: OnceLock<BuildingCode> = OnceLock::new();
    of(d).next().unwrap_or_else(|| NONE.get_or_init(BuildingCode::default))
}

/// The architectural code of a project that chose none.
pub fn generic() -> &'static BuildingCode {
    default_of(Discipline::Architectural)
}

/// The first of BIMCraft's codes of discipline `d` that holds section `key`, the default first: where a section none of
/// a project's layers has is read from.
pub fn library(d: Discipline, key: &str) -> Option<&'static BuildingCode> {
    of(d).find(|c| c.sections.contains_key(key))
}

/// Section `S` as BIMCraft's library has it (the default code's, else the first code's that holds it), read once: what
/// the modules' functions without a project read, and what a project that chose nothing is held to.
pub fn library_rules<S: Section>() -> Arc<S> {
    type Kept = Mutex<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>;
    static KEPT: OnceLock<Kept> = OnceLock::new();
    let kept = KEPT.get_or_init(Default::default);
    if let Ok(map) = kept.lock()
        && let Some(any) = map.get(&TypeId::of::<S>())
        && let Ok(s) = Arc::clone(any).downcast::<S>()
    {
        return s;
    }
    let made =
        Arc::new(library(S::DISCIPLINE, S::KEY).and_then(|c| c.section(S::KEY)).and_then(|v| typed::<S>(v.clone())).unwrap_or_default());
    if let Ok(mut map) = kept.lock() {
        map.insert(TypeId::of::<S>(), Arc::clone(&made) as Arc<dyn Any + Send + Sync>);
    }
    made
}

/// Section `key` of every one of BIMCraft's codes of discipline `d` that holds it, read as `T`, in the library's order: where a
/// module that knows its rules by a standard's or a utility's own name looks them up.
pub fn library_sections<T: DeserializeOwned>(d: Discipline, key: &str) -> Vec<T> {
    of(d).filter_map(|c| c.section(key)).filter_map(|o| serde_json::from_value(Value::Object(o.clone())).ok()).collect()
}

/// A region's codes, one for each discipline, picked at once on the Overview.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub name: String,
    #[serde(default)]
    pub note: String,
    pub choices: BTreeMap<Discipline, Pick>,
}

/// A base and its amendments.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pick {
    pub code: String,
    #[serde(default)]
    pub amendments: Vec<String>,
}

/// The region presets of `data/codes/presets.json`.
pub fn presets() -> &'static [Preset] {
    static ALL: OnceLock<Vec<Preset>> = OnceLock::new();
    ALL.get_or_init(data::presets)
}

/// What a project chose for one discipline: its base code, the amendments over it in order, and the rules it changed,
/// by section, each a rule's whole value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Choice {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub amendments: Vec<String>,
    /// The edition of each of BIMCraft's codes it chose, by the code: the one it is held to while the library keeps it.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub editions: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub changes: BTreeMap<String, Map<String, Value>>,
}

impl Choice {
    pub fn is_empty(&self) -> bool {
        self.code.is_none() && self.amendments.is_empty() && self.editions.is_empty() && self.changes.is_empty()
    }
}

/// The numbers a code's maps give for the project's place, each with the map or table it was read from (`sources`, by
/// field). None is bundled: they are the project's.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Site {
    /// Rainfall intensity for storm drains, mm/h, and the return period it is for, years.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rain_intensity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rain_return_period: Option<f64>,
    /// The region's exposure to thunderstorms, as NBR 5410 classes it: AQ1, AQ2 or AQ3.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thunderstorms: Option<String>,
    /// Basic wind speed, m/s.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wind_speed: Option<f64>,
    /// Ground snow load, kN/m².
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ground_snow: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seismic_zone: Option<String>,
    /// Frost depth, mm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frost_depth: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub climate_zone: Option<String>,
    /// Altitude, m.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub altitude: Option<f64>,
    /// Outdoor design temperatures, °C.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cooling_design: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heating_design: Option<f64>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, String>,
}

/// Site and Climate's numbers: their key, their unit, and the range a number may have.
pub const SITE_NUMBERS: [(&str, &str, f64, f64); 9] = [
    ("rain_intensity", "mm/h", 0.0, 2000.0),
    ("rain_return_period", "years", 0.0, 10_000.0),
    ("wind_speed", "m/s", 0.0, 150.0),
    ("ground_snow", "kN/m²", 0.0, 100.0),
    ("frost_depth", "mm", 0.0, 10_000.0),
    ("altitude", "m", -500.0, 9_000.0),
    ("cooling_design", "°C", -80.0, 70.0),
    ("heating_design", "°C", -80.0, 70.0),
    ("_", "", 0.0, 0.0),
];

/// Its texts: their key, and the values they may have (any, up to 64 characters, where none are listed).
pub const SITE_TEXTS: [(&str, &[&str]); 3] = [("thunderstorms", &["AQ1", "AQ2", "AQ3"]), ("seismic_zone", &[]), ("climate_zone", &[])];

impl Site {
    pub fn is_empty(&self) -> bool {
        *self == Site::default()
    }

    /// Whether it can be: numbers in their range, texts short or of their list, sources by a field it has.
    pub fn check(&self) -> Result<(), String> {
        let Ok(Value::Object(o)) = serde_json::to_value(self) else { return Ok(()) };
        for (key, unit, low, high) in SITE_NUMBERS {
            if let Some(v) = o.get(key).and_then(Value::as_f64)
                && !(low..=high).contains(&v)
            {
                return Err(format!("site and climate: `{key}` is from {low} to {high} {unit}, not {v}"));
            }
        }
        for (key, allowed) in SITE_TEXTS {
            if let Some(t) = o.get(key).and_then(Value::as_str)
                && (t.chars().count() > 64 || (!allowed.is_empty() && !allowed.contains(&t)))
            {
                let what = if allowed.is_empty() { "64 characters at most".to_string() } else { allowed.join(", ") };
                return Err(format!("site and climate: `{key}` is {what}, not {t}"));
            }
        }
        for (k, v) in &self.sources {
            if !o.contains_key(k) || k == "sources" || v.chars().count() > 500 {
                return Err(format!("site and climate: a source for `{k}`, which it has no number for, or over 500 characters"));
            }
        }
        Ok(())
    }
}

/// A project's building codes: what it chose for each discipline, the codes of its own, and its site's numbers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Codes {
    #[serde(skip_serializing_if = "Choice::is_empty")]
    pub architectural: Choice,
    #[serde(skip_serializing_if = "Choice::is_empty")]
    pub structural: Choice,
    #[serde(skip_serializing_if = "Choice::is_empty")]
    pub electrical: Choice,
    #[serde(skip_serializing_if = "Choice::is_empty")]
    pub mechanical: Choice,
    #[serde(skip_serializing_if = "Choice::is_empty")]
    pub plumbing: Choice,
    /// Building codes of the project's own, saved under names it gave them (`building_code {save}`), or read from a file.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub own: Vec<BuildingCode>,
    #[serde(skip_serializing_if = "Site::is_empty")]
    pub site: Site,
}

impl Codes {
    pub fn is_empty(&self) -> bool {
        *self == Codes::default()
    }

    /// A file from before a utility's or a country's method was changed in its amendment (#502) keeps the project's change
    /// of one among its own changes, where nothing reads it now (`building_code` refuses to put one there). It goes into the
    /// layer whose method it changed, as `amend` puts a change today: the layer whose method names the same utility or
    /// standard, else the last that holds the method (the one in force when the file was saved), copied as the project's
    /// if it is BIMCraft's. A change no amendment holds (a country's method, a utility not chosen) stays where it is, as does
    /// one a project with as many codes of its own as it may keep would need a copy for.
    pub fn methods_into_layers(&mut self) {
        for (d, key) in WHOLE {
            let Some(changed) = self.choice(d).changes.get(key).cloned() else { continue };
            let field = match key {
                "unit_demand" => "utility",
                "entrance" => "id",
                _ => "standard",
            };
            let named = |m: &Map<String, Value>| m.get(field).and_then(Value::as_str).map(str::to_lowercase);
            let wanted = named(&changed);
            // An amendment's: a country's method, held by the base, is left as it is (a copy of a base under its own
            // name would be a project code BIMCraft has).
            let target = self
                .layers(d)
                .iter()
                .rev()
                .filter(|c| !c.amends.is_empty())
                .find(|c| whole_section(c, key).is_some_and(|w| wanted.is_none() || named(&w) == wanted))
                .map(|c| c.code.clone());
            let Some(target) = target else { continue };
            let Some(mut whole) = self.find(d, &target).and_then(|c| whole_section(c, key)) else { continue };
            // A code holds 10 000 sources and kinds at most (`BuildingCode::check`): one at the brim keeps the change where it is.
            let room = self.find(d, &target).is_some_and(|c| c.sources.len().max(c.kinds.len()) + changed.len() <= 10_000);
            if !room {
                continue;
            }
            if !self.own.iter().any(|k| k.code == target && k.discipline == d) {
                if self.own.len() >= MAX_OWN {
                    continue;
                }
                let Some(mut copy) = self.find(d, &target).cloned() else { continue };
                copy.default = false;
                copy.superseded_by.clear();
                copy.based_on = copy.code.clone();
                self.own.push(copy);
            }
            let Some(own) = self.own.iter_mut().find(|k| k.code == target && k.discipline == d) else { continue };
            for (rule, v) in changed {
                if whole.get(&rule) != Some(&v) {
                    own.sources.insert(format!("{key}.{rule}"), Source::Text(format!("{target}: changed in this project")));
                    own.kinds.insert(format!("{key}.{rule}"), Kind::Assumption);
                }
                whole.insert(rule, v);
            }
            own.sections.insert(key.to_string(), Value::Object(whole));
            self.choice_mut(d).changes.remove(key);
        }
    }

    pub fn choice(&self, d: Discipline) -> &Choice {
        match d {
            Discipline::Architectural => &self.architectural,
            Discipline::Structural => &self.structural,
            Discipline::Electrical => &self.electrical,
            Discipline::Mechanical => &self.mechanical,
            Discipline::Plumbing => &self.plumbing,
        }
    }

    pub fn choice_mut(&mut self, d: Discipline) -> &mut Choice {
        match d {
            Discipline::Architectural => &mut self.architectural,
            Discipline::Structural => &mut self.structural,
            Discipline::Electrical => &mut self.electrical,
            Discipline::Mechanical => &mut self.mechanical,
            Discipline::Plumbing => &mut self.plumbing,
        }
    }

    /// One of discipline `d`'s codes, the project's own first, by what it is called: in the edition the project pinned,
    /// while the library keeps it.
    pub fn find(&self, d: Discipline, name: &str) -> Option<&BuildingCode> {
        if let Some(own) = self.own.iter().find(|c| c.discipline == d && c.code == name) {
            return Some(own);
        }
        let pinned = self.choice(d).editions.get(name).and_then(|e| find_edition(d, name, e));
        pinned.or_else(|| find(d, name))
    }

    /// The codes of discipline `d` whose pinned edition the library no longer has: the code, the edition pinned, and the
    /// edition it is held to instead.
    pub fn editions_gone(&self, d: Discipline) -> Vec<(String, String, String)> {
        let choice = self.choice(d);
        let mut out = Vec::new();
        for (name, pinned) in &choice.editions {
            if find_edition(d, name, pinned).is_none()
                && let Some(now) = find(d, name)
            {
                out.push((name.clone(), pinned.clone(), now.edition.clone()));
            }
        }
        out
    }

    /// The editions now in force of the codes chosen for `d`, for the choice to pin.
    pub fn pins(&self, d: Discipline) -> BTreeMap<String, String> {
        let choice = self.choice(d);
        let names = choice.code.iter().chain(&choice.amendments);
        names
            .filter_map(|n| self.find(d, n))
            .filter(|c| !c.edition.is_empty() && !self.own.contains(c))
            .map(|c| (c.code.clone(), c.edition.clone()))
            .collect()
    }

    /// Every code of discipline `d` there is for this project: BIMCraft's, then its own.
    /// One of BIMCraft's that the project changed (an amendment edited) is its own copy, listed once.
    pub fn all(&self, d: Discipline) -> impl Iterator<Item = (&BuildingCode, bool)> {
        let shadowed = move |c: &BuildingCode| self.own.iter().any(|o| o.discipline == d && o.code == c.code);
        of(d).filter(move |c| !shadowed(c)).map(|c| (c, false)).chain(self.own.iter().filter(move |c| c.discipline == d).map(|c| (c, true)))
    }

    /// BIMCraft's code of discipline `d` called `name`, in the edition the project pinned while the library keeps it: what
    /// a copy of the project's own of it was taken from.
    pub fn library_of(&self, d: Discipline, name: &str) -> Option<&'static BuildingCode> {
        let pinned = self.choice(d).editions.get(name).and_then(|e| find_edition(d, name, e));
        pinned.or_else(|| find(d, name))
    }

    /// The base code of discipline `d`: the one chosen, if it is still there and a base, else the default.
    pub fn base(&self, d: Discipline) -> &BuildingCode {
        self.choice(d).code.as_deref().and_then(|k| self.find(d, k)).filter(|c| c.amends.is_empty()).unwrap_or_else(|| default_of(d))
    }

    /// The layers of discipline `d` in order: the base, then the amendments in the order chosen, the last winning, each one
    /// that is still there and amends the base or another amendment held (a member of a group over the group's standard).
    pub fn layers(&self, d: Discipline) -> Vec<&BuildingCode> {
        let base = self.base(d);
        let chosen: Vec<&BuildingCode> =
            self.choice(d).amendments.iter().filter_map(|a| self.find(d, a)).filter(|c| !c.amends.is_empty()).collect();
        let mut held: Vec<&str> = vec![base.code.as_str()];
        let mut on = vec![false; chosen.len()];
        // What stands over the base, then over what stands, until nothing more does: so order is the user's to choose.
        loop {
            let mut grew = false;
            for (i, c) in chosen.iter().enumerate() {
                if !on[i] && held.contains(&c.amends.as_str()) && !held.contains(&c.code.as_str()) {
                    on[i] = true;
                    held.push(c.code.as_str());
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        std::iter::once(base).chain(chosen.into_iter().zip(on).filter(|(_, on)| *on).map(|(c, _)| c)).collect()
    }

    /// Whether everything chosen can be: own codes few and each one that can be, amendments few, each change a rule of a
    /// section there is that leaves the section one that can be, the site's numbers in range, and all of it of a size.
    pub fn check(&self) -> Result<(), String> {
        if self.own.len() > MAX_OWN {
            return Err(format!("a project keeps {MAX_OWN} building codes of its own at most"));
        }
        let mut total = 0;
        for (i, c) in self.own.iter().enumerate() {
            c.check()?;
            if self.own.iter().take(i).any(|o| o.code == c.code && o.discipline == c.discipline) {
                return Err(format!("two of the project's building codes are called `{}`", c.code));
            }
            total += c.sections.values().map(|v| values_in(v, MAX_VALUES)).sum::<usize>();
        }
        for d in Discipline::ALL {
            let choice = self.choice(d);
            if choice.code.as_ref().is_some_and(|c| c.chars().count() > MAX_NAME)
                || choice.amendments.len() > MAX_AMENDMENTS
                || choice.editions.len() > MAX_AMENDMENTS + 1
                || choice.editions.iter().any(|(k, v)| k.chars().count() > MAX_NAME || v.chars().count() > MAX_NAME)
            {
                return Err(format!("{}: a code's name is {MAX_NAME} characters at most, and {MAX_AMENDMENTS} amendments", d.key()));
            }
            for changed in choice.changes.values() {
                total += changed.values().map(|v| values_in(v, MAX_VALUES)).sum::<usize>();
            }
            // What the project changed, and what an amendment of its own lays over a base, is checked as it stands.
            let mut keys: Vec<&String> = choice.changes.keys().collect();
            for layer in self.layers(d).into_iter().skip(1).filter(|c| self.own.contains(c)) {
                keys.extend(layer.sections.keys());
            }
            keys.sort();
            keys.dedup();
            for key in keys {
                let Some(def) = section_def(d, key) else { continue };
                let resolved = resolve_in(self, d, key);
                def.check(&Value::Object(resolved.value)).map_err(|e| format!("{} {key}: {e}", d.key()))?;
            }
        }
        if total > MAX_VALUES {
            return Err(format!("the project's building codes hold more than {MAX_VALUES} values"));
        }
        self.site.check()
    }
}

/// Where a rule in force comes from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "layer", content = "code")]
pub enum Layer {
    /// The base code.
    Base(String),
    /// An amendment over it.
    Amendment(String),
    /// None of the project's codes has the section: the code of BIMCraft's it was read from.
    Library(String),
    /// The project changed it.
    Project,
}

/// A section's rules in force, and where each comes from, with its source and kind.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resolved {
    pub value: Map<String, Value>,
    pub from: BTreeMap<String, Layer>,
    pub sources: BTreeMap<String, String>,
    pub kinds: BTreeMap<String, Kind>,
}

impl Resolved {
    /// The rules as they were before the project changed any.
    pub fn of_code(&self) -> bool {
        !self.from.values().any(|l| *l == Layer::Project)
    }
}

/// Section `key` of discipline `d` as `codes` holds it: its layers', the last one winning rule by rule, then the
/// project's changes; from the library where no layer has it.
pub fn resolve_in(codes: &Codes, d: Discipline, key: &str) -> Resolved {
    let mut out = layered(codes, d, key);
    if let Some(changed) = codes.choice(d).changes.get(key) {
        for (k, v) in changed {
            out.value.insert(k.clone(), v.clone());
            out.from.insert(k.clone(), Layer::Project);
            out.sources.remove(k);
            out.kinds.insert(k.clone(), Kind::Assumption);
        }
    }
    out
}

/// The sections that are one utility's or one country's method whole: an amendment holding one replaces the one under it
/// instead of being laid over it rule by rule, so one utility's rules never answer for another's.
pub const WHOLE: [(Discipline, &str); 3] =
    [(Discipline::Electrical, "unit_demand"), (Discipline::Electrical, "building_demand"), (Discipline::Electrical, "entrance")];

/// Whether section `key` of discipline `d` is a method whole (`WHOLE`).
pub fn is_whole(d: Discipline, key: &str) -> bool {
    WHOLE.iter().any(|(wd, k)| *wd == d && *k == key)
}

/// Section `key` of `c` as a method whole: what `c` holds, over what the code it was made from holds (a project's copy of
/// one of BIMCraft's utilities keeps that utility's rules it took out), never another code's.
pub fn whole_section(c: &BuildingCode, key: &str) -> Option<Map<String, Value>> {
    let own = c.section(key)?;
    let origin = [c.based_on.as_str(), c.code.as_str()].into_iter().find_map(|n| find(c.discipline, n)).filter(|o| !std::ptr::eq(*o, c));
    let mut out = origin.and_then(|o| o.section(key)).cloned().unwrap_or_default();
    // A null is a value here (a method with no common areas' factors says so): `amend` takes a rule out by leaving it out.
    for (k, v) in own {
        out.insert(k.clone(), v.clone());
    }
    Some(out)
}

/// Section `key` as the project's layers have it, without the project's changes.
pub fn layered(codes: &Codes, d: Discipline, key: &str) -> Resolved {
    let mut out = Resolved::default();
    let layers = codes.layers(d);
    let whole_key = is_whole(d, key);
    let take = |c: &BuildingCode, layer: Layer, out: &mut Resolved| {
        let Some(rules) = (if whole_key { whole_section(c, key) } else { c.section(key).cloned() }) else { return };
        // A method whole replaces the one under it.
        if whole_key {
            *out = Resolved::default();
        }
        for (k, v) in &rules {
            out.value.insert(k.clone(), v.clone());
            out.from.insert(k.clone(), layer.clone());
            match c.source(key, k) {
                Some(s) => out.sources.insert(k.clone(), s),
                None => out.sources.remove(k),
            };
            out.kinds.insert(k.clone(), c.kind(key, k));
        }
    };
    let lay = |out: &mut Resolved| {
        for (i, c) in layers.iter().enumerate() {
            let layer = if i == 0 { Layer::Base(c.code.clone()) } else { Layer::Amendment(c.code.clone()) };
            take(c, layer, out);
        }
    };
    lay(&mut out);
    // A section none of the layers has is the library's; one only amendments have, and that isn't whole without the
    // rules they leave out, is the library's under what they change of it.
    let whole = |r: &Resolved| section_def(d, key).is_none_or(|def| def.check(&Value::Object(r.value.clone())).is_ok());
    let base_has = layers.first().is_some_and(|b| b.section(key).is_some());
    // A method whole is never laid over another code's: only none held at all is the library's. The electrical settings are
    // entries by name, so an amendment's few are laid over the library's others, never in place of them.
    let entries = d == Discipline::Electrical && crate::electrical::SECTIONS.contains(&key);
    if (out.value.is_empty() || (!whole_key && !base_has && (entries || !whole(&out))))
        && let Some(lib) = library(d, key)
    {
        out = Resolved::default();
        take(lib, Layer::Library(lib.code.clone()), &mut out);
        lay(&mut out);
    }
    out
}

/// The kept typed sections of one set of codes.
type Kept = HashMap<TypeId, Arc<dyn Any + Send + Sync>>;

/// A model's typed sections, kept while its codes are the same allocation.
#[derive(Default)]
pub struct Cache {
    kept: Mutex<Option<(crate::shared::Identity<Codes>, Kept)>>,
}

impl Clone for Cache {
    fn clone(&self) -> Cache {
        Cache::default()
    }
}

impl PartialEq for Cache {
    /// What a model keeps is no part of what it is.
    fn eq(&self, _: &Cache) -> bool {
        true
    }
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cache")
    }
}

impl Cache {
    fn get<S: Section>(&self, codes: &crate::shared::Shared<Codes>, make: impl FnOnce() -> S) -> Arc<S> {
        let id = TypeId::of::<S>();
        if let Ok(kept) = self.kept.lock()
            && let Some((of, map)) = kept.as_ref()
            && of.is_of(codes)
            && let Some(any) = map.get(&id)
            && let Ok(s) = Arc::clone(any).downcast::<S>()
        {
            return s;
        }
        let made = Arc::new(make());
        if let Ok(mut kept) = self.kept.lock() {
            if !kept.as_ref().is_some_and(|(of, _)| of.is_of(codes)) {
                *kept = Some((codes.identity(), HashMap::new()));
            }
            if let Some((_, map)) = kept.as_mut() {
                map.insert(id, Arc::clone(&made) as Arc<dyn Any + Send + Sync>);
            }
        }
        made
    }
}

/// `v` as section `S`, or none where it can't be one.
fn typed<S: Section>(v: Map<String, Value>) -> Option<S> {
    serde_json::from_value::<S>(Value::Object(v)).ok().filter(|s| s.check().is_ok())
}

impl Model {
    /// The project's codes of discipline `d`: its base.
    pub fn base_code(&self, d: Discipline) -> &BuildingCode {
        self.codes.base(d)
    }

    /// The architectural code the project's stairs are held to.
    pub fn building_code(&self) -> &BuildingCode {
        self.codes.base(Discipline::Architectural)
    }

    /// An architectural code by name: the project's own first, then BIMCraft's.
    pub fn code(&self, name: &str) -> Option<&BuildingCode> {
        let name = if name == "nbr" { "nbr-9050" } else { name };
        self.codes.find(Discipline::Architectural, name)
    }

    /// Section `key` of discipline `d` in force, with where each rule comes from.
    pub fn resolve_code(&self, d: Discipline, key: &str) -> Resolved {
        resolve_in(&self.codes, d, key)
    }

    /// Section `S` in force, kept until the project's codes change: the project's, else its layers' alone, else the
    /// library's, else nothing's (which a test keeps from happening).
    pub fn rules<S: Section>(&self) -> Arc<S> {
        self.code_cache.get(&self.codes, || {
            typed::<S>(resolve_in(&self.codes, S::DISCIPLINE, S::KEY).value)
                .or_else(|| typed::<S>(layered(&self.codes, S::DISCIPLINE, S::KEY).value))
                .or_else(|| library(S::DISCIPLINE, S::KEY).and_then(|c| c.section(S::KEY)).and_then(|v| typed::<S>(v.clone())))
                .unwrap_or_default()
        })
    }

    /// Section `S` as code `name` has it: the rules in force where the code is one of the project's layers, with what it
    /// changed; that code's own otherwise. None where there is no such code, or it hasn't the section.
    pub fn rules_in<S: Section>(&self, name: &str) -> Option<Arc<S>> {
        let layers = self.codes.layers(S::DISCIPLINE);
        if layers.iter().any(|c| c.code == name) {
            return Some(self.rules::<S>());
        }
        let c = self.codes.find(S::DISCIPLINE, name)?;
        typed::<S>(c.section(S::KEY)?.clone()).map(Arc::new)
    }

    /// Section `key` of discipline `d` in force for the project, read as a module's own type `T`, and whether one of the
    /// project's layers or changes holds it (false: it is BIMCraft's library's). Read each time: for commands, not for checks.
    pub fn section_as<T: DeserializeOwned>(&self, d: Discipline, key: &str) -> Option<(T, bool)> {
        let r = self.resolve_code(d, key);
        if r.value.is_empty() {
            return None;
        }
        let own = r.from.values().any(|l| !matches!(l, Layer::Library(_)));
        serde_json::from_value(Value::Object(r.value)).ok().map(|t| (t, own))
    }

    /// A method whole (`WHOLE`) as each of the project's layers holds it, the last first, read as `T`, with the source of
    /// its rule `about` and whether it is the project's (an amendment changed in it, or its own code): what a command
    /// naming a utility reads, so a utility held under another answers by its own rules.
    pub fn held_wholes<T: DeserializeOwned>(&self, d: Discipline, key: &str, about: &str) -> Vec<(T, Option<String>, bool)> {
        self.codes
            .layers(d)
            .iter()
            .rev()
            .filter_map(|c| {
                let rules = whole_section(c, key)?;
                let t = serde_json::from_value(Value::Object(rules)).ok()?;
                let own = find(d, &c.code).is_none_or(|lib| !std::ptr::eq(lib, *c));
                Some((t, c.source(key, about).or_else(|| find(d, &c.based_on).and_then(|o| o.source(key, about))), own))
            })
            .collect()
    }

    /// The rules the project's stairs and railings are held to.
    pub fn stair_rules(&self) -> Arc<StairRules> {
        self.rules::<StairRules>()
    }
}

/// Where element `e` is outside the project's building codes; none for what no section checks yet.
pub fn findings(m: &Model, _id: crate::ids::Id, e: &crate::model::Element) -> Vec<Finding> {
    match e {
        crate::model::Element::Stair(st) => stair_findings(m, st),
        crate::model::Element::Ramp(r) => checks::ramp_findings(m, r),
        crate::model::Element::Door(o) => checks::door_findings(m, e, o),
        crate::model::Element::Room(r) => checks::room_findings(m, r),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod engine_tests;
#[cfg(test)]
mod matrix_tests;
#[cfg(test)]
mod rule_tests;
#[cfg(test)]
pub(crate) mod tests;
