//! The demand of an installation, and the category of service it asks of the utility, as the utility's own standard works them out.
//! A pure function of its input and the utility's rules, with no model behind it: the `demand` command is its only caller. The numbers are those of the
//! utility's document, which is not an ABNT standard (NBR 5410 gives no demand factors), so each is named by its utility and its
//! edition. CPFL works out a demand; ENEL and CELESC leave it to the designer and frame a supply by the installed load:
//!
//! - **CPFL** (CPFL Paulista, Piratininga, Santa Cruz and RGE): Norma Técnica GED 13, "Fornecimento em Tensão Secundária de
//!   Distribuição" (DIST-13-2026-RE-NOR), version 46.0, 19/03/2026. `D = a + b + c + d + e + f + g + h + i` (item 6.22.1): a
//!   lighting and outlets (Tabelas 2 and 3, or 15), b showers, taps, instant heaters and irons (Tabela 4), c boilers (Tabela 5), d
//!   dryers, ovens, dishwashers and microwaves (Tabela 6), e electric stoves (Tabela 7), f window air conditioners (Tabelas 8 and
//!   9), g motors (Tabelas 13, 14 and 10), h special equipment (Tabela 11, power factor 0.75), i hydromassage (Tabela 10). The
//!   category comes of the installed load up to 25 kW and of the demand above it (Tabelas 1A, 1B and 1C).
//!
//! - **Enel** (GRI-0017): the category by the installed load in Anexos A, C, D and E, or by the demand current in Anexo B.
//! - **Celesc** (N-321.0001): the category by the installed load in Tabela 01.
//!
//! Each utility's numbers are the `unit_demand` section of its electrical code, an amendment over NBR 5410
//! (`data/codes/electrical/`), with its source; the project may change any of them. The methods are here.
//!
//! What is not applied: the poles, boxes and the earthing conduit of Tabela 1 and its motor limits, Tabela 1D (two clients on one
//! pole), the motor starting rules of Tabela 12, and the power factors of discharge lighting in Tabela 15's buildings beyond the
//! one given.

use crate::codes::Steps;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// A load of `count` equal appliances of `w` watts each.
#[derive(Clone, Copy, Debug)]
pub struct Load {
    pub w: f64,
    pub count: u32,
}

/// Window air conditioners: the VA and the W of one, and how many.
#[derive(Clone, Copy, Debug)]
pub struct Air {
    pub va: f64,
    pub w: f64,
    pub count: u32,
}

/// Motors: the kVA and the kW of one, and how many.
#[derive(Clone, Copy, Debug)]
pub struct Motor {
    pub kva: f64,
    pub kw: f64,
    pub count: u32,
}

/// Special equipment: the kW of one, and how many.
#[derive(Clone, Debug)]
pub struct Special {
    /// One of the rules' `special` kinds.
    pub kind: String,
    pub kw: f64,
    pub count: u32,
}

/// What a building that is not a dwelling declares for its lighting and outlets: its use (Tabela 15), the kW and the power factor.
#[derive(Clone, Debug)]
pub struct OtherUse {
    pub kind: String,
    pub kw: f64,
    pub pf: f64,
}

#[derive(Clone, Debug)]
pub struct Input {
    pub residential: bool,
    pub area: Option<f64>,
    pub outlets: Option<u32>,
    pub lighting_points: u32,
    pub other: Option<OtherUse>,
    pub heaters: Vec<Load>,
    pub boilers: Vec<Load>,
    pub appliances: Vec<Load>,
    pub stoves: Vec<Load>,
    pub air: Vec<Air>,
    pub motors: Vec<Motor>,
    pub special: Vec<Special>,
    pub hydro: Vec<Load>,
    /// The air conditioners are for commercial use (Tabela 9), not residential (a factor of 1).
    pub commercial_air: bool,
}

/// The terms of `D`, in kVA.
#[derive(Clone, Debug, Default)]
pub struct Terms {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
    pub g: f64,
    pub h: f64,
    pub i: f64,
}

#[derive(Clone, Debug)]
pub struct Output {
    pub installed_kw: f64,
    pub lighting_outlets_w: f64,
    pub terms: Terms,
    pub demand_kva: f64,
}

/// A utility's standard for one unit: the `unit_demand` section of its electrical code, an amendment over NBR 5410.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Standard {
    /// The name a command gives it.
    pub utility: String,
    pub method: Method,
}

