//! Which way swing doors open and which jamb they hang from, and the folding door's family: both engines draw doors with
//! them. The swing rules are read off the traced house (`data/house-swing.json`, SW- ids, all exploratory): what the
//! checker judges and what the generators set.
//!
//! A rule names a pair of room kinds (sorted), the kind the door opens into (`opens_into`) and the `hinge` (`start` or `end`).
//! The one pair with `exterior` in it is the exterior object: it applies to every exterior swing door, whatever its room's
//! kind. Its `opens_into` is `exterior` when the door opens out, any other value when it opens in, toward its own room.
//! `either` means that half is not enforced. A door with no `opens_into` or `hinge` is not judged.

use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwingRule {
    pub id: String,
    pub pair: [String; 2],
    pub opens_into: String,
    pub hinge: String,
    /// Always `exploratory`: read off one plan.
    pub status: String,
    /// Where in the traced house the rule is read from; for people reading the file.
    #[allow(dead_code)]
    pub provenance: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[allow(dead_code)]
    schema_version: String,
    #[allow(dead_code)]
    source: String,
    #[allow(dead_code)]
    note: String,
    swing: Vec<SwingRule>,
}

/// The embedded rules, parsed once.
pub fn rules() -> Result<&'static [SwingRule], String> {
    static RULES: OnceLock<Result<File, String>> = OnceLock::new();
    match RULES.get_or_init(|| {
        serde_json::from_str(include_str!("data/house-swing.json")).map_err(|e| format!("the embedded house swing rules: {e}"))
    }) {
        Ok(f) => Ok(&f.swing),
        Err(e) => Err(e.clone()),
    }
}

/// Unit 1's rules, read off its script by the same definition (`data/house-swing-unit1.json`).
fn unit1_rules() -> Result<&'static [SwingRule], String> {
    static RULES: OnceLock<Result<File, String>> = OnceLock::new();
    match RULES.get_or_init(|| {
        serde_json::from_str(include_str!("data/house-swing-unit1.json")).map_err(|e| format!("the embedded Unit 1 swing rules: {e}"))
    }) {
        Ok(f) => Ok(&f.swing),
        Err(e) => Err(e.clone()),
    }
}

/// Whether an exterior rule's `opens_into` says the door opens in (any value but `exterior` and `either`).
fn inward(rule: &SwingRule, word: &str) -> bool {
    rule.pair.iter().any(|k| k == "exterior") && word != "exterior" && word != "either"
}

/// What the checker judges by: the traced house's rules, each half (`opens_into`, `hinge`) left to `either` where a pair that
/// Unit 1 also has says something else. A pair only one trace has keeps that trace's rule, and where the two traces agree
/// the rule is as it was.
pub fn judged_rules() -> Result<&'static [SwingRule], String> {
    static JUDGED: OnceLock<Result<Vec<SwingRule>, String>> = OnceLock::new();
    match JUDGED.get_or_init(|| {
        let unit1 = unit1_rules()?;
        let relax = |rule: &SwingRule, ours: &str, theirs: &str| {
            if ours == theirs || (inward(rule, ours) && inward(rule, theirs)) { ours.to_string() } else { "either".to_string() }
        };
        Ok(rules()?
            .iter()
            .map(|r| match unit1.iter().find(|u| u.pair == r.pair) {
                Some(u) => {
                    SwingRule { opens_into: relax(r, &r.opens_into, &u.opens_into), hinge: relax(r, &r.hinge, &u.hinge), ..r.clone() }
                }
                None => r.clone(),
            })
            .collect())
    }) {
        Ok(v) => Ok(v),
        Err(e) => Err(e.clone()),
    }
}

/// The judged rule for two room kinds, in either order.
pub fn judged_rule_for(a: &str, b: &str) -> Option<&'static SwingRule> {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    judged_rules().ok()?.iter().find(|r| r.pair[0] == lo && r.pair[1] == hi)
}

