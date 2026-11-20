//! A circuit sized as NBR 5410:2004 asks, for copper conductors: the design current, the smallest conductor section that carries
//! it once the temperature, grouping and soil corrections are applied and drops no more than the limit, the circuit breaker, and
//! the neutral and the protective conductor. A pure function of its input and of the rules, with no model behind it: the
//! `circuit` command is its caller, with the project's code. The tables are the `circuits` section of the electrical code
//! (`data/codes/electrical/nbr-5410.json`), each with its source; the project may change any of them.
//!
//! What is checked: `I_B <= I_n <= I_Z`. For a breaker of NBR NM 60898, whose conventional trip current is `I_2 = 1.45 I_n`, that
//! already gives `I_2 <= 1.45 I_Z`, the second condition of the standard, so it is not checked again. The smallest section for the use (Tabela 47), and the voltage drop `b * I_B * L * (R cos(phi) + X sin(phi))`, with `R` at the
//! insulation's highest working temperature (so on the safe side) and `X` the code's reactance. What is not: aluminium, cable trays (methods
//! E and F), several layers of cables, buried cables without a conduit, short circuit, harmonics, the conditions of a reduced
//! neutral. The caller says so.

use serde::Deserialize;
use std::sync::OnceLock;

/// Copper's temperature coefficient of resistance, per degree: physics.
const COPPER_ALPHA: f64 = 0.003_93;
const MAX_CIRCUITS: u32 = 1_000;

/// A voltage system a project may be at: its name, phase to neutral and phase to phase, volts.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct System {
    pub name: String,
    pub phase_v: f64,
    pub line_v: f64,
}

/// An installation method of Tabela 33 the tables cover: its number as the standard writes it, its reference method and the
/// row of the grouping tables it takes.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Method {
    pub method: String,
    pub reference: Reference,
    pub grouping: Group,
}

/// A value for each insulation.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByInsulation {
    pub pvc: f64,
    pub epr: f64,
}

/// A value for each use of a circuit.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByUse {
    pub lighting: f64,
    pub power: f64,
}

/// A row of Tabela 40: the temperature (°C), the factor for PVC and for EPR or XLPE (none: hotter than it allows).
pub type TemperatureRow = (f64, Option<f64>, Option<f64>);

/// The rules of the sizing: the `circuits` section of an electrical code.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    pub systems: Vec<System>,
    pub methods: Vec<Method>,
    /// The method taken when none is named.
    pub default_method: String,
    /// Tabelas 36 and 37: amperes by section (mm²), each row the reference methods A1, A2, B1, B2, C and D, two loaded
    /// conductors then three.
    pub ampacity_pvc: Vec<(f64, [f64; 12])>,
    pub ampacity_epr: Vec<(f64, [f64; 12])>,
    /// The insulation's highest working temperature, °C.
    pub highest_celsius: ByInsulation,
    pub temperature_air: Vec<TemperatureRow>,
    pub temperature_soil: Vec<TemperatureRow>,
    /// The coldest Tabela 40 holds, and the reference in air and in soil, °C.
    pub lowest_celsius: f64,
    pub reference_air_celsius: f64,
    pub reference_soil_celsius: f64,
    /// Tabela 41: (resistivity K·m/W, factor), and the resistivity of factor 1.
    pub soil_resistivity: Vec<(f64, f64)>,
    pub reference_soil_resistivity: f64,
    /// Tabela 42, 1 to 8 circuits, then (up to this many circuits, factor) and the factor past those.
    pub grouping_bundle: Vec<f64>,
    pub grouping_layer: Vec<f64>,
    pub grouping_ceiling: Vec<f64>,
    pub grouping_bundle_more: Vec<(u32, f64)>,
    pub grouping_bundle_most: f64,
    pub grouping_layer_more: f64,
    pub grouping_ceiling_more: f64,
    /// Tabela 45: the spacings (m) between buried conduits, and a row of 2 to `buried_most_circuits` circuits for each.
    pub buried_spacings_m: Vec<f64>,
    pub buried_multi: Vec<Vec<f64>>,
    pub buried_single: Vec<Vec<f64>>,
    pub buried_most_circuits: u32,
    /// Tabela 47, mm².
    pub minimum_section: ByUse,
    /// Tabela 48: a neutral as the phases up to this section, and (phase, reduced neutral) above it, mm².
    pub neutral_whole_up_to: f64,
    pub neutral_reduced: Vec<(f64, f64)>,
    /// Tabela 58: the phase's section up to `protective_whole_up_to`, `protective_fixed` up to `protective_fixed_up_to`, half the
    /// phase above, mm².
    pub protective_whole_up_to: f64,
    pub protective_fixed_up_to: f64,
    pub protective_fixed: f64,
    /// Resistance at 20 °C, Ω/km, by section: rigid and flexible conductors, and how much a flexible one is over a rigid one
    /// where only the rigid is listed.
    pub resistance_rigid: Vec<(f64, f64)>,
    pub resistance_flexible: Vec<(f64, f64)>,
    pub flexible_over_rigid: f64,
    /// Ω/km.
    pub reactance: f64,
    /// Per cent of the voltage.
    pub drop_limit: f64,
    pub sections: Vec<f64>,
    pub breakers: Vec<f64>,
    /// The least breaker a new circuit is given for its load, A.
    pub least_breaker: f64,
}