/// How the standard frames a unit's supply.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Method {
    /// A demand worked out term by term, then the category by the installed load and the demand (CPFL's GED 13).
    Terms(Box<ByTerms>),
    /// The category by the installed load in annexes by network, or by the demand current (Enel's GRI-0017).
    Annexes(Box<ByAnnexes>),
    /// The category by the installed load in one table by network (Celesc's N-321.0001 Tabela 01).
    Table(Box<ByTable>),
}

/// A row of a table of categories by the installed load (Tabelas 1A and 1B): up to `installed_up_to` kW, and up to
/// `demand_up_to` kVA where it says.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Category {
    pub name: String,
    /// 1 (one phase and the neutral), 2 or 3.
    pub phases: u8,
    pub installed_up_to: f64,
    #[serde(default)]
    pub demand_up_to: Option<f64>,
    /// The breaker (A), the copper PVC cable and its aluminium alternative (mm²), the conduit (mm), the earthing conductor (mm²).
    pub breaker: f64,
    pub cable: f64,
    pub aluminium: f64,
    pub conduit: f64,
    pub earthing: f64,
}

/// A supply system's categories.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Categories {
    pub system: String,
    pub rows: Vec<Category>,
}

/// A use of Tabela 15 and its tiers: (up to kW, or none for the rest; factor).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Use {
    #[serde(rename = "use")]
    pub use_: String,
    pub tiers: Vec<(Option<f64>, f64)>,
}

/// A kind of special equipment and its factors by rank, the biggest first.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecialFactors {
    pub kind: String,
    pub factors: Steps,
}

/// CPFL's way: `D = a + b + c + d + e + f + g + h + i`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByTerms {
    /// Tabela 2: (up to m² of built area, outlets, kitchen outlets) a dwelling has at least; above the last they are declared.
    pub least_outlets: Vec<(f64, u32, u32)>,
    /// The W of an outlet, of a kitchen outlet and of a lighting point, and the kitchen outlets of a dwelling whose area is not given.
    pub outlet: f64,
    pub kitchen_outlet: f64,
    pub kitchen_outlets: u32,
    pub lighting_point: f64,
    /// Tabela 3, by the kW of lighting and outlets; Tabelas 4 to 7, 9 and 10, by how many (the motors by rank).
    pub lighting: Steps,
    pub heaters: Steps,
    /// The uses whose showers and taps take Tabela 4 rather than a factor of 1.
    pub heaters_by_table: Vec<String>,
    pub boilers: Steps,
    pub appliances: Steps,
    pub stoves: Steps,
    pub air: Steps,
    pub motors: Steps,
    /// Tabela 8: (BTU/h, VA, W).
    pub window_air: Vec<(f64, f64, f64)>,
    /// Tabelas 13 and 14: (cv, kW, kVA).
    pub motors_single_phase: Vec<(f64, f64, f64)>,
    pub motors_three_phase: Vec<(f64, f64, f64)>,
    /// Tabela 15.
    pub uses: Vec<Use>,
    /// Tabela 11: the power factor of special equipment, and each kind's factors.
    pub special_power_factor: f64,
    pub special: Vec<SpecialFactors>,
    /// Tabelas 1A and 1B, the first system the one taken when none is named.
    pub categories: Vec<Categories>,
    /// Above this installed load (kW) the unit is served at primary voltage.
    pub primary_above: f64,
    pub not_covered: String,
}

/// A conductor of a connection branch: how many a phase has (none where the annex prints none), the phase's and the neutral's, mm².
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub conductors_per_phase: Option<u8>,
    pub phase: f64,
    pub neutral: f64,
}

/// The phase's and the neutral's section, mm².
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseNeutral {
    pub phase: f64,
    pub neutral: f64,
}

/// One row of an annex: a category of supply for an installed load over `over` up to `up_to` kW.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnexRow {
    pub name: String,
    pub phases: u8,
    pub over: f64,
    pub up_to: f64,
    /// The general breaker, A.
    pub breaker: f64,
    pub connection: Connection,
    pub entry: PhaseNeutral,
    /// The earthing conductor, copper, mm².
    pub earthing: f64,
    pub measurement: String,
    #[serde(default)]
    pub junction_box: Option<String>,
}

