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

/// PVC soldável: (DN = outside diameter, wall) in mm, NBR 5648.
pub const PVC: &[(f64, f64)] =
    &[(20.0, 1.5), (25.0, 1.7), (32.0, 2.1), (40.0, 2.4), (50.0, 3.0), (60.0, 3.3), (75.0, 4.2), (85.0, 4.7), (110.0, 6.1)];

/// Equivalent lengths (m) of rigid PVC fittings by DN: elbow 90°, elbow 45°, tee straight through, tee side outlet, gate valve,
/// globe valve, check valve (light).
const LE: &[(f64, [f64; 7])] = &[
    (20.0, [1.1, 0.4, 0.7, 2.3, 0.1, 11.1, 2.5]),
    (25.0, [1.2, 0.5, 0.8, 2.4, 0.2, 11.4, 2.7]),
    (32.0, [1.5, 0.7, 0.9, 3.1, 0.3, 15.0, 3.8]),
    (40.0, [2.0, 1.0, 1.5, 4.6, 0.4, 22.0, 4.9]),
    (50.0, [3.2, 1.3, 2.2, 7.3, 0.7, 35.8, 6.8]),
    (60.0, [3.4, 1.5, 2.3, 7.6, 0.8, 37.9, 7.1]),
    (75.0, [3.7, 1.7, 2.4, 7.8, 0.9, 38.0, 8.2]),
    (85.0, [3.9, 1.8, 2.5, 8.0, 0.9, 40.0, 9.3]),
    (110.0, [4.3, 1.9, 2.6, 8.3, 1.0, 42.3, 10.4]),
];

/// The fittings `water_size` counts, in the order of `LE`'s columns.
pub const FITTINGS: &[&str] = &["elbow90", "elbow45", "tee_through", "tee_side", "gate", "globe", "check"];

/// The fastest water may go, m/s.
pub const MAX_SPEED: f64 = 3.0;
/// The least dynamic pressure at a point of use, kPa.
pub const MIN_USE_KPA: f64 = 10.0;
/// The roughness of PVC, mm.
pub const EPS_PVC: f64 = 0.0015;
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

/// The design flow (L/s) of fixtures given as (kind, count).
pub fn design_flow(method: Method, fixtures: &[(&str, f64)]) -> f64 {
    let (mut q, mut w) = (0.0, 0.0);
    for (kind, n) in fixtures {
        if let Some((fq, fw)) = fixture(kind) {
            q += fq * n;
            w += fw * n;
        }
    }
    match method {
        Method::Sum => q,
        Method::Weights => (0.3 * w.sqrt()).min(q),
    }
}

/// The inner diameter (mm) of a PVC soldável DN, or `None` for a DN the standard does not list.
pub fn inside(dn: f64) -> Option<f64> {
    PVC.iter().find(|p| p.0 == dn).map(|p| p.0 - 2.0 * p.1)
}

/// A fitting's equivalent length (m) in a DN, or `None` for a DN or a fitting the table does not have.
pub fn equivalent_length(dn: f64, fitting: &str) -> Option<f64> {
    let col = FITTINGS.iter().position(|f| *f == fitting)?;
    LE.iter().find(|r| r.0 == dn).map(|r| r.1[col])
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

/// The smallest PVC soldável DN whose speed at `q` L/s is within `MAX_SPEED`, or `None` when the table has none that carries it.
pub fn smallest_dn(q: f64) -> Option<f64> {
    PVC.iter().map(|p| p.0).find(|dn| inside(*dn).is_some_and(|d| head_loss(q, d, EPS_PVC).0 <= MAX_SPEED + 1e-9))
}

#[cfg(test)]
mod tests;
