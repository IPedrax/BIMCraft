//! Surge protection (DPS) of a power line as NBR 5410:2004 asks: when it is required (item 5.4.2.1.1), where it stands (6.3.5.2.1),
//! how it is connected (6.3.5.2.2 and 6.3.5.2.3, Figure 13), the least discharge and impulse currents (6.3.5.2.4 d) and the
//! connecting conductors (6.3.5.2.9). A pure function of its input, with no model behind it: the `dps_check` command is its only
//! caller.
//!
//! - Required where the line is overhead (wholly or partly, or the installation has an overhead line itself) and the region is AQ2
//!   (more than 25 days of thunderstorms a year), or in any AQ3 region. A project may take the risk, as the standard allows when
//!   the consequences are only material and calculated, but never where they may put people's safety or health at risk.
//! - Every external signal line (telephone, data, video, pay TV, external antennas, links to other buildings) asks for it at the
//!   building's entry or exit (5.4.2.2.1 and 6.3.5.3.1).
//! - Where: against surges coming in by the line (atmospheric or switching), at the line's entry or at the main board, as near
//!   to the entry as possible; against direct lightning, at the entry. In an existing individual consumer unit on the public
//!   low-voltage network, at the meter box too, if the PE bar there is bonded to the main equipotential bar and the box is no more
//!   than 10 m from the entry.
//! - Connection: a line with no neutral, and TN-C, connection 1 (each phase to the PE); TN-S, TT and IT with a neutral, connection 2
//!   (each phase and the neutral to the PE) or 3 (each phase to the neutral, and the neutral to the PE).
//! - At least 5 kA (8/20 µs) of nominal discharge current for each mode of protection (20 kA on a three-phase network, 10 kA on a
//!   single-phase one, between the neutral and the PE of connection 3); at least 12.5 kA of impulse current (the standard fixes no waveform for its test) against
//!   direct lightning (50 kA three-phase, 25 kA single-phase, between the neutral and the PE of connection 3, where it can't be
//!   worked out by IEC 61312-1). Connecting conductors of 4 mm² copper at least (16 mm² against direct lightning), no more than
//!   0.5 m in all.
//!
//! - Uc, the least continuous operating voltage (Table 49): 1.1 Uo for a mode with a phase in TT, TN-S and TN-C; Uo between the neutral
//!   and the PE; in IT with a neutral, √3 Uo between phase and PE; in IT without one, U.
//!
//! What is not here: the protection level Up (category II of Table 31), the
//! short-circuit withstand and the overcurrent protection of the DPS (6.3.5.2.5), and coordination between DPS.
//!
//! The numbers are the `surge` section of the electrical code (`data/codes/electrical/nbr-5410.json`), each with its item; the
//! project may change any of them. The schemes of Figure 13 and Tabela 49's rows are the method, here.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// A current for each mode of protection, and between neutral and PE of connection 3 on a single-phase and a three-phase line.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByMode {
    pub each_mode: f64,
    pub neutral_pe_single_phase: f64,
    pub neutral_pe_three_phase: f64,
}

/// A value against surges carried by the line, and against direct lightning.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByPurpose {
    pub line: f64,
    pub direct: f64,
}

/// A row of Tabela 31: the nominal systems it holds and the withstand (kV) of categories IV, III, II and I.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Withstand {
    pub systems: Vec<String>,
    pub kv: [f64; 4],
}