/// An annex: the network it is for and its rows; above `primary_above` kW the unit is served at primary voltage.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Annex {
    pub network: String,
    pub annex: String,
    #[serde(default)]
    pub primary_above: Option<f64>,
    pub rows: Vec<AnnexRow>,
}

/// A row of the annex by current: a supply by its breaker, which serves the names (and their phases) it lists.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurrentRow {
    pub names: Vec<String>,
    pub phases: Vec<u8>,
    pub breaker: f64,
    pub connection: Connection,
    /// The entry branch's phase conductor, mm².
    pub entry: f64,
    pub earthing: f64,
    pub measurement: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByCurrent {
    pub network: String,
    pub annex: String,
    pub rows: Vec<CurrentRow>,
}

/// Enel's way.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByAnnexes {
    pub networks: Vec<Annex>,
    pub by_current: ByCurrent,
    pub demand_note: String,
    /// `{kw}` is the annex's limit.
    pub primary_note: String,
    pub not_covered: String,
}

/// The copper and aluminium sections of an aerial connection branch, mm².
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Metals {
    pub copper: f64,
    pub aluminium: f64,
}

/// The copper and copper-clad steel sections of an earthing conductor, mm².
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Earthing {
    pub copper: f64,
    pub copper_clad_steel: f64,
}

/// One row of Celesc's Tabela 01: a category for an installed load over `over` up to `up_to` kW.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableRow {
    pub name: String,
    pub phases: u8,
    pub wires: u8,
    pub over: f64,
    pub up_to: f64,
    /// The demand (kVA) a three-phase row is printed with beside its load: over, up to.
    #[serde(default)]
    pub demand_over: Option<f64>,
    #[serde(default)]
    pub demand_up_to: Option<f64>,
    pub breaker: f64,
    pub connection: Metals,
    /// The entry, exit and load branch, copper, mm².
    pub entry: f64,
    pub earthing: Earthing,
}

/// A network's rows, and what is said past them.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableNetwork {
    pub network: String,
    pub rows: Vec<TableRow>,
    pub beyond: String,
}

/// Celesc's way.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByTable {
    pub table: String,
    /// The first is the one taken when none is named.
    pub networks: Vec<TableNetwork>,
    /// From this breaker up a three-phase supply asks the designer for a demand.
    pub asks_demand_from_breaker: f64,
    pub asks_demand_note: String,
    pub demand_note: String,
    pub not_covered: String,
}

/// BIMCraft's utilities, each one's code's `unit_demand` section, with its source: a project reads its own
/// (`Model::section_as`), and a utility named reads the library's.
pub fn standards() -> &'static [(Standard, String)] {
    static ALL: OnceLock<Vec<(Standard, String)>> = OnceLock::new();
    ALL.get_or_init(|| {
        crate::codes::of(crate::codes::Discipline::Electrical)
            .filter_map(|c| {
                let rules = c.section("unit_demand")?;
                let s = serde_json::from_value::<Standard>(serde_json::Value::Object(rules.clone())).ok()?;
                Some((s, c.source("unit_demand", "method").unwrap_or_default()))
            })
            .collect()
    })
}

/// A utility's standard by its name, in any case, with its source.
pub fn find(utility: &str) -> Option<&'static (Standard, String)> {
    standards().iter().find(|s| s.0.utility.eq_ignore_ascii_case(utility))
}

/// The names `find` knows.
pub fn names() -> Vec<&'static str> {
    standards().iter().map(|s| s.0.utility.as_str()).collect()
}

