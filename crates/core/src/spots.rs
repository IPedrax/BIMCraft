//! Spot coordinate types, as Revit's type parameters: how a spot coordinate draws (its leader and the arrowhead on it, the
//! leader's pens and colour, its symbol) and what it reads (which point it measures from, which coordinate is on top and
//! which below, the indicators and where they go, the elevation, its units and alternate units), lettered in a font, a size,
//! bold, italic, underline, a width factor and a colour of its own, over an opaque or a transparent background. Lengths are
//! mm on paper, so a spot coordinate reads the same at any scale. A project starts with one from each base point.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::coordinates::Origin;
use crate::dims::{Background, dim_text, rounded};
use crate::pen::Pen;
use crate::text::TextType;
use crate::units::Units;

/// The type a spot coordinate starts in: from the Project Base Point.
pub const DEFAULT: &str = "Spot Coordinate - Project Base Point";
/// The type that reads the shared coordinates, from the Survey Point.
pub const SURVEY: &str = "Spot Coordinate - Survey Point";
/// The most spot coordinate types a project holds.
pub const MAX_TYPES: usize = 1000;

/// What ends a leader at the point: Revit's Leader Arrowhead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arrowhead {
    /// A filled triangle, its tip on the point.
    #[default]
    Filled,
    /// A triangle's outline.
    Empty,
    /// A filled dot on the point.
    Dot,
    /// A dot's outline.
    EmptyDot,
    None,
}

impl Arrowhead {
    pub const ALL: [Arrowhead; 5] = [Arrowhead::Filled, Arrowhead::Empty, Arrowhead::Dot, Arrowhead::EmptyDot, Arrowhead::None];
}

/// What marks the point itself: Revit's Symbol.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Symbol {
    None,
    /// Two short strokes across the point.
    #[default]
    Cross,
    /// A circle with a cross through it.
    Target,
}

/// Which coordinate a line of the text reads: Revit's Top Value and Bottom Value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Value {
    NorthSouth,
    EastWest,
    None,
}

/// Which side of the shoulder the text stands: Revit's Text Location.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextLocation {
    #[default]
    AboveLeader,
    BelowLeader,
}

/// Whether the indicators come before the numbers or after: Revit's Indicator as Prefix / Suffix.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Indicator {
    #[default]
    Prefix,
    Suffix,
}

/// Where the numbers in alternate units go: Revit's Alternate Units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Alternate {
    #[default]
    None,
    /// After each number, on its line.
    Right,
    /// On a line of their own under each.
    Below,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpotCoordinateType {
    /// The text turns as the component it reads is turned.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rotate_with_component: bool,
    #[serde(default)]
    pub leader_arrowhead: Arrowhead,
    /// How long an arrowhead is, or how wide a dot.
    #[serde(default = "arrowhead_size")]
    pub arrowhead_size: f64,
    #[serde(default = "thin")]
    pub leader_pen: Pen,
    #[serde(default = "thin")]
    pub arrowhead_pen: Pen,
    #[serde(default)]
    pub leader_color: [u8; 3],
    #[serde(default)]
    pub symbol: Symbol,
    #[serde(default = "symbol_size")]
    pub symbol_size: f64,
    #[serde(default = "font")]
    pub font: String,
    #[serde(default = "text_size")]
    pub text_size: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    #[serde(default = "one")]
    pub width_factor: f64,
    #[serde(default)]
    pub text_color: [u8; 3],
    /// How far the text stands off the shoulder.
    #[serde(default = "offset")]
    pub text_offset_leader: f64,
    /// How far the text stands off the symbol, with no leader.
    #[serde(default = "offset")]
    pub text_offset_symbol: f64,
    #[serde(default)]
    pub text_location: TextLocation,
    #[serde(default = "transparent")]
    pub background: Background,
    /// Revit's Coordinate Origin.
    #[serde(default)]
    pub origin: Origin,
    #[serde(default = "north_south")]
    pub top: Value,
    #[serde(default = "east_west")]
    pub bottom: Value,
    #[serde(default = "north")]
    pub north_south_indicator: String,
    #[serde(default = "east")]
    pub east_west_indicator: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub include_elevation: bool,
    #[serde(default = "elevation")]
    pub elevation_indicator: String,
    #[serde(default)]
    pub indicator: Indicator,
    /// Its own units, else the project's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<Units>,
    /// Places after the point, in mm, cm, m or decimal feet; else as the units show them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,
    #[serde(default)]
    pub alternate: Alternate,
    #[serde(default = "feet_inches")]
    pub alternate_units: Units,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternate_decimals: Option<u8>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alternate_prefix: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alternate_suffix: String,
}

