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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildingCode {
    /// What `building_code {set}` takes, and a project file holds. One of a project's own is called by its name.
    pub code: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub region: String,
    /// The documents its numbers are from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<String>,
    /// What an entry leaves out of its code, and which of its numbers are forgiving.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// The code one of a project's own started from, by what it is called.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub based_on: String,
    pub stairs: StairRules,
}

/// The most building codes a project keeps of its own, the longest name of one, and the largest file one is read from.
pub const MAX_OWN: usize = 100;
pub const MAX_NAME: usize = 64;
pub const MAX_XML: usize = 1 << 20;

/// What a name for a project's own code must be: something, 64 characters at most, on one line, and not what BIMCraft
/// calls one of its own. The name, trimmed.
pub fn own_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME || name.chars().any(char::is_control) {
        return Err(format!("a building code's name is 1 to {MAX_NAME} characters on one line"));
    }
    if codes().iter().any(|c| c.code.eq_ignore_ascii_case(name) || c.name.eq_ignore_ascii_case(name)) {
        return Err(format!("`{name}` is a building code BIMCraft has: give the project's own another name"));
    }
    Ok(name.to_string())
}

impl BuildingCode {
    /// One of a project's own: a name that can be, texts of a size, rules that can be.
    pub fn check(&self) -> Result<(), String> {
        if own_name(&self.code)? != self.code {
            return Err(format!("the building code `{}` has spaces round its name", self.code));
        }
        let long = |t: &str| t.chars().count() > 2000;
        if long(&self.name)
            || long(&self.region)
            || long(&self.note)
            || long(&self.based_on)
            || self.references.len() > 50
            || self.references.iter().any(|r| long(r))
        {
            return Err(format!("the building code `{}` has a text over 2000 characters, or more than 50 references", self.code));
        }
        self.stairs.check().map_err(|e| format!("the building code `{}`: {e}", self.code))
    }

    /// As a file to hand to another project: XML, a `rule` for each of its rules.
    pub fn to_xml(&self) -> String {
        let esc = |t: &str| t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str(&format!(
            "<building-code format=\"1\" name=\"{}\" region=\"{}\" based-on=\"{}\">\n",
            esc(&self.name),
            esc(&self.region),
            esc(&self.based_on)
        ));
        if !self.note.is_empty() {
            out.push_str(&format!("  <note>{}</note>\n", esc(&self.note)));
        }
        for r in &self.references {
            out.push_str(&format!("  <reference>{}</reference>\n", esc(r)));
        }
        out.push_str("  <!-- Lengths in millimetres. A rule left out is read as the generic code's. -->\n  <stairs>\n");
        if let Ok(serde_json::Value::Object(rules)) = serde_json::to_value(&self.stairs) {
            for key in ORDER {
                let said = match rules.get(key) {
                    Some(serde_json::Value::String(t)) => esc(t),
                    Some(v) => v.to_string(),
                    None => continue,
                };
                out.push_str(&format!("    <rule name=\"{key}\" value=\"{said}\"/>\n"));
            }
        }
        out.push_str("  </stairs>\n</building-code>\n");
        out
    }