impl crate::codes::Valid for Standard {
    fn valid(&self) -> Result<(), String> {
        fn bad(mut xs: impl Iterator<Item = f64>) -> bool {
            xs.any(|x| !x.is_finite() || x < 0.0)
        }
        let fail = || Err("every number of a unit's demand is finite and not negative".to_string());
        match &self.method {
            Method::Terms(t) => {
                for (k, s) in [
                    ("lighting", &t.lighting),
                    ("heaters", &t.heaters),
                    ("boilers", &t.boilers),
                    ("appliances", &t.appliances),
                    ("stoves", &t.stoves),
                    ("air", &t.air),
                    ("motors", &t.motors),
                ] {
                    s.valid(k)?;
                }
                for s in &t.special {
                    s.factors.valid(&s.kind)?;
                }
                let rows = t.least_outlets.iter().map(|r| r.0);
                let equipment =
                    t.window_air.iter().chain(&t.motors_single_phase).chain(&t.motors_three_phase).flat_map(|r| [r.0, r.1, r.2]);
                let tiers = t.uses.iter().flat_map(|u| u.tiers.iter().flat_map(|x| [x.0.unwrap_or(0.0), x.1]));
                let cats = t.categories.iter().flat_map(|c| &c.rows).flat_map(|r| {
                    [r.installed_up_to, r.demand_up_to.unwrap_or(0.0), r.breaker, r.cable, r.aluminium, r.conduit, r.earthing]
                });
                let one = [t.outlet, t.kitchen_outlet, t.lighting_point, t.special_power_factor, t.primary_above];
                if bad(rows.chain(equipment).chain(tiers).chain(cats).chain(one)) {
                    return fail();
                }
                if t.categories.is_empty() || !(t.special_power_factor > 0.0 && t.special_power_factor <= 1.0) {
                    return Err("`categories` holds a system at least, and `special_power_factor` is over 0, up to 1".into());
                }
            }
            Method::Annexes(a) => {
                let rows = a.networks.iter().flat_map(|n| &n.rows).flat_map(|r| {
                    [r.over, r.up_to, r.breaker, r.connection.phase, r.connection.neutral, r.entry.phase, r.entry.neutral, r.earthing]
                });
                let current =
                    a.by_current.rows.iter().flat_map(|r| [r.breaker, r.connection.phase, r.connection.neutral, r.entry, r.earthing]);
                let primary = a.networks.iter().filter_map(|n| n.primary_above);
                if bad(rows.chain(current).chain(primary)) {
                    return fail();
                }
                if a.networks.is_empty() {
                    return Err("`networks` holds an annex at least".into());
                }
            }
            Method::Table(t) => {
                let rows = t.networks.iter().flat_map(|n| &n.rows).flat_map(|r| {
                    [r.over, r.up_to, r.demand_over.unwrap_or(0.0), r.demand_up_to.unwrap_or(0.0), r.breaker, r.connection.copper]
                        .into_iter()
                        .chain([r.connection.aluminium, r.entry, r.earthing.copper, r.earthing.copper_clad_steel])
                });
                if bad(rows.chain([t.asks_demand_from_breaker])) {
                    return fail();
                }
                if t.networks.is_empty() {
                    return Err("`networks` holds a network at least".into());
                }
            }
        }
        Ok(())
    }
}

impl ByTerms {
    /// Tabela 2: the outlets and the kitchen outlets a dwelling has at least, by built area (m²); `None` above the table, where they
    /// are declared.
    pub fn least_outlets(&self, area: f64) -> Option<(u32, u32)> {
        self.least_outlets.iter().find(|r| area <= r.0).map(|r| (r.1, r.2))
    }

    /// Tabela 8: a window air conditioner by its BTU/h, the VA and the W of one.
    pub fn window_air(&self, btu: f64) -> Option<(f64, f64)> {
        self.window_air.iter().find(|r| r.0 == btu).map(|r| (r.1, r.2))
    }

    /// Tabelas 13 (single-phase) and 14 (three-phase): a motor by its cv, its kVA and its kW.
    pub fn motor(&self, cv: f64, phases: u32) -> Option<(f64, f64)> {
        let rows = if phases == 1 { &self.motors_single_phase } else { &self.motors_three_phase };
        rows.iter().find(|r| (r.0 - cv).abs() < 1e-6).map(|r| (r.2, r.1))
    }

    /// The uses of Tabela 15, as a command gives them.
    pub fn uses(&self) -> Vec<&str> {
        self.uses.iter().map(|u| u.use_.as_str()).collect()
    }

    /// Tabela 15: the demand (kW) of a declared load of `kw` for a use; `None` for a use the table does not have.
    pub fn commercial_demand(&self, kind: &str, kw: f64) -> Option<f64> {
        let tiers = &self.uses.iter().find(|u| u.use_ == kind)?.tiers;
        let (mut left, mut from, mut total) = (kw.max(0.0), 0.0, 0.0);
        for (to, f) in tiers {
            let to = to.unwrap_or(f64::INFINITY);
            let part = left.min(to - from);
            total += part * f;
            left -= part;
            from = to;
            if left <= 0.0 {
                break;
            }
        }
        Some(total)
    }

    /// The kinds of special equipment.
    pub fn special_kinds(&self) -> Vec<&str> {
        self.special.iter().map(|s| s.kind.as_str()).collect()
    }

