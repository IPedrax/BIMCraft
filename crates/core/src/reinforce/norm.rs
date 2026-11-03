//! NBR 6118:2014's usual limits on the bars `reinforce` makes, as notes: the check asks, it never refuses, and it designs nothing.
//! The numbers are the ones course material and handbooks print, typed here and not read from the standard's own text (which is
//! paid for): check one against your copy before relying on it. Section numbers are the 2014 edition's, at the level of the item.
//! A limit that needs what a bar list doesn't hold (shear, the load, the aggregate) is not here.

use std::f64::consts::PI;

use super::{Bar, Cover, Frame, along};
use crate::model::TOL;
use crate::view3d::P3;

/// What the check is told besides the bars: the environmental aggressiveness class (1 to 4: I to IV) and the concrete's `fck` in MPa.
pub struct Norm {
    pub class: Option<u8>,
    pub fck: Option<f64>,
}

const ROMAN: [&str; 4] = ["I", "II", "III", "IV"];
/// The most notes one host gets.
const MAX_NOTES: usize = 24;
/// Slack for comparing a figure with a limit: the limits are exact, so a bar of 4 mm is under 5.
const EPS: f64 = 1e-6;

fn area(d: f64) -> f64 {
    PI * d * d / 4.0
}

fn on<'a>(bars: &'a [Bar], layers: &'a [&str]) -> impl Iterator<Item = &'a Bar> {
    bars.iter().filter(move |b| layers.contains(&b.layer))
}

fn steel(bars: &[Bar], layers: &[&str]) -> f64 {
    on(bars, layers).map(|b| area(b.diameter)).sum()
}

fn thickest(bars: &[Bar], layers: &[&str]) -> f64 {
    on(bars, layers).map(|b| b.diameter).fold(0.0, f64::max)
}

/// The widest gap between neighbours among `xs`, the ones within `TOL` of each other being one.
fn widest(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < TOL);
    xs.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max)
}

impl Norm {
    /// Table 7.2's nominal cover, mm, for the class and the kind of member, if a class was told.
    fn cover(&self, kind: &str, cv: &Cover, notes: &mut Vec<String>) {
        let Some(class) = self.class else { return };
        let row = match kind {
            "beam" | "column" => [25.0, 30.0, 40.0, 50.0],
            "floor" => [20.0, 25.0, 35.0, 45.0],
            // In contact with the ground.
            _ => [30.0, 30.0, 40.0, 50.0],
        };
        let least = row[usize::from(class - 1)];
        for (face, c) in [("top", cv.top), ("bottom", cv.bottom), ("sides", cv.sides)] {
            if c < least - EPS {
                notes.push(format!(
                    "NBR 6118 7.4.7.2: the {kind}'s {face} cover of {c} mm is under the {least} mm that Table 7.2 gives for class {}",
                    ROMAN[usize::from(class - 1)]
                ));
            }
        }
    }

    /// A beam `w` wide and `h` deep, its bars `bars` in the frame `f`.
    pub fn beam(&self, (w, h): (f64, f64), cv: &Cover, f: &Frame, bars: &[Bar]) -> Vec<String> {
        let mut notes = Vec::new();
        self.cover("beam", cv, &mut notes);
        let ac = w * h;
        let (all, tension) = (steel(bars, &["bottom", "bent", "top", "skin"]), steel(bars, &["bottom", "bent"]));
        if all > 0.04 * ac {
            notes.push(format!("NBR 6118 17.3.5.2: the longitudinal bars are {:.1} % of the section, over the 4 % most", 100.0 * all / ac));
        }
        match self.fck {
            Some(c) if c > 30.0 && tension > 0.0 => {
                notes.push(format!("NBR 6118 17.3.5.2: the minimum steel for C{c} isn't coded here (Table 17.3): only up to C30 is"));
            }
            _ if tension > 0.0 && tension < 0.0015 * ac => notes.push(format!(
                "NBR 6118 17.3.5.2: the bottom bars are {:.3} % of the section, under the 0.15 % Table 17.3 gives up to C30",
                100.0 * tension / ac
            )),
            _ => {}
        }
        let dst = thickest(bars, &["stirrup"]);
        let dl = bars.iter().find(|b| b.layer == "bottom").map_or(0.0, |b| b.diameter);
        let d = h - cv.bottom - dst - dl / 2.0;
        if h > 600.0 {
            let per_face = steel(bars, &["skin"]) / 2.0;
            if per_face < 0.001 * ac - EPS {
                notes.push(format!(
                    "NBR 6118 17.3.5.2.3: a beam over 600 mm deep wants skin bars of 0.10 % of the section on each face, and these have {:.3} %",
                    100.0 * per_face / ac
                ));
            } else {
                let gap = widest(on(bars, &["skin"]).filter_map(|b| b.path.first().map(|p| along(f, f.c, *p))).collect());
                if gap > (d / 3.0).min(200.0) + EPS {
                    notes.push(format!(
                        "NBR 6118 17.3.5.2.3: skin bars {gap:.0} mm apart, over the d/3 or 200 mm limit ({:.0} mm)",
                        (d / 3.0).min(200.0)
                    ));
                }
            }
        }
        if dst > 0.0 {
            if dst < 5.0 - EPS {
                notes.push(format!("NBR 6118 17.4.1.1: stirrups of {dst} mm are under the 5 mm least"));
            }
            if dst > w / 10.0 + EPS {
                notes.push(format!("NBR 6118 17.4.1.1: stirrups of {dst} mm are over a tenth of the beam's width ({:.1} mm)", w / 10.0));
            }
            let gap = widest(on(bars, &["stirrup"]).filter_map(|b| b.path.first().map(|p| along(f, f.a, *p))).collect());
            let limit = (0.6 * d).min(300.0);
            if gap > limit + EPS {
                notes.push(format!(
                    "NBR 6118 17.4.1.1: stirrups {gap:.0} mm apart, over min(0.6 d, 300) = {limit:.0} mm (0.3 d and 200 mm where shear is high)"
                ));
            }
        }
        notes.truncate(MAX_NOTES);
        notes
    }

