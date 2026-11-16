//! Hydrants and hose reels ("hidrantes e mangotinhos") sized as Instrução Técnica 22/2025 of the Corpo de Bombeiros of São Paulo
//! (CBPMESP, updated by Portaria CCB 003/800/25) asks. Pure functions of their input: the `hydrant_size` command is their caller,
//! so none of this needs a pipe network in the model. The numbers are the IT's own tables:
//!
//! - Tabela 2: the system types (1 to 5): nozzle and hose DN, the hose's length, the flow of one jet (L/min) and the head it needs
//!   at the hydrant's nozzle (mca). Type 4 comes with a DN 40 or a DN 65 nozzle.
//! - Tabela 3: the type of system and the fire reserve (RTI, m³) by the occupation's column (1 to 5) and the built area's band.
//! - Tabela 1: Hazen-Williams' factor C by the pipe's material (new pipes); the head loss is item 4.8.10.2's
//!   `J = 605 Q^1.85 C^-1.85 D^-4.87 10^4` (m/m, Q in L/min, D the inner diameter in mm).
//! - Item 4.8.3: the design flow is that of the two most unfavourable jets; 4.8.12 limits the speed to 5 m/s (4.8.11 is the pump's
//!   suction pipe, 2 m/s with negative suction and 3 m/s with positive, and is not checked here); 4.8.8 the
//!   head at a nozzle to 100 mca; 4.11.6 the smallest pipe to DN 65 (DN 50 for types 1 and 2).
//!
//! This is the rule of São Paulo only (IT 22/2025 of the CBPMESP); no other state's. The band edges of Tabela 3 are taken as
//! inclusive above (`area <= 2500` is the first band): the IT's rows read "Até 2.500 m²", "Acima de 2.500 até 5.000 m²" and so on. Its
//! notes 1 and 2 (a system of one type lower where the occupation has, or has not, sprinklers) are not applied. What `need_mca` covers is
//! the nozzle head, the straight pipe's loss and the rise: no hose or fitting loss, and it checks no pressure at the source.

use serde::{Deserialize, Serialize};

/// One system type of Tabela 2: its key, type, nozzle and hose DN (mm), the hose's length (m), one jet's flow (L/min) and the
/// head it needs at the nozzle (mca).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemType {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: u8,
    pub nozzle_dn: f64,
    pub hose_dn: f64,
    pub hose_length: f64,
    pub jet_flow: f64,
    pub nozzle_head: f64,
}

/// A pipe material's Hazen-Williams C.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HazenC {
    pub material: String,
    pub c: f64,
}

/// The rules hydrants and hose reels are sized by: the Plumbing tab's Hydrants section (CBPMESP IT 22, an amendment in
/// `data/codes/plumbing/cbpmesp-it22.json`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HydrantRules {
    pub types: Vec<SystemType>,
    /// Upper limits of the area bands (m²), inclusive; the last band is above the last limit.
    pub area_bands: Vec<f64>,
    /// The type and the fire reserve (m³) by occupation column (1, 2…), one [type, reserve] per band, smallest area first.
    pub reserve: Vec<Vec<(u8, f64)>>,
    pub hazen_c: Vec<HazenC>,
    /// The fastest water may go (m/s), the most head at a nozzle (mca), and how many jets the design flow takes.
    pub max_speed: f64,
    pub max_nozzle_head: f64,
    pub jets: u32,
    /// The smallest pipe DN (mm), and the smaller one types up to `small_types_up_to` may have.
    pub min_dn: f64,
    pub min_dn_small: f64,
    pub small_types_up_to: u8,
}

