//! A project's units: how lengths and areas are shown, and how typed lengths read. The model keeps
//! millimetres, and so does every command; units change only what drawings, schedules and the
//! desktop show. Feet and inches round to the nearest sixteenth of an inch.

use std::collections::BTreeMap;

use bimcraft_i18n::ui_msg;

use serde::{Deserialize, Serialize};

use crate::dims::{PositiveSign, ZeroSign};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Units {
    #[default]
    #[serde(rename = "mm")]
    Millimetres,
    #[serde(rename = "cm")]
    Centimetres,
    #[serde(rename = "m")]
    Metres,
    /// Fractional inches.
    #[serde(rename = "in")]
    Inches,
    /// Decimal feet.
    #[serde(rename = "ft")]
    Feet,
    /// Feet and fractional inches.
    #[serde(rename = "ft-in")]
    FeetInches,
}

const INCH: f64 = 25.4;
const FOOT: f64 = 304.8;

impl Units {
    pub const ALL: [Units; 6] = [Units::Millimetres, Units::Centimetres, Units::Metres, Units::Inches, Units::Feet, Units::FeetInches];

    /// Its name in commands and files: mm, cm, m, in, ft, ft-in.
    pub fn name(self) -> &'static str {
        match self {
            Units::Millimetres => "mm",
            Units::Centimetres => "cm",
            Units::Metres => "m",
            Units::Inches => "in",
            Units::Feet => "ft",
            Units::FeetInches => "ft-in",
        }
    }

    /// As a menu names it.
    pub fn label(self) -> &'static str {
        match self {
            Units::Millimetres => ui_msg!("Millimetres"),
            Units::Centimetres => ui_msg!("Centimetres"),
            Units::Metres => ui_msg!("Metres"),
            Units::Inches => ui_msg!("Inches"),
            Units::Feet => ui_msg!("Feet"),
            Units::FeetInches => ui_msg!("Feet and inches"),
        }
    }

    pub fn from_name(name: &str) -> Option<Units> {
        Units::ALL.into_iter().find(|u| u.name().eq_ignore_ascii_case(name.trim()))
    }

    pub fn imperial(self) -> bool {
        matches!(self, Units::Inches | Units::Feet | Units::FeetInches)
    }

    /// A length as drawings show it: 3000, 300, 3.00, 118 1/8", 9.84', 9'-10 1/8".
    pub fn length(self, mm: f64) -> String {
        match self {
            Units::Millimetres => format!("{}", mm.round() + 0.0),
            Units::Centimetres => format!("{}", (mm).round() / 10.0 + 0.0),
            // Rounded first, so what rounds to nothing shows no minus.
            Units::Metres => format!("{:.2}", (mm / 10.0).round() / 100.0 + 0.0),
            Units::Feet => format!("{:.2}'", (mm / FOOT * 100.0).round() / 100.0 + 0.0),
            Units::Inches | Units::FeetInches => self.fractional(mm, 4),
        }
    }

    /// A length in inches or feet and inches rounded to `1/2^exp` of an inch: 0 whole inches, 4 sixteenths, and so on.
    fn fractional(self, mm: f64, exp: u8) -> String {
        let denom = 1u64 << exp.min(10);
        let n = (mm.abs() / INCH * denom as f64).round() as u64;
        let sign = if mm < 0.0 && n > 0 { "-" } else { "" };
        let per_foot = 12 * denom;
        let (feet, rest) = if self == Units::FeetInches { (n / per_foot, n % per_foot) } else { (0, n) };
        let (whole, part) = (rest / denom, rest % denom);
        let (mut top, mut bottom) = (part as u32, denom as u32);
        while top > 0 && top % 2 == 0 {
            (top, bottom) = (top / 2, bottom / 2);
        }
        let inches = match (whole, top) {
            (w, 0) => format!("{w}\""),
            (0, t) if self == Units::Inches => format!("{t}/{bottom}\""),
            (w, t) => format!("{w} {t}/{bottom}\""),
        };
        if self == Units::FeetInches { format!("{sign}{feet}'-{inches}") } else { format!("{sign}{inches}") }
    }

    /// The fewest places a project shows lengths to in these units, which drawings use: from mm 0, cm 1, metres and feet 2;
    /// inches and feet-inches show sixteenths (four binary places).
    pub fn fewest(self) -> Option<u8> {
        match self {
            Units::Millimetres => Some(0),
            Units::Centimetres => Some(1),
            Units::Metres | Units::Feet => Some(2),
            Units::Inches | Units::FeetInches => Some(4),
        }
    }

    /// The places a project may show lengths to in these units: decimal ones for mm, cm, metres and feet; binary fractions of
    /// an inch for inches and feet-inches (4 = 1/16", 0 = whole inches).
    pub fn decimals(self) -> Option<std::ops::RangeInclusive<u8>> {
        let fewest = self.fewest()?;
        if matches!(self, Units::Inches | Units::FeetInches) {
            return Some(0..=4);
        }
        let most = fewest + 3;
        Some(0..=most)
    }

    /// [`Units::length`] to `decimals` places after the point (a project's; none, the fewest), the places of a file held to the
    /// range. A count asked for is shown exactly, trailing zeros and all.
    pub fn length_to(self, mm: f64, decimals: Option<u8>) -> String {
        let Some(range) = self.decimals() else { return self.length(mm) };
        if matches!(self, Units::Inches | Units::FeetInches) {
            return match decimals {
                Some(d) => self.fractional(mm, d.clamp(0, 4)),
                None => self.length(mm),
            };
        }
        match decimals.map(|d| d.clamp(*range.start(), *range.end())) {
            Some(d) => crate::dims::rounded(self, Some(d), mm),
            None => self.length(mm),
        }
    }

    /// A length as a field to edit is filled: as [`Units::length`], unless that reads back as another length (3 mm in metres
    /// is `0.00`), and then in mm with its unit, so a field left alone keeps its value. Inches and feet round as drawings do.
    pub fn field(self, mm: f64) -> String {
        self.field_to(mm, None)
    }

    /// [`Units::field`] to a project's `decimals`.
    pub fn field_to(self, mm: f64, decimals: Option<u8>) -> String {
        let shown = self.length_to(mm, decimals);
        if self.imperial() || self.parse(&shown) == self.parse(&format!("{mm} mm")) {
            return shown;
        }
        format!("{} mm", (mm * 1e3).round() / 1e3 + 0.0)
    }

    /// A length rounded to `digits` places before the point in these units: 1 to the nearest 10, 2 to 100, 3 to 1000.
    pub fn round_before(self, mm: f64, digits: u8) -> f64 {
        let per = match self {
            Units::Millimetres => 1.0,
            Units::Centimetres => 10.0,
            Units::Metres => 1000.0,
            Units::Inches | Units::FeetInches => INCH,
            Units::Feet => FOOT,
        };
        let step = 10f64.powi(i32::from(digits)) * per;
        let rounded = (mm / step).round() * step;
        if rounded.is_finite() { rounded + 0.0 } else { mm }
    }

    /// A length with its unit, as the desktop labels one: 3000 mm, 3.00 m, 9'-10 1/8".
    pub fn with_unit(self, mm: f64) -> String {
        self.with_unit_to(mm, None)
    }

    /// [`Units::with_unit`], the name the common formatter uses.
    pub fn length_with_unit(self, mm: f64) -> String {
        self.with_unit(mm)
    }

    /// [`Units::with_unit`] to a project's `decimals`.
    pub fn with_unit_to(self, mm: f64, decimals: Option<u8>) -> String {
        match self {
            Units::Millimetres | Units::Centimetres | Units::Metres => format!("{} {}", self.length_to(mm, decimals), self.name()),
            _ => self.length_to(mm, decimals),
        }
    }

    /// An area in square metres or, in inches and feet, square feet; and that unit.
    pub fn area(self, mm2: f64) -> (f64, &'static str) {
        if self.imperial() { (mm2 / (FOOT * FOOT), "ft²") } else { (mm2 / 1e6, "m²") }
    }

    /// A volume in cubic metres or, in inches and feet, cubic feet; and that unit.
    pub fn volume(self, mm3: f64) -> (f64, &'static str) {
        if self.imperial() { (mm3 / (FOOT * FOOT * FOOT), "ft³") } else { (mm3 / 1e9, "m³") }
    }

    /// An angle in degrees; and its unit, which no project changes.
    pub fn angle(self, degrees: f64) -> (f64, &'static str) {
        (degrees, "°")
    }

    /// A mass in kilograms or, in inches and feet, pounds; and that unit.
    pub fn weight(self, kg: f64) -> (f64, &'static str) {
        if self.imperial() { (kg / 0.453_592_37, "lb") } else { (kg, "kg") }
    }

    /// A density in kilograms per cubic metre or, in inches and feet, pounds per cubic foot; and that unit.
    pub fn ratio(self, kg_m3: f64) -> (f64, &'static str) {
        if self.imperial() { (kg_m3 * 0.062_427_960_6, "lb/ft³") } else { (kg_m3, "kg/m³") }
    }

    /// The scales, 1 : n, a view in these units is offered: the metric ones; in inches or feet, the architect's (an
    /// inch, or a fraction of one, to the foot) and then the engineer's (an inch to tens of feet).
    pub fn scales(self) -> &'static [f64] {
        if self.imperial() { &IMPERIAL_SCALES } else { &METRIC_SCALES }
    }

    /// The scale a view in these units has until it is given one: 1 : 100, or 1/8" = 1'-0" (1 : 96).
    pub fn default_scale(self) -> f64 {
        if self.imperial() { 96.0 } else { 100.0 }
    }

    /// Scale 1 : `n` as these units name it: `1 : 50`; in inches or feet, the architect's `1/4" = 1'-0"` where an inch or
    /// a fraction of one (to a sixty-fourth) is a foot, the engineer's `1" = 20'-0"` where an inch is ten feet or more,
    /// and `1 : n` for any other.
    pub fn scale(self, n: f64) -> String {
        if self.imperial() {
            let sixty_fourths = 12.0 * 64.0 / n;
            if n <= 12.0 * 64.0 && (sixty_fourths - sixty_fourths.round()).abs() < 1e-9 {
                return format!("{}\" = 1'-0\"", fraction(sixty_fourths.round() as u32));
            }
            let feet = n / 12.0;
            if feet >= 10.0 && (feet - feet.round()).abs() < 1e-9 {
                return format!("1\" = {}'-0\"", feet.round());
            }
        }
        format!("1 : {n}")
    }

    /// A scale typed, the n of 1 : n: `100`, `1:100`, or two lengths, the paper's and the model's, `1/8" = 1'-0"`,
    /// `1" = 20'`, `1 mm = 100 mm` (a length without its unit in these units).
    pub fn parse_scale(self, text: &str) -> Option<f64> {
        let t = text.trim();
        let n = match t.split_once('=') {
            Some((paper, model)) => self.parse_exact(model)? / self.parse_exact(paper)?,
            None => match t.split_once(':') {
                Some((one, n)) if number(one) == Some(1.0) => number(n)?,
                Some(_) => return None,
                None => number(t)?,
            },
        };
        (n.is_finite() && n > 0.0).then_some((n * 1e6).round() / 1e6)
    }

    /// A length typed, in mm: a number alone is in these units (feet, in feet and inches); one
    /// written with its unit (mm, cm, m, ft or ', in or ") is in that. Feet and inches can come
    /// together, and inches with a fraction: 10' 6 1/2", 10'-6", 6 1/2".
    pub fn parse(self, text: &str) -> Option<f64> {
        // To the micron: what was typed exactly stays exact.
        let mm = (self.parse_exact(text)? * 1e3).round() / 1e3;
        mm.is_finite().then_some(mm + 0.0)
    }

    /// [`Units::parse`], not rounded.
    fn parse_exact(self, text: &str) -> Option<f64> {
        let lowered = text.trim().to_ascii_lowercase();
        // A sign as a field shows zero reads as zero.
        let t = lowered.strip_prefix('±').unwrap_or(&lowered).trim().to_string();
        let mm = if let Some(n) = t.strip_suffix("mm") {
            number(n)?
        } else if let Some(n) = t.strip_suffix("cm") {
            number(n)? * 10.0
        } else if let Some(n) = t.strip_suffix('m') {
            number(n)? * 1000.0
        } else if t.contains(['\'', '"']) || t.ends_with("ft") || t.ends_with("in") {
            let t = t.replace("ft", "'").replace("in", "\"");
            let (feet, rest) = t.split_once('\'').unwrap_or(("", &t));
            let negative = feet.trim().starts_with('-') || (feet.trim().is_empty() && rest.trim().starts_with('-'));
            let feet = if feet.trim().is_empty() { 0.0 } else { number(feet)?.abs() };
            let rest = rest.trim().trim_start_matches('-').trim().trim_end_matches('"');
            let inches = if rest.trim().is_empty() { 0.0 } else { inches(rest)? };
            (feet * FOOT + inches * INCH) * if negative { -1.0 } else { 1.0 }
        } else {
            match self {
                Units::Millimetres => number(&t)?,
                Units::Centimetres => number(&t)? * 10.0,
                Units::Metres => number(&t)? * 1000.0,
                Units::Inches => t.strip_prefix('-').map_or_else(|| inches(&t), |t| inches(t).map(|x| -x))? * INCH,
                Units::Feet | Units::FeetInches => number(&t)? * FOOT,
            }
        };
        mm.is_finite().then_some(mm)
    }
}

