//! Line patterns, line styles and detail lines, as Revit's Manage > Additional Settings and Annotate > Detail Line.
//!
//! A [`LinePattern`] is dashes, dots and spaces in millimetres of paper, so it reads the same at any scale; a [`LineStyle`]
//! is a pen, a colour and a pattern (or none, solid); a [`DetailLine`] is a line drawn in one view, in a style. Both are
//! entries of the project's library: a style names its pattern, a line its style, and a file holds their GUIDs.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::geom::P2;
use crate::ids::Guid;
use crate::language::Language;
use crate::model::MAX_POINTS;
use crate::pen::{MAX_PEN, Pen};

/// A piece of a line pattern: a dash or a space, so many mm long on paper, or a dot.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Segment {
    Dash(f64),
    Space(f64),
    Dot,
}

/// A line pattern: its segments in order, a dash or a dot then a space, again and again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LinePattern(pub Vec<Segment>);

/// What a line with no pattern is: not an entry, a name no pattern may take.
pub const SOLID: &str = "Solid";

/// The most segments a pattern has, and the shortest and the longest one, mm on paper.
pub const MAX_SEGMENTS: usize = 32;
pub const MIN_SEGMENT: f64 = 0.05;
pub const MAX_SEGMENT: f64 = 100.0;

/// How long a dot draws, mm on paper: next to nothing, its round ends make it a dot as wide as the line.
const DOT: f64 = 0.01;

/// A pattern's repeat shorter than this, px, draws solid: dashes too small to see, and too many to draw.
const SMALLEST_REPEAT: f32 = 2.0;

impl LinePattern {
    /// An error saying what to fix, unless it is a dash or a dot then a space, each pair once or more, every length a
    /// length.
    pub fn check(&self, name: &str) -> Result<(), String> {
        let who = format!("line pattern `{name}`");
        let n = self.0.len();
        if !(2..=MAX_SEGMENTS).contains(&n) || !n.is_multiple_of(2) {
            return Err(format!("{who}: a pattern is pairs of a dash or a dot then a space, 2 to {MAX_SEGMENTS} segments, not {n}"));
        }
        for (i, s) in self.0.iter().enumerate() {
            let mark = i % 2 == 0;
            let len = match (s, mark) {
                (Segment::Dash(l), true) | (Segment::Space(l), false) => Some(*l),
                (Segment::Dot, true) => None,
                _ => {
                    let want = if mark { "a dash or a dot" } else { "a space" };
                    return Err(format!("{who}: segment {} is to be {want}: a dash or a dot then a space, again and again", i + 1));
                }
            };
            if let Some(l) = len
                && !(l.is_finite() && (MIN_SEGMENT..=MAX_SEGMENT).contains(&l))
            {
                return Err(format!("{who}: segment {} is {l} mm; a segment is {MIN_SEGMENT} to {MAX_SEGMENT} mm on paper", i + 1));
            }
        }
        Ok(())
    }

    /// Its on and off lengths, px, where a mm of paper is `px_per_mm` pixels: none when its repeat is too small to see, so
    /// it draws solid.
    pub fn dash(&self, px_per_mm: f32) -> Option<Vec<f32>> {
        let px: Vec<f32> = self
            .0
            .iter()
            .map(|s| match s {
                Segment::Dash(l) | Segment::Space(l) => *l as f32 * px_per_mm,
                Segment::Dot => DOT as f32 * px_per_mm,
            })
            .collect();
        let repeat: f32 = px.iter().sum();
        (repeat.is_finite() && repeat >= SMALLEST_REPEAT).then_some(px)
    }
}

/// A line style: the pen it draws in, its colour, and its pattern by name (none, solid).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineStyle {
    pub pen: Pen,
    #[serde(default)]
    pub color: [u8; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
}

