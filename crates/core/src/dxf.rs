//! DXF, read from the public DXF reference: a drawing's layers and the entities of its model space, each in the
//! drawing's own unit, which `$INSUNITS` names when the file knows it. What a plan can't draw as it is (a polyline's
//! bulges, an ellipse, a spline) is flattened to points on the curve; a block is placed where each INSERT puts it, arrays
//! and nesting included, under a bound; paper space, a binary file and the kinds of entity the reader doesn't draw are
//! left out and named. A `CadImport` element places what is read; this module knows no element.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

use crate::geom::P2;
use crate::render::Rgb;

/// What a file may be before it's refused, bytes.
pub const MAX_BYTES: usize = 128 << 20;
/// The most shapes a drawing may hold once its blocks are placed.
pub const MAX_SHAPES: usize = 1_000_000;
/// How deep a block may be inside a block.
pub const MAX_DEPTH: usize = 16;
/// How far a file's coordinates may lie from its origin, in its own unit.
pub const MAX_FILE_COORD: f64 = 1e12;
/// The most entities visited placing blocks (an array of a block of arrays): a bound on hostile nesting.
const MAX_WORK: usize = 4_000_000;
/// The most values one entity may hold: a polyline's vertices, a hatch's edges.
const MAX_PAIRS: usize = 2_000_000;
/// The most rows, and the most columns, an INSERT's array may have.
const MAX_ARRAY: usize = 10_000;
/// The most segments a flattened arc, circle or ellipse gets.
const CURVE_STEPS: usize = 64;
/// Segments a spline gets in each of its spans, and the most in all.
const SPLINE_STEPS: usize = 8;
const MAX_SPLINE_POINTS: usize = 2048;
/// The most characters of one text.
const MAX_CHARS: usize = 4096;

/// A colour as an entity or a layer names it: the standard index, a true colour, or its layer's or its block's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    ByLayer,
    ByBlock,
    Index(u8),
    True(u8, u8, u8),
}

impl Color {
    /// The colour itself, when it is one: none for the drawing's ink (index 7, black on white paper) and for a colour
    /// left to the layer or the block.
    pub fn rgb(self) -> Option<Rgb> {
        match self {
            Color::Index(i) => aci(i),
            Color::True(r, g, b) => Some(Rgb(r, g, b)),
            Color::ByLayer | Color::ByBlock => None,
        }
    }
}

/// The standard colour index: the nine named colours, then 24 hues in five shades, each full and pale, then six greys.
/// Index 7 is the drawing's ink (black on white paper, white on black), so it is none here; 0 is by block and 256 by
/// layer, neither an index.
pub fn aci(i: u8) -> Option<Rgb> {
    Some(match i {
        1 => Rgb(255, 0, 0),
        2 => Rgb(255, 255, 0),
        3 => Rgb(0, 255, 0),
        4 => Rgb(0, 255, 255),
        5 => Rgb(0, 0, 255),
        6 => Rgb(255, 0, 255),
        8 => Rgb(128, 128, 128),
        9 => Rgb(192, 192, 192),
        10..=249 => {
            let (hue, shade) = (f64::from((i - 10) / 10) * 15.0, (i - 10) % 10);
            let high = match shade / 2 {
                0 => 255.0,
                1 => 204.0,
                2 => 153.0,
                3 => 127.0,
                _ => 76.0,
            };
            let low = if shade % 2 == 1 { high / 2.0 } else { 0.0 };
            hue_rgb(hue, high, low)
        }
        250..=255 => {
            let g = match i - 250 {
                0 => 51,
                1 => 91,
                2 => 132,
                3 => 173,
                4 => 214,
                _ => 255,
            };
            Rgb(g, g, g)
        }
        _ => return None,
    })
}

/// The colour at `hue` degrees whose brightest channel is `high` and dimmest `low`: six sectors of 60°, the middle
/// channel ramping between them.
fn hue_rgb(hue: f64, high: f64, low: f64) -> Rgb {
    let sector = (hue / 60.0).floor();
    let f = hue / 60.0 - sector;
    let (down, up) = (high - (high - low) * f, low + (high - low) * f);
    let (r, g, b) = match sector as i64 % 6 {
        0 => (high, up, low),
        1 => (down, high, low),
        2 => (low, high, up),
        3 => (low, down, high),
        4 => (up, low, high),
        _ => (high, low, down),
    };
    Rgb(r as u8, g as u8, b as u8)
}

/// A layer of the drawing, as its LAYER table holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    /// Its colour; an entity whose colour is by layer takes it.
    pub color: Color,
    /// Its linetype's name (`Continuous` when the file names none).
    pub linetype: String,
    /// Its line weight, mm, or none for the default.
    pub weight: Option<f64>,
    /// Turned off: its entities aren't shown.
    pub off: bool,
    /// Frozen: neither shown nor regenerated.
    pub frozen: bool,
}

/// A linetype: its dashes in drawing units (a gap is negative, a dot zero), to be scaled by the drawing's `ltscale`.
#[derive(Clone, Debug, PartialEq)]
pub struct Linetype {
    pub name: String,
    pub dashes: Vec<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Horizontal {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vertical {
    Baseline,
    Bottom,
    Middle,
    Top,
}

/// Where a text's point sits on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Align {
    pub h: Horizontal,
    pub v: Vertical,
}

/// What an entity draws, in drawing units.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Line(P2, P2),
    Polyline {
        pts: Vec<P2>,
        closed: bool,
    },
    Circle {
        c: P2,
        r: f64,
    },
    /// Counter-clockwise from `start` to `end`, degrees.
    Arc {
        c: P2,
        r: f64,
        start: f64,
        end: f64,
    },
    Point(P2),
    /// One line of lettering: `height` is its capital height, `angle` its baseline's in degrees counter-clockwise from east.
    Text {
        at: P2,
        height: f64,
        angle: f64,
        text: String,
        align: Align,
    },
    /// A hatch's boundaries, or a solid: `solid` when filled through, else patterned, which a view draws as it can.
    Fill {
        rings: Vec<Vec<P2>>,
        solid: bool,
    },
}

impl Shape {
    /// Whether every number of it is finite and within [`MAX_FILE_COORD`].
    fn fits(&self) -> bool {
        let ok = |x: f64| x.is_finite() && x.abs() <= MAX_FILE_COORD;
        let p = |p: &P2| ok(p.0) && ok(p.1);
        match self {
            Shape::Line(a, b) => p(a) && p(b),
            Shape::Polyline { pts, .. } => pts.iter().all(p),
            Shape::Circle { c, r } => p(c) && ok(*r),
            Shape::Arc { c, r, start, end } => p(c) && ok(*r) && ok(*start) && ok(*end),
            Shape::Point(q) => p(q),
            Shape::Text { at, height, angle, .. } => p(at) && ok(*height) && ok(*angle),
            Shape::Fill { rings, .. } => rings.iter().flatten().all(p),
        }
    }
}

/// An entity of model space, placed: its layer, how it is drawn, and its shape.
#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    pub layer: String,
    pub color: Color,
    /// Its linetype's name when it has one of its own; none takes the layer's.
    pub linetype: Option<String>,
    /// Its line weight, mm, when it has one of its own.
    pub weight: Option<f64>,
    pub shape: Shape,
}

/// A DXF file read: what it draws, in its own unit.
#[derive(Clone, Debug, PartialEq)]
pub struct Dxf {
    /// Millimetres to the drawing's unit, from `$INSUNITS`; none when the file is unitless, or in a unit no building is
    /// drawn in, and the user says.
    pub scale: Option<f64>,
    /// The unit's name (`mm`, `m`, `in`, `ft`…), or `unitless`.
    pub unit: String,
    /// `$ACADVER` as the file says it, or empty.
    pub version: String,
    /// The LAYER table, and a layer an entity names that the table doesn't, with defaults.
    pub layers: Vec<Layer>,
    pub linetypes: Vec<Linetype>,
    /// `$LTSCALE`: what every linetype's dashes are multiplied by.
    pub ltscale: f64,
    pub entities: Vec<Entity>,
    /// `$EXTMIN` and `$EXTMAX`, the drawing's extents as the file says them.
    pub extents: Option<[P2; 2]>,
    /// What was left out, by what it was, each with its count: kinds of entity the reader doesn't draw, paper space,
    /// a block nested too deep or not defined, an entity of no size.
    pub left_out: BTreeMap<String, usize>,
}

impl Dxf {
    /// The layer named, if the drawing has it.
    pub fn layer(&self, name: &str) -> Option<&Layer> {
        self.layers.iter().find(|l| l.name == name)
    }

    /// The box round everything drawn, drawing units: an arc's or circle's whole circle, a text's point.
    pub fn bounds(&self) -> Option<[P2; 2]> {
        union(self.entities.iter().map(|e| e.shape.bounds()))
    }
}

/// The box round the boxes given.
pub fn union(boxes: impl IntoIterator<Item = [P2; 2]>) -> Option<[P2; 2]> {
    boxes.into_iter().reduce(|[lo, hi], [a, b]| [P2(lo.0.min(a.0), lo.1.min(a.1)), P2(hi.0.max(b.0), hi.1.max(b.1))])
}

impl Shape {
    /// The box round it, drawing units: a circle's whole circle, an arc's sweep, a text's point.
    pub fn bounds(&self) -> [P2; 2] {
        let pts: Vec<P2> = match self {
            Shape::Line(a, b) => vec![*a, *b],
            Shape::Polyline { pts, .. } => pts.clone(),
            Shape::Circle { c, r } => vec![*c - P2(*r, *r), *c + P2(*r, *r)],
            // Its ends, and where it passes east, north, west or south of its centre.
            Shape::Arc { c, r, start, end } => {
                let at = |deg: f64| *c + P2(deg.to_radians().cos(), deg.to_radians().sin()) * *r;
                let quarters = (0..8).map(|k| f64::from(k) * 90.0).filter(|q| q > start && q < end);
                [*start, *end].into_iter().chain(quarters).map(at).collect()
            }
            Shape::Point(p) | Shape::Text { at: p, .. } => vec![*p],
            Shape::Fill { rings, .. } => rings.iter().flatten().copied().collect(),
        };
        union(pts.into_iter().map(|p| [p, p])).unwrap_or_default()
    }
}

/// Millimetres to the unit `$INSUNITS` names, and the unit's name; none for unitless, and for a unit no building is
/// drawn in.
fn insunits(code: i64) -> (Option<f64>, &'static str) {
    match code {
        1 => (Some(25.4), "in"),
        2 => (Some(304.8), "ft"),
        3 => (Some(1_609_344.0), "miles"),
        4 => (Some(1.0), "mm"),
        5 => (Some(10.0), "cm"),
        6 => (Some(1000.0), "m"),
        7 => (Some(1e6), "km"),
        8 => (Some(2.54e-5), "microinches"),
        9 => (Some(0.0254), "mils"),
        10 => (Some(914.4), "yards"),
        11 => (Some(1e-7), "angstroms"),
        12 => (Some(1e-6), "nanometres"),
        13 => (Some(1e-3), "microns"),
        14 => (Some(100.0), "dm"),
        15 => (Some(1e4), "decametres"),
        16 => (Some(1e5), "hectometres"),
        18 => (None, "astronomical units"),
        19 => (None, "light years"),
        20 => (None, "parsecs"),
        21 => (Some(304.800_609_601_2), "US survey feet"),
        _ => (None, "unitless"),
    }
}

