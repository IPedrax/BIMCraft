//! Title blocks of the project's own, as Revit's title block families with their labels: lines, solid fills and lettering in
//! paper millimetres, drawn in a sheet's bottom right corner inside its border in place of the built-in title box. Lettering
//! may hold fields, `{KEY}`, which a sheet fills in when it's drawn: a field of the project's (`scope: project`, one value for
//! every sheet, `Model::title_values`), a field of each sheet's (`scope: sheet`, `Sheet::fields`), or one of the automatic
//! ones ([`AUTO`]: the project's name, the sheet's name, number and scale, the date, the sheet's place among the sheets, its
//! revision). A title block is made by commands, or read from a DXF drawing of one ([`from_dxf`]). This module knows the
//! title block and how it fills in; `sheet.rs` draws it.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::dxf::{self, Dxf, Horizontal, Shape};
use crate::geom::P2;
use crate::language::Language;
use crate::model::{Element, Key, Model};
use crate::pen::{MAX_PEN, Pen};
use crate::render::{Drawing, Rgb};

/// The widest and tallest a title block may be, mm: an A0's long side.
pub const MAX_SIZE: f64 = 1189.0;
/// The most lines, fills and texts one title block may hold, together.
pub const MAX_ITEMS: usize = 20_000;
/// The most points one line or fill of it may have.
pub const MAX_POINTS: usize = 10_000;
/// The most fields one title block may define.
pub const MAX_FIELDS: usize = 200;
/// The longest a field's key may be.
pub const MAX_KEY: usize = 40;
/// The longest a text or a field's value may be.
pub const MAX_TEXT: usize = 1024;
/// The smallest and largest lettering, mm on paper.
pub const MIN_LETTERING: f64 = 0.5;
pub const MAX_LETTERING: f64 = 100.0;

/// The fields every title block has without defining them, each with what it gives, as `title_block` lists them.
pub const AUTO: &[(&str, &str)] = &[
    ("PROJECT", "the project's name"),
    ("SHEET_NAME", "the sheet's name"),
    ("SHEET_NUMBER", "the sheet's number"),
    ("SCALE", "the scale of the views on it, as the built-in title box says it"),
    ("DATE", "today's date, as the project's language writes dates"),
    ("SHEET_INDEX", "the sheet's place among the project's sheets, by number, from 1 (two digits)"),
    ("SHEET_COUNT", "how many sheets the project has (two digits)"),
    ("REVISION", "the latest revision of the revision clouds on the sheet and on the views placed on it"),
];

/// Whose a field is: the project's, one value for every sheet, or each sheet's own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    #[default]
    Sheet,
    Project,
}

/// Which way a text runs from its point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// A field the user fills in: its key, as the lettering names it (`{CLIENT}`), its label, as the desktop shows it, whose it
/// is, and what it says until it's given a value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(default)]
    pub scope: Scope,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default: String,
}

impl Field {
    /// Its label, else its key.
    pub fn shown(&self) -> &str {
        if self.label.trim().is_empty() { &self.key } else { &self.label }
    }
}

/// A line of it: points in mm from the title block's bottom left corner, closed or not, in a pen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stroke {
    pub points: Vec<P2>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub closed: bool,
    #[serde(default = "fine")]
    pub pen: Pen,
}

fn fine() -> Pen {
    Pen::FINE
}

/// A filled area of it: rings in mm from its bottom left corner, filled through (a logo's).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fill {
    pub rings: Vec<Vec<P2>>,
}

/// A line of lettering: where it starts (or is centred, or ends), mm from its bottom left corner, on its baseline; its
/// capitals' height, mm; its angle, degrees counter-clockwise; and its text, which may name fields as `{KEY}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Text {
    pub at: P2,
    pub text: String,
    #[serde(default = "lettering")]
    pub size: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub angle: f64,
    #[serde(default)]
    pub align: Align,
}

