//! A structural code's usual limits on the bars `reinforce` makes, as notes: the check asks, it never refuses, and it designs
//! nothing. The limits are the Structural tab's Concrete section of the project's building code ([`ConcreteRules`]); BIMCraft's
//! library has NBR 6118:2014's (`data/codes/structural/nbr-6118.json`), typed from course material and handbooks and not read
//! from the standard's own text (which is paid for): check one against your copy before relying on it. Each note cites the
//! code's standard and the item its rule names. A limit that needs what a bar list doesn't hold (shear, the load, the aggregate)
//! is not here.

use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{Bar, Cover, Frame, along};
use crate::model::TOL;
use crate::view3d::P3;

/// The limits bars are checked against: the code's section.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConcreteRules {
    /// What `reinforce {check}` names and each note cites: `NBR 6118`.
    pub standard: String,
    /// The environmental exposure classes, by the names the code gives them (I to IV), in order: `aggressiveness` 1 is the
    /// first.
    pub classes: Vec<String>,
    /// The nominal cover (mm) by member (`beam`, `column`, `floor`, `ground`: in contact with the ground), one for each class.
    pub covers: BTreeMap<String, Vec<f64>>,
    pub beam: BeamRules,
    pub column: ColumnRules,
    pub floor: FloorRules,
    /// The item of the code each check cites, by the check: `cover`, `beam_ratio`, `beam_skin`, `beam_stirrup`,
    /// `column_bars`, `column_ratio`, `column_spacing`, `column_stirrup`, `floor`.
    pub items: BTreeMap<String, String>,
}

/// A beam's limits.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeamRules {
    /// The most longitudinal steel, a fraction of the section.
    pub ratio_max: f64,
    /// The least tension steel, a fraction of the section, up to concrete of `ratio_min_fck_max` MPa.
    pub ratio_min: f64,
    pub ratio_min_fck_max: f64,
    /// A beam deeper than `skin_from_depth` (mm) wants skin bars of `skin_ratio` of the section on each face, at most
    /// `skin_spacing_max` mm and d / `skin_spacing_d` apart.
    pub skin_from_depth: f64,
    pub skin_ratio: f64,
    pub skin_spacing_max: f64,
    pub skin_spacing_d: f64,
    /// Stirrups: the least diameter (mm), at most the width over `stirrup_width_ratio`, at most `stirrup_spacing_d` d and
    /// `stirrup_spacing_max` mm apart.
    pub stirrup_dia_min: f64,
    pub stirrup_width_ratio: f64,
    pub stirrup_spacing_d: f64,
    pub stirrup_spacing_max: f64,
}

/// A column's limits.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnRules {
    /// Bars: the least diameter (mm), at most the smaller side over `bar_side_ratio`, the fewest in a rectangular and a round
    /// column.
    pub bar_dia_min: f64,
    pub bar_side_ratio: f64,
    pub bars_min_rect: u32,
    pub bars_min_round: u32,
    /// The least and the most steel, fractions of the section.
    pub ratio_min: f64,
    pub ratio_max: f64,
    /// Bars along a face at most `bar_spacing_sides` times the smaller side and `bar_spacing_max` mm apart.
    pub bar_spacing_sides: f64,
    pub bar_spacing_max: f64,
    /// Stirrups: at least `stirrup_dia_min` mm and the bars' diameter over `stirrup_bar_ratio`; at most `stirrup_spacing_max`
    /// mm, the smaller side and `stirrup_spacing_bars` bar diameters apart.
    pub stirrup_dia_min: f64,
    pub stirrup_bar_ratio: f64,
    pub stirrup_spacing_max: f64,
    pub stirrup_spacing_bars: f64,
}

/// A slab's limits.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FloorRules {
    /// Bars at most the thickness over `bar_thickness_ratio`, and at most `spacing_thickness` times it and `spacing_max` mm
    /// apart.
    pub bar_thickness_ratio: f64,
    pub spacing_thickness: f64,
    pub spacing_max: f64,
}

