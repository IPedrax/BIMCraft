//! A building's demand, for the supply of a building of dwellings: the method of a utility or of a country, read from
//! `data/building-demand.json`. A pure function of what it is given, with no model behind it: the `building_demand` command is its
//! only caller. `demand.rs` is the other half, one installation's demand and category by a utility's own tables.
//!
//! The methods agree on a shape, which is what lets them be data: a building of `n` dwellings asks
//!
//! ```text
//! D = factor · F(n) · d + (factor or 1) · common + special + commercial + premises
//! ```
//!
//! - `d` is one dwelling's demand: by its floor area, from a table and past its end a curve `c·A^e` (the Brazilian utilities, after
//!   CODI's RTD-027), the mean area of dwellings of different sizes weighted by their number; or as declared, the mean of the
//!   dwellings' own demands, none under a least (Spain's ITC-BT-10).
//! - `F(n)` is an equivalent number of dwellings, a table by `n` and, past it, a step for each dwelling more.
//! - `factor` is a method's own constant (a growth of 1.2, Cemig's 1.4), on the common areas too where the method says so.
//! - `common` is the common areas' demand: an installed load of a class (lighting, outlets) by bands of demand factors and a power
//!   factor, where the method gives them, and what the designer worked out (lifts, pumps).
//! - `special` counts whole (an electric vehicle charging station), `commercial` is the shops' and offices' demand, and `premises`
//!   the loads a method gives by floor area (ITC-BT-10's shops and garages).
//!
//! The demand then picks the building's entrance from the method's table, where it has one. Every table keeps the values as
//! printed; an erratum corrects a row that the same table in other standards prints otherwise, and `as_printed` uses the printed
//! value. A utility or a country is added as an entry of the file, never as code.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// What a method takes the mean of, for dwellings that are not alike.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeanOf {
    /// The floor area, weighted by the number of dwellings of each; the demand is the mean area's.
    Area,
    /// Each dwelling's declared demand.
    Demand,
}

/// A row of a table that the same table elsewhere prints otherwise.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Erratum {
    /// The row's key (a number of dwellings, an area).
    pub at: f64,
    /// The value as the table prints it.
    pub printed: f64,
    /// The value taken.
    pub value: f64,
    /// Why, in words: the other standards that print the value taken.
    pub why: String,
}

/// A demand past the end of a table: `coefficient · A^exponent`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Curve {
    pub coefficient: f64,
    pub exponent: f64,
    pub source: String,
}

/// A named demand for a dwelling, such as ITC-BT-10's grades of electrification.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grade {
    pub name: String,
    pub demand: f64,
    pub source: String,
}

/// How a method finds one dwelling's demand.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dwelling {
    pub source: String,
    /// [up to m², demand]: an area takes the first row that reaches it.
    #[serde(default)]
    pub by_area: Vec<(f64, f64)>,
    /// Past the last row of `by_area`.
    #[serde(default)]
    pub above: Option<Curve>,
    #[serde(default)]
    pub grades: Vec<Grade>,
    /// The least a dwelling's declared demand may be.
    #[serde(default)]
    pub least: Option<f64>,
}

/// The equivalent number of dwellings by their number.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diversity {
    pub source: String,
    /// [up to n, F]: a number takes the first row that reaches it.
    pub steps: Vec<(f64, f64)>,
    /// What each dwelling past the last row adds; the table ends there without it.
    #[serde(default)]
    pub beyond: Option<f64>,
    #[serde(default)]
    pub errata: Vec<Erratum>,
}

/// A band of a demand factor: the load up to `up_to` (kW, from the band before), or all the rest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Band {
    #[serde(default)]
    pub up_to: Option<f64>,
    pub factor: f64,
}

/// The common areas' demand of a class of load, from its installed load.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonClass {
    pub class: String,
    pub power_factor: f64,
    pub bands: Vec<Band>,
    pub source: String,
}

/// A load other than the dwellings that a method gives by floor area.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Premise {
    #[serde(rename = "use")]
    pub use_: String,
    pub w_per_m2: f64,
    pub least_w: f64,
    pub source: String,
}

/// A row of an entrance table: a demand over `over`, up to `up_to`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntranceRow {
    pub over: f64,
    pub up_to: f64,
    pub breaker_a: f64,
    /// The other columns, as the table prints them (`None` where it prints none).
    pub columns: BTreeMap<String, Option<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entrance {
    pub source: String,
    pub rows: Vec<EntranceRow>,
}

