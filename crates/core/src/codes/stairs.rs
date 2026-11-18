//! Stairs and railings held to a building code: the rules (`StairRules`, the Architectural tab's Stairs section), the
//! rules a stair is held to once its type has its say (`limits`), where a stair is outside them (`stair_findings`) and
//! the solver that proposes the nearest stairs that fit (`solve`). The numbers are data, in `data/codes/architectural/`.

use serde::{Deserialize, Serialize};

use crate::model::{Element, Landing, Model, Stair};
use crate::stairs;

/// How long a landing must be.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LandingRule {
    /// As long as the stair is wide, `landing_length` being enough.
    #[default]
    Width,
    /// `landing_length`.
    Fixed,
    /// A going and one step (two risers and a going), and `landing_length` at least.
    Step,
}

/// Where a winder's going is measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WalkLine {
    /// Along the middle of the stair.
    #[default]
    Centre,
    /// `walk_line_offset` in from the narrow side.
    NarrowSide,
}

/// The rules stairs and railings are held to, in millimetres: a code's, or a project's own. What each means is in
/// `data/codes/architectural/`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StairRules {
    /// The riser and the going a new stair starts with.
    pub riser: f64,
    pub going: f64,
    pub riser_min: f64,
    pub riser_max: f64,
    pub going_min: f64,
    pub going_max: f64,
    /// The range of two risers and a going.
    pub step_min: f64,
    pub step_max: f64,
    pub width_min: f64,
    pub headroom: f64,
    /// A landing is due when a flight would rise more than this, or hold more risers than `flight_risers_max`.
    pub flight_rise_max: f64,
    pub flight_risers_max: u32,
    pub landing_rule: LandingRule,
    pub landing_length: f64,
    /// Whether winding treads are allowed.
    pub winders: bool,
    pub walk_line: WalkLine,
    pub walk_line_offset: f64,
    pub winder_going_min: f64,
    pub winder_narrow_min: f64,
    /// The least and the most a tread's nosing stands out past the riser below it (finish to finish).
    #[serde(default)]
    pub nosing_min: f64,
    pub nosing_max: f64,
    pub open_risers: bool,
    /// The most two risers or goings of a flight may differ.
    pub uniformity: f64,
    pub handrail_min: f64,
    pub handrail_max: f64,
    pub guard_min: f64,
}

/// The longest any rule may be, 100 m: more is a mistake, or hostile.
const MAX_RULE: f64 = 100_000.0;

