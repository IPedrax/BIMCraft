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

/// The DNs of the standard's tables, smallest first.
pub const DNS: &[f64] = &[40.0, 50.0, 75.0, 100.0, 150.0, 200.0, 250.0, 300.0, 400.0];

/// The smallest DN of a building drain ("coletor predial").
pub const MAIN_MIN_DN: f64 = 100.0;

/// Tabela 5: the units a branch of each DN carries.
const BRANCH: &[(f64, f64)] = &[(40.0, 3.0), (50.0, 6.0), (75.0, 20.0), (100.0, 160.0)];

/// Tabela 7's slopes (fractions) and, by DN, the units a building drain carries at each (0: not allowed at that slope).
const MAIN_SLOPES: [f64; 4] = [0.005, 0.01, 0.02, 0.04];
const MAIN: &[(f64, [f64; 4])] = &[
    (100.0, [0.0, 180.0, 216.0, 250.0]),
    (150.0, [0.0, 700.0, 840.0, 1000.0]),
    (200.0, [1400.0, 1600.0, 1920.0, 2300.0]),
    (250.0, [2500.0, 2900.0, 3500.0, 4200.0]),
    (300.0, [3900.0, 4600.0, 5600.0, 6700.0]),
    (400.0, [7000.0, 8300.0, 10000.0, 12000.0]),
];

/// Tabela 6: the units a stack of each DN carries: (DN, up to three floors, more than three).
const STACK: &[(f64, f64, f64)] = &[
    (40.0, 4.0, 8.0),
    (50.0, 10.0, 24.0),
    (75.0, 30.0, 70.0),
    (100.0, 240.0, 500.0),
    (150.0, 960.0, 1900.0),
    (200.0, 2200.0, 3600.0),
    (250.0, 3800.0, 5600.0),
    (300.0, 6000.0, 8400.0),
];

/// Tabela 8: a group of fixtures' units up to which a branch vent has the DN, without and with a water closet in the group.
const BRANCH_VENT_NO_WC: &[(f64, f64)] = &[(12.0, 40.0), (18.0, 50.0), (36.0, 75.0)];
const BRANCH_VENT_WC: &[(f64, f64)] = &[(17.0, 50.0), (60.0, 75.0)];