    /// The systems of Tabelas 1A and 1B.
    pub fn systems(&self) -> Vec<&str> {
        self.categories.iter().map(|c| c.system.as_str()).collect()
    }

    /// The category of service for an installed load `c` (kW) and a demand `d` (kVA) on `system`: the first row both hold, `None`
    /// past the table.
    pub fn category(&self, system: &str, c: f64, d: f64) -> Option<&Category> {
        let rows = &self.categories.iter().find(|x| x.system == system)?.rows;
        rows.iter().find(|r| c <= r.installed_up_to && r.demand_up_to.is_none_or(|up| d <= up))
    }

    /// The demand of an installation (item 6.22.1 of the GED) and the load it installs.
    pub fn compute(&self, i: &Input) -> Result<Output, String> {
        // a) lighting and outlets.
        let (lighting_outlets_w, a, kind) = if i.residential {
            let kitchen = i.area.and_then(|a| self.least_outlets(a)).map_or(self.kitchen_outlets, |x| x.1);
            let outlets_w = match (i.outlets, i.area) {
                (Some(n), _) => f64::from(n) * self.outlet + f64::from(kitchen) * self.kitchen_outlet,
                (None, Some(area)) => match self.least_outlets(area) {
                    Some((n, k)) => f64::from(n) * self.outlet + f64::from(k) * self.kitchen_outlet,
                    None => {
                        let last = self.least_outlets.last().map_or(0.0, |r| r.0);
                        return Err(format!(
                            "above {last} m² the outlets are declared: give `outlets` ({} W each; the kitchen's are added)",
                            self.outlet
                        ));
                    }
                },
                (None, None) => return Err("give the built `area` (m²) or the number of `outlets`".into()),
            };
            let w = outlets_w + f64::from(i.lighting_points) * self.lighting_point;
            (w, w / 1000.0 * self.lighting.at(w / 1000.0), "")
        } else {
            let o =
                i.other.as_ref().ok_or("a building that is not a dwelling declares its lighting and outlets: give `lighting_outlets`")?;
            let d = self.commercial_demand(&o.kind, o.kw).ok_or_else(|| format!("no use `{}` in Tabela 15", o.kind))?;
            (o.kw * 1000.0, d / o.pf, o.kind.as_str())
        };
        let mut t = Terms { a, ..Terms::default() };
        let mut installed = lighting_outlets_w / 1000.0;
        let count = |n: u32| f64::from(n);
        // b) showers, taps, instant heaters, irons.
        let (n, kw) = total(&i.heaters);
        installed += kw;
        t.b = kw * if i.residential || self.heaters_by_table.iter().any(|k| k == kind) { self.heaters.at(count(n.max(1))) } else { 1.0 };
        // c) boilers.
        let (n, kw) = total(&i.boilers);
        installed += kw;
        t.c = kw * self.boilers.at(count(n));
        // d) dryers, ovens, dishwashers, microwaves.
        let (n, kw) = total(&i.appliances);
        installed += kw;
        t.d = kw * self.appliances.at(count(n));
        // e) stoves.
        let (n, kw) = total(&i.stoves);
        installed += kw;
        t.e = kw * self.stoves.at(count(n));
        // f) window air conditioners: the installed load in W, the demand in VA.
        let n: u32 = i.air.iter().map(|a| a.count).sum();
        installed += i.air.iter().map(|a| a.w * f64::from(a.count)).sum::<f64>() / 1000.0;
        let va: f64 = i.air.iter().map(|a| a.va * f64::from(a.count)).sum::<f64>() / 1000.0;
        t.f = va * if i.commercial_air { self.air.at(count(n)) } else { 1.0 };
        // g) motors, the biggest first.
        let mut motors: Vec<f64> = i.motors.iter().flat_map(|m| std::iter::repeat_n(m.kva, m.count as usize)).collect();
        motors.sort_by(|a, b| b.total_cmp(a));
        installed += i.motors.iter().map(|m| m.kw * f64::from(m.count)).sum::<f64>();
        t.g = motors.iter().enumerate().map(|(r, kva)| kva * self.motors.at(r as f64 + 1.0)).sum();
        // h) special equipment at its power factor.
        for kind in &self.special {
            let mut v: Vec<f64> =
                i.special.iter().filter(|s| s.kind == kind.kind).flat_map(|s| std::iter::repeat_n(s.kw, s.count as usize)).collect();
            v.sort_by(|a, b| b.total_cmp(a));
            installed += v.iter().sum::<f64>();
            t.h += v.iter().enumerate().map(|(r, kw)| kw / self.special_power_factor * kind.factors.at(r as f64 + 1.0)).sum::<f64>();
        }
        // i) hydromassage, by Tabela 10, at a power factor of 1.
        let mut v: Vec<f64> = i.hydro.iter().flat_map(|l| std::iter::repeat_n(l.w / 1000.0, l.count as usize)).collect();
        v.sort_by(|a, b| b.total_cmp(a));
        installed += v.iter().sum::<f64>();
        t.i = v.iter().enumerate().map(|(r, kw)| kw * self.motors.at(r as f64 + 1.0)).sum();
        let demand_kva = t.a + t.b + t.c + t.d + t.e + t.f + t.g + t.h + t.i;
        Ok(Output { installed_kw: installed, lighting_outlets_w, terms: t, demand_kva })
    }
}