/// The drawing a DXF file holds, or what's wrong with it.
pub fn read(bytes: &[u8]) -> Result<Dxf, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("it's {} bytes; up to {MAX_BYTES}", bytes.len()));
    }
    if bytes.starts_with(b"AutoCAD Binary DXF") {
        return Err("it's a binary DXF; save it as an ASCII DXF (any version) and bring that in".into());
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    // A file of 2007 and later is UTF-8. One before is in the code page its header names in $DWGCODEPAGE: the header is ASCII
    // in every page, so the bytes are scanned for it before any decoding. A page this doesn't know, and a file that names none,
    // read as Latin-1, as the West's always have and never fail.
    let text: Cow<str> = match std::str::from_utf8(bytes) {
        Ok(t) => Cow::Borrowed(t),
        Err(_) => Cow::Owned(codepage(bytes).map_or_else(|| latin1(bytes), |e| e.decode(bytes).0.into_owned())),
    };
    read_text(&text)
}

/// [`read`] of text.
pub fn read_text(text: &str) -> Result<Dxf, String> {
    Reader { p: Pairs::new(text), header: HashMap::new(), layers: Vec::new(), linetypes: Vec::new(), blocks: HashMap::new(), ents: None }
        .read()
}

/// The text as Latin-1, byte for byte.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// The encoding the file's `$DWGCODEPAGE` names, looked for in the first 64 KiB: the header is there, and it is ASCII in every
/// page, so the bytes scan line by line without any decoding.
fn codepage(bytes: &[u8]) -> Option<&'static encoding_rs::Encoding> {
    let head = &bytes[..bytes.len().min(65_536)];
    let mut lines = head.split(|&b| b == b'\n').map(|l| std::str::from_utf8(l).ok().map(str::trim).unwrap_or(""));
    while let Some(line) = lines.next() {
        if line != "$DWGCODEPAGE" {
            continue;
        }
        // The variable's name (9), the value's code (3), then the page: anything else is not a header this reads the page from.
        let Some(3) = lines.next().and_then(|l| l.trim().parse::<i32>().ok()) else { return None };
        return lines.next().map(str::trim).and_then(page);
    }
    None
}

/// The encoding a code page's name says, where a decoder here has one: the East Asian pages, the Windows ones and the few
/// others. The DOS pages (437, 850, …) have no decoder here and keep the Latin-1 they always had.
fn page(name: &str) -> Option<&'static encoding_rs::Encoding> {
    use encoding_rs::*;
    Some(match name.to_ascii_uppercase().as_str() {
        "ANSI_874" => WINDOWS_874,
        "ANSI_932" => SHIFT_JIS,
        "ANSI_936" => GBK,
        "ANSI_949" => EUC_KR,
        "ANSI_950" => BIG5,
        "ANSI_1250" => WINDOWS_1250,
        "ANSI_1251" => WINDOWS_1251,
        "ANSI_1252" => WINDOWS_1252,
        "ANSI_1253" => WINDOWS_1253,
        "ANSI_1254" => WINDOWS_1254,
        "ANSI_1255" => WINDOWS_1255,
        "ANSI_1256" => WINDOWS_1256,
        "ANSI_1257" => WINDOWS_1257,
        "ANSI_1258" => WINDOWS_1258,
        "ANSI_8859_2" => ISO_8859_2,
        "ANSI_8859_3" => ISO_8859_3,
        "ANSI_8859_4" => ISO_8859_4,
        "ANSI_8859_5" => ISO_8859_5,
        "ANSI_8859_6" => ISO_8859_6,
        "ANSI_8859_7" => ISO_8859_7,
        "ANSI_8859_8" => ISO_8859_8,
        // Latin-5's bytes are the Windows Turkish ones', so the decoder here has no ISO_8859_9 of its own.
        "ANSI_8859_9" => WINDOWS_1254,
        "DOS866" => IBM866,
        "KOI8_R" => KOI8_R,
        "MACINTOSH" => MACINTOSH,
        "ANSI_65001" => UTF_8,
        _ => return None,
    })
}

// ---------- the pairs of a file ----------

/// A file's group codes and values, a pair at a time.
struct Pairs<'a> {
    lines: std::str::Split<'a, char>,
    /// Lines given so far, so a message can say where.
    line: usize,
    peeked: Option<(i32, &'a str)>,
}

impl<'a> Pairs<'a> {
    fn new(text: &'a str) -> Pairs<'a> {
        Pairs { lines: text.split('\n'), line: 0, peeked: None }
    }

    fn next(&mut self) -> Result<Option<(i32, &'a str)>, String> {
        if let Some(p) = self.peeked.take() {
            return Ok(Some(p));
        }
        let code = loop {
            let Some(code) = self.lines.next() else { return Ok(None) };
            self.line += 1;
            let code = code.trim();
            if !code.is_empty() {
                break code;
            }
        };
        let n: i32 = code.parse().map_err(|_| format!("line {}: `{code}` isn't a group code", self.line))?;
        // The last line of a file that ends in a line break is empty: no value either.
        let value = self.lines.next().filter(|v| !(v.trim().is_empty() && self.lines.clone().next().is_none()));
        let Some(value) = value else {
            return Err(format!("line {}: the file ends after group code {n}, with no value", self.line));
        };
        self.line += 1;
        // Text keeps its spaces; a name, a number or a flag has none that matter.
        let value = if matches!(n, 1 | 3) { value.strip_suffix('\r').unwrap_or(value) } else { value.trim() };
        Ok(Some((n, value)))
    }

    fn peek(&mut self) -> Result<Option<(i32, &'a str)>, String> {
        if self.peeked.is_none() {
            self.peeked = self.next()?;
        }
        Ok(self.peeked)
    }
}

/// A number a file holds, or why it isn't one a drawing can hold.
fn number(line: usize, v: &str) -> Result<f64, String> {
    let x: f64 = v.trim().parse().map_err(|_| format!("line {line}: `{v}` isn't a number"))?;
    if !x.is_finite() {
        return Err(format!("line {line}: `{v}` isn't a number a drawing can hold"));
    }
    if x.abs() <= MAX_FILE_COORD { Ok(x) } else { Err(format!("line {line}: {v} is past what a drawing holds ({MAX_FILE_COORD})")) }
}

/// One entity, table entry or block header: its kind, where it starts, and its pairs in order.
struct Ent<'a> {
    kind: &'a str,
    line: usize,
    codes: Vec<(i32, &'a str)>,
}

impl<'a> Ent<'a> {
    fn str(&self, code: i32) -> Option<&'a str> {
        self.codes.iter().find(|(c, _)| *c == code).map(|(_, v)| *v)
    }

    fn num(&self, code: i32) -> Result<Option<f64>, String> {
        self.str(code).map(|v| number(self.line, v)).transpose()
    }

    fn int(&self, code: i32) -> Result<Option<i64>, String> {
        Ok(self.num(code)?.map(|x| x as i64))
    }

    /// The point at `code` (its x) and `code + 10` (its y), first of each.
    fn point(&self, code: i32) -> Result<Option<P2>, String> {
        match (self.num(code)?, self.num(code + 10)?) {
            (Some(x), Some(y)) => Ok(Some(P2(x, y))),
            _ => Ok(None),
        }
    }

    /// Every point whose x is at `code`, in order, each with the value at `extra` that follows it (or 0).
    fn points(&self, code: i32, extra: i32) -> Result<Vec<(P2, f64)>, String> {
        let mut out: Vec<(P2, f64)> = Vec::new();
        let mut x = None;
        for (c, v) in &self.codes {
            if *c == code {
                x = Some(number(self.line, v)?);
            } else if *c == code + 10 {
                if let Some(x) = x.take() {
                    out.push((P2(x, number(self.line, v)?), 0.0));
                }
            } else if *c == extra
                && let Some(last) = out.last_mut()
            {
                last.1 = number(self.line, v)?;
            }
        }
        Ok(out)
    }

    /// Every value at `code`, in order.
    fn all(&self, code: i32) -> Result<Vec<f64>, String> {
        self.codes.iter().filter(|(c, _)| *c == code).map(|(_, v)| number(self.line, v)).collect()
    }
}

// ---------- the sections ----------

struct Block<'a> {
    base: P2,
    ents: Vec<Ent<'a>>,
}

struct Reader<'a> {
    p: Pairs<'a>,
    /// Each header variable's pairs.
    header: HashMap<&'a str, Vec<(i32, &'a str)>>,
    layers: Vec<Layer>,
    linetypes: Vec<Linetype>,
    blocks: HashMap<String, Block<'a>>,
    /// The ENTITIES section's, once seen.
    ents: Option<Vec<Ent<'a>>>,
}

impl<'a> Reader<'a> {
    fn read(mut self) -> Result<Dxf, String> {
        while let Some((code, value)) = self.p.next()? {
            if code != 0 {
                continue;
            }
            match value {
                "EOF" => break,
                "SECTION" => {
                    let name = match self.p.next()? {
                        Some((2, n)) => n,
                        _ => return Err(format!("line {}: a SECTION with no name", self.p.line)),
                    };
                    match name {
                        "HEADER" => self.header()?,
                        "TABLES" => self.tables()?,
                        "BLOCKS" => self.blocks()?,
                        "ENTITIES" => self.ents = Some(self.entities()?),
                        _ => self.skip()?,
                    }
                }
                _ => {}
            }
        }
        let Some(ents) = self.ents.take() else {
            return Err("it isn't a DXF: there's no ENTITIES section".into());
        };
        let var = |name: &str, code: i32| self.header.get(name).and_then(|v| v.iter().find(|(c, _)| *c == code)).map(|(_, v)| *v);
        let (scale, unit) = insunits(var("$INSUNITS", 70).and_then(|v| v.trim().parse::<f64>().ok()).map_or(0, |x| x as i64));
        let version = var("$ACADVER", 1).unwrap_or("").trim().to_string();
        let ltscale = var("$LTSCALE", 40).and_then(|v| number(0, v).ok()).filter(|x| *x > 0.0).unwrap_or(1.0);
        let corner = |name: &str| -> Option<P2> {
            let (x, y) = (var(name, 10)?, var(name, 20)?);
            Some(P2(number(0, x).ok()?, number(0, y).ok()?))
        };
        let extents = match (corner("$EXTMIN"), corner("$EXTMAX")) {
            (Some(lo), Some(hi)) if lo.0 <= hi.0 && lo.1 <= hi.1 => Some([lo, hi]),
            _ => None,
        };
        let mut out = Out { entities: Vec::new(), left_out: BTreeMap::new(), work: 0 };
        self.place(&mut out, &ents, Xf::ID, None, 0)?;
        let mut layers = std::mem::take(&mut self.layers);
        for e in &out.entities {
            if !layers.iter().any(|l| l.name == e.layer) {
                layers.push(Layer {
                    name: e.layer.clone(),
                    color: Color::Index(7),
                    linetype: "Continuous".into(),
                    weight: None,
                    off: false,
                    frozen: false,
                });
            }
        }
        Ok(Dxf {
            scale,
            unit: unit.into(),
            version,
            layers,
            linetypes: std::mem::take(&mut self.linetypes),
            ltscale,
            entities: out.entities,
            extents,
            left_out: out.left_out,
        })
    }

    /// Past the end of this section.
    fn skip(&mut self) -> Result<(), String> {
        while let Some((code, value)) = self.p.next()? {
            if code == 0 && value == "ENDSEC" {
                break;
            }
        }
        Ok(())
    }

    fn header(&mut self) -> Result<(), String> {
        let mut name = None;
        while let Some((code, value)) = self.p.next()? {
            match (code, value) {
                (0, "ENDSEC") => break,
                (9, n) => name = Some(n),
                (c, v) => {
                    if let Some(n) = name {
                        self.header.entry(n).or_default().push((c, v));
                    }
                }
            }
        }
        Ok(())
    }

