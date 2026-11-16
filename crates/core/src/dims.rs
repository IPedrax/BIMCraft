//! Dimension types, as Revit's: how a dimension draws (its string, ticks, lines and witness lines) and letters its
//! numbers (a font, a size, bold, italic, underline, a width factor, an offset from its line, an opaque or transparent
//! background, which way it reads, its units and rounding, and the symbols a radius, a diameter and equal segments
//! show). Lengths are mm on paper, so a dimension reads the same at any scale. A project starts with two: diagonal
//! ticks, which aligned, linear, angular and arc length dimensions take, and arrows, which radial and diameter ones do.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::geom::P2;
use crate::ids::Key;
use crate::model::{DimKind, Face, Model, Orient, Reference};
use crate::pen::Pen;
use crate::text::TextType;
use crate::units::Units;

/// The type aligned, linear, angular and arc length dimensions start in.
pub const DIAGONAL: &str = "Diagonal - 2.5mm Arial";
/// The type radial and diameter dimensions start in.
pub const ARROW: &str = "Arrow - 2.5mm Arial";
/// Characters a prefix, suffix, symbol or any of a dimension's own texts may have.
pub const MAX_DIM_TEXT: usize = 200;

/// How a dimension of several segments is drawn: Revit's Dimension String Type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimString {
    /// Segment after segment, on one line.
    #[default]
    Continuous,
    /// Each from the first reference, on lines stacked out from `at`.
    Baseline,
    /// On one line, each witness line with its distance from the first.
    Ordinate,
}

/// What marks where a witness line meets the dimension line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tick {
    #[default]
    Diagonal,
    /// A filled arrowhead.
    Arrow,
    /// An arrowhead of two strokes.
    OpenArrow,
    /// A filled dot.
    Dot,
    None,
}

/// Where a witness line starts: Revit's Witness Line Control.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Witness {
    /// `witness_gap` off what it measures.
    #[default]
    GapToElement,
    /// `witness_length` long, whatever it measures, from past the dimension line back toward it.
    Fixed,
}

/// Whether the paper shows behind a dimension's numbers, hiding what they cross.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Background {
    #[default]
    Opaque,
    Transparent,
}

/// Which way the numbers of a dimension running up the page read: Revit's Read Convention. Up, then left: from the
/// bottom up, read from the right of the page; up, then right: from the top down, read from the left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Read {
    #[default]
    UpThenLeft,
    UpThenRight,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DimensionType {
    #[serde(default)]
    pub string: DimString,
    #[serde(default)]
    pub tick: Tick,
    #[serde(default = "tick_size")]
    pub tick_size: f64,
    #[serde(default = "line_pen")]
    pub line_pen: Pen,
    #[serde(default = "tick_pen")]
    pub tick_pen: Pen,
    #[serde(default)]
    pub color: [u8; 3],
    /// How far the dimension line runs past its outermost witness lines.
    #[serde(default = "extension")]
    pub line_extension: f64,
    #[serde(default)]
    pub witness: Witness,
    #[serde(default = "witness_length")]
    pub witness_length: f64,
    #[serde(default = "witness_gap")]
    pub witness_gap: f64,
    /// How far a witness line runs past the dimension line.
    #[serde(default = "extension")]
    pub witness_extension: f64,
    /// How far apart a baseline string's lines are.
    #[serde(default = "baseline_spacing")]
    pub baseline_spacing: f64,
    /// A cross at a radial or diameter dimension's centre.
    #[serde(default = "yes")]
    pub centre_mark: bool,
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
    /// How far its numbers stand off the dimension line.
    #[serde(default = "text_offset")]
    pub text_offset: f64,
    #[serde(default)]
    pub background: Background,
    #[serde(default)]
    pub read: Read,
    /// Its own units, else the project's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<Units>,
    /// Places after the point, in mm, cm, m or decimal feet; else as the units show them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,
    #[serde(default = "radius_symbol")]
    pub radius_symbol: String,
    #[serde(default = "diameter_symbol")]
    pub diameter_symbol: String,
    #[serde(default = "equality_text")]
    pub equality_text: String,
}

