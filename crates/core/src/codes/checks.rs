//! The architectural sections beyond stairs, and the checks of the elements they hold to a rule: ramps (`RampRules`), doors
//! (`DoorRules`) and rooms (`RoomRules`). Each check reads the element alone, its layout or its room's region: fast and local,
//! as every code check must be (`docs/areas/building-codes.md`). Nothing is refused: what is outside a rule is a finding.

use serde::{Deserialize, Serialize};

use super::{Discipline, Finding, Section};
use crate::model::{Model, Opening, Ramp, Room};

/// Lengths nearer than this are the same.
const EPS: f64 = 1e-6;

fn mm(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// The rules ramps are held to, and what a new ramp starts with.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RampRules {
    /// A new ramp's slope (per cent) and width (mm).
    pub slope: f64,
    pub width: f64,
    /// The steepest slope, per cent.
    pub slope_max: f64,
    /// The steepest slope by a run's rise: `[rise up to (mm), slope (%)]`, the first row whose rise holds the run's; a run
    /// rising past the last row is due a landing. Empty: `slope_max` whatever the rise.
    #[serde(default)]
    pub slope_by_rise: Vec<[f64; 2]>,
    /// The most a run may rise between landings, mm; 0 for no limit.
    pub run_rise_max: f64,
    /// The least clear width, and the least length of a landing, mm.
    pub width_min: f64,
    pub landing_length_min: f64,
}

impl Section for RampRules {
    const DISCIPLINE: Discipline = Discipline::Architectural;
    const KEY: &'static str = "ramps";
    fn check(&self) -> Result<(), String> {
        for (k, v, most) in [
            ("slope", self.slope, 100.0),
            ("slope_max", self.slope_max, 100.0),
            ("width", self.width, 100_000.0),
            ("run_rise_max", self.run_rise_max, 100_000.0),
            ("width_min", self.width_min, 100_000.0),
            ("landing_length_min", self.landing_length_min, 100_000.0),
        ] {
            if !v.is_finite() || !(0.0..=most).contains(&v) {
                return Err(format!("ramps: `{k}` is from 0 to {most}, not {v}"));
            }
        }
        if self.slope <= 0.0 || self.width <= 0.0 {
            return Err("ramps: a new ramp's `slope` and `width` are more than 0".into());
        }
        if self.slope_by_rise.len() > 50
            || self.slope_by_rise.iter().any(|[r, s]| !r.is_finite() || !s.is_finite() || *r <= 0.0 || !(0.0..=100.0).contains(s))
            || self.slope_by_rise.windows(2).any(|w| w[0][0] >= w[1][0])
        {
            return Err("ramps: `slope_by_rise` is [rise mm, slope %] rows, rises from the lowest up, 50 at most".into());
        }
        Ok(())
    }
}

impl RampRules {
    /// The steepest slope (%) a run rising `rise` mm may have, and whether the table holds the rise at all.
    pub fn slope_for(&self, rise: f64) -> (f64, bool) {
        if self.slope_by_rise.is_empty() {
            return (self.slope_max, true);
        }
        match self.slope_by_rise.iter().find(|[r, _]| rise <= r + EPS) {
            Some([_, s]) => (s.min(self.slope_max), true),
            None => (self.slope_max, false),
        }
    }
}

