//! The categories of a low-voltage supply, as data: what a utility's table gives a unit for its installed load or its demand (the
//! breaker, the branches, the earthing, the measurement). Read from `data/entrance-categories.json`; the `demand` command is the only
//! caller, for a utility it finds there. `demand.rs` keeps the utilities whose tables came before this file (CPFL, ENEL, CELESC).
//!
//! The utilities' tables agree on a shape, which is what lets them be data:
//!
//! - A table is one supply system (`127/220`, `220/380`, …, the lower voltage first) and one use (`individual`, `metering_centre`), with its rows in the
//!   order printed.
//! - A row is a category: its name, its phases, the installed load (kW) and the demand (kVA) it covers, each a range over `over` up
//!   to `up_to` where the table prints one, its breaker (more than one value where the table lets the designer take either), its
//!   measurement, and the other columns as printed, under names the utility's `columns` explain.
//! - A group of utilities that share one standard (Rio Grande do Sul's cooperatives and Fecoergs' RIC) lists its members; a member
//!   is found by its own name and may change a category by a document of its own.
//!
//! A row holds a unit when every range it prints holds it and its phases are those asked, if any. The demand is the designer's,
//! carried as given: a utility that works one out has its own module. A utility is added as an entry of the file, never as code.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// A range over `over` (from 0 inclusive when `over` is 0) up to `up_to`, inclusive.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
    #[serde(default)]
    pub over: f64,
    pub up_to: f64,
}

impl Range {
    pub fn holds(&self, x: f64) -> bool {
        (x > self.over || (self.over == 0.0 && x >= 0.0)) && x <= self.up_to
    }
}

/// A category of supply, one row of a table.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub name: String,
    pub phases: u8,
    #[serde(default)]
    pub installed_kw: Option<Range>,
    #[serde(default)]
    pub demand_kva: Option<Range>,
    /// The breaker, A: one value, or the values the table lets the designer choose between.
    pub breaker_a: Vec<f64>,
    #[serde(default)]
    pub measurement: Option<String>,
    /// The other columns, as printed, under the names the utility's `columns` explain.
    #[serde(default)]
    pub columns: BTreeMap<String, serde_json::Value>,
}

/// A table of categories for one supply system and one use.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    pub system: String,
    #[serde(rename = "use")]
    pub use_: String,
    /// Where in the standard: its annex or table, and its page.
    pub source: String,
    pub rows: Vec<Row>,
}

/// A member's document on one category: a value it changes, or, with none, an exception it allows that the source says.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub system: String,
    #[serde(rename = "use")]
    pub use_: String,
    pub category: String,
    #[serde(default)]
    pub measurement: Option<String>,
    pub source: String,
}

/// A utility of a group that shares one standard.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub id: String,
    pub seat: String,
    #[serde(default)]
    pub changes: Vec<Change>,
}

/// A utility, or a group of them, and its standard's tables.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Utility {
    pub id: String,
    pub name: String,
    pub standard: String,
    pub source: String,
    /// How the standard picks a category, in words.
    pub rule: String,
    #[serde(default)]
    pub members: Vec<Member>,
    /// What each of the rows' `columns` is.
    pub columns: BTreeMap<String, String>,
    pub tables: Vec<Table>,
    #[serde(default)]
    pub notes: Vec<String>,
    pub not_covered: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    utilities: Vec<Utility>,
}

/// Every utility of the file.
pub fn utilities() -> &'static [Utility] {
    static ALL: OnceLock<Vec<Utility>> = OnceLock::new();
    ALL.get_or_init(|| {
        serde_json::from_str::<Data>(include_str!("../data/entrance-categories.json")).map(|d| d.utilities).unwrap_or_default()
    })
}

/// A utility by its id or by a member's (in any case), and the member when it is one.
pub fn find(name: &str) -> Option<(&'static Utility, Option<&'static Member>)> {
    let all = utilities();
    if let Some(u) = all.iter().find(|u| u.id.eq_ignore_ascii_case(name)) {
        return Some((u, None));
    }
    all.iter().find_map(|u| u.members.iter().find(|m| m.id.eq_ignore_ascii_case(name)).map(|m| (u, Some(m))))
}

/// Every name `find` knows, groups and members.
pub fn names() -> Vec<&'static str> {
    utilities().iter().flat_map(|u| std::iter::once(u.id.as_str()).chain(u.members.iter().map(|m| m.id.as_str()))).collect()
}

impl Utility {
    /// The systems a use has tables for, in the file's order.
    pub fn systems(&self, use_: &str) -> Vec<&str> {
        self.tables.iter().filter(|t| t.use_ == use_).map(|t| t.system.as_str()).collect()
    }

    /// The uses its tables cover, in the file's order, each once.
    pub fn uses(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for t in &self.tables {
            if !out.contains(&t.use_.as_str()) {
                out.push(&t.use_);
            }
        }
        out
    }

    pub fn table(&self, system: &str, use_: &str) -> Option<&Table> {
        self.tables.iter().find(|t| t.system == system && t.use_ == use_)
    }
}

/// What a unit is, for picking its category.
#[derive(Clone, Copy, Debug, Default)]
pub struct Unit {
    pub installed_kw: Option<f64>,
    pub demand_kva: Option<f64>,
    pub phases: Option<u8>,
}

/// What a table gives a unit.
#[derive(Debug)]
pub struct Pick {
    /// The first row that holds the unit, with a member's changes made.
    pub category: Option<Row>,
    /// Every row that holds it, by name, when more than one does.
    pub candidates: Vec<String>,
    /// The rows that might hold it but need what it was not given: the installed load or the demand.
    pub needs: Vec<(String, &'static str)>,
    /// The member's documents on the category, by their sources: a change made, or one it allows as an exception.
    pub changed_by: Vec<String>,
}

/// Whether a range holds a value: yes, no, or not known for the value's lack.
fn fits(range: Option<Range>, x: Option<f64>) -> Option<bool> {
    match (range, x) {
        (None, _) => Some(true),
        (Some(r), Some(x)) => Some(r.holds(x)),
        (Some(_), None) => None,
    }
}

/// The category a table gives a unit, with a member's changes made.
pub fn pick(table: &Table, member: Option<&Member>, unit: Unit) -> Pick {
    let mut holds = Vec::new();
    let mut needs = Vec::new();
    for row in &table.rows {
        if unit.phases.is_some_and(|p| p != row.phases) {
            continue;
        }
        match (fits(row.installed_kw, unit.installed_kw), fits(row.demand_kva, unit.demand_kva)) {
            (Some(true), Some(true)) => holds.push(row),
            (Some(false), _) | (_, Some(false)) => {}
            (None, _) => needs.push((row.name.clone(), "installed_kw")),
            (_, None) => needs.push((row.name.clone(), "demand_kva")),
        }
    }
    let candidates = if holds.len() > 1 { holds.iter().map(|r| r.name.clone()).collect() } else { Vec::new() };
    let mut changed_by = Vec::new();
    let category = holds.first().map(|r| {
        let mut row = (*r).clone();
        for change in member.map(|m| m.changes.as_slice()).unwrap_or_default() {
            if change.system == table.system && change.use_ == table.use_ && change.category == row.name {
                if let Some(m) = &change.measurement {
                    row.measurement = Some(m.clone());
                }
                changed_by.push(change.source.clone());
            }
        }
        row
    });
    Pick { category, candidates, needs, changed_by }
}

#[cfg(test)]
mod tests;