impl crate::codes::Section for HydrantRules {
    const DISCIPLINE: crate::codes::Discipline = crate::codes::Discipline::Plumbing;
    const KEY: &'static str = "hydrants";
    fn check(&self) -> Result<(), String> {
        let positive = |v: f64| v.is_finite() && v > 0.0 && v < 1.0e6;
        if self.types.is_empty()
            || self.types.len() > 50
            || self.reserve.len() > 50
            || self.hazen_c.len() > 50
            || self.area_bands.len() > 50
        {
            return Err("hydrants: some types, and 50 rows of each table at most".into());
        }
        for t in &self.types {
            if t.key.is_empty()
                || t.key.chars().count() > 16
                || ![t.nozzle_dn, t.hose_dn, t.hose_length, t.jet_flow, t.nozzle_head].into_iter().all(positive)
            {
                return Err(format!("hydrants: type `{}` has a key and positive sizes, flow and head", t.key));
            }
        }
        if self.area_bands.windows(2).any(|w| w[0] >= w[1]) || !self.area_bands.iter().all(|v| positive(*v)) {
            return Err("hydrants: `area_bands` are positive, from the smallest up".into());
        }
        if self.reserve.iter().any(|col| {
            col.len() != self.area_bands.len() + 1
                || col.iter().any(|(t, r)| !self.types.iter().any(|x| x.kind == *t) || !r.is_finite() || *r < 0.0)
        }) {
            return Err("hydrants: each column of `reserve` has a [type, m³] for each band and one above, of types there are".into());
        }
        if !self.hazen_c.iter().all(|h| positive(h.c))
            || ![self.max_speed, self.max_nozzle_head, self.min_dn, self.min_dn_small].into_iter().all(positive)
            || self.jets == 0
            || self.jets > 100
        {
            return Err("hydrants: the factors C, the limits and the least DNs are positive, and 1 to 100 jets".into());
        }
        Ok(())
    }
}

impl HydrantRules {
    /// A system type (its type) and the fire reserve (m³) of an occupation column for a built area (m²), or `None` for a
    /// column or an area the table does not have.
    pub fn system_for(&self, column: u8, area: f64) -> Option<(u8, f64)> {
        let list = self.reserve.get(usize::from(column).checked_sub(1)?)?;
        if area.is_nan() || area <= 0.0 {
            return None;
        }
        let band = self.area_bands.iter().position(|u| area <= *u).unwrap_or(self.area_bands.len());
        list.get(band).copied()
    }

    /// A system type by key.
    pub fn kind(&self, key: &str) -> Option<&SystemType> {
        self.types.iter().find(|t| t.key == key)
    }

    /// Hazen-Williams' C for a material.
    pub fn hazen_c(&self, material: &str) -> Option<f64> {
        self.hazen_c.iter().find(|m| m.material == material).map(|m| m.c)
    }

    /// The design flow (L/min): the most unfavourable jets of the type, `jets` at most.
    pub fn design_flow(&self, key: &str, hydrants: u32) -> f64 {
        self.kind(key).map_or(0.0, |t| t.jet_flow * f64::from(hydrants.min(self.jets)))
    }

    /// The smallest nominal diameter (mm) a hydrant pipe of the type may have.
    pub fn min_nominal(&self, key: &str) -> f64 {
        if self.kind(key).is_some_and(|t| t.kind <= self.small_types_up_to) { self.min_dn_small } else { self.min_dn }
    }
}

fn library() -> std::sync::Arc<HydrantRules> {
    crate::codes::library_rules::<HydrantRules>()
}

/// A system type and the fire reserve, by the library's Tabela 3.
pub fn system_for(column: u8, area: f64) -> Option<(u8, f64)> {
    library().system_for(column, area)
}

/// Hazen-Williams' C for a material, by the library's table.
pub fn hazen_c(material: &str) -> Option<f64> {
    library().hazen_c(material)
}

/// `J` (m of water per m) for `q` L/min in an inner diameter `d` mm.
pub fn hazen_j(q: f64, c: f64, d: f64) -> f64 {
    605.0 * q.powf(1.85) * c.powf(-1.85) * d.powf(-4.87) * 1e4
}

/// Speed (m/s) for `q` L/min in an inner diameter `d` mm.
pub fn speed(q: f64, d: f64) -> f64 {
    (q / 60000.0) / (std::f64::consts::PI * (d / 1000.0).powi(2) / 4.0)
}

/// The design flow (L/min), by the library's rules.
pub fn design_flow(key: &str, hydrants: u32) -> f64 {
    library().design_flow(key, hydrants)
}

/// The smallest nominal diameter (mm), by the library's rules.
pub fn min_nominal(key: &str) -> f64 {
    library().min_nominal(key)
}

#[cfg(test)]
mod tests;