/// The rules of surge protection: the `surge` section of an electrical code.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    pub regions: Vec<Region>,
    /// The regions where an overhead line asks for it, and where any line does.
    pub overhead_in: Vec<Region>,
    pub always_in: Vec<Region>,
    /// What a region is, in words, by its name.
    #[serde(default)]
    pub region_notes: BTreeMap<String, String>,
    /// The kinds of external signal line.
    pub signals: Vec<String>,
    /// How far the meter box may be from the entry, m.
    pub meter_box_up_to: f64,
    /// The most the connecting conductors' total length may be, m.
    pub conductor_length: f64,
    /// The least section of the connecting conductors, mm² of copper.
    pub conductor_section: ByPurpose,
    /// The least nominal discharge current, kA (8/20 µs), and impulse current, kA (no waveform fixed).
    pub discharge: ByMode,
    pub impulse: ByMode,
    /// The least follow current a spark-gap device between neutral and PE interrupts in TN and TT, A.
    pub follow_current: f64,
    /// Tabela 49's factor on Uo of a mode with a phase.
    pub uc_factor: f64,
    pub withstand: Vec<Withstand>,
    /// The least impulse withstand of the components tied to a signal line at the entry, kV.
    pub signal_entry_withstand: f64,
}

/// BIMCraft's surge rules, each electrical code's `surge` section: a project reads its own (`Model::section_as`).
pub fn rule_sets() -> &'static [Rules] {
    static SETS: OnceLock<Vec<Rules>> = OnceLock::new();
    SETS.get_or_init(|| crate::codes::library_sections(crate::codes::Discipline::Electrical, "surge"))
}

impl crate::codes::Valid for Rules {
    fn valid(&self) -> Result<(), String> {
        let modes =
            [self.discharge, self.impulse].into_iter().flat_map(|m| [m.each_mode, m.neutral_pe_single_phase, m.neutral_pe_three_phase]);
        let all = modes
            .chain([self.meter_box_up_to, self.conductor_length, self.conductor_section.line, self.conductor_section.direct])
            .chain([self.follow_current, self.uc_factor, self.signal_entry_withstand])
            .chain(self.withstand.iter().flat_map(|w| w.kv));
        if all.into_iter().any(|x| !x.is_finite() || x < 0.0) {
            return Err("every number of surge protection is finite and not negative".into());
        }
        if self.regions.is_empty() {
            return Err("`regions` holds one at least".into());
        }
        Ok(())
    }
}

impl Rules {
    /// The regions' names, as a command lists them.
    pub fn region_names(&self) -> Vec<&'static str> {
        self.regions.iter().map(|r| r.name()).collect()
    }

    /// The nominal systems Tabela 31 lists.
    pub fn systems(&self) -> Vec<&str> {
        self.withstand.iter().flat_map(|w| w.systems.iter().map(String::as_str)).collect()
    }
}

/// The external influence of a region on lightning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Region {
    /// Few thunderstorms a year.
    #[serde(rename = "AQ1")]
    Aq1,
    /// Many thunderstorms a year.
    #[serde(rename = "AQ2")]
    Aq2,
    #[serde(rename = "AQ3")]
    Aq3,
}

impl Region {
    pub fn name(self) -> &'static str {
        match self {
            Region::Aq1 => "AQ1",
            Region::Aq2 => "AQ2",
            Region::Aq3 => "AQ3",
        }
    }

    pub fn parse(s: &str) -> Option<Region> {
        Some(match s {
            "AQ1" => Region::Aq1,
            "AQ2" => Region::Aq2,
            "AQ3" => Region::Aq3,
            _ => return None,
        })
    }
}

/// A power line and what the project does about the risk.
#[derive(Clone, Copy, Debug)]
pub struct Line {
    /// Wholly or partly overhead, or the installation has an overhead line itself.
    pub overhead: bool,
    pub region: Region,
    /// The project takes the risk of leaving it out, its consequences being only material and calculated.
    pub accepted_risk: bool,
    /// The omission may put people's safety or health at risk, directly or not.
    pub risk_to_people: bool,
}

#[derive(Debug)]
pub struct Verdict {
    pub required: bool,
    /// It would be required, but the project takes a calculated risk the standard allows.
    pub exempt: bool,
    pub reasons: Vec<String>,
}