/// BIMCraft's sizing rules, each electrical code's `circuits` section: a project reads its own (`Model::section_as`).
pub fn rule_sets() -> &'static [Rules] {
    static SETS: OnceLock<Vec<Rules>> = OnceLock::new();
    SETS.get_or_init(|| crate::codes::library_sections(crate::codes::Discipline::Electrical, "circuits"))
}

impl crate::codes::Valid for Rules {
    fn valid(&self) -> Result<(), String> {
        fn finite(what: &str, mut xs: impl Iterator<Item = f64>) -> Result<(), String> {
            match xs.find(|x| !x.is_finite() || *x < 0.0) {
                Some(x) => Err(format!("`{what}` holds {x}: every number is finite and not negative")),
                None => Ok(()),
            }
        }
        if self.systems.is_empty() || self.methods.is_empty() || self.sections.is_empty() || self.breakers.is_empty() {
            return Err("`systems`, `methods`, `sections` and `breakers` each hold one at least".into());
        }
        if !self.methods.iter().any(|m| m.method == self.default_method) {
            return Err(format!("`default_method` {} is not one of `methods`", self.default_method));
        }
        if self.systems.iter().any(|s| !(s.phase_v > 0.0 && s.line_v > 0.0 && s.phase_v.is_finite() && s.line_v.is_finite())) {
            return Err("a system's voltages are over 0".into());
        }
        let ampacity = self.ampacity_pvc.iter().chain(&self.ampacity_epr);
        finite("ampacity", ampacity.flat_map(|r| std::iter::once(r.0).chain(r.1)))?;
        let pairs = [&self.soil_resistivity, &self.neutral_reduced, &self.resistance_rigid, &self.resistance_flexible];
        finite("a table", pairs.into_iter().flatten().flat_map(|r| [r.0, r.1]))?;
        let lists =
            [&self.grouping_bundle, &self.grouping_layer, &self.grouping_ceiling, &self.buried_spacings_m, &self.sections, &self.breakers];
        finite("a list", lists.into_iter().flatten().copied())?;
        let temps = self.temperature_air.iter().chain(&self.temperature_soil);
        finite("temperature", temps.flat_map(|r| [Some(r.0), r.1, r.2]).flatten())?;
        finite(
            "a rule",
            [
                self.highest_celsius.pvc,
                self.highest_celsius.epr,
                self.lowest_celsius,
                self.reference_air_celsius,
                self.reference_soil_celsius,
                self.reference_soil_resistivity,
                self.grouping_bundle_most,
                self.grouping_layer_more,
                self.grouping_ceiling_more,
                self.minimum_section.lighting,
                self.minimum_section.power,
                self.neutral_whole_up_to,
                self.protective_whole_up_to,
                self.protective_fixed_up_to,
                self.protective_fixed,
                self.flexible_over_rigid,
                self.reactance,
                self.drop_limit,
                self.least_breaker,
            ]
            .into_iter(),
        )?;
        if self.drop_limit <= 0.0 {
            return Err("`drop_limit` is over 0".into());
        }
        Ok(())
    }
}

