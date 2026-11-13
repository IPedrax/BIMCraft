//! Grid types, as Revit's: how a grid line draws (its line pattern, pen and colour) and how its bubble letters its name (a
//! font, a size, bold, italic, underline and a width factor). A pattern is a [`LinePattern`], its lengths mm on paper, so a
//! grid reads the same at any scale; the pen and the colour are those of the annotation. A project starts with types for the
//! standards a drawing follows (ISO 128 and EN ISO 7519, DIN 1356, ASME Y14.2) and for the office's own.

use std::collections::BTreeMap;

use bimcraft_i18n::msg;
use serde::{Deserialize, Serialize};

use crate::language::Language;
use crate::lines::{LinePattern, SOLID};
use crate::pen::{MAX_PEN, Pen};
use crate::text::TextType;

/// The type a new grid gets when none is named.
pub const ISO: &str = msg!("Grid - ISO 128");
/// The other starter types, in the order they are listed here.
pub const DIN: &str = msg!("Grid - DIN 1356");
pub const ASME: &str = msg!("Grid - ASME Y14.2");
pub const ARCHITECTURAL: &str = msg!("Grid - Architectural");
pub const CONTINUOUS: &str = msg!("Grid - Continuous");
pub const COMPACT: &str = msg!("Grid - Compact");

/// The line patterns the starter types draw in, by their English names: `lines::patterns` names them in the project's
/// language.
const ISO_PATTERN: &str = msg!("Grid - ISO Dash Dot");
const DIN_PATTERN: &str = msg!("Grid - DIN Dash Dot");
const ASME_PATTERN: &str = msg!("Grid - ASME Chain");
const ARCHITECTURAL_PATTERN: &str = msg!("Grid - Architectural Chain");
const COMPACT_PATTERN: &str = msg!("Grid - Compact Dash Dot");

/// How a grid line draws and how its bubble is lettered: a line pattern by name (none, solid), a pen, a colour, and the
/// text of its name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridType {
    /// The project's line pattern it draws in, mm on paper; none, a solid line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_pattern: Option<String>,
    #[serde(default = "line_pen")]
    pub line_pen: Pen,
    #[serde(default = "color")]
    pub color: [u8; 3],
    #[serde(default = "font")]
    pub font: String,
    /// Its name's capitals, mm on paper.
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
}

fn line_pen() -> Pen {
    Pen::PROJECTION
}
fn color() -> [u8; 3] {
    [60, 100, 150]
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

impl Default for GridType {
    fn default() -> GridType {
        GridType {
            line_pattern: None,
            line_pen: line_pen(),
            color: color(),
            font: font(),
            text_size: text_size(),
            bold: false,
            italic: false,
            underline: false,
            width_factor: one(),
        }
    }
}

impl GridType {
    /// An error saying what to fix, unless its pattern is one of `line_patterns`, its pen one, and it letters a name.
    pub fn check(&self, name: &str, line_patterns: &BTreeMap<String, LinePattern>) -> Result<(), String> {
        let who = format!("grid type `{name}`");
        if let Some(p) = &self.line_pattern
            && !line_patterns.contains_key(p)
        {
            let there: Vec<&str> = line_patterns.keys().map(String::as_str).collect();
            return Err(format!("{who}: there's no line pattern `{p}` (there are: {SOLID}, {})", there.join(", ")));
        }
        if !self.line_pen.is_valid() {
            return Err(format!("{who}: a line weight is a pen, 0 (a hairline) to {MAX_PEN}, not {}", self.line_pen.0));
        }
        self.text_type().check_who(&who)
    }

    /// The text type its name is lettered in, so a name is laid out by [`crate::text::layout`] as a note is.
    pub fn text_type(&self) -> TextType {
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
}

/// The grid types a project starts with, named in `language`. A pattern is mm on paper; the standards fix one reading each,
/// which the user may change as any type's.
pub fn types(language: Language) -> BTreeMap<String, GridType> {
    let pattern = |n: &str| Some(language.word(n).to_string());
    let grid = |line_pattern, line_pen, text_size| GridType { line_pattern, line_pen, text_size, ..GridType::default() };
    [
        (ISO, grid(pattern(ISO_PATTERN), Pen::PROJECTION, 2.5)),
        (DIN, grid(pattern(DIN_PATTERN), Pen(7), 2.5)),
        (ASME, grid(pattern(ASME_PATTERN), Pen(9), 3.5)),
        (ARCHITECTURAL, grid(pattern(ARCHITECTURAL_PATTERN), Pen::PROJECTION, 2.5)),
        (CONTINUOUS, grid(None, Pen::PROJECTION, 2.5)),
        (COMPACT, grid(pattern(COMPACT_PATTERN), Pen::PROJECTION, 2.0)),
    ]
    .into_iter()
    .map(|(n, t)| (language.word(n).to_string(), t))
    .collect()
}

/// [`types`] in English: what a file from before grid types has.
pub fn default_types() -> BTreeMap<String, GridType> {
    types(Language::English)
}

/// The type a new grid gets when none is named: [`ISO`], in `language` (else its English name, a file from another
/// language), else the first.
pub fn default_type(types: &BTreeMap<String, GridType>, language: Language) -> Option<&String> {
    [language.word(ISO), ISO].into_iter().find_map(|n| types.get_key_value(n).map(|(k, _)| k)).or_else(|| types.keys().next())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lines::SOLID;

    #[test]
    fn a_grid_type_checks_its_pattern_pen_and_lettering() {
        let patterns = crate::lines::default_patterns();
        let mut all = types(Language::English);
        let iso = all.remove(ISO).expect("a starter type");
        assert_eq!(iso.check(ISO, &patterns), Ok(()));
        assert!(iso.line_pattern.as_deref() == Some(ISO_PATTERN));

        let bad = GridType { line_pattern: Some("Nope".into()), ..iso.clone() };
        let e = bad.check(ISO, &patterns).unwrap_err();
        assert!(e.starts_with(&format!("grid type `{ISO}`")), "{e}");
        assert!(e.contains(SOLID) && e.contains("Dash"), "{e}");
        let bad = GridType { line_pen: Pen(17), ..iso.clone() };
        assert!(bad.check(ISO, &patterns).unwrap_err().contains("0 (a hairline) to 16"));
        for (font, size, width) in [(" ".into(), 2.5, 1.0), (crate::text::ARIAL.into(), 0.0, 1.0), (crate::text::ARIAL.into(), 2.5, 0.0)] {
            let bad = GridType { font, text_size: size, width_factor: width, ..iso.clone() };
            let e = bad.check(ISO, &patterns).unwrap_err();
            assert!(e.starts_with(&format!("grid type `{ISO}`")), "{e}");
        }
    }

    #[test]
    fn the_starter_types_are_named_in_the_projects_language_and_the_default_is_iso() {
        let starter = types(Language::English);
        for name in [ISO, DIN, ASME, ARCHITECTURAL, CONTINUOUS, COMPACT] {
            assert!(starter.contains_key(name), "{name}");
            assert_eq!(starter[name].check(name, &crate::lines::default_patterns()), Ok(()));
        }
        assert_eq!(default_type(&starter, Language::English).map(String::as_str), Some(ISO));
        let patterns = crate::lines::patterns(Language::Portuguese);
        let translated = types(Language::Portuguese);
        for t in translated.values() {
            assert!(t.line_pattern.as_ref().is_none_or(|p| patterns.contains_key(p)), "{t:?}");
        }
        assert_eq!(default_type(&translated, Language::Portuguese).map(String::as_str), Some(Language::Portuguese.word(ISO)));
    }
}