    /// The pairs after a `(0, kind)` up to the next code 0.
    fn entity(&mut self, kind: &'a str) -> Result<Ent<'a>, String> {
        let line = self.p.line;
        let mut codes = Vec::new();
        while let Some((c, v)) = self.p.peek()? {
            if c == 0 {
                break;
            }
            self.p.next()?;
            codes.push((c, v));
            if codes.len() > MAX_PAIRS {
                return Err(format!("line {line}: one {kind} holds more than {MAX_PAIRS} values"));
            }
        }
        Ok(Ent { kind, line, codes })
    }

    fn tables(&mut self) -> Result<(), String> {
        while let Some((code, value)) = self.p.next()? {
            if code != 0 {
                continue;
            }
            match value {
                "ENDSEC" => break,
                "LAYER" => {
                    let e = self.entity("LAYER")?;
                    let (color, off) = match (e.int(420)?, e.int(62)?) {
                        (Some(rgb), _) => (true_color(rgb), false),
                        (None, Some(n)) if n < 0 => (index_color(-n), true),
                        (None, Some(n)) => (index_color(n), false),
                        (None, None) => (Color::Index(7), false),
                    };
                    self.layers.push(Layer {
                        name: e.str(2).unwrap_or("0").to_string(),
                        color,
                        linetype: e.str(6).filter(|s| !s.is_empty()).unwrap_or("Continuous").to_string(),
                        weight: weight(e.int(370)?),
                        off,
                        frozen: e.int(70)?.unwrap_or(0) & 1 != 0,
                    });
                }
                "LTYPE" => {
                    let e = self.entity("LTYPE")?;
                    self.linetypes.push(Linetype { name: e.str(2).unwrap_or("").to_string(), dashes: e.all(49)? });
                }
                kind => {
                    self.entity(kind)?;
                }
            }
        }
        Ok(())
    }

    fn blocks(&mut self) -> Result<(), String> {
        let mut open: Option<(String, Block<'a>)> = None;
        while let Some((code, value)) = self.p.next()? {
            if code != 0 {
                continue;
            }
            match value {
                "ENDSEC" => break,
                "BLOCK" => {
                    let e = self.entity("BLOCK")?;
                    let name = e.str(2).unwrap_or("").to_string();
                    open = Some((name, Block { base: e.point(10)?.unwrap_or(P2(0.0, 0.0)), ents: Vec::new() }));
                }
                "ENDBLK" => {
                    self.entity("ENDBLK")?;
                    if let Some((name, block)) = open.take() {
                        self.blocks.insert(name, block);
                    }
                }
                kind => {
                    let e = self.entity(kind)?;
                    if let Some((_, block)) = &mut open {
                        block.ents.push(e);
                    }
                }
            }
        }
        Ok(())
    }

    fn entities(&mut self) -> Result<Vec<Ent<'a>>, String> {
        let mut ents = Vec::new();
        while let Some((code, value)) = self.p.next()? {
            if code != 0 {
                continue;
            }
            if value == "ENDSEC" {
                break;
            }
            ents.push(self.entity(value)?);
        }
        Ok(ents)
    }
}

fn true_color(rgb: i64) -> Color {
    Color::True((rgb >> 16 & 0xFF) as u8, (rgb >> 8 & 0xFF) as u8, (rgb & 0xFF) as u8)
}

fn index_color(n: i64) -> Color {
    match n {
        0 => Color::ByBlock,
        1..=255 => Color::Index(n as u8),
        _ => Color::ByLayer,
    }
}

/// A line weight as a file holds it, hundredths of a millimetre; by layer, by block and the default are none.
fn weight(v: Option<i64>) -> Option<f64> {
    v.filter(|w| *w >= 0).map(|w| f64::from(w as i32) / 100.0)
}

// ---------- placing ----------

/// Where a block's entities go: `p` lands on `o + x * p.0 + y * p.1`.
#[derive(Clone, Copy)]
struct Xf {
    o: P2,
    x: P2,
    y: P2,
}

impl Xf {
    const ID: Xf = Xf { o: P2(0.0, 0.0), x: P2(1.0, 0.0), y: P2(0.0, 1.0) };
    /// What an extrusion pointing down does: x the other way.
    const MIRROR_X: Xf = Xf { o: P2(0.0, 0.0), x: P2(-1.0, 0.0), y: P2(0.0, 1.0) };

    fn at(self, p: P2) -> P2 {
        self.o + self.x * p.0 + self.y * p.1
    }

    fn vec(self, v: P2) -> P2 {
        self.x * v.0 + self.y * v.1
    }

    /// `self` after `inner`: a point goes through `inner` first.
    fn then(self, inner: Xf) -> Xf {
        Xf { o: self.at(inner.o), x: self.vec(inner.x), y: self.vec(inner.y) }
    }

    /// When circles stay circles: the scale, the turn in degrees, and whether it mirrors.
    fn similarity(self) -> Option<(f64, f64, bool)> {
        let (lx, ly) = (self.x.length(), self.y.length());
        let square = self.x.dot(self.y).abs() <= 1e-9 * lx * ly;
        (lx > 0.0 && (lx - ly).abs() <= 1e-9 * lx && square)
            .then(|| (lx, self.x.1.atan2(self.x.0).to_degrees(), self.x.cross(self.y) < 0.0))
    }
}

/// What a block's entities take from the INSERT that places them: layer 0 becomes its layer, and by block its colour
/// and linetype.
struct Inherit<'b> {
    layer: &'b str,
    color: Color,
    linetype: Option<&'b str>,
}

struct Out {
    entities: Vec<Entity>,
    left_out: BTreeMap<String, usize>,
    work: usize,
}

impl Out {
    fn left(&mut self, what: &str) {
        *self.left_out.entry(what.into()).or_default() += 1;
    }

    fn push(&mut self, e: Entity) -> Result<(), String> {
        // Blocks scaled inside blocks can carry a point past any drawing, or past any number.
        if !e.shape.fits() {
            self.left("past what a drawing holds");
            return Ok(());
        }
        if self.entities.len() >= MAX_SHAPES {
            return Err(format!("it draws more than {MAX_SHAPES} shapes; up to that many"));
        }
        self.entities.push(e);
        Ok(())
    }
}

/// An entity's layer, colour, linetype and weight, with what it takes from the block it's in.
struct Style {
    layer: String,
    color: Color,
    linetype: Option<String>,
    weight: Option<f64>,
}

impl Style {
    fn of(e: &Ent, inh: Option<&Inherit>) -> Result<Style, String> {
        let layer = match (e.str(8).unwrap_or("0"), inh) {
            ("0", Some(i)) => i.layer,
            (l, _) => l,
        };
        let color = match (e.int(420)?, e.int(62)?) {
            (Some(rgb), _) => true_color(rgb),
            (None, Some(n)) => index_color(n.abs()),
            (None, None) => Color::ByLayer,
        };
        let color = match (color, inh) {
            (Color::ByBlock, Some(i)) => i.color,
            (c, _) => c,
        };
        let linetype = match e.str(6).map(str::trim) {
            None | Some("") => None,
            Some(lt) if lt.eq_ignore_ascii_case("BYLAYER") => None,
            Some(lt) if lt.eq_ignore_ascii_case("BYBLOCK") => inh.and_then(|i| i.linetype).map(String::from),
            Some(lt) => Some(lt.to_string()),
        };
        Ok(Style { layer: layer.chars().take(MAX_CHARS).collect(), color, linetype, weight: weight(e.int(370)?) })
    }
}

