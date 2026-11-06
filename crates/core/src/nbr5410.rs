//! A circuit sized as NBR 5410:2004 asks, for copper conductors: the design current, the smallest conductor section that carries
//! it once the temperature, grouping and soil corrections are applied and drops no more than the limit, the circuit breaker, and
//! the neutral and the protective conductor. A pure function of its input, with no model behind it: the `circuit` command is
//! its only caller. The tables are in [`tables`], as the standard prints them.
//!
//! What is checked: `I_B <= I_n <= I_Z`. For a breaker of NBR NM 60898, whose conventional trip current is `I_2 = 1.45 I_n`, that
//! already gives `I_2 <= 1.45 I_Z`, the second condition of the standard, so it is not checked again. The smallest section for the use (Tabela 47), and the voltage drop `b * I_B * L * (R cos(phi) + X sin(phi))`, with `R` at the
//! insulation's highest working temperature (so on the safe side) and `X` 0.1 ohm/km. What is not: aluminium, cable trays (methods
//! E and F), several layers of cables, buried cables without a conduit, short circuit, harmonics, the conditions of a reduced
//! neutral. The caller says so.

mod tables;

use tables::*;
pub use tables::{BREAKERS, CU_EPR, CU_PVC, SECTIONS};

/// A conductor's reactance in a conduit, ohm/km: with it the drop agrees with the makers' V/A.km tables within about 3 %.
pub const REACTANCE: f64 = 0.1;
/// Copper's temperature coefficient of resistance, per degree.
const COPPER_ALPHA: f64 = 0.003_93;
/// The drop a terminal circuit may have, per cent of the voltage.
pub const DROP_LIMIT: f64 = 4.0;
const MAX_CIRCUITS: u32 = 1_000;

/// The voltage systems a project may be at: (name, phase to neutral, phase to phase), volts.
pub const SYSTEMS: &[(&str, f64, f64)] = &[("127/220", 127.0, 220.0), ("220/380", 220.0, 380.0), ("230/400", 230.0, 400.0)];

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
    fn max_celsius(self) -> f64 {
        match self {
            Insulation::Pvc => 70.0,
            Insulation::Epr => 90.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    Lighting,
    Power,
}

/// How the cables lie in the installation methods of Tabela 33 that Tabelas 36 and 37 hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reference {
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Group {
    Bundle,
    Layer,
    Ceiling,
    BuriedMulti,
    BuriedSingle,
}

/// The installation methods of Tabela 33 the tables cover: (number as the standard writes it, reference method, grouping row).
const METHODS: &[(&str, Reference, Group)] = &[
    ("1", Reference::A1, Group::Bundle),
    ("2", Reference::A2, Group::Bundle),
    ("3", Reference::B1, Group::Bundle),
    ("4", Reference::B2, Group::Bundle),
    ("5", Reference::B1, Group::Bundle),
    ("6", Reference::B2, Group::Bundle),
    ("7", Reference::B1, Group::Bundle),
    ("8", Reference::B2, Group::Bundle),
    ("11", Reference::C, Group::Layer),
    ("11A", Reference::C, Group::Ceiling),
    ("61", Reference::D, Group::BuriedMulti),
    ("61A", Reference::D, Group::BuriedSingle),
];

/// The methods' numbers, as a command lists them.
pub fn method_keys() -> Vec<&'static str> {
    METHODS.iter().map(|m| m.0).collect()
}

fn method(key: &str) -> Option<(Reference, Group)> {
    METHODS.iter().find(|m| m.0 == key).map(|m| (m.1, m.2))
}

/// The current a conductor of `section` mm² carries in a method, amperes, with `loaded` (2 or 3) loaded conductors: Tabela 36 or 37.
pub fn ampacity(insulation: Insulation, section: f64, reference_method: &str, loaded: usize) -> Option<f64> {
    let (reference, _) = method(reference_method)?;
    ampacity_of(insulation, section, reference, loaded)
}

fn ampacity_of(insulation: Insulation, section: f64, reference: Reference, loaded: usize) -> Option<f64> {
    let table = match insulation {
        Insulation::Pvc => CU_PVC,
        Insulation::Epr => CU_EPR,
    };
    let row = table.iter().find(|r| (r.0 - section).abs() < 1e-9)?;
    row.1.get(reference.column() + usize::from(loaded >= 3)).copied()
}

/// Tabela 40's factor for an ambient (or soil) temperature in °C: none under 10 °C or above what the insulation allows.
pub fn temperature_factor(insulation: Insulation, celsius: f64, soil: bool) -> Option<f64> {
    if celsius.is_nan() || celsius < 10.0 {
        return None;
    }
    let table = if soil { T_SOIL } else { T_AIR };
    let row = table.iter().find(|r| r.0 >= celsius - 1e-9)?;
    match insulation {
        Insulation::Pvc => row.1,
        Insulation::Epr => row.2,
    }
}

