//! Building codes: the rules a project's stairs and railings are held to, apart from its technical standard, which is
//! about drafting. A project names a code (`Model::building_code`) and starts with its rules; it may change any of them
//! (`Model::stair_rules`: a state's or a city's own numbers), and nothing is ever refused for breaking one: a stair
//! outside its rules is built and reported ([`stair_findings`], the `stair_check` command).
//!
//! The numbers are data, not code: `data/building-codes.json` holds an entry for each code, naming its sources. They
//! were typed from memory of those documents and are unchecked, as the file says; where a code gives no number the
//! entry holds a forgiving one. The plan this is the first piece of: `docs/stair-tool-plan.md`.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::model::{Landing, Model, Stair};
use crate::stairs;

/// The code of a project that has chosen none: wide ranges round common practice.
pub const GENERIC: &str = "generic";

/// How long a landing must be.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LandingRule {
    /// As long as the stair is wide, `landing_length` being enough.
    Width,
    /// `landing_length`.
    Fixed,
    /// A going and one step (two risers and a going), and `landing_length` at least.
    Step,
}

/// Where a winder's going is measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WalkLine {
    /// Along the middle of the stair.
    Centre,
    /// `walk_line_offset` in from the narrow side.
    NarrowSide,
}

/// The rules stairs and railings are held to, in millimetres: a code's, or a project's own. What each means is in
/// `data/building-codes.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    fn lengths(&self) -> [(&'static str, f64); 20] {
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

/// The rules' names, in the order the data lists them.
pub const ORDER: [&str; 25] = [
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
    "nosing_max",
    "open_risers",
    "uniformity",
    "handrail_min",
    "handrail_max",
    "guard_min",
];

/// A building code, an entry of `data/building-codes.json`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildingCode {
    /// What `building_code {set}` takes, and a project file holds.
    pub code: String,
    pub name: String,
    pub region: String,
    /// The documents its numbers are from.
    pub references: Vec<String>,
    /// What an entry leaves out of its code, and which of its numbers are forgiving.
    pub note: String,
    pub stairs: StairRules,
}

#[derive(Deserialize)]
struct Data {
    codes: Vec<BuildingCode>,
}

/// The rules of no code, should the data not read (which a test keeps from happening): a stair as it always was.
fn fallback() -> BuildingCode {
    BuildingCode {
        code: GENERIC.into(),
        name: "No building code".into(),
        region: String::new(),
        references: Vec::new(),
        note: String::new(),
        stairs: StairRules {
            riser: 180.0,
            going: 280.0,
            riser_min: 100.0,
            riser_max: 220.0,
            going_min: 210.0,
            going_max: 400.0,
            step_min: 550.0,
            step_max: 700.0,
            width_min: 800.0,
            headroom: 2000.0,
            flight_rise_max: 4000.0,
            flight_risers_max: 22,
            landing_rule: LandingRule::Width,
            landing_length: 1200.0,
            winders: true,
            walk_line: WalkLine::Centre,
            walk_line_offset: 300.0,
            winder_going_min: 210.0,
            winder_narrow_min: 100.0,
            nosing_max: 40.0,
            open_risers: true,
            uniformity: 10.0,
            handrail_min: 800.0,
            handrail_max: 1100.0,
            guard_min: 900.0,
        },
    }
}

/// The codes of `data/building-codes.json`, the generic one first.
pub fn codes() -> &'static [BuildingCode] {
    static CODES: OnceLock<Vec<BuildingCode>> = OnceLock::new();
    CODES.get_or_init(|| {
        let read = serde_json::from_str::<Data>(include_str!("../data/building-codes.json")).map(|d| d.codes).unwrap_or_default();
        if read.iter().any(|c| c.code == GENERIC) { read } else { vec![fallback()] }
    })
}

/// A code by what it is called (`nbr`, `ibc`…).
pub fn code(name: &str) -> Option<&'static BuildingCode> {
    codes().iter().find(|c| c.code == name)
}

/// The generic code.
pub fn generic() -> &'static BuildingCode {
    static NONE: OnceLock<BuildingCode> = OnceLock::new();
    code(GENERIC).unwrap_or_else(|| NONE.get_or_init(fallback))
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
    let rules = m.stair_rules();
    let (n, riser, _) = m.flight(st);
    let mut out = Vec::new();
    if n == 0 {
        return out;
    }
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
    range(
        "going_min",
        "going_max",
        "each going",
        st.tread,
        rules.going_min,
        rules.going_max,
        ("deepen its `tread`", "shorten its `tread`"),
    );
    range(
        "step_min",
        "step_max",
        "two risers and a going",
        2.0 * riser + st.tread,
        rules.step_min,
        rules.step_max,
        ("deepen its `tread`, or give it fewer risers", "shorten its `tread`, or give it more risers"),
    );
    if st.width < rules.width_min - EPS {
        out.push(Finding {
            rule: "width_min",
            value: mm(st.width),
            limit: rules.width_min,
            message: format!("it is {} mm wide, under the least, {} mm: widen it", mm(st.width), rules.width_min),
        });
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
        let going = radius * l.turn.abs().to_radians() / f64::from(l.winders);
        if going < rules.winder_going_min - EPS {
            out.push(Finding {
                rule: "winder_going_min",
                value: mm(going),
                limit: rules.winder_going_min,
                message: format!(
                    "its {} winders are {} mm deep on the walk line, {} mm from the narrow side, under the least, {} mm: give the turn fewer winders, or widen it",
                    l.winders,
                    mm(going),
                    mm(radius),
                    rules.winder_going_min
                ),
            });
        }
        if rules.winder_narrow_min > EPS {
            out.push(Finding {
                rule: "winder_narrow_min",
                value: 0.0,
                limit: rules.winder_narrow_min,
                message: format!("its winders come to a point at the narrow end, where the least depth is {} mm", rules.winder_narrow_min),
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

#[cfg(test)]
mod tests;