impl LineStyle {
    /// An error saying what to fix, unless its pen is one and its pattern is one of `patterns`.
    pub fn check(&self, name: &str, patterns: &BTreeMap<String, LinePattern>) -> Result<(), String> {
        let who = format!("line style `{name}`");
        if !self.pen.is_valid() {
            return Err(format!("{who}: a line weight is a pen, 0 (a hairline) to {MAX_PEN}, not {}", self.pen.0));
        }
        if let Some(p) = &self.pattern
            && !patterns.contains_key(p)
        {
            let there: Vec<&str> = patterns.keys().map(String::as_str).collect();
            return Err(format!("{who}: there's no line pattern `{p}` (there are: {SOLID}, {})", there.join(", ")));
        }
        Ok(())
    }
}

/// The line patterns a project starts with, named in `language`: our own lengths, mm on paper.
pub fn patterns(language: Language) -> BTreeMap<String, LinePattern> {
    use Segment::{Dash as D, Dot, Space as S};
    [
        ("Dash", vec![D(3.0), S(1.5)]),
        ("Long Dash", vec![D(8.0), S(2.0)]),
        ("Dot", vec![Dot, S(1.0)]),
        ("Dash Dot", vec![D(5.0), S(1.5), Dot, S(1.5)]),
        ("Hidden", vec![D(1.5), S(1.0)]),
        ("Centre", vec![D(10.0), S(1.5), D(2.0), S(1.5)]),
    ]
    .into_iter()
    .map(|(n, s)| (language.word(n).to_string(), LinePattern(s)))
    .collect()
}

pub fn default_patterns() -> BTreeMap<String, LinePattern> {
    patterns(Language::English)
}

pub fn is_default_patterns(p: &BTreeMap<String, LinePattern>) -> bool {
    *p == default_patterns()
}

/// The style a detail line is drawn in unless told another, the first of a project's styles: in English.
pub const THIN: &str = "Thin Lines";

/// The line styles a project starts with, named in `language`, black: a hairline, thin, medium and wide lines in the pens
/// of fine lines, of what's seen and of what's cut, and hidden, centre and overhead lines.
pub fn styles(language: Language) -> BTreeMap<String, LineStyle> {
    let pattern = |n: &str| Some(language.word(n).to_string());
    let style = |pen, pattern| LineStyle { pen, color: [0, 0, 0], pattern };
    [
        ("Hairline", style(Pen::HAIRLINE, None)),
        (THIN, style(Pen::FINE, None)),
        ("Medium Lines", style(Pen::PROJECTION, None)),
        ("Wide Lines", style(Pen::CUT, None)),
        ("Hidden Lines", style(Pen(2), pattern("Hidden"))),
        ("Centre Lines", style(Pen(2), pattern("Centre"))),
        ("Overhead Lines", style(Pen(2), pattern("Dash"))),
    ]
    .into_iter()
    .map(|(n, s)| (language.word(n).to_string(), s))
    .collect()
}

pub fn default_styles() -> BTreeMap<String, LineStyle> {
    styles(Language::English)
}

pub fn is_default_styles(s: &BTreeMap<String, LineStyle>) -> bool {
    *s == default_styles()
}

/// A detail line: a line drawn in one view (a level's plan, a section, an elevation or a sheet), through `points` in the
/// view's own terms (plan mm, along and up, or paper mm), round to the first again where `closed`, in line style `style`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetailLine {
    pub view: Guid,
    pub points: Vec<P2>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub closed: bool,
    pub style: String,
}