/// Tabela 2: (DN of the stack or drain served, units up to, and by vent DN the longest length in metres).
/// One row of Tabela 2: the DN served, its units, and by vent DN the longest length (m).
type VentRow = (f64, f64, &'static [(f64, f64)]);
const VENT_STACKS: &[VentRow] = &[
    (40.0, 8.0, &[(40.0, 46.0)]),
    (40.0, 10.0, &[(40.0, 30.0)]),
    (50.0, 12.0, &[(40.0, 23.0), (50.0, 61.0)]),
    (50.0, 20.0, &[(40.0, 15.0), (50.0, 46.0)]),
    (75.0, 10.0, &[(40.0, 13.0), (50.0, 46.0), (75.0, 317.0)]),
    (75.0, 21.0, &[(40.0, 10.0), (50.0, 33.0), (75.0, 247.0)]),
    (75.0, 53.0, &[(40.0, 8.0), (50.0, 29.0), (75.0, 207.0)]),
    (75.0, 102.0, &[(40.0, 8.0), (50.0, 26.0), (75.0, 189.0)]),
    (100.0, 43.0, &[(50.0, 11.0), (75.0, 76.0), (100.0, 299.0)]),
    (100.0, 140.0, &[(50.0, 8.0), (75.0, 61.0), (100.0, 229.0)]),
    (100.0, 320.0, &[(50.0, 7.0), (75.0, 52.0), (100.0, 195.0)]),
    (100.0, 530.0, &[(50.0, 6.0), (75.0, 46.0), (100.0, 177.0)]),
    (150.0, 500.0, &[(75.0, 10.0), (100.0, 40.0), (150.0, 305.0)]),
    (150.0, 1100.0, &[(75.0, 8.0), (100.0, 31.0), (150.0, 238.0)]),
    (150.0, 2000.0, &[(75.0, 7.0), (100.0, 26.0), (150.0, 201.0)]),
    (150.0, 2900.0, &[(75.0, 6.0), (100.0, 23.0), (150.0, 183.0)]),
    (200.0, 1800.0, &[(100.0, 10.0), (150.0, 73.0), (200.0, 286.0)]),
    (200.0, 3400.0, &[(100.0, 7.0), (150.0, 57.0), (200.0, 219.0)]),
    (200.0, 5600.0, &[(100.0, 6.0), (150.0, 49.0), (200.0, 186.0)]),
    (200.0, 7600.0, &[(100.0, 5.0), (150.0, 43.0), (200.0, 171.0)]),
    (250.0, 4000.0, &[(150.0, 24.0), (200.0, 94.0), (250.0, 293.0)]),
    (250.0, 7200.0, &[(150.0, 18.0), (200.0, 73.0), (250.0, 225.0)]),
    (250.0, 11000.0, &[(150.0, 16.0), (200.0, 60.0), (250.0, 192.0)]),
    (250.0, 15000.0, &[(150.0, 14.0), (200.0, 55.0), (250.0, 174.0)]),
    (300.0, 7300.0, &[(150.0, 9.0), (200.0, 37.0), (250.0, 116.0), (300.0, 287.0)]),
    (300.0, 13000.0, &[(150.0, 7.0), (200.0, 29.0), (250.0, 90.0), (300.0, 219.0)]),
    (300.0, 20000.0, &[(150.0, 6.0), (200.0, 24.0), (250.0, 76.0), (300.0, 186.0)]),
    (300.0, 26000.0, &[(150.0, 5.0), (200.0, 22.0), (250.0, 70.0), (300.0, 152.0)]),
];

/// Tabela 1: a trap's DN and the longest run from it to its vent, in millimetres.
pub const TRAP_TO_VENT: &[(f64, f64)] = &[(40.0, 1000.0), (50.0, 1200.0), (75.0, 1800.0), (100.0, 2400.0)];

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

fn find<T: Copy>(table: &[(f64, T)], dn: f64) -> Option<T> {
    table.iter().find(|r| (r.0 - dn).abs() < 1e-9).map(|r| r.1)
}

/// The units a drain of `dn` carries: a branch from Tabela 5, a building drain from Tabela 7 by slope, a stack from Tabela 6 by the
/// building's `floors`. 0 when the table has none for it: a blank in Tabela 7 (DN 100 and 150 at 0.5 %, a slope flatter than 0.5 %)
/// means "not allowed", and a branch never borrows the building-drain table, nor the other way round.
pub fn capacity(dn: f64, slope: f64, role: Role, floors: u32) -> f64 {
    match role {
        Role::Stack => STACK.iter().find(|r| (r.0 - dn).abs() < 1e-9).map_or(0.0, |r| if floors > 3 { r.2 } else { r.1 }),
        Role::Branch => find(BRANCH, dn).unwrap_or(0.0),
        Role::Main => {
            let column = MAIN_SLOPES.iter().rposition(|s| slope + 1e-9 >= *s);
            column.and_then(|c| find(MAIN, dn).and_then(|row| row.get(c).copied())).unwrap_or(0.0)
        }
    }
}

/// The smallest DN that carries `units`, at least `floor` (and, a building drain, at least [`MAIN_MIN_DN`]); none when the tables end.
pub fn smallest(units: f64, slope: f64, role: Role, floors: u32, floor: f64) -> Option<f64> {
    let floor = if role == Role::Main { floor.max(MAIN_MIN_DN) } else { floor };
    DNS.iter().copied().find(|dn| *dn + 1e-9 >= floor && capacity(*dn, slope, role, floors) >= units)
}

/// A branch vent's DN for the units of the group it vents (none: more than Tabela 8 gives).
pub fn branch_vent(units: f64, with_wc: bool) -> Option<f64> {
    let table = if with_wc { BRANCH_VENT_WC } else { BRANCH_VENT_NO_WC };
    table.iter().find(|r| units <= r.0).map(|r| r.1)
}

/// A vent stack's smallest DN among the DNs for the stack (or drain) it serves, its units and its length in metres.
pub fn vent_stack(served_dn: f64, units: f64, length_m: f64) -> Option<f64> {
    let row = VENT_STACKS.iter().find(|r| (r.0 - served_dn).abs() < 1e-9 && units <= r.1)?;
    DNS.iter().copied().find(|dn| find(row.2, *dn).is_some_and(|max| max + 1e-9 >= length_m))
}

/// The longest length (m) a vent of `dn` may have for the stack (or drain) served and its units.
pub fn vent_stack_limit(served_dn: f64, units: f64, dn: f64) -> Option<f64> {
    let row = VENT_STACKS.iter().find(|r| (r.0 - served_dn).abs() < 1e-9 && units <= r.1)?;
    find(row.2, dn)
}

/// The longest run from a trap of `dn` to its vent, mm.
pub fn trap_distance(dn: f64) -> Option<f64> {
    find(TRAP_TO_VENT, dn)
}

/// The units of a fixture kind and the smallest DN of its drain (Tabela 3, from the fixtures library, `data/plumbing.json`).
pub fn fixture(kind: &str) -> Option<(f64, f64)> {
    crate::plumbing::fixture(kind).map(|f| (f.uhc, f.drain_dn))
}

#[cfg(test)]
mod tests;