impl StairRules {
    /// Its lengths by name, as the data and `building_code` name them.
    fn lengths(&self) -> [(&'static str, f64); 21] {
        [
            ("riser", self.riser),
            ("going", self.going),
            ("riser_min", self.riser_min),
            ("riser_max", self.riser_max),
            ("going_min", self.going_min),
            ("going_max", self.going_max),
            ("step_min", self.step_min),
            ("step_max", self.step_max),
            ("width_min", self.width_min),
            ("headroom", self.headroom),
            ("flight_rise_max", self.flight_rise_max),
            ("landing_length", self.landing_length),
            ("walk_line_offset", self.walk_line_offset),
            ("winder_going_min", self.winder_going_min),
            ("winder_narrow_min", self.winder_narrow_min),
            ("nosing_min", self.nosing_min),
            ("nosing_max", self.nosing_max),
            ("uniformity", self.uniformity),
            ("handrail_min", self.handrail_min),
            ("handrail_max", self.handrail_max),
            ("guard_min", self.guard_min),
        ]
    }

    /// Whether these can be rules at all: every length a number from 0 to 100 m, what a new stair starts with and what a
    /// flight may rise more than nothing, and no range upside down. Says what to fix.
    pub fn check(&self) -> Result<(), String> {
        for (name, v) in self.lengths() {
            if !v.is_finite() || !(0.0..=MAX_RULE).contains(&v) {
                return Err(format!("stair rules: `{name}` is a length from 0 to {MAX_RULE} mm, not {v}"));
            }
        }
        for (name, v) in
            [("riser", self.riser), ("going", self.going), ("riser_max", self.riser_max), ("flight_rise_max", self.flight_rise_max)]
        {
            if v < 1.0 {
                return Err(format!("stair rules: `{name}` is 1 mm at least, not {v}"));
            }
        }
        if !(1..=1000).contains(&self.flight_risers_max) {
            return Err(format!("stair rules: `flight_risers_max` is from 1 to 1000 risers, not {}", self.flight_risers_max));
        }
        for (low, a, high, b) in [
            ("riser_min", self.riser_min, "riser_max", self.riser_max),
            ("going_min", self.going_min, "going_max", self.going_max),
            ("step_min", self.step_min, "step_max", self.step_max),
            ("handrail_min", self.handrail_min, "handrail_max", self.handrail_max),
            ("nosing_min", self.nosing_min, "nosing_max", self.nosing_max),
        ] {
            if a > b {
                return Err(format!("stair rules: `{low}` ({a}) is more than `{high}` ({b}); swap them, or change one"));
            }
        }
        Ok(())
    }

    /// The names of the rules that differ from `other`'s, in the order the data lists them.
    pub fn changed(&self, other: &StairRules) -> Vec<String> {
        let (Ok(serde_json::Value::Object(a)), Ok(serde_json::Value::Object(b))) =
            (serde_json::to_value(self), serde_json::to_value(other))
        else {
            return Vec::new();
        };
        ORDER.iter().filter(|k| a.get(**k) != b.get(**k)).map(|k| (*k).to_string()).collect()
    }

    /// How long a landing must be on a stair `width` wide, of risers `riser` high and goings `going` deep.
    pub fn landing_due(&self, width: f64, riser: f64, going: f64) -> f64 {
        match self.landing_rule {
            LandingRule::Width => width.min(self.landing_length),
            LandingRule::Fixed => self.landing_length,
            LandingRule::Step => (going + 2.0 * riser + going).max(self.landing_length),
        }
    }

    /// The most risers a flight of risers `riser` high may hold before a landing is due: 2 at least, a flight's fewest.
    pub fn flight_risers(&self, riser: f64) -> u32 {
        let by_rise = if riser > 0.0 { (self.flight_rise_max / riser + EPS).floor().clamp(0.0, 1000.0) as u32 } else { 1000 };
        self.flight_risers_max.min(by_rise).max(2)
    }

    /// The landings a straight stair of `risers` risers, each `riser` high, is due: as few as keep every flight inside the
    /// rules, the flights as equal as whole risers allow, each landing straight on.
    /// What a new landing's `length` is on a stair `width` wide: 0, as deep as the stair is wide, where that is what
    /// the rules ask; else what they ask.
    pub fn landing_length_for(&self, width: f64, riser: f64, going: f64) -> f64 {
        let due = self.landing_due(width, riser, going);
        if (due - width).abs() < EPS { 0.0 } else { (due * 100.0).round() / 100.0 }
    }

    pub fn landings(&self, risers: u32, riser: f64) -> Vec<Landing> {
        let most = self.flight_risers(riser);
        let flights = risers.div_ceil(most).clamp(1, 100);
        (1..flights)
            .map(|i| Landing { at: (f64::from(risers) * f64::from(i) / f64::from(flights)).round(), ..Landing::default() })
            .filter(|l| l.at >= 2.0 && l.at <= f64::from(risers) - 2.0)
            .collect()
    }
}

impl crate::codes::Section for StairRules {
    const DISCIPLINE: crate::codes::Discipline = crate::codes::Discipline::Architectural;
    const KEY: &'static str = "stairs";
    fn check(&self) -> Result<(), String> {
        StairRules::check(self)
    }
}

/// The rules' names, in the order the data lists them.
pub const ORDER: [&str; 26] = [
    "riser",
    "going",
    "riser_min",
    "riser_max",
    "going_min",
    "going_max",
    "step_min",
    "step_max",
    "width_min",
    "headroom",
    "flight_rise_max",
    "flight_risers_max",
    "landing_rule",
    "landing_length",
    "winders",
    "walk_line",
    "walk_line_offset",
    "winder_going_min",
    "winder_narrow_min",
    "nosing_min",
    "nosing_max",
    "open_risers",
    "uniformity",
    "handrail_min",
    "handrail_max",
    "guard_min",
];

/// The rules a stair is held to: the project's, with what its type has on in their place; and the rules only a type
/// has, each a least and a most that may be off: the tread over the riser, the tread and the riser together, and the
/// pitch in degrees.
#[derive(Clone, Debug, PartialEq)]
pub struct Limits {
    pub rules: StairRules,
    pub ratio: (Option<f64>, Option<f64>),
    pub sum: (Option<f64>, Option<f64>),
    pub pitch: (Option<f64>, Option<f64>),
}

/// The rules stair `st` is held to.
pub fn limits(m: &Model, st: &Stair) -> Limits {
    limits_of(m, &st.type_name)
}

/// The rules a stair of the stair type `name` is held to; the project's, for no type.
pub fn limits_of(m: &Model, name: &str) -> Limits {
    let mut rules = (*m.stair_rules()).clone();
    let Some(t) = m.types.stair.get(name).map(|t| t.rules) else {
        return Limits { rules, ratio: (None, None), sum: (None, None), pitch: (None, None) };
    };
    let set = |to: &mut f64, v: Option<f64>| {
        if let Some(v) = v {
            *to = v;
        }
    };
    set(&mut rules.riser_min, t.riser_min);
    set(&mut rules.riser_max, t.riser_max);
    set(&mut rules.going_min, t.going_min);
    set(&mut rules.going_max, t.going_max);
    set(&mut rules.step_min, t.step_min);
    set(&mut rules.step_max, t.step_max);
    // A range the type turned upside down against the code's other end holds at the type's number.
    rules.riser_min = rules.riser_min.min(rules.riser_max);
    rules.going_min = rules.going_min.min(rules.going_max);
    rules.step_min = rules.step_min.min(rules.step_max);
    // What a new stair starts with is inside the range it is held to.
    rules.riser = rules.riser.clamp(rules.riser_min, rules.riser_max);
    rules.going = rules.going.clamp(rules.going_min, rules.going_max);
    if let Some(v) = t.landing_min {
        (rules.landing_rule, rules.landing_length) = (LandingRule::Fixed, v);
    }
    if let Some(v) = t.walk_offset_min {
        (rules.walk_line, rules.walk_line_offset) = (WalkLine::NarrowSide, v);
    }
    Limits { rules, ratio: (t.ratio_min, t.ratio_max), sum: (t.sum_min, t.sum_max), pitch: (t.pitch_min, t.pitch_max) }
}

/// Where a stair is outside its project's rules: the rule by name, what the stair has, what the rule asks, and a
/// sentence saying both and what to change.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Finding {
    pub rule: &'static str,
    pub value: f64,
    pub limit: f64,
    pub message: String,
}