fn tick_size() -> f64 {
    2.0
}
fn line_pen() -> Pen {
    Pen::PROJECTION
}
fn tick_pen() -> Pen {
    Pen::HEAVY
}
fn extension() -> f64 {
    0.8
}
fn witness_length() -> f64 {
    5.0
}
fn witness_gap() -> f64 {
    0.6
}
fn baseline_spacing() -> f64 {
    7.0
}
fn yes() -> bool {
    true
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
fn text_offset() -> f64 {
    1.0
}
fn radius_symbol() -> String {
    "R".into()
}
fn diameter_symbol() -> String {
    "Ø".into()
}
fn equality_text() -> String {
    "EQ".into()
}

impl Default for DimensionType {
    fn default() -> DimensionType {
        DimensionType {
            string: DimString::Continuous,
            tick: Tick::Diagonal,
            tick_size: tick_size(),
            line_pen: line_pen(),
            tick_pen: tick_pen(),
            color: [0, 0, 0],
            line_extension: extension(),
            witness: Witness::GapToElement,
            witness_length: witness_length(),
            witness_gap: witness_gap(),
            witness_extension: extension(),
            baseline_spacing: baseline_spacing(),
            centre_mark: true,
            font: font(),
            text_size: text_size(),
            bold: false,
            italic: false,
            underline: false,
            width_factor: 1.0,
            text_offset: text_offset(),
            background: Background::Opaque,
            read: Read::UpThenLeft,
            units: None,
            decimals: None,
            radius_symbol: radius_symbol(),
            diameter_symbol: diameter_symbol(),
            equality_text: equality_text(),
        }
    }
}

impl DimensionType {
    /// An error saying what to fix, unless it is one a dimension can be drawn in.
    pub fn check(&self, name: &str) -> Result<(), String> {
        let who = format!("dimension type `{name}`");
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
            ("tick size", self.tick_size),
            ("dimension line extension", self.line_extension),
            ("witness line length", self.witness_length),
            ("witness line gap", self.witness_gap),
            ("witness line extension", self.witness_extension),
            ("baseline spacing", self.baseline_spacing),
            ("text offset", self.text_offset),
        ] {
            if !(0.0..=50.0).contains(&x) {
                return Err(format!("{who}: its {what}, {x} mm on paper, isn't from 0 to 50"));
            }
        }
        if self.decimals.is_some_and(|d| d > 6) {
            return Err(format!("{who}: it rounds to at most 6 places, not {}", self.decimals.unwrap_or_default()));
        }
        for (what, t) in
            [("radius symbol", &self.radius_symbol), ("diameter symbol", &self.diameter_symbol), ("equality text", &self.equality_text)]
        {
            dim_text(&who, what, t)?;
        }
        Ok(())
    }

    /// The text type its numbers are lettered in.
    pub fn lettering(&self) -> TextType {
        TextType {
            font: self.font.clone(),
            size: self.text_size,
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            width_factor: self.width_factor,
            color: self.color,
            ..TextType::default()
        }
    }

    /// A length as it shows one, in `project` units unless it has its own.
    pub fn length(&self, project: Units, mm: f64) -> String {
        rounded(self.units.unwrap_or(project), self.decimals, mm)
    }
}

/// A length in `units`, to `decimals` places after the point in mm, cm, m or decimal feet; else as the units show it.
pub fn rounded(units: Units, decimals: Option<u8>, mm: f64) -> String {
    let Some(places) = decimals else { return units.length(mm) };
    let (per, mark) = match units {
        Units::Millimetres => (1.0, ""),
        Units::Centimetres => (10.0, ""),
        Units::Metres => (1000.0, ""),
        Units::Feet => (304.8, "'"),
        // Inches and feet and inches round to sixteenths.
        Units::Inches | Units::FeetInches => return units.length(mm),
    };
    let x = mm / per;
    let k = 10f64.powi(i32::from(places));
    format!("{:.*}{mark}", usize::from(places), (x * k).round() / k + 0.0)
}

/// An error unless `t`, a dimension's or a type's `what`, is up to [`MAX_DIM_TEXT`] characters on one line.
pub fn dim_text(who: &str, what: &str, t: &str) -> Result<(), String> {
    if t.chars().count() > MAX_DIM_TEXT || t.chars().any(char::is_control) {
        return Err(format!("{who}: its {what} is up to {MAX_DIM_TEXT} characters on one line"));
    }
    Ok(())
}