fn total(loads: &[Load]) -> (u32, f64) {
    (loads.iter().map(|l| l.count).sum(), loads.iter().map(|l| l.w * f64::from(l.count)).sum::<f64>() / 1000.0)
}

impl ByAnnexes {
    /// The networks' names, the annexes' and the one by current, in the file's order.
    pub fn network_names(&self) -> Vec<&str> {
        self.networks.iter().map(|n| n.network.as_str()).chain([self.by_current.network.as_str()]).collect()
    }

    pub fn annex(&self, network: &str) -> Option<&Annex> {
        self.networks.iter().find(|n| n.network == network)
    }

    /// The rows of an annex whose range holds `kw` (over `over`, up to `up_to`), those of `phases` phases if given; where none
    /// holds it (the annex prints whole kilowatts, so there is a gap between two ranges) the first that is higher. `None` above
    /// the table.
    pub fn pick<'a>(&self, annex: &'a Annex, kw: f64, phases: Option<u8>) -> Option<Vec<&'a AnnexRow>> {
        let rows: Vec<&AnnexRow> = annex.rows.iter().filter(|r| phases.is_none_or(|p| r.phases == p)).collect();
        let holding: Vec<&AnnexRow> =
            rows.iter().copied().filter(|r| kw > r.over && kw <= r.up_to || (r.over == 0.0 && kw <= r.up_to)).collect();
        if !holding.is_empty() {
            return Some(holding);
        }
        rows.iter().copied().find(|r| r.up_to >= kw).map(|r| vec![r])
    }

    /// The rows by current with the smallest breaker that holds `amps` (those of `phases` phases if given), the smaller supply
    /// first; `None` above the table.
    pub fn by_current(&self, amps: f64, phases: Option<u8>) -> Option<Vec<&CurrentRow>> {
        let serves = |r: &CurrentRow| phases.is_none_or(|p| r.phases.contains(&p));
        let rows = &self.by_current.rows;
        let breaker = rows.iter().filter(|r| serves(r) && r.breaker >= amps).map(|r| r.breaker).fold(f64::INFINITY, f64::min);
        let out: Vec<&CurrentRow> = rows.iter().filter(|r| serves(r) && r.breaker == breaker).collect();
        (!out.is_empty()).then_some(out)
    }
}

impl ByTable {
    pub fn network_names(&self) -> Vec<&str> {
        self.networks.iter().map(|n| n.network.as_str()).collect()
    }

    /// The row for an installed load of `kw` on a network: the one whose range holds it, of `phases` phases if given (a unit with
    /// a two- or three-phase appliance takes that supply whatever its load, items 5.3.2.2 and 5.3.2.3), else the first of those
    /// phases that goes higher. `None` above the table.
    pub fn pick<'a>(&self, network: &'a TableNetwork, kw: f64, phases: Option<u8>) -> Option<&'a TableRow> {
        let fits = |r: &&TableRow| phases.is_none_or(|p| r.phases == p);
        network
            .rows
            .iter()
            .filter(fits)
            .find(|r| kw > r.over && kw <= r.up_to || (r.over == 0.0 && kw <= r.up_to))
            .or_else(|| phases.and_then(|_| network.rows.iter().filter(fits).find(|r| r.up_to >= kw)))
    }
}

#[cfg(test)]
mod tests;
