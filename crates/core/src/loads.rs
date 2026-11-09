//! The loads a dwelling's rooms must have as NBR 5410:2004 item 9.5.2 asks (residential): the fixed lighting point of each room
//! (9.5.2.1) and its general-use outlets, TUG (9.5.2.2.1 and 9.5.2.2.2). Pure functions of their input: the `load_points`
//! command is their caller, reading the rooms' areas and perimeters from the model or from its params.
//!
//! - Lighting: at least one point in the ceiling of each room, 100 VA up to 6 m² and 100 VA + 60 VA for each whole 4 m² above 6.
//! - Outlets, by the kind of room (known by its name, or given): a bathroom 1; kitchens, pantries, service areas and laundries
//!   one for each 3.5 m of perimeter (a whole number up, 1 at least); rooms and bedrooms one for each 5 m; a veranda 1; the others
//!   1 up to 6 m² and one for each 5 m of perimeter above.
//! - Power: bathrooms, kitchens, pantries, service areas and laundries 600 VA for each of the first 3 points of a room and 100 VA
//!   for each other; where those rooms together have more than 6 points the standard lets it be 600 VA for the first 2 of each room
//!   (the alternative, taken only when asked). Rooms, bedrooms, verandas and the others 100 VA a point. Power is paid for the points
//!   a room has, not for the least it needs.
//!
//! What is not applied: the exceptions the standard allows to the lighting point and to a veranda's outlet, and the placement of
//! the outlets (near the lavatory; above a counter; for a room of 2.25 m² or less, up to 0.80 m outside the door). The TUE, the
//! outlets for specific use, are not here.
//!
//! The numbers are data, not code: `data/load-points.json` holds a rule set for each standard, every row naming its source, and the
//! sum here only reads them (`RuleSet`). NBR 5410:2004 is the default and, for now, the only set.

use serde::Deserialize;
use std::sync::OnceLock;

/// The kinds of room the standard tells apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bath,
    Kitchen,
    Service,
    Dry,
    Veranda,
    Other,
}

/// The keys of `Kind`, as a command gives them.
pub const KINDS: &[&str] = &["bath", "kitchen", "service", "dry", "veranda", "other"];

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "bath" => Kind::Bath,
            "kitchen" => Kind::Kitchen,
            "service" => Kind::Service,
            "dry" => Kind::Dry,
            "veranda" => Kind::Veranda,
            "other" => Kind::Other,
            _ => return None,
        })
    }

    pub fn key(self) -> &'static str {
        KINDS[self as usize]
    }
}

/// A name in lower case without accents.
fn plain(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

/// The kind of room a name says (Portuguese or English), `Other` when it says none.
pub fn kind_of(name: &str) -> Kind {
    let n = plain(name);
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    let word = |w: &str| n.split(|c: char| !c.is_alphanumeric()).any(|x| x == w);
    if has(&["servico", "lavanderia", "laundry", "utility"]) {
        Kind::Service
    } else if has(&["banheiro", "lavabo", "sanitario", "bathroom", "toilet", "restroom"]) || word("wc") || word("bwc") || word("bath") {
        Kind::Bath
    } else if has(&["cozinha", "copa", "kitchen", "pantry"]) {
        Kind::Kitchen
    } else if has(&["varanda", "sacada", "balcony", "veranda"]) {
        Kind::Veranda
    } else if has(&["sala", "dormitorio", "quarto", "suite", "living", "bedroom"]) {
        Kind::Dry
    } else {
        Kind::Other
    }
}

/// The standard `load_points` follows when none is named.
pub const DEFAULT: &str = "NBR 5410:2004";

/// One standard's numbers for a dwelling's rooms, a row of `data/load-points.json`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSet {
    /// Its name and edition, which `load_points {standard}` takes.
    pub standard: String,
    /// The document, its item and the other standards that answer the same question.
    pub source: String,
    pub lighting: Lighting,
    pub alternative: Alternative,
    pub rooms: Vec<RoomRule>,
}