impl crate::codes::Section for ConcreteRules {
    const DISCIPLINE: crate::codes::Discipline = crate::codes::Discipline::Structural;
    const KEY: &'static str = "concrete";
    fn check(&self) -> Result<(), String> {
        if self.standard.trim().is_empty() || self.standard.chars().count() > 64 {
            return Err("concrete: `standard` is what notes cite, 1 to 64 characters".into());
        }
        if self.classes.is_empty() || self.classes.len() > 20 || self.covers.len() > 20 {
            return Err("concrete: 1 to 20 exposure classes, and covers for 20 members at most".into());
        }
        for (member, row) in &self.covers {
            if row.len() != self.classes.len() || row.iter().any(|c| !c.is_finite() || !(0.0..=500.0).contains(c)) {
                return Err(format!("concrete: the `{member}` covers are one for each class, from 0 to 500 mm"));
            }
        }
        let (b, c, f) = (&self.beam, &self.column, &self.floor);
        let numbers = [
            b.ratio_max,
            b.ratio_min,
            b.ratio_min_fck_max,
            b.skin_from_depth,
            b.skin_ratio,
            b.skin_spacing_max,
            b.skin_spacing_d,
            b.stirrup_dia_min,
            b.stirrup_width_ratio,
            b.stirrup_spacing_d,
            b.stirrup_spacing_max,
            c.bar_dia_min,
            c.bar_side_ratio,
            c.ratio_min,
            c.ratio_max,
            c.bar_spacing_sides,
            c.bar_spacing_max,
            c.stirrup_dia_min,
            c.stirrup_bar_ratio,
            c.stirrup_spacing_max,
            c.stirrup_spacing_bars,
            f.bar_thickness_ratio,
            f.spacing_thickness,
            f.spacing_max,
        ];
        if numbers.iter().any(|v| !v.is_finite() || *v <= 0.0 || *v > 100_000.0) {
            return Err("concrete: every limit is a positive number".into());
        }
        if b.ratio_min > b.ratio_max || c.ratio_min > c.ratio_max || b.ratio_max > 1.0 || c.ratio_max > 1.0 {
            return Err("concrete: a steel ratio is a fraction of the section, the least no more than the most".into());
        }
        if self.items.len() > 50 || self.items.iter().any(|(k, v)| k.chars().count() > 64 || v.chars().count() > 64) {
            return Err("concrete: 50 items at most, of 64 characters".into());
        }
        Ok(())
    }
}

/// What the check is told besides the bars: the exposure class (1 for the code's first) and the concrete's `fck` in MPa, and
/// the code's limits.
pub(super) struct Norm {
    pub class: Option<u8>,
    pub fck: Option<f64>,
    pub rules: Arc<ConcreteRules>,
}

/// The most notes one host gets.
pub(super) const MAX_NOTES: usize = 24;
/// Slack for comparing a figure with a limit: the limits are exact, so a bar of 4 mm is under 5.
pub(super) const EPS: f64 = 1e-6;

pub(super) fn area(d: f64) -> f64 {
    PI * d * d / 4.0
}

pub(super) fn on<'a>(bars: &'a [Bar], layers: &'a [&str]) -> impl Iterator<Item = &'a Bar> {
    bars.iter().filter(move |b| layers.contains(&b.layer))
}

pub(super) fn steel(bars: &[Bar], layers: &[&str]) -> f64 {
    on(bars, layers).map(|b| area(b.diameter)).sum()
}

pub(super) fn thickest(bars: &[Bar], layers: &[&str]) -> f64 {
    on(bars, layers).map(|b| b.diameter).fold(0.0, f64::max)
}

/// The widest gap between neighbours among `xs`, the ones within `TOL` of each other being one.
pub(super) fn widest(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < TOL);
    xs.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max)
}

/// `x` as a percentage, as a note says it: 0.0015 is 0.15, never 0.15000000000000002.
fn pc(x: f64) -> f64 {
    (100.0 * x * 1e9).round() / 1e9
}

impl Norm {
    /// What a note of check `which` cites: the standard and the item.
    fn cite(&self, which: &str) -> String {
        match self.rules.items.get(which) {
            Some(item) => format!("{} {item}", self.rules.standard),
            None => self.rules.standard.clone(),
        }
    }

    /// The nominal cover, mm, for the class and the kind of member, if a class was told.
    fn cover(&self, kind: &str, cv: &Cover, notes: &mut Vec<String>) {
        let Some(class) = self.class else { return };
        let r = &self.rules;
        let member = if r.covers.contains_key(kind) { kind } else { "ground" };
        let i = usize::from(class.max(1) - 1);
        let (Some(least), Some(name)) = (r.covers.get(member).and_then(|row| row.get(i)).copied(), r.classes.get(i)) else { return };
        for (face, c) in [("top", cv.top), ("bottom", cv.bottom), ("sides", cv.sides)] {
            if c < least - EPS {
                notes.push(format!(
                    "{}: the {kind}'s {face} cover of {c} mm is under the {least} mm the code gives for class {name}",
                    self.cite("cover")
                ));
            }
        }
    }