/// The judged exterior rule.
pub fn judged_exterior() -> Option<&'static SwingRule> {
    judged_rules().ok()?.iter().find(|r| r.pair.iter().any(|k| k == "exterior"))
}

/// The rule for two room kinds, in either order.
pub fn rule_for(a: &str, b: &str) -> Option<&'static SwingRule> {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    rules().ok()?.iter().find(|r| r.pair[0] == lo && r.pair[1] == hi)
}

/// The exterior object: the rule whose pair has `exterior` in it.
pub fn exterior_rule() -> Option<&'static SwingRule> {
    rules().ok()?.iter().find(|r| r.pair.iter().any(|k| k == "exterior"))
}

/// The family of a folding door: a built-in has none, so it is defined here, with its jambs and its two leaves folded in plan.
pub const FOLDING_FAMILY: &str = "Folding Door";

pub(crate) fn folding_family(types: serde_json::Map<String, Value>) -> Value {
    let jamb = |lo: &str, hi: &str| {
        json!({
            "plane": "xy",
            "profile": [[lo, "-Wall Thickness / 2"], [hi, "-Wall Thickness / 2"], [hi, "Wall Thickness / 2"], [lo, "Wall Thickness / 2"]],
            "from": "0", "to": "Height", "material": "Frame Material",
        })
    };
    let leaf = "(Width - 2 * Frame Width) / 4";
    json!({
        "name": FOLDING_FAMILY,
        "category": "door",
        "operation": "folding",
        "parameters": [
            {"name": "Width", "kind": "length", "value": 900},
            {"name": "Height", "kind": "length", "value": 2100},
            {"name": "Frame Width", "kind": "length", "value": 20},
            {"name": "Frame Material", "kind": "material", "value": "Timber"},
        ],
        "types": types,
        "forms": [jamb("-Width / 2", "-Width / 2 + Frame Width"), jamb("Width / 2 - Frame Width", "Width / 2")],
        "symbols": [
            {"line": [
                ["-Width / 2 + Frame Width", "Wall Thickness / 2"],
                [format!("-Width / 2 + Frame Width + {leaf} * 0.84"), format!("Wall Thickness / 2 + {leaf} * 0.54")],
                [format!("-Width / 2 + Frame Width + {leaf} * 1.68"), "Wall Thickness / 2"],
            ]},
            {"line": [
                ["Width / 2 - Frame Width", "Wall Thickness / 2"],
                [format!("Width / 2 - Frame Width - {leaf} * 0.84"), format!("Wall Thickness / 2 + {leaf} * 0.54")],
                [format!("Width / 2 - Frame Width - {leaf} * 1.68"), "Wall Thickness / 2"],
            ]},
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_parses_with_stable_exploratory_ids() {
        let r = rules().expect("parses");
        let ids: Vec<&str> = r.iter().map(|r| r.id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
        assert!(ids.iter().all(|i| i.strip_prefix("SW-").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))));
        assert!(r.iter().all(|r| r.status == "exploratory"));
    }

    #[test]
    fn the_four_rules_are_the_traced_houses() {
        let got: Vec<(&str, &str, &str, &str, &str)> = rules()
            .expect("parses")
            .iter()
            .map(|r| (r.id.as_str(), r.pair[0].as_str(), r.pair[1].as_str(), r.opens_into.as_str(), r.hinge.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("SW-001", "bath", "hall", "bath", "start"),
                ("SW-002", "bedroom", "hall", "bedroom", "start"),
                ("SW-003", "exterior", "kitchen", "kitchen", "end"),
                ("SW-004", "hall", "living", "hall", "end"),
            ]
        );
        assert_eq!(rule_for("hall", "bedroom").map(|r| r.id.as_str()), Some("SW-002"));
        assert!(rule_for("bath", "bedroom").is_none());
        assert_eq!(exterior_rule().map(|r| r.id.as_str()), Some("SW-003"));
    }
}