pub fn required(r: &Rules, l: &Line) -> Verdict {
    let mut reasons = Vec::new();
    let region = |name: &str| match r.region_notes.get(name) {
        Some(note) => format!("{name} ({note})"),
        None => name.to_string(),
    };
    if l.overhead && r.overhead_in.contains(&l.region) {
        reasons.push(format!("an overhead line in a region of {}", region(l.region.name())));
    }
    if r.always_in.contains(&l.region) {
        reasons.push(format!("a region of {}", region(l.region.name())));
    }
    let asked = !reasons.is_empty();
    let exempt = asked && l.accepted_risk && !l.risk_to_people;
    Verdict { required: asked && !exempt, exempt, reasons }
}

/// Why an external signal line of `kind` needs it, or `None` for a kind the standard does not name.
pub fn signal_required(r: &Rules, kind: &str) -> Option<&'static str> {
    r.signals.iter().any(|s| s == kind).then_some("every external signal line is protected where it enters or leaves the building")
}

/// What the device is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// Surges of atmospheric origin carried by the line, and switching surges.
    Line,
    /// Direct lightning on the building or near it.
    Direct,
    /// Both at once.
    Both,
}

pub const PURPOSES: &[&str] = &["line", "direct", "both"];

impl Purpose {
    pub fn parse(s: &str) -> Option<Purpose> {
        Some(match s {
            "line" => Purpose::Line,
            "direct" => Purpose::Direct,
            "both" => Purpose::Both,
            _ => return None,
        })
    }
}

/// Where the devices stand; `existing` is an individual consumer unit already on the public low-voltage network.
pub fn place(r: &Rules, purpose: Purpose, existing: bool) -> Vec<String> {
    let entry = "at the point where the line enters the building".to_string();
    if purpose != Purpose::Line {
        return vec![entry];
    }
    let mut out = vec![entry, "or at the main distribution board, as near to the entry as possible".to_string()];
    if existing {
        out.push(format!(
            "or, in an existing individual consumer unit, at the meter box, if the PE bar the devices use is bonded to the main \
             equipotential bar and the box is no more than {} m from the entry",
            r.meter_box_up_to
        ));
    }
    out
}

/// The earthing schemes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Earthing {
    Tt,
    TnS,
    TnC,
    It,
}

pub const EARTHINGS: &[&str] = &["TT", "TN-S", "TN-C", "IT"];

impl Earthing {
    pub fn parse(s: &str) -> Option<Earthing> {
        Some(match s {
            "TT" => Earthing::Tt,
            "TN-S" => Earthing::TnS,
            "TN-C" => Earthing::TnC,
            "IT" => Earthing::It,
            _ => return None,
        })
    }
}

/// The connection schemes (1, 2 or 3 of Figure 13) a line allows.
pub fn schemes(earthing: Earthing, neutral: bool) -> Vec<u8> {
    match (earthing, neutral) {
        (Earthing::TnC, _) | (_, false) => vec![1],
        _ => vec![2, 3],
    }
}

/// The follow current a DPS with spark gaps must be able to interrupt between the neutral and the PE (6.3.5.2.4 e).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Subsequent {
    AtLeastA(f64),
    LikePhaseNeutral,
}

/// 6.3.5.2.4 e: 100 A at least in TN and TT, and that of the DPS between phase and neutral in IT; `None` where there is no neutral-PE
/// device (TN-C).
pub fn subsequent_current(r: &Rules, earthing: Earthing) -> Option<Subsequent> {
    match earthing {
        Earthing::TnC => None,
        Earthing::It => Some(Subsequent::LikePhaseNeutral),
        Earthing::Tt | Earthing::TnS => Some(Subsequent::AtLeastA(r.follow_current)),
    }
}

/// 6.3.5.2.6 b: where the DPS stand upstream of a DR in a TT installation they are connected by scheme 3; otherwise as `schemes`.
pub fn schemes_before_dr(earthing: Earthing, neutral: bool) -> Vec<u8> {
    if earthing == Earthing::Tt && neutral {
        return vec![3];
    }
    schemes(earthing, neutral)
}

