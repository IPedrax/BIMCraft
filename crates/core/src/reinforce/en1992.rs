//! EN 1992-1-1:2004's usual limits on the bars `reinforce` makes, as notes, the way `norm` does for NBR 6118: the check asks, it
//! never refuses, and it designs nothing. The numbers are the ones public guides and handbooks print (the Concrete Centre's
//! detailing guide, the designers' guides, an open-source implementation of Table 4.4N), typed here and not read from the
//! standard's own text (which is paid for): check one against your copy and your National Annex before relying on it, since the
//! values here are the recommended ones, which a country may change. Clause numbers are the clause's, not the paragraph's.
//!
//! What a bar list doesn't hold is not here: a column's least steel of 0.10 NEd / fyd needs the load, and the cover for bond (a
//! bar's own diameter) is left out. Structural class S4 (a life of 50 years, Table 4.3N) and 10 mm for deviation (4.4.1.3) are taken
//! for a beam, a column and a footing, and S3 for a floor (4.4.1.2: slab geometry takes a class off). The last column of Table
//! 4.4N (XD3 and XS3) takes the value of the one before it (XD2 and XS2) until a public text confirms the step up, so a note
//! about it errs low.

use std::f64::consts::PI;

use super::norm::{EPS, MAX_NOTES, area, on, steel, thickest, widest};
use super::{Bar, Cover, Frame, along};

/// The deviation added to the least cover, mm (4.4.1.3).
const DEVIATION: f64 = 10.0;
/// The least nominal cover of concrete cast against blinding, mm (4.4.1.3: 75 against the soil).
const GROUND: f64 = 40.0;
/// Table 4.4N's c_min,dur, mm, for the structural classes S1 to S6 and the columns X0, XC1, XC2 and XC3, XC4, XD1 and XS1, XD2 and
/// XS2 (and XD3 and XS3, as the module says).
const CMIN_DUR: [[f64; 6]; 6] = [
    [10.0, 10.0, 10.0, 15.0, 20.0, 25.0],
    [10.0, 10.0, 15.0, 20.0, 25.0, 30.0],
    [10.0, 10.0, 20.0, 25.0, 30.0, 35.0],
    [10.0, 15.0, 25.0, 30.0, 35.0, 40.0],
    [15.0, 20.0, 30.0, 35.0, 40.0, 45.0],
    [20.0, 25.0, 35.0, 40.0, 45.0, 50.0],
];
/// The exposure classes of Table 4.1 that cover depends on, and the column of `CMIN_DUR` each reads.
const EXPOSURES: [(&str, usize); 11] =
    [("X0", 0), ("XC1", 1), ("XC2", 2), ("XC3", 2), ("XC4", 3), ("XD1", 4), ("XS1", 4), ("XD2", 5), ("XS2", 5), ("XD3", 5), ("XS3", 5)];

/// An exposure class, by its name and its column of Table 4.4N.
#[derive(Clone, Copy)]
pub struct Exposure {
    name: &'static str,
    column: usize,
}

impl Exposure {
    /// The names `read` knows, for a message.
    pub const NAMES: &'static str = "X0, XC1 to XC4, XD1 to XD3 or XS1 to XS3";

    /// The class `text` names, in any case.
    pub fn read(text: &str) -> Option<Exposure> {
        EXPOSURES.iter().find(|(n, _)| n.eq_ignore_ascii_case(text.trim())).map(|&(name, column)| Exposure { name, column })
    }
}

/// What the check is told besides the bars: the exposure class, the concrete's `fck` in MPa and the steel's `fyk` in MPa.
pub struct En1992 {
    pub exposure: Option<Exposure>,
    pub fck: Option<f64>,
    pub fyk: Option<f64>,
}

/// The concrete's mean tensile strength, MPa (Table 3.1).
fn fctm(fck: f64) -> f64 {
    if fck <= 50.0 { 0.30 * fck.powf(2.0 / 3.0) } else { 2.12 * (1.0 + (fck + 8.0) / 10.0).ln() }
}