/// The units an area is shown in: square millimetres, centimetres, metres, inches or feet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AreaUnit {
    #[serde(rename = "mm2")]
    SquareMillimetres,
    #[serde(rename = "cm2")]
    SquareCentimetres,
    #[default]
    #[serde(rename = "m2")]
    SquareMetres,
    #[serde(rename = "in2")]
    SquareInches,
    #[serde(rename = "ft2")]
    SquareFeet,
}

impl AreaUnit {
    pub const ALL: [AreaUnit; 5] =
        [AreaUnit::SquareMillimetres, AreaUnit::SquareCentimetres, AreaUnit::SquareMetres, AreaUnit::SquareInches, AreaUnit::SquareFeet];

    pub fn name(self) -> &'static str {
        match self {
            AreaUnit::SquareMillimetres => "mm2",
            AreaUnit::SquareCentimetres => "cm2",
            AreaUnit::SquareMetres => "m2",
            AreaUnit::SquareInches => "in2",
            AreaUnit::SquareFeet => "ft2",
        }
    }

    pub fn from_name(name: &str) -> Option<AreaUnit> {
        AreaUnit::ALL.into_iter().find(|u| u.name().eq_ignore_ascii_case(name.trim()))
    }

    pub fn label(self) -> &'static str {
        match self {
            AreaUnit::SquareMillimetres => ui_msg!("Square millimetres"),
            AreaUnit::SquareCentimetres => ui_msg!("Square centimetres"),
            AreaUnit::SquareMetres => ui_msg!("Square metres"),
            AreaUnit::SquareInches => ui_msg!("Square inches"),
            AreaUnit::SquareFeet => ui_msg!("Square feet"),
        }
    }

    /// An area in mm² as these units show it, and their symbol.
    pub fn convert(self, mm2: f64) -> (f64, &'static str) {
        match self {
            AreaUnit::SquareMillimetres => (mm2, "mm²"),
            AreaUnit::SquareCentimetres => (mm2 / 100.0, "cm²"),
            AreaUnit::SquareMetres => (mm2 / 1e6, "m²"),
            AreaUnit::SquareInches => (mm2 / (INCH * INCH), "in²"),
            AreaUnit::SquareFeet => (mm2 / (FOOT * FOOT), "ft²"),
        }
    }

    /// Its symbol: mm², cm², m², in² or ft².
    pub fn symbol(self) -> &'static str {
        self.convert(0.0).1
    }

    /// How many mm² one of it is.
    pub fn mm2_per(self) -> f64 {
        match self {
            AreaUnit::SquareMillimetres => 1.0,
            AreaUnit::SquareCentimetres => 100.0,
            AreaUnit::SquareMetres => 1e6,
            AreaUnit::SquareInches => INCH * INCH,
            AreaUnit::SquareFeet => FOOT * FOOT,
        }
    }

    /// The area whose symbol `s` names, such as `cm²`.
    pub fn from_symbol(s: &str) -> Option<AreaUnit> {
        AreaUnit::ALL.into_iter().find(|u| u.symbol() == s)
    }

    /// Whether an area in the project's units is imperial.
    pub fn imperial(self) -> bool {
        matches!(self, AreaUnit::SquareInches | AreaUnit::SquareFeet)
    }
}

