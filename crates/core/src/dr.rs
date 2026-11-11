//! The circuits NBR 5410:2004 asks additional protection by a high-sensitivity residual-current device (DR) for: a rated residual
//! current of 30 mA at most, whatever the earthing scheme. A pure function of the circuits it is given, with no model behind it:
//! the `dr_check` command is its only caller.
//!
//! A circuit needs it (item 5.1.3.2.2) if any point it serves is in one of these cases:
//!
//! - a place with a bathtub or a shower: every point;
//! - an outlet in an external area;
//! - an internal outlet that may feed equipment outdoors;
//! - in a dwelling, a point in a kitchen, pantry-kitchen, laundry, service area, garage or other internal area that is wet in
//!   normal use or may be washed, but for lights 2.5 m up or more;
//! - in a building that is not a dwelling, an outlet in those same areas.
//!
//! An outlet is covered up to a rated current of 32 A (Note 1 of 5.1.3.2.2), in a bathroom too. A circuit or sector of an IT scheme that must not be cut, for the safety of
//! persons or to keep lives, is left out. What is not covered, and the caller says so: the further rules for the volumes of a
//! bathroom and for swimming pools, and the other protections against shock the device does not replace.
//!
//! The rules are data, not code: `data/dr-check.json` holds a rule set for each standard, every rule naming its source, and
//! `check` only reads them (`RuleSet`). NBR 5410:2004 is the default and, for now, the only set.

use serde::Deserialize;
use std::sync::OnceLock;

/// The standard `dr_check` follows when none is named.
pub const DEFAULT: &str = "NBR 5410:2004";

/// Where a point is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// A place with a bathtub or a shower.
    Bath,
    External,
    Kitchen,
    Laundry,
    Service,
    Garage,
    /// Another internal area that is wet in normal use or may be washed.
    Wet,
    Dry,
}

/// The keys of `Place`, as a command gives them.
pub const PLACES: &[&str] = &["bath", "external", "kitchen", "laundry", "service", "garage", "wet", "dry"];

impl Place {
    pub fn parse(s: &str) -> Option<Place> {
        Some(match s {
            "bath" => Place::Bath,
            "external" => Place::External,
            "kitchen" => Place::Kitchen,
            "laundry" => Place::Laundry,
            "service" => Place::Service,
            "garage" => Place::Garage,
            "wet" => Place::Wet,
            "dry" => Place::Dry,
            _ => return None,
        })
    }
}

/// What a point is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Outlet,
    Lighting,
    /// Any other point of use, such as a shower heater connected directly.
    Fixed,
}

/// The keys of `Kind`, as a command gives them.
pub const KINDS: &[&str] = &["outlet", "lighting", "fixed"];

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "outlet" => Kind::Outlet,
            "lighting" => Kind::Lighting,
            "fixed" => Kind::Fixed,
            _ => return None,
        })
    }
}

/// A point a circuit serves.
#[derive(Clone, Debug)]
pub struct Point {
    pub place: Place,
    pub kind: Kind,
    /// An outlet's rated current, A; unknown, it is taken as within the 32 A.
    pub rating: Option<f64>,
    /// A light's height, mm.
    pub height: Option<f64>,
    /// An internal outlet that may feed equipment outdoors.
    pub outdoor: bool,
}

/// A circuit: its points, the rated residual current (mA) of the device it has, if any, and whether it is of an IT scheme that must
/// not be cut.
#[derive(Clone, Debug)]
pub struct Circuit {
    pub points: Vec<Point>,
    pub residual: Option<f64>,
    pub it_continuity: bool,
}

/// What the standard asks of a circuit.
#[derive(Debug)]
pub struct Verdict {
    pub required: bool,
    /// It would need it, but is an IT circuit that must not be cut.
    pub exempt: bool,
    pub reasons: Vec<String>,
    /// The source of each reason, the standard's item.
    pub sources: Vec<String>,
    /// Whether what it has is enough; `None` for a circuit that does not need it.
    pub ok: Option<bool>,
}

/// What a building is, as a rule tells them apart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Building {
    #[default]
    Any,
    Residential,
    Other,
}

/// One case a standard asks the device for, a row of `data/dr-check.json`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// A word for the case, which leads the reason the answer gives.
    pub reason: String,
    /// The case, in words.
    pub says: String,
    /// Keys of `PLACES`; any place when empty.
    #[serde(default)]
    pub places: Vec<String>,
    /// Keys of `KINDS`; any kind when empty.
    #[serde(default)]
    pub kinds: Vec<String>,
    #[serde(default)]
    pub building: Building,
    /// Only an internal outlet that may feed equipment outdoors.
    #[serde(default)]
    pub outdoor: bool,
    /// A light this high (mm) or higher is left out.
    #[serde(default)]
    pub lights_from_mm: Option<f64>,
    pub source: String,
}