fn arrowhead_size() -> f64 {
    2.5
}
fn thin() -> Pen {
    Pen(1)
}
fn symbol_size() -> f64 {
    3.0
}
fn font() -> String {
    crate::text::ARIAL.into()
}
fn text_size() -> f64 {
    2.5
}
fn one() -> f64 {
    1.0
}
fn offset() -> f64 {
    1.0
}
fn transparent() -> Background {
    Background::Transparent
}
fn north_south() -> Value {
    Value::NorthSouth
}
fn east_west() -> Value {
    Value::EastWest
}
fn north() -> String {
    "N ".into()
}
fn east() -> String {
    "E ".into()
}
fn elevation() -> String {
    "EL ".into()
}
fn feet_inches() -> Units {
    Units::FeetInches
}

impl Default for SpotCoordinateType {
    fn default() -> SpotCoordinateType {
        SpotCoordinateType {
            rotate_with_component: false,
            leader_arrowhead: Arrowhead::Filled,
            arrowhead_size: arrowhead_size(),
            leader_pen: thin(),
            arrowhead_pen: thin(),
            leader_color: [0, 0, 0],
            symbol: Symbol::Cross,
            symbol_size: symbol_size(),
            font: font(),
            text_size: text_size(),
            bold: false,
            italic: false,
            underline: false,
            width_factor: 1.0,
            text_color: [0, 0, 0],
            text_offset_leader: offset(),
            text_offset_symbol: offset(),
            text_location: TextLocation::AboveLeader,
            background: Background::Transparent,
            origin: Origin::Base,
            top: Value::NorthSouth,
            bottom: Value::EastWest,
            north_south_indicator: north(),
            east_west_indicator: east(),
            include_elevation: false,
            elevation_indicator: elevation(),
            indicator: Indicator::Prefix,
            units: None,
            decimals: None,
            alternate: Alternate::None,
            alternate_units: Units::FeetInches,
            alternate_decimals: None,
            alternate_prefix: String::new(),
            alternate_suffix: String::new(),
        }
    }
}

/// What a spot coordinate reads, mm from its type's origin: east, north and, where it has one, the elevation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reads {
    pub east: f64,
    pub north: f64,
    pub elevation: Option<f64>,
}

impl SpotCoordinateType {
    /// An error saying what to fix, unless it is one a spot coordinate can be drawn in.
    pub fn check(&self, name: &str) -> Result<(), String> {
        let who = format!("spot coordinate type `{name}`");
        let n = self.font.trim().chars().count();
        if n == 0 || n > crate::model::MAX_TEXT || self.font.chars().any(char::is_control) {
            return Err(format!("{who}: a font is named by up to {} characters", crate::model::MAX_TEXT));
        }
        if !(self.text_size > 0.0 && self.text_size <= 100.0) {
            return Err(format!("{who}: its text size, {} mm, isn't above 0 and at most 100", self.text_size));
        }
        if !(0.1..=10.0).contains(&self.width_factor) {
            return Err(format!("{who}: its width factor, {}, isn't from 0.1 to 10", self.width_factor));
        }
        for (what, x) in [
            ("arrowhead size", self.arrowhead_size),
            ("symbol size", self.symbol_size),
            ("text offset from leader", self.text_offset_leader),
            ("text offset from symbol", self.text_offset_symbol),
        ] {
            if !(0.0..=50.0).contains(&x) {
                return Err(format!("{who}: its {what}, {x} mm on paper, isn't from 0 to 50"));
            }
        }
        for d in [self.decimals, self.alternate_decimals].into_iter().flatten() {
            if d > 6 {
                return Err(format!("{who}: it rounds to at most 6 places, not {d}"));
            }
        }
        for (what, t) in [
            ("north/south indicator", &self.north_south_indicator),
            ("east/west indicator", &self.east_west_indicator),
            ("elevation indicator", &self.elevation_indicator),
            ("alternate units prefix", &self.alternate_prefix),
            ("alternate units suffix", &self.alternate_suffix),
        ] {
            dim_text(&who, what, t)?;
        }
        Ok(())
    }

    /// The text type its text is lettered in.
    pub fn lettering(&self) -> TextType {
        TextType {
            font: self.font.clone(),
            size: self.text_size,
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            width_factor: self.width_factor,
            color: self.text_color,
            ..TextType::default()
        }
    }