impl<'a> Reader<'a> {
    /// `ents` placed through `xf`, those of a block with what they inherit from its INSERT.
    fn place(&self, out: &mut Out, ents: &[Ent<'a>], xf: Xf, inh: Option<&Inherit>, depth: usize) -> Result<(), String> {
        let mut i = 0;
        while let Some(e) = ents.get(i) {
            i += 1;
            out.work += 1;
            if out.work > MAX_WORK {
                return Err(format!("its blocks place more than {MAX_WORK} entities; up to that many"));
            }
            if e.int(67)?.unwrap_or(0) == 1 {
                out.left("paper space");
                continue;
            }
            let style = Style::of(e, inh)?;
            let xf = if e.num(230)?.is_some_and(|z| z < 0.0) { xf.then(Xf::MIRROR_X) } else { xf };
            let shape = match e.kind {
                "LINE" => match (e.point(10)?, e.point(11)?) {
                    (Some(a), Some(b)) => Some(Shape::Line(a, b)),
                    _ => None,
                },
                "CIRCLE" => match (e.point(10)?, e.num(40)?) {
                    (Some(c), Some(r)) if r > 0.0 => Some(Shape::Circle { c, r }),
                    _ => None,
                },
                "ARC" => match (e.point(10)?, e.num(40)?) {
                    (Some(c), Some(r)) if r > 0.0 => {
                        let start = e.num(50)?.unwrap_or(0.0);
                        Some(arc(c, r, start, e.num(51)?.unwrap_or(360.0)))
                    }
                    _ => None,
                },
                "LWPOLYLINE" => {
                    let closed = e.int(70)?.unwrap_or(0) & 1 != 0;
                    polyline(e.points(10, 42)?, closed)
                }
                "POLYLINE" => {
                    let flags = e.int(70)?.unwrap_or(0);
                    let mut pts = Vec::new();
                    while let Some(v) = ents.get(i).filter(|v| v.kind == "VERTEX") {
                        i += 1;
                        let vflags = v.int(70)?.unwrap_or(0);
                        // A spline-fit polyline keeps its frame's control points too; they aren't drawn.
                        if flags & 4 != 0 && vflags & 16 != 0 {
                            continue;
                        }
                        if let Some(p) = v.point(10)? {
                            pts.push((p, v.num(42)?.unwrap_or(0.0)));
                        }
                    }
                    if ents.get(i).is_some_and(|v| v.kind == "SEQEND") {
                        i += 1;
                    }
                    if flags & (16 | 64) != 0 {
                        out.left("POLYLINE mesh");
                        continue;
                    }
                    polyline(pts, flags & 1 != 0)
                }
                "POINT" => e.point(10)?.map(Shape::Point),
                "ELLIPSE" => ellipse(e)?,
                "SPLINE" => spline(e)?,
                "TEXT" | "ATTRIB" => {
                    if e.kind == "ATTRIB" && e.int(70)?.unwrap_or(0) & 1 != 0 {
                        continue;
                    }
                    text(e)?
                }
                "MTEXT" => {
                    for line in mtext(e)? {
                        out.push(Entity {
                            layer: style.layer.clone(),
                            color: style.color,
                            linetype: style.linetype.clone(),
                            weight: style.weight,
                            shape: transformed(line, xf),
                        })?;
                    }
                    continue;
                }
                "HATCH" => hatch(e)?,
                "SOLID" | "TRACE" => match (e.point(10)?, e.point(11)?, e.point(12)?, e.point(13)?) {
                    (Some(a), Some(b), Some(c), d) => {
                        let mut ring = vec![a, b];
                        if let Some(d) = d.filter(|d| d.dist(c) > 0.0) {
                            ring.push(d);
                        }
                        ring.push(c);
                        Some(Shape::Fill { rings: vec![ring], solid: true })
                    }
                    _ => None,
                },
                "INSERT" => {
                    self.insert(out, e, &style, xf, depth)?;
                    continue;
                }
                "DIMENSION" => {
                    if let Some(name) = e.str(2) {
                        self.block(out, name, &style, xf, depth)?;
                    }
                    continue;
                }
                "SEQEND" | "VERTEX" | "ATTDEF" => continue,
                other => {
                    out.left(other);
                    continue;
                }
            };
            match shape {
                Some(shape) => out.push(Entity {
                    layer: style.layer,
                    color: style.color,
                    linetype: style.linetype,
                    weight: style.weight,
                    shape: transformed(shape, xf),
                })?,
                None => out.left("empty"),
            }
        }
        Ok(())
    }

    /// The block an INSERT names, placed at its point, scaled, turned, and as many times as its array says.
    fn insert(&self, out: &mut Out, e: &Ent<'a>, style: &Style, xf: Xf, depth: usize) -> Result<(), String> {
        let Some(name) = e.str(2) else {
            out.left("INSERT of no block");
            return Ok(());
        };
        let at = e.point(10)?.unwrap_or(P2(0.0, 0.0));
        let (sx, sy) = (e.num(41)?.unwrap_or(1.0), e.num(42)?.unwrap_or(1.0));
        if sx == 0.0 || sy == 0.0 {
            out.left("empty");
            return Ok(());
        }
        let turn = e.num(50)?.unwrap_or(0.0).to_radians();
        let (cols, rows) = (e.int(70)?.unwrap_or(1).clamp(1, MAX_ARRAY as i64), e.int(71)?.unwrap_or(1).clamp(1, MAX_ARRAY as i64));
        let (dx, dy) = (e.num(44)?.unwrap_or(0.0), e.num(45)?.unwrap_or(0.0));
        let (ex, ey) = (P2(turn.cos(), turn.sin()), P2(-turn.sin(), turn.cos()));
        for row in 0..rows {
            for col in 0..cols {
                let o = at + ex * (dx * col as f64) + ey * (dy * row as f64);
                let local = Xf { o, x: ex * sx, y: ey * sy };
                self.block(out, name, style, xf.then(local), depth)?;
            }
        }
        Ok(())
    }

    /// The block named, its base on `xf`'s origin.
    fn block(&self, out: &mut Out, name: &str, style: &Style, xf: Xf, depth: usize) -> Result<(), String> {
        let Some(block) = self.blocks.get(name) else {
            out.left("INSERT of a block the file doesn't define");
            return Ok(());
        };
        if depth >= MAX_DEPTH {
            out.left(&format!("a block nested deeper than {MAX_DEPTH}"));
            return Ok(());
        }
        let unbased = Xf { o: P2(-block.base.0, -block.base.1), ..Xf::ID };
        let inh = Inherit { layer: &style.layer, color: style.color, linetype: style.linetype.as_deref() };
        self.place(out, &block.ents, xf.then(unbased), Some(&inh), depth + 1)
    }
}

/// `shape` through `xf`: a circle or an arc stays one where circles stay circles, else it's flattened first.
fn transformed(shape: Shape, xf: Xf) -> Shape {
    let at = |p: P2| xf.at(p);
    match shape {
        Shape::Line(a, b) => Shape::Line(at(a), at(b)),
        Shape::Polyline { pts, closed } => Shape::Polyline { pts: pts.into_iter().map(at).collect(), closed },
        Shape::Point(p) => Shape::Point(at(p)),
        Shape::Fill { rings, solid } => Shape::Fill { rings: rings.into_iter().map(|r| r.into_iter().map(at).collect()).collect(), solid },
        Shape::Circle { c, r } => match xf.similarity() {
            Some((k, _, _)) => Shape::Circle { c: at(c), r: r * k },
            None => {
                let mut pts: Vec<P2> = arc_points(c, r, 0.0, 360.0).into_iter().map(at).collect();
                pts.pop();
                Shape::Polyline { pts, closed: true }
            }
        },
        Shape::Arc { c, r, start, end } => match xf.similarity() {
            Some((k, turn, mirror)) => {
                let (s, e) = if mirror { (turn - end, turn - start) } else { (start + turn, end + turn) };
                arc(at(c), r * k, s, e)
            }
            None => Shape::Polyline { pts: arc_points(c, r, start, end - start).into_iter().map(at).collect(), closed: false },
        },
        Shape::Text { at: p, height, angle, text, align } => {
            let (k, turn) = xf.similarity().map_or((xf.x.length(), xf.x.1.atan2(xf.x.0).to_degrees()), |(k, t, _)| (k, t));
            Shape::Text { at: at(p), height: height * k, angle: angle + turn, text, align }
        }
    }
}

/// The arc counter-clockwise from `start` to `end` degrees, `start` brought into [0, 360) and `end` after it (a whole
/// turn when they're equal).
fn arc(c: P2, r: f64, start: f64, end: f64) -> Shape {
    let start = start.rem_euclid(360.0);
    let mut sweep = (end - start).rem_euclid(360.0);
    if sweep == 0.0 {
        sweep = 360.0;
    }
    Shape::Arc { c, r, start, end: start + sweep }
}

/// Points along the arc from `start` through `sweep` degrees (counter-clockwise when positive), both ends included.
pub(crate) fn arc_points(c: P2, r: f64, start: f64, sweep: f64) -> Vec<P2> {
    let n = ((sweep.abs() / 360.0 * CURVE_STEPS as f64).ceil() as usize).clamp(2, CURVE_STEPS);
    (0..=n)
        .map(|i| {
            let a = (start + sweep * i as f64 / n as f64).to_radians();
            c + P2(a.cos(), a.sin()) * r
        })
        .collect()
}

/// A polyline of vertices, each with the bulge of the segment that leaves it: bent segments flattened.
fn polyline(pts: Vec<(P2, f64)>, closed: bool) -> Option<Shape> {
    if pts.len() < 2 {
        return None;
    }
    if pts.iter().all(|(_, b)| *b == 0.0) {
        return Some(Shape::Polyline { pts: pts.into_iter().map(|(p, _)| p).collect(), closed });
    }
    let mut out: Vec<P2> = Vec::new();
    let last = if closed { pts.len() } else { pts.len() - 1 };
    for i in 0..last {
        let (Some((a, bulge)), Some((b, _))) = (pts.get(i), pts.get((i + 1) % pts.len())) else { break };
        if *bulge == 0.0 || a.dist(*b) == 0.0 {
            out.push(*a);
            continue;
        }
        let bent = bulge_points(*a, *b, *bulge);
        out.extend(bent.iter().take(bent.len().saturating_sub(1)));
    }
    if !closed && let Some((p, _)) = pts.last() {
        out.push(*p);
    }
    Some(Shape::Polyline { pts: out, closed })
}

/// Points along the arc from `a` to `b` whose bulge (the tangent of a quarter of its angle, counter-clockwise when
/// positive) is `bulge`, both ends included.
fn bulge_points(a: P2, b: P2, bulge: f64) -> Vec<P2> {
    let theta = 4.0 * bulge.atan();
    let d = a.dist(b);
    let r = d / (2.0 * (theta / 2.0).sin().abs());
    let c = (a + b) * 0.5 + (b - a).unit().perp() * (d / 2.0 / (theta / 2.0).tan());
    let start = (a - c).1.atan2((a - c).0).to_degrees();
    let mut pts = arc_points(c, r, start, theta.to_degrees());
    if let Some(first) = pts.first_mut() {
        *first = a;
    }
    if let Some(last) = pts.last_mut() {
        *last = b;
    }
    pts
}

fn ellipse(e: &Ent) -> Result<Option<Shape>, String> {
    let (Some(c), Some(m), Some(ratio)) = (e.point(10)?, e.point(11)?, e.num(40)?) else { return Ok(None) };
    if ratio <= 0.0 || m.length() == 0.0 {
        return Ok(None);
    }
    let (u0, u1) = (e.num(41)?.unwrap_or(0.0), e.num(42)?.unwrap_or(std::f64::consts::TAU));
    let mut sweep = (u1 - u0).rem_euclid(std::f64::consts::TAU);
    let whole = sweep < 1e-9;
    if whole {
        sweep = std::f64::consts::TAU;
    }
    let n = ((sweep / std::f64::consts::TAU * CURVE_STEPS as f64).ceil() as usize).clamp(2, CURVE_STEPS);
    let minor = m.perp() * ratio;
    let mut pts: Vec<P2> = (0..=n)
        .map(|i| {
            let u = u0 + sweep * i as f64 / n as f64;
            c + m * u.cos() + minor * u.sin()
        })
        .collect();
    if whole {
        pts.pop();
    }
    Ok(Some(Shape::Polyline { pts, closed: whole }))
}

fn spline(e: &Ent) -> Result<Option<Shape>, String> {
    let flags = e.int(70)?.unwrap_or(0);
    let closed = flags & 1 != 0;
    let degree = e.int(71)?.unwrap_or(3).clamp(1, 11) as usize;
    let knots = e.all(40)?;
    let controls = e.points(10, 41)?;
    let fit = e.points(11, 0)?;
    if controls.len() > degree && knots.len() == controls.len() + degree + 1 {
        let weighted: Vec<(P2, f64)> = controls.iter().map(|(p, w)| (*p, if *w > 0.0 { *w } else { 1.0 })).collect();
        let (Some(&u0), Some(&u1)) = (knots.get(degree), knots.get(controls.len())) else { return Ok(None) };
        let n = ((controls.len() - degree) * SPLINE_STEPS).clamp(SPLINE_STEPS, MAX_SPLINE_POINTS);
        let pts: Vec<P2> = (0..=n).filter_map(|i| de_boor(degree, &knots, &weighted, u0 + (u1 - u0) * i as f64 / n as f64)).collect();
        return Ok((pts.len() >= 2).then_some(Shape::Polyline { pts, closed }));
    }
    if fit.len() >= 2 {
        return Ok(Some(Shape::Polyline { pts: fit.into_iter().map(|(p, _)| p).collect(), closed }));
    }
    Ok(None)
}

/// The point at `u` of the B-spline of `degree` over `knots` with weighted `controls`, by de Boor's recurrence in
/// homogeneous coordinates.
fn de_boor(degree: usize, knots: &[f64], controls: &[(P2, f64)], u: f64) -> Option<P2> {
    let n = controls.len();
    let mut k = degree;
    while k + 1 < n && u >= *knots.get(k + 1)? {
        k += 1;
    }
    let mut d: Vec<[f64; 3]> = (0..=degree)
        .map(|j| {
            let (p, w) = controls.get(j + k - degree)?;
            Some([p.0 * w, p.1 * w, *w])
        })
        .collect::<Option<_>>()?;
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let (t0, t1) = (*knots.get(j + k - degree)?, *knots.get(j + 1 + k - r)?);
            let a = if t1 > t0 { (u - t0) / (t1 - t0) } else { 0.0 };
            let (p, q) = (*d.get(j - 1)?, *d.get(j)?);
            let v = [0, 1, 2].map(|i| (1.0 - a) * p[i] + a * q[i]);
            if let Some(slot) = d.get_mut(j) {
                *slot = v;
            }
        }
    }
    let [x, y, w] = *d.get(degree)?;
    (w.abs() > 1e-12).then(|| P2(x / w, y / w))
}

/// A text's words as a file writes them: `%%d` is a degree sign, `%%p` plus or minus, `%%c` a diameter, `%%%` a
/// percent, `%%u` and `%%o` underline and overline (dropped), `\U+XXXX` a character, and the formatting codes of
/// multi-line text (`\P` a new line, `{…}` groups, `\f…;` fonts, `\H…;` heights and the rest) are dropped.
fn decode(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if out.len() >= MAX_CHARS {
            break;
        }
        match c {
            '%' if chars.peek() == Some(&'%') => {
                chars.next();
                match chars.next().map(|c| c.to_ascii_lowercase()) {
                    Some('d') => out.push('°'),
                    Some('p') => out.push('±'),
                    Some('c') => out.push('Ø'),
                    Some('%') => out.push('%'),
                    Some('u' | 'o') => {}
                    Some(other) => {
                        out.push_str("%%");
                        out.push(other);
                    }
                    None => out.push_str("%%"),
                }
            }
            '{' | '}' => {}
            '\\' => match chars.next() {
                Some('P') => out.push('\n'),
                Some('~') => out.push(' '),
                Some('\\') => out.push('\\'),
                Some('{') => out.push('{'),
                Some('}') => out.push('}'),
                Some('U') if chars.peek() == Some(&'+') => {
                    chars.next();
                    let hex: String = chars.by_ref().take(4).collect();
                    match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        Some(ch) => out.push(ch),
                        None => {
                            out.push_str("\\U+");
                            out.push_str(&hex);
                        }
                    }
                }
                Some('S') => {
                    // A fraction, stacked: its two parts, parted by a slash.
                    let body: String = chars.by_ref().take_while(|c| *c != ';').collect();
                    out.push_str(&body.replace(['^', '#'], "/"));
                }
                Some('A' | 'C' | 'F' | 'f' | 'H' | 'Q' | 'T' | 'W' | 'p') => {
                    chars.by_ref().take_while(|c| *c != ';').for_each(drop);
                }
                Some('L' | 'l' | 'O' | 'o' | 'K' | 'k' | 'N') => {}
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            },
            c => out.push(c),
        }
    }
    out
}

