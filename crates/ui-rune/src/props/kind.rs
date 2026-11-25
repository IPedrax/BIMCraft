//! What a palette field holds, and how a value is shown and read back: the editor kinds of egui's palette (`properties.rs`, `Kind`),
//! with lengths and areas in the project's units (`Units::with_unit`, `Units::area`, `Units::parse`) and every number and size checked,
//! since an element's JSON and what is typed are hostile. Files and commands stay in millimetres.

use bimcraft_core::model::{MAX_COORD, MAX_POINTS};
use bimcraft_core::units::Units;
use serde_json::{Value, json};

use crate::lang::tf;

/// The longest text one row shows, in characters.
pub const MAX_VALUE: usize = 200;
/// The longest text a field takes, in characters.
pub const MAX_TYPED: usize = 10_000;
/// The most items of a list (a point's two numbers, a dimension's values) one row shows.
const MAX_ITEMS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Text,
    /// A number with no unit: an angle, a scale, a size on paper.
    Number,
    /// A length or an elevation, in the project's units.
    Length,
    /// `[x, y]`, lengths in the project's units, typed as `x, y`.
    Point,
    /// `[x, y]` on paper, in mm.
    PaperPoint,
    /// A mass's top outline: corners as `x, y`, parted by `;`, or blank for none.
    Outline,
    Flag,
    Level,
    /// A level, shown by name; read only.
    LevelName,
    /// A view, shown by name; read only.
    ViewName,
    /// A level, or none for a wall of unconnected height.
    TopLevel,
    /// A room's Upper Limit: a level, or Not Bounded (none), its Limit Offset then its height over its own level.
    UpperLimit,
    /// A wall's height: typed when its top is unconnected, else what its levels make it.
    Height,
    /// A length, or nothing, which its hint names when blank (a plan's top at the level above, a view with no far clip).
    Limit(&'static str),
    /// Whether a view is cropped: on crops it to what it shows (`crop`), off uncrops it.
    Crop,
    /// A plan's view range: Edit… opens the View Range dialog.
    Range,
    /// Derived from the rest; read only.
    Shown,
    /// A length (or several), read only.
    LengthShown,
    /// A length some elements have and others don't (an extrusion roof's start and end): typed, and no row without one.
    LengthIf,
    /// An area in m², read only: shown in m² or ft².
    AreaShown,
    /// A family's area in mm², read only: shown in m² or ft².
    Mm2Shown,
    /// A volume in m³, read only: shown in m³ or ft³.
    VolumeShown,
    /// An angle in degrees, typed as a number, shown with °.
    Angle,
    /// A length in m, read only.
    MetresShown,
    /// A dimension's values: lengths, or an angle's degrees.
    Values,
    /// A wall's location line: a drop-down.
    LocationLine,
    /// A detail line's line style: a drop-down of the project's.
    LineStyle,
    /// A filled region's boundary: a line style, or invisible lines (none).
    RegionLines,
    /// Which line of a run's section its heights are: a drop-down.
    Justification,
    /// Whether a roof or a floor is there (a wall's attachment), and a link that selects it; read only.
    Attached,
    /// An element it points at (a door's host wall): a link that selects it; read only.
    Link,
    /// One of these values (as the command names it, and its label): a drop-down, the first when it has none.
    Choice(&'static [(&'static str, &'static str)]),
    /// A wall's, a floor's or a roof's framing type: None or one of the project's, a drop-down, and beside it its Type Properties.
    Framing,
    /// A number, or nothing, which its hint names when blank (a floor's joists' direction, automatic): typed, or cleared.
    NumberOr(&'static str),
    /// A view's scale, the n of 1 : n, typed as `100`, `1 : 100` or two lengths (`Units::parse_scale`).
    Scale,
}

use Kind::*;

/// Field `key` of element `e`'s JSON, through the dots, as `kind` reads it. The JSON leaves a number out at 0 (a floor's offset), so a
/// length or a number it hasn't got is 0; a `null` it has is none (a section's far clip, which sees all there is).
pub fn field_of(e: &Value, key: &str, kind: Kind) -> Value {
    match key.split('.').try_fold(e, |v, k| v.get(k)) {
        Some(v) => v.clone(),
        None if matches!(kind, Length | Number | Angle) => json!(0.0),
        None => Value::Null,
    }
}

/// Whether parameter `key` means anything for the one element `e` (as its JSON has it): a detail line's Center Mark Visible is for an
/// arc or a circle, its Revision for a cloud; on any other line they would only be switches that do nothing.
pub fn relevant(e: &Value, key: &str) -> bool {
    match (e["shape"].as_str(), key) {
        (Some(shape), "centre_mark") => matches!(shape, "arc" | "circle"),
        (Some(_), "revision") => !e["cloud"].is_null(),
        // A wall's Top Offset is from its top constraint: an unconnected wall stands `height` tall and has none.
        (_, "top_offset") => !e["top_level"].is_null(),
        // Opening Area is for a family that cuts its host, or a door's or a window's rectangle.
        (_, "values.Opening Area") => !e["values"]["Opening Area"].is_null(),
        // An extrusion roof takes its pitch from its profile, not the footprint's slope.
        (_, "slope") => e["extrusion"].is_null(),
        // A floor's joists run some way only once it's framed.
        (_, "framing_angle") => !e["framing"].is_null(),
        _ => true,
    }
}

/// `{key: val}` for `modify`, nested through the dots: modify merges into fields that are objects.
pub fn nested(key: &str, val: Value) -> Value {
    key.rsplit('.').fold(val, |v, k| json!({ k: v }))
}

/// `text` held to `most` characters, an ellipsis at the cut.
pub fn cut(text: String, most: usize) -> String {
    if text.chars().count() <= most {
        return text;
    }
    let mut short: String = text.chars().take(most.saturating_sub(1)).collect();
    short.push('…');
    short
}

/// A number to the hundredth, whole numbers whole; nothing for what is not a number.
fn num(x: f64) -> String {
    let r = (x * 100.0).round() / 100.0 + 0.0;
    if r.is_finite() {
        format!("{r}")
    } else if x.is_finite() {
        format!("{x}")
    } else {
        String::new()
    }
}

/// A value as the palette shows it, numbers to the hundredth: a string as it is, a list joined, yes or no.
pub fn text(v: &Value) -> String {
    let said = match v {
        Value::Number(n) => n.as_f64().map_or_else(String::new, num),
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().take(MAX_ITEMS).map(text).collect::<Vec<_>>().join(", "),
        Value::Bool(b) => (if *b { "Yes" } else { "No" }).into(),
        Value::Null => String::new(),
        Value::Object(fields) => format!("{{{} fields}}", fields.len()),
    };
    cut(said, MAX_VALUE)
}

/// What was typed as a number: a decimal comma is a point, and what is not finite is none.
pub fn number(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").parse::<f64>().ok().filter(|x| x.is_finite())
}

/// `v`, a number or a list of numbers, each as `f` shows it, joined by commas.
fn each(v: &Value, f: &dyn Fn(f64) -> String) -> String {
    match v {
        Value::Array(a) => {
            a.iter().take(MAX_ITEMS).filter_map(Value::as_f64).filter(|x| x.is_finite()).map(f).collect::<Vec<_>>().join(", ")
        }
        v => v.as_f64().filter(|x| x.is_finite()).map_or_else(String::new, f),
    }
}

/// A value of `kind` as the palette shows it, lengths and areas in `units`.
pub fn shown(kind: Kind, v: &Value, units: Units) -> String {
    match kind {
        Outline => match v {
            Value::Array(corners) => {
                let corner = |p: &Value| {
                    let (x, y) = (p.get(0)?.as_f64()?, p.get(1)?.as_f64()?);
                    (x.is_finite() && y.is_finite()).then(|| format!("{}, {}", units.length(x), units.length(y)))
                };
                corners.iter().take(MAX_POINTS).filter_map(corner).collect::<Vec<_>>().join("; ")
            }
            _ => String::new(),
        },
        Length | LengthIf | Height | Limit(_) | LengthShown | Point => each(v, &|x| units.with_unit(x)),
        MetresShown => each(v, &|x| units.with_unit(x * 1000.0)),
        AreaShown => each(v, &|x| {
            let (area, unit) = units.area(x * 1e6);
            format!("{area:.2} {unit}")
        }),
        Mm2Shown => each(v, &|x| {
            let (area, unit) = units.area(x);
            format!("{area:.2} {unit}")
        }),
        VolumeShown => each(v, &|x| {
            let (volume, unit) = units.volume(x * 1e9);
            format!("{volume:.3} {unit}")
        }),
        Angle => each(v, &|x| format!("{x}°")),
        Values => each(v, &|x| units.length(x)),
        PaperPoint => each(v, &num),
        _ => text(v),
    }
}

/// A length typed in `units`, in mm: finite, and no farther than the model reaches.
fn length(typed: &str, units: Units) -> Result<f64, String> {
    match units.parse(typed) {
        Some(x) if x.abs() <= MAX_COORD => Ok(x),
        Some(_) => Err(tf("`{}` is out of range", &[typed.trim()])),
        None => Err(tf("`{}` is not a length", &[typed.trim()])),
    }
}

/// What was typed into a field of `kind`, as the value `modify` takes: lengths in `units`. Or why it is none.
pub fn parse(kind: Kind, s: &str, units: Units) -> Result<Value, String> {
    if s.chars().count() > MAX_TYPED {
        return Err(tf("`{}` is too long", &["…"]));
    }
    let blank = s.trim().is_empty();
    match kind {
        Number => number(s).map(|x| json!(x)).ok_or_else(|| tf("`{}` is not a number", &[s.trim()])),
        Angle => number(s.trim().trim_end_matches('°')).map(|x| json!(x)).ok_or_else(|| tf("`{}` is not an angle", &[s.trim()])),
        Length | LengthIf | Height => length(s, units).map(|x| json!(x)),
        Limit(_) if blank => Ok(Value::Null),
        Limit(_) => length(s, units).map(|x| json!(x)),
        NumberOr(_) if blank => Ok(Value::Null),
        NumberOr(_) => parse(Number, s, units),
        Outline if blank => Ok(Value::Null),
        Outline => {
            let corners: Vec<&str> = s.split(';').collect();
            if corners.len() > MAX_POINTS {
                return Err(tf("`{}` is too long", &["…"]));
            }
            corners.into_iter().map(|corner| parse(Point, corner, units)).collect::<Result<Vec<_>, _>>().map(Value::Array)
        }
        // Lengths with feet and inches hold spaces: a comma parts x from y.
        Point => match s.split(',').collect::<Vec<_>>()[..] {
            [x, y] => Ok(json!([length(x, units)?, length(y, units)?])),
            _ => Err(tf("`{}` is not a point; type x, y", &[s.trim()])),
        },
        PaperPoint => match s.split([',', ' ']).filter(|t| !t.trim().is_empty()).collect::<Vec<_>>()[..] {
            [x, y] => match (number(x), number(y)) {
                (Some(x), Some(y)) if x.abs() <= MAX_COORD && y.abs() <= MAX_COORD => Ok(json!([x, y])),
                _ => Err(tf("`{}` is not a point; type x, y in mm", &[s.trim()])),
            },
            _ => Err(tf("`{}` is not a point; type x, y in mm", &[s.trim()])),
        },
        Scale => units.parse_scale(s).map(|n| json!(n)).ok_or_else(|| tf("`{}` is not a scale", &[s.trim()])),
        _ => Ok(json!(s.trim())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_are_shown_and_typed_in_the_projects_units_and_the_core_keeps_millimetres() {
        let v = json!(3000.0);
        assert_eq!(shown(Length, &v, Units::Millimetres), "3000 mm");
        assert_eq!(shown(Length, &v, Units::Metres), "3.00 m");
        assert_eq!(parse(Length, "3 m", Units::Millimetres), Ok(json!(3000.0)), "a typed unit wins");
        assert_eq!(parse(Length, "3", Units::Metres), Ok(json!(3000.0)), "a bare number is in the project's unit");
        assert_eq!(shown(Point, &json!([1000.0, 2500.0]), Units::Millimetres), "1000 mm, 2500 mm");
        assert_eq!(parse(Point, "1000 mm, 2.5 m", Units::Millimetres), Ok(json!([1000.0, 2500.0])));
        assert_eq!(shown(AreaShown, &json!(12.5), Units::Millimetres), "12.50 m²");
        assert!(shown(AreaShown, &json!(12.5), Units::Feet).ends_with("ft²"));
        assert_eq!(shown(VolumeShown, &json!(2.0), Units::Metres), "2.000 m³");
    }

    #[test]
    fn what_is_not_a_number_or_is_out_of_range_is_refused_with_its_reason() {
        for bad in ["", "abc", "NaN", "inf", "-inf", "1e999"] {
            assert!(parse(Length, bad, Units::Millimetres).is_err(), "{bad:?}");
            assert!(parse(Number, bad, Units::Millimetres).is_err(), "{bad:?}");
        }
        assert!(parse(Length, "1e12", Units::Millimetres).is_err(), "past the model's reach");
        assert!(parse(Point, "1", Units::Millimetres).is_err());
        assert!(parse(Point, "1, 2, 3", Units::Millimetres).is_err());
        assert!(parse(PaperPoint, "1", Units::Millimetres).is_err());
        assert_eq!(parse(PaperPoint, "10, 20.5", Units::Metres), Ok(json!([10.0, 20.5])), "on paper it is mm whatever the units");
        assert!(parse(Text, &"x".repeat(MAX_TYPED + 1), Units::Millimetres).is_err());
        assert!(parse(Outline, &"1, 1;".repeat(MAX_POINTS + 1), Units::Millimetres).is_err());
    }

    #[test]
    fn a_blank_clears_what_may_be_none_and_an_angle_may_carry_its_degree_sign() {
        assert_eq!(parse(Limit("No Clip"), "  ", Units::Millimetres), Ok(Value::Null));
        assert_eq!(parse(NumberOr("Automatic"), "", Units::Millimetres), Ok(Value::Null));
        assert_eq!(parse(Outline, "", Units::Millimetres), Ok(Value::Null));
        assert_eq!(parse(NumberOr("Automatic"), "30", Units::Millimetres), Ok(json!(30.0)));
        assert_eq!(parse(Angle, "12.5°", Units::Millimetres), Ok(json!(12.5)));
        assert_eq!(shown(Angle, &json!(12.5), Units::Millimetres), "12.5°");
        assert_eq!(
            parse(Outline, "0, 0; 1000, 0; 1000, 1000", Units::Millimetres),
            Ok(json!([[0.0, 0.0], [1000.0, 0.0], [1000.0, 1000.0]]))
        );
        assert_eq!(parse(Text, "  Ground ", Units::Millimetres), Ok(json!("Ground")));
    }

    #[test]
    fn a_number_the_json_leaves_out_is_zero_and_a_null_it_has_is_none() {
        let e = json!({"a": {"b": 5}, "far": null});
        assert_eq!(field_of(&e, "a.b", Length), json!(5));
        assert_eq!(field_of(&e, "offset", Length), json!(0.0));
        assert_eq!(field_of(&e, "offset", Text), Value::Null);
        assert_eq!(field_of(&e, "far", Length), Value::Null);
        assert_eq!(nested("view_range.cut", json!(3)), json!({"view_range": {"cut": 3}}));
        assert_eq!(nested("height", json!(3)), json!({"height": 3}));
    }

    #[test]
    fn a_row_is_relevant_only_where_it_means_something() {
        assert!(!relevant(&json!({"shape": "line"}), "centre_mark"));
        assert!(relevant(&json!({"shape": "arc"}), "centre_mark"));
        assert!(!relevant(&json!({"top_level": null}), "top_offset"));
        assert!(relevant(&json!({"top_level": "x"}), "top_offset"));
        assert!(!relevant(&json!({"extrusion": {}}), "slope"));
        assert!(relevant(&json!({}), "name"));
    }

    #[test]
    fn hostile_values_show_as_short_text_and_never_panic() {
        let long = "x".repeat(100_000);
        assert!(text(&json!(long)).chars().count() <= MAX_VALUE);
        let many: Vec<i32> = (0..10_000).collect();
        assert!(text(&json!(many)).chars().count() <= MAX_VALUE);
        assert_eq!(text(&json!({"a": 1, "b": 2})), "{2 fields}");
        assert!(!text(&json!(1e308)).is_empty(), "a huge number stays a number");
    }
}
