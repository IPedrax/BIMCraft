//! Text notes' lettering: text types (Revit's: a font, a size on paper, bold, italic and underline, a width factor, a
//! colour and a border), the fonts that letter them, the formatting a note may override on parts of itself, and a note
//! laid out line by line and character by character: what its drawing, the desktop's editor (its caret and selection)
//! and a click on it all read.
//!
//! A type's font is any the system has (`system.rs` lists them), our own single-stroke lettering (`font.rs`), or IBM Plex
//! Sans or Mono, under the OFL (`assets/fonts`, which every package carries with its licence). A font the system hasn't
//! is stood in for by its look-alikes (Arial by Liberation Sans, Arimo, Helvetica…), else Plex Sans: so Arial, the
//! default, letters a note on any system, and a project from another machine opens and draws. A character a face has
//! no letter for is lettered from Plex Sans, then Plex Mono, then the system's wide fonts (CJK ones among them), else
//! drawn as a question mark. Bold and italic are a family's own faces where it has them; else a face is made bold by
//! drawing its outlines and strokes heavier (a variable one by its weight) and italic by slanting it.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

pub mod system;

use crate::language::Language;

/// IBM Plex Sans, its weights from 100 to 700 (OFL).
pub const PLEX_SANS: &[u8] = include_bytes!("../../../assets/fonts/ibm-plex-sans/IBMPlexSans-Variable.ttf");
/// IBM Plex Mono, regular (OFL).
pub const PLEX_MONO: &[u8] = include_bytes!("../../../assets/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf");

/// The text size a project's first text type has, mm on paper.
pub const DEFAULT_SIZE: f64 = 2.5;
/// Formatting runs one note may have.
pub const MAX_RUNS: usize = 1000;
/// Characters one note may have.
pub const MAX_NOTE: usize = 4000;
/// From one line's top to the next, times the text size.
pub const LINE_PITCH: f64 = 1.6;
/// How far an italic letter leans: across, for each unit up.
const SLANT: f32 = 0.2;
/// Spaces a tab is.
const TAB: usize = 4;

/// The font a new text type has: on Windows and macOS, Arial itself; elsewhere, what stands in for it.
pub const ARIAL: &str = "Arial";
/// Our single-stroke lettering.
pub const LETTERING: &str = "BIMCraft Lettering";
pub const SANS: &str = "IBM Plex Sans";
pub const MONO: &str = "IBM Plex Mono";
/// The fonts every BIMCraft has.
pub const BUNDLED: [&str; 3] = [SANS, MONO, LETTERING];

/// Where a face's letters come from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    Lettering,
    Sans,
    Mono,
    /// A system font's file, which of its faces, and its family.
    File {
        path: Arc<Path>,
        index: u32,
        family: Arc<str>,
    },
}

/// A face characters are drawn from: where its letters come from, the weight a variable one is set to, and whether its
/// outlines are drawn heavier (a bold it hasn't) or slanted (an italic it hasn't).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Face {
    pub source: Source,
    pub weight: Option<u16>,
    pub heavy: bool,
    pub slant: bool,
}

impl Face {
    /// One of ours, bold and italic as asked: Plex Sans by its own weight, the others drawn heavier; all slanted.
    pub fn bundled(source: Source, bold: bool, italic: bool) -> Face {
        let sans = source == Source::Sans;
        Face { source, weight: (bold && sans).then_some(700), heavy: bold && !sans, slant: italic }
    }

    /// The family that letters it.
    pub fn family(&self) -> &str {
        match &self.source {
            Source::Lettering => LETTERING,
            Source::Sans => SANS,
            Source::Mono => MONO,
            Source::File { family, .. } => family,
        }
    }
}

/// The face font `font` letters bold or italic characters in, on this system ([`resolve_in`] its fonts), kept.
pub fn resolve(font: &str, bold: bool, italic: bool) -> Face {
    type Kept = Mutex<HashMap<(String, bool, bool), Face>>;
    static KEPT: OnceLock<Kept> = OnceLock::new();
    let kept = KEPT.get_or_init(Default::default);
    let key = (font.to_string(), bold, italic);
    if let Some(f) = kept.lock().ok().and_then(|m| m.get(&key).cloned()) {
        return f;
    }
    let f = resolve_in(system::index(), font, bold, italic);
    if let Ok(mut m) = kept.lock() {
        if m.len() > 1000 {
            m.clear();
        }
        m.insert(key, f.clone());
    }
    f
}

/// The face font `font` letters bold or italic characters in, of the fonts `index` lists: ours by name, else the family,
/// else the first of its stand-ins the system has, else Plex Sans.
pub fn resolve_in(index: &system::Index, font: &str, bold: bool, italic: bool) -> Face {
    let name = font.trim();
    for (n, source) in [(LETTERING, Source::Lettering), (SANS, Source::Sans), (MONO, Source::Mono)] {
        if name.eq_ignore_ascii_case(n) {
            return Face::bundled(source, bold, italic);
        }
    }
    std::iter::once(name)
        .chain(system::substitutes(name).iter().copied())
        .find_map(|n| index.family(n))
        .and_then(|faces| pick(faces, bold, italic))
        .unwrap_or_else(|| Face::bundled(Source::Sans, bold, italic))
}