/// One method, an entry of `data/building-demand.json`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Method {
    /// Its name, which `building_demand {standard}` takes.
    pub standard: String,
    /// The utilities that follow it, which `building_demand {utility}` takes.
    #[serde(default)]
    pub utilities: Vec<String>,
    /// ISO 3166 code.
    pub country: String,
    pub source: String,
    /// kVA or kW: the unit of every demand in and out.
    pub unit: String,
    pub min_dwellings: u32,
    pub mean_of: MeanOf,
    pub dwelling: Dwelling,
    pub diversity: Diversity,
    pub factor: f64,
    pub factor_source: String,
    /// Whether `factor` multiplies the common areas' demand too.
    pub factor_on_common: bool,
    #[serde(default)]
    pub common: Vec<CommonClass>,
    #[serde(default)]
    pub premises: Vec<Premise>,
    #[serde(default)]
    pub entrance: Option<Entrance>,
    #[serde(default)]
    pub notes: Vec<String>,
    pub not_covered: Vec<String>,
}

#[derive(Deserialize)]
struct Data {
    methods: Vec<Method>,
}

/// The methods of `data/building-demand.json`; none if it does not read, which a test keeps from happening.
pub fn methods() -> &'static [Method] {
    static ALL: OnceLock<Vec<Method>> = OnceLock::new();
    ALL.get_or_init(|| serde_json::from_str::<Data>(include_str!("../data/building-demand.json")).map(|d| d.methods).unwrap_or_default())
}

/// A method by its standard's name, or by a utility that follows it (in any case).
pub fn method(name: &str) -> Option<&'static Method> {
    let all = methods();
    all.iter().find(|m| m.standard == name).or_else(|| all.iter().find(|m| m.utilities.iter().any(|u| u.eq_ignore_ascii_case(name))))
}

/// The first row of `steps` that reaches `x`.
fn reach(steps: &[(f64, f64)], x: f64) -> Option<(f64, f64)> {
    steps.iter().copied().find(|s| s.0 >= x)
}

impl Method {
    /// One dwelling's demand by its floor area: the row that reaches it, or the curve past the table.
    pub fn by_area(&self, area: f64) -> Option<f64> {
        match reach(&self.dwelling.by_area, area) {
            Some((_, d)) => Some(d),
            None => {
                self.dwelling.above.as_ref().filter(|_| !self.dwelling.by_area.is_empty()).map(|c| c.coefficient * area.powf(c.exponent))
            }
        }
    }

    /// The equivalent number of dwellings for `n`, and the erratum it took, if any; `None` under the method's least number.
    pub fn diversity(&self, n: u32, as_printed: bool) -> Option<(f64, Option<&Erratum>)> {
        if n < self.min_dwellings {
            return None;
        }
        let n = f64::from(n);
        let d = &self.diversity;
        match reach(&d.steps, n) {
            Some((at, printed)) => {
                let fix = d.errata.iter().find(|e| e.at == at).filter(|_| !as_printed);
                Some((fix.map_or(printed, |e| e.value), fix))
            }
            None => {
                let (at, last) = d.steps.last().copied()?;
                let fix = d.errata.iter().find(|e| e.at == at).filter(|_| !as_printed);
                d.beyond.map(|b| (fix.map_or(last, |e| e.value) + (n - at) * b, None))
            }
        }
    }

    /// The common areas' demand of an installed load `kw` of a class, in the method's unit.
    pub fn common_demand(&self, class: &str, kw: f64) -> Option<f64> {
        let c = self.common.iter().find(|c| c.class == class)?;
        let (mut from, mut demand) = (0.0_f64, 0.0);
        for b in &c.bands {
            let to = b.up_to.unwrap_or(f64::INFINITY);
            demand += (kw.min(to) - from).max(0.0) * b.factor;
            from = to;
        }
        Some(if self.unit == "kVA" { demand / c.power_factor } else { demand })
    }

    /// A premise's load by its floor area, in the method's unit.
    pub fn premise(&self, use_: &str, area: f64) -> Option<f64> {
        self.premises.iter().find(|p| p.use_ == use_).map(|p| (area * p.w_per_m2).max(p.least_w) / 1000.0)
    }

    /// The entrance a demand asks, `None` outside the table.
    pub fn entrance_for(&self, demand: f64) -> Option<&EntranceRow> {
        self.entrance.as_ref()?.rows.iter().find(|r| demand > r.over && demand <= r.up_to)
    }
}

/// Dwellings alike: their number and their floor area (m²), or their demand, or a grade naming it.
#[derive(Clone, Debug, Default)]
pub struct Group {
    pub count: u32,
    pub area: Option<f64>,
    pub demand: Option<f64>,
    pub grade: Option<String>,
}

/// What a building has, every demand in the method's unit.
#[derive(Clone, Debug, Default)]
pub struct Input {
    pub dwellings: Vec<Group>,
    /// The common areas' installed load (kW) by class.
    pub common: Vec<(String, f64)>,
    /// The common areas' demand the designer worked out.
    pub common_demand: f64,
    pub special: f64,
    pub commercial: f64,
    /// Floor areas (m²) by use.
    pub premises: Vec<(String, f64)>,
    pub as_printed: bool,
}