/// The conductors a circuit has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phases {
    /// One phase and neutral.
    PhaseNeutral,
    /// Two phases.
    TwoPhases,
    /// Three phases, no neutral.
    Three,
    /// Three phases and neutral, balanced.
    ThreeNeutral,
}

impl Phases {
    pub fn parse(s: &str) -> Option<Phases> {
        match s {
            "FN" => Some(Phases::PhaseNeutral),
            "FF" => Some(Phases::TwoPhases),
            "3F" => Some(Phases::Three),
            "3FN" => Some(Phases::ThreeNeutral),
            _ => None,
        }
    }
    fn three(self) -> bool {
        matches!(self, Phases::Three | Phases::ThreeNeutral)
    }
    /// Loaded conductors, 2 or 3: the neutral of a balanced three-phase circuit does not count (6.2.5.6).
    pub fn loaded(self) -> usize {
        if self.three() { 3 } else { 2 }
    }
    pub fn count(self) -> usize {
        match self {
            Phases::PhaseNeutral => 1,
            Phases::TwoPhases => 2,
            Phases::Three | Phases::ThreeNeutral => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Insulation {
    /// PVC, 70 °C.
    Pvc,
    /// EPR or XLPE, 90 °C.
    Epr,
}

impl Insulation {
    pub fn parse(s: &str) -> Option<Insulation> {
        match s {
            "pvc" => Some(Insulation::Pvc),
            "epr" => Some(Insulation::Epr),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    Lighting,
    Power,
}

/// How the cables lie in the installation methods of Tabela 33 that Tabelas 36 and 37 hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Reference {
    A1,
    A2,
    B1,
    B2,
    C,
    D,
}

impl Reference {
    /// The column of the table's first loaded-conductors pair.
    fn column(self) -> usize {
        match self {
            Reference::A1 => 0,
            Reference::A2 => 2,
            Reference::B1 => 4,
            Reference::B2 => 6,
            Reference::C => 8,
            Reference::D => 10,
        }
    }
}

/// Which row of the grouping tables a method uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    Bundle,
    Layer,
    Ceiling,
    BuriedMulti,
    BuriedSingle,
}

impl Rules {
    /// The methods' numbers, as a command lists them.
    pub fn method_keys(&self) -> Vec<&str> {
        self.methods.iter().map(|m| m.method.as_str()).collect()
    }

    fn method(&self, key: &str) -> Option<(Reference, Group)> {
        self.methods.iter().find(|m| m.method == key).map(|m| (m.reference, m.grouping))
    }

    /// The system's voltages, the first system's for an index past them.
    fn system(&self, index: usize) -> (f64, f64) {
        self.systems.get(index).or(self.systems.first()).map_or((f64::NAN, f64::NAN), |s| (s.phase_v, s.line_v))
    }

    fn highest(&self, insulation: Insulation) -> f64 {
        match insulation {
            Insulation::Pvc => self.highest_celsius.pvc,
            Insulation::Epr => self.highest_celsius.epr,
        }
    }

    /// The current a conductor of `section` mm² carries in a method, amperes, with `loaded` (2 or 3) loaded conductors: Tabela 36 or 37.
    pub fn ampacity(&self, insulation: Insulation, section: f64, reference_method: &str, loaded: usize) -> Option<f64> {
        let (reference, _) = self.method(reference_method)?;
        self.ampacity_of(insulation, section, reference, loaded)
    }

    fn ampacity_of(&self, insulation: Insulation, section: f64, reference: Reference, loaded: usize) -> Option<f64> {
        let table = match insulation {
            Insulation::Pvc => &self.ampacity_pvc,
            Insulation::Epr => &self.ampacity_epr,
        };
        let row = table.iter().find(|r| (r.0 - section).abs() < 1e-9)?;
        row.1.get(reference.column() + usize::from(loaded >= 3)).copied()
    }

    /// Tabela 40's factor for an ambient (or soil) temperature in °C: none under the table or above what the insulation allows.
    pub fn temperature_factor(&self, insulation: Insulation, celsius: f64, soil: bool) -> Option<f64> {
        if celsius.is_nan() || celsius < self.lowest_celsius {
            return None;
        }
        let table = if soil { &self.temperature_soil } else { &self.temperature_air };
        let row = table.iter().find(|r| r.0 >= celsius - 1e-9)?;
        match insulation {
            Insulation::Pvc => row.1,
            Insulation::Epr => row.2,
        }
    }

    /// Tabela 41's factor for a soil's resistivity in K·m/W.
    pub fn soil_factor(&self, resistivity: f64) -> Option<f64> {
        let lowest = self.soil_resistivity.first()?.0;
        if resistivity.is_nan() || resistivity < lowest - 1e-9 {
            return None;
        }
        self.soil_resistivity.iter().find(|r| r.0 >= resistivity - 1e-9).map(|r| r.1)
    }

    /// Tabela 42's (and, buried, Tabela 45's) grouping factor for `circuits` in a method: `bundle` says the cables of a method C lie
    /// bundled rather than in a layer; `spacing` is the distance between buried conduits in metres.
    pub fn grouping_factor(&self, method_key: &str, bundle: bool, circuits: u32, spacing: f64) -> Option<f64> {
        let (reference, group) = self.method(method_key)?;
        let n = circuits.clamp(1, MAX_CIRCUITS);
        if matches!(group, Group::BuriedMulti | Group::BuriedSingle) {
            if n == 1 {
                return Some(1.0);
            }
            let widest = self.buried_spacings_m.last().copied().unwrap_or(0.0);
            if n > self.buried_most_circuits || spacing.is_nan() || !(0.0..=widest + 1e-9).contains(&spacing) {
                return None;
            }
            // The factors grow with the spacing, so between two printed columns the narrower one is the safe side.
            let at = self.buried_spacings_m.iter().rposition(|s| *s <= spacing + 1e-9)?;
            let table = if group == Group::BuriedMulti { &self.buried_multi } else { &self.buried_single };
            return table.get(at)?.get(n as usize - 2).copied();
        }
        let group = if reference == Reference::C && bundle { Group::Bundle } else { group };
        let head = match group {
            Group::Ceiling => &self.grouping_ceiling,
            Group::Layer => &self.grouping_layer,
            _ => &self.grouping_bundle,
        };
        if let Some(f) = head.get(n as usize - 1) {
            return Some(*f);
        }
        Some(match group {
            Group::Ceiling => self.grouping_ceiling_more,
            Group::Layer => self.grouping_layer_more,
            _ => self.grouping_bundle_more.iter().find(|r| n <= r.0).map_or(self.grouping_bundle_most, |r| r.1),
        })
    }

    /// Tabela 47: the smallest section, mm².
    pub fn minimum_section(&self, use_: Use) -> f64 {
        match use_ {
            Use::Lighting => self.minimum_section.lighting,
            Use::Power => self.minimum_section.power,
        }
    }

    /// The neutral's section (Tabela 48): as the phases, or smaller above the code's section when `reduced` (the standard's
    /// conditions are the project's to check).
    pub fn neutral_section(&self, phase: f64, reduced: bool) -> f64 {
        if phase <= self.neutral_whole_up_to || !reduced {
            phase
        } else {
            self.neutral_reduced.iter().find(|r| (r.0 - phase).abs() < 1e-9).map_or(phase, |r| r.1)
        }
    }

    /// The protective conductor's section (Tabela 58): the phase's up to 16 mm², 16 above to 35, half of it above, rounded up to the
    /// next standard section (95 mm² gets 50, 120 gets 70, 150 and 185 get 95), as a cable is only made in those. Item 6.4.3.1.3 asks
    /// for the nearest standard section, which is sometimes the smaller (150 mm² gets 70 there); up is the safe side.
    pub fn protective_section(&self, phase: f64) -> f64 {
        if phase <= self.protective_whole_up_to {
            phase
        } else if phase <= self.protective_fixed_up_to {
            self.protective_fixed
        } else {
            let half = phase / 2.0;
            self.sections.iter().copied().find(|s| *s + 1e-9 >= half).unwrap_or(half)
        }
    }

    /// Resistance in ohm/km at the insulation's highest working temperature (what a cable carries at its rated current): a flexible
    /// conductor's (the default, the larger) or a rigid one's; where the flexible rows stop, the rigid figure raised by
    /// `flexible_over_rigid`, which is on the safe side.
    pub fn resistance(&self, section: f64, insulation: Insulation, flexible: bool) -> Option<f64> {
        let find = |t: &[(f64, f64)]| t.iter().find(|r| (r.0 - section).abs() < 1e-9).map(|r| r.1);
        let r20 = if flexible {
            find(&self.resistance_flexible).or_else(|| find(&self.resistance_rigid).map(|r| r * self.flexible_over_rigid))
        } else {
            find(&self.resistance_rigid)
        }?;
        Some(r20 * (1.0 + COPPER_ALPHA * (self.highest(insulation) - 20.0)))
    }
}

/// A circuit to size.
#[derive(Clone, Debug)]
pub struct Circuit {
    /// The load, W (or VA when `apparent`).
    pub load: f64,
    pub apparent: bool,
    /// Demand factor, 0 to 1, and power factor, over 0 to 1.
    pub demand: f64,
    pub power_factor: f64,
    pub phases: Phases,
    /// The voltage system's index in the rules' `systems`.
    pub system: usize,
    pub use_: Use,
    /// To the farthest load, metres; 0 when not known (no drop is checked).
    pub length: f64,
    /// A key of Tabela 33's methods (see [`method_keys`]).
    pub method: String,
    pub insulation: Insulation,
    pub flexible: bool,
    /// Ambient (air, or soil for method D) in °C, when not the standard's reference (30 in air, 20 in soil).
    pub ambient: Option<f64>,
    /// Circuits in the same conduit or bundle, this one included.
    pub grouped: u32,
    /// Method C cables bundled, not in a layer.
    pub bundle: bool,
    /// Distance between buried conduits, m.
    pub spacing: f64,
    /// Soil resistivity, K·m/W, method D.
    pub soil: f64,
    /// A section (mm²) and a breaker (A) already chosen: the design is never smaller. 0: none.
    pub section: f64,
    pub breaker: f64,
    /// The drop the circuit may have, per cent.
    pub drop_limit: f64,
    pub reduced_neutral: bool,
}

impl Default for Circuit {
    fn default() -> Circuit {
        Circuit {
            load: 0.0,
            apparent: false,
            demand: 1.0,
            power_factor: 1.0,
            phases: Phases::PhaseNeutral,
            system: 0,
            use_: Use::Power,
            length: 0.0,
            method: String::new(),
            insulation: Insulation::Pvc,
            flexible: true,
            ambient: None,
            grouped: 1,
            bundle: false,
            spacing: 0.0,
            soil: 0.0,
            section: 0.0,
            breaker: 0.0,
            drop_limit: 0.0,
            reduced_neutral: false,
        }
    }
}

/// What cannot be looked up, or sized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The method is not one of Tabela 33's that the tables cover.
    Method,
    /// The ambient is under 10 °C or hotter than the insulation allows (Tabela 40).
    Temperature,
    /// No grouping factor for that many circuits at that spacing.
    Grouping,
    /// The soil's resistivity is under 1 K·m/W or above Tabela 41.
    Soil,
    /// No section of the tables carries it.
    TooBig,
}

impl Fault {
    pub fn message(self) -> &'static str {
        match self {
            Fault::Method => "the installation method is not one of Tabela 33's that Tabelas 36 and 37 cover",
            Fault::Temperature => "the temperature is under 10 degrees or above what the insulation allows (Tabela 40)",
            Fault::Grouping => "Tabela 42 and 45 have no factor for that many circuits at that spacing",
            Fault::Soil => "the soil's resistivity is outside Tabela 41 (1 to 3 K.m/W)",
            Fault::TooBig => "no conductor of the tables carries it: split the circuit",
        }
    }
}

/// A section and a breaker evaluated for a circuit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Evaluation {
    pub section: f64,
    pub breaker: f64,
    /// I_Z, amperes: the table's value times the corrections.
    pub carries: f64,
    pub ampacity_ok: bool,
    /// `I_B <= I_n <= I_Z` and the trip current: none when no breaker is given.
    pub breaker_ok: Option<bool>,
    /// The drop in per cent and whether it is within the limit; none without a length.
    pub drop: Option<(f64, bool)>,
    pub minimum_ok: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Factors {
    pub temperature: f64,
    pub grouping: f64,
    pub soil: f64,
}

/// A circuit's design.
#[derive(Clone, Debug, PartialEq)]
pub struct Design {
    /// I_B, amperes, and the voltage the circuit is at.
    pub current: f64,
    pub volts: f64,
    pub factors: Option<Factors>,
    pub minimum: f64,
    /// What the chosen section and breaker give, when either is chosen.
    pub chosen: Option<Evaluation>,
    /// The smallest section from the chosen one up and the smallest breaker from the chosen one up that pass.
    pub needed: Option<Evaluation>,
    pub fault: Option<Fault>,
}

/// The voltage a circuit is at: phase to neutral for one phase and neutral, phase to phase for the others.
pub fn volts(r: &Rules, system: usize, phases: Phases) -> f64 {
    let (phase, line) = r.system(system);
    if phases == Phases::PhaseNeutral { phase } else { line }
}

/// I_B in amperes: S * demand / V, S = P / power factor for a load in W; for three phases divided by the square root of 3 too.
pub fn design_current(r: &Rules, c: &Circuit) -> f64 {
    let pf = if c.power_factor > 0.0 { c.power_factor } else { 1.0 };
    let demand = if c.demand > 0.0 { c.demand } else { 1.0 };
    let s = c.load * demand / if c.apparent { 1.0 } else { pf };
    s / (if c.phases.three() { 3f64.sqrt() } else { 1.0 } * volts(r, c.system, c.phases))
}

/// The drop of a section in volts and per cent of the voltage; none when the length is not known.
fn drop_of(rules: &Rules, c: &Circuit, section: f64) -> Option<(f64, f64)> {
    if c.length.is_nan() || c.length <= 0.0 {
        return None;
    }
    let pf = if c.power_factor > 0.0 && c.power_factor <= 1.0 { c.power_factor } else { 1.0 };
    let sin = (1.0 - pf * pf).max(0.0).sqrt();
    let b = if c.phases.three() { 3f64.sqrt() } else { 2.0 };
    let r = rules.resistance(section, c.insulation, c.flexible)?;
    let volts_lost = b * design_current(rules, c) * (c.length / 1000.0) * (r * pf + rules.reactance * sin);
    Some((volts_lost, volts_lost / volts(rules, c.system, c.phases) * 100.0))
}

/// The method a circuit is laid by: its own, or the code's default.
fn method_of<'a>(r: &'a Rules, c: &'a Circuit) -> &'a str {
    if c.method.is_empty() { &r.default_method } else { &c.method }
}

fn factors(r: &Rules, c: &Circuit) -> Result<Factors, Fault> {
    let (reference, _) = r.method(method_of(r, c)).ok_or(Fault::Method)?;
    let soil = reference == Reference::D;
    let ambient = c.ambient.unwrap_or(if soil { r.reference_soil_celsius } else { r.reference_air_celsius });
    let temperature = r.temperature_factor(c.insulation, ambient, soil).ok_or(Fault::Temperature)?;
    let grouping = r.grouping_factor(method_of(r, c), c.bundle, c.grouped, c.spacing).ok_or(Fault::Grouping)?;
    let resistivity = if c.soil > 0.0 { c.soil } else { r.reference_soil_resistivity };
    let soil = if soil { r.soil_factor(resistivity).ok_or(Fault::Soil)? } else { 1.0 };
    Ok(Factors { temperature, grouping, soil })
}

/// What a section and a breaker give for the circuit.
pub fn evaluate(r: &Rules, c: &Circuit, section: f64, breaker: f64) -> Result<Evaluation, Fault> {
    let f = factors(r, c)?;
    let (reference, _) = r.method(method_of(r, c)).ok_or(Fault::Method)?;
    let base = r.ampacity_of(c.insulation, section, reference, c.phases.loaded()).ok_or(Fault::TooBig)?;
    let carries = base * f.temperature * f.grouping * f.soil;
    let ib = design_current(r, c);
    let limit = if c.drop_limit > 0.0 { c.drop_limit } else { r.drop_limit };
    Ok(Evaluation {
        section,
        breaker,
        carries,
        ampacity_ok: ib <= carries + 1e-9,
        breaker_ok: (breaker > 0.0).then_some(ib <= breaker + 1e-9 && breaker <= carries + 1e-9),
        drop: drop_of(r, c, section).map(|(_, percent)| (percent, percent <= limit + 1e-9)),
        minimum_ok: section + 1e-9 >= r.minimum_section(c.use_),
    })
}

/// Sizes the circuit: from the chosen section and breaker up, the smallest that give `I_B <= I_n <= I_Z` with a drop within the limit.
pub fn design(r: &Rules, c: &Circuit) -> Design {
    let current = design_current(r, c);
    let mut out = Design {
        current,
        volts: volts(r, c.system, c.phases),
        factors: None,
        minimum: r.minimum_section(c.use_),
        chosen: None,
        needed: None,
        fault: None,
    };
    match factors(r, c) {
        Err(fault) => out.fault = Some(fault),
        Ok(f) => {
            out.factors = Some(f);
            if c.section > 0.0 || c.breaker > 0.0 {
                out.chosen = evaluate(r, c, c.section, c.breaker).ok();
            }
            let wanted = current.max(c.breaker);
            let breaker = r.breakers.iter().copied().find(|b| *b + 1e-9 >= wanted);
            out.needed = breaker.and_then(|b| {
                r.sections
                    .iter()
                    .copied()
                    .filter(|s| *s + 1e-9 >= out.minimum.max(c.section))
                    .filter_map(|s| evaluate(r, c, s, b).ok())
                    .find(|e| b <= e.carries + 1e-9 && e.drop.is_none_or(|d| d.1))
            });
            if out.needed.is_none() {
                out.fault = Some(Fault::TooBig);
            }
        }
    }
    out
}

/// The conductors as a schedule writes them: "1 × 2.5 + N 2.5 + PE 2.5 mm²".
pub fn cable_text(r: &Rules, phases: Phases, section: f64, reduced_neutral: bool) -> String {
    let neutral = match phases {
        Phases::PhaseNeutral => Some(section),
        Phases::ThreeNeutral => Some(r.neutral_section(section, reduced_neutral)),
        _ => None,
    };
    let n = |x: f64| format!("{x}");
    let mut text = format!("{} × {}", phases.count(), n(section));
    if let Some(neutral) = neutral {
        text.push_str(&format!(" + N {}", n(neutral)));
    }
    text.push_str(&format!(" + PE {} mm²", n(r.protective_section(section))));
    text
}

#[cfg(test)]
mod tests;