/// A room's least lighting load: `va` up to `up_to_m2` and `step_va` more for each whole `step_m2` above.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lighting {
    pub va: f64,
    pub up_to_m2: f64,
    pub step_m2: f64,
    pub step_va: f64,
    pub source: String,
}

/// Where the rooms paid at `first_va` hold more than `above` points together, `first` points of each may be paid so.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Alternative {
    pub above: u32,
    pub first: u32,
    pub source: String,
}

/// The outlets of one kind of room and their power.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomRule {
    /// A key of `KINDS`.
    pub kind: String,
    /// The fewest outlets.
    #[serde(default)]
    pub least: u32,
    /// One outlet for each so many metres of perimeter, a whole number up.
    #[serde(default)]
    pub per_perimeter_m: Option<f64>,
    /// The perimeter counts only above this area (m²).
    #[serde(default)]
    pub above_m2: Option<f64>,
    /// The outlets a kitchen needs over the counter of its sink.
    #[serde(default)]
    pub counter: u32,
    /// The points paid at `first_va`; a room with some is one the alternative counts.
    #[serde(default)]
    pub first: u32,
    #[serde(default)]
    pub first_va: f64,
    /// The power of every other point.
    pub va: f64,
    pub source: String,
}

#[derive(Default, Deserialize)]
struct Data {
    rule_sets: Vec<RuleSet>,
}

/// The rule sets of `data/load-points.json`; none if it does not read, which a test keeps from happening.
pub fn rule_sets() -> &'static [RuleSet] {
    static SETS: OnceLock<Vec<RuleSet>> = OnceLock::new();
    SETS.get_or_init(|| serde_json::from_str::<Data>(include_str!("../data/load-points.json")).map(|d| d.rule_sets).unwrap_or_default())
}

/// The rule set of a standard, by its name.
pub fn rule_set(standard: &str) -> Option<&'static RuleSet> {
    rule_sets().iter().find(|s| s.standard == standard)
}

/// The default standard's rules (empty, so asking nothing, if the data did not read).
fn default_set() -> &'static RuleSet {
    static EMPTY: OnceLock<RuleSet> = OnceLock::new();
    rule_set(DEFAULT).unwrap_or_else(|| EMPTY.get_or_init(RuleSet::default))
}

/// How many times `step` goes into `length`, a whole number up and 1 at least.
fn per(length: f64, step: f64) -> u32 {
    (length / step - 1e-9).ceil().max(1.0) as u32
}

impl RuleSet {
    /// The row of a kind of room; one that asks nothing if the set has none.
    pub fn room(&self, kind: Kind) -> &RoomRule {
        static NONE: OnceLock<RoomRule> = OnceLock::new();
        self.rooms.iter().find(|r| r.kind == kind.key()).unwrap_or_else(|| NONE.get_or_init(RoomRule::default))
    }

    /// Whether a kind of room is paid at `first_va` for its first points (the wet rooms of NBR 5410).
    pub fn wet(&self, kind: Kind) -> bool {
        self.room(kind).first > 0
    }

    /// The lighting of a room (VA) for its area (m²).
    pub fn lighting_va(&self, area: f64) -> f64 {
        let l = &self.lighting;
        if area > l.up_to_m2 && l.step_m2 > 0.0 { l.va + ((area - l.up_to_m2) / l.step_m2 + 1e-9).floor() * l.step_va } else { l.va }
    }

    /// The fewest outlets a room needs, for its area (m²) and perimeter (m).
    pub fn min_points(&self, kind: Kind, area: f64, perimeter: f64) -> u32 {
        let r = self.room(kind);
        match r.per_perimeter_m {
            Some(step) if step > 0.0 && r.above_m2.is_none_or(|a| area > a) => per(perimeter, step).max(r.least),
            _ => r.least,
        }
    }

