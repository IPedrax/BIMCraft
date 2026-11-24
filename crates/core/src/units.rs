//! A project's units: how lengths and areas are shown, and how typed lengths read. The model keeps
//! millimetres, and so does every command; units change only what drawings, schedules and the
//! desktop show. Feet and inches round to the nearest sixteenth of an inch.

use bimcraft_i18n::ui_msg;

use serde::{Deserialize, Serialize};

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
            Units::Inches | Units::FeetInches => {
                // In sixteenths of an inch.
                let n = (mm.abs() / INCH * 16.0).round();
                let sign = if mm < 0.0 && n > 0.0 { "-" } else { "" };
                let (feet, rest) = if self == Units::FeetInches { ((n / 192.0).floor(), n % 192.0) } else { (0.0, n) };
                let (whole, part) = ((rest / 16.0).floor(), rest % 16.0);
                // The sixteenths in lowest terms.
                let (mut top, mut bottom) = (part as u32, 16);
                while top > 0 && top % 2 == 0 {
                    (top, bottom) = (top / 2, bottom / 2);
                }
                let inches = match (whole, top) {
                    (w, 0) => format!("{w}\""),
                    (0.0, t) if self == Units::Inches => format!("{t}/{bottom}\""),
                    (w, t) => format!("{w} {t}/{bottom}\""),
                };
                if self == Units::FeetInches { format!("{sign}{feet}'-{inches}") } else { format!("{sign}{inches}") }
            }
        }
    }

    /// The places after the point a project may show lengths to in these units: from the fewest, which drawings use (metres
    /// and feet 2, centimetres 1, millimetres 0), to three more. Inches and feet-inches show sixteenths, so none.
    pub fn decimals(self) -> Option<std::ops::RangeInclusive<u8>> {
        let fewest = match self {
            Units::Millimetres => 0,
            Units::Centimetres => 1,
            Units::Metres | Units::Feet => 2,
            Units::Inches | Units::FeetInches => return None,
        };
        Some(fewest..=fewest + 3)
    }

    /// [`Units::length`] to `decimals` places after the point (a project's, [`Units::decimals`]; none, the fewest), the places
    /// of a file held to the range.
    pub fn length_to(self, mm: f64, decimals: Option<u8>) -> String {
        let Some(range) = self.decimals() else { return self.length(mm) };
        match decimals.map(|d| d.clamp(*range.start(), *range.end())) {
            Some(d) if d != *range.start() => crate::dims::rounded(self, Some(d), mm),
            _ => self.length(mm),
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

    /// A length with its unit, as the desktop labels one: 3000 mm, 3.00 m, 9'-10 1/8".
    pub fn with_unit(self, mm: f64) -> String {
        self.with_unit_to(mm, None)
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
        let t = text.trim().to_ascii_lowercase();
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

/// The scales a metric project offers, 1 : n.
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
        assert_eq!(Units::Metres.decimals(), Some(2..=5));
        assert_eq!(Units::Centimetres.decimals(), Some(1..=4));
        assert_eq!(Units::Millimetres.decimals(), Some(0..=3));
        assert_eq!(Units::Feet.decimals(), Some(2..=5));
        assert_eq!((Units::Inches.decimals(), Units::FeetInches.decimals()), (None, None));
        // None, and the fewest places, are what drawings show; inches and feet-inches keep their sixteenths.
        for u in Units::ALL {
            assert_eq!(u.length_to(2955.5, None), u.length(2955.5), "{u:?}");
            assert_eq!(u.length_to(2955.5, u.decimals().map(|r| *r.start())), u.length(2955.5), "{u:?}");
            assert_eq!(u.length_to(2955.5, Some(3)) == u.length(2955.5), u.decimals().is_none(), "{u:?}");
        }
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
        assert_eq!(Units::Metres.length_to(2955.0, Some(0)), Units::Metres.length(2955.0));
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