    /// A column `w` by `d` (`w` across when `round`), its bars `bars` in the frame `f`.
    pub fn column(&self, (w, d): (f64, f64), round: bool, cv: &Cover, f: &Frame, bars: &[Bar]) -> Vec<String> {
        let mut notes = Vec::new();
        self.cover("column", cv, &mut notes);
        let least = if round { w } else { w.min(d) };
        let (dl, n) = (thickest(bars, &["bars"]), on(bars, &["bars"]).count());
        if n > 0 {
            if dl < 10.0 - EPS {
                notes.push(format!("NBR 6118 18.4.2.1: bars of {dl} mm are under the 10 mm least for a column"));
            }
            if dl > least / 8.0 + EPS {
                notes.push(format!(
                    "NBR 6118 18.4.2.1: bars of {dl} mm are over an eighth of the column's smaller side ({:.1} mm)",
                    least / 8.0
                ));
            }
            let fewest = if round { 6 } else { 4 };
            if n < fewest {
                notes.push(format!(
                    "NBR 6118 18.4.2.1: a {} column has at least {fewest} bars, and this has {n}",
                    if round { "round" } else { "rectangular" }
                ));
            }
            let ac = if round { PI * w * w / 4.0 } else { w * d };
            let as_ = steel(bars, &["bars"]);
            if as_ < 0.004 * ac - EPS {
                notes.push(format!("NBR 6118 17.3.5.3: the bars are {:.2} % of the section, under the 0.4 % least", 100.0 * as_ / ac));
            } else if as_ > 0.08 * ac {
                notes.push(format!("NBR 6118 17.3.5.3: the bars are {:.2} % of the section, over the 8 % most", 100.0 * as_ / ac));
            }
            if !round {
                // Where a bar stands in the section: its outermost point, since a leg and a jog both go inward.
                let spots: Vec<(f64, f64)> = on(bars, &["bars"])
                    .filter_map(|b| {
                        let at = |p: &P3| (along(f, f.b, *p), along(f, f.c, *p));
                        b.path.iter().map(at).max_by(|p, q| (p.0 * p.0 + p.1 * p.1).total_cmp(&(q.0 * q.0 + q.1 * q.1)))
                    })
                    .collect();
                let (bmax, cmax) =
                    (spots.iter().map(|s| s.0.abs()).fold(0.0, f64::max), spots.iter().map(|s| s.1.abs()).fold(0.0, f64::max));
                let on_c = spots.iter().filter(|s| (s.1.abs() - cmax).abs() < TOL).map(|s| s.0).collect();
                let on_b = spots.iter().filter(|s| (s.0.abs() - bmax).abs() < TOL).map(|s| s.1).collect();
                let (gap, limit) = (widest(on_c).max(widest(on_b)), (2.0 * least).min(400.0));
                if gap > limit + EPS {
                    notes.push(format!("NBR 6118 18.4.2.2: bars {gap:.0} mm apart along a face, over the {limit:.0} mm most"));
                }
            }
        }
        let dst = thickest(bars, &["stirrup"]);
        if dst > 0.0 {
            let need = 5.0_f64.max(dl / 4.0);
            if dst < need - EPS {
                notes.push(format!(
                    "NBR 6118 18.4.3: stirrups of {dst} mm are under the {need} mm least (5 mm, or a quarter of the bars' diameter)"
                ));
            }
            let gap = widest(on(bars, &["stirrup"]).filter_map(|b| b.path.first().map(|p| along(f, f.a, *p))).collect());
            let limit = 200.0_f64.min(least).min(12.0 * dl);
            if dl > 0.0 && gap > limit + EPS {
                notes.push(format!(
                    "NBR 6118 18.4.3: stirrups {gap:.0} mm apart, over min(200, the smaller side, 12 bar diameters) = {limit:.0} mm"
                ));
            }
        }
        notes.truncate(MAX_NOTES);
        notes
    }

    /// A floor `thick` mm thick.
    pub fn floor(&self, thick: f64, cv: &Cover, bars: &[Bar]) -> Vec<String> {
        let mut notes = Vec::new();
        self.cover("floor", cv, &mut notes);
        let dl = bars.iter().map(|b| b.diameter).fold(0.0, f64::max);
        if dl > thick / 8.0 + EPS {
            notes.push(format!("NBR 6118 20.1: bars of {dl} mm are over an eighth of the slab's thickness ({:.1} mm)", thick / 8.0));
        }
        let limit = (2.0 * thick).min(200.0);
        for name in ["bottom x", "bottom y", "top x", "top y"] {
            let across = |b: &Bar| b.path.get(b.path.len() / 2).map(|p| if name.ends_with('x') { p[1] } else { p[0] });
            let gap = widest(bars.iter().filter(|b| b.layer == name).filter_map(across).collect());
            if gap > limit + EPS {
                notes.push(format!("NBR 6118 20.1: the {name} bars are {gap:.0} mm apart, over the {limit:.0} mm most (2 h, and 200 mm)"));
            }
        }
        notes.truncate(MAX_NOTES);
        notes
    }

    /// A footing: its cover only.
    pub fn footing(&self, cv: &Cover) -> Vec<String> {
        let mut notes = Vec::new();
        self.cover("footing", cv, &mut notes);
        notes
    }
}