/// The units a volume is shown in: cubic millimetres, centimetres, metres, inches or feet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VolumeUnit {
    #[serde(rename = "mm3")]
    CubicMillimetres,
    #[serde(rename = "cm3")]
    CubicCentimetres,
    #[default]
    #[serde(rename = "m3")]
    CubicMetres,
    #[serde(rename = "in3")]
    CubicInches,
    #[serde(rename = "ft3")]
    CubicFeet,
}

impl VolumeUnit {
    pub const ALL: [VolumeUnit; 5] = [
        VolumeUnit::CubicMillimetres,
        VolumeUnit::CubicCentimetres,
        VolumeUnit::CubicMetres,
        VolumeUnit::CubicInches,
        VolumeUnit::CubicFeet,
    ];

    pub fn name(self) -> &'static str {
        match self {
            VolumeUnit::CubicMillimetres => "mm3",
            VolumeUnit::CubicCentimetres => "cm3",
            VolumeUnit::CubicMetres => "m3",
            VolumeUnit::CubicInches => "in3",
            VolumeUnit::CubicFeet => "ft3",
        }
    }

    pub fn from_name(name: &str) -> Option<VolumeUnit> {
        VolumeUnit::ALL.into_iter().find(|u| u.name().eq_ignore_ascii_case(name.trim()))
    }

    pub fn label(self) -> &'static str {
        match self {
            VolumeUnit::CubicMillimetres => ui_msg!("Cubic millimetres"),
            VolumeUnit::CubicCentimetres => ui_msg!("Cubic centimetres"),
            VolumeUnit::CubicMetres => ui_msg!("Cubic metres"),
            VolumeUnit::CubicInches => ui_msg!("Cubic inches"),
            VolumeUnit::CubicFeet => ui_msg!("Cubic feet"),
        }
    }

    /// A volume in mm³ as these units show it, and their symbol.
    pub fn convert(self, mm3: f64) -> (f64, &'static str) {
        match self {
            VolumeUnit::CubicMillimetres => (mm3, "mm³"),
            VolumeUnit::CubicCentimetres => (mm3 / 1000.0, "cm³"),
            VolumeUnit::CubicMetres => (mm3 / 1e9, "m³"),
            VolumeUnit::CubicInches => (mm3 / (INCH * INCH * INCH), "in³"),
            VolumeUnit::CubicFeet => (mm3 / (FOOT * FOOT * FOOT), "ft³"),
        }
    }

    pub fn symbol(self) -> &'static str {
        self.convert(0.0).1
    }

    pub fn mm3_per(self) -> f64 {
        match self {
            VolumeUnit::CubicMillimetres => 1.0,
            VolumeUnit::CubicCentimetres => 1000.0,
            VolumeUnit::CubicMetres => 1e9,
            VolumeUnit::CubicInches => INCH * INCH * INCH,
            VolumeUnit::CubicFeet => FOOT * FOOT * FOOT,
        }
    }

    pub fn from_symbol(s: &str) -> Option<VolumeUnit> {
        VolumeUnit::ALL.into_iter().find(|u| u.symbol() == s)
    }

    pub fn imperial(self) -> bool {
        matches!(self, VolumeUnit::CubicInches | VolumeUnit::CubicFeet)
    }
}