fn lettering() -> f64 {
    2.5
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

/// A title block: its box, `width` by `height` mm, which stands in the sheet's bottom right corner inside its border, and
/// what it draws there; and the fields its lettering fills in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TitleBlock {
    pub width: f64,
    pub height: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<Stroke>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fills: Vec<Fill>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<Text>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Field>,
}

impl Default for TitleBlock {
    /// The starter: the built-in title box's size (NBR 16752's 178 mm wide), its outline and a rule, and the sheet's name,
    /// number and scale, and the project's.
    fn default() -> TitleBlock {
        let (w, h) = (178.0, 40.0);
        TitleBlock {
            width: w,
            height: h,
            lines: vec![
                Stroke { points: vec![P2(0.0, 0.0), P2(w, 0.0), P2(w, h), P2(0.0, h)], closed: true, pen: Pen::BORDER },
                Stroke { points: vec![P2(0.0, 26.0), P2(w, 26.0)], closed: false, pen: Pen::PROJECTION },
                Stroke { points: vec![P2(w - 50.0, 0.0), P2(w - 50.0, 26.0)], closed: false, pen: Pen::PROJECTION },
            ],
            fills: Vec::new(),
            texts: vec![
                Text { at: P2(4.0, 30.5), text: "{PROJECT}".into(), size: 5.0, angle: 0.0, align: Align::Left },
                Text { at: P2(4.0, 15.25), text: "{SHEET_NAME}".into(), size: 3.5, angle: 0.0, align: Align::Left },
                Text { at: P2(4.0, 6.0), text: "{SCALE}".into(), size: 2.0, angle: 0.0, align: Align::Left },
                Text { at: P2(w - 25.0, 9.5), text: "{SHEET_NUMBER}".into(), size: 7.0, angle: 0.0, align: Align::Center },
            ],
            fields: Vec::new(),
        }
    }
}

/// Whether `key` may name a field: letters, digits and `_`, starting with a letter, up to [`MAX_KEY`].
pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_KEY
        && key.starts_with(|c: char| c.is_ascii_alphabetic())
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether `key` is one of the automatic fields.
pub fn is_auto(key: &str) -> bool {
    AUTO.iter().any(|(k, _)| k.eq_ignore_ascii_case(key))
}

/// The keys `text` names, `{KEY}`, in the order it names them, uppercase.
pub fn keys_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if valid_key(&after[..close]) => {
                out.push(after[..close].to_ascii_uppercase());
                rest = &after[close + 1..];
            }
            _ => rest = after,
        }
    }
    out
}

