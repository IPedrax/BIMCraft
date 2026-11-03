//! The demand of an installation, and the category of service it asks of the utility, as the utility's own standard works them out.
//! A pure function of its input, with no model behind it: the `demand` command is its only caller. The numbers are those of the
//! utility's document, which is not an ABNT standard (NBR 5410 gives no demand factors), so each is named by its utility and its
//! edition. Only one is here:
//!
//! - **CPFL** (CPFL Paulista, Piratininga, Santa Cruz and RGE): Norma Técnica GED 13, "Fornecimento em Tensão Secundária de
//!   Distribuição", version 2.24, 18/09/2020. `D = a + b + c + d + e + f + g + h + i` (item 6.25.1): a lighting and outlets
//!   (Tabelas 2 and 3, or 18), b showers, taps, instant heaters and irons (Tabela 4), c boilers (Tabela 5), d dryers, ovens,
//!   dishwashers and microwaves (Tabela 6), e electric stoves (Tabela 7), f window air conditioners (Tabelas 8 and 9), g motors
//!   (Tabelas 14, 15 and 10), h special equipment (Tabela 11, power factor 0.75), i hydromassage (Tabela 10). The category comes
//!   of the installed load up to 25 kW and of the demand above it (Tabelas 1A and 1B).
//!
//! What is not applied: the cables, conduits and breakers of Tabelas 1D and 1E (EPR and aluminium), the poles and the other
//! columns of Tabela 1, the motor starting rules of Tabela 13, and the power factors of discharge lighting in Tabela 18's buildings
//! beyond the one given.

/// The utilities there are tables for.
pub const UTILITIES: &[&str] = &["CPFL"];

/// Where the numbers come from, for each utility.
pub fn source(utility: &str) -> Option<&'static str> {
    (utility == "CPFL")
        .then_some("CPFL Energia, Norma Técnica GED 13 'Fornecimento em Tensão Secundária de Distribuição', version 2.24, 18/09/2020")
}

/// The supply voltages of Tabelas 1A and 1B.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum System {
    S127_220,
    S220_380,
}