/// Tabela 41's factor for a soil's resistivity in K·m/W.
pub fn soil_factor(resistivity: f64) -> Option<f64> {
    if resistivity.is_nan() || resistivity < 1.0 {
        return None;
    }
    SOIL.iter().find(|r| r.0 >= resistivity - 1e-9).map(|r| r.1)
}

/// Tabela 42's (and, buried, Tabela 45's) grouping factor for `circuits` in a method: `bundle` says the cables of a method C lie
/// bundled rather than in a layer; `spacing` is the distance between buried conduits in metres.
pub fn grouping_factor(method_key: &str, bundle: bool, circuits: u32, spacing: f64) -> Option<f64> {
    let (reference, group) = method(method_key)?;
    let n = circuits.clamp(1, MAX_CIRCUITS);
    if matches!(group, Group::BuriedMulti | Group::BuriedSingle) {
        if n == 1 {
            return Some(1.0);
        }
        if n > 6 || spacing.is_nan() || !(0.0..=1.0 + 1e-9).contains(&spacing) {
            return None;
        }
        // The factors grow with the spacing, so between two printed columns the narrower one is the safe side.
        let at = SPACINGS.iter().rposition(|s| *s <= spacing + 1e-9)?;
        let table = if group == Group::BuriedMulti { G45_MULTI } else { G45_SINGLE };
        return table.get(at)?.get(n as usize - 2).copied();
    }
    let group = if reference == Reference::C && bundle { Group::Bundle } else { group };
    let head = match group {
        Group::Ceiling => G42_CEILING,
        Group::Layer => G42_LAYER,
        _ => G42_BUNDLE,
    };
    if n <= 8 {
        return head.get(n as usize - 1).copied();
    }
    Some(match group {
        Group::Ceiling => G42_CEILING_TAIL,
        Group::Layer => G42_LAYER_TAIL,
        _ => G42_BUNDLE_TAIL.iter().find(|r| n <= r.0)?.1,
    })
}

/// Tabela 47: the smallest section, mm²: 1.5 for lighting, 2.5 for power.
pub fn minimum_section(use_: Use) -> f64 {
    match use_ {
        Use::Lighting => 1.5,
        Use::Power => 2.5,
    }
}

/// The neutral's section (Tabela 48): as the phases, or smaller above 25 mm² when `reduced` (the standard's conditions are the project's to check).
pub fn neutral_section(phase: f64, reduced: bool) -> f64 {
    if phase <= 25.0 || !reduced { phase } else { N48.iter().find(|r| (r.0 - phase).abs() < 1e-9).map_or(phase, |r| r.1) }
}

/// The protective conductor's section (Tabela 58): the phase's up to 16 mm², 16 above to 35, half of it above, rounded up to the
/// next standard section (95 mm² gets 50, 120 gets 70, 150 and 185 get 95), as a cable is only made in those. Item 6.4.3.1.3 asks
/// for the nearest standard section, which is sometimes the smaller (150 mm² gets 70 there); up is the safe side.
pub fn protective_section(phase: f64) -> f64 {
    if phase <= 16.0 {
        phase
    } else if phase <= 35.0 {
        16.0
    } else {
        let half = phase / 2.0;
        SECTIONS.iter().copied().find(|s| *s + 1e-9 >= half).unwrap_or(half)
    }
}

/// How far a flexible conductor of 300 mm² and over is above a rigid one: NBR NM 280 Tabela 3 against Tabela 2 gives 0.0641 against
/// 0.0601 ohm/km at 300 mm², 6.7 %. shortcut: that margin, rounded up, for every section above 240 mm² until those rows are
/// added to `RESISTANCE_FLEX`; the other sections above 300 mm² were not compared.
const FLEX_OVER_RIGID: f64 = 1.07;