/// `text` with each `{KEY}` that `values` has (by its uppercase key) put in its place; a key it hasn't stays as written.
pub fn fill(text: &str, values: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if valid_key(&after[..close]) => {
                let key = &after[..close];
                match values.get(&key.to_ascii_uppercase()) {
                    Some(v) => out.push_str(v),
                    None => {
                        out.push('{');
                        out.push_str(key);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

impl TitleBlock {
    /// The fields its lettering names that it doesn't define and that aren't automatic, uppercase.
    pub fn undefined(&self) -> BTreeSet<String> {
        let defined: BTreeSet<String> = self.fields.iter().map(|f| f.key.to_ascii_uppercase()).collect();
        self.texts.iter().flat_map(|t| keys_in(&t.text)).filter(|k| !is_auto(k) && !defined.contains(k)).collect()
    }

    /// The field of `key`, by its uppercase key.
    pub fn field(&self, key: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.key.eq_ignore_ascii_case(key))
    }

    /// Why it isn't one `name` may be, if it isn't.
    pub fn check(&self, name: &str) -> Result<(), String> {
        let who = format!("title block `{name}`");
        let size = |what: &str, x: f64| {
            if x.is_finite() && x > 0.0 && x <= MAX_SIZE {
                Ok(())
            } else {
                Err(format!("{who}: its {what} is {x} mm; it's more than 0, up to {MAX_SIZE}"))
            }
        };
        size("width", self.width)?;
        size("height", self.height)?;
        let items = self.lines.len() + self.fills.len() + self.texts.len();
        if items > MAX_ITEMS {
            return Err(format!("{who}: it holds {items} lines, fills and texts; up to {MAX_ITEMS}"));
        }
        // A point may stand a little past its box (a logo's edge), not anywhere at all.
        let near = |p: &P2| p.0.is_finite() && p.1.is_finite() && p.0.abs() <= 2.0 * MAX_SIZE && p.1.abs() <= 2.0 * MAX_SIZE;
        for (i, l) in self.lines.iter().enumerate() {
            if l.points.len() < 2 || l.points.len() > MAX_POINTS {
                return Err(format!("{who}: line {i} has {} points; it has 2 to {MAX_POINTS}", l.points.len()));
            }
            if !l.points.iter().all(near) {
                return Err(format!("{who}: line {i} has a point that isn't a number, or lies far past the title block"));
            }
            if l.pen.0 > MAX_PEN {
                return Err(format!("{who}: line {i} is in pen {}; pens are 0 to {MAX_PEN}", l.pen.0));
            }
        }
        for (i, f) in self.fills.iter().enumerate() {
            if f.rings.is_empty() || f.rings.iter().any(|r| r.len() < 3 || r.len() > MAX_POINTS) {
                return Err(format!("{who}: fill {i} needs rings of 3 to {MAX_POINTS} points"));
            }
            if !f.rings.iter().flatten().all(near) {
                return Err(format!("{who}: fill {i} has a point that isn't a number, or lies far past the title block"));
            }
        }
        for (i, t) in self.texts.iter().enumerate() {
            if !near(&t.at) {
                return Err(format!("{who}: text {i} stands at a point that isn't a number, or lies far past the title block"));
            }
            if !(MIN_LETTERING..=MAX_LETTERING).contains(&t.size) {
                return Err(format!("{who}: text {i} is {} mm tall; lettering is {MIN_LETTERING} to {MAX_LETTERING} mm", t.size));
            }
            if !t.angle.is_finite() {
                return Err(format!("{who}: text {i}'s angle isn't a number"));
            }
            if t.text.chars().count() > MAX_TEXT || t.text.contains(['\n', '\r']) {
                return Err(format!("{who}: text {i} is one line of up to {MAX_TEXT} characters"));
            }
        }
        if self.fields.len() > MAX_FIELDS {
            return Err(format!("{who}: it defines {} fields; up to {MAX_FIELDS}", self.fields.len()));
        }
        let mut seen = BTreeSet::new();
        for f in &self.fields {
            if !valid_key(&f.key) {
                return Err(format!(
                    "{who}: field `{}` isn't a key: letters, digits and `_`, starting with a letter, up to {MAX_KEY}",
                    f.key
                ));
            }
            if is_auto(&f.key) {
                return Err(format!("{who}: `{}` is an automatic field; it needn't be defined", f.key));
            }
            if !seen.insert(f.key.to_ascii_uppercase()) {
                return Err(format!("{who}: field `{}` is defined twice", f.key));
            }
            if f.label.chars().count() > crate::model::MAX_TEXT || f.default.chars().count() > MAX_TEXT {
                return Err(format!("{who}: field `{}`'s label or default is too long", f.key));
            }
        }
        Ok(())
    }

    /// Its lines, fills and texts moved by `by`, mm.
    fn moved(&mut self, by: P2) {
        self.lines.iter_mut().flat_map(|l| l.points.iter_mut()).for_each(|p| *p = *p + by);
        self.fills.iter_mut().flat_map(|f| f.rings.iter_mut().flatten()).for_each(|p| *p = *p + by);
        self.texts.iter_mut().for_each(|t| t.at = t.at + by);
    }
}

/// Why `values`, fields' values by key, aren't (`who`'s, as the error names them), if they aren't: each key a key, uppercase,
/// each value one line of up to [`MAX_TEXT`] characters, up to [`MAX_FIELDS`] of them.
pub fn check_values(who: &str, values: &BTreeMap<String, String>) -> Result<(), String> {
    if values.len() > MAX_FIELDS {
        return Err(format!("{who}: {} values; up to {MAX_FIELDS}", values.len()));
    }
    for (k, v) in values {
        if !valid_key(k) || k.to_ascii_uppercase() != *k {
            return Err(format!("{who}: `{k}` isn't a field's key: capitals, digits and `_`, starting with a letter, up to {MAX_KEY}"));
        }
        if v.chars().count() > MAX_TEXT || v.contains(['\n', '\r']) {
            return Err(format!("{who}: `{k}` is one line of up to {MAX_TEXT} characters"));
        }
    }
    Ok(())
}

/// Today's date, as `language` writes dates: day, month and year, ISO's year, month and day in English and Swedish.
pub fn today(language: Language) -> String {
    let days = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86_400);
    let (y, m, d) = civil(i64::try_from(days).unwrap_or(0));
    date(language, y, m, d)
}

/// Year, month and day as `language` writes them.
pub fn date(language: Language, y: i64, m: u32, d: u32) -> String {
    match language {
        Language::English | Language::Swedish => format!("{y:04}-{m:02}-{d:02}"),
        Language::German | Language::Czech => format!("{d:02}.{m:02}.{y:04}"),
        Language::Dutch => format!("{d:02}-{m:02}-{y:04}"),
        _ => format!("{d:02}/{m:02}/{y:04}"),
    }
}

/// The year, month and day `days` days after 1 January 1970 (Howard Hinnant's civil-from-days).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// The latest of `revisions`: the longest, then the last in order, so `Rev 10` comes after `Rev 9` and `C` after `B`.
fn latest<'a>(revisions: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    revisions.filter(|r| !r.trim().is_empty()).max_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)))
}

/// What every field of sheet `id`'s title block says, by uppercase key: the automatic ones (`scale` is its scale as the
/// built-in title box gives it), then each field's default, the project's value of a project field and the sheet's of a
/// sheet field over it. A sheet's or the project's value of an automatic field (`DATE`, say) is given over the automatic one.
pub fn values(m: &Model, id: impl Key, scale: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(sheet) = m.sheet(id) else { return out };
    let guid = m.guid(id);
    let mut numbers: Vec<&str> =
        m.elements.values().filter_map(|e| if let Element::Sheet(s) = e { Some(s.number.as_str()) } else { None }).collect();
    numbers.sort_by(|a, b| natural(a, b));
    let index = numbers.iter().position(|n| *n == sheet.number).map_or(0, |i| i + 1);
    // Its views', and its own: the clouds on it and on what it shows.
    let views: BTreeSet<crate::ids::Guid> = m
        .on_sheet(id)
        .into_iter()
        .filter_map(|v| match m.elements.get(v) {
            Some(Element::Viewport(vp)) => Some(vp.view),
            _ => None,
        })
        .chain(std::iter::once(guid))
        .collect();
    let revision = latest(m.elements.values().filter_map(|e| match e {
        Element::DetailLine(l) if l.cloud.is_some() && views.contains(&l.view) => l.revision.as_deref(),
        _ => None,
    }));
    for (k, v) in [
        ("PROJECT", m.name.clone()),
        ("SHEET_NAME", sheet.name.clone()),
        ("SHEET_NUMBER", sheet.number.clone()),
        ("SCALE", scale.to_string()),
        ("DATE", today(m.language)),
        ("SHEET_INDEX", format!("{index:02}")),
        ("SHEET_COUNT", format!("{:02}", numbers.len())),
        ("REVISION", revision.unwrap_or_default().to_string()),
    ] {
        out.insert(k.to_string(), v);
    }
    let tb = m.title_blocks.get(&sheet.title_block);
    for f in tb.iter().flat_map(|t| t.fields.iter()) {
        let key = f.key.to_ascii_uppercase();
        let given = match f.scope {
            Scope::Project => m.title_values.get(&key),
            Scope::Sheet => sheet.fields.get(&key),
        };
        out.insert(key, given.filter(|v| !v.is_empty()).unwrap_or(&f.default).clone());
    }
    // An automatic field given a value, the project's or the sheet's (a date to issue on), says that.
    for (k, _) in AUTO {
        if let Some(v) = sheet.fields.get(*k).or_else(|| m.title_values.get(*k)).filter(|v| !v.is_empty()) {
            out.insert((*k).to_string(), v.clone());
        }
    }
    out
}

/// Sheet numbers in the order people read them: `A2` before `A10`.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let split = |s: &str| {
        let digits = s.trim_end_matches(|c: char| !c.is_ascii_digit());
        let start = digits.rfind(|c: char| !c.is_ascii_digit()).map_or(0, |i| i + 1);
        (s[..start].to_string(), digits[start..].parse::<u64>().unwrap_or(0), s.to_string())
    };
    split(a).cmp(&split(b))
}