/// Lengths nearer than this are the same: a riser worked out by dividing is not held to the last digit.
const EPS: f64 = 1e-6;

fn mm(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// Where stair `st` is outside the project's rules, in the order the rules are listed; none when it is inside them all.
pub fn stair_findings(m: &Model, st: &Stair) -> Vec<Finding> {
    let held = limits(m, st);
    let rules = &held.rules;
    let (n, riser, _) = m.flight(st);
    let mut out = Vec::new();
    if n == 0 {
        return out;
    }
    // One drawn by hand is measured as drawn: its shallowest tread on the line through its risers' middles, and its
    // narrowest riser.
    let drawn = !st.sketch.is_empty();
    let going = if drawn {
        st.sketch.windows(2).map(|w| w.first().zip(w.get(1)).map_or(0.0, |(a, b)| a.mid().dist(b.mid()))).fold(f64::INFINITY, f64::min)
    } else {
        st.tread
    };
    let width = if drawn {
        st.sketch.iter().map(|s| s.riser[0].dist(s.riser[1])).fold(f64::INFINITY, f64::min)
    } else {
        // Its narrowest run.
        st.landings.iter().filter(|l| l.width > 0.0).map(|l| l.width).fold(st.width, f64::min)
    };
    let mut range = |rule_min: &'static str, rule_max: &'static str, what: &str, v: f64, min: f64, max: f64, fix: (&str, &str)| {
        if v < min - EPS {
            out.push(Finding {
                rule: rule_min,
                value: mm(v),
                limit: min,
                message: format!("{what} is {} mm, under the least, {min} mm: {}", mm(v), fix.0),
            });
        } else if v > max + EPS {
            out.push(Finding {
                rule: rule_max,
                value: mm(v),
                limit: max,
                message: format!("{what} is {} mm, over the most, {max} mm: {}", mm(v), fix.1),
            });
        }
    };
    range(
        "riser_min",
        "riser_max",
        "each riser",
        riser,
        rules.riser_min,
        rules.riser_max,
        ("give it fewer risers (a higher `riser`)", "lower its `riser`, for more risers"),
    );
    range("going_min", "going_max", "each going", going, rules.going_min, rules.going_max, ("deepen its `tread`", "shorten its `tread`"));
    range(
        "step_min",
        "step_max",
        "two risers and a going",
        2.0 * riser + going,
        rules.step_min,
        rules.step_max,
        ("deepen its `tread`, or give it fewer risers", "shorten its `tread`, or give it more risers"),
    );
    if width < rules.width_min - EPS {
        out.push(Finding {
            rule: "width_min",
            value: mm(width),
            limit: rules.width_min,
            message: format!("it is {} mm wide, under the least, {} mm: widen it", mm(width), rules.width_min),
        });
    }
    // How far a tread stands out over the riser below it, finish to finish; and whether its risers are open.
    if let Some(t) = m.types.stair.get(&st.type_name) {
        let nosing = stairs::reach(t);
        if nosing < rules.nosing_min - EPS {
            out.push(Finding {
                rule: "nosing_min",
                value: mm(nosing),
                limit: rules.nosing_min,
                message: format!(
                    "its treads stand out {} mm past the riser below, under the least, {} mm: give its type a longer nosing (`nosing`, or `cover_nosing` for the finish)",
                    mm(nosing),
                    rules.nosing_min
                ),
            });
        } else if nosing > rules.nosing_max + EPS {
            out.push(Finding {
                rule: "nosing_max",
                value: mm(nosing),
                limit: rules.nosing_max,
                message: format!(
                    "its treads stand out {} mm past the riser below, over the most, {} mm: give its type a shorter nosing",
                    mm(nosing),
                    rules.nosing_max
                ),
            });
        }
        if !rules.open_risers && !t.monolithic() && t.riser_thickness <= 0.0 {
            out.push(Finding {
                rule: "open_risers",
                value: 1.0,
                limit: 0.0,
                message: "its risers are open, which this code does not allow: use a type with risers (`riser_thickness`)".into(),
            });
        }
    }
    // The rules only a stair type has: the tread over the riser, the two together, and the pitch.
    if riser > 0.0 && going.is_finite() {
        let more = [
            ("ratio_min", "ratio_max", "the tread over the riser", going / riser, held.ratio, ""),
            ("sum_min", "sum_max", "a tread and a riser together", going + riser, held.sum, " mm"),
            ("pitch_min", "pitch_max", "its pitch", (riser / going.max(1e-9)).atan().to_degrees(), held.pitch, "°"),
        ];
        for (low, high, what, v, (min, max), unit) in more {
            let v = (v * 1000.0).round() / 1000.0;
            if let Some(min) = min.filter(|min| v < min - EPS) {
                out.push(Finding {
                    rule: low,
                    value: v,
                    limit: min,
                    message: format!("{what} is {v}{unit}, under the least its type asks, {min}{unit}"),
                });
            } else if let Some(max) = max.filter(|max| v > max + EPS) {
                out.push(Finding {
                    rule: high,
                    value: v,
                    limit: max,
                    message: format!("{what} is {v}{unit}, over the most its type asks, {max}{unit}"),
                });
            }
        }
    }
    // A floor, a ceiling or a beam over its steps with less than the headroom between them, and not cut open there.
    if let Some((over, least, _)) = stairs::headroom(m, st, rules.headroom).into_iter().min_by(|a, b| a.1.total_cmp(&b.1)) {
        let message = match m.elements.get(over) {
            Some(Element::Floor(_)) => format!(
                "under floor {over} there is {} mm of headroom at the least, under the {} mm asked: cut the floor open over the stair (`stair_opening` does)",
                mm(least),
                rules.headroom
            ),
            Some(e) => format!(
                "under {} {over} there is {} mm of headroom at the least, under the {} mm asked: raise it or move it clear of the stair",
                e.category(),
                mm(least),
                rules.headroom
            ),
            None => String::new(),
        };
        out.push(Finding { rule: "headroom", value: mm(least), limit: rules.headroom, message });
    }
    // A multistorey stair's other storeys have risers of their own heights: each inside the range, and none far from
    // the first's.
    let others: Vec<f64> = m.storeys(st).into_iter().skip(1).map(|s| m.flight_of(st, s.2).1).collect();
    let (low, high) = others.iter().fold((riser, riser), |(lo, hi), r| (lo.min(*r), hi.max(*r)));
    if let Some((i, r)) = others.iter().enumerate().find(|(_, r)| **r < rules.riser_min - EPS || **r > rules.riser_max + EPS) {
        let (rule, limit, what) = if *r > rules.riser_max {
            ("riser_max", rules.riser_max, "over the most")
        } else {
            ("riser_min", rules.riser_min, "under the least")
        };
        if !out.iter().any(|f| f.rule == rule) {
            out.push(Finding {
                rule,
                value: mm(*r),
                limit,
                message: format!(
                    "each riser of storey {} is {} mm, {what}, {limit} mm: change its `riser`, which every storey counts its risers by",
                    i + 2,
                    mm(*r)
                ),
            });
        }
    }
    if high - low > rules.uniformity + EPS {
        out.push(Finding {
            rule: "uniformity",
            value: mm(high - low),
            limit: rules.uniformity,
            message: format!(
                "its storeys are of different heights, and their risers differ by {} mm ({} to {}), over the most, {} mm: a `riser` that divides each storey more evenly brings them together",
                mm(high - low),
                mm(low),
                mm(high),
                rules.uniformity
            ),
        });
    }
    // One drawn by hand is one flight.
    if !st.sketch.is_empty() {
        let rise = f64::from(n) * riser;
        if rise > rules.flight_rise_max + EPS {
            out.push(Finding {
                rule: "flight_rise_max",
                value: mm(rise),
                limit: rules.flight_rise_max,
                message: format!(
                    "it rises {} mm with no landing, and one is due every {} mm at most: draw a tread as deep as a landing",
                    mm(rise),
                    rules.flight_rise_max
                ),
            });
        }
    }
    // A spiral is one flight, and every tread of it a winder.
    if let Some(sp) = st.spiral {
        let rise = f64::from(n) * riser;
        if rise > rules.flight_rise_max + EPS {
            out.push(Finding {
                rule: "flight_rise_max",
                value: mm(rise),
                limit: rules.flight_rise_max,
                message: format!(
                    "the spiral rises {} mm in one flight, and a landing is due every {} mm at most",
                    mm(rise),
                    rules.flight_rise_max
                ),
            });
        } else if n > rules.flight_risers_max {
            out.push(Finding {
                rule: "flight_risers_max",
                value: f64::from(n),
                limit: f64::from(rules.flight_risers_max),
                message: format!("the spiral has {n} risers in one flight, and a landing is due after {} at most", rules.flight_risers_max),
            });
        }
        if !rules.winders {
            out.push(Finding {
                rule: "winders",
                value: f64::from(n.saturating_sub(1)),
                limit: 0.0,
                message: "it is a spiral, all winders, which this code does not allow".into(),
            });
        }
        let (step, inside) = (stairs::spiral_step(st, sp, n), (sp.radius.abs() - st.width / 2.0).max(0.0));
        let radius = match rules.walk_line {
            WalkLine::Centre => sp.radius.abs(),
            WalkLine::NarrowSide => inside + rules.walk_line_offset.min(st.width),
        };
        let (going, narrow) = (radius * step, inside * step);
        if going < rules.winder_going_min - EPS {
            out.push(Finding {
                rule: "winder_going_min",
                value: mm(going),
                limit: rules.winder_going_min,
                message: format!(
                    "its treads are {} mm deep on the walk line, {} mm from the centre, under the least, {} mm: deepen its `tread`, give it a larger `radius`, or a wider `sweep`",
                    mm(going),
                    mm(radius),
                    rules.winder_going_min
                ),
            });
        }
        if narrow < rules.winder_narrow_min - EPS {
            out.push(Finding {
                rule: "winder_narrow_min",
                value: mm(narrow),
                limit: rules.winder_narrow_min,
                message: format!(
                    "its treads are {} mm deep at the narrow end, under the least, {} mm: give it a larger `radius`, so it stands further from its centre",
                    mm(narrow),
                    rules.winder_narrow_min
                ),
            });
        }
    }
    for (i, run) in stairs::stair(m, st).runs.iter().enumerate() {
        let rise = run.z.1 - run.z.0;
        if rise > rules.flight_rise_max + EPS {
            out.push(Finding {
                rule: "flight_rise_max",
                value: mm(rise),
                limit: rules.flight_rise_max,
                message: format!(
                    "flight {} rises {} mm, and a landing is due every {} mm at most: give it a landing (`landings`: \"auto\" places them)",
                    i + 1,
                    mm(rise),
                    rules.flight_rise_max
                ),
            });
        } else if run.risers > rules.flight_risers_max {
            out.push(Finding {
                rule: "flight_risers_max",
                value: f64::from(run.risers),
                limit: f64::from(rules.flight_risers_max),
                message: format!(
                    "flight {} has {} risers, and a landing is due after {} at most: give it a landing (`landings`: \"auto\" places them)",
                    i + 1,
                    run.risers,
                    rules.flight_risers_max
                ),
            });
        }
    }
    // Winders: whether the code has them at all, and their goings on its walk line and at the narrow end, which
    // is nothing where they are fanned round a point.
    let mut said_no = false;
    for l in st.landings.iter().filter(|l| l.winders > 0) {
        if !rules.winders && !said_no {
            said_no = true;
            out.push(Finding {
                rule: "winders",
                value: f64::from(l.winders),
                limit: 0.0,
                message: "it has winders, which this code does not allow: give the turn a landing (`winders`: 0)".into(),
            });
        }
        let radius = match rules.walk_line {
            WalkLine::Centre => st.width / 2.0,
            WalkLine::NarrowSide => rules.walk_line_offset.min(st.width),
        };
        // Balanced, the turn's steps are its winders and the treads it takes of the flights, equal along the walk line and
        // along the inside string.
        let (into, steps) = (f64::from(l.balance) * st.tread, f64::from(l.winders + 2 * l.balance));
        let going = (2.0 * into + radius * l.turn.abs().to_radians()) / steps;
        let narrow = 2.0 * into / steps;
        if going < rules.winder_going_min - EPS {
            out.push(Finding {
                rule: "winder_going_min",
                value: mm(going),
                limit: rules.winder_going_min,
                message: format!(
                    "its {} winders are {} mm deep on the walk line, {} mm from the narrow side, under the least, {} mm: give the turn fewer winders, balance them into the flights (`balance`), or widen it",
                    l.winders,
                    mm(going),
                    mm(radius),
                    rules.winder_going_min
                ),
            });
        }
        if narrow < rules.winder_narrow_min - EPS {
            out.push(Finding {
                rule: "winder_narrow_min",
                value: mm(narrow),
                limit: rules.winder_narrow_min,
                message: if l.balance == 0 {
                    format!(
                        "its winders come to a point at the narrow end, where the least depth is {} mm: balance them into the flights (`balance`: treads of each flight)",
                        rules.winder_narrow_min
                    )
                } else {
                    format!(
                        "its winders are {} mm deep at the narrow end, under the least, {} mm: balance them over more treads (`balance`), or give the turn fewer winders",
                        mm(narrow),
                        rules.winder_narrow_min
                    )
                },
            });
        }
    }
    // A landing is as deep as the stair is wide unless it says, and no less where it turns left or right.
    let due = rules.landing_due(st.width, riser, st.tread);
    let deep = |l: &Landing| {
        if l.length <= 0.0 {
            st.width
        } else if l.turn.abs() == 90.0 {
            l.length.max(st.width)
        } else {
            l.length
        }
    };
    if let Some((i, d)) =
        st.landings.iter().enumerate().filter(|(_, l)| l.winders == 0).map(|(i, l)| (i, deep(l))).find(|(_, d)| *d < due - EPS)
    {
        out.push(Finding {
            rule: "landing_length",
            value: mm(d),
            limit: mm(due),
            message: format!(
                "landing {} is {} mm long, under the least, {} mm: give it a `length` of {} or more",
                i + 1,
                mm(d),
                mm(due),
                mm(due)
            ),
        });
    }
    out
}

/// What `solve` leaves as it is: the count of risers, the treads' depth, the width, the landings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Locks {
    pub risers: bool,
    pub tread: bool,
    pub width: bool,
    pub landings: bool,
}