/// The dimension types a project starts with.
pub fn types() -> BTreeMap<String, DimensionType> {
    BTreeMap::from([
        (DIAGONAL.to_string(), DimensionType::default()),
        (ARROW.to_string(), DimensionType { tick: Tick::Arrow, tick_size: 2.5, ..DimensionType::default() }),
    ])
}

/// The type a new dimension of `kind` gets when none is named: [`ARROW`] for a radius or a diameter, else [`DIAGONAL`];
/// else the first there is.
pub fn default_type(types: &BTreeMap<String, DimensionType>, kind: DimKind) -> Option<&String> {
    let wanted = if matches!(kind, DimKind::Radial | DimKind::Diameter) { ARROW } else { DIAGONAL };
    types.get_key_value(wanted).map(|(k, _)| k).or_else(|| types.keys().next())
}

/// How the desktop's Dimension tool lays a dimension between what was picked: as the cursor says (between points only:
/// aligned among them, else horizontal or vertical, [`Orient::follow`]), aligned, horizontal or vertical.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placing {
    #[default]
    Auto,
    Aligned,
    Horizontal,
    Vertical,
}

impl Placing {
    pub const ALL: [Placing; 4] = [Placing::Auto, Placing::Aligned, Placing::Horizontal, Placing::Vertical];

    /// The next, as Space steps through them.
    pub fn next(self) -> Placing {
        match self {
            Placing::Auto => Placing::Aligned,
            Placing::Aligned => Placing::Horizontal,
            Placing::Horizontal => Placing::Vertical,
            Placing::Vertical => Placing::Auto,
        }
    }

    /// The kind and orientation of a dimension placed this way through `at` between `refs`. A line picked fixes the way
    /// an automatic one runs: square to it.
    pub fn kind(self, m: &Model, refs: &[Reference], at: P2) -> (DimKind, Option<Orient>) {
        match self {
            Placing::Aligned => (DimKind::Aligned, None),
            Placing::Horizontal => (DimKind::Linear, Some(Orient::Horizontal)),
            Placing::Vertical => (DimKind::Linear, Some(Orient::Vertical)),
            Placing::Auto if refs.iter().any(|r| r.point.is_none()) => (DimKind::Aligned, None),
            Placing::Auto => {
                let points: Vec<P2> = refs.iter().filter_map(|r| m.reference_point(r).ok()).collect();
                match Orient::follow(&points, at) {
                    Some(o) => (DimKind::Linear, Some(o)),
                    None => (DimKind::Aligned, None),
                }
            }
        }
    }
}

/// Of element `id`'s round lines a dimension of `kind` measures (radial and diameter: a round column's edge, a curved
/// wall's centre line or faces, a circle or an arc; arc length: a curved wall's or an arc), the one passing nearest `p`.
pub fn round_at(m: &Model, id: impl Key, p: P2, kind: DimKind) -> Option<Reference> {
    let id = m.guid(id);
    [Face::Centre, Face::Left, Face::Right]
        .into_iter()
        .map(|face| Reference::line(id, face))
        .filter_map(|r| {
            let (c, radius) = match kind {
                DimKind::ArcLength => m.reference_sweep(&r).ok().map(|(c, radius, _, _)| (c, radius))?,
                _ => m.reference_arc(&r).ok()?,
            };
            Some((r, (p.dist(c) - radius).abs()))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(r, _)| r)
}

/// Of element `id`'s ends, the one within `near` mm of `p` that lies on the arc `arc` measures along: where an arc length
/// dimension between two points starts or ends.
pub fn arc_point(m: &Model, arc: &Reference, id: impl Key, p: P2, near: f64) -> Option<Reference> {
    let (c, radius, _, _) = m.reference_sweep(arc).ok()?;
    let (r, d) = m.endpoint_at(id, p)?;
    let q = m.reference_point(&r).ok()?;
    (d <= near && (q.dist(c) - radius).abs() <= crate::model::TOL).then_some(r)
}
