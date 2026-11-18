//! Cold water sized as NBR 5626 (Sistemas prediais de água fria e água quente) asks. Pure functions of their input: the
//! `water_size` command is their caller, so none of this needs a pipe network or connectors in the model.
//!
//! - The method that gives a pipe's design flow is the project's to choose and to justify in its report. Two are offered:
//!   `weights`, the empirical method of NBR 5626:1998 Annex A, `Q = 0.3 √ΣP` (L/s), never more than the sum of the fixtures' own
//!   design flows (the 2020 edition has no such annex and asks for a recognised method, justified in the design, items 6.14.1 and
//!   6.14.2: this is offered as one); and `sum`, every fixture at once. The fixtures' design flows and relative weights are not the standard's: they
//!   are Tigre's table "Vazões de projeto e pesos relativos nos pontos de utilização" (Manual Técnico Tigre, 2021_10, page 198 of
//!   the PDF), the reference makers' table. Only the flush-tank WC, the lavatory, the sink, the shower and the tub (`laundry`,
//!   "Tanque", 0.25 L/s, weight 0.7) are taken from it.
//! - Head loss by the universal (Darcy-Weisbach) equation, the friction factor by Colebrook, water at 20 °C. A fitting counts as
//!   the equivalent length of Tigre's table "Perdas de carga localizadas - Sua equivalência em metros de tubulação de PVC rígido"
//!   (Manual Técnico Tigre, 2021_10, Apêndices, page 199 of the PDF; DE 20 to 110 mm). `LE` takes its columns Joelho 90°, Joelho 45°,
//!   Tê 90° passagem direita, Tê 90° saída de lado, Registro de gaveta aberto, Registro de globo aberto and Válvula de retenção tipo
//!   leve; all 63 numbers were compared with the page by program, with no difference.
//! - Speed 3 m/s at most (the standard lets a pipe go faster where water hammer and noise are shown not to matter); at least
//!   10 kPa dynamic at a point of use.

use serde::{Deserialize, Serialize};

/// The rules cold water is sized by: the Plumbing tab's Cold Water section (`data/codes/plumbing/nbr.json`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaterRules {
    /// The method a design flow is worked out by when the caller names none: `weights` or `sum`.
    pub flow_method: String,
    /// `Q = k √ΣP`'s `k`, L/s.
    pub flow_coefficient: f64,
    /// The fastest water may go, m/s.
    pub max_speed: f64,
    /// The least dynamic pressure at a point of use, kPa.
    pub min_use_kpa: f64,
    /// The pipes' roughness, mm.
    pub roughness: f64,
    /// The pipes: (DN, the outside diameter, and wall) in mm.
    pub pipes: Vec<[f64; 2]>,
    /// The fittings `water_size` counts, in the order of `equivalent_lengths`' columns after the DN.
    pub fittings: Vec<String>,
    /// Equivalent lengths (m) by DN: `[DN, one for each fitting…]`.
    pub equivalent_lengths: Vec<Vec<f64>>,
}

impl crate::codes::Section for WaterRules {
    const DISCIPLINE: crate::codes::Discipline = crate::codes::Discipline::Plumbing;
    const KEY: &'static str = "cold_water";
    fn check(&self) -> Result<(), String> {
        let positive = |v: f64| v.is_finite() && v > 0.0;
        for (k, v) in [("flow_coefficient", self.flow_coefficient), ("max_speed", self.max_speed), ("roughness", self.roughness)] {
            if !positive(v) || v > 1000.0 {
                return Err(format!("cold water: `{k}` is a positive number, not {v}"));
            }
        }
        if !self.min_use_kpa.is_finite() || !(0.0..=10_000.0).contains(&self.min_use_kpa) {
            return Err(format!("cold water: `min_use_kpa` is from 0 to 10000 kPa, not {}", self.min_use_kpa));
        }
        if Method::parse(&self.flow_method).is_none() {
            return Err(format!("cold water: `flow_method` is weights or sum, not `{}`", self.flow_method));
        }
        if self.pipes.is_empty()
            || self.pipes.len() > 200
            || self.pipes.iter().any(|[dn, wall]| !positive(*dn) || !positive(*wall) || 2.0 * wall >= *dn)
        {
            return Err("cold water: `pipes` are [DN, wall] in mm, the wall under half the DN, 200 at most".into());
        }
        if self.pipes.windows(2).any(|w| w[0][0] >= w[1][0]) {
            return Err("cold water: `pipes` go from the smallest DN up".into());
        }
        if self.fittings.len() > 50 || self.equivalent_lengths.len() > 200 {
            return Err("cold water: 50 fittings and 200 rows of equivalent lengths at most".into());
        }
        for row in &self.equivalent_lengths {
            if row.len() != self.fittings.len() + 1 || row.iter().any(|v| !v.is_finite() || *v < 0.0) {
                return Err("cold water: each row of `equivalent_lengths` is a DN and a length for each fitting, none negative".into());
            }
        }
        Ok(())
    }
}