    /// A beam `w` wide and `h` deep, its bars `bars` in the frame `f`.
    pub(super) fn beam(&self, (w, h): (f64, f64), cv: &Cover, f: &Frame, bars: &[Bar]) -> Vec<String> {
        let r = &self.rules.beam;
        let mut notes = Vec::new();
        self.cover("beam", cv, &mut notes);
        let ac = w * h;
        let (all, tension) = (steel(bars, &["bottom", "bent", "top", "skin"]), steel(bars, &["bottom", "bent"]));
        let ratio = self.cite("beam_ratio");
        if all > r.ratio_max * ac {
            notes.push(format!(
                "{ratio}: the longitudinal bars are {:.1} % of the section, over the {} % most",
                pc(all / ac),
                pc(r.ratio_max)
            ));
        }
        match self.fck {
            Some(c) if c > r.ratio_min_fck_max && tension > 0.0 => {
                notes.push(format!(
                    "{ratio}: the minimum steel for C{c} isn't in the code's rules here: only up to C{} is",
                    r.ratio_min_fck_max
                ));
            }
            _ if tension > 0.0 && tension < r.ratio_min * ac => notes.push(format!(
                "{ratio}: the bottom bars are {:.3} % of the section, under the {} % the code gives up to C{}",
                pc(tension / ac),
                pc(r.ratio_min),
                r.ratio_min_fck_max
            )),
            _ => {}
        }
        let dst = thickest(bars, &["stirrup"]);
        let dl = bars.iter().find(|b| b.layer == "bottom").map_or(0.0, |b| b.diameter);
        let d = h - cv.bottom - dst - dl / 2.0;
        if h > r.skin_from_depth {
            let skin = self.cite("beam_skin");
            let per_face = steel(bars, &["skin"]) / 2.0;
            if per_face < r.skin_ratio * ac - EPS {
                notes.push(format!(
                    "{skin}: a beam over {} mm deep wants skin bars of {:.2} % of the section on each face, and these have {:.3} %",
                    r.skin_from_depth,
                    pc(r.skin_ratio),
                    pc(per_face / ac)
                ));
            } else {
                let gap = widest(on(bars, &["skin"]).filter_map(|b| b.path.first().map(|p| along(f, f.c, *p))).collect());
                let limit = (d / r.skin_spacing_d).min(r.skin_spacing_max);
                if gap > limit + EPS {
                    notes.push(format!(
                        "{skin}: skin bars {gap:.0} mm apart, over the d/{} or {} mm limit ({limit:.0} mm)",
                        r.skin_spacing_d, r.skin_spacing_max
                    ));
                }
            }
        }
        if dst > 0.0 {
            let stirrup = self.cite("beam_stirrup");
            if dst < r.stirrup_dia_min - EPS {
                notes.push(format!("{stirrup}: stirrups of {dst} mm are under the {} mm least", r.stirrup_dia_min));
            }
            if dst > w / r.stirrup_width_ratio + EPS {
                notes.push(format!(
                    "{stirrup}: stirrups of {dst} mm are over 1/{} of the beam's width ({:.1} mm)",
                    r.stirrup_width_ratio,
                    w / r.stirrup_width_ratio
                ));
            }
            let gap = widest(on(bars, &["stirrup"]).filter_map(|b| b.path.first().map(|p| along(f, f.a, *p))).collect());
            let limit = (r.stirrup_spacing_d * d).min(r.stirrup_spacing_max);
            if gap > limit + EPS {
                notes.push(format!(
                    "{stirrup}: stirrups {gap:.0} mm apart, over min({} d, {}) = {limit:.0} mm (closer where shear is high)",
                    r.stirrup_spacing_d, r.stirrup_spacing_max
                ));
            }
        }
        notes.truncate(MAX_NOTES);
        notes
    }