/// One standard's rules, an entry of `data/dr-check.json`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSet {
    /// Its name and edition, which `dr_check {standard}` takes.
    pub standard: String,
    /// The document, its item and the other standards that answer the same question.
    pub source: String,
    /// The most the device's rated residual current may be, mA.
    pub max_residual_ma: f64,
    /// The highest rated current (A) of an outlet the rules reach; none above it.
    pub max_outlet_a: Option<f64>,
    #[serde(default)]
    pub max_outlet_source: String,
    /// Whether a circuit of an IT scheme that must not be cut is left out.
    #[serde(default)]
    pub it_exempt: bool,
    #[serde(default)]
    pub it_source: String,
    pub rules: Vec<Rule>,
    /// What the set does not check, which the answer says.
    pub not_covered: String,
}

#[derive(Default, Deserialize)]
struct Data {
    rule_sets: Vec<RuleSet>,
}

/// The rule sets of `data/dr-check.json`; none if it does not read, which a test keeps from happening.
pub fn rule_sets() -> &'static [RuleSet] {
    static SETS: OnceLock<Vec<RuleSet>> = OnceLock::new();
    SETS.get_or_init(|| serde_json::from_str::<Data>(include_str!("../data/dr-check.json")).map(|d| d.rule_sets).unwrap_or_default())
}

/// The rule set of a standard, by its name.
pub fn rule_set(standard: &str) -> Option<&'static RuleSet> {
    rule_sets().iter().find(|s| s.standard == standard)
}

impl Place {
    pub fn key(self) -> &'static str {
        PLACES[self as usize]
    }
}

impl Kind {
    pub fn key(self) -> &'static str {
        KINDS[self as usize]
    }
}

impl Rule {
    /// Whether a point meets this rule in a building that is, or is not, a dwelling.
    fn holds(&self, p: &Point, residential: bool) -> bool {
        let building = match self.building {
            Building::Any => true,
            Building::Residential => residential,
            Building::Other => !residential,
        };
        let any = |list: &[String], key: &str| list.is_empty() || list.iter().any(|k| k == key);
        let high_light = p.kind == Kind::Lighting && self.lights_from_mm.is_some_and(|h| p.height.is_some_and(|x| x >= h));
        building
            && any(&self.places, p.place.key())
            && any(&self.kinds, p.kind.key())
            && (!self.outdoor || (p.kind == Kind::Outlet && p.outdoor))
            && !high_light
    }
}

impl RuleSet {
    fn covered(&self, p: &Point) -> bool {
        p.kind != Kind::Outlet || p.rating.is_none_or(|a| self.max_outlet_a.is_none_or(|m| a <= m))
    }

    /// The rule a point meets first, if any.
    pub fn rule(&self, p: &Point, residential: bool) -> Option<&Rule> {
        if !self.covered(p) {
            return None;
        }
        self.rules.iter().find(|r| r.holds(p, residential))
    }

    /// What a circuit needs and whether it has it; `residential` says the building is a dwelling.
    pub fn check(&self, c: &Circuit, residential: bool) -> Verdict {
        let mut reasons: Vec<String> = Vec::new();
        let mut sources: Vec<String> = Vec::new();
        for r in c.points.iter().filter_map(|p| self.rule(p, residential)) {
            let reason = format!("{}: {}", r.reason, r.says);
            if !reasons.contains(&reason) {
                reasons.push(reason);
                sources.push(r.source.clone());
            }
        }
        let asked = !reasons.is_empty();
        let exempt = asked && self.it_exempt && c.it_continuity;
        let required = asked && !exempt;
        let ok = required.then(|| c.residual.is_some_and(|r| r <= self.max_residual_ma));
        Verdict { required, exempt, reasons, sources, ok }
    }
}

/// The default standard's rules (empty, so asking nothing, if the data did not read).
pub fn default_set() -> &'static RuleSet {
    static EMPTY: OnceLock<RuleSet> = OnceLock::new();
    rule_set(DEFAULT).unwrap_or_else(|| EMPTY.get_or_init(RuleSet::default))
}

/// What a circuit needs by the default standard; `residential` says the building is a dwelling.
pub fn check(c: &Circuit, residential: bool) -> Verdict {
    default_set().check(c, residential)
}

#[cfg(test)]
mod tests;