/// Of a family's faces, the one nearest bold or italic as asked, and what it is made to be the rest.
fn pick(faces: &[system::SystemFace], bold: bool, italic: bool) -> Option<Face> {
    let target: u16 = if bold { 700 } else { 400 };
    let off = |f: &system::SystemFace| {
        // Upright or italic as asked first, then of normal width (not a Narrow listed in the family), then the weight.
        u32::from(f.italic != italic) * 100_000
            + u32::from(f.width.abs_diff(5)) * 1_000
            + if f.variable { 0 } else { u32::from(f.weight.abs_diff(target)) }
    };
    let f = faces.iter().min_by_key(|f| off(f))?;
    Some(Face {
        source: Source::File { path: f.path.clone(), index: f.index, family: Arc::from(f.family.as_str()) },
        weight: (bold && f.variable).then_some(700),
        heavy: bold && !f.variable && f.weight < 600,
        slant: italic && !f.italic,
    })
}

/// Font `name` as a text type names it, if this system letters it: one of ours, a family it has, or one it stands
/// another in for (Arial anywhere).
pub fn known(name: &str) -> Option<String> {
    known_in(system::index(), name)
}

pub fn known_in(index: &system::Index, name: &str) -> Option<String> {
    let name = name.trim();
    if let Some(b) = BUNDLED.iter().find(|b| b.eq_ignore_ascii_case(name)) {
        return Some(b.to_string());
    }
    if let Some(f) = index.family(name).and_then(|f| f.first()) {
        return Some(f.family.clone());
    }
    system::substitutes(name).iter().find(|s| s.eq_ignore_ascii_case(name)).map(|s| s.to_string())
}

/// Every font a text type can name here, by name: Arial, ours and the system's.
pub fn families() -> Vec<String> {
    let mut out: Vec<String> =
        std::iter::once(ARIAL.to_string()).chain(BUNDLED.iter().map(|b| b.to_string())).chain(system::index().names()).collect();
    out.sort_by_key(|n| n.to_lowercase());
    out.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    out
}

/// A text type: the font its notes are lettered in, their capitals' height on paper (mm), whether they are bold, italic
/// and underlined, how much wider than the font draws them, their colour, and the border drawn round them `border_offset`
/// mm on paper out from their text (from which a leader starts too) in pen `border_pen`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextType {
    #[serde(default = "sans")]
    pub font: String,
    #[serde(default = "default_size")]
    pub size: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub width_factor: f64,
    #[serde(default)]
    pub color: [u8; 3],
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub border: bool,
    #[serde(default = "default_offset")]
    pub border_offset: f64,
    #[serde(default = "default_pen")]
    pub border_pen: crate::pen::Pen,
}

fn sans() -> String {
    ARIAL.into()
}
fn default_size() -> f64 {
    DEFAULT_SIZE
}
fn one() -> f64 {
    1.0
}
fn is_one(x: &f64) -> bool {
    *x == 1.0
}
fn default_offset() -> f64 {
    1.0
}
fn default_pen() -> crate::pen::Pen {
    crate::pen::Pen::FINE
}

impl Default for TextType {
    fn default() -> TextType {
        TextType {
            font: sans(),
            size: DEFAULT_SIZE,
            bold: false,
            italic: false,
            underline: false,
            width_factor: 1.0,
            color: [0, 0, 0],
            border: false,
            border_offset: default_offset(),
            border_pen: default_pen(),
        }
    }
}

impl TextType {
    /// An error saying what to fix, unless it is one a note can be lettered in.
    pub fn check(&self, name: &str) -> Result<(), String> {
        let who = format!("text type `{name}`");
        // Any name: a font this system hasn't is stood in for, so a project from another opens and draws.
        let n = self.font.trim().chars().count();
        if n == 0 || n > crate::model::MAX_TEXT || self.font.chars().any(char::is_control) {
            return Err(format!("{who}: a font is named by up to {} characters", crate::model::MAX_TEXT));
        }
        if !(self.size > 0.0 && self.size <= 100.0) {
            return Err(format!("{who}: its size, {} mm, isn't above 0 and at most 100", self.size));
        }
        if !(0.1..=10.0).contains(&self.width_factor) {
            return Err(format!("{who}: its width factor, {}, isn't from 0.1 to 10", self.width_factor));
        }
        if !(0.0..=50.0).contains(&self.border_offset) {
            return Err(format!("{who}: its border offset, {} mm, isn't from 0 to 50", self.border_offset));
        }
        Ok(())
    }

    /// How its notes are lettered where they override nothing.
    pub fn style(&self) -> Style {
        Style { bold: self.bold, italic: self.italic, underline: self.underline }
    }
}

