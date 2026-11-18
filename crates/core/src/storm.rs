//! Storm drains sized as NBR 10844 (Instalações prediais de águas pluviais, 1989) asks. Pure functions of their input: the
//! `storm_size` command is their caller, so none of this needs a pipe network in the model.
//!
//! - The design flow is `Q = I * A / 60` (L/min, item 5.3.1): `I` the rainfall intensity in mm/h (item 5.1: by place and return
//!   period; item 5.1.4: "para construção até 100 m² de área de projeção horizontal, salvo casos especiais, pode-se adotar I = 150 mm/h") and `A` the area that drains, in m², as the standard
//!   counts it.
//! - A downpipe ("condutor vertical") takes the inner diameter its Figure 3 abacuses give: (a) a gutter with a sharp-edged
//!   outlet, (b) with a funnel outlet, by `Q`, the water depth in the gutter `H` (mm) and the downpipe's length `L` (m): the
//!   higher of what the `H` curves and the `L` curves give at `Q`, interpolating between curves; at least 70 mm inside.
//! - A horizontal drain ("condutor horizontal") takes Manning-Strickler with the water at two thirds of the inner diameter,
//!   `Q = 60000 A R^(2/3) i^(1/2) / n` (L/min), `n` 0.011 for PVC, which the standard's Table 2 gives (Table 4 is the flows computed
//!   with it); PVC is the only material here; at least 0.5 % slope.
//!
//! Source: NBR 10844:1989 itself (a paid ABNT standard), read from a licensed copy: item 5.6.4 (the method: raise a vertical at Q to
//! the H and L curves, interpolate between existing curves when there is none, take the highest intersection, carry it to the D
//! axis), 5.6.3 (70 mm), 5.7.1 (0.5 %), 5.7.2 (two thirds full) and Figure 3, "Ábacos para a determinação de diâmetros de condutores
//! verticais", (a) gutter with a sharp-edged outlet, (b) with a funnel outlet, on page 8 of the copy (the page of 5.6.4 is 7). The
//! abacuses were built for rough pipes (f = 0.04) with two bends at the base (5.6.4.2). Table 4 (Manning, n 0.011) is a formula and
//! is computed, not stored; only its 32 values are in the tests, as a check. Figure 3 is a drawing, not a table, so the fits are this
//! project's own: each curve was digitised from the figure, its own grid calibrating the reading. The standard adopts the nominal
//! diameter whose inside diameter is at least the value read; the command replies the inside diameter read, the nominal is the
//! caller's choice. Each `H` curve is a straight line `D = m Q + b` (mm, L/min) and each `L` curve `ln Q = c0 + c1 ln D + c2 (ln D)^2`, to
//! within 0.2 to 0.6 mm (rms) of the drawn lines. Off the chart (H under 50 or over 100 mm, L under 0.3 m, D over 150 mm)
//! nothing is read, and the caller says so.

use serde::{Deserialize, Serialize};

/// The rules storm water is sized by: the Plumbing tab's Storm Drainage section (`data/codes/plumbing/nbr.json`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StormRules {
    /// The rainfall intensity the code allows up to `default_rain_max_area` m² of roof in plan, mm/h; above it the caller (or
    /// the project's Site and Climate) gives it.
    pub default_rain: f64,
    pub default_rain_max_area: f64,
    /// The smallest inner diameter of a downpipe, mm.
    pub min_downpipe: f64,
    /// The smallest slope of a horizontal drain, as a fraction.
    pub min_slope: f64,
    /// Manning's roughness by material.
    pub roughness: Vec<Roughness>,
    /// The abacuses' diameter axis (mm), the water depths their `H` curves span (mm), and the shortest downpipe (m).
    pub diameter_axis: [f64; 2],
    pub depth_range: [f64; 2],
    pub min_length: f64,
    /// The downpipe abacuses: a gutter with a sharp-edged outlet, and with a funnel outlet.
    pub downpipe_curves: Curves,
}

/// A material's roughness.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roughness {
    pub material: String,
    pub n: f64,
}

/// The two abacuses.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Curves {
    pub edge: Abacus,
    pub funnel: Abacus,
}

/// One abacus: the `H` lines (depth mm, slope m, intercept b: `D = m Q + b`) and the `L` curves (length m, none for any
/// longer, and c0, c1, c2: `ln Q = c0 + c1 ln D + c2 (ln D)²`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Abacus {
    pub depths: Vec<[f64; 3]>,
    pub lengths: Vec<LengthCurve>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LengthCurve {
    pub length: Option<f64>,
    pub c: [f64; 3],
}