impl System {
    pub fn parse(s: &str) -> Option<System> {
        match s {
            "127/220" => Some(System::S127_220),
            "220/380" => Some(System::S220_380),
            _ => None,
        }
    }
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialKind {
    ArcWeld,
    ResistanceWeld,
    Xray,
}

pub const SPECIAL_KINDS: &[&str] = &["arc_weld", "resistance_weld", "xray"];

impl SpecialKind {
    pub fn parse(s: &str) -> Option<SpecialKind> {
        Some(match s {
            "arc_weld" => SpecialKind::ArcWeld,
            "resistance_weld" => SpecialKind::ResistanceWeld,
            "xray" => SpecialKind::Xray,
            _ => return None,
        })
    }
}

/// Special equipment: the kW of one, and how many.
#[derive(Clone, Copy, Debug)]
pub struct Special {
    pub kind: SpecialKind,
    pub kw: f64,
    pub count: u32,
}

/// What a building that is not a dwelling declares for its lighting and outlets: its use (Tabela 18), the kW and the power factor.
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

/// Tabela 2: the outlets of 100 W and the outlets of 600 W of the kitchen a dwelling has at least, by built area (m²); `None`
/// above 250 m², where they are declared.
pub fn least_outlets(area: f64) -> Option<(u32, u32)> {
    const ROWS: &[(f64, u32, u32)] = &[
        (8.0, 1, 1),
        (15.0, 3, 1),
        (20.0, 4, 2),
        (30.0, 5, 2),
        (50.0, 6, 3),
        (70.0, 7, 3),
        (90.0, 8, 3),
        (110.0, 9, 3),
        (140.0, 10, 3),
        (170.0, 11, 3),
        (200.0, 12, 3),
        (220.0, 13, 3),
        (250.0, 14, 3),
    ];
    ROWS.iter().find(|r| area <= r.0).map(|r| (r.1, r.2))
}

/// Tabela 3: the demand factor of a residence's lighting and outlets by its installed load (kW).
pub fn lighting_factor(kw: f64) -> f64 {
    const ROWS: &[(f64, f64)] = &[
        (1.0, 0.86),
        (2.0, 0.75),
        (3.0, 0.66),
        (4.0, 0.59),
        (5.0, 0.52),
        (6.0, 0.45),
        (7.0, 0.40),
        (8.0, 0.35),
        (9.0, 0.31),
        (10.0, 0.27),
    ];
    ROWS.iter().find(|r| kw <= r.0).map_or(0.24, |r| r.1)
}

/// Tabela 4: showers, taps, instant heaters and irons, by how many there are in all.
pub fn heater_factor(n: u32) -> f64 {
    const F: [f64; 25] = [
        1.00, 1.00, 0.84, 0.76, 0.70, 0.65, 0.60, 0.57, 0.54, 0.52, 0.49, 0.48, 0.46, 0.45, 0.44, 0.43, 0.42, 0.41, 0.40, 0.40, 0.39, 0.39,
        0.39, 0.38, 0.38,
    ];
    F.get((n.max(1) - 1) as usize).copied().unwrap_or(0.38)
}

/// Tabela 5: central or storage heaters (boilers).
pub fn boiler_factor(n: u32) -> f64 {
    match n {
        0 | 1 => 1.0,
        2 => 0.72,
        _ => 0.62,
    }
}

/// Tabela 6: dryers, ovens, dishwashers and microwave ovens.
pub fn appliance_factor(n: u32) -> f64 {
    match n {
        0 | 1 => 1.0,
        2..=4 => 0.70,
        5..=6 => 0.60,
        _ => 0.50,
    }
}

/// Tabela 7: electric stoves.
pub fn stove_factor(n: u32) -> f64 {
    match n {
        0 | 1 => 1.00,
        2 => 0.60,
        3 => 0.48,
        4 => 0.40,
        5 => 0.37,
        6 => 0.35,
        7 => 0.33,
        8 => 0.32,
        9 => 0.31,
        10..=11 => 0.30,
        12..=15 => 0.28,
        _ => 0.26,
    }
}

/// Tabela 9: commercial window air conditioners, by how many.
pub fn air_factor(n: u32) -> f64 {
    match n {
        0..=10 => 1.00,
        11..=20 => 0.90,
        21..=30 => 0.82,
        31..=40 => 0.80,
        41..=50 => 0.77,
        _ => 0.75,
    }
}

/// Tabela 10: motors (and hydromassage), by the rank of the motor from the biggest: the 1st at 1, the 2nd at 0.9, the 3rd to 5th
/// at 0.8, the others at 0.7.
pub fn motor_factor(rank: u32) -> f64 {
    match rank {
        0 | 1 => 1.0,
        2 => 0.9,
        3..=5 => 0.8,
        _ => 0.7,
    }
}

/// Tabela 8: a window air conditioner by its BTU/h, the VA and the W of one (the figures are the same at 110 and 220 V).
pub fn window_air(btu: f64) -> Option<(f64, f64)> {
    const ROWS: &[(f64, f64, f64)] = &[
        (7100.0, 1100.0, 900.0),
        (8500.0, 1550.0, 1300.0),
        (10000.0, 1650.0, 1400.0),
        (12000.0, 1900.0, 1600.0),
        (14000.0, 2100.0, 1900.0),
        (18000.0, 2860.0, 2600.0),
        (21000.0, 3080.0, 2800.0),
        (30000.0, 4000.0, 3600.0),
    ];
    ROWS.iter().find(|r| r.0 == btu).map(|r| (r.1, r.2))
}

/// Tabelas 14 (single-phase) and 15 (three-phase): a motor by its cv, its kVA and its kW. Single-phase stops at 15 cv.
pub fn motor(cv: f64, phases: u32) -> Option<(f64, f64)> {
    // (cv, kW, kVA)
    const ONE: &[(f64, f64, f64)] = &[
        (0.25, 0.42, 0.66),
        (1.0 / 3.0, 0.51, 0.77),
        (0.5, 0.79, 1.18),
        (0.75, 0.90, 1.34),
        (1.0, 1.14, 1.56),
        (1.5, 1.67, 2.35),
        (2.0, 2.17, 2.97),
        (3.0, 3.22, 4.07),
        (5.0, 5.11, 6.16),
        (7.5, 7.07, 8.84),
        (10.0, 9.31, 11.64),
        (12.5, 11.58, 14.94),
        (15.0, 13.72, 16.94),
    ];
    const THREE: &[(f64, f64, f64)] = &[
        (0.25, 0.35, 0.58),
        (1.0 / 3.0, 0.39, 0.65),
        (0.5, 0.58, 0.87),
        (0.75, 0.83, 1.26),
        (1.0, 1.05, 1.52),
        (1.5, 1.54, 2.17),
        (2.0, 1.95, 2.70),
        (3.0, 2.95, 4.04),
        (4.0, 3.72, 5.03),
        (5.0, 4.51, 6.02),
        (6.0, 5.33, 7.00),
        (7.5, 6.57, 8.65),
        (10.0, 8.89, 11.54),
        (12.5, 10.85, 14.09),
        (15.0, 12.82, 16.65),
        (20.0, 17.01, 22.10),
        (25.0, 20.92, 25.83),
        (30.0, 25.03, 30.52),
        (40.0, 33.38, 39.74),
        (50.0, 40.93, 48.73),
        (60.0, 49.42, 58.15),
        (75.0, 61.44, 72.28),
    ];
    let rows = if phases == 1 { ONE } else { THREE };
    rows.iter().find(|r| (r.0 - cv).abs() < 1e-6).map(|r| (r.2, r.1))
}

/// Tabela 18: a building's lighting and outlets, the least W/m² and the demand (kW) of a declared load of `kw`; `None` for a use
/// the table does not have.
pub fn commercial_demand(kind: &str, kw: f64) -> Option<f64> {
    // Tiers: (up to kW, factor), the last factor for what is over.
    let tiers: &[(f64, f64)] = match kind {
        "school" => &[(12.0, 1.0), (f64::INFINITY, 0.5)],
        "office" => &[(20.0, 1.0), (f64::INFINITY, 0.7)],
        "hospital" => &[(50.0, 0.4), (f64::INFINITY, 0.2)],
        "hotel" => &[(20.0, 0.5), (f64::INFINITY, 0.4)],
        k if USES.contains(&k) => &[(f64::INFINITY, 1.0)],
        _ => return None,
    };
    let (mut left, mut from, mut total) = (kw.max(0.0), 0.0, 0.0);
    for (to, f) in tiers {
        let part = left.min(to - from);
        total += part * f;
        left -= part;
        from = *to;
        if left <= 0.0 {
            break;
        }
    }
    Some(total)
}

/// The uses of Tabela 18, as a command gives them.
pub const USES: &[&str] =
    &["auditorium", "shop", "barbershop", "club", "school", "office", "garage", "hospital", "hotel", "church", "industry", "restaurant"];

/// Whether Tabela 4 (not a factor of 1) applies to the showers and taps of a building of this use: hotels, motels, hospitals,
/// commercial houses and churches, as item 6.25.1 b) names.
fn heaters_by_table(kind: &str) -> bool {
    matches!(kind, "hotel" | "hospital" | "shop" | "church")
}

/// One category of Tabela 1A or 1B.
#[derive(Clone, Copy, Debug)]
pub struct Category {
    pub name: &'static str,
    /// The breaker (A), the copper PVC cable (mm²), the conduit (mm) and the earthing conductor (mm²).
    pub breaker: f64,
    pub cable: f64,
    pub conduit: f64,
    pub earth: f64,
    /// The phases: 1 (one phase and the neutral), 2 or 3.
    pub phases: u8,
}

/// The category of service for an installed load `c` (kW) and a demand `d` (kVA), `None` above the table.
pub fn category(system: System, c: f64, d: f64) -> Option<Category> {
    let k = |name, breaker, cable, conduit, earth, phases| Some(Category { name, breaker, cable, conduit, earth, phases });
    match system {
        System::S127_220 => match c {
            c if c <= 6.0 => k("A1", 32.0, 6.0, 32.0, 6.0, 1),
            c if c <= 12.0 => k("A2", 63.0, 16.0, 32.0, 10.0, 1),
            c if c <= 18.0 => k("B1", 63.0, 16.0, 40.0, 10.0, 2),
            c if c <= 25.0 => k("B2", 80.0, 25.0, 40.0, 10.0, 2),
            c if c <= 75.0 => match d {
                d if d <= 23.0 => k("C1", 63.0, 16.0, 40.0, 10.0, 3),
                d if d <= 30.0 => k("C2", 80.0, 25.0, 40.0, 10.0, 3),
                d if d <= 38.0 => k("C3", 100.0, 35.0, 40.0, 10.0, 3),
                d if d <= 47.0 => k("C4", 125.0, 50.0, 50.0, 16.0, 3),
                d if d <= 57.0 => k("C5", 150.0, 70.0, 60.0, 25.0, 3),
                d if d <= 76.0 => k("C6", 200.0, 95.0, 60.0, 35.0, 3),
                _ => None,
            },
            _ => None,
        },
        System::S220_380 => match c {
            c if c <= 10.0 => k("A3", 32.0, 6.0, 32.0, 6.0, 1),
            c if c <= 15.0 => k("A4", 63.0, 16.0, 32.0, 10.0, 1),
            c if c <= 25.0 => k("B3", 63.0, 16.0, 40.0, 10.0, 2),
            c if c <= 75.0 => match d {
                d if d <= 26.0 => k("C7", 40.0, 10.0, 40.0, 10.0, 3),
                d if d <= 40.0 => k("C8", 63.0, 16.0, 40.0, 10.0, 3),
                d if d <= 46.0 => k("C9", 80.0, 25.0, 40.0, 10.0, 3),
                d if d <= 66.0 => k("C10", 100.0, 35.0, 40.0, 10.0, 3),
                d if d <= 82.0 => k("C11", 125.0, 50.0, 50.0, 16.0, 3),
                _ => None,
            },
            _ => None,
        },
    }
}

fn total(loads: &[Load]) -> (u32, f64) {
    (loads.iter().map(|l| l.count).sum(), loads.iter().map(|l| l.w * f64::from(l.count)).sum::<f64>() / 1000.0)
}

/// The demand of an installation (item 6.25.1 of the GED) and the load it installs.
pub fn compute(i: &Input) -> Result<Output, String> {
    // a) lighting and outlets.
    let (lighting_outlets_w, a, kind) = if i.residential {
        let kitchen = i.area.and_then(least_outlets).map_or(3, |x| x.1);
        let outlets_w = match (i.outlets, i.area) {
            (Some(n), _) => f64::from(n) * 100.0 + f64::from(kitchen) * 600.0,
            (None, Some(area)) => match least_outlets(area) {
                Some((n, k)) => f64::from(n) * 100.0 + f64::from(k) * 600.0,
                None => return Err("above 250 m² the outlets are declared: give `outlets` (100 W each; the kitchen's are added)".into()),
            },
            (None, None) => return Err("give the built `area` (m²) or the number of `outlets`".into()),
        };
        let w = outlets_w + f64::from(i.lighting_points) * 100.0;
        (w, w / 1000.0 * lighting_factor(w / 1000.0), "")
    } else {
        let o = i.other.as_ref().ok_or("a building that is not a dwelling declares its lighting and outlets: give `lighting_outlets`")?;
        let d = commercial_demand(&o.kind, o.kw).ok_or_else(|| format!("no use `{}` in Tabela 18", o.kind))?;
        (o.kw * 1000.0, d / o.pf, o.kind.as_str())
    };
    let mut t = Terms { a, ..Terms::default() };
    let mut installed = lighting_outlets_w / 1000.0;
    // b) showers, taps, instant heaters, irons.
    let (n, kw) = total(&i.heaters);
    installed += kw;
    t.b = kw * if i.residential || heaters_by_table(kind) { heater_factor(n) } else { 1.0 };
    // c) boilers.
    let (n, kw) = total(&i.boilers);
    installed += kw;
    t.c = kw * boiler_factor(n);
    // d) dryers, ovens, dishwashers, microwaves.
    let (n, kw) = total(&i.appliances);
    installed += kw;
    t.d = kw * appliance_factor(n);
    // e) stoves.
    let (n, kw) = total(&i.stoves);
    installed += kw;
    t.e = kw * stove_factor(n);
    // f) window air conditioners: the installed load in W, the demand in VA.
    let n: u32 = i.air.iter().map(|a| a.count).sum();
    installed += i.air.iter().map(|a| a.w * f64::from(a.count)).sum::<f64>() / 1000.0;
    let va: f64 = i.air.iter().map(|a| a.va * f64::from(a.count)).sum::<f64>() / 1000.0;
    t.f = va * if i.commercial_air { air_factor(n) } else { 1.0 };
    // g) motors, the biggest first.
    let mut motors: Vec<f64> = i.motors.iter().flat_map(|m| std::iter::repeat_n(m.kva, m.count as usize)).collect();
    motors.sort_by(|a, b| b.total_cmp(a));
    installed += i.motors.iter().map(|m| m.kw * f64::from(m.count)).sum::<f64>();
    t.g = motors.iter().enumerate().map(|(r, kva)| kva * motor_factor(r as u32 + 1)).sum();
    // h) special equipment at a power factor of 0.75.
    for kind in [SpecialKind::ArcWeld, SpecialKind::ResistanceWeld, SpecialKind::Xray] {
        let mut v: Vec<f64> =
            i.special.iter().filter(|s| s.kind == kind).flat_map(|s| std::iter::repeat_n(s.kw, s.count as usize)).collect();
        v.sort_by(|a, b| b.total_cmp(a));
        installed += v.iter().sum::<f64>();
        t.h += v
            .iter()
            .enumerate()
            .map(|(r, kw)| {
                let f = match (kind, r) {
                    (_, 0) => 1.0,
                    (SpecialKind::ArcWeld, 1) => 0.7,
                    (SpecialKind::ArcWeld, 2) => 0.4,
                    (SpecialKind::ArcWeld, _) => 0.3,
                    (SpecialKind::ResistanceWeld, _) => 0.6,
                    (SpecialKind::Xray, _) => 0.7,
                };
                kw / 0.75 * f
            })
            .sum::<f64>();
    }
    // i) hydromassage, by Tabela 10, at a power factor of 1.
    let mut v: Vec<f64> = i.hydro.iter().flat_map(|l| std::iter::repeat_n(l.w / 1000.0, l.count as usize)).collect();
    v.sort_by(|a, b| b.total_cmp(a));
    installed += v.iter().sum::<f64>();
    t.i = v.iter().enumerate().map(|(r, kw)| kw * motor_factor(r as u32 + 1)).sum();
    let demand_kva = t.a + t.b + t.c + t.d + t.e + t.f + t.g + t.h + t.i;
    Ok(Output { installed_kw: installed, lighting_outlets_w, terms: t, demand_kva })
}

#[cfg(test)]
mod tests;