/// What a connection scheme connects.
pub fn scheme_text(scheme: u8, earthing: Earthing) -> String {
    match (scheme, earthing) {
        (1, Earthing::TnC) => "each phase to the PE (the PEN)".into(),
        (1, _) => "each phase to the PE".into(),
        (2, _) => "each phase to the PE, and the neutral to the PE".into(),
        _ => "each phase to the neutral, and the neutral to the PE".into(),
    }
}

/// One row of Tabela 49: the least continuous operating voltage Uc of a device between two points.
#[derive(Clone, Debug)]
pub struct Uc {
    pub between: &'static str,
    /// As the table prints it: `1.1 Uo`, `√3 Uo`, `Uo` or `U`.
    pub formula: String,
    pub volts: f64,
}

/// Tabela 49: the least Uc by the earthing scheme and the connection, `uo` being the phase-to-neutral voltage (V) and `U`, the
/// voltage between phases, √3 `uo`. A line with no neutral on TT or TN-S has the phase-to-PE row; IT without a neutral
/// distributed has `U`.
pub fn uc_min(r: &Rules, earthing: Earthing, neutral: bool, uo: f64) -> Vec<Uc> {
    let (k, root) = (r.uc_factor * uo, 3f64.sqrt() * uo);
    let factor = format!("{} Uo", r.uc_factor);
    let row = |between, formula: &str, volts| Uc { between, formula: formula.to_string(), volts };
    match (earthing, neutral) {
        (Earthing::TnC, _) => vec![row("phase-PEN", &factor, k)],
        (Earthing::Tt | Earthing::TnS, true) => {
            vec![row("phase-neutral", &factor, k), row("phase-PE", &factor, k), row("neutral-PE", "Uo", uo)]
        }
        (Earthing::Tt | Earthing::TnS, false) => vec![row("phase-PE", &factor, k)],
        (Earthing::It, true) => {
            vec![row("phase-neutral", &factor, k), row("phase-PE", "√3 Uo", root), row("neutral-PE", "Uo", uo)]
        }
        (Earthing::It, false) => vec![row("phase-PE", "U", root)],
    }
}

/// Tabela 31: the impulse withstand voltage (kV) required of the components of an installation of a nominal voltage, by category of
/// product: `[IV, III, II, I]`, IV for a product at the origin of the installation, III for the distribution and final circuits, II
/// for the equipment of use and I for the specially protected. `None` for a system the table does not list.
pub fn withstand(r: &Rules, system: &str) -> Option<[f64; 4]> {
    r.withstand.iter().find(|w| w.systems.iter().any(|s| s == system)).map(|w| w.kv)
}

/// The least nominal discharge current (kA, 8/20 µs) for each mode of protection; for the neutral–PE mode of connection 3
/// (`neutral_pe`) it is more. Nothing is asked of a device that is only against direct lightning.
pub fn discharge(r: &Rules, purpose: Purpose, neutral_pe: bool, phases: u32) -> f64 {
    match (purpose, neutral_pe) {
        (Purpose::Direct, _) => 0.0,
        (_, false) => r.discharge.each_mode,
        (_, true) if phases == 1 => r.discharge.neutral_pe_single_phase,
        (_, true) => r.discharge.neutral_pe_three_phase,
    }
}

/// The least impulse current (kA, no waveform fixed) against direct lightning for each mode of protection; for the neutral–PE mode of
/// connection 3 it is the standard's figure where it can't be worked out.
pub fn impulse(r: &Rules, neutral_pe: bool, phases: u32) -> f64 {
    match (neutral_pe, phases) {
        (false, _) => r.impulse.each_mode,
        (true, 1) => r.impulse.neutral_pe_single_phase,
        (true, _) => r.impulse.neutral_pe_three_phase,
    }
}

/// The least section (mm², copper) of the conductors that connect the devices to the line and to the earth.
pub fn conductor_section(r: &Rules, purpose: Purpose) -> f64 {
    if purpose == Purpose::Line { r.conductor_section.line } else { r.conductor_section.direct }
}

#[cfg(test)]
mod tests;