/// The terms of the demand.
#[derive(Clone, Debug, Default)]
pub struct Terms {
    pub dwellings: f64,
    pub common: f64,
    pub special: f64,
    pub commercial: f64,
    pub premises: f64,
}

#[derive(Debug)]
pub struct Output<'a> {
    pub dwellings: u32,
    pub mean_area: Option<f64>,
    /// One dwelling's demand, the mean's.
    pub per_dwelling: f64,
    pub diversity: f64,
    pub terms: Terms,
    pub demand: f64,
    pub errata: Vec<&'a Erratum>,
    /// Dwellings whose declared demand was under the least, raised to it.
    pub raised: u32,
    pub entrance: Option<&'a EntranceRow>,
}

/// A building's demand by `m`.
pub fn compute<'a>(m: &'a Method, i: &Input) -> Result<Output<'a>, String> {
    let n: u32 = i.dwellings.iter().try_fold(0u32, |a, g| a.checked_add(g.count)).ok_or("too many dwellings")?;
    if n < m.min_dwellings {
        return Err(format!("{} covers {} dwellings or more, not {n}", m.standard, m.min_dwellings));
    }
    let mut raised = 0;
    let (mean_area, per) = match m.mean_of {
        MeanOf::Area => {
            if i.dwellings.iter().any(|g| g.area.is_none() || g.demand.is_some() || g.grade.is_some()) {
                return Err(format!("{} takes each dwelling's floor `area` (m²), not a demand", m.standard));
            }
            let area = i.dwellings.iter().map(|g| f64::from(g.count) * g.area.unwrap_or(0.0)).sum::<f64>() / f64::from(n);
            let last = m.dwelling.by_area.last().map_or(0.0, |s| s.0);
            let d = m.by_area(area).ok_or_else(|| format!("{}'s table ends at {last} m²; the mean area is {area:.1} m²", m.standard))?;
            (Some(area), d)
        }
        MeanOf::Demand => {
            let least = m.dwelling.least.unwrap_or(0.0);
            let mut sum = 0.0;
            for g in &i.dwellings {
                let d = match (g.demand, &g.grade, g.area) {
                    (Some(d), None, None) => d,
                    (None, Some(name), None) => m.dwelling.grades.iter().find(|x| &x.name == name).map(|x| x.demand).ok_or_else(|| {
                        let names: Vec<&str> = m.dwelling.grades.iter().map(|x| x.name.as_str()).collect();
                        format!("{} has the grades {}, not `{name}`", m.standard, names.join(", "))
                    })?,
                    _ => return Err(format!("{} takes each dwelling's `demand` ({}) or its `grade`, not its area", m.standard, m.unit)),
                };
                if d < least {
                    raised += g.count;
                }
                sum += f64::from(g.count) * d.max(least);
            }
            (None, sum / f64::from(n))
        }
    };
    let (f, erratum) = m.diversity(n, i.as_printed).ok_or_else(|| {
        format!("{}'s diversity table ends at {} dwellings, not {n}", m.standard, m.diversity.steps.last().map_or(0.0, |s| s.0))
    })?;
    let mut common = i.common_demand;
    for (class, kw) in &i.common {
        common += m.common_demand(class, *kw).ok_or_else(|| {
            let names: Vec<&str> = m.common.iter().map(|c| c.class.as_str()).collect();
            if names.is_empty() {
                format!("{} gives no demand factors for the common areas: give their demand in `common_demand`", m.standard)
            } else {
                format!("{} gives demand factors for {}, not `{class}`", m.standard, names.join(", "))
            }
        })?;
    }
    let mut premises = 0.0;
    for (use_, area) in &i.premises {
        premises += m.premise(use_, *area).ok_or_else(|| {
            let names: Vec<&str> = m.premises.iter().map(|p| p.use_.as_str()).collect();
            if names.is_empty() {
                format!("{} gives no loads by floor area: give the demand in `commercial`", m.standard)
            } else {
                format!("{} gives loads by floor area for {}, not `{use_}`", m.standard, names.join(", "))
            }
        })?;
    }
    let terms = Terms {
        dwellings: m.factor * f * per,
        common: if m.factor_on_common { m.factor * common } else { common },
        special: i.special,
        commercial: i.commercial,
        premises,
    };
    let demand = terms.dwellings + terms.common + terms.special + terms.commercial + terms.premises;
    Ok(Output {
        dwellings: n,
        mean_area,
        per_dwelling: per,
        diversity: f,
        terms,
        demand,
        errata: erratum.into_iter().collect(),
        raised,
        entrance: m.entrance_for(demand),
    })
}

#[cfg(test)]
mod tests;