/// Where ramp `r` is outside the project's rules.
pub fn ramp_findings(m: &Model, r: &Ramp) -> Vec<Finding> {
    let rules = m.rules::<RampRules>();
    let mut out = Vec::new();
    let lay = crate::stairs::ramp(m, r);
    let width = |w: f64| if w > 0.0 { w } else { r.width };
    let narrowest = lay.runs.iter().map(|x| width(x.width)).fold(r.width, f64::min);
    if narrowest < rules.width_min - EPS {
        out.push(Finding {
            rule: "width_min",
            value: mm(narrowest),
            limit: rules.width_min,
            message: format!("ramp: {} mm wide, under the {} mm `width_min`: make it wider", mm(narrowest), rules.width_min),
        });
    }
    if r.slope > rules.slope_max + EPS {
        out.push(Finding {
            rule: "slope_max",
            value: mm(r.slope),
            limit: rules.slope_max,
            message: format!("ramp: a slope of {} %, steeper than the {} % `slope_max`: make it gentler", mm(r.slope), rules.slope_max),
        });
    }
    let mut steep = false;
    for run in &lay.runs {
        let rise = run.z.1 - run.z.0;
        let (allowed, held) = rules.slope_for(rise);
        if !held || (rules.run_rise_max > 0.0 && rise > rules.run_rise_max + EPS) {
            let most = if rules.run_rise_max > 0.0 { rules.run_rise_max } else { rules.slope_by_rise.last().map_or(0.0, |r| r[0]) };
            out.push(Finding {
                rule: "run_rise_max",
                value: mm(rise),
                limit: most,
                message: format!("ramp: a run rises {} mm, more than the {most} mm a run may rise: add a landing", mm(rise)),
            });
            break;
        }
        if !steep && r.slope > allowed + EPS && allowed < rules.slope_max - EPS {
            steep = true;
            out.push(Finding {
                rule: "slope_by_rise",
                value: mm(r.slope),
                limit: allowed,
                message: format!(
                    "ramp: a run rising {} mm may slope {allowed} % at most, and this one slopes {} %: make it gentler, or add a landing",
                    mm(rise),
                    mm(r.slope)
                ),
            });
        }
    }
    for l in &r.landings {
        let depth = if l.length > 0.0 { l.length } else { r.width };
        if depth < rules.landing_length_min - EPS {
            out.push(Finding {
                rule: "landing_length_min",
                value: mm(depth),
                limit: rules.landing_length_min,
                message: format!(
                    "ramp: a landing {} mm long, under the {} mm `landing_length_min`: make it longer",
                    mm(depth),
                    rules.landing_length_min
                ),
            });
            break;
        }
    }
    out
}

/// The rules doors are held to.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoorRules {
    /// The least clear width and height, mm.
    pub clear_width_min: f64,
    pub clear_height_min: f64,
    /// The widest leaf, mm; 0 for no limit.
    pub leaf_width_max: f64,
    /// What a door's width loses to its frame and stops: its clear width is its type's width less this, mm.
    pub frame_allowance: f64,
    /// Doors whose type's name holds one of these (any case) are not held to the rules: a closet's, a shaft's.
    #[serde(default)]
    pub exempt: Vec<String>,
}

impl Section for DoorRules {
    const DISCIPLINE: Discipline = Discipline::Architectural;
    const KEY: &'static str = "doors";
    fn check(&self) -> Result<(), String> {
        for (k, v) in [
            ("clear_width_min", self.clear_width_min),
            ("clear_height_min", self.clear_height_min),
            ("leaf_width_max", self.leaf_width_max),
            ("frame_allowance", self.frame_allowance),
        ] {
            if !v.is_finite() || !(0.0..=100_000.0).contains(&v) {
                return Err(format!("doors: `{k}` is from 0 to 100000 mm, not {v}"));
            }
        }
        if self.exempt.len() > 50 || self.exempt.iter().any(|t| t.trim().is_empty() || t.chars().count() > 64) {
            return Err("doors: 50 `exempt` names at most, each 1 to 64 characters".into());
        }
        Ok(())
    }
}

/// Where door `e` is outside the project's rules.
pub fn door_findings(m: &Model, e: &crate::model::Element, o: &Opening) -> Vec<Finding> {
    let rules = m.rules::<DoorRules>();
    let name = o.family.as_deref().unwrap_or(&o.type_name).to_lowercase();
    if rules.exempt.iter().any(|x| name.contains(&x.to_lowercase())) {
        return Vec::new();
    }
    let Ok(t) = m.opening_dims(e) else { return Vec::new() };
    let mut out = Vec::new();
    let clear = t.width - rules.frame_allowance;
    if clear < rules.clear_width_min - EPS {
        out.push(Finding {
            rule: "clear_width_min",
            value: mm(clear),
            limit: rules.clear_width_min,
            message: format!("door: {} mm clear, under the {} mm `clear_width_min`: use a wider door", mm(clear), rules.clear_width_min),
        });
    }
    if t.height < rules.clear_height_min - EPS {
        out.push(Finding {
            rule: "clear_height_min",
            value: mm(t.height),
            limit: rules.clear_height_min,
            message: format!(
                "door: {} mm high, under the {} mm `clear_height_min`: use a taller door",
                mm(t.height),
                rules.clear_height_min
            ),
        });
    }
    if rules.leaf_width_max > 0.0 && t.width > rules.leaf_width_max + EPS {
        out.push(Finding {
            rule: "leaf_width_max",
            value: mm(t.width),
            limit: rules.leaf_width_max,
            message: format!(
                "door: a leaf {} mm wide, over the {} mm `leaf_width_max`: use a pair of leaves",
                mm(t.width),
                rules.leaf_width_max
            ),
        });
    }
    out
}

