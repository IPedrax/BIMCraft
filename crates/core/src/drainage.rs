//! Sanitary drains and vents sized as NBR 8160 (Sistemas prediais de esgoto sanitário) asks, by the fixture units (UHC, "unidades
//! Hunter de contribuição") they carry. Pure functions of their input: the `drain_size`, `vent_size` and `trap_distance` commands are
//! their callers, so none of this needs a pipe network in the model. The tables are the standard's own:
//!
//! - Tabela 3: the units of each fixture and the smallest DN of its drain.
//! - Tabela 5: branches ("ramais de esgoto"), DN 40, 50, 75 and 100.
//! - Tabela 7: building drains and collectors ("subcoletores e coletor predial") by slope: 0.5, 1, 2 and 4 %.
//! - Tabela 6: stacks ("tubos de queda") by the building's floors: up to three, or more than three.
//! - Tabela 8: branch vents ("ramais de ventilação"), by the units of the group they vent and whether a water closet is in it.
//! - Tabela 2: vent stacks and headers ("colunas e barriletes de ventilação"), by the DN of the stack served, its units, and the
//!   longest length each vent DN may have.
//! - Tabela 1: the longest run from a trap to its vent, by the trap's DN.
//!
//! Source: NBR 8160 is a paid ABNT standard. These are its numbers (units, DNs, lengths) transcribed from a licensed copy "as
//! printed"; no clause text is copied. They are limits chosen by the standard's authors, tabulated per DN and slope, with no formula
//! that yields them, so they stay tables. A blank cell of a table means "not allowed" and is kept as 0, never filled from another
//! table. Tabela 7 is complete, DN 100 to 400, as printed (checked against the standard's page).

use serde::{Deserialize, Serialize};

/// A row of the vent stacks' table: the DN served, the units up to, and by vent DN the longest length (m).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VentRow {
    pub served: f64,
    pub units: f64,
    pub longest: Vec<[f64; 2]>,
}

/// The tables drains and vents are sized by: the Plumbing tab's Sanitary Drainage section (`data/codes/plumbing/nbr.json`). A
/// blank of a table (not allowed) is 0, never filled from another table.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrainageRules {
    /// The DNs of the tables, smallest first.
    pub dns: Vec<f64>,
    /// The smallest DN of a building drain.
    pub main_min_dn: f64,
    /// Branches: [DN, units].
    pub branch: Vec<[f64; 2]>,
    /// The slopes of the building drains' columns, as fractions.
    pub main_slopes: Vec<f64>,
    /// Building drains and collectors: [DN, units at each slope…].
    pub main: Vec<Vec<f64>>,
    /// Stacks: [DN, units up to three floors, units above].
    pub stack: Vec<[f64; 3]>,
    /// Branch vents: [units up to, DN], without and with a water closet in the group.
    pub branch_vent_no_wc: Vec<[f64; 2]>,
    pub branch_vent_wc: Vec<[f64; 2]>,
    pub vent_stacks: Vec<VentRow>,
    /// A trap's DN and the longest run from it to its vent, mm.
    pub trap_to_vent: Vec<[f64; 2]>,
}

impl crate::codes::Section for DrainageRules {
    const DISCIPLINE: crate::codes::Discipline = crate::codes::Discipline::Plumbing;
    const KEY: &'static str = "drainage";
    fn check(&self) -> Result<(), String> {
        let ok = |v: &f64| v.is_finite() && (0.0..=1.0e6).contains(v);
        let rows = self.branch.len() + self.main.len() + self.stack.len() + self.vent_stacks.len() + self.trap_to_vent.len();
        if self.dns.is_empty() || self.dns.len() > 100 || rows > 2000 || self.vent_stacks.iter().any(|r| r.longest.len() > 100) {
            return Err("drainage: some DNs, and 2000 rows at most".into());
        }
        let all = self
            .dns
            .iter()
            .chain([&self.main_min_dn])
            .chain(self.branch.iter().flatten())
            .chain(&self.main_slopes)
            .chain(self.main.iter().flatten())
            .chain(self.stack.iter().flatten())
            .chain(self.branch_vent_no_wc.iter().flatten())
            .chain(self.branch_vent_wc.iter().flatten())
            .chain(self.vent_stacks.iter().flat_map(|r| [&r.served, &r.units].into_iter().chain(r.longest.iter().flatten())))
            .chain(self.trap_to_vent.iter().flatten());
        let mut all = all;
        if !all.all(ok) {
            return Err("drainage: every number of the tables is from 0 to a million".into());
        }
        if self.dns.windows(2).any(|w| w[0] >= w[1]) || self.main_slopes.windows(2).any(|w| w[0] >= w[1]) {
            return Err("drainage: `dns` and `main_slopes` go from the smallest up".into());
        }
        if self.main.iter().any(|r| r.len() != self.main_slopes.len() + 1) {
            return Err("drainage: each row of `main` is a DN and the units at each of `main_slopes`".into());
        }
        Ok(())
    }
}