impl En1992 {
    /// The nominal cover Table 4.4N asks for a member of structural class `class` (1 to 6), mm, and `ground` for a member cast on
    /// the ground: each face under it gets a note.
    fn cover(&self, kind: &str, class: usize, ground: bool, cv: &Cover, notes: &mut Vec<String>) {
        let table = self.exposure.map(|e| (CMIN_DUR[class - 1][e.column] + DEVIATION, e.name));
        let least = table.map_or(0.0, |t| t.0).max(if ground { GROUND } else { 0.0 });
        if least == 0.0 {
            return;
        }
        for (face, c) in [("top", cv.top), ("bottom", cv.bottom), ("sides", cv.sides)] {
            if c >= least - EPS {
                continue;
            }
            notes.push(match table {
                Some((t, name)) if t >= least => format!(
                    "EN 1992-1-1 4.4.1.2: the {kind}'s {face} cover of {c} mm is under the {least} mm that Table 4.4N gives for {name} (structural class S{class}, plus {DEVIATION} mm for deviation)"
                ),
                _ => format!(
                    "EN 1992-1-1 4.4.1.3: the {kind}'s {face} cover of {c} mm is under the {least} mm least for concrete cast on blinding (75 mm against the soil)"
                ),
            });
        }
    }

    /// A beam `w` wide and `h` deep, its bars `bars` in the frame `f`.
    pub fn beam(&self, (w, h): (f64, f64), cv: &Cover, f: &Frame, bars: &[Bar]) -> Vec<String> {
        let mut notes = Vec::new();
        self.cover("beam", 4, false, cv, &mut notes);
        let ac = w * h;
        let (tension, compression) = (steel(bars, &["bottom", "bent"]), steel(bars, &["top"]));
        for (name, a) in [("bottom", tension), ("top", compression)] {
            if a > 0.04 * ac {
                notes.push(format!("EN 1992-1-1 9.2.1.1: the {name} bars are {:.1} % of the section, over the 4 % most", 100.0 * a / ac));
            }
        }
        let dst = thickest(bars, &["stirrup"]);
        let dl = bars.iter().find(|b| b.layer == "bottom").map_or(0.0, |b| b.diameter);
        let d = h - cv.bottom - dst - dl / 2.0;
        let strengths = self.fck.zip(self.fyk);
        match strengths {
            Some((c, y)) if tension > 0.0 => {
                let least = (0.26 * fctm(c) / y).max(0.0013) * w * d;
                if tension < least - EPS {
                    notes.push(format!(
                        "EN 1992-1-1 9.2.1.1: the bottom bars are {tension:.0} mm2, under the {least:.0} mm2 least (0.26 fctm / fyk of b d, and 0.0013 b d at least) for C{c} and a d of {d:.0} mm"
                    ));
                }
            }
            None if tension > 0.0 || dst > 0.0 => {
                notes.push("EN 1992-1-1 9.2.1.1: the least steel, and the stirrups' least ratio (9.2.2), need `fck` and `fyk`, and weren't checked".into());
            }
            _ => {}
        }
        if dst > 0.0 {
            let gap = widest(on(bars, &["stirrup"]).filter_map(|b| b.path.first().map(|p| along(f, f.a, *p))).collect());
            let limit = 0.75 * d;
            if gap > limit + EPS {
                notes.push(format!("EN 1992-1-1 9.2.2: stirrups {gap:.0} mm apart, over 0.75 d = {limit:.0} mm"));
            }
            if let Some((c, y)) = strengths
                && gap > 0.0
            {
                // Two legs: each stirrup is a closed rectangle.
                let (have, least) = (2.0 * area(dst) / (gap * w), 0.08 * c.sqrt() / y);
                if have < least - EPS {
                    notes.push(format!(
                        "EN 1992-1-1 9.2.2: stirrups of {dst} mm {gap:.0} mm apart are {:.3} % of b s, under the {:.3} % least (0.08 sqrt(fck) / fyk), counting two legs",
                        100.0 * have,
                        100.0 * least
                    ));
                }
            }
        }
        notes.truncate(MAX_NOTES);
        notes
    }

