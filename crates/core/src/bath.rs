//! The volumes around a bathtub or a shower, and what may stand in each, as NBR 5410:2004 item 9.1 asks: which volume a point is
//! in, the least IP code there, and whether an outlet, a switch, a box, a luminaire, a heater or other equipment may stand there,
//! by its class and its protection. A pure function of the fixture and the things it is given, with no model behind it: the
//! `bath_volumes` command is its only caller.
//!
//! The numbers are data, not code: `data/bath-volumes.json` holds a rule set for each standard, every volume and every thing it
//! allows naming its source, and this only reads them (`RuleSet`). NBR 5410:2004 is the default and, for now, the only set. The
//! volumes are measured straight, in plan from the edge of the tub or tray (or from the outlet of a shower with no tray) and in
//! height from the floor one stands on; the walls and partitions the standard lets them go around are not seen, so what stands
//! behind one is taken as inside, on the safe side.

use crate::geom::{self, P2};
use serde::Deserialize;
use std::sync::OnceLock;

/// The standard `bath_volumes` follows when none is named.
pub const DEFAULT: &str = "NBR 5410:2004";

/// The kinds of thing, as a command gives them.
pub const KINDS: &[&str] = &["outlet", "switch", "box", "luminaire", "heater", "equipment"];
/// The protection classes.
pub const CLASSES: &[&str] = &["I", "II", "III"];
/// How a thing is fed: from the mains, at safety extra-low voltage, or through its own separating transformer.
pub const SUPPLIES: &[&str] = &["mains", "selv", "separation"];

/// What may stand in a volume: one row of its `allows`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Allow {
    pub kinds: Vec<String>,
    /// The classes it must be of; any when empty.
    #[serde(default)]
    pub classes: Vec<String>,
    /// The protections it must have one of (selv, separation, rcd); none asked when empty.
    #[serde(default)]
    pub any_of: Vec<String>,
    /// Only a box that connects equipment of these volumes.
    #[serde(default)]
    pub serving: bool,
    pub source: String,
}

/// One volume of a row of `data/bath-volumes.json`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Volume {
    pub name: String,
    /// How far it goes out from the edge of the tub or tray, mm (0: its inside).
    pub reach_mm: f64,
    /// How far it goes out from the outlet of a shower with no tray, mm; none there when absent.
    #[serde(default)]
    pub shower_reach_mm: Option<f64>,
    /// How high above the floor, mm.
    #[serde(default)]
    pub top_mm: Option<f64>,
    /// Up to the rim of the tub or tray instead.
    #[serde(default)]
    pub to_rim: bool,
    /// The least second digit of the IP code.
    pub ip: u8,
    /// The same in a public bathroom.
    pub ip_public: u8,
    /// Everything in it fed at SELV up to so many volts.
    #[serde(default)]
    pub selv_max_v: Option<f64>,
    pub allows: Vec<Allow>,
    pub source: String,
}

/// One standard's volumes, an entry of `data/bath-volumes.json`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSet {
    /// Its name and edition, which `bath_volumes {standard}` takes.
    pub standard: String,
    /// The document, its item and the other standards that answer the same question.
    pub source: String,
    /// The most a residual-current device may be, mA, where one protects an outlet.
    pub max_rcd_ma: f64,
    /// From the inside out.
    pub volumes: Vec<Volume>,
    /// What the set does not check, which the answer says.
    pub not_covered: String,
}

#[derive(Default, Deserialize)]
struct Data {
    rule_sets: Vec<RuleSet>,
}

/// The rule sets of `data/bath-volumes.json`; none if it does not read, which a test keeps from happening.
pub fn rule_sets() -> &'static [RuleSet] {
    static SETS: OnceLock<Vec<RuleSet>> = OnceLock::new();
    SETS.get_or_init(|| serde_json::from_str::<Data>(include_str!("../data/bath-volumes.json")).map(|d| d.rule_sets).unwrap_or_default())
}

/// The rule set of a standard, by its name.
pub fn rule_set(standard: &str) -> Option<&'static RuleSet> {
    rule_sets().iter().find(|s| s.standard == standard)
}

/// The tub, tray or recess in plan, or the outlet of a shower with no tray.
#[derive(Clone, Debug)]
pub enum Fixture {
    Outline(Vec<P2>),
    Shower(P2),
}

/// Where the bath is: the fixture, the level of the floor one stands on (mm) and the rim's height above it (mm).
#[derive(Clone, Debug)]
pub struct Bath {
    pub fixture: Fixture,
    pub floor: f64,
    pub rim: f64,
}

impl Bath {
    /// How far a point is from the fixture in plan, mm: 0 inside the outline.
    fn reach(&self, p: P2) -> f64 {
        match &self.fixture {
            Fixture::Shower(s) => s.dist(p),
            Fixture::Outline(ring) if geom::contains(ring, p) => 0.0,
            Fixture::Outline(ring) => geom::ring_edges(ring).map(|(a, b)| geom::dist_to_segment(p, a, b)).fold(f64::INFINITY, f64::min),
        }
    }
}

