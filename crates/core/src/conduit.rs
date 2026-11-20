//! The fill of a conduit as NBR 5410:2004 asks: the sum of the cross-sections of the conductors it holds (phases, neutral and
//! protective, by their outside diameter, insulation included) over the conduit's inside cross-section, at most 53 % for one
//! cable, 31 % for two and 40 % for three or more. The limits, the makers' conduits and the cables' diameters are the
//! `conduit_fill` section of the electrical code (`data/codes/electrical/nbr-5410.json`), each family naming its maker,
//! document, page and address; the project may change any of them. A conduit's inside diameter is not printed in those
//! catalogues, so it is derived: the outside diameter less twice the wall. A cable the catalogue has no diameter for (EPR or
//! XLPE insulation, sections above 240 mm²) is not guessed: the fill is pending. Cable trays are not sized here.

use crate::codes::Steps;
use crate::nbr5410::{self, Phases};
use serde::Deserialize;
use std::sync::OnceLock;

/// A conduit size: its mark, DN in mm, outside diameter in mm and wall in mm.
pub type Size = (String, f64, f64, f64);

/// A conduit family: its label, its material, where the numbers are from, and its sizes smallest first.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Family {
    pub key: String,
    pub label: String,
    pub material: String,
    pub maker: String,
    pub document: String,
    pub page: String,
    pub url: String,
    pub sizes: Vec<Size>,
}

/// The rules of the fill: the `conduit_fill` section of an electrical code.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    /// The most a conduit may be filled, as a fraction, by the number of cables in it.
    pub limits: Steps,
    /// The family taken when none is named.
    pub default_family: String,
    pub families: Vec<Family>,
    /// A cable's outside diameter by section: (mm², mm), and where they are from.
    pub cable_diameters: Vec<(f64, f64)>,
    pub cable_source: String,
}

/// BIMCraft's fill rules, each electrical code's `conduit_fill` section: a project reads its own (`Model::section_as`).
pub fn rule_sets() -> &'static [Rules] {
    static SETS: OnceLock<Vec<Rules>> = OnceLock::new();
    SETS.get_or_init(|| crate::codes::library_sections(crate::codes::Discipline::Electrical, "conduit_fill"))
}

impl crate::codes::Valid for Rules {
    fn valid(&self) -> Result<(), String> {
        self.limits.valid("limits")?;
        if self.limits.rows.iter().map(|r| r.1).chain([self.limits.beyond]).any(|f| !(f > 0.0 && f <= 1.0)) {
            return Err("a fill limit is a fraction over 0, up to 1".into());
        }
        if !self.families.iter().any(|f| f.key == self.default_family) {
            return Err(format!("`default_family` {} is not one of `families`", self.default_family));
        }
        for f in &self.families {
            if f.sizes.iter().any(|s| ![s.1, s.2, s.3].iter().all(|x| x.is_finite() && *x >= 0.0) || s.2 - 2.0 * s.3 <= 0.0) {
                return Err(format!("{}: a size's diameters are finite, and the outside is more than twice the wall", f.key));
            }
        }
        if self.cable_diameters.iter().any(|c| !(c.0.is_finite() && c.1.is_finite() && c.0 > 0.0 && c.1 > 0.0)) {
            return Err("a cable's section and diameter are over 0".into());
        }
        Ok(())
    }
}

impl Rules {
    /// The most a conduit may be filled, as a fraction, by the number of cables in it.
    pub fn limit(&self, cables: usize) -> f64 {
        self.limits.at(cables.max(1) as f64)
    }

    pub fn family(&self, key: &str) -> Option<&Family> {
        self.families.iter().find(|f| f.key == key)
    }

    /// The outside diameter of a cable of `section` mm², when the catalogue has it.
    pub fn cable_diameter(&self, section: f64) -> Option<f64> {
        self.cable_diameters.iter().find(|c| (c.0 - section).abs() < 1e-9).map(|c| c.1)
    }

    /// The fill of `size` by `sections`; none when a cable has no diameter in the catalogue.
    pub fn fill(&self, size: &Size, sections: &[f64]) -> Option<Fill> {
        let mut area = 0.0;
        for s in sections {
            let d = self.cable_diameter(*s)?;
            area += std::f64::consts::PI * d * d / 4.0;
        }
        let d = inside(size);
        let inside = std::f64::consts::PI * d * d / 4.0;
        let (ratio, limit) = (area / inside, self.limit(sections.len()));
        Some(Fill { cables: sections.len(), area, inside, ratio, limit, ok: ratio <= limit + 1e-9 })
    }

    /// The smallest size of a family that takes `sections`, when there is one.
    pub fn smallest<'a>(&self, family: &'a Family, sections: &[f64]) -> Option<&'a Size> {
        family.sizes.iter().find(|s| self.fill(s, sections).is_some_and(|f| f.ok))
    }
}

/// The conductors a circuit puts in its conduit, as the section of each: the phases, the neutral and the protective conductor.
pub fn conductors(sizing: &nbr5410::Rules, phases: Phases, section: f64, reduced_neutral: bool) -> Vec<f64> {
    let mut out = vec![section; phases.count()];
    match phases {
        Phases::PhaseNeutral => out.push(section),
        Phases::ThreeNeutral => out.push(sizing.neutral_section(section, reduced_neutral)),
        _ => {}
    }
    out.push(sizing.protective_section(section));
    out
}

/// A size's inside diameter in mm: its outside diameter less twice its wall.
pub fn inside(size: &Size) -> f64 {
    size.2 - 2.0 * size.3
}

/// A conduit size filled by conductors of the given sections.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fill {
    pub cables: usize,
    /// The cables' cross-section and the conduit's inside, mm².
    pub area: f64,
    pub inside: f64,
    /// area / inside, and the most the standard allows.
    pub ratio: f64,
    pub limit: f64,
    pub ok: bool,
}

#[cfg(test)]
mod tests;
