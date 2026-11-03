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
//!   single-phase one, between the neutral and the PE of connection 3); at least 12.5 kA (10/350 µs) of impulse current against
//!   direct lightning (50 kA three-phase, 25 kA single-phase, between the neutral and the PE of connection 3, where it can't be
//!   worked out by IEC 61312-1). Connecting conductors of 4 mm² copper at least (16 mm² against direct lightning), no more than
//!   0.5 m in all.
//!
//! - Uc, the least continuous operating voltage (Table 49): 1.1 Uo for a mode with a phase in TT, TN-S and TN-C; Uo between the neutral
//!   and the PE; in IT with a neutral, √3 Uo between phase and PE; in IT without one, U.
//!
//! What is not here: the protection level Up (category II of Table 31), the
//! short-circuit withstand and the overcurrent protection of the DPS (6.3.5.2.5), and coordination between DPS.

/// The most the connecting conductors' total length may be, m.
pub const CONDUCTOR_LENGTH: f64 = 0.5;

/// The external influence of a region on lightning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    /// Up to 25 days of thunderstorms a year.
    Aq1,
    /// More than 25 days of thunderstorms a year.
    Aq2,
    Aq3,
}

pub const REGIONS: &[&str] = &["AQ1", "AQ2", "AQ3"];

impl Region {
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

pub fn required(l: &Line) -> Verdict {
    let mut reasons = Vec::new();
    if l.overhead && l.region == Region::Aq2 {
        reasons.push("an overhead line in a region of AQ2 (more than 25 days of thunderstorms a year)".to_string());
    }
    if l.region == Region::Aq3 {
        reasons.push("a region of AQ3".to_string());
    }
    let asked = !reasons.is_empty();
    let exempt = asked && l.accepted_risk && !l.risk_to_people;
    Verdict { required: asked && !exempt, exempt, reasons }
}

/// The kinds of external signal line.
pub const SIGNALS: &[&str] = &["telephone", "data", "video", "tv", "antenna", "interconnection"];

/// Why an external signal line of `kind` needs it, or `None` for a kind the standard does not name.
pub fn signal_required(kind: &str) -> Option<&'static str> {
    SIGNALS.contains(&kind).then_some("every external signal line is protected where it enters or leaves the building")
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
pub fn place(purpose: Purpose, existing: bool) -> Vec<String> {
    let entry = "at the point where the line enters the building".to_string();
    if purpose != Purpose::Line {
        return vec![entry];
    }
    let mut out = vec![entry, "or at the main distribution board, as near to the entry as possible".to_string()];
    if existing {
        out.push(
            "or, in an existing individual consumer unit, at the meter box, if the PE bar the devices use is bonded to the main \
             equipotential bar and the box is no more than 10 m from the entry"
                .to_string(),
        );
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
#[derive(Clone, Copy, Debug)]
pub struct Uc {
    pub between: &'static str,
    /// As the table prints it: `1.1 Uo`, `√3 Uo`, `Uo` or `U`.
    pub formula: &'static str,
    pub volts: f64,
}

/// Tabela 49: the least Uc by the earthing scheme and the connection, `uo` being the phase-to-neutral voltage (V) and `U`, the
/// voltage between phases, √3 `uo`. A line with no neutral on TT or TN-S has the phase-to-PE row; IT without a neutral
/// distributed has `U`.
pub fn uc_min(earthing: Earthing, neutral: bool, uo: f64) -> Vec<Uc> {
    let (k, root) = (1.1 * uo, 3f64.sqrt() * uo);
    let row = |between, formula, volts| Uc { between, formula, volts };
    match (earthing, neutral) {
        (Earthing::TnC, _) => vec![row("phase-PEN", "1.1 Uo", k)],
        (Earthing::Tt | Earthing::TnS, true) => {
            vec![row("phase-neutral", "1.1 Uo", k), row("phase-PE", "1.1 Uo", k), row("neutral-PE", "Uo", uo)]
        }
        (Earthing::Tt | Earthing::TnS, false) => vec![row("phase-PE", "1.1 Uo", k)],
        (Earthing::It, true) => {
            vec![row("phase-neutral", "1.1 Uo", k), row("phase-PE", "√3 Uo", root), row("neutral-PE", "Uo", uo)]
        }
        (Earthing::It, false) => vec![row("phase-PE", "U", root)],
    }
}

/// The nominal voltages of Tabela 31's three-phase systems.
pub const SYSTEMS: &[&str] = &["120/208", "127/220", "220/380", "230/400", "277/480", "400/690"];

/// Tabela 31: the impulse withstand voltage (kV) required of the components of an installation of a nominal voltage, by category of
/// product: `[IV, III, II, I]`, IV for a product at the origin of the installation, III for the distribution and final circuits, II
/// for the equipment of use and I for the specially protected. `None` for a system the table does not list.
pub fn withstand(system: &str) -> Option<[f64; 4]> {
    match system {
        "120/208" | "127/220" => Some([4.0, 2.5, 1.5, 0.8]),
        "220/380" | "230/400" | "277/480" => Some([6.0, 4.0, 2.5, 1.5]),
        "400/690" => Some([8.0, 6.0, 4.0, 2.5]),
        _ => None,
    }
}

/// The least impulse withstand (kV) of the components tied to a signal line at the entry of the installation (note 3 of Tabela 31,
/// IEC 61663-2).
pub const SIGNAL_ENTRY_WITHSTAND_KV: f64 = 1.5;

/// The least nominal discharge current (kA, 8/20 µs) for each mode of protection; for the neutral–PE mode of connection 3
/// (`neutral_pe`) it is more. Nothing is asked of a device that is only against direct lightning.
pub fn discharge(purpose: Purpose, neutral_pe: bool, phases: u32) -> f64 {
    match (purpose, neutral_pe) {
        (Purpose::Direct, _) => 0.0,
        (_, false) => 5.0,
        (_, true) if phases == 1 => 10.0,
        (_, true) => 20.0,
    }
}

/// The least impulse current (kA, 10/350 µs) against direct lightning for each mode of protection; for the neutral–PE mode of
/// connection 3 it is the standard's figure where it can't be worked out.
pub fn impulse(neutral_pe: bool, phases: u32) -> f64 {
    match (neutral_pe, phases) {
        (false, _) => 12.5,
        (true, 1) => 25.0,
        (true, _) => 50.0,
    }
}

/// The least section (mm², copper) of the conductors that connect the devices to the line and to the earth.
pub fn conductor_section(purpose: Purpose) -> f64 {
    if purpose == Purpose::Line { 4.0 } else { 16.0 }
}

#[cfg(test)]
mod tests;