    /// A column `w` by `d` (`w` across when `round`), its bars `bars` in the frame `f`.
    pub(super) fn column(&self, (w, d): (f64, f64), round: bool, cv: &Cover, f: &Frame, bars: &[Bar]) -> Vec<String> {
        let r = &self.rules.column;
        let mut notes = Vec::new();
        self.cover("column", cv, &mut notes);
        let least = if round { w } else { w.min(d) };
        let (dl, n) = (thickest(bars, &["bars"]), on(bars, &["bars"]).count());
        if n > 0 {
            let cite = self.cite("column_bars");
            if dl < r.bar_dia_min - EPS {
                notes.push(format!("{cite}: bars of {dl} mm are under the {} mm least for a column", r.bar_dia_min));
            }
            if dl > least / r.bar_side_ratio + EPS {
                notes.push(format!(
                    "{cite}: bars of {dl} mm are over 1/{} of the column's smaller side ({:.1} mm)",
                    r.bar_side_ratio,
                    least / r.bar_side_ratio
                ));
            }
            let fewest = if round { r.bars_min_round } else { r.bars_min_rect };
            if (n as u32) < fewest {
                notes.push(format!(
                    "{cite}: a {} column has at least {fewest} bars, and this has {n}",
                    if round { "round" } else { "rectangular" }
                ));
            }
            let ac = if round { PI * w * w / 4.0 } else { w * d };
            let as_ = steel(bars, &["bars"]);
            let ratio = self.cite("column_ratio");
            if as_ < r.ratio_min * ac - EPS {
                notes.push(format!("{ratio}: the bars are {:.2} % of the section, under the {} % least", pc(as_ / ac), pc(r.ratio_min)));
            } else if as_ > r.ratio_max * ac {
                notes.push(format!("{ratio}: the bars are {:.2} % of the section, over the {} % most", pc(as_ / ac), pc(r.ratio_max)));
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
                let (gap, limit) = (widest(on_c).max(widest(on_b)), (r.bar_spacing_sides * least).min(r.bar_spacing_max));
                if gap > limit + EPS {
                    notes
                        .push(format!("{}: bars {gap:.0} mm apart along a face, over the {limit:.0} mm most", self.cite("column_spacing")));
                }
            }
        }
        let dst = thickest(bars, &["stirrup"]);
        if dst > 0.0 {
            let cite = self.cite("column_stirrup");
            let need = r.stirrup_dia_min.max(dl / r.stirrup_bar_ratio);
            if dst < need - EPS {
                notes.push(format!(
                    "{cite}: stirrups of {dst} mm are under the {need} mm least ({} mm, or 1/{} of the bars' diameter)",
                    r.stirrup_dia_min, r.stirrup_bar_ratio
                ));
            }
            let gap = widest(on(bars, &["stirrup"]).filter_map(|b| b.path.first().map(|p| along(f, f.a, *p))).collect());
            let limit = r.stirrup_spacing_max.min(least).min(r.stirrup_spacing_bars * dl);
            if dl > 0.0 && gap > limit + EPS {
                notes.push(format!(
                    "{cite}: stirrups {gap:.0} mm apart, over min({}, the smaller side, {} bar diameters) = {limit:.0} mm",
                    r.stirrup_spacing_max, r.stirrup_spacing_bars
                ));
            }
        }
        notes.truncate(MAX_NOTES);
        notes
    }

    /// A floor `thick` mm thick.
    pub(super) fn floor(&self, thick: f64, cv: &Cover, bars: &[Bar]) -> Vec<String> {
        let r = &self.rules.floor;
        let cite = self.cite("floor");
        let mut notes = Vec::new();
        self.cover("floor", cv, &mut notes);
        let dl = bars.iter().map(|b| b.diameter).fold(0.0, f64::max);
        if dl > thick / r.bar_thickness_ratio + EPS {
            notes.push(format!(
                "{cite}: bars of {dl} mm are over 1/{} of the slab's thickness ({:.1} mm)",
                r.bar_thickness_ratio,
                thick / r.bar_thickness_ratio
            ));
        }
        let limit = (r.spacing_thickness * thick).min(r.spacing_max);
        for name in ["bottom x", "bottom y", "top x", "top y"] {
            let across = |b: &Bar| b.path.get(b.path.len() / 2).map(|p| if name.ends_with('x') { p[1] } else { p[0] });
            let gap = widest(bars.iter().filter(|b| b.layer == name).filter_map(across).collect());
            if gap > limit + EPS {
                notes.push(format!(
                    "{cite}: the {name} bars are {gap:.0} mm apart, over the {limit:.0} mm most ({} h, and {} mm)",
                    r.spacing_thickness, r.spacing_max
                ));
            }
        }
        notes.truncate(MAX_NOTES);
        notes
    }

    /// A footing: its cover only.
    pub(super) fn footing(&self, cv: &Cover) -> Vec<String> {
        let mut notes = Vec::new();
        self.cover("footing", cv, &mut notes);
        notes
    }
}