    /// A code read from such a file, and what the file holds that this version doesn't know. A rule the file leaves out
    /// is the generic code's; the code is called by its name, or by `name`.
    pub fn from_xml(text: &str, name: Option<&str>) -> Result<(BuildingCode, Vec<String>), String> {
        if text.len() > MAX_XML {
            return Err("the file is larger than a building code's (1 MB)".into());
        }
        let unesc =
            |t: &str| t.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&");
        let Ok(serde_json::Value::Object(mut rules)) = serde_json::to_value(&generic().stairs) else {
            return Err("the rules could not be read".into());
        };
        let mut code = BuildingCode {
            code: String::new(),
            name: String::new(),
            region: String::new(),
            references: Vec::new(),
            note: String::new(),
            based_on: String::new(),
            stairs: generic().stairs.clone(),
        };
        let (mut left_out, mut root, mut rest) = (Vec::new(), false, text);
        while let Some(open) = rest.find('<') {
            let after = rest.get(open + 1..).unwrap_or("");
            // A declaration or a comment: past it.
            let skip = if after.starts_with("!--") {
                Some("-->")
            } else if after.starts_with('?') {
                Some("?>")
            } else {
                None
            };
            if let Some(end) = skip {
                rest = after.find(end).and_then(|i| after.get(i + end.len()..)).unwrap_or("");
                continue;
            }
            let Some(close) = after.find('>') else { return Err("a tag of the file never closes".into()) };
            let tag = after.get(..close).unwrap_or("").trim_end_matches('/').trim();
            rest = after.get(close + 1..).unwrap_or("");
            if tag.starts_with('/') {
                continue;
            }
            let (name, attrs) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
            // name="value" pairs, in either quote.
            let mut pairs: Vec<(String, String)> = Vec::new();
            let mut a = attrs.trim_start();
            while let Some((key, value)) = a.split_once('=') {
                let value = value.trim_start();
                let Some(q) = value.chars().next().filter(|q| *q == '"' || *q == '\'') else { break };
                let Some(end) = value.get(1..).and_then(|v| v.find(q)) else { break };
                pairs.push((key.trim().to_string(), unesc(value.get(1..=end).unwrap_or(""))));
                a = value.get(end + 2..).unwrap_or("").trim_start();
                if pairs.len() > 64 {
                    break;
                }
            }
            let attr = |key: &str| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
            let text = || unesc(rest.split('<').next().unwrap_or("").trim());
            match name {
                "building-code" => {
                    root = true;
                    code.name = attr("name").unwrap_or_default();
                    code.region = attr("region").unwrap_or_default();
                    code.based_on = attr("based-on").unwrap_or_default();
                }
                "note" => code.note = text(),
                "reference" if code.references.len() < 50 => code.references.push(text()),
                "stairs" => {}
                "rule" => {
                    let (key, value) = (attr("name").unwrap_or_default(), attr("value").unwrap_or_default());
                    if !ORDER.contains(&key.as_str()) {
                        left_out.push(format!("rule `{key}`"));
                        continue;
                    }
                    let v = match value.trim() {
                        "true" => serde_json::Value::Bool(true),
                        "false" => serde_json::Value::Bool(false),
                        t => match t.parse::<f64>() {
                            Ok(x) if x.is_finite() && x.fract() == 0.0 && (0.0..1e9).contains(&x) && key == "flight_risers_max" => {
                                serde_json::json!(x as u64)
                            }
                            Ok(x) if x.is_finite() => serde_json::json!(x),
                            _ => serde_json::Value::String(t.to_string()),
                        },
                    };
                    rules.insert(key, v);
                }
                other => {
                    if left_out.len() < 50 {
                        left_out.push(format!("<{}>", other.chars().take(40).collect::<String>()));
                    }
                }
            }
        }
        if !root {
            return Err("the file is no building code: it has no <building-code>".into());
        }
        code.code = own_name(name.unwrap_or(&code.name)).map_err(|e| format!("{e} (`name` gives it one)"))?;
        code.name = code.code.clone();
        code.stairs = serde_json::from_value(serde_json::Value::Object(rules)).map_err(|e| format!("a rule of the file: {e}"))?;
        code.check()?;
        Ok((code, left_out))
    }
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
        based_on: String::new(),
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
    // A floor standing over its steps with less than the headroom between them, and not cut open there.
    if let Some((floor, least, _)) = stairs::headroom(m, st, rules.headroom).into_iter().min_by(|a, b| a.1.total_cmp(&b.1)) {
        out.push(Finding {
            rule: "headroom",
            value: mm(least),
            limit: rules.headroom,
            message: format!(
                "under floor {floor} there is {} mm of headroom at the least, under the {} mm asked: cut the floor open over the stair (`stair_opening` does)",
                mm(least),
                rules.headroom
            ),
        });
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
    let rules = m.stair_rules();
    let (n0, each0, _) = m.flight(st);
    if n0 == 0 {
        return Vec::new();
    }
    let rise = f64::from(n0) * each0;
    let counts: Vec<u32> = if locks.risers {
        vec![n0]
    } else {
        let lo = (rise / rules.riser_max - 1e-9).ceil().max(2.0) as u32;
        let hi = if rules.riser_min > 0.0 { (rise / rules.riser_min + 1e-9).floor().max(2.0) as u32 } else { lo + 40 };
        let hi = hi.clamp(lo, lo + 40);
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
        if !locks.landings && st.spiral.is_none() {
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

#[cfg(test)]
mod tests;