fn text(e: &Ent) -> Result<Option<Shape>, String> {
    let (Some(first), Some(height), Some(words)) = (e.point(10)?, e.num(40)?, e.str(1)) else { return Ok(None) };
    if height <= 0.0 {
        return Ok(None);
    }
    let words = decode(words).replace('\n', " ");
    if words.trim().is_empty() {
        return Ok(None);
    }
    let (h, v) = (e.int(72)?.unwrap_or(0), e.int(73)?.unwrap_or(0));
    let second = e.point(11)?;
    let (at, align) = match (h, v, second) {
        (0, 0, _) | (3 | 5, _, _) | (_, _, None) => (first, Align { h: Horizontal::Left, v: Vertical::Baseline }),
        (4, _, Some(p)) => (p, Align { h: Horizontal::Center, v: Vertical::Middle }),
        (h, v, Some(p)) => {
            let h = match h {
                1 => Horizontal::Center,
                2 => Horizontal::Right,
                _ => Horizontal::Left,
            };
            let v = match v {
                1 => Vertical::Bottom,
                2 => Vertical::Middle,
                3 => Vertical::Top,
                _ => Vertical::Baseline,
            };
            (p, Align { h, v })
        }
    };
    Ok(Some(Shape::Text { at, height, angle: e.num(50)?.unwrap_or(0.0), text: words, align }))
}

/// A multi-line text's lines, each a text of its own, stepped down from where its attachment point puts the block.
fn mtext(e: &Ent) -> Result<Vec<Shape>, String> {
    let (Some(at), Some(height)) = (e.point(10)?, e.num(40)?) else { return Ok(Vec::new()) };
    if height <= 0.0 {
        return Ok(Vec::new());
    }
    let mut raw = String::new();
    for (c, v) in &e.codes {
        if *c == 3 {
            raw.push_str(v);
        }
    }
    raw.push_str(e.str(1).unwrap_or(""));
    let words = decode(&raw);
    let lines: Vec<&str> = words.lines().collect();
    if lines.iter().all(|l| l.trim().is_empty()) {
        return Ok(Vec::new());
    }
    let angle = match (e.num(50)?, e.point(11)?) {
        (Some(a), _) => a,
        (None, Some(dir)) if dir.length() > 0.0 => dir.1.atan2(dir.0).to_degrees(),
        _ => 0.0,
    };
    let attach = e.int(71)?.unwrap_or(1).clamp(1, 9) - 1;
    let h = match attach % 3 {
        0 => Horizontal::Left,
        1 => Horizontal::Center,
        _ => Horizontal::Right,
    };
    let spacing = height * 5.0 / 3.0 * e.num(44)?.filter(|f| *f > 0.0).unwrap_or(1.0);
    let block = height + spacing * (lines.len() - 1) as f64;
    let up = P2(-angle.to_radians().sin(), angle.to_radians().cos());
    let top = at
        + up * match attach / 3 {
            0 => 0.0,
            1 => block / 2.0,
            _ => block,
        };
    Ok(lines
        .iter()
        .enumerate()
        .map(|(i, line)| Shape::Text {
            at: top - up * (height + spacing * i as f64),
            height,
            angle,
            text: line.to_string(),
            align: Align { h, v: Vertical::Baseline },
        })
        .collect())
}

/// A hatch's boundary paths, each a ring: a polyline's (bulges flattened) or edges joined end to end.
fn hatch(e: &Ent) -> Result<Option<Shape>, String> {
    let solid = e.int(70)?.unwrap_or(0) & 1 != 0 || e.str(2).is_some_and(|n| n.eq_ignore_ascii_case("SOLID"));
    let Some(start) = e.codes.iter().position(|(c, _)| *c == 91) else { return Ok(None) };
    let mut cur = Cursor { e, k: start + 1 };
    let paths = e.int(91)?.unwrap_or(0).clamp(0, 10_000);
    let mut rings = Vec::new();
    for _ in 0..paths {
        let Some(flags) = cur.take(92)? else { break };
        let flags = flags as i64;
        let mut ring: Vec<P2> = Vec::new();
        let join = |ring: &mut Vec<P2>, pts: Vec<P2>| {
            for p in pts {
                if ring.last().is_none_or(|q| q.dist(p) > 1e-9) {
                    ring.push(p);
                }
            }
        };
        if flags & 2 != 0 {
            let bulged = cur.take(72)?.unwrap_or(0.0) != 0.0;
            let _closed = cur.take(73)?;
            let n = cur.take(93)?.unwrap_or(0.0).clamp(0.0, MAX_PAIRS as f64) as usize;
            let mut pts = Vec::new();
            for _ in 0..n {
                let (Some(x), Some(y)) = (cur.take(10)?, cur.take(20)?) else { break };
                let bulge = if bulged { cur.take(42)?.unwrap_or(0.0) } else { 0.0 };
                pts.push((P2(x, y), bulge));
            }
            if let Some(Shape::Polyline { pts, .. }) = polyline(pts, true) {
                join(&mut ring, pts);
            }
        } else {
            let n = cur.take(93)?.unwrap_or(0.0).clamp(0.0, MAX_PAIRS as f64) as usize;
            for _ in 0..n {
                let Some(kind) = cur.take(72)? else { break };
                match kind as i64 {
                    1 => {
                        let (Some(x0), Some(y0), Some(x1), Some(y1)) = (cur.take(10)?, cur.take(20)?, cur.take(11)?, cur.take(21)?) else {
                            break;
                        };
                        join(&mut ring, vec![P2(x0, y0), P2(x1, y1)]);
                    }
                    2 => {
                        let (Some(x), Some(y), Some(r)) = (cur.take(10)?, cur.take(20)?, cur.take(40)?) else { break };
                        let (s, t) = (cur.take(50)?.unwrap_or(0.0), cur.take(51)?.unwrap_or(360.0));
                        let ccw = cur.take(73)?.unwrap_or(1.0) != 0.0;
                        let (s, t) = if ccw { (s, t) } else { (-t, -s) };
                        let sweep = {
                            let w = (t - s).rem_euclid(360.0);
                            if w == 0.0 { 360.0 } else { w }
                        };
                        if r > 0.0 {
                            join(&mut ring, arc_points(P2(x, y), r, s, sweep));
                        }
                    }
                    3 => {
                        let (Some(x), Some(y), Some(mx), Some(my), Some(ratio)) =
                            (cur.take(10)?, cur.take(20)?, cur.take(11)?, cur.take(21)?, cur.take(40)?)
                        else {
                            break;
                        };
                        let (s, t) = (cur.take(50)?.unwrap_or(0.0), cur.take(51)?.unwrap_or(360.0));
                        let ccw = cur.take(73)?.unwrap_or(1.0) != 0.0;
                        let (s, t) = if ccw { (s, t) } else { (-t, -s) };
                        let sweep = {
                            let w = (t - s).rem_euclid(360.0);
                            if w == 0.0 { 360.0 } else { w }
                        };
                        let (c, m) = (P2(x, y), P2(mx, my));
                        if ratio > 0.0 && m.length() > 0.0 {
                            let n = ((sweep / 360.0 * CURVE_STEPS as f64).ceil() as usize).clamp(2, CURVE_STEPS);
                            let minor = m.perp() * ratio;
                            let pts = (0..=n)
                                .map(|i| {
                                    let u = (s + sweep * i as f64 / n as f64).to_radians();
                                    c + m * u.cos() + minor * u.sin()
                                })
                                .collect();
                            join(&mut ring, pts);
                        }
                    }
                    4 => {
                        let degree = cur.take(94)?.unwrap_or(3.0).clamp(1.0, 11.0) as usize;
                        let rational = cur.take(73)?.unwrap_or(0.0) != 0.0;
                        let _periodic = cur.take(74)?;
                        let nk = cur.take(95)?.unwrap_or(0.0).clamp(0.0, MAX_PAIRS as f64) as usize;
                        let nc = cur.take(96)?.unwrap_or(0.0).clamp(0.0, MAX_PAIRS as f64) as usize;
                        let mut knots = Vec::with_capacity(nk.min(1 << 16));
                        for _ in 0..nk {
                            let Some(t) = cur.take(40)? else { break };
                            knots.push(t);
                        }
                        let mut controls = Vec::with_capacity(nc.min(1 << 16));
                        for _ in 0..nc {
                            let (Some(x), Some(y)) = (cur.take(10)?, cur.take(20)?) else { break };
                            let w = if rational { cur.take(42)?.filter(|w| *w > 0.0).unwrap_or(1.0) } else { 1.0 };
                            controls.push((P2(x, y), w));
                        }
                        let nf = cur.take(97)?.unwrap_or(0.0).clamp(0.0, MAX_PAIRS as f64) as usize;
                        let mut fit = Vec::new();
                        for _ in 0..nf {
                            let (Some(x), Some(y)) = (cur.take(11)?, cur.take(21)?) else { break };
                            fit.push(P2(x, y));
                        }
                        if controls.len() > degree && knots.len() == controls.len() + degree + 1 {
                            if let (Some(&u0), Some(&u1)) = (knots.get(degree), knots.get(controls.len())) {
                                let n = ((controls.len() - degree) * SPLINE_STEPS).clamp(SPLINE_STEPS, MAX_SPLINE_POINTS);
                                let pts = (0..=n)
                                    .filter_map(|i| de_boor(degree, &knots, &controls, u0 + (u1 - u0) * i as f64 / n as f64))
                                    .collect();
                                join(&mut ring, pts);
                            }
                        } else {
                            join(&mut ring, fit);
                        }
                    }
                    _ => break,
                }
            }
        }
        if ring.len() >= 2 && ring.first().zip(ring.last()).is_some_and(|(a, b)| a.dist(*b) <= 1e-9) {
            ring.pop();
        }
        if ring.len() >= 3 {
            rings.push(ring);
        }
    }
    Ok((!rings.is_empty()).then_some(Shape::Fill { rings, solid }))
}

/// A walk forward through an entity's pairs: a hatch lays its paths out in order, each value after the last.
struct Cursor<'e, 'a> {
    e: &'e Ent<'a>,
    k: usize,
}