/// The text types a project starts with: 2.5, 3.5 and 5 mm in Arial.
pub fn types(_language: Language) -> BTreeMap<String, TextType> {
    [(DEFAULT_SIZE, "2.5mm"), (3.5, "3.5mm"), (5.0, "5mm")]
        .into_iter()
        .map(|(size, name)| (name.to_string(), TextType { size, ..TextType::default() }))
        .collect()
}

/// [`types`] in English: what a file from before text types has.
pub fn default_types() -> BTreeMap<String, TextType> {
    types(Language::English)
}

/// The type a new note gets when none is named: 2.5mm, else the first.
pub fn default_type(types: &BTreeMap<String, TextType>) -> Option<&String> {
    types.get_key_value("2.5mm").map(|(k, _)| k).or_else(|| types.keys().next())
}

/// The name of a text type of `size` mm for a note from before text types: one in Arial of that size and nothing
/// else, or a new one named for it.
pub fn type_for_size(types: &mut BTreeMap<String, TextType>, size: f64) -> String {
    let wanted = TextType { size, ..TextType::default() };
    if let Some((n, _)) = types.iter().find(|(_, t)| **t == wanted) {
        return n.clone();
    }
    let base = format!("{}mm", (size * 100.0).round() / 100.0);
    let mut name = base.clone();
    let mut i = 2;
    while types.contains_key(&name) {
        name = format!("{base} {i}");
        i += 1;
    }
    types.insert(name.clone(), wanted);
    name
}

/// How a character is lettered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// What a part of a note overrides of its type's lettering: each of bold, italic and underline set either way, or left
/// to the type. A note keeps them when its type changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Format {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
}

impl Format {
    pub fn is_empty(&self) -> bool {
        *self == Format::default()
    }

    /// `base` with what it overrides.
    pub fn over(&self, base: Style) -> Style {
        Style {
            bold: self.bold.unwrap_or(base.bold),
            italic: self.italic.unwrap_or(base.italic),
            underline: self.underline.unwrap_or(base.underline),
        }
    }
}

/// A part of a note's text, characters `from` up to `to` (counted in characters, a line break one), lettered as its
/// format overrides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    pub from: usize,
    pub to: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
}

impl Run {
    pub fn format(&self) -> Format {
        Format { bold: self.bold, italic: self.italic, underline: self.underline }
    }
}

/// An error saying what to fix, unless `runs` are in order, each of some characters of a text `len` characters long
/// and overriding something, none overlapping.
pub fn check_runs(who: &str, runs: &[Run], len: usize) -> Result<(), String> {
    if runs.len() > MAX_RUNS {
        return Err(format!("{who}: it has {} formatting runs; a note holds up to {MAX_RUNS}", runs.len()));
    }
    let mut end = 0;
    for (i, r) in runs.iter().enumerate() {
        if r.from >= r.to || r.to > len {
            return Err(format!("{who}: run {i} is of characters {} to {}; it needs some of the text's {len}", r.from, r.to));
        }
        if r.from < end {
            return Err(format!("{who}: run {i} starts at {}, inside the run before it; give them in order, apart", r.from));
        }
        if r.format().is_empty() {
            return Err(format!("{who}: run {i} overrides nothing; give it `bold`, `italic` or `underline`"));
        }
        end = r.to;
    }
    Ok(())
}

/// Each of `len` characters' format, from `runs`.
pub fn formats(runs: &[Run], len: usize) -> Vec<Format> {
    let mut out = vec![Format::default(); len];
    for r in runs {
        for f in out.iter_mut().take(r.to.min(len)).skip(r.from) {
            *f = r.format();
        }
    }
    out
}

/// Characters' formats as runs: each stretch of characters that override the same.
pub fn runs(formats: &[Format]) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for (i, f) in formats.iter().enumerate() {
        if f.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(r) if r.to == i && r.format() == *f => r.to = i + 1,
            _ => out.push(Run { from: i, to: i + 1, bold: f.bold, italic: f.italic, underline: f.underline }),
        }
    }
    out
}

/// `runs` of text `old`, kept on what is the same in `new`: the characters before and after what changed keep theirs,
/// and what came in takes the format of the character before it.
pub fn remap(old: &str, new: &str, runs_of_old: &[Run]) -> Vec<Run> {
    let (a, b): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    let before = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let after = a[before..].iter().rev().zip(b[before..].iter().rev()).take_while(|(x, y)| x == y).count();
    let was = formats(runs_of_old, a.len());
    let typed = before.checked_sub(1).and_then(|i| was.get(i)).copied().unwrap_or_default();
    let mut now = was[..before].to_vec();
    now.extend(std::iter::repeat_n(typed, b.len() - before - after));
    now.extend_from_slice(&was[a.len() - after..]);
    runs(&now)
}

/// Each character of a note in its type `t`: its type's lettering, with what its runs override where it lets them.
pub fn styles(t: &TextType, text: &str, override_format: bool, runs: &[Run]) -> Vec<Style> {
    let len = text.chars().count();
    let base = t.style();
    if override_format { formats(runs, len).iter().map(|f| f.over(base)).collect() } else { vec![base; len] }
}