    /// A column `w` by `d` (`w` across when `round`), its bars `bars` in the frame `f`.
    pub fn column(&self, (w, d): (f64, f64), round: bool, cv: &Cover, f: &Frame, bars: &[Bar]) -> Vec<String> {
        let mut notes = Vec::new();
        self.cover("column", 4, false, cv, &mut notes);
        let least = if round { w } else { w.min(d) };
        let (dmax, n) = (thickest(bars, &["bars"]), on(bars, &["bars"]).count());
        let dmin = on(bars, &["bars"]).map(|b| b.diameter).fold(f64::INFINITY, f64::min);
        if n > 0 {
            if dmin < 8.0 - EPS {
                notes.push(format!("EN 1992-1-1 9.5.2: bars of {dmin} mm are under the 8 mm least the standard recommends"));
            }
            if n < 4 {
                notes.push(format!("EN 1992-1-1 9.5.2: a column has at least 4 bars, and this has {n}"));
            }
            let ac = if round { PI * w * w / 4.0 } else { w * d };
            let as_ = steel(bars, &["bars"]);
            if as_ < 0.002 * ac - EPS {
                notes.push(format!(
                    "EN 1992-1-1 9.5.2: the bars are {:.2} % of the section, under the 0.2 % least (0.10 NEd / fyd needs the load, and isn't checked)",
                    100.0 * as_ / ac
                ));
            } else if as_ > 0.04 * ac {
                notes.push(format!(
                    "EN 1992-1-1 9.5.2: the bars are {:.2} % of the section, over the 4 % most (8 % at laps)",
                    100.0 * as_ / ac
                ));
            }
        }
        let dst = thickest(bars, &["stirrup"]);
        if dst > 0.0 && n > 0 {
            let need = 6.0_f64.max(dmax / 4.0);
            if dst < need - EPS {
                notes.push(format!(
                    "EN 1992-1-1 9.5.3: stirrups of {dst} mm are under the {need} mm least (6 mm, or a quarter of the thickest bar)"
                ));
            }
            let gap = widest(on(bars, &["stirrup"]).filter_map(|b| b.path.first().map(|p| along(f, f.a, *p))).collect());
            let limit = (20.0 * dmin).min(least).min(400.0);
            if gap > limit + EPS {
                notes.push(format!(
                    "EN 1992-1-1 9.5.3: stirrups {gap:.0} mm apart, over min(20 smallest bar diameters, the smaller side, 400 mm) = {limit:.0} mm (0.6 of it near a beam or slab, and at laps)"
                ));
            }
        }
        notes.truncate(MAX_NOTES);
        notes
    }

    /// A floor `thick` mm thick.
    pub fn floor(&self, thick: f64, cv: &Cover, bars: &[Bar]) -> Vec<String> {
        let mut notes = Vec::new();
        self.cover("floor", 3, false, cv, &mut notes);
        let limit = (3.0 * thick).min(400.0);
        for name in ["bottom x", "bottom y", "top x", "top y"] {
            let across = |b: &Bar| b.path.get(b.path.len() / 2).map(|p| if name.ends_with('x') { p[1] } else { p[0] });
            let gap = widest(bars.iter().filter(|b| b.layer == name).filter_map(across).collect());
            if gap > limit + EPS {
                notes.push(format!(
                    "EN 1992-1-1 9.3.1.1: the {name} bars are {gap:.0} mm apart, over the {limit:.0} mm most for main bars (3 h, and 400 mm; 2 h and 250 mm where the moment is greatest)"
                ));
            }
        }
        notes.truncate(MAX_NOTES);
        notes
    }

    /// A footing: its cover only.
    pub fn footing(&self, cv: &Cover) -> Vec<String> {
        let mut notes = Vec::new();
        self.cover("footing", 4, true, cv, &mut notes);
        notes
    }
}