fn find(table: &[[f64; 2]], dn: f64) -> Option<f64> {
    table.iter().find(|r| (r[0] - dn).abs() < 1e-9).map(|r| r[1])
}

impl DrainageRules {
    /// The units a drain of `dn` carries: a branch's, a building drain's by slope, a stack's by the building's `floors`. 0
    /// where the table has none: a blank means "not allowed", and a branch never borrows the building drains' table.
    pub fn capacity(&self, dn: f64, slope: f64, role: Role, floors: u32) -> f64 {
        match role {
            Role::Stack => self.stack.iter().find(|r| (r[0] - dn).abs() < 1e-9).map_or(0.0, |r| if floors > 3 { r[2] } else { r[1] }),
            Role::Branch => find(&self.branch, dn).unwrap_or(0.0),
            Role::Main => {
                let column = self.main_slopes.iter().rposition(|s| slope + 1e-9 >= *s);
                let row = self.main.iter().find(|r| r.first().is_some_and(|d| (d - dn).abs() < 1e-9));
                column.and_then(|c| row.and_then(|r| r.get(c + 1).copied())).unwrap_or(0.0)
            }
        }
    }

    /// The smallest DN that carries `units`, at least `floor` (and, a building drain, at least `main_min_dn`); none when
    /// the tables end.
    pub fn smallest(&self, units: f64, slope: f64, role: Role, floors: u32, floor: f64) -> Option<f64> {
        let floor = if role == Role::Main { floor.max(self.main_min_dn) } else { floor };
        self.dns.iter().copied().find(|dn| *dn + 1e-9 >= floor && self.capacity(*dn, slope, role, floors) >= units)
    }

    /// A branch vent's DN for the units of the group it vents (none: more than the table gives).
    pub fn branch_vent(&self, units: f64, with_wc: bool) -> Option<f64> {
        let table = if with_wc { &self.branch_vent_wc } else { &self.branch_vent_no_wc };
        table.iter().find(|r| units <= r[0]).map(|r| r[1])
    }

    fn vent_row(&self, served_dn: f64, units: f64) -> Option<&VentRow> {
        self.vent_stacks.iter().find(|r| (r.served - served_dn).abs() < 1e-9 && units <= r.units)
    }

    /// A vent stack's smallest DN for the stack (or drain) it serves, its units and its length in metres.
    pub fn vent_stack(&self, served_dn: f64, units: f64, length_m: f64) -> Option<f64> {
        let row = self.vent_row(served_dn, units)?;
        self.dns.iter().copied().find(|dn| find(&row.longest, *dn).is_some_and(|max| max + 1e-9 >= length_m))
    }

    /// The longest length (m) a vent of `dn` may have for the stack (or drain) served and its units.
    pub fn vent_stack_limit(&self, served_dn: f64, units: f64, dn: f64) -> Option<f64> {
        find(&self.vent_row(served_dn, units)?.longest, dn)
    }

    /// The longest run from a trap of `dn` to its vent, mm.
    pub fn trap_distance(&self, dn: f64) -> Option<f64> {
        find(&self.trap_to_vent, dn)
    }
}

fn library() -> std::sync::Arc<DrainageRules> {
    crate::codes::library_rules::<DrainageRules>()
}

/// What a drain is: a branch from fixtures, a building drain or collector, or a stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Branch,
    Main,
    Stack,
}

impl Role {
    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "branch" => Some(Role::Branch),
            "main" => Some(Role::Main),
            "stack" => Some(Role::Stack),
            _ => None,
        }
    }
}

/// The units a drain of `dn` carries, by the library's tables.
pub fn capacity(dn: f64, slope: f64, role: Role, floors: u32) -> f64 {
    library().capacity(dn, slope, role, floors)
}

/// The smallest DN that carries `units`, by the library's tables.
pub fn smallest(units: f64, slope: f64, role: Role, floors: u32, floor: f64) -> Option<f64> {
    library().smallest(units, slope, role, floors, floor)
}

/// A branch vent's DN, by the library's table.
pub fn branch_vent(units: f64, with_wc: bool) -> Option<f64> {
    library().branch_vent(units, with_wc)
}

/// A vent stack's smallest DN, by the library's table.
pub fn vent_stack(served_dn: f64, units: f64, length_m: f64) -> Option<f64> {
    library().vent_stack(served_dn, units, length_m)
}

/// The longest length (m) a vent of `dn` may have, by the library's table.
pub fn vent_stack_limit(served_dn: f64, units: f64, dn: f64) -> Option<f64> {
    library().vent_stack_limit(served_dn, units, dn)
}

/// The longest run from a trap of `dn` to its vent, mm, by the library's table.
pub fn trap_distance(dn: f64) -> Option<f64> {
    library().trap_distance(dn)
}

/// The units of a fixture kind and the smallest DN of its drain (Tabela 3, from the fixtures library).
pub fn fixture(kind: &str) -> Option<(f64, f64)> {
    crate::plumbing::fixture(kind).map(|f| (f.uhc, f.drain_dn))
}

#[cfg(test)]
mod tests;