impl WaterRules {
    /// The design flow (L/s) of fixtures given as (kind, count), by `fixtures`' flows and weights.
    pub fn design_flow(&self, method: Method, fixtures: &[(&str, f64)], library: &crate::plumbing::Fixtures) -> f64 {
        let (mut q, mut w) = (0.0, 0.0);
        for (kind, n) in fixtures {
            if let Some(f) = library.fixture(kind) {
                q += f.cold_flow * n;
                w += f.weight * n;
            }
        }
        match method {
            Method::Sum => q,
            Method::Weights => (self.flow_coefficient * w.sqrt()).min(q),
        }
    }

    /// The inner diameter (mm) of a pipe by its DN, or `None` for a DN the table does not list.
    pub fn inside(&self, dn: f64) -> Option<f64> {
        self.pipes.iter().find(|p| p[0] == dn).map(|p| p[0] - 2.0 * p[1])
    }

    /// The DNs the pipes come in.
    pub fn dns(&self) -> Vec<f64> {
        self.pipes.iter().map(|p| p[0]).collect()
    }

    /// A fitting's equivalent length (m) in a DN, or `None` for a DN or a fitting the table does not have.
    pub fn equivalent_length(&self, dn: f64, fitting: &str) -> Option<f64> {
        let col = self.fittings.iter().position(|f| f == fitting)?;
        self.equivalent_lengths.iter().find(|r| r.first() == Some(&dn)).and_then(|r| r.get(col + 1).copied())
    }

    /// The smallest DN whose speed at `q` L/s is within `max_speed`, or `None` when the table has none that carries it.
    pub fn smallest_dn(&self, q: f64) -> Option<f64> {
        self.dns().into_iter().find(|dn| self.inside(*dn).is_some_and(|d| head_loss(q, d, self.roughness).0 <= self.max_speed + 1e-9))
    }
}

/// The library's cold water rules.
fn library() -> std::sync::Arc<WaterRules> {
    crate::codes::library_rules::<WaterRules>()
}

/// A metre of water, kPa.
pub const KPA_PER_M: f64 = 9.81;
const NU: f64 = 1.004e-6;
const G: f64 = 9.81;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Weights,
    Sum,
}

impl Method {
    pub fn parse(s: &str) -> Option<Method> {
        match s {
            "weights" => Some(Method::Weights),
            "sum" => Some(Method::Sum),
            _ => None,
        }
    }
}

/// A fixture's (design flow L/s, weight), or `None` for a kind the table does not have.
pub fn fixture(kind: &str) -> Option<(f64, f64)> {
    crate::plumbing::fixture(kind).map(|f| (f.cold_flow, f.weight))
}

/// The design flow (L/s) of fixtures given as (kind, count), by the library's rules.
pub fn design_flow(method: Method, fixtures: &[(&str, f64)]) -> f64 {
    library().design_flow(method, fixtures, &crate::plumbing::fixtures())
}

/// The inner diameter (mm) of a library pipe's DN, or `None` for a DN it does not list.
pub fn inside(dn: f64) -> Option<f64> {
    library().inside(dn)
}

/// A fitting's equivalent length (m) in a DN, by the library's table.
pub fn equivalent_length(dn: f64, fitting: &str) -> Option<f64> {
    library().equivalent_length(dn, fitting)
}

/// Colebrook's friction factor for a Reynolds number and a relative roughness.
fn colebrook(re: f64, rel: f64) -> f64 {
    if re < 2300.0 {
        return 64.0 / re;
    }
    let mut f = 0.25 / (rel / 3.7 + 5.74 / re.powf(0.9)).log10().powi(2);
    for _ in 0..60 {
        let x = -2.0 * (rel / 3.7 + 2.51 / (re * f.sqrt())).log10();
        let g = 1.0 / (x * x);
        let done = (g - f).abs() < 1e-14;
        f = g;
        if done {
            break;
        }
    }
    f
}

/// Speed (m/s) and head loss per metre (m/m) for `q` L/s in an inner diameter `d` mm of roughness `eps` mm.
pub fn head_loss(q: f64, d: f64, eps: f64) -> (f64, f64) {
    if !(q > 0.0 && d > 0.0) {
        return (0.0, 0.0);
    }
    let dm = d / 1000.0;
    let v = q / 1000.0 / (std::f64::consts::PI * dm * dm / 4.0);
    let f = colebrook(v * dm / NU, eps / d);
    (v, f / dm * v * v / (2.0 * G))
}

/// The smallest library DN whose speed at `q` L/s is within its most speed.
pub fn smallest_dn(q: f64) -> Option<f64> {
    library().smallest_dn(q)
}

#[cfg(test)]
mod tests;