/// A kind of room and what it must have: rooms are of a kind by their name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomKind {
    pub kind: String,
    /// A room whose name holds one of these (any case) is of this kind; the first kind that holds a room's name wins.
    pub names: Vec<String>,
    /// The least area (m²), side (mm: the room's narrower side) and height (mm), each 0 for none.
    pub area_min: f64,
    pub side_min: f64,
    pub height_min: f64,
}

/// The rules rooms are held to, by kind.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomRules {
    pub kinds: Vec<RoomKind>,
}

impl Section for RoomRules {
    const DISCIPLINE: Discipline = Discipline::Architectural;
    const KEY: &'static str = "rooms";
    fn check(&self) -> Result<(), String> {
        if self.kinds.len() > 100 {
            return Err("rooms: 100 kinds at most".into());
        }
        for k in &self.kinds {
            if k.kind.trim().is_empty()
                || k.kind.chars().count() > 64
                || k.names.len() > 50
                || k.names.iter().any(|n| n.trim().is_empty() || n.chars().count() > 64)
            {
                return Err(format!("rooms: kind `{}` has a name and up to 50 names of rooms, each 1 to 64 characters", k.kind));
            }
            for (what, v) in [("area_min", k.area_min), ("side_min", k.side_min), ("height_min", k.height_min)] {
                if !v.is_finite() || !(0.0..=1.0e6).contains(&v) {
                    return Err(format!("rooms: kind `{}`'s `{what}` is from 0 to a million, not {v}", k.kind));
                }
            }
        }
        Ok(())
    }
}

impl RoomRules {
    /// The kind a room of name `name` is.
    pub fn kind_of(&self, name: &str) -> Option<&RoomKind> {
        let name = name.to_lowercase();
        self.kinds.iter().find(|k| k.names.iter().any(|n| name.contains(&n.to_lowercase())))
    }
}

/// The narrower side of an outline: across its longest edge, the width of what it covers.
fn narrower_side(ring: &[crate::geom::P2]) -> f64 {
    let n = ring.len();
    if n < 3 {
        return 0.0;
    }
    let longest = (0..n).map(|i| (ring[i], ring[(i + 1) % n])).max_by(|a, b| a.0.dist(a.1).total_cmp(&b.0.dist(b.1)));
    let Some((a, b)) = longest else { return 0.0 };
    let len = a.dist(b);
    if len < EPS {
        return 0.0;
    }
    let (ux, uy) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    let along: Vec<f64> = ring.iter().map(|p| p.0 * ux + p.1 * uy).collect();
    let across: Vec<f64> = ring.iter().map(|p| -p.0 * uy + p.1 * ux).collect();
    let span = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max) - v.iter().copied().fold(f64::INFINITY, f64::min);
    span(&along).min(span(&across))
}

/// Where room `r` is outside the project's rules for its kind.
pub fn room_findings(m: &Model, r: &Room) -> Vec<Finding> {
    let rules = m.rules::<RoomRules>();
    let Some(kind) = rules.kind_of(&r.name) else { return Vec::new() };
    let mut out = Vec::new();
    let (ring, closed) = m.room_outline(r);
    if closed && ring.len() >= 3 {
        let area = crate::geom::area(&ring).abs() / 1.0e6;
        if kind.area_min > 0.0 && area < kind.area_min - 1e-9 {
            out.push(Finding {
                rule: "area_min",
                value: mm(area),
                limit: kind.area_min,
                message: format!("room ({}): {} m², under the {} m² a {} has at least", kind.kind, mm(area), kind.area_min, kind.kind),
            });
        }
        let side = narrower_side(&ring);
        if kind.side_min > 0.0 && side < kind.side_min - EPS {
            out.push(Finding {
                rule: "side_min",
                value: mm(side),
                limit: kind.side_min,
                message: format!(
                    "room ({}): {} mm across, under the {} mm a {} has at least",
                    kind.kind,
                    mm(side),
                    kind.side_min,
                    kind.kind
                ),
            });
        }
    }
    let height = m.room_height(r);
    if kind.height_min > 0.0 && height < kind.height_min - EPS {
        out.push(Finding {
            rule: "height_min",
            value: mm(height),
            limit: kind.height_min,
            message: format!("room ({}): {} mm high, under the {} mm a {} has at least", kind.kind, mm(height), kind.height_min, kind.kind),
        });
    }
    out
}