/// A thing that stands, or would, near the bath.
#[derive(Clone, Debug)]
pub struct Thing {
    pub at: [f64; 3],
    /// A key of `KINDS`.
    pub kind: String,
    /// The second digit of its IP code.
    pub ip: Option<u8>,
    /// A key of `CLASSES`.
    pub class: Option<String>,
    /// A key of `SUPPLIES`.
    pub supply: String,
    /// The SELV voltage, V.
    pub voltage: Option<f64>,
    /// The residual-current device that protects it, mA.
    pub rcd: Option<f64>,
    /// A box that connects only equipment of these volumes.
    pub serving: bool,
}

/// What the standard says of a thing where it stands.
#[derive(Debug)]
pub struct Verdict {
    /// The volume it is in; none outside them all.
    pub volume: Option<String>,
    /// The least IP digit there.
    pub ip_min: Option<u8>,
    /// Whether its IP is enough; `None` where it is not known, or none is asked.
    pub ip_ok: Option<bool>,
    /// Whether it may stand there.
    pub allowed: bool,
    /// Why not, or what is missing to say.
    pub why: Vec<String>,
    pub sources: Vec<String>,
}

impl RuleSet {
    /// The volume a point is in.
    pub fn volume(&self, bath: &Bath, at: [f64; 3]) -> Option<&Volume> {
        let d = bath.reach(P2(at[0], at[1]));
        let h = at[2] - bath.floor;
        let shower = matches!(bath.fixture, Fixture::Shower(_));
        self.volumes.iter().find(|v| {
            let reach = if shower { v.shower_reach_mm } else { Some(v.reach_mm) };
            let top = if v.to_rim { Some(bath.rim) } else { v.top_mm };
            match (reach, top) {
                (Some(r), Some(t)) => d <= r + 1e-6 && h >= -1e-6 && h <= t + 1e-6,
                _ => false,
            }
        })
    }

    /// Whether a thing has one of the protections a row asks.
    fn protected(&self, t: &Thing, any_of: &[String]) -> bool {
        any_of.is_empty()
            || any_of.iter().any(|p| match p.as_str() {
                "rcd" => t.rcd.is_some_and(|r| r <= self.max_rcd_ma),
                s => t.supply == s,
            })
    }

    /// What the standard says of a thing; `public` is a public bathroom.
    pub fn check(&self, bath: &Bath, t: &Thing, public: bool) -> Verdict {
        let Some(v) = self.volume(bath, t.at) else {
            return Verdict { volume: None, ip_min: None, ip_ok: None, allowed: true, why: Vec::new(), sources: Vec::new() };
        };
        let mut why = Vec::new();
        let mut sources = vec![v.source.clone()];
        let ip_min = if public { v.ip_public } else { v.ip };
        let ip_ok = t.ip.map(|ip| ip >= ip_min);
        match ip_ok {
            Some(false) => why.push(format!("IPX{} is less than the IPX{ip_min} volume {} asks", t.ip.unwrap_or(0), v.name)),
            None => why.push(format!("give `ip`: volume {} asks IPX{ip_min} at least", v.name)),
            Some(true) => {}
        }
        let mut allowed = true;
        if let Some(max) = v.selv_max_v {
            let selv = t.supply == "selv" && t.voltage.is_some_and(|u| u <= max);
            if !selv {
                allowed = false;
                why.push(format!("volume {} takes only SELV up to {max} V (give `supply` selv and `voltage`)", v.name));
            }
        }
        let rows: Vec<&Allow> = v.allows.iter().filter(|a| a.kinds.contains(&t.kind)).collect();
        let fit = rows.iter().find(|a| {
            (a.classes.is_empty() || t.class.as_ref().is_some_and(|c| a.classes.contains(c)))
                && self.protected(t, &a.any_of)
                && (!a.serving || t.serving)
        });
        match fit {
            Some(a) => sources.push(a.source.clone()),
            None => {
                allowed = false;
                if rows.is_empty() {
                    why.push(format!("no {} may stand in volume {}", t.kind, v.name));
                } else {
                    let ways: Vec<String> = rows
                        .iter()
                        .map(|a| {
                            let mut w = Vec::new();
                            if !a.classes.is_empty() {
                                w.push(format!("class {}", a.classes.join(" or ")));
                            }
                            if !a.any_of.is_empty() {
                                w.push(a.any_of.join(" or "));
                            }
                            if a.serving {
                                w.push("connecting only equipment of these volumes".into());
                            }
                            w.join(", ")
                        })
                        .collect();
                    why.push(format!("in volume {} a {} must be {}", v.name, t.kind, ways.join("; or ")));
                    sources.extend(rows.iter().map(|a| a.source.clone()));
                }
            }
        }
        Verdict { volume: Some(v.name.clone()), ip_min: Some(ip_min), ip_ok, allowed, why, sources }
    }
}

#[cfg(test)]
mod tests;