/// How a slope is shown: degrees, percent, or a grade (1 : n).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlopeUnit {
    #[default]
    Degrees,
    Percent,
}

impl SlopeUnit {
    pub const ALL: [SlopeUnit; 2] = [SlopeUnit::Degrees, SlopeUnit::Percent];

    pub fn name(self) -> &'static str {
        match self {
            SlopeUnit::Degrees => "deg",
            SlopeUnit::Percent => "percent",
        }
    }

    pub fn from_name(name: &str) -> Option<SlopeUnit> {
        SlopeUnit::ALL.into_iter().find(|u| u.name().eq_ignore_ascii_case(name.trim()))
    }

    pub fn label(self) -> &'static str {
        match self {
            SlopeUnit::Degrees => ui_msg!("Degrees"),
            SlopeUnit::Percent => ui_msg!("Percent"),
        }
    }

    /// A slope in degrees as these units show it.
    pub fn convert(self, degrees: f64) -> (f64, &'static str) {
        match self {
            SlopeUnit::Degrees => (degrees, "°"),
            SlopeUnit::Percent => (degrees.to_radians().tan() * 100.0, "%"),
        }
    }
}

/// The units a mass density is shown in: kilograms per cubic metre or pounds per cubic foot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MassDensityUnit {
    #[default]
    KilogramsPerCubicMetre,
    PoundsPerCubicFoot,
}

impl MassDensityUnit {
    pub const ALL: [MassDensityUnit; 2] = [MassDensityUnit::KilogramsPerCubicMetre, MassDensityUnit::PoundsPerCubicFoot];

    pub fn name(self) -> &'static str {
        match self {
            MassDensityUnit::KilogramsPerCubicMetre => "kg-m3",
            MassDensityUnit::PoundsPerCubicFoot => "lb-ft3",
        }
    }

    pub fn from_name(name: &str) -> Option<MassDensityUnit> {
        MassDensityUnit::ALL.into_iter().find(|u| u.name().eq_ignore_ascii_case(name.trim()))
    }

    pub fn label(self) -> &'static str {
        match self {
            MassDensityUnit::KilogramsPerCubicMetre => ui_msg!("Kilograms per cubic metre"),
            MassDensityUnit::PoundsPerCubicFoot => ui_msg!("Pounds per cubic foot"),
        }
    }

    pub fn convert(self, kg_m3: f64) -> (f64, &'static str) {
        match self {
            MassDensityUnit::KilogramsPerCubicMetre => (kg_m3, "kg/m³"),
            MassDensityUnit::PoundsPerCubicFoot => (kg_m3 * 0.062_427_960_6, "lb/ft³"),
        }
    }
}

/// The units time is shown in: seconds, minutes or hours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeUnit {
    #[default]
    Seconds,
    Minutes,
    Hours,
}

impl TimeUnit {
    pub const ALL: [TimeUnit; 3] = [TimeUnit::Seconds, TimeUnit::Minutes, TimeUnit::Hours];

    pub fn name(self) -> &'static str {
        match self {
            TimeUnit::Seconds => "s",
            TimeUnit::Minutes => "min",
            TimeUnit::Hours => "h",
        }
    }

    pub fn from_name(name: &str) -> Option<TimeUnit> {
        TimeUnit::ALL.into_iter().find(|u| u.name().eq_ignore_ascii_case(name.trim()))
    }

    pub fn label(self) -> &'static str {
        match self {
            TimeUnit::Seconds => ui_msg!("Seconds"),
            TimeUnit::Minutes => ui_msg!("Minutes"),
            TimeUnit::Hours => ui_msg!("Hours"),
        }
    }

    pub fn convert(self, seconds: f64) -> (f64, &'static str) {
        match self {
            TimeUnit::Seconds => (seconds, "s"),
            TimeUnit::Minutes => (seconds / 60.0, "min"),
            TimeUnit::Hours => (seconds / 3600.0, "h"),
        }
    }
}

/// The units speed is shown in: metres a second, kilometres an hour, feet a second or miles an hour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpeedUnit {
    #[default]
    MetresPerSecond,
    KilometresPerHour,
    FeetPerSecond,
    MilesPerHour,
}

impl SpeedUnit {
    pub const ALL: [SpeedUnit; 4] =
        [SpeedUnit::MetresPerSecond, SpeedUnit::KilometresPerHour, SpeedUnit::FeetPerSecond, SpeedUnit::MilesPerHour];