const INK: Rgb = Rgb(25, 25, 25);

/// Title block `tb` drawn on sheet drawing `d` with its bottom left corner at paper point `at`, mm, its fields filled in
/// from `values`.
pub fn draw(d: &mut Drawing, tb: &TitleBlock, at: P2, values: &BTreeMap<String, String>) {
    let density = d.view.scale;
    for f in &tb.fills {
        let rings: Vec<Vec<P2>> = f.rings.iter().map(|r| r.iter().map(|p| *p + at).collect()).collect();
        d.fill(&rings, INK);
    }
    for l in &tb.lines {
        let pts: Vec<P2> = l.points.iter().map(|p| *p + at).collect();
        d.line(&pts, l.pen, INK, l.closed);
    }
    for t in &tb.texts {
        let text = fill(&t.text, values);
        if text.trim().is_empty() {
            continue;
        }
        let h = (t.size * density) as f32;
        let w = crate::font::width(&text, h);
        // From the point along the baseline to the text's middle, by its alignment, then turned.
        let along = match t.align {
            Align::Left => w / 2.0,
            Align::Center => 0.0,
            Align::Right => -w / 2.0,
        };
        let angle = -(t.angle.to_radians() as f32);
        let (s, c) = angle.sin_cos();
        let (x, y) = d.view.px(t.at + at);
        // Up the page is minus y in pixels: the middle of the capitals is half their height above the baseline.
        let up = -h / 2.0;
        d.text_turned(&text, (x + along * c - up * s, y + along * s + up * c), h, angle, INK);
    }
}