/// A letter drawn: in units of the font's capital height, x right and y up from its left end on the baseline. Its
/// `fill`'s rings fill by the non-zero rule; its `strokes` are lines `stroke` wide.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    pub fill: Vec<Vec<(f32, f32)>>,
    pub strokes: Vec<Vec<(f32, f32)>>,
    pub stroke: f32,
    pub advance: f32,
}

type Cache = Mutex<HashMap<(Face, char), Arc<Shape>>>;

/// How character `c` is drawn in `face`: from the faces after it when it has no letter for it.
pub fn shape(face: &Face, c: char) -> Arc<Shape> {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = (face.clone(), c);
    if let Some(s) = cache.lock().ok().and_then(|m| m.get(&key).cloned()) {
        return s;
    }
    let s = Arc::new(made(face, c));
    if let Ok(mut m) = cache.lock() {
        // A hostile note can't grow it without end.
        if m.len() > 20_000 {
            m.clear();
        }
        m.insert(key, s.clone());
    }
    s
}

fn made(face: &Face, c: char) -> Shape {
    if c == '\t' {
        let space = made(face, ' ');
        return Shape { advance: space.advance * TAB as f32, ..Shape::default() };
    }
    let bold = face.heavy || face.weight.is_some_and(|w| w >= 600);
    let ours = [Source::Sans, Source::Mono].map(|s| Face::bundled(s, bold, face.slant));
    let index = system::index();
    let wide = system::FALLBACKS.iter().filter_map(|f| index.family(f)).filter_map(|faces| pick(faces, bold, face.slant));
    std::iter::once(face.clone())
        .chain(ours)
        .find_map(|f| drawn(&f, c))
        .or_else(|| wide.into_iter().find_map(|f| drawn(&f, c)))
        .unwrap_or_else(|| lettered(face.heavy, '?'))
}

/// `c` in `face`, if it has a letter for it.
fn drawn(face: &Face, c: char) -> Option<Shape> {
    let file;
    let (data, index): (&[u8], u32) = match &face.source {
        Source::Lettering => return (crate::font::has(c) || c.is_whitespace()).then(|| lettered(face.heavy, c)),
        Source::Sans => (PLEX_SANS, 0),
        Source::Mono => (PLEX_MONO, 0),
        Source::File { path, index, .. } => {
            file = system::load(path)?;
            (file.as_slice(), *index)
        }
    };
    let mut parsed = ttf_parser::Face::parse(data, index).ok()?;
    if let Some(w) = face.weight {
        parsed.set_variation(ttf_parser::Tag::from_bytes(b"wght"), f32::from(w));
    }
    let glyph = parsed.glyph_index(c).filter(|g| g.0 != 0)?;
    let em = f32::from(parsed.units_per_em().max(16));
    // Its capitals' height as it says, else as its H stands (an older font's OS/2 table doesn't say), else half its em.
    let h = || parsed.glyph_index('H').and_then(|g| parsed.glyph_bounding_box(g)).map(|b| b.y_max);
    let cap = f32::from(parsed.capital_height().filter(|&h| h > 0).or_else(h).unwrap_or(0)).max(em * 0.5);
    let advance = f32::from(parsed.glyph_hor_advance(glyph).unwrap_or(0)) / cap;
    let mut out = Outline { k: 1.0 / cap, rings: Vec::new(), at: (0.0, 0.0) };
    parsed.outline_glyph(glyph, &mut out);
    let fill: Vec<Vec<(f32, f32)>> = out.rings.into_iter().filter(|r| r.len() > 2 && r.len() < 20_000).take(1000).collect();
    // A bold the family hasn't: its outline drawn round in a heavy line.
    let strokes = if face.heavy { fill.iter().map(|r| r.iter().chain(r.first()).copied().collect()).collect() } else { Vec::new() };
    let stroke = if face.heavy { 0.07 } else { 0.0 };
    Some(Shape { fill, strokes, stroke, advance: advance + stroke })
}

/// `c` in our lettering, its capital 1 high.
fn lettered(bold: bool, c: char) -> Shape {
    let s: String = c.to_string();
    let strokes = crate::font::strokes(&s, 0.0, 0.0, 12.0).into_iter().map(|l| l.into_iter().map(|(x, y)| (x / 12.0, -y / 12.0)).collect());
    let n = c.to_uppercase().count() as f32;
    Shape { fill: Vec::new(), strokes: strokes.collect(), stroke: if bold { 0.16 } else { 0.1 }, advance: n * 11.0 / 12.0 }
}

/// A glyph's outline, its curves cut into short lines.
struct Outline {
    k: f32,
    rings: Vec<Vec<(f32, f32)>>,
    at: (f32, f32),
}