    /// Its lines of text for what it reads (none known: `?`), lengths in `project` units unless it has its own: the top
    /// value, the bottom one and the elevation, each with its indicator, and the alternate units as it says.
    pub fn lines(&self, read: Option<Reads>, project: Units) -> Vec<String> {
        let units = self.units.unwrap_or(project);
        let mut out = Vec::new();
        let mut put = |indicator: &str, mm: Option<f64>| {
            let shown = mm.map_or_else(|| "?".to_string(), |x| rounded(units, self.decimals, x));
            let mut line = match self.indicator {
                Indicator::Prefix => format!("{indicator}{shown}"),
                Indicator::Suffix => format!("{shown}{indicator}"),
            };
            let alt = mm.map(|x| {
                format!("{}{}{}", self.alternate_prefix, rounded(self.alternate_units, self.alternate_decimals, x), self.alternate_suffix)
            });
            match (self.alternate, alt) {
                (Alternate::Right, Some(a)) => {
                    line.push(' ');
                    line.push_str(&a);
                    out.push(line);
                }
                (Alternate::Below, Some(a)) => {
                    out.push(line);
                    out.push(a);
                }
                _ => out.push(line),
            }
        };
        for v in [self.top, self.bottom] {
            match v {
                Value::NorthSouth => put(&self.north_south_indicator, read.map(|r| r.north)),
                Value::EastWest => put(&self.east_west_indicator, read.map(|r| r.east)),
                Value::None => {}
            }
        }
        if self.include_elevation {
            put(&self.elevation_indicator, read.and_then(|r| r.elevation));
        }
        out
    }
}

/// The spot coordinate types a project starts with: one from the Project Base Point, one in the shared coordinates (in metres
/// to four places), which a new spot coordinate takes unless it says otherwise.
pub fn types() -> BTreeMap<String, SpotCoordinateType> {
    BTreeMap::from([
        (DEFAULT.to_string(), SpotCoordinateType::default()),
        // The map's numbers, in metres to the tenth of a millimetre, as a surveyor's CAD reads them out.
        (
            SURVEY.to_string(),
            SpotCoordinateType { origin: Origin::Survey, units: Some(Units::Metres), decimals: Some(4), ..SpotCoordinateType::default() },
        ),
    ])
}

/// The type a new spot coordinate takes when none is named: the first that reads from `origin`, [`DEFAULT`] or [`SURVEY`]
/// before the rest; else the first there is.
pub fn default_type(types: &BTreeMap<String, SpotCoordinateType>, origin: Origin) -> Option<&String> {
    let starter = if origin == Origin::Survey { SURVEY } else { DEFAULT };
    types
        .get_key_value(starter)
        .filter(|(_, t)| t.origin == origin)
        .map(|(k, _)| k)
        .or_else(|| types.iter().find(|(_, t)| t.origin == origin).map(|(k, _)| k))
        .or_else(|| types.keys().next())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_type_reads_its_lines_as_revit_lays_them() {
        let t = SpotCoordinateType::default();
        let r = Reads { east: 3000.0, north: 4000.0, elevation: Some(-300.0) };
        assert_eq!(t.lines(Some(r), Units::Millimetres), ["N 4000", "E 3000"]);
        assert_eq!(t.lines(None, Units::Metres), ["N ?", "E ?"]);
        let t = SpotCoordinateType { alternate: Alternate::Right, alternate_units: Units::Feet, top: Value::None, ..t };
        assert_eq!(t.lines(Some(r), Units::Millimetres), ["E 3000 9.84'"]);
        let t = SpotCoordinateType { alternate: Alternate::Below, alternate_prefix: "[".into(), alternate_suffix: "]".into(), ..t };
        assert_eq!(t.lines(Some(r), Units::Millimetres), ["E 3000", "[9.84']"]);
    }

    #[test]
    fn a_new_spot_coordinate_takes_the_type_of_its_origin() {
        let mut ts = types();
        assert_eq!(default_type(&ts, Origin::Survey).map(String::as_str), Some(SURVEY));
        assert_eq!(default_type(&ts, Origin::Base).map(String::as_str), Some(DEFAULT));
        ts.remove(SURVEY);
        assert_eq!(default_type(&ts, Origin::Survey).map(String::as_str), Some(DEFAULT), "else the first there is");
    }
}