/// Title block `tb` on its own, filled in from `values`, as a sheet draws it: what a preview of it shows, `ROOM` mm clear
/// round it, at [`crate::sheet::DENSITY`] px a paper mm, in model `m`'s annotation pens.
pub fn preview(m: &Model, tb: &TitleBlock, values: &BTreeMap<String, String>) -> Drawing {
    const ROOM: f64 = 2.0;
    let density = crate::sheet::DENSITY;
    let side = |mm: f64| ((mm.clamp(1.0, MAX_SIZE) + 2.0 * ROOM) * density).round() as u32;
    let view =
        crate::render::View { scale: density, cx: tb.width / 2.0, cy: tb.height / 2.0, width: side(tb.width), height: side(tb.height) };
    let mut d = Drawing::new(view);
    d.weights = m.line_weights().at(m.units.default_scale());
    draw(&mut d, tb, P2(0.0, 0.0), values);
    d.weigh(|_| true);
    d
}

/// A title block read from DXF drawing `dxf`, whose unit is `mm` millimetres (a drawing of the title block at full size on
/// paper: 1 for one drawn in mm, 10 in cm): its lines, arcs and circles as lines, its solid hatches as fills, its texts as
/// texts, moved so its extents' bottom left corner is the title block's; its box, those extents; and its fields, each key its
/// texts name that isn't automatic, as each sheet's. And what was left out, counted by kind.
pub fn from_dxf(dxf: &Dxf, mm: f64) -> (TitleBlock, BTreeMap<String, usize>) {
    let mut tb = TitleBlock { width: 0.0, height: 0.0, lines: Vec::new(), fills: Vec::new(), texts: Vec::new(), fields: Vec::new() };
    let mut left_out: BTreeMap<String, usize> = BTreeMap::new();
    let k = |p: &P2| P2(p.0 * mm, p.1 * mm);
    let mut lo = P2(f64::INFINITY, f64::INFINITY);
    let mut hi = P2(f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut grow = |p: P2| {
        lo = P2(lo.0.min(p.0), lo.1.min(p.1));
        hi = P2(hi.0.max(p.0), hi.1.max(p.1));
    };
    for e in &dxf.entities {
        let hidden = dxf.layer(&e.layer).is_some_and(|l| l.off || l.frozen);
        if hidden {
            *left_out.entry("on a layer turned off or frozen".into()).or_default() += 1;
            continue;
        }
        let weight = e.weight.or_else(|| dxf.layer(&e.layer).and_then(|l| l.weight));
        let pen = weight.map_or(Pen::FINE, Pen::nearest);
        match &e.shape {
            Shape::Text { at, height, angle, text, align } => {
                let at = k(at);
                grow(at);
                let align = match align.h {
                    Horizontal::Left => Align::Left,
                    Horizontal::Center => Align::Center,
                    Horizontal::Right => Align::Right,
                };
                let size = (height * mm).clamp(MIN_LETTERING, MAX_LETTERING);
                let text: String = text.chars().filter(|c| *c != '\n' && *c != '\r').take(MAX_TEXT).collect();
                tb.texts.push(Text { at, text, size, angle: *angle, align });
            }
            Shape::Fill { rings, solid: true } => {
                let rings: Vec<Vec<P2>> = rings.iter().filter(|r| r.len() >= 3).map(|r| r.iter().map(k).collect()).collect();
                rings.iter().flatten().for_each(|p| grow(*p));
                if !rings.is_empty() {
                    tb.fills.push(Fill { rings });
                }
            }
            Shape::Fill { solid: false, .. } => *left_out.entry("patterned hatch".into()).or_default() += 1,
            Shape::Point(_) => *left_out.entry("point".into()).or_default() += 1,
            shape => {
                for (pts, closed) in crate::cad::paths(shape) {
                    let points: Vec<P2> = pts.iter().map(k).collect();
                    points.iter().for_each(|p| grow(*p));
                    if points.len() >= 2 {
                        tb.lines.push(Stroke { points, closed, pen });
                    }
                }
            }
        }
    }
    for (what, n) in &dxf.left_out {
        *left_out.entry(what.clone()).or_default() += n;
    }
    if lo.0.is_finite() && hi.0.is_finite() {
        tb.width = hi.0 - lo.0;
        tb.height = hi.1 - lo.1;
        tb.moved(P2(-lo.0, -lo.1));
    }
    let keys: BTreeSet<String> = tb.texts.iter().flat_map(|t| keys_in(&t.text)).filter(|k| !is_auto(k)).collect();
    tb.fields = keys.into_iter().map(|key| Field { key, label: String::new(), scope: Scope::Sheet, default: String::new() }).collect();
    (tb, left_out)
}

/// [`from_dxf`] of DXF text, a title block drawn in `units` (`auto`: the file's own, else mm).
pub fn read_dxf(bytes: &[u8], units: Option<f64>) -> Result<(TitleBlock, BTreeMap<String, usize>), String> {
    let dxf = dxf::read(bytes)?;
    let mm = units.or(dxf.scale).unwrap_or(1.0);
    Ok(from_dxf(&dxf, mm))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_found_and_filled_in() {
        assert_eq!(keys_in("Client: {client} — {SHEET_NUMBER}/{SHEET_COUNT} {not a key} {}"), ["CLIENT", "SHEET_NUMBER", "SHEET_COUNT"]);
        let values = BTreeMap::from([("CLIENT".to_string(), "ACME".to_string())]);
        assert_eq!(fill("Client: {client} {NOPE} {x y} {", &values), "Client: ACME {NOPE} {x y} {");
    }

    #[test]
    fn dates_are_written_as_the_language_writes_them() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_399), (2025, 11, 7));
        assert_eq!(civil(11_016), (2000, 2, 29));
        assert_eq!(date(Language::Portuguese, 2026, 11, 7), "07/11/2026");
        assert_eq!(date(Language::English, 2026, 11, 7), "2026-11-07");
        assert_eq!(date(Language::German, 2026, 11, 7), "07.11.2026");
    }

    #[test]
    fn the_latest_revision_is_the_longest_then_the_last() {
        assert_eq!(latest(["Rev 2", "Rev 10", "Rev 9"].into_iter()), Some("Rev 10"));
        assert_eq!(latest(["A", "C", "B", " "].into_iter()), Some("C"));
        assert_eq!(latest(std::iter::empty()), None);
    }

    #[test]
    fn sheet_numbers_sort_as_read() {
        let mut n = vec!["A10", "A2", "A1", "B1"];
        n.sort_by(|a, b| natural(a, b));
        assert_eq!(n, ["A1", "A2", "A10", "B1"]);
    }

    #[test]
    fn a_title_block_is_checked() {
        assert!(TitleBlock::default().check("T").is_ok());
        let mut tb = TitleBlock::default();
        tb.fields.push(Field { key: "client".into(), label: String::new(), scope: Scope::Project, default: String::new() });
        tb.fields.push(Field { key: "CLIENT".into(), label: String::new(), scope: Scope::Sheet, default: String::new() });
        assert!(tb.check("T").unwrap_err().contains("defined twice"));
        let mut tb = TitleBlock::default();
        tb.fields.push(Field { key: "DATE".into(), label: String::new(), scope: Scope::Sheet, default: String::new() });
        assert!(tb.check("T").unwrap_err().contains("automatic"));
        let tb = TitleBlock { width: 0.0, ..TitleBlock::default() };
        assert!(tb.check("T").unwrap_err().contains("width"));
    }

    #[test]
    fn a_dxf_title_block_comes_in_at_its_corner_with_its_fields() {
        let dxf = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
                   0\nLINE\n8\nCT_SELO\n10\n100\n20\n50\n11\n280\n21\n50\n\
                   0\nLINE\n8\nCT_SELO\n10\n100\n20\n100\n11\n280\n21\n100\n\
                   0\nTEXT\n8\nCT_TEXTO\n10\n105\n20\n90\n40\n3\n1\nCliente: {CLIENTE}\n\
                   0\nTEXT\n8\nCT_TEXTO\n10\n105\n20\n60\n40\n2.5\n1\nFolha {SHEET_INDEX}/{SHEET_COUNT}\n\
                   0\nENDSEC\n0\nEOF\n";
        let (tb, _) = read_dxf(dxf.as_bytes(), None).unwrap();
        assert_eq!((tb.width, tb.height), (180.0, 50.0));
        assert_eq!(tb.lines.len(), 2);
        assert_eq!(tb.lines[0].points, [P2(0.0, 0.0), P2(180.0, 0.0)]);
        assert_eq!(tb.texts[0].at, P2(5.0, 40.0));
        assert_eq!(tb.fields.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), ["CLIENTE"]);
        assert_eq!(tb.texts[0].text, "Cliente: {CLIENTE}", "a single line's braces are its own");
        assert!(tb.check("CIVILTEC").is_ok());
        // In cm, it's ten times the size.
        let (tb, _) = read_dxf(dxf.as_bytes(), Some(10.0)).unwrap();
        assert_eq!((tb.width, tb.height), (1800.0, 500.0));
    }
}