/// Resistance in ohm/km at the insulation's highest working temperature (what a cable carries at its rated current): a flexible
/// conductor's (the default, the larger) or a rigid one's; above 240 mm² the flexible rows are not here, so the rigid figure
/// raised by [`FLEX_OVER_RIGID`], which is on the safe side.
pub fn resistance(section: f64, insulation: Insulation, flexible: bool) -> Option<f64> {
    let find = |t: &[(f64, f64)]| t.iter().find(|r| (r.0 - section).abs() < 1e-9).map(|r| r.1);
    let r20 =
        if flexible { find(RESISTANCE_FLEX).or_else(|| find(RESISTANCE_20).map(|r| r * FLEX_OVER_RIGID)) } else { find(RESISTANCE_20) }?;
    Some(r20 * (1.0 + COPPER_ALPHA * (insulation.max_celsius() - 20.0)))
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
    /// The voltage system's index in [`SYSTEMS`].
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
            method: "7".to_string(),
            insulation: Insulation::Pvc,
            flexible: true,
            ambient: None,
            grouped: 1,
            bundle: false,
            spacing: 0.0,
            soil: 2.5,
            section: 0.0,
            breaker: 0.0,
            drop_limit: DROP_LIMIT,
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
pub fn volts(system: usize, phases: Phases) -> f64 {
    let (_, phase, line) = SYSTEMS.get(system).copied().unwrap_or(SYSTEMS[0]);
    if phases == Phases::PhaseNeutral { phase } else { line }
}

/// I_B in amperes: S * demand / V, S = P / power factor for a load in W; for three phases divided by the square root of 3 too.
pub fn design_current(c: &Circuit) -> f64 {
    let pf = if c.power_factor > 0.0 { c.power_factor } else { 1.0 };
    let demand = if c.demand > 0.0 { c.demand } else { 1.0 };
    let s = c.load * demand / if c.apparent { 1.0 } else { pf };
    s / (if c.phases.three() { 3f64.sqrt() } else { 1.0 } * volts(c.system, c.phases))
}

/// The drop of a section in volts and per cent of the voltage; none when the length is not known.
fn drop_of(c: &Circuit, section: f64) -> Option<(f64, f64)> {
    if c.length.is_nan() || c.length <= 0.0 {
        return None;
    }
    let pf = if c.power_factor > 0.0 && c.power_factor <= 1.0 { c.power_factor } else { 1.0 };
    let sin = (1.0 - pf * pf).max(0.0).sqrt();
    let b = if c.phases.three() { 3f64.sqrt() } else { 2.0 };
    let r = resistance(section, c.insulation, c.flexible)?;
    let volts_lost = b * design_current(c) * (c.length / 1000.0) * (r * pf + REACTANCE * sin);
    Some((volts_lost, volts_lost / volts(c.system, c.phases) * 100.0))
}

fn factors(c: &Circuit) -> Result<Factors, Fault> {
    let (reference, _) = method(&c.method).ok_or(Fault::Method)?;
    let soil = reference == Reference::D;
    let ambient = c.ambient.unwrap_or(if soil { 20.0 } else { 30.0 });
    let temperature = temperature_factor(c.insulation, ambient, soil).ok_or(Fault::Temperature)?;
    let grouping = grouping_factor(&c.method, c.bundle, c.grouped, c.spacing).ok_or(Fault::Grouping)?;
    let soil = if soil { soil_factor(c.soil).ok_or(Fault::Soil)? } else { 1.0 };
    Ok(Factors { temperature, grouping, soil })
}

/// What a section and a breaker give for the circuit.
pub fn evaluate(c: &Circuit, section: f64, breaker: f64) -> Result<Evaluation, Fault> {
    let f = factors(c)?;
    let (reference, _) = method(&c.method).ok_or(Fault::Method)?;
    let base = ampacity_of(c.insulation, section, reference, c.phases.loaded()).ok_or(Fault::TooBig)?;
    let carries = base * f.temperature * f.grouping * f.soil;
    let ib = design_current(c);
    let limit = if c.drop_limit > 0.0 { c.drop_limit } else { DROP_LIMIT };
    Ok(Evaluation {
        section,
        breaker,
        carries,
        ampacity_ok: ib <= carries + 1e-9,
        breaker_ok: (breaker > 0.0).then_some(ib <= breaker + 1e-9 && breaker <= carries + 1e-9),
        drop: drop_of(c, section).map(|(_, percent)| (percent, percent <= limit + 1e-9)),
        minimum_ok: section + 1e-9 >= minimum_section(c.use_),
    })
}

/// Sizes the circuit: from the chosen section and breaker up, the smallest that give `I_B <= I_n <= I_Z` with a drop within the limit.
pub fn design(c: &Circuit) -> Design {
    let current = design_current(c);
    let mut out = Design {
        current,
        volts: volts(c.system, c.phases),
        factors: None,
        minimum: minimum_section(c.use_),
        chosen: None,
        needed: None,
        fault: None,
    };
    match factors(c) {
        Err(fault) => out.fault = Some(fault),
        Ok(f) => {
            out.factors = Some(f);
            if c.section > 0.0 || c.breaker > 0.0 {
                out.chosen = evaluate(c, c.section, c.breaker).ok();
            }
            let wanted = current.max(c.breaker);
            let breaker = BREAKERS.iter().copied().find(|b| *b + 1e-9 >= wanted);
            out.needed = breaker.and_then(|b| {
                SECTIONS
                    .iter()
                    .copied()
                    .filter(|s| *s + 1e-9 >= out.minimum.max(c.section))
                    .filter_map(|s| evaluate(c, s, b).ok())
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
pub fn cable_text(phases: Phases, section: f64, reduced_neutral: bool) -> String {
    let neutral = match phases {
        Phases::PhaseNeutral => Some(section),
        Phases::ThreeNeutral => Some(neutral_section(section, reduced_neutral)),
        _ => None,
    };
    let n = |x: f64| format!("{x}");
    let mut text = format!("{} × {}", phases.count(), n(section));
    if let Some(neutral) = neutral {
        text.push_str(&format!(" + N {}", n(neutral)));
    }
    text.push_str(&format!(" + PE {} mm²", n(protective_section(section))));
    text
}

#[cfg(test)]
mod tests;