/// A stair `solve` proposes in place of one: its risers, treads, width and landings, what that changes, said, and what
/// it would still be outside of (nothing, for one that fits).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Proposal {
    pub risers: u32,
    pub riser: f64,
    pub tread: f64,
    pub width: f64,
    pub landings: Vec<Landing>,
    pub changes: Vec<String>,
    pub findings: Vec<Finding>,
    #[serde(skip)]
    pub cost: f64,
}

impl Proposal {
    /// `st` as the proposal makes it.
    pub fn applied(&self, st: &Stair) -> Stair {
        Stair { riser: self.riser, tread: self.tread, width: self.width, landings: self.landings.clone(), ..st.clone() }
    }
}

/// Whether every run of a stair of `n` risers with `landings` keeps two risers or more.
fn runs_hold(n: u32, landings: &[Landing]) -> bool {
    let mut last = 0.0;
    for l in landings {
        let (wound, into) = (f64::from(l.wound()), f64::from(l.balance));
        if l.at - into < last + 2.0 || l.at + wound + into > f64::from(n) - 2.0 {
            return false;
        }
        last = l.at + wound + into;
    }
    true
}

/// The stairs nearest `st` that are inside the project's rules, the one that changes least first: other counts of
/// risers, a tread the step rule allows, the least width, the landings the rules ask (the stair's own moved with its
/// risers, lengthened where they are short, and more where a flight is too long). What `locks` names stays as it is.
/// When none fits, the nearest, with what each would still break. A stair already inside the rules is the only answer.
pub fn solve(m: &Model, st: &Stair, locks: Locks) -> Vec<Proposal> {
    let held = limits(m, st);
    let rules = &held.rules;
    let (n0, each0, _) = m.flight(st);
    // One drawn by hand is as it was drawn: there is nothing to count again.
    if n0 == 0 || !st.sketch.is_empty() {
        return Vec::new();
    }
    let rise = f64::from(n0) * each0;
    let counts: Vec<u32> = if locks.risers {
        vec![n0]
    } else {
        let lo = (rise / rules.riser_max - 1e-9).ceil().max(2.0) as u32;
        let hi = if rules.riser_min > 0.0 { (rise / rules.riser_min + 1e-9).floor().max(2.0) as u32 } else { lo.saturating_add(40) };
        // A stair takes 2 to 200 risers: a proposal of more could never be made.
        let hi = hi.clamp(lo, lo.saturating_add(40)).min(crate::model::MAX_RISERS);
        let mut all: Vec<u32> = (lo..=hi).collect();
        if !all.contains(&n0) {
            all.push(n0);
        }
        all
    };
    let mut out: Vec<Proposal> = Vec::new();
    for n in counts {
        let riser = rise / f64::from(n);
        // A tread inside its own range and the step rule's, the nearest to the stair's.
        let tread = if locks.tread {
            st.tread
        } else {
            let (lo, hi) =
                (rules.going_min.max(rules.step_min - 2.0 * riser).ceil(), rules.going_max.min(rules.step_max - 2.0 * riser).floor());
            if lo <= hi { st.tread.clamp(lo, hi) } else { st.tread.clamp(rules.going_min, rules.going_max) }
        };
        let width = if locks.width { st.width } else { st.width.max(rules.width_min) };
        // Its landings, moved with its risers; and with the landings the rules ask of flights too long.
        let scale = f64::from(n) / f64::from(n0);
        let due = rules.landing_length_for(width, riser, tread);
        let moved: Vec<Landing> = st
            .landings
            .iter()
            .map(|l| {
                let mut l = *l;
                l.at = (l.at * scale).round();
                // A landing shorter than the rules ask takes their length.
                let deep = if l.length > 0.0 { l.length } else { width };
                if !locks.landings
                    && l.winders == 0
                    && matches!(l.turn.abs(), 0.0 | 90.0 | 180.0)
                    && deep < rules.landing_due(width, riser, tread) - EPS
                {
                    l.length = due;
                }
                l
            })
            .collect();
        let mut sets = vec![moved.clone()];
        // Winders that come to a point, or too near one: balanced into the flights by as few treads as give their narrow
        // end the least depth the rules ask.
        if !locks.landings {
            let mut balanced = moved.clone();
            for l in balanced.iter_mut().filter(|l| l.winders > 0) {
                let narrow = |j: u32| 2.0 * f64::from(j) * tread / f64::from(l.winders + 2 * j);
                if narrow(l.balance) < rules.winder_narrow_min - EPS
                    && let Some(j) = (l.balance + 1..=12).find(|j| narrow(*j) >= rules.winder_narrow_min - EPS)
                {
                    l.balance = j;
                }
            }
            if balanced != moved {
                sets.insert(0, balanced);
            }
        }
        if !locks.landings && st.spiral.is_none() && st.path.is_empty() {
            let most = f64::from(rules.flight_risers(riser));
            let mut more = Vec::new();
            let mut from = 0.0;
            let ends = moved.iter().map(|l| (l.at - f64::from(l.balance), Some(*l))).chain([(f64::from(n), None)]);
            for (to, turn) in ends {
                let flights = ((to - from) / most).ceil().max(1.0);
                for i in 1..(flights as u32) {
                    more.push(Landing { at: from + ((to - from) * f64::from(i) / flights).round(), length: due, ..Landing::default() });
                }
                if let Some(l) = turn {
                    from = l.at + f64::from(l.wound()) + f64::from(l.balance);
                    more.push(l);
                }
            }
            if more.len() != moved.len() {
                sets.push(more);
            }
        }
        for landings in sets {
            if !runs_hold(n, &landings) {
                continue;
            }
            let p = Proposal { risers: n, riser, tread, width, landings, changes: Vec::new(), findings: Vec::new(), cost: 0.0 };
            let made = p.applied(st);
            let findings = stair_findings(m, &made);
            let mut changes = Vec::new();
            if n != n0 {
                changes.push(format!("{n} risers of {} mm, where it has {n0} of {}", mm(riser), mm(each0)));
            }
            if (tread - st.tread).abs() > EPS {
                changes.push(format!("treads {} mm deep, where they are {}", mm(tread), mm(st.tread)));
            }
            if (width - st.width).abs() > EPS {
                changes.push(format!("{} mm wide, where it is {}", mm(width), mm(st.width)));
            }
            let added = made.landings.len().saturating_sub(st.landings.len());
            if added > 0 {
                let at: Vec<String> = made
                    .landings
                    .iter()
                    .filter(|l| !st.landings.iter().any(|o| (o.at * scale).round() == l.at))
                    .map(|l| l.at.to_string())
                    .collect();
                changes.push(format!("{added} more landing{}, after {} risers", if added == 1 { "" } else { "s" }, at.join(" and ")));
            }
            if added == 0
                && let Some((l, _)) = made.landings.iter().zip(&st.landings).find(|(a, b)| a.balance != b.balance)
            {
                changes.push(format!(
                    "its winders balanced over {} tread{} of the flight on each side",
                    l.balance,
                    if l.balance == 1 { "" } else { "s" }
                ));
            }
            if made.landings.iter().zip(&st.landings).any(|(a, b)| (a.length - b.length).abs() > EPS) && added == 0 {
                changes.push(format!("landings {} mm long", mm(due)));
            }
            let cost = f64::from(n.abs_diff(n0)) * 10.0
                + (tread - st.tread).abs() / 10.0
                + (width - st.width).abs() / 50.0
                + added as f64 * 3.0
                + if changes.is_empty() { 0.0 } else { 0.5 }
                + findings.len() as f64 * 1000.0;
            out.push(Proposal { changes, findings, cost, ..p });
        }
    }
    out.sort_by(|a, b| a.cost.total_cmp(&b.cost));
    // A stair inside the rules as it is needs nothing; else what changes nothing is no proposal.
    if out.first().is_some_and(|p| p.changes.is_empty() && p.findings.is_empty()) {
        out.truncate(1);
        return out;
    }
    out.retain(|p| !p.changes.is_empty());
    let fits = out.iter().filter(|p| p.findings.is_empty()).count();
    out.truncate(if fits > 0 { fits.min(5) } else { 3 });
    out
}

impl crate::codes::BuildingCode {
    /// Its stairs' rules, where it has them whole; the default (zeros) otherwise.
    pub fn stairs(&self) -> StairRules {
        self.section("stairs").and_then(|v| serde_json::from_value(serde_json::Value::Object(v.clone())).ok()).unwrap_or_default()
    }
}