impl crate::codes::Section for StormRules {
    const DISCIPLINE: crate::codes::Discipline = crate::codes::Discipline::Plumbing;
    const KEY: &'static str = "storm";
    fn check(&self) -> Result<(), String> {
        let positive = |v: f64| v.is_finite() && v > 0.0 && v < 1.0e6;
        for (k, v) in [
            ("default_rain", self.default_rain),
            ("default_rain_max_area", self.default_rain_max_area),
            ("min_downpipe", self.min_downpipe),
            ("min_slope", self.min_slope),
            ("min_length", self.min_length),
        ] {
            if !positive(v) {
                return Err(format!("storm drainage: `{k}` is a positive number, not {v}"));
            }
        }
        if self.roughness.is_empty()
            || self.roughness.len() > 50
            || self.roughness.iter().any(|r| !positive(r.n) || r.material.chars().count() > 64)
        {
            return Err("storm drainage: a positive `n` for each material, 50 at most".into());
        }
        for (k, [lo, hi]) in [("diameter_axis", self.diameter_axis), ("depth_range", self.depth_range)] {
            if !positive(lo) || !positive(hi) || lo >= hi {
                return Err(format!("storm drainage: `{k}` is a least and a most, the least smaller"));
            }
        }
        for a in [&self.downpipe_curves.edge, &self.downpipe_curves.funnel] {
            let finite = a.depths.iter().flatten().chain(a.lengths.iter().flat_map(|l| l.c.iter())).all(|v| v.is_finite());
            if a.depths.is_empty() || a.lengths.is_empty() || a.depths.len() > 100 || a.lengths.len() > 100 || !finite {
                return Err("storm drainage: each abacus has its depth lines and its length curves, of finite numbers".into());
            }
            let lengths: Vec<f64> = a.lengths.iter().map(|l| l.length.unwrap_or(f64::INFINITY)).collect();
            if a.depths.windows(2).any(|w| w[0][0] >= w[1][0])
                || lengths.windows(2).any(|w| w[0] >= w[1])
                || lengths.iter().any(|l| *l <= 0.0)
            {
                return Err(
                    "storm drainage: an abacus's depths and lengths go from the smallest up, the last length none (any longer)".into()
                );
            }
        }
        Ok(())
    }
}

impl StormRules {
    /// Manning's `n` of a material.
    pub fn n(&self, material: &str) -> Option<f64> {
        self.roughness.iter().find(|r| r.material == material).map(|r| r.n)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outlet {
    /// A gutter with a sharp-edged outlet: Figure 3 (a).
    Edge,
    /// A gutter with a funnel outlet: Figure 3 (b).
    Funnel,
}

impl Outlet {
    pub fn parse(s: &str) -> Option<Outlet> {
        match s {
            "edge" => Some(Outlet::Edge),
            "funnel" => Some(Outlet::Funnel),
            _ => None,
        }
    }
}

/// What a downpipe's abacus reading is off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Off {
    /// The water depth in the gutter is under 50 or over 100 mm.
    Depth,
    /// The downpipe is shorter than 0.3 m.
    Length,
    /// It needs more than 150 mm inside.
    Diameter,
}

/// A downpipe's inner diameter in mm (at least 70), and what the `H` and `L` curves each gave.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Downpipe {
    pub diameter: f64,
    pub by_depth: f64,
    pub by_length: f64,
}

fn ln_q(c: &[f64; 3], d: f64) -> f64 {
    c[0] + c[1] * d.ln() + c[2] * d.ln().powi(2)
}