    pub fn name(self) -> &'static str {
        match self {
            SpeedUnit::MetresPerSecond => "m-s",
            SpeedUnit::KilometresPerHour => "km-h",
            SpeedUnit::FeetPerSecond => "ft-s",
            SpeedUnit::MilesPerHour => "mph",
        }
    }

    pub fn from_name(name: &str) -> Option<SpeedUnit> {
        SpeedUnit::ALL.into_iter().find(|u| u.name().eq_ignore_ascii_case(name.trim()))
    }

    pub fn label(self) -> &'static str {
        match self {
            SpeedUnit::MetresPerSecond => ui_msg!("Metres per second"),
            SpeedUnit::KilometresPerHour => ui_msg!("Kilometres per hour"),
            SpeedUnit::FeetPerSecond => ui_msg!("Feet per second"),
            SpeedUnit::MilesPerHour => ui_msg!("Miles per hour"),
        }
    }

    pub fn convert(self, m_s: f64) -> (f64, &'static str) {
        match self {
            SpeedUnit::MetresPerSecond => (m_s, "m/s"),
            SpeedUnit::KilometresPerHour => (m_s * 3.6, "km/h"),
            SpeedUnit::FeetPerSecond => (m_s / FOOT * 1000.0, "ft/s"),
            SpeedUnit::MilesPerHour => (m_s * 3.6 / 1.609_344, "mph"),
        }
    }
}

/// A kind of value a project shows, as Project Units names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quantity {
    Length,
    Area,
    Volume,
    Angle,
    Slope,
    MassDensity,
    Time,
    Speed,
    Currency,
}

impl Quantity {
    pub const ALL: [Quantity; 9] = [
        Quantity::Length,
        Quantity::Area,
        Quantity::Volume,
        Quantity::Angle,
        Quantity::Slope,
        Quantity::MassDensity,
        Quantity::Time,
        Quantity::Speed,
        Quantity::Currency,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Quantity::Length => "length",
            Quantity::Area => "area",
            Quantity::Volume => "volume",
            Quantity::Angle => "angle",
            Quantity::Slope => "slope",
            Quantity::MassDensity => "mass_density",
            Quantity::Time => "time",
            Quantity::Speed => "speed",
            Quantity::Currency => "currency",
        }
    }

    pub fn from_name(name: &str) -> Option<Quantity> {
        Quantity::ALL.into_iter().find(|q| q.name().eq_ignore_ascii_case(name.trim()))
    }

    /// The places after the point its number shows unless its format says otherwise.
    pub fn default_decimals(self) -> u8 {
        match self {
            Quantity::Length => 2,
            Quantity::Area => 2,
            Quantity::Volume => 3,
            Quantity::Angle => 0,
            Quantity::Slope => 2,
            Quantity::MassDensity => 1,
            Quantity::Time => 1,
            Quantity::Speed => 2,
            Quantity::Currency => 2,
        }
    }
}

/// How one kind of value is written: its unit symbol and what may be left out or added around the number.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FormatOptions {
    /// Whether its unit is shown beside the number; none, the user's Show units in values decides.
    pub show_units: Option<bool>,
    /// The sign of positives; none, the project's signs, or none.
    pub positive: Option<PositiveSign>,
    /// The sign of zero; none, the project's signs, or none.
    pub zero: Option<ZeroSign>,
    pub suppress_trailing_zeros: bool,
    pub suppress_zero_feet: bool,
    pub digit_grouping: bool,
    pub suppress_spaces: bool,
    /// How many places before the point its number rounds to: 1 to 10, 2 to 100, 3 to 1000; none, not.
    pub rounding: Option<u8>,
    /// How many places after the point its number shows; none, the quantity's own.
    pub decimals: Option<u8>,
    /// A symbol of its own in place of the unit's, such as `m` for a metre; none, the unit's.
    pub symbol: Option<String>,
}

impl FormatOptions {
    /// `value` rounded to `rounding` places before the point: 1 to 10, 2 to 100, 3 to 1000.
    pub fn round_value(&self, value: f64) -> f64 {
        match self.rounding {
            Some(digits) => {
                let step = 10f64.powi(i32::from(digits));
                let rounded = (value / step).round() * step;
                if rounded.is_finite() { rounded + 0.0 } else { value }
            }
            None => value,
        }
    }

    /// `text` with its trailing zeros after the point taken away, when asked: `1.230` is `1.23`, `2.000` is `2`.
    pub fn without_trailing_zeros(&self, text: String) -> String {
        if !self.suppress_trailing_zeros || !text.contains('.') {
            return text;
        }
        let end = text.trim_end_matches('0').trim_end_matches('.').to_string();
        if end.is_empty() { "0".into() } else { end }
    }

    /// `text` with its integer digits grouped in threes by commas, when asked: `1234567.5` is `1,234,567.5`.
    pub fn grouped(&self, text: String) -> String {
        if !self.digit_grouping {
            return text;
        }
        let (sign, rest) = match text.strip_prefix('-') {
            Some(rest) => ("-", rest),
            None => ("", text.as_str()),
        };
        let (whole, point) = rest.split_once('.').map_or((rest, ""), |(w, p)| (w, p));
        let mut grouped = String::new();
        for (i, c) in whole.chars().enumerate() {
            if i > 0 && (whole.len() - i) % 3 == 0 {
                grouped.push(',');
            }
            grouped.push(c);
        }
        if point.is_empty() { format!("{sign}{grouped}") } else { format!("{sign}{grouped}.{point}") }
    }

    /// `number` and `unit` as this format writes them: no space when asked, the symbol of its own, or the unit's.
    pub fn with_unit(&self, number: String, unit: &str) -> String {
        let unit = self.symbol.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or(unit);
        let tight = unit.starts_with('°') || unit.starts_with('%') || unit.contains('\'') || unit.contains('"');
        if self.suppress_spaces || tight { format!("{number}{unit}") } else { format!("{number} {unit}") }
    }
}