impl Cursor<'_, '_> {
    /// The next value at `code` from here on, or none when there is none; the walk moves past it.
    fn take(&mut self, code: i32) -> Result<Option<f64>, String> {
        let Some(at) = self.e.codes.get(self.k..).and_then(|rest| rest.iter().position(|(c, _)| *c == code)) else {
            return Ok(None);
        };
        self.k += at + 1;
        let Some((_, v)) = self.e.codes.get(self.k - 1) else { return Ok(None) };
        Ok(Some(number(self.e.line, v)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_arcs_box_is_round_its_sweep_not_its_circle() {
        let near = |a: [P2; 2], b: [P2; 2]| a.iter().zip(&b).all(|(p, q)| p.dist(*q) < 1e-6);
        // A quarter from east to north.
        let quarter = arc(P2(0.0, 0.0), 10.0, 0.0, 90.0);
        assert!(near(quarter.bounds(), [P2(0.0, 0.0), P2(10.0, 10.0)]), "{:?}", quarter.bounds());
        // From north-east to south-east the long way: through north, west and south.
        let most = arc(P2(0.0, 0.0), 10.0, 45.0, 315.0);
        assert!(near(most.bounds(), [P2(-10.0, -10.0), P2(45f64.to_radians().cos() * 10.0, 10.0)]), "{:?}", most.bounds());
        // Across east, from 350 to 10 degrees: a sliver of a great circle, as a near-straight wall is drawn.
        let sliver = arc(P2(0.0, 0.0), 30_000.0, 359.0, 1.0);
        let [lo, hi] = sliver.bounds();
        assert!(hi.0 - lo.0 < 10.0 && hi.1 - lo.1 < 1100.0 && (hi.0 - 30_000.0).abs() < 1e-6, "{lo:?} {hi:?}");
    }

    /// Pairs as a file writes them: a code line, then its value.
    fn pairs(list: &[(i32, &str)]) -> String {
        list.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect()
    }

    /// A file of these sections; an empty one is left out.
    fn file(header: &str, tables: &str, blocks: &str, entities: &str) -> String {
        let mut s = String::new();
        for (name, body) in [("HEADER", header), ("TABLES", tables), ("BLOCKS", blocks)] {
            if !body.is_empty() {
                s += &format!("0\nSECTION\n2\n{name}\n{body}0\nENDSEC\n");
            }
        }
        s + &format!("0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n")
    }

    fn drawing(entities: &str) -> Dxf {
        read_text(&file("", "", "", entities)).unwrap()
    }

    fn near(a: P2, b: P2) -> bool {
        a.dist(b) < 1e-6
    }

    fn has_point_near(pts: &[P2], p: P2) -> bool {
        pts.iter().any(|q| q.dist(p) < 1e-6)
    }

    fn shapes(d: &Dxf) -> Vec<&Shape> {
        d.entities.iter().map(|e| &e.shape).collect()
    }

    #[test]
    fn a_line_a_circle_and_an_arc_read_with_their_layer_colour_and_linetype() {
        let d = drawing(&pairs(&[
            (0, "LINE"),
            (8, "WALLS"),
            (62, "1"),
            (6, "HIDDEN"),
            (370, "35"),
            (10, "0"),
            (20, "0"),
            (30, "0"),
            (11, "4000"),
            (21, "0"),
            (31, "0"),
            (0, "CIRCLE"),
            (8, "COLS"),
            (420, "65280"),
            (10, "100"),
            (20, "200"),
            (40, "50"),
            (0, "ARC"),
            (10, "0"),
            (20, "0"),
            (40, "10"),
            (50, "90"),
            (51, "180"),
        ]));
        assert_eq!(d.entities.len(), 3, "{d:?}");
        let line = &d.entities[0];
        assert_eq!(
            (line.layer.as_str(), line.color, line.linetype.as_deref(), line.weight),
            ("WALLS", Color::Index(1), Some("HIDDEN"), Some(0.35))
        );
        assert_eq!(line.shape, Shape::Line(P2(0.0, 0.0), P2(4000.0, 0.0)));
        let circle = &d.entities[1];
        assert_eq!((circle.color, circle.linetype.as_deref(), circle.weight), (Color::True(0, 255, 0), None, None));
        assert_eq!(circle.shape, Shape::Circle { c: P2(100.0, 200.0), r: 50.0 });
        assert_eq!(d.entities[2].shape, Shape::Arc { c: P2(0.0, 0.0), r: 10.0, start: 90.0, end: 180.0 });
        assert_eq!(d.entities[2].layer, "0", "no layer named: layer 0");
        // A layer an entity names and no table does is a layer still, with defaults; and the drawing's bounds.
        let names: Vec<&str> = d.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["WALLS", "COLS", "0"]);
        assert_eq!(d.layer("COLS").map(|l| (l.color, l.off, l.frozen)), Some((Color::Index(7), false, false)));
        assert_eq!(d.bounds(), Some([P2(-10.0, 0.0), P2(4000.0, 250.0)]), "the arc from north to west, not its circle");
        assert!(d.left_out.is_empty(), "{:?}", d.left_out);
        assert_eq!((d.scale, d.unit.as_str()), (None, "unitless"));
    }

    #[test]
    fn the_header_says_the_unit_the_version_and_the_extents() {
        let header = |units: &str| {
            pairs(&[
                (9, "$ACADVER"),
                (1, "AC1027"),
                (9, "$INSUNITS"),
                (70, units),
                (9, "$LTSCALE"),
                (40, "2"),
                (9, "$EXTMIN"),
                (10, "-1"),
                (20, "-2"),
                (30, "0"),
                (9, "$EXTMAX"),
                (10, "10"),
                (20, "5"),
                (30, "0"),
            ])
        };
        let d = read_text(&file(&header("6"), "", "", &pairs(&[(0, "POINT"), (10, "1"), (20, "1")]))).unwrap();
        assert_eq!((d.scale, d.unit.as_str(), d.version.as_str(), d.ltscale), (Some(1000.0), "m", "AC1027", 2.0));
        assert_eq!(d.extents, Some([P2(-1.0, -2.0), P2(10.0, 5.0)]));
        for (code, scale, unit) in [
            ("1", Some(25.4), "in"),
            ("2", Some(304.8), "ft"),
            ("4", Some(1.0), "mm"),
            ("5", Some(10.0), "cm"),
            ("0", None, "unitless"),
            ("19", None, "light years"),
        ] {
            let d = read_text(&file(&header(code), "", "", &pairs(&[(0, "POINT"), (10, "1"), (20, "1")]))).unwrap();
            assert_eq!((d.scale, d.unit.as_str()), (scale, unit), "$INSUNITS {code}");
        }
        // Extents the wrong way round are none.
        let bad = header("4").replace("10\n10\n20\n5\n", "10\n-10\n20\n5\n");
        assert_eq!(read_text(&file(&bad, "", "", &pairs(&[(0, "POINT"), (10, "1"), (20, "1")]))).unwrap().extents, None);
    }

    #[test]
    fn the_layer_table_gives_each_layers_colour_linetype_weight_and_state_and_the_linetypes_their_dashes() {
        let tables = pairs(&[
            (0, "TABLE"),
            (2, "LTYPE"),
            (70, "2"),
            (0, "LTYPE"),
            (2, "DASHED"),
            (73, "2"),
            (40, "7.5"),
            (49, "5"),
            (49, "-2.5"),
            (0, "ENDTAB"),
            (0, "TABLE"),
            (2, "LAYER"),
            (70, "3"),
            (0, "LAYER"),
            (2, "WALLS"),
            (70, "0"),
            (62, "1"),
            (6, "DASHED"),
            (370, "50"),
            (0, "LAYER"),
            (2, "OFF"),
            (70, "0"),
            (62, "-3"),
            (0, "LAYER"),
            (2, "FROZEN"),
            (70, "1"),
            (420, "16711680"),
            (0, "ENDTAB"),
        ]);
        let d = read_text(&file("", &tables, "", &pairs(&[(0, "POINT"), (8, "WALLS"), (10, "1"), (20, "1")]))).unwrap();
        assert_eq!(d.linetypes, vec![Linetype { name: "DASHED".into(), dashes: vec![5.0, -2.5] }]);
        type Row<'a> = (&'a str, Color, &'a str, Option<f64>, bool, bool);
        let got: Vec<Row> = d.layers.iter().map(|l| (l.name.as_str(), l.color, l.linetype.as_str(), l.weight, l.off, l.frozen)).collect();
        assert_eq!(
            got,
            [
                ("WALLS", Color::Index(1), "DASHED", Some(0.5), false, false),
                ("OFF", Color::Index(3), "Continuous", None, true, false),
                ("FROZEN", Color::True(255, 0, 0), "Continuous", None, false, true),
            ]
        );
    }

    #[test]
    fn the_colour_index_is_the_standard_table() {
        assert_eq!(aci(1), Some(Rgb(255, 0, 0)));
        assert_eq!(aci(5), Some(Rgb(0, 0, 255)));
        assert_eq!(aci(7), None, "the ink");
        assert_eq!(aci(0), None, "by block");
        assert_eq!(aci(10), Some(Rgb(255, 0, 0)));
        assert_eq!(aci(11), Some(Rgb(255, 127, 127)), "pale red");
        assert_eq!(aci(12), Some(Rgb(204, 0, 0)));
        assert_eq!(aci(13), Some(Rgb(204, 102, 102)));
        assert_eq!(aci(19), Some(Rgb(76, 38, 38)));
        assert_eq!(aci(20), Some(Rgb(255, 63, 0)));
        assert_eq!(aci(50), Some(Rgb(255, 255, 0)));
        assert_eq!(aci(90), Some(Rgb(0, 255, 0)));
        assert_eq!(aci(130), Some(Rgb(0, 255, 255)));
        assert_eq!(aci(170), Some(Rgb(0, 0, 255)));
        assert_eq!(aci(210), Some(Rgb(255, 0, 255)));
        assert_eq!(aci(250), Some(Rgb(51, 51, 51)));
        assert_eq!(aci(255), Some(Rgb(255, 255, 255)));
        assert_eq!(Color::True(1, 2, 3).rgb(), Some(Rgb(1, 2, 3)));
        assert_eq!(Color::ByLayer.rgb(), None);
    }

    #[test]
    fn a_polylines_bulge_bends_its_segment_and_a_closed_one_closes() {
        let d = drawing(&pairs(&[
            (0, "LWPOLYLINE"),
            (90, "2"),
            (70, "0"),
            (10, "0"),
            (20, "0"),
            (42, "1"),
            (10, "100"),
            (20, "0"),
            (0, "LWPOLYLINE"),
            (90, "3"),
            (70, "1"),
            (10, "0"),
            (20, "0"),
            (10, "10"),
            (20, "0"),
            (10, "10"),
            (20, "10"),
        ]));
        let Shape::Polyline { pts, closed } = &d.entities[0].shape else { panic!("{:?}", d.entities[0]) };
        assert!(!closed);
        // A bulge of 1 is a half circle, counter-clockwise from (0, 0) to (100, 0): below the chord, round (50, 0).
        assert!(near(pts[0], P2(0.0, 0.0)) && near(pts[pts.len() - 1], P2(100.0, 0.0)), "{pts:?}");
        assert!(pts.iter().all(|p| (p.dist(P2(50.0, 0.0)) - 50.0).abs() < 1e-6), "{pts:?}");
        assert!(has_point_near(pts, P2(50.0, -50.0)), "{pts:?}");
        assert!(pts.len() > 10);
        assert_eq!(d.entities[1].shape, Shape::Polyline { pts: vec![P2(0.0, 0.0), P2(10.0, 0.0), P2(10.0, 10.0)], closed: true });
    }

    #[test]
    fn an_old_polyline_is_its_vertices_up_to_seqend() {
        let d = drawing(&pairs(&[
            (0, "POLYLINE"),
            (8, "A"),
            (66, "1"),
            (70, "1"),
            (0, "VERTEX"),
            (8, "A"),
            (10, "0"),
            (20, "0"),
            (0, "VERTEX"),
            (8, "A"),
            (10, "5"),
            (20, "0"),
            (0, "VERTEX"),
            (8, "A"),
            (10, "5"),
            (20, "5"),
            (0, "SEQEND"),
            (8, "A"),
            (0, "POINT"),
            (10, "9"),
            (20, "9"),
        ]));
        assert_eq!(
            shapes(&d),
            [&Shape::Polyline { pts: vec![P2(0.0, 0.0), P2(5.0, 0.0), P2(5.0, 5.0)], closed: true }, &Shape::Point(P2(9.0, 9.0))]
        );
    }

    #[test]
    fn a_block_is_placed_where_each_insert_puts_it_scaled_turned_and_arrayed_taking_its_layer_and_colour() {
        let blocks = pairs(&[
            (0, "BLOCK"),
            (8, "0"),
            (2, "DOOR"),
            (70, "0"),
            (10, "0"),
            (20, "0"),
            (0, "LINE"),
            (8, "0"),
            (62, "0"),
            (10, "0"),
            (20, "0"),
            (11, "10"),
            (21, "0"),
            (0, "CIRCLE"),
            (8, "GLASS"),
            (10, "0"),
            (20, "0"),
            (40, "1"),
            (0, "ENDBLK"),
        ]);
        let entities = pairs(&[
            (0, "INSERT"),
            (8, "DOORS"),
            (62, "3"),
            (2, "DOOR"),
            (10, "100"),
            (20, "100"),
            (41, "2"),
            (42, "2"),
            (50, "90"),
            (70, "2"),
            (71, "1"),
            (44, "50"),
            (45, "0"),
        ]);
        let d = read_text(&file("", "", &blocks, &entities)).unwrap();
        assert_eq!(d.entities.len(), 4, "{:?}", d.entities);
        let lines: Vec<&Entity> = d.entities.iter().filter(|e| matches!(e.shape, Shape::Line(..))).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!((lines[0].layer.as_str(), lines[0].color), ("DOORS", Color::Index(3)), "layer 0 and by block take the INSERT's");
        let Shape::Line(a, b) = lines[0].shape else { panic!() };
        assert!(near(a, P2(100.0, 100.0)) && near(b, P2(100.0, 120.0)), "scaled by 2, turned 90°: {a:?} {b:?}");
        let Shape::Line(a, b) = lines[1].shape else { panic!() };
        assert!(near(a, P2(100.0, 150.0)) && near(b, P2(100.0, 170.0)), "the second column, 50 along the turned x: {a:?} {b:?}");
        let circles: Vec<&Entity> = d.entities.iter().filter(|e| matches!(e.shape, Shape::Circle { .. })).collect();
        assert_eq!(circles[0].layer, "GLASS", "a block's own layer stays");
        assert_eq!(circles[1].shape, Shape::Circle { c: P2(100.0, 150.0), r: 2.0 });
    }

    #[test]
    fn a_block_in_a_block_is_placed_through_both_and_one_in_itself_stops_at_the_depth() {
        let blocks = pairs(&[
            (0, "BLOCK"),
            (2, "INNER"),
            (10, "0"),
            (20, "0"),
            (0, "POINT"),
            (10, "1"),
            (20, "0"),
            (0, "ENDBLK"),
            (0, "BLOCK"),
            (2, "OUTER"),
            (10, "0"),
            (20, "0"),
            (0, "INSERT"),
            (2, "INNER"),
            (10, "10"),
            (20, "0"),
            (0, "ENDBLK"),
            (0, "BLOCK"),
            (2, "SELF"),
            (10, "0"),
            (20, "0"),
            (0, "POINT"),
            (10, "0"),
            (20, "0"),
            (0, "INSERT"),
            (2, "SELF"),
            (10, "1"),
            (20, "0"),
            (0, "ENDBLK"),
        ]);
        let d = read_text(&file("", "", &blocks, &pairs(&[(0, "INSERT"), (2, "OUTER"), (10, "100"), (20, "0")]))).unwrap();
        assert_eq!(shapes(&d), [&Shape::Point(P2(111.0, 0.0))]);
        let d = read_text(&file("", "", &blocks, &pairs(&[(0, "INSERT"), (2, "SELF"), (10, "0"), (20, "0")]))).unwrap();
        assert_eq!(d.entities.len(), MAX_DEPTH, "{:?}", d.entities);
        assert_eq!(d.left_out.get(&format!("a block nested deeper than {MAX_DEPTH}")), Some(&1));
        let d = read_text(&file("", "", &blocks, &pairs(&[(0, "INSERT"), (2, "NOWHERE"), (10, "0"), (20, "0")]))).unwrap();
        assert_eq!(d.left_out.get("INSERT of a block the file doesn't define"), Some(&1));
        // A base point other than the origin: the block's base lands on the insertion point.
        let based = pairs(&[(0, "BLOCK"), (2, "B"), (10, "5"), (20, "5"), (0, "POINT"), (10, "5"), (20, "5"), (0, "ENDBLK")]);
        let d = read_text(&file("", "", &based, &pairs(&[(0, "INSERT"), (2, "B"), (10, "1"), (20, "2")]))).unwrap();
        assert_eq!(shapes(&d), [&Shape::Point(P2(1.0, 2.0))]);
    }

    #[test]
    fn a_block_mirrored_or_stretched_keeps_its_arcs_as_arcs_only_while_they_stay_round() {
        let blocks = pairs(&[
            (0, "BLOCK"),
            (2, "A"),
            (10, "0"),
            (20, "0"),
            (0, "ARC"),
            (10, "0"),
            (20, "0"),
            (40, "1"),
            (50, "0"),
            (51, "90"),
            (0, "ENDBLK"),
        ]);
        // Mirrored in x: the quarter from east to north becomes the one from north to west.
        let d = read_text(&file("", "", &blocks, &pairs(&[(0, "INSERT"), (2, "A"), (10, "0"), (20, "0"), (41, "-1")]))).unwrap();
        assert_eq!(shapes(&d), [&Shape::Arc { c: P2(0.0, 0.0), r: 1.0, start: 90.0, end: 180.0 }]);
        // Stretched: no longer round, so it's points on the ellipse.
        let d = read_text(&file("", "", &blocks, &pairs(&[(0, "INSERT"), (2, "A"), (10, "0"), (20, "0"), (41, "2"), (42, "1")]))).unwrap();
        let Shape::Polyline { pts, closed: false } = &d.entities[0].shape else { panic!("{:?}", d.entities) };
        assert!(near(pts[0], P2(2.0, 0.0)) && near(pts[pts.len() - 1], P2(0.0, 1.0)), "{pts:?}");
        // An extrusion pointing down mirrors x too.
        let d = drawing(&pairs(&[(0, "LINE"), (10, "1"), (20, "2"), (11, "3"), (21, "4"), (210, "0"), (220, "0"), (230, "-1")]));
        assert_eq!(shapes(&d), [&Shape::Line(P2(-1.0, 2.0), P2(-3.0, 4.0))]);
    }

    #[test]
    fn text_keeps_its_words_height_angle_and_alignment_and_decodes_the_codes() {
        let d = drawing(&pairs(&[
            (0, "TEXT"),
            (10, "5"),
            (20, "5"),
            (40, "2.5"),
            (50, "30"),
            (1, "Room %%d %%c %%p %%% \\U+00E9 x"),
            (0, "TEXT"),
            (10, "0"),
            (20, "0"),
            (11, "8"),
            (21, "9"),
            (40, "1"),
            (72, "1"),
            (73, "3"),
            (1, "Centred"),
            (0, "TEXT"),
            (10, "1"),
            (20, "1"),
            (11, "8"),
            (21, "9"),
            (40, "1"),
            (72, "4"),
            (1, "Middle"),
            (0, "TEXT"),
            (10, "0"),
            (20, "0"),
            (40, "1"),
            (1, "   "),
        ]));
        assert_eq!(
            shapes(&d),
            [
                &Shape::Text {
                    at: P2(5.0, 5.0),
                    height: 2.5,
                    angle: 30.0,
                    text: "Room ° Ø ± % é x".into(),
                    align: Align { h: Horizontal::Left, v: Vertical::Baseline }
                },
                &Shape::Text {
                    at: P2(8.0, 9.0),
                    height: 1.0,
                    angle: 0.0,
                    text: "Centred".into(),
                    align: Align { h: Horizontal::Center, v: Vertical::Top }
                },
                &Shape::Text {
                    at: P2(8.0, 9.0),
                    height: 1.0,
                    angle: 0.0,
                    text: "Middle".into(),
                    align: Align { h: Horizontal::Center, v: Vertical::Middle }
                },
            ]
        );
        assert_eq!(d.left_out.get("empty"), Some(&1), "blank text");
    }

    /// A file whose text is in a code page, as bytes: the header names the page, one TEXT holds the words as the page's bytes.
    fn codepage_file(page: &str, words: &[u8]) -> Vec<u8> {
        let mut f = format!(
            "0\nSECTION\n2\nHEADER\n9\n$DWGCODEPAGE\n3\n{page}\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nTEXT\n10\n0\n20\n0\n40\n2.5\n1\n"
        )
        .into_bytes();
        f.extend_from_slice(words);
        f.extend_from_slice(b"\n0\nENDSEC\n0\nEOF\n");
        f
    }

    fn is_the_words(words: &str) -> impl Fn(&Entity) -> bool + '_ {
        move |e| matches!(&e.shape, Shape::Text { text, align: Align { h: Horizontal::Left, v: Vertical::Baseline }, .. } if text == words)
    }

    #[test]
    fn a_file_in_the_korean_code_page_reads_its_hangul() {
        // "제1층", a floor's name as the issue shows it, in the bytes EUC-KR holds.
        let d = read(&codepage_file("ANSI_949", &[0xC1, 0xA6, b'1', 0xC3, 0xFE])).unwrap();
        assert!(d.entities.iter().any(is_the_words("제1층")), "{d:?}");
    }

    #[test]
    fn a_page_this_does_not_know_reads_latin1_and_a_utf8_file_its_own_words_whatever_the_header_says() {
        // A page without a decoder here keeps the Latin-1 it always had.
        let d = read(&codepage_file("DOS437", &[0xC1])).unwrap();
        assert!(d.entities.iter().any(is_the_words("Á")), "{d:?}");
        // A file of 2007 and later is UTF-8 even when its header names a page.
        let d = read(&codepage_file("ANSI_949", "제1층".as_bytes())).unwrap();
        assert!(d.entities.iter().any(is_the_words("제1층")), "{d:?}");
    }

    #[test]
    fn multi_line_text_is_a_text_a_line_stepped_down_from_its_attachment_point_without_its_formatting() {
        let d = drawing(&pairs(&[
            (0, "MTEXT"),
            (10, "0"),
            (20, "10"),
            (40, "3"),
            (71, "1"),
            (3, "{\\fArial|b1|i0;First} line\\P"),
            (1, "\\H1.5x;Second \\S1^2; \\A1;line"),
            (0, "MTEXT"),
            (10, "0"),
            (20, "0"),
            (40, "3"),
            (71, "8"),
            (11, "0"),
            (21, "1"),
            (1, "Up"),
        ]));
        let texts: Vec<(P2, &str, Align, f64)> = d
            .entities
            .iter()
            .map(|e| match &e.shape {
                Shape::Text { at, text, align, angle, .. } => (*at, text.as_str(), *align, *angle),
                s => panic!("{s:?}"),
            })
            .collect();
        // Top left at (0, 10): the first baseline a height down, the next 5/3 of a height further.
        assert_eq!(texts.len(), 3, "{texts:?}");
        assert!(near(texts[0].0, P2(0.0, 7.0)) && texts[0].1 == "First line", "{:?}", texts[0]);
        assert!(near(texts[1].0, P2(0.0, 2.0)) && texts[1].1 == "Second 1/2 line", "{:?}", texts[1]);
        assert_eq!(texts[0].2, Align { h: Horizontal::Left, v: Vertical::Baseline });
        // Bottom centre at the origin, turned to read upward: the one line's baseline is on the point.
        assert!(near(texts[2].0, P2(0.0, 0.0)), "{:?}", texts[2]);
        assert_eq!((texts[2].1, texts[2].2.h, texts[2].3), ("Up", Horizontal::Center, 90.0));
    }

    #[test]
    fn a_hatch_gives_its_boundaries_as_a_fill_from_a_polyline_path_or_from_edges() {
        let d = drawing(&pairs(&[
            (0, "HATCH"),
            (2, "SOLID"),
            (70, "1"),
            (91, "1"),
            (92, "2"),
            (72, "0"),
            (73, "1"),
            (93, "4"),
            (10, "0"),
            (20, "0"),
            (10, "10"),
            (20, "0"),
            (10, "10"),
            (20, "10"),
            (10, "0"),
            (20, "10"),
            (97, "0"),
            (98, "1"),
            (10, "5"),
            (20, "5"),
            (0, "HATCH"),
            (2, "ANSI31"),
            (70, "0"),
            (91, "1"),
            (92, "0"),
            (93, "2"),
            (72, "1"),
            (10, "0"),
            (20, "0"),
            (11, "10"),
            (21, "0"),
            (72, "2"),
            (10, "5"),
            (20, "0"),
            (40, "5"),
            (50, "0"),
            (51, "180"),
            (73, "1"),
            (97, "0"),
        ]));
        assert_eq!(d.entities.len(), 2, "{:?}", d.entities);
        assert_eq!(
            d.entities[0].shape,
            Shape::Fill { rings: vec![vec![P2(0.0, 0.0), P2(10.0, 0.0), P2(10.0, 10.0), P2(0.0, 10.0)]], solid: true }
        );
        let Shape::Fill { rings, solid: false } = &d.entities[1].shape else { panic!("{:?}", d.entities[1]) };
        assert_eq!(rings.len(), 1);
        assert!(near(rings[0][0], P2(0.0, 0.0)) && near(rings[0][1], P2(10.0, 0.0)), "{:?}", rings[0]);
        assert!(has_point_near(&rings[0], P2(5.0, 5.0)), "the arc's top: {:?}", rings[0]);
        assert!(rings[0].last().is_some_and(|p| !near(*p, P2(0.0, 0.0))), "the ring's end isn't its start again");
    }

    #[test]
    fn an_ellipse_and_a_spline_are_points_on_their_curves() {
        let d = drawing(&pairs(&[
            (0, "ELLIPSE"),
            (10, "0"),
            (20, "0"),
            (11, "10"),
            (21, "0"),
            (40, "0.5"),
            (41, "0"),
            (42, "6.283185307179586"),
            (0, "SPLINE"),
            (70, "8"),
            (71, "2"),
            (72, "6"),
            (73, "3"),
            (74, "0"),
            (40, "0"),
            (40, "0"),
            (40, "0"),
            (40, "1"),
            (40, "1"),
            (40, "1"),
            (10, "0"),
            (20, "0"),
            (10, "5"),
            (20, "10"),
            (10, "10"),
            (20, "0"),
            (0, "SPLINE"),
            (70, "0"),
            (71, "3"),
            (74, "2"),
            (11, "0"),
            (21, "0"),
            (11, "1"),
            (21, "1"),
        ]));
        let Shape::Polyline { pts, closed: true } = &d.entities[0].shape else { panic!("{:?}", d.entities[0]) };
        assert!(has_point_near(pts, P2(10.0, 0.0)) && has_point_near(pts, P2(0.0, 5.0)) && has_point_near(pts, P2(-10.0, 0.0)), "{pts:?}");
        assert_eq!(pts.len(), CURVE_STEPS, "a whole ellipse: its end isn't its start again");
        // A quadratic B-spline over one span is a Bézier: its middle is (p0 + 2 p1 + p2) / 4.
        let Shape::Polyline { pts, closed: false } = &d.entities[1].shape else { panic!("{:?}", d.entities[1]) };
        assert!(near(pts[0], P2(0.0, 0.0)) && near(pts[pts.len() - 1], P2(10.0, 0.0)), "{pts:?}");
        assert!(has_point_near(pts, P2(5.0, 5.0)), "{pts:?}");
        // With fit points only, the line through them.
        assert_eq!(d.entities[2].shape, Shape::Polyline { pts: vec![P2(0.0, 0.0), P2(1.0, 1.0)], closed: false });
    }

    #[test]
    fn a_dimension_draws_its_block_a_solid_fills_and_an_attribute_shows_unless_invisible() {
        let blocks = pairs(&[
            (0, "BLOCK"),
            (2, "*D1"),
            (10, "0"),
            (20, "0"),
            (0, "LINE"),
            (10, "0"),
            (20, "0"),
            (11, "1"),
            (21, "0"),
            (0, "ENDBLK"),
        ]);
        let entities = pairs(&[
            (0, "DIMENSION"),
            (2, "*D1"),
            (10, "0"),
            (20, "0"),
            (0, "SOLID"),
            (10, "0"),
            (20, "0"),
            (11, "10"),
            (21, "0"),
            (12, "0"),
            (22, "10"),
            (13, "10"),
            (23, "10"),
            (0, "ATTRIB"),
            (10, "0"),
            (20, "0"),
            (40, "1"),
            (1, "hidden"),
            (70, "1"),
            (0, "ATTRIB"),
            (10, "0"),
            (20, "0"),
            (40, "1"),
            (1, "shown"),
            (70, "0"),
            (0, "SEQEND"),
        ]);
        let d = read_text(&file("", "", &blocks, &entities)).unwrap();
        assert_eq!(d.entities.len(), 3, "{:?}", d.entities);
        assert_eq!(d.entities[0].shape, Shape::Line(P2(0.0, 0.0), P2(1.0, 0.0)));
        assert_eq!(
            d.entities[1].shape,
            Shape::Fill { rings: vec![vec![P2(0.0, 0.0), P2(10.0, 0.0), P2(10.0, 10.0), P2(0.0, 10.0)]], solid: true }
        );
        assert!(matches!(&d.entities[2].shape, Shape::Text { text, .. } if text == "shown"));
    }

    #[test]
    fn paper_space_and_the_kinds_not_drawn_are_left_out_and_counted() {
        let d = drawing(&pairs(&[
            (0, "LINE"),
            (67, "1"),
            (10, "0"),
            (20, "0"),
            (11, "1"),
            (21, "1"),
            (0, "VIEWPORT"),
            (10, "0"),
            (20, "0"),
            (0, "XLINE"),
            (10, "0"),
            (20, "0"),
            (0, "XLINE"),
            (10, "0"),
            (20, "0"),
            (0, "CIRCLE"),
            (10, "0"),
            (20, "0"),
            (40, "0"),
        ]));
        assert!(d.entities.is_empty());
        let left: Vec<(&str, usize)> = d.left_out.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        assert_eq!(left, [("VIEWPORT", 1), ("XLINE", 2), ("empty", 1), ("paper space", 1)]);
    }

    #[test]
    fn a_binary_file_a_file_that_isnt_dxf_and_a_file_cut_short_are_refused_and_say_why() {
        let e = read(b"AutoCAD Binary DXF\r\n\x1a\0\0\0").unwrap_err();
        assert!(e.contains("binary"), "{e}");
        let e = read(b"hello\nworld\n").unwrap_err();
        assert!(e.contains("line 1") && e.contains("group code"), "{e}");
        let e = read(b"0\nEOF\n").unwrap_err();
        assert!(e.contains("no ENTITIES"), "{e}");
        let e = read(b"0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n").unwrap_err();
        assert!(e.contains("line 7") && e.contains("ends after"), "{e}");
        let e = read_text(&file("", "", "", &pairs(&[(0, "LINE"), (10, "nan"), (20, "0"), (11, "1"), (21, "1")]))).unwrap_err();
        assert!(e.contains("isn't a number"), "{e}");
        let e = read_text(&file("", "", "", &pairs(&[(0, "LINE"), (10, "1e300"), (20, "0"), (11, "1"), (21, "1")]))).unwrap_err();
        assert!(e.contains("past what a drawing holds"), "{e}");
        // Too big, before it's read.
        let e = read(&vec![b' '; MAX_BYTES + 1]).unwrap_err();
        assert!(e.contains("bytes"), "{e}");
    }

    #[test]
    fn a_file_in_a_code_page_or_with_a_byte_order_mark_still_reads_and_blank_lines_are_passed_over() {
        let mut bytes = b"\xEF\xBB\xBF0\nSECTION\n2\nENTITIES\n\n0\nTEXT\n10\n0\n20\n0\n40\n1\n1\ncaf".to_vec();
        bytes.push(0xE9);
        bytes.extend_from_slice(b"\r\n0\nENDSEC\n0\nEOF\n\n\n");
        let d = read(&bytes).unwrap();
        assert!(matches!(&d.entities[0].shape, Shape::Text { text, .. } if text == "café"), "{:?}", d.entities);
        // Windows line ends throughout.
        let d = read_text(&file("", "", "", &pairs(&[(0, "POINT"), (10, "1"), (20, "2")])).replace('\n', "\r\n")).unwrap();
        assert_eq!(shapes(&d), [&Shape::Point(P2(1.0, 2.0))]);
    }

    #[test]
    fn hostile_counts_and_arrays_stop_at_the_bounds_rather_than_run_away() {
        // A count the vertices don't bear out is ignored.
        let d = drawing(&pairs(&[(0, "LWPOLYLINE"), (90, "1000000000"), (10, "0"), (20, "0"), (10, "1"), (20, "1")]));
        assert_eq!(shapes(&d), [&Shape::Polyline { pts: vec![P2(0.0, 0.0), P2(1.0, 1.0)], closed: false }]);
        // An array past what the drawing may draw is refused, not drawn for an hour.
        let blocks = pairs(&[(0, "BLOCK"), (2, "A"), (10, "0"), (20, "0"), (0, "POINT"), (10, "0"), (20, "0"), (0, "ENDBLK")]);
        let e = read_text(&file(
            "",
            "",
            &blocks,
            &pairs(&[(0, "INSERT"), (2, "A"), (10, "0"), (20, "0"), (70, "1000000"), (71, "1000000"), (44, "1"), (45, "1")]),
        ))
        .unwrap_err();
        assert!(e.contains("more than"), "{e}");
        // A block of a block of arrays that multiplies past the work bound is refused too.
        let blocks = pairs(&[
            (0, "BLOCK"),
            (2, "S"),
            (10, "0"),
            (20, "0"),
            (0, "INSERT"),
            (2, "S"),
            (10, "1"),
            (20, "0"),
            (70, "4"),
            (71, "4"),
            (44, "1"),
            (45, "1"),
            (0, "ENDBLK"),
        ]);
        let e = read_text(&file("", "", &blocks, &pairs(&[(0, "INSERT"), (2, "S"), (10, "0"), (20, "0")]))).unwrap_err();
        assert!(e.contains("more than"), "{e}");
        // A hatch whose counts outrun its values ends where they do.
        let d = drawing(&pairs(&[
            (0, "HATCH"),
            (91, "5"),
            (92, "2"),
            (72, "0"),
            (73, "1"),
            (93, "100000"),
            (10, "0"),
            (20, "0"),
            (10, "1"),
            (20, "0"),
            (10, "1"),
            (20, "1"),
        ]));
        assert_eq!(d.entities.len(), 1, "{:?}", d.entities);
        // A spline with knots that don't match its controls, and a degree past sense, draws nothing and breaks nothing.
        let d = drawing(&pairs(&[(0, "SPLINE"), (71, "99"), (40, "0"), (10, "0"), (20, "0"), (10, "1"), (20, "1")]));
        assert!(d.entities.is_empty());
        // Negative and zero scales, and a degenerate ellipse.
        let d = read_text(&file(
            "",
            "",
            &blocks,
            &pairs(&[
                (0, "INSERT"),
                (2, "S"),
                (10, "0"),
                (20, "0"),
                (41, "0"),
                (0, "ELLIPSE"),
                (10, "0"),
                (20, "0"),
                (11, "0"),
                (21, "0"),
                (40, "1"),
            ]),
        ))
        .unwrap();
        assert_eq!(d.left_out.get("empty"), Some(&2));
        // Scales that multiply past any number through nested blocks leave the shape out.
        let blocks = pairs(&[
            (0, "BLOCK"),
            (2, "BIG"),
            (10, "0"),
            (20, "0"),
            (0, "POINT"),
            (10, "1"),
            (20, "1"),
            (0, "INSERT"),
            (2, "BIG"),
            (10, "0"),
            (20, "0"),
            (41, "1e11"),
            (42, "1e11"),
            (0, "ENDBLK"),
        ]);
        let d = read_text(&file("", "", &blocks, &pairs(&[(0, "INSERT"), (2, "BIG"), (10, "0"), (20, "0")]))).unwrap();
        assert!(d.left_out.get("past what a drawing holds").is_some_and(|n| *n > 0), "{:?}", d.left_out);
        assert!(d.entities.iter().all(|e| e.shape.fits()));
    }
}