impl DetailLine {
    /// An error saying what to fix, unless it has two points or more, apart, and few enough.
    pub fn check(&self, who: &str) -> Result<(), String> {
        let n = self.points.len();
        if n < 2 {
            return Err(format!("{who}: a line needs two points or more, not {n}"));
        }
        if n > MAX_POINTS {
            return Err(format!("{who}: a line has at most {MAX_POINTS} points"));
        }
        if self.closed && n < 3 {
            return Err(format!("{who}: a closed line needs three points or more"));
        }
        if self.points.windows(2).any(|w| w[0].dist(w[1]) < 1e-6) {
            return Err(format!("{who}: two points one after another are the same; a line runs between points apart"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use crate::library::Kind;
    use crate::model::{Element, Id, Model};
    use crate::render::{Drawing, Prim, View};
    use serde_json::json;

    /// The lines `id` draws in `d`: their width, colour and dash.
    fn lines_of(d: &Drawing, id: Id) -> Vec<(f32, [u8; 3], Option<Vec<f32>>)> {
        (d.prims.iter().zip(&d.owners))
            .filter_map(|(p, o)| match p {
                Prim::Line { width, color, dash, .. } if *o == Some(id) => Some((*width, [color.0, color.1, color.2], dash.clone())),
                _ => None,
            })
            .collect()
    }

    fn plan(m: &Model, level: Id, scale: f64) -> Drawing {
        crate::render::draw(m, level, View { scale, cx: 500.0, cy: 0.0, width: 800, height: 600 })
    }

    #[test]
    fn line_patterns_and_styles_are_the_projects_named_in_its_language_saved_renamed_and_kept_while_used() {
        let mut s = Session::new();
        // A project starts with its own, and asking changes nothing.
        let r = s.execute("line_style", &json!({})).unwrap();
        assert_eq!(r["styles"]["Thin Lines"], json!({"pen": 1, "color": [0, 0, 0]}));
        assert_eq!(r["styles"]["Hidden Lines"]["pattern"], "Hidden");
        let r = s.execute("line_pattern", &json!({})).unwrap();
        assert_eq!(r["patterns"]["Dash"], json!([{"dash": 3.0}, {"space": 1.5}]));
        assert!(s.history().is_empty());
        assert_eq!(Model::new_in("p", Language::Portuguese).line_styles.keys().filter(|n| n.starts_with("Linhas")).count(), 6);
        // Made, each an entry of the library with an id and a GUID.
        s.execute("line_pattern", &json!({"name": "Fence", "segments": [{"dash": 4}, {"space": 1}, "dot", {"space": 1}]})).unwrap();
        s.execute("line_style", &json!({"name": "Fence Line", "pen": 3, "color": "#c80000", "pattern": "Fence"})).unwrap();
        assert_eq!(s.history(), ["Line Patterns", "Line Styles"]);
        assert!(s.model.library.id(&Kind::LinePattern, "Fence").is_some());
        assert!(s.model.library.id(&Kind::LineStyle, "Fence Line").is_some());
        let line =
            s.execute("detail_line", &json!({"points": [[0, 0], [1000, 0]], "style": "Fence Line"})).unwrap()["id"].as_u64().unwrap();
        // A style changed keeps what it isn't given.
        s.execute("line_style", &json!({"name": "Fence Line", "pen": 5})).unwrap();
        assert_eq!(s.model.line_styles["Fence Line"], LineStyle { pen: Pen(5), color: [200, 0, 0], pattern: Some("Fence".into()) });
        // Renamed, what names them follows, and the file holds GUIDs, so it reads back the same.
        s.execute("line_pattern", &json!({"name": "Fence", "rename": "Boundary"})).unwrap();
        s.execute("line_style", &json!({"name": "Fence Line", "rename": "Boundary Line"})).unwrap();
        assert_eq!(s.model.line_styles["Boundary Line"].pattern.as_deref(), Some("Boundary"));
        let Some(Element::DetailLine(l)) = s.model.elements.get(line) else { panic!("no line") };
        assert_eq!(l.style, "Boundary Line");
        let file = s.model.to_json().unwrap();
        assert!(!file.contains("\"Boundary Line\"\n") && !file.contains("\"style\": \"Boundary Line\""), "a file names a style by GUID");
        let (m, _) = Model::read(&file).unwrap();
        assert_eq!((&m.line_styles, &m.line_patterns, &m.elements), (&s.model.line_styles, &s.model.line_patterns, &s.model.elements));
        // In use, kept; once not, deleted.
        let e = s.execute("line_pattern", &json!({"name": "Boundary", "delete": true})).unwrap_err();
        assert!(e.contains("line style `Boundary Line`"), "{e}");
        let e = s.execute("line_style", &json!({"name": "Boundary Line", "delete": true})).unwrap_err();
        assert!(e.contains(&format!("detail line {line}")), "{e}");
        s.execute("delete", &json!({"ids": [line]})).unwrap();
        s.execute("line_style", &json!({"name": "Boundary Line", "delete": true})).unwrap();
        s.execute("line_pattern", &json!({"name": "Boundary", "delete": true})).unwrap();
        assert!(s.model.library.id(&Kind::LinePattern, "Boundary").is_none());
        // What isn't one is refused, saying why, and changes nothing.
        let before = s.model.clone();
        for (cmd, p, says) in [
            ("line_pattern", json!({"name": "Solid", "segments": [{"dash": 1}, {"space": 1}]}), "Solid"),
            ("line_pattern", json!({"name": "X", "segments": [{"dash": 1}]}), "pairs"),
            ("line_pattern", json!({"name": "X", "segments": [{"space": 1}, {"dash": 1}]}), "segment 1 is to be a dash or a dot"),
            ("line_pattern", json!({"name": "X", "segments": [{"dash": 0}, {"space": 1}]}), "0.05 to 100"),
            ("line_pattern", json!({"name": "X", "segments": [{"dash": 1e300}, {"space": 1}]}), "0.05 to 100"),
            ("line_pattern", json!({"name": "X", "segments": [{"dash": 1}, {"gap": 1}]}), "segments"),
            ("line_pattern", json!({"name": "X"}), "no line pattern `X`"),
            ("line_pattern", json!({"name": "Dash", "rename": "Dot"}), "already"),
            ("line_style", json!({"name": "X", "pen": 2, "pattern": "Nope"}), "no line pattern `Nope`"),
            ("line_style", json!({"name": "X", "pen": 17}), "0 (a hairline) to 16"),
            ("line_style", json!({"name": "Thin Lines", "color": "red"}), "color"),
            ("line_style", json!({"name": "X"}), "`pen`"),
            ("line_style", json!({"name": "Thin Lines", "colour": [1, 2, 3]}), "colour"),
            ("detail_line", json!({"points": [[0, 0], [1000, 0]], "style": "Nope"}), "no line style `Nope`"),
            ("detail_line", json!({"points": [[0, 0]]}), "two points"),
            ("detail_line", json!({"points": [[0, 0], [0, 0]]}), "apart"),
            ("detail_line", json!({"points": [[0, 0], [5, 0]], "closed": true}), "three points"),
            ("detail_line", json!({"points": [[0, 0], [5, 0]], "view": "Nowhere"}), "Nowhere"),
        ] {
            let e = s.execute(cmd, &p).unwrap_err();
            assert!(e.contains(says), "{cmd} {p}: {e}");
        }
        assert_eq!(*s.model, *before);
    }

    #[test]
    fn a_detail_line_draws_in_its_styles_pen_colour_and_pattern_in_its_own_view_only_at_any_zoom() {
        let mut s = Session::new();
        s.execute("line_pattern", &json!({"name": "Fence", "segments": [{"dash": 4}, {"space": 1}, "dot", {"space": 1}]})).unwrap();
        s.execute("line_style", &json!({"name": "Fence Line", "pen": 7, "color": [200, 0, 0], "pattern": "Fence"})).unwrap();
        let line = s.execute("detail_line", &json!({"points": [[0, 0], [1000, 0], [1000, 500]], "style": "Fence Line"})).unwrap()["id"]
            .as_u64()
            .unwrap();
        let (ground, upper) = (100000, 100001);
        for zoom in [0.2, 0.8] {
            let d = plan(&s.model, ground, zoom);
            let drawn = lines_of(&d, line);
            assert_eq!(drawn.len(), 1, "{drawn:?}");
            let (width, color, dash) = &drawn[0];
            assert!((width - d.width(Pen(7))).abs() < 1e-4);
            assert_eq!(*color, [200, 0, 0]);
            // Dashes as long on paper as the pattern says, as the pen's width is: 4 mm of dash, 1 mm of space, a dot.
            let dash = dash.as_ref().expect("dashed");
            // Pixels to a mm of the paper: the pen's width over its width on paper at the plan's 1 : 100.
            let mm = width / s.model.line_weights().at(100.0).mm(Pen(7), false) as f32;
            assert!((dash[0] / mm - 4.0).abs() < 1e-3 && (dash[1] / mm - 1.0).abs() < 1e-3 && dash[2] < dash[1] / 10.0, "{dash:?}");
        }
        // Not in another level's plan.
        assert!(lines_of(&plan(&s.model, upper, 0.2), line).is_empty());
        // So small it can't be seen, a pattern draws solid.
        assert_eq!(lines_of(&plan(&s.model, ground, 0.0005), line)[0].2, None);
        // In a section, along and up; on a sheet, in paper mm.
        let section =
            s.execute("section", &json!({"start": [-1000, 2000], "end": [3000, 2000], "name": "S"})).unwrap()["id"].as_u64().unwrap();
        s.execute("sheet", &json!({"number": "A1", "name": "Details"})).unwrap();
        let cut = s.execute("detail_line", &json!({"view": "S", "points": [[0, 0], [500, 500]]})).unwrap()["id"].as_u64().unwrap();
        let paper = s.execute("detail_line", &json!({"view": "A1", "points": [[20, 20], [80, 20]], "style": "Wide Lines"})).unwrap()["id"]
            .as_u64()
            .unwrap();
        let d = crate::render::section(&s.model, section, crate::render::section_fit(&s.model, section, 800, 600));
        assert_eq!(lines_of(&d, cut).len(), 1);
        assert!(lines_of(&d, line).is_empty());
        let sheet =
            crate::sheet::draw(&s.model, s.model.elements.iter().find(|(_, e)| matches!(e, Element::Sheet(_))).map(|(i, _)| *i).unwrap())
                .unwrap();
        assert_eq!(lines_of(&sheet, paper).len(), 1);
        assert_eq!(lines_of(&sheet, paper)[0].1, [0, 0, 0]);
        // It goes with its view.
        s.execute("delete", &json!({"ids": [section]})).unwrap();
        assert!(s.model.elements.get(cut).is_none());
    }

    #[test]
    fn a_detail_line_moves_turns_and_copies_in_its_view() {
        let mut s = Session::new();
        let line = s.execute("detail_line", &json!({"points": [[0, 0], [1000, 0]]})).unwrap()["id"].as_u64().unwrap();
        let Some(Element::DetailLine(l)) = s.model.elements.get(line) else { panic!("no line") };
        assert_eq!(l.style, THIN);
        s.execute("move", &json!({"ids": [line], "by": [100, 200]})).unwrap();
        s.execute("rotate", &json!({"ids": [line], "about": [100, 200], "angle": 90})).unwrap();
        let Some(Element::DetailLine(l)) = s.model.elements.get(line) else { panic!("no line") };
        assert!(l.points[0].dist(P2(100.0, 200.0)) < 1e-6 && l.points[1].dist(P2(100.0, 1200.0)) < 1e-6, "{:?}", l.points);
        let r = s.execute("copy", &json!({"ids": [line], "by": [0, 3000]})).unwrap();
        assert_eq!(r["ids"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn a_pattern_too_small_to_see_draws_solid() {
        let p = LinePattern(vec![Segment::Dash(1.0), Segment::Space(0.5)]);
        assert_eq!(p.dash(2.0), Some(vec![2.0, 1.0]));
        assert_eq!(p.dash(1.0), None);
        assert_eq!(p.dash(f32::INFINITY), None);
    }
}
