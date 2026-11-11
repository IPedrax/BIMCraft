//! The plumbing fixtures library: what `water_size` and `drain_size` know of each kind of fixture, read from
//! `data/plumbing.json`, the way `electrical.rs` reads `data/electrical.json`. A row holds the fixture's design flows (L/s: cold, and
//! hot where a source gives one), its relative weight (NBR 5626 method of weights), its units of contribution (UHC, NBR 8160) and the
//! smallest DN of its drain, each group with the source it comes from. A row without a source is refused by [`validate`], and a test
//! runs it over the shipped file, so a number is never added without saying where it is from.

use serde::Deserialize;
use std::sync::OnceLock;

/// One fixture of the library.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    /// The name commands take: `wc`, `lav`, …
    pub kind: String,
    /// What it is called in words.
    pub name: String,
    /// Design flow of cold water, in `unit`.
    pub cold_flow: f64,
    /// Design flow of hot water, in `unit`, or `None` where no source in the library gives one.
    pub hot_flow: Option<f64>,
    /// Relative weight, for the method of weights.
    pub weight: f64,
    /// Units of contribution (UHC) to a drain.
    pub uhc: f64,
    /// Smallest DN of its drain, mm.
    pub drain_dn: f64,
    /// The unit of the flows.
    pub unit: String,
    /// Where the flows and the weight are from.
    pub water_source: String,
    /// Where the UHC and the drain's DN are from.
    pub drain_source: String,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Library {
    fixtures: Vec<Fixture>,
}

/// A library is usable when every row names its source and its numbers are positive and finite.
pub fn validate(fixtures: &[Fixture]) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for f in fixtures {
        let who = &f.kind;
        if f.kind.trim().is_empty() || !seen.insert(f.kind.as_str()) {
            return Err(format!("fixture `{who}`: its kind is empty or repeated"));
        }
        if f.water_source.trim().is_empty() {
            return Err(format!("fixture `{who}`: it has no `water_source`; say where its flows and weight are from"));
        }
        if f.drain_source.trim().is_empty() {
            return Err(format!("fixture `{who}`: it has no `drain_source`; say where its UHC and drain DN are from"));
        }
        if f.unit.trim().is_empty() {
            return Err(format!("fixture `{who}`: it has no `unit` for its flows"));
        }
        let hot = f.hot_flow.unwrap_or(1.0);
        for (what, v) in [("cold_flow", f.cold_flow), ("hot_flow", hot), ("weight", f.weight), ("uhc", f.uhc), ("drain_dn", f.drain_dn)] {
            if !v.is_finite() || v <= 0.0 {
                return Err(format!("fixture `{who}`: `{what}` must be a positive number"));
            }
        }
    }
    Ok(())
}

/// The fixtures of the library, in file order. Empty if the shipped file does not read, which a test rules out.
pub fn fixtures() -> &'static [Fixture] {
    static LIBRARY: OnceLock<Vec<Fixture>> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        let lib: Library = serde_json::from_str(include_str!("../data/plumbing.json")).unwrap_or_default();
        if validate(&lib.fixtures).is_ok() { lib.fixtures } else { Vec::new() }
    })
}

/// A fixture by its kind.
pub fn fixture(kind: &str) -> Option<&'static Fixture> {
    fixtures().iter().find(|f| f.kind == kind)
}

/// The kinds the library has, in file order.
pub fn kinds() -> Vec<&'static str> {
    fixtures().iter().map(|f| f.kind.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> Fixture {
        Fixture {
            kind: "wc".into(),
            name: "Water closet".into(),
            cold_flow: 0.15,
            hot_flow: None,
            weight: 0.3,
            uhc: 6.0,
            drain_dn: 100.0,
            unit: "L/s".into(),
            water_source: "a source".into(),
            drain_source: "another".into(),
        }
    }

    #[test]
    fn the_shipped_library_reads_and_every_row_names_its_sources() {
        let lib: Library = serde_json::from_str(include_str!("../data/plumbing.json")).expect("data/plumbing.json reads");
        assert_eq!(lib.fixtures.len(), 5);
        assert_eq!(validate(&lib.fixtures), Ok(()));
        assert_eq!(kinds(), ["wc", "lav", "sink", "shower", "laundry"]);
    }

    #[test]
    fn the_numbers_water_size_and_drain_size_always_used_are_kept() {
        for (kind, q, p, uhc, dn) in [
            ("wc", 0.15, 0.3, 6.0, 100.0),
            ("lav", 0.15, 0.3, 1.0, 40.0),
            ("sink", 0.25, 0.7, 3.0, 50.0),
            ("shower", 0.20, 0.4, 2.0, 40.0),
            ("laundry", 0.25, 0.7, 3.0, 40.0),
        ] {
            let f = fixture(kind).expect("kind");
            assert_eq!((f.cold_flow, f.weight, f.uhc, f.drain_dn), (q, p, uhc, dn), "{kind}");
        }
        assert!(fixture("bidet").is_none());
    }

    #[test]
    fn a_row_without_a_source_is_refused() {
        assert_eq!(validate(&[row()]), Ok(()));
        let mut a = row();
        a.water_source = "  ".into();
        assert!(validate(&[a]).is_err_and(|e| e.contains("water_source")));
        let mut b = row();
        b.drain_source.clear();
        assert!(validate(&[b]).is_err_and(|e| e.contains("drain_source")));
    }

    #[test]
    fn a_row_with_a_bad_number_or_a_repeated_kind_is_refused() {
        let mut a = row();
        a.cold_flow = f64::NAN;
        assert!(validate(&[a]).is_err_and(|e| e.contains("cold_flow")));
        let mut b = row();
        b.hot_flow = Some(-1.0);
        assert!(validate(&[b]).is_err_and(|e| e.contains("hot_flow")));
        assert!(validate(&[row(), row()]).is_err_and(|e| e.contains("repeated")));
    }
}