    /// The power (VA) of `points` outlets in a room; `alternative` pays the alternative's first points, not the row's.
    pub fn power_va(&self, kind: Kind, points: u32, alternative: bool) -> f64 {
        let r = self.room(kind);
        let n = f64::from(points);
        if r.first > 0 {
            let full = f64::from(if alternative { self.alternative.first } else { r.first });
            r.first_va * n.min(full) + r.va * (n - full).max(0.0)
        } else {
            r.va * n
        }
    }

    /// The outlets a kitchen needs over the counter of its sink, at the same point or at others.
    pub fn counter_outlets(&self, kind: Kind, name: &str) -> u32 {
        let n = plain(name);
        if kind == Kind::Kitchen && ["cozinha", "kitchen", "copa"].iter().any(|w| n.contains(w)) { self.room(kind).counter } else { 0 }
    }

    /// Adds up the rooms; `alternative` is taken only where the standard allows it.
    pub fn total(&self, rooms: &[Room], alternative: bool) -> Total {
        let counts: Vec<(u32, u32)> = rooms
            .iter()
            .map(|r| {
                let least = self.min_points(r.kind, r.area, r.perimeter);
                (least, r.provided.unwrap_or(least))
            })
            .collect();
        let wet_points: u32 = rooms.iter().zip(&counts).filter(|(r, _)| self.wet(r.kind)).map(|(_, c)| c.1).sum();
        let possible = wet_points > self.alternative.above;
        let applied = alternative && possible;
        let shares: Vec<Share> = rooms
            .iter()
            .zip(&counts)
            .map(|(r, &(minimum, points))| Share {
                lighting_va: self.lighting_va(r.area),
                minimum,
                points,
                tug_va: self.power_va(r.kind, points, applied),
                counter_outlets: self.counter_outlets(r.kind, r.name),
            })
            .collect();
        Total {
            lighting_va: shares.iter().map(|s| s.lighting_va).sum(),
            tug_va: shares.iter().map(|s| s.tug_va).sum(),
            rooms: shares,
            wet_points,
            alternative_possible: possible,
            alternative_applied: applied,
        }
    }
}

/// The lighting of a room (VA) for its area (m²), by the default standard.
pub fn lighting_va(area: f64) -> f64 {
    default_set().lighting_va(area)
}

/// The fewest outlets a room needs, by the default standard.
pub fn min_points(kind: Kind, area: f64, perimeter: f64) -> u32 {
    default_set().min_points(kind, area, perimeter)
}

/// The power (VA) of `points` outlets in a room, by the default standard.
pub fn power_va(kind: Kind, points: u32, alternative: bool) -> f64 {
    default_set().power_va(kind, points, alternative)
}

/// The outlets over a kitchen's counter, by the default standard.
pub fn counter_outlets(kind: Kind, name: &str) -> u32 {
    default_set().counter_outlets(kind, name)
}

/// A room as the sum takes it: the area in m², the perimeter in m, and the outlets it has (`None`: just the least).
pub struct Room<'a> {
    pub name: &'a str,
    pub kind: Kind,
    pub area: f64,
    pub perimeter: f64,
    pub provided: Option<u32>,
}

/// One room's share.
#[derive(Debug)]
pub struct Share {
    pub lighting_va: f64,
    pub minimum: u32,
    pub points: u32,
    pub tug_va: f64,
    pub counter_outlets: u32,
}

/// The shares of all the rooms and their sums.
#[derive(Debug)]
pub struct Total {
    pub rooms: Vec<Share>,
    pub lighting_va: f64,
    pub tug_va: f64,
    /// The outlets of the rooms paid at 600 VA.
    pub wet_points: u32,
    /// Whether those are above 6, so that the alternative may be taken.
    pub alternative_possible: bool,
    pub alternative_applied: bool,
}

/// Adds up the rooms by the default standard.
pub fn total(rooms: &[Room], alternative: bool) -> Total {
    default_set().total(rooms, alternative)
}

#[cfg(test)]
mod tests;