/// How numbers write their decimal point and group thousands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecimalStyle {
    /// `123,456,789.00`
    #[default]
    #[serde(rename = "dot-comma")]
    DotComma,
    /// `123.456.789,00`
    #[serde(rename = "comma-dot")]
    CommaDot,
    /// `123 456 789.00`
    #[serde(rename = "space-dot")]
    SpaceDot,
    /// `123456789.00`
    #[serde(rename = "none-dot")]
    NoneDot,
}

impl DecimalStyle {
    pub const ALL: [DecimalStyle; 4] = [DecimalStyle::DotComma, DecimalStyle::CommaDot, DecimalStyle::SpaceDot, DecimalStyle::NoneDot];

    pub fn name(self) -> &'static str {
        match self {
            DecimalStyle::DotComma => "dot-comma",
            DecimalStyle::CommaDot => "comma-dot",
            DecimalStyle::SpaceDot => "space-dot",
            DecimalStyle::NoneDot => "none-dot",
        }
    }

    pub fn from_name(name: &str) -> Option<DecimalStyle> {
        DecimalStyle::ALL.into_iter().find(|s| s.name().eq_ignore_ascii_case(name.trim()))
    }

    /// The sample the list writes it as: `123,456,789.00`.
    pub fn label(self) -> &'static str {
        match self {
            DecimalStyle::DotComma => "123,456,789.00",
            DecimalStyle::CommaDot => "123.456.789,00",
            DecimalStyle::SpaceDot => "123 456 789.00",
            DecimalStyle::NoneDot => "123456789.00",
        }
    }

    pub fn decimal(self) -> char {
        match self {
            DecimalStyle::CommaDot => ',',
            _ => '.',
        }
    }

    pub fn group(self) -> Option<char> {
        match self {
            DecimalStyle::DotComma => Some(','),
            DecimalStyle::CommaDot => Some('.'),
            DecimalStyle::SpaceDot => Some(' '),
            DecimalStyle::NoneDot => None,
        }
    }

    /// `number`, which writes its decimal point as `.`, with this style's point and, when `group`, grouped thousands.
    pub fn apply(self, number: String, group: bool) -> String {
        let (whole, point) = number.split_once('.').map_or((number.as_str(), ""), |(w, p)| (w, p));
        let (sign, whole) = match whole.strip_prefix('-') {
            Some(rest) => ("-", rest),
            None => ("", whole),
        };
        let grouped = match self.group() {
            Some(group_char) if group && !whole.is_empty() => {
                let mut out = String::new();
                for (i, c) in whole.chars().enumerate() {
                    if i > 0 && (whole.len() - i) % 3 == 0 {
                        out.push(group_char);
                    }
                    out.push(c);
                }
                out
            }
            _ => whole.to_string(),
        };
        if point.is_empty() { format!("{sign}{grouped}") } else { format!("{sign}{grouped}{}{point}", self.decimal()) }
    }
}

/// A format with nothing set, for a quantity that has none.
const EMPTY_FORMAT: FormatOptions = FormatOptions {
    show_units: None,
    positive: None,
    zero: None,
    suppress_trailing_zeros: false,
    suppress_zero_feet: false,
    digit_grouping: false,
    suppress_spaces: false,
    rounding: None,
    decimals: None,
    symbol: None,
};

/// The units each kind of value is shown in, apart from a length's; none, the one its project's length units imply.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DisplayUnits {
    pub area: Option<AreaUnit>,
    pub volume: Option<VolumeUnit>,
    pub slope: Option<SlopeUnit>,
    pub mass_density: Option<MassDensityUnit>,
    pub time: Option<TimeUnit>,
    pub speed: Option<SpeedUnit>,
    /// The symbol money is shown in, such as `$` or `€`; none, `$`.
    pub currency: Option<String>,
    /// How numbers write their decimal point and grouped thousands; none, `123,456,789.00`.
    pub decimal_style: DecimalStyle,
    /// Each kind's format options, by [`Quantity::name`].
    pub formats: BTreeMap<String, FormatOptions>,
}

impl DisplayUnits {
    pub fn is_default(&self) -> bool {
        self == &DisplayUnits::default()
    }

    /// The format options of `quantity`; none, everything default.
    pub fn options(&self, quantity: Quantity) -> &FormatOptions {
        self.formats.get(quantity.name()).unwrap_or(&EMPTY_FORMAT)
    }

    /// The format options of `quantity`, made when it has none.
    pub fn options_mut(&mut self, quantity: Quantity) -> &mut FormatOptions {
        self.formats.entry(quantity.name().to_string()).or_default()
    }

    /// An area in mm² as the project shows it: its own unit, else m² or ft² by the project's length units.
    pub fn area(&self, length: Units, mm2: f64) -> (f64, &'static str) {
        self.area.unwrap_or(if length.imperial() { AreaUnit::SquareFeet } else { AreaUnit::SquareMetres }).convert(mm2)
    }

    /// A volume in mm³ as the project shows it: its own unit, else m³ or ft³ by the project's length units.
    pub fn volume(&self, length: Units, mm3: f64) -> (f64, &'static str) {
        self.volume.unwrap_or(if length.imperial() { VolumeUnit::CubicFeet } else { VolumeUnit::CubicMetres }).convert(mm3)
    }

    pub fn angle(&self, degrees: f64) -> (f64, &'static str) {
        (degrees, "°")
    }

    pub fn slope(&self, degrees: f64) -> (f64, &'static str) {
        self.slope.unwrap_or_default().convert(degrees)
    }

    pub fn mass_density(&self, kg_m3: f64) -> (f64, &'static str) {
        self.mass_density.unwrap_or_default().convert(kg_m3)
    }

    pub fn time(&self, seconds: f64) -> (f64, &'static str) {
        self.time.unwrap_or_default().convert(seconds)
    }

    pub fn speed(&self, m_s: f64) -> (f64, &'static str) {
        self.speed.unwrap_or_default().convert(m_s)
    }

    pub fn currency(&self) -> &str {
        self.currency.as_deref().filter(|c| !c.trim().is_empty()).unwrap_or("$")
    }
}