impl Outline {
    fn to(&mut self, p: (f32, f32)) {
        self.at = p;
        if let Some(r) = self.rings.last_mut() {
            r.push((p.0 * self.k, p.1 * self.k));
        }
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.rings.push(Vec::new());
        self.to((x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.to((x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let a = self.at;
        for i in 1..=6 {
            let t = i as f32 / 6.0;
            let u = 1.0 - t;
            self.to((u * u * a.0 + 2.0 * u * t * x1 + t * t * x, u * u * a.1 + 2.0 * u * t * y1 + t * t * y));
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let a = self.at;
        for i in 1..=8 {
            let t = i as f32 / 8.0;
            let u = 1.0 - t;
            let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.to((b0 * a.0 + b1 * x1 + b2 * x2 + b3 * x, b0 * a.1 + b1 * y1 + b2 * y2 + b3 * y));
        }
    }
    fn close(&mut self) {}
}

/// A character laid out: the `index`-th of its note's text, `x` mm from its line's left end, `advance` mm wide.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub index: usize,
    pub ch: char,
    pub x: f64,
    pub advance: f64,
    pub style: Style,
}

/// A line of a note laid out: characters `start` up to `end` (its line break not among them), its `top` and `baseline` mm
/// down from the note's top left.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub start: usize,
    pub end: usize,
    pub top: f64,
    pub baseline: f64,
    pub width: f64,
    pub chars: Vec<Placed>,
}

/// A note laid out in its type, mm on paper, x right and y down from its first line's top left, not turned.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub font: String,
    pub size: f64,
    pub width_factor: f64,
    pub lines: Vec<Line>,
    /// How wide its widest line is, and how far down its last line's descenders go.
    pub width: f64,
    pub height: f64,
}

/// `text` laid out in type `t`, each character in its `styles` (as [`styles`] gives them).
pub fn layout(t: &TextType, text: &str, styles: &[Style]) -> Layout {
    let (font, size, wf) = (t.font.clone(), t.size, t.width_factor);
    let mut lines = Vec::new();
    let mut index = 0;
    for (i, line) in text.split('\n').enumerate() {
        let top = i as f64 * LINE_PITCH * size;
        let mut x = 0.0;
        let mut chars = Vec::new();
        for ch in line.chars() {
            let style = styles.get(index).copied().unwrap_or_default();
            let advance = f64::from(shape(&resolve(&font, style.bold, style.italic), ch).advance) * size * wf;
            chars.push(Placed { index, ch, x, advance, style });
            x += advance;
            index += 1;
        }
        lines.push(Line { start: index - chars.len(), end: index, top, baseline: top + size, width: x, chars });
        index += 1;
    }
    let width = lines.iter().map(|l| l.width).fold(0.0, f64::max);
    let height = lines.last().map_or(size, |l| l.baseline + 0.3 * size);
    Layout { font, size, width_factor: wf, lines, width, height }
}

impl Layout {
    /// Where a caret before character `index` stands: x, and its top and bottom, mm.
    pub fn caret(&self, index: usize) -> (f64, f64, f64) {
        let line = self.lines.iter().find(|l| index <= l.end).or(self.lines.last());
        let Some(l) = line else { return (0.0, 0.0, self.size) };
        let x = l.chars.iter().find(|c| c.index >= index).map_or(l.width, |c| c.x);
        (x, l.top - 0.2 * self.size, l.baseline + 0.3 * self.size)
    }

    /// The line the caret before character `index` is on.
    pub fn line_of(&self, index: usize) -> usize {
        self.lines.iter().position(|l| index <= l.end).unwrap_or(self.lines.len().saturating_sub(1))
    }

    /// The caret place nearest point `(x, y)` mm: on the line it's level with (the first above, the last below), before
    /// the character whose middle is past it.
    pub fn index_at(&self, (x, y): (f64, f64)) -> usize {
        let pitch = LINE_PITCH * self.size;
        let n = self.lines.len();
        let i = if y.is_finite() { ((y + 0.3 * self.size) / pitch).floor().clamp(0.0, n.saturating_sub(1) as f64) as usize } else { 0 };
        let Some(l) = self.lines.get(i) else { return 0 };
        l.chars.iter().find(|c| x < c.x + c.advance / 2.0).map_or(l.end, |c| c.index)
    }

    /// The boxes characters `from` up to `to` take, line by line: left, top, right, bottom, mm.
    pub fn boxes(&self, from: usize, to: usize) -> Vec<[f64; 4]> {
        let (from, to) = (from.min(to), from.max(to));
        let mut out = Vec::new();
        for l in &self.lines {
            if to <= l.start && !(from == to && from == l.start) || from > l.end {
                continue;
            }
            let x = |i: usize| l.chars.iter().find(|c| c.index >= i).map_or(l.width, |c| c.x);
            let (a, b) = (x(from.max(l.start)), x(to.min(l.end)));
            // A line break taken in shows as a sliver past its line's end.
            let b = if to > l.end && from <= l.end { b + 0.3 * self.size } else { b };
            if b > a {
                out.push([a, l.top - 0.2 * self.size, b, l.baseline + 0.3 * self.size]);
            }
        }
        out
    }

    /// The box its text takes, grown `by` mm all round: left, top, right, bottom.
    pub fn frame(&self, by: f64) -> [f64; 4] {
        [-by, -0.2 * self.size - by, self.width + by, self.height + by]
    }

    /// What is drawn of each character, mm (x right, y down from the note's top left): each letter's rings, filled by
    /// the non-zero rule, lines and how wide they are, and the underlines.
    pub fn drawn(&self) -> Drawn {
        let mut out = Drawn::default();
        let (size, wf) = (self.size as f32, self.width_factor as f32);
        for l in &self.lines {
            for c in &l.chars {
                let face = resolve(&self.font, c.style.bold, c.style.italic);
                let s = shape(&face, c.ch);
                let slant = if face.slant { SLANT } else { 0.0 };
                let (x0, base) = (c.x as f32, l.baseline as f32);
                let at = |&(gx, gy): &(f32, f32)| (x0 + (gx * wf + slant * gy) * size, base - gy * size);
                if !s.fill.is_empty() {
                    out.fill.push(s.fill.iter().map(|r| r.iter().map(at).collect()).collect());
                }
                out.strokes.extend(s.strokes.iter().map(|r| (r.iter().map(at).collect::<Vec<_>>(), s.stroke * size)));
            }
            // An underline under each stretch of underlined characters.
            let mut stretch: Option<(f64, f64)> = None;
            let y = l.baseline + 0.18 * self.size;
            for c in &l.chars {
                match (&mut stretch, c.style.underline) {
                    (Some((_, b)), true) => *b = c.x + c.advance,
                    (None, true) => stretch = Some((c.x, c.x + c.advance)),
                    (Some((a, b)), false) => {
                        out.underlines.push(((*a, y), (*b, y)));
                        stretch = None;
                    }
                    (None, false) => {}
                }
            }
            if let Some((a, b)) = stretch {
                out.underlines.push(((a, y), (b, y)));
            }
        }
        out.underline_width = 0.07 * size;
        out
    }
}

/// A layout's lettering, mm: what [`Layout::drawn`] gives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawn {
    pub fill: Vec<Vec<Vec<(f32, f32)>>>,
    pub strokes: Vec<(Vec<(f32, f32)>, f32)>,
    pub underlines: Vec<((f64, f64), (f64, f64))>,
    pub underline_width: f32,
}

/// Triangles that fill `rings` by the non-zero rule (a letter's outline, its holes wound the other way), each corner
/// counter-clockwise on paper as it is given: what a front end that fills no ring with holes draws a letter with. Cut
/// into strips at every corner's and crossing's height, each strip's spans that wind are two triangles.
pub fn triangles(rings: &[Vec<(f32, f32)>]) -> Vec<[(f32, f32); 3]> {
    // Each edge top to bottom (y down), and which way it went.
    type Edge = ((f32, f32), (f32, f32), i32);
    let mut edges: Vec<Edge> = Vec::new();
    for r in rings {
        for (i, &a) in r.iter().enumerate() {
            let b = r[(i + 1) % r.len()];
            if a.1 != b.1 && a.0.is_finite() && a.1.is_finite() && b.0.is_finite() && b.1.is_finite() {
                if a.1 < b.1 { edges.push((a, b, 1)) } else { edges.push((b, a, -1)) }
            }
        }
    }
    if edges.len() > 20_000 {
        return Vec::new();
    }
    let mut ys: Vec<f32> = edges.iter().flat_map(|e| [e.0.1, e.1.1]).collect();
    let x_at = |e: &Edge, y: f32| e.0.0 + (e.1.0 - e.0.0) * (y - e.0.1) / (e.1.1 - e.0.1);
    for (i, e) in edges.iter().enumerate() {
        for f in &edges[i + 1..] {
            let (lo, hi) = (e.0.1.max(f.0.1), e.1.1.min(f.1.1));
            if hi <= lo {
                continue;
            }
            let (d0, d1) = (x_at(e, lo) - x_at(f, lo), x_at(e, hi) - x_at(f, hi));
            if d0 * d1 < 0.0 {
                ys.push(lo + (hi - lo) * d0 / (d0 - d1));
            }
        }
    }
    ys.sort_by(f32::total_cmp);
    ys.dedup();
    let mut out = Vec::new();
    for w in ys.windows(2) {
        let (y0, y1) = (w[0], w[1]);
        if y1 - y0 <= 1e-6 {
            continue;
        }
        let mid = (y0 + y1) / 2.0;
        let mut cross: Vec<(f32, f32, f32, i32)> =
            edges.iter().filter(|e| e.0.1 <= y0 && e.1.1 >= y1).map(|e| (x_at(e, mid), x_at(e, y0), x_at(e, y1), e.2)).collect();
        cross.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut wind = 0;
        for pair in cross.windows(2) {
            wind += pair[0].3;
            if wind != 0 {
                let (l, r) = (pair[0], pair[1]);
                out.push([(l.1, y0), (l.2, y1), (r.2, y1)]);
                out.push([(l.1, y0), (r.2, y1), (r.1, y0)]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Layout {
        let t = TextType::default();
        layout(&t, text, &styles(&t, text, false, &[]))
    }

    #[test]
    fn a_note_is_laid_out_line_by_line_with_every_character_counted() {
        let l = plain("AB\nőЖ");
        assert_eq!(l.lines.len(), 2);
        assert_eq!((l.lines[0].start, l.lines[0].end, l.lines[1].start, l.lines[1].end), (0, 2, 3, 5));
        assert_eq!(l.lines[1].top, LINE_PITCH * DEFAULT_SIZE);
        assert!(l.lines[1].chars.iter().all(|c| c.advance > 0.5), "Hungarian and Cyrillic letters have widths of their own");
        assert_eq!(l.index_at((-5.0, 0.0)), 0);
        assert_eq!(l.index_at((100.0, 0.0)), 2);
        assert_eq!(l.index_at((100.0, 100.0)), 5);
        assert_eq!(l.line_of(3), 1);
        let (x, ..) = l.caret(1);
        assert_eq!(x, l.lines[0].chars[1].x);
    }

    #[test]
    fn plex_letters_have_outlines_with_holes_and_what_no_font_has_is_a_question_mark() {
        let sans = |bold| Face::bundled(Source::Sans, bold, false);
        let o = shape(&sans(false), 'O');
        assert_eq!(o.fill.len(), 2, "an O is an outline round a hole");
        let cap = o.fill.iter().flatten().map(|p| p.1).fold(f32::MIN, f32::max);
        assert!((cap - 1.0).abs() < 0.05, "a capital is the text size high: {cap}");
        assert!(shape(&sans(true), 'l').advance > shape(&sans(false), 'l').advance * 0.99);
        assert!(shape(&sans(true), 'I').fill != shape(&sans(false), 'I').fill, "bold is Plex Sans's own weight");
        // A character of the last private-use plane: no font has it.
        assert_eq!(*shape(&sans(false), '\u{10FFFD}'), lettered(false, '?'));
        // The lettering has no Σ: Plex Sans letters it.
        let lettering = Face::bundled(Source::Lettering, false, false);
        assert_eq!(*shape(&lettering, 'Σ'), *shape(&sans(false), 'Σ'));
        assert!(*shape(&sans(false), 'Σ') != lettered(false, '?'), "Plex Sans has a Σ of its own");
        assert!(shape(&lettering, 'Ő').fill.is_empty());
        assert!(shape(&Face::bundled(Source::Mono, true, false), 'A').stroke > 0.0, "Plex Mono is made bold by a heavier outline");
    }

    #[test]
    fn arial_is_lettered_by_what_stands_in_for_it_and_a_family_by_its_own_bold_and_italic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.ttf"), PLEX_SANS).unwrap();
        std::fs::write(dir.path().join("b.ttf"), PLEX_MONO).unwrap();
        let mut index = system::scan(&[dir.path().to_path_buf()]);
        // Plex Sans's file listed as Liberation Sans, a Linux system's Arial; Plex Mono's as a family with a bold face.
        let rename = |index: &mut system::Index, from: &str, to: &str| {
            let mut faces = index.families.remove(from).unwrap();
            faces.iter_mut().for_each(|f| f.family = to.into());
            index.families.insert(to.to_lowercase(), faces);
        };
        rename(&mut index, "ibm plex sans", "Liberation Sans");
        rename(&mut index, "ibm plex mono", "Test Mono");
        let regular = index.families["test mono"][0].clone();
        index.families.get_mut("test mono").unwrap().push(system::SystemFace { weight: 700, index: 7, ..regular });
        let arial = resolve_in(&index, "Arial", false, false);
        assert_eq!(arial.family(), "Liberation Sans");
        assert_eq!(shape(&arial, 'A').fill, shape(&Face::bundled(Source::Sans, false, false), 'A').fill);
        // Bold, a variable font's weight; italic, slanted: it has no italic face.
        let both = resolve_in(&index, "arial", true, true);
        assert_eq!((both.weight, both.heavy, both.slant), (Some(700), false, true));
        // A family's own bold face.
        let bold = resolve_in(&index, "Test Mono", true, false);
        assert!(matches!(bold.source, Source::File { index: 7, .. }) && !bold.heavy && bold.weight.is_none());
        assert!(resolve_in(&index, "Test Mono", false, false).source != bold.source);
        // A font no one has, and Arial where nothing stands in: Plex Sans. Ours, by name.
        assert_eq!(resolve_in(&index, "Futura", false, false).source, Source::Sans);
        assert_eq!(resolve_in(&system::Index::default(), "Arial", false, false).source, Source::Sans);
        assert_eq!(resolve_in(&index, "bimcraft LETTERING", true, false), Face::bundled(Source::Lettering, true, false));
        assert_eq!(known_in(&index, "arial").as_deref(), Some("Arial"));
        assert_eq!(known_in(&index, "liberation SANS").as_deref(), Some("Liberation Sans"));
        assert_eq!(known_in(&system::Index::default(), "Futura"), None);
    }

    #[test]
    fn a_font_that_doesnt_say_its_capitals_height_is_measured_by_its_h() {
        // Plex Sans with its OS/2 table made version 1, which has no capital height: as an older font is.
        let mut bytes = PLEX_SANS.to_vec();
        let n = usize::from(u16::from_be_bytes([bytes[4], bytes[5]]));
        let os2 = (0..n).map(|i| 12 + i * 16).find(|&r| &bytes[r..r + 4] == b"OS/2").unwrap();
        let at = u32::from_be_bytes(bytes[os2 + 8..os2 + 12].try_into().unwrap()) as usize;
        bytes[at..at + 2].copy_from_slice(&1u16.to_be_bytes());
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("old.ttf"), &bytes).unwrap();
        let index = system::scan(&[dir.path().to_path_buf()]);
        let face = pick(index.family("IBM Plex Sans").unwrap(), false, false).unwrap();
        assert!(ttf_parser::Face::parse(&bytes, 0).unwrap().capital_height().is_none());
        let h = shape(&face, 'H').fill.iter().flatten().map(|p| p.1).fold(f32::MIN, f32::max);
        assert!((h - 1.0).abs() < 0.02, "its H is the text size high: {h}");
    }

    #[test]
    fn a_family_letters_in_its_face_of_normal_width_before_a_narrow_one_listed_first() {
        // Windows' Arial Narrow calls its family Arial (its typographic family), a face as regular as Arial's own.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.ttf"), PLEX_SANS).unwrap();
        let mut index = system::scan(&[dir.path().to_path_buf()]);
        let mut faces = index.families.remove("ibm plex sans").unwrap();
        faces.iter_mut().for_each(|f| f.family = "Arial".into());
        let normal = faces[0].clone();
        assert_eq!(normal.width, 5, "Plex Sans is of normal width");
        let narrow = system::SystemFace { width: 3, index: 9, ..normal.clone() };
        index.families.insert("arial".into(), vec![narrow.clone(), normal]);
        assert!(matches!(resolve_in(&index, "Arial", false, false).source, Source::File { index: 0, .. }));
        assert!(matches!(resolve_in(&index, "Arial", true, false).source, Source::File { index: 0, .. }));
        // Narrow alone, it's what there is.
        index.families.insert("arial".into(), vec![narrow]);
        assert!(matches!(resolve_in(&index, "Arial", false, false).source, Source::File { index: 9, .. }));
    }

    #[test]
    fn runs_override_where_the_note_lets_them_and_keep_their_place_when_the_text_changes() {
        let t = TextType { bold: true, ..TextType::default() };
        let r = vec![Run { from: 1, to: 3, bold: Some(false), italic: Some(true), underline: None }];
        assert!(styles(&t, "abcd", false, &r).iter().all(|s| s.bold && !s.italic));
        let s = styles(&t, "abcd", true, &r);
        assert_eq!((s[0].bold, s[1].bold, s[1].italic, s[3].bold), (true, false, true, true));
        assert_eq!(runs(&formats(&r, 4)), r);
        // Typed inside the run, it grows; typed before it, it moves along.
        assert_eq!(remap("abcd", "abXcd", &r)[0], Run { to: 4, ..r[0] });
        assert_eq!(remap("abcd", "XXabcd", &r)[0], Run { from: 3, to: 5, ..r[0] });
        assert!(remap("abcd", "ad", &r).is_empty());
        assert!(check_runs("t", &r, 4).is_ok());
        assert!(check_runs("t", &r, 2).is_err());
        assert!(check_runs("t", &[r[0], r[0]], 4).is_err());
        assert!(check_runs("t", &[Run { from: 0, to: 1, bold: None, italic: None, underline: None }], 4).is_err());
    }

    #[test]
    fn underlines_run_under_each_underlined_stretch() {
        let t = TextType::default();
        let r = [Run { from: 0, to: 2, bold: None, italic: None, underline: Some(true) }];
        let l = layout(&t, "ab cd", &styles(&t, "ab cd", true, &r));
        let d = l.drawn();
        assert_eq!(d.underlines.len(), 1);
        assert_eq!(d.underlines[0].1.0, l.lines[0].chars[2].x);
    }

    #[test]
    fn a_letter_with_a_hole_fills_round_it() {
        let square = |a: f32, b: f32, ccw: bool| {
            let r = vec![(a, a), (b, a), (b, b), (a, b)];
            if ccw { r } else { r.into_iter().rev().collect() }
        };
        let area = |t: &[[(f32, f32); 3]]| {
            t.iter().map(|[a, b, c]| ((b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)).abs() / 2.0).sum::<f32>()
        };
        let t = triangles(&[square(0.0, 10.0, true), square(3.0, 7.0, false)]);
        assert!((area(&t) - 84.0).abs() < 1e-3, "{}", area(&t));
        // Two outlines wound the same way, overlapping, as a variable font's are: their union.
        let t = triangles(&[square(0.0, 10.0, true), square(5.0, 15.0, true)]);
        assert!((area(&t) - 175.0).abs() < 1e-3, "{}", area(&t));
    }
}