/// `D` (mm) on an `L` curve at `Q`: the axis's foot when the curve is above `Q` there, infinite beyond its top.
fn d_on_curve(c: &[f64; 3], q: f64, [foot, top]: [f64; 2]) -> f64 {
    let t = q.ln();
    if ln_q(c, foot) >= t {
        return foot;
    }
    if ln_q(c, top) < t {
        return f64::INFINITY;
    }
    let (mut lo, mut hi) = (foot, top);
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if ln_q(c, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) / 2.0
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// The inner diameter a downpipe needs, as the library's abacus gives it, for the flow `q` (L/min), the water depth `h` (mm) and
/// the length `l` (m).
pub fn downpipe_diameter(outlet: Outlet, q: f64, h: f64, l: f64) -> Result<Downpipe, Off> {
    downpipe_by(&crate::codes::library_rules::<StormRules>(), outlet, q, h, l)
}

/// The inner diameter a downpipe needs, as `r`'s abacus gives it.
pub fn downpipe_by(r: &StormRules, outlet: Outlet, q: f64, h: f64, l: f64) -> Result<Downpipe, Off> {
    let a = if outlet == Outlet::Funnel { &r.downpipe_curves.funnel } else { &r.downpipe_curves.edge };
    let ([h_lo, h_hi], [d_lo, d_hi]) = (r.depth_range, r.diameter_axis);
    if h.is_nan() || !(h_lo - 1e-9..=h_hi + 1e-9).contains(&h) {
        return Err(Off::Depth);
    }
    if l.is_nan() || l < r.min_length - 1e-9 {
        return Err(Off::Length);
    }
    if q.is_nan() || q <= 0.0 {
        return Ok(Downpipe { diameter: r.min_downpipe, by_depth: 0.0, by_length: 0.0 });
    }
    if a.depths.is_empty() || a.lengths.is_empty() {
        return Err(Off::Diameter);
    }
    // the H curves: straight lines, linear between them
    let at = |row: &[f64; 3]| row[1] * q + row[2];
    let last = a.depths.len() - 1;
    let hi = a.depths.iter().position(|r| r[0] >= h - 1e-9).unwrap_or(last);
    let (h1, h0) = (a.depths[hi], a.depths[hi.saturating_sub(1)]);
    let by_depth = if h1[0] == h || hi == 0 { at(&h1) } else { lerp(at(&h0), at(&h1), (h - h0[0]) / (h1[0] - h0[0])) };
    // the L curves, each never above the one of the shorter downpipe; linear in L between them (in 1/L between 25 m and infinity)
    let mut ds: Vec<f64> = Vec::with_capacity(a.lengths.len());
    for c in &a.lengths {
        let d = d_on_curve(&c.c, q, [d_lo, d_hi]);
        ds.push(ds.last().map_or(d, |prev| d.min(*prev)));
    }
    let len = |c: &LengthCurve| c.length.unwrap_or(f64::INFINITY);
    let j = a.lengths.iter().position(|c| len(c) >= l - 1e-9).unwrap_or(a.lengths.len() - 1);
    let (c1, c0) = (len(&a.lengths[j]), len(&a.lengths[j.saturating_sub(1)]));
    let by_length = if c1 == l || j == 0 {
        ds[j]
    } else if c1.is_infinite() {
        lerp(ds[j - 1], ds[j], (1.0 / c0 - 1.0 / l) / (1.0 / c0))
    } else {
        lerp(ds[j - 1], ds[j], (l - c0) / (c1 - c0))
    };
    // a curve off the chart between two of the L curves gives NaN, which `max` would drop: that is an error, never a smaller pipe
    if by_depth.is_nan() || by_length.is_nan() {
        return Err(Off::Diameter);
    }
    let d = by_depth.max(by_length);
    if !d.is_finite() || d > d_hi + 1e-9 {
        return Err(Off::Diameter);
    }
    Ok(Downpipe { diameter: d.max(r.min_downpipe), by_depth, by_length })
}

/// Manning-Strickler at two thirds full: L/min for an inner diameter (mm), a slope (fraction) and a roughness `n`.
pub fn manning_flow(inside: f64, slope: f64, n: f64) -> f64 {
    if slope.is_nan() || slope <= 0.0 || inside.is_nan() || inside <= 0.0 || n.is_nan() || n <= 0.0 {
        return 0.0;
    }
    let d = inside / 1000.0;
    let theta = 2.0 * (1.0 - 2.0 * (2.0 / 3.0f64)).acos();
    let area = d * d / 8.0 * (theta - theta.sin());
    let perimeter = d * theta / 2.0;
    60000.0 / n * area * (area / perimeter).powf(2.0 / 3.0) * slope.sqrt()
}

/// The inner diameter (mm) at which a horizontal drain carries `q` L/min at two thirds full: the flow grows as the diameter to the 8/3.
pub fn horizontal_diameter(q: f64, slope: f64, n: f64) -> f64 {
    let one = manning_flow(100.0, slope, n);
    if one <= 0.0 { f64::NAN } else { 100.0 * (q / one).powf(3.0 / 8.0) }
}

/// `Q = I * A / 60`, L/min, for `i` mm/h over `a` m².
pub fn design_flow(i: f64, a: f64) -> f64 {
    if i > 0.0 && a > 0.0 { i * a / 60.0 } else { 0.0 }
}

#[cfg(test)]
mod tests;