const METRIC_SCALES: [f64; 13] = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 75.0, 100.0, 150.0, 200.0, 250.0, 500.0, 1000.0];
/// The scales a project in inches or feet offers: 12" to 1/32" to the foot, then 1" to 10' to 1" to 200'.
const IMPERIAL_SCALES: [f64; 22] = [
    1.0, 2.0, 4.0, 8.0, 12.0, 16.0, 24.0, 32.0, 48.0, 64.0, 96.0, 128.0, 192.0, 256.0, 384.0, 120.0, 240.0, 360.0, 480.0, 600.0, 1200.0,
    2400.0,
];

/// `n` sixty-fourths of an inch as a drawing writes inches: `1/8`, `1 1/2`, `12`.
fn fraction(n: u32) -> String {
    let (whole, mut num, mut den) = (n / 64, n % 64, 64);
    while num > 0 && num % 2 == 0 {
        (num, den) = (num / 2, den / 2);
    }
    match (whole, num) {
        (w, 0) => w.to_string(),
        (0, _) => format!("{num}/{den}"),
        (w, _) => format!("{w} {num}/{den}"),
    }
}

fn number(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|x| x.is_finite())
}

/// Inches typed: 6, 6.5, 6 1/2, or 1/2.
fn inches(s: &str) -> Option<f64> {
    let fraction = |f: &str| {
        let (a, b) = f.split_once('/')?;
        let (a, b) = (number(a)?, number(b)?);
        (b != 0.0).then(|| a / b)
    };
    match s.split_whitespace().collect::<Vec<_>>().as_slice() {
        [whole, part] => Some(number(whole)? + fraction(part)?),
        [one] if one.contains('/') => fraction(one),
        [one] => number(one),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_are_named_and_read_as_the_units_draw_them() {
        let (m, ft) = (Units::Millimetres, Units::FeetInches);
        assert_eq!(m.scale(100.0), "1 : 100");
        assert_eq!(m.scale(96.0), "1 : 96", "metric names every scale 1 : n");
        for (n, name) in [
            (1.0, "12\" = 1'-0\""),
            (8.0, "1 1/2\" = 1'-0\""),
            (12.0, "1\" = 1'-0\""),
            (16.0, "3/4\" = 1'-0\""),
            (48.0, "1/4\" = 1'-0\""),
            (64.0, "3/16\" = 1'-0\""),
            (96.0, "1/8\" = 1'-0\""),
            (128.0, "3/32\" = 1'-0\""),
            (384.0, "1/32\" = 1'-0\""),
            (120.0, "1\" = 10'-0\""),
            (240.0, "1\" = 20'-0\""),
            (1200.0, "1\" = 100'-0\""),
            (100.0, "1 : 100"),
            (50.0, "1 : 50"),
        ] {
            assert_eq!(ft.scale(n), name, "1 : {n}");
            assert_eq!(ft.parse_scale(name), Some(n), "{name}");
        }
        assert!(Units::Inches.scales().iter().all(|n| !Units::Inches.scale(*n).starts_with("1 :")), "every one offered is named");
        assert_eq!(Units::Metres.scales(), &METRIC_SCALES);
        // The scales offices draw at most, 1 : 75, 1 : 150 and 1 : 250 with them, from finest to coarsest.
        assert!([75.0, 150.0, 250.0].iter().all(|n| Units::Metres.scales().contains(n)));
        assert!(Units::Metres.scales().windows(2).all(|w| w[0] < w[1]));
        assert_eq!((m.default_scale(), ft.default_scale()), (100.0, 96.0));
        // Typed: n, 1 : n, or the paper's length to the model's.
        assert_eq!(m.parse_scale("75"), Some(75.0));
        assert_eq!(m.parse_scale(" 1 : 250 "), Some(250.0));
        assert_eq!(m.parse_scale("1 mm = 2 m"), Some(2000.0));
        assert_eq!(ft.parse_scale("1/4\" = 1'"), Some(48.0));
        assert_eq!(ft.parse_scale("1\" = 30'"), Some(360.0));
        for bad in ["", "big", "2 : 100", "0", "-5", "1/8\" = 0'", "1 : 0"] {
            assert_eq!(m.parse_scale(bad), None, "{bad}");
        }
    }

    #[test]
    fn lengths_show_in_the_project_units() {
        let shown = |u: Units, mm: f64| u.length(mm);
        assert_eq!(shown(Units::Millimetres, 3000.4), "3000");
        assert_eq!(shown(Units::Centimetres, 3005.0), "300.5");
        assert_eq!(shown(Units::Metres, 2950.0), "2.95");
        assert_eq!(shown(Units::Inches, 3000.0), "118 1/8\"");
        assert_eq!(shown(Units::Inches, 12.7), "1/2\"");
        assert_eq!(shown(Units::Feet, 3000.0), "9.84'");
        assert_eq!(shown(Units::FeetInches, 3000.0), "9'-10 1/8\"");
        assert_eq!(shown(Units::FeetInches, 304.8), "1'-0\"");
        assert_eq!(shown(Units::FeetInches, -457.2), "-1'-6\"");
        assert_eq!(shown(Units::FeetInches, 0.1), "0'-0\"", "no minus on what rounds to nothing");
        assert_eq!(Units::Metres.with_unit(3000.0), "3.00 m");
        assert_eq!(Units::FeetInches.with_unit(3000.0), "9'-10 1/8\"");
        let (ft2, unit) = Units::FeetInches.area(18e6);
        assert_eq!((format!("{ft2:.2}"), unit), ("193.75".to_string(), "ft²"));
    }

    #[test]
    fn a_field_is_filled_with_what_reads_back_as_the_value() {
        // What the units round away is said in mm, so Apply on a field left alone changes nothing.
        assert_eq!(Units::Metres.field(3.0), "3 mm");
        assert_eq!(Units::Metres.field(55.0), "55 mm");
        assert_eq!(Units::Metres.field(-12.5), "-12.5 mm");
        assert_eq!(Units::Centimetres.field(3005.4), "3005.4 mm");
        assert_eq!(Units::Millimetres.field(3.5), "3.5 mm");
        // What they show exactly is shown as drawings show it.
        assert_eq!(Units::Metres.field(2950.0), "2.95");
        assert_eq!(Units::Metres.field(0.0), "0.00");
        assert_eq!(Units::Centimetres.field(3005.0), "300.5");
        assert_eq!(Units::Millimetres.field(3000.0), "3000");
        // Inches and feet round by design, to a sixteenth of an inch and a hundredth of a foot.
        assert_eq!(Units::FeetInches.field(3000.0), "9'-10 1/8\"");
        assert_eq!(Units::Feet.field(3000.0), "9.84'");
        for u in [Units::Millimetres, Units::Centimetres, Units::Metres] {
            for mm in [0.0, 3.0, 55.0, 2950.0, 3213.1, -12.5, 0.0004] {
                assert_eq!(u.parse(&u.field(mm)), Some((mm * 1e3).round() / 1e3 + 0.0), "{u:?} {mm}");
            }
        }
    }

    #[test]
    fn lengths_show_to_the_decimals_a_project_asks_for() {
        assert_eq!(Units::Metres.decimals(), Some(0..=5));
        assert_eq!(Units::Centimetres.decimals(), Some(0..=4));
        assert_eq!(Units::Millimetres.decimals(), Some(0..=3));
        assert_eq!(Units::Feet.decimals(), Some(0..=5));
        assert_eq!((Units::Inches.decimals(), Units::FeetInches.decimals()), (Some(0..=4), Some(0..=4)));
        // None, and the fewest places, are what drawings show; inches and feet-inches keep their sixteenths.
        for u in Units::ALL {
            assert_eq!(u.length_to(2955.5, None), u.length(2955.5), "{u:?}");
            assert_eq!(u.length_to(2955.5, u.fewest()), u.length(2955.5), "{u:?}");
        }
        for u in [Units::Millimetres, Units::Centimetres, Units::Metres, Units::Feet] {
            assert_eq!(u.length_to(2955.5, Some(3)) == u.length(2955.5), u.decimals().is_none(), "{u:?}");
        }
        // A count asked for is shown exactly, trailing zeros too: a centimetre's whole and tenth are 100 and 100.0.
        assert_eq!(Units::Centimetres.length_to(3173.0, Some(0)), "317");
        assert_eq!(Units::Centimetres.length_to(3173.0, Some(1)), "317.3");
        assert_eq!(Units::Centimetres.length_to(1000.0, Some(1)), "100.0");
        assert_eq!(Units::Centimetres.length_to(1000.0, Some(2)), "100.00");
        assert_eq!(Units::Centimetres.length_to(1000.0, None), "100");
        assert_eq!(Units::Metres.length_to(3.0, Some(3)), "0.003");
        assert_eq!(Units::Metres.length_to(55.0, Some(3)), "0.055");
        assert_eq!(Units::Metres.length_to(2950.0, Some(5)), "2.95000");
        assert_eq!(Units::Centimetres.length_to(3005.4, Some(2)), "300.54");
        assert_eq!(Units::Millimetres.length_to(3.5, Some(1)), "3.5");
        assert_eq!(Units::Feet.length_to(3000.0, Some(4)), "9.8425'");
        assert_eq!(Units::Metres.with_unit_to(3.0, Some(3)), "0.003 m");
        assert_eq!(Units::FeetInches.with_unit_to(3000.0, Some(3)), "9'-10 1/8\"");
        // More than the most is the most, and fewer than the fewest the fewest.
        assert_eq!(Units::Metres.length_to(3.0, Some(9)), "0.00300");
        assert_eq!(Units::Metres.length_to(2955.0, Some(0)), "3");
        // A field says what it holds to those places, and in mm when they would not.
        assert_eq!(Units::Metres.field_to(3.0, Some(3)), "0.003");
        assert_eq!(Units::Metres.field_to(3.4, Some(3)), "3.4 mm");
        assert_eq!(Units::Metres.field_to(3.0, None), "3 mm");
    }

    #[test]
    fn typed_lengths_read_in_the_project_units_or_their_own() {
        let read = |u: Units, s: &str| u.parse(s);
        assert_eq!(read(Units::Millimetres, "3000"), Some(3000.0));
        assert_eq!(read(Units::Millimetres, " 3 m"), Some(3000.0));
        assert_eq!(read(Units::Metres, "3.2"), Some(3200.0));
        assert_eq!(read(Units::Metres, "250mm"), Some(250.0));
        assert_eq!(read(Units::Centimetres, "12.5"), Some(125.0));
        assert_eq!(read(Units::FeetInches, "10' 6\""), Some(3200.4));
        assert_eq!(read(Units::FeetInches, "10'-6 1/2\""), Some(3213.1));
        assert_eq!(read(Units::FeetInches, "10"), Some(3048.0), "a number alone is feet");
        assert_eq!(read(Units::FeetInches, "-1'-6\""), Some(-457.2));
        assert_eq!(read(Units::FeetInches, "2.5ft"), Some(762.0));
        assert_eq!(read(Units::Millimetres, "6\""), Some(152.4));
        assert_eq!(read(Units::Inches, "6 1/2"), Some(165.1));
        assert_eq!(read(Units::Inches, "30in"), Some(762.0));
        for bad in ["", "x", "1/0\"", "3 m m", "1e999"] {
            assert_eq!(read(Units::FeetInches, bad), None, "{bad}");
        }
        // What's shown reads back as itself.
        for u in Units::ALL {
            let shown = u.length(3213.1);
            assert!((u.parse(&shown).unwrap() - 3213.1).abs() <= 5.0, "{u:?} {shown}");
        }
    }
}

#[cfg(test)]
mod more_quantity_tests {
    use super::*;

    #[test]
    fn angle_weight_and_ratio_name_their_units() {
        assert_eq!(Units::Metres.angle(30.0), (30.0, "°"));
        assert_eq!(Units::Millimetres.weight(2.5), (2.5, "kg"));
        assert_eq!(Units::Millimetres.ratio(2400.0), (2400.0, "kg/m³"));
        assert_eq!(Units::FeetInches.weight(0.453_592_37).1, "lb");
        assert_eq!(Units::FeetInches.ratio(1.0).1, "lb/ft³");
        assert_eq!(Units::Metres.length_with_unit(3000.0), "3.00 m");
    }
}
