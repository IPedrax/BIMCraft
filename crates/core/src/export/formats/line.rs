//! Drawings as lines: DXF, written from the public DXF reference as AutoCAD 2004's (AC1018), which every CAD program opens.
//!
//! - **Where.** A plan, a section or an elevation is written in model millimetres where the model has it (`$INSUNITS` 4), so it
//!   can be referenced under another drawing; a sheet and a schedule in the paper's millimetres.
//! - **Layers** by the category of the element drawn (`wall`, `door`…, on a sheet too), a sheet's title block on `sheet`
//!   and what no element draws (a view's title) on `0`.
//! - **Lines** are LINEs and LWPOLYLINEs with the pen's weight on paper (group 370) and their dashes as a linetype of their
//!   own; fills are solid HATCHes. Colours are true colours (420), with the nearest standard index (62) for a reader that
//!   knows only those. Lettering is the lines it is drawn with.
//! - **Left out:** pictures (a section's shading, a 3D view, an image), which DXF holds only as files beside it, and white
//!   fills, which only hide what is under them on paper.
//!
//! The file is the sections AutoCAD asks of an AC1018 file: a header, the tables (each entry with a handle and its owner), the
//! model and paper space blocks, the entities, and the objects' root dictionary.

use std::collections::BTreeMap;
use std::fmt::Write;

use super::super::{Content, Ctx, Family, Format, Options, Out, Paged, Prepared, Takes};
use super::styled;
use crate::render::{Prim, Rgb, View};
use crate::sheet::DENSITY;

/// How often, in primitives, the writer asks whether to stop.
const ASK_EVERY: usize = 4096;
/// The most characters a TEXT's value is written with.
const MAX_TEXT: usize = 1000;
/// The line weights a file may hold, hundredths of a mm (DXF's group 370 takes only these).
const WEIGHTS: [i32; 24] = [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211];

pub struct Dxf;

impl Format for Dxf {
    fn id(&self) -> &'static str {
        "dxf"
    }
    fn label(&self) -> &'static str {
        "DXF"
    }
    fn extension(&self) -> &'static str {
        "dxf"
    }
    fn family(&self) -> Family {
        Family::Line
    }
    fn takes(&self) -> &'static [Takes] {
        &[Takes::Drawing]
    }
    fn write(&self, items: &[Prepared], options: &Options, ctx: &Ctx) -> Result<Vec<Out>, String> {
        let [item] = items else { return Err("a DXF file holds one view or sheet".to_string()) };
        let Content::Drawing(paged) = &item.content else { return Err(format!("{} has no drawing to write", item.title)) };
        Ok(vec![Out { suffix: None, bytes: dxf(paged, options, ctx)?.into_bytes() }])
    }
}

/// The standard colour index nearest `c`: 7, the drawing's ink, for black.
fn index(c: Rgb) -> u8 {
    if c == Rgb(0, 0, 0) {
        return 7;
    }
    let d = |a: Rgb| {
        let (r, g, b) = (i32::from(a.0) - i32::from(c.0), i32::from(a.1) - i32::from(c.1), i32::from(a.2) - i32::from(c.2));
        r * r + g * g + b * b
    };
    (1..=255u8).filter_map(|i| Some((i, crate::dxf::aci(i)?))).min_by_key(|(_, rgb)| d(*rgb)).map_or(7, |(i, _)| i)
}

/// The nearest weight a file may hold to `mm` on paper, hundredths of a mm.
fn weight(mm: f64) -> i32 {
    let w = (mm * 100.0).round() as i32;
    WEIGHTS.iter().copied().min_by_key(|v| (v - w).abs()).unwrap_or(0)
}

/// A name a DXF table takes: no character it refuses, and what isn't ASCII as `\U+XXXX`, as a file whose header says
/// ANSI_1252 writes it (a character past four digits, `_`).
fn table_name(s: &str) -> String {
    let mut n = String::new();
    for c in s.chars() {
        match c {
            c if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '$') => n.push(c),
            c if !c.is_ascii() && u32::from(c) <= 0xFFFF => {
                let _ = write!(n, "\\U+{:04X}", u32::from(c));
            }
            _ => n.push('_'),
        }
    }
    if n.is_empty() { "0".to_string() } else { n }
}

/// A point as the file holds it, its numbers as [`num`] writes them: what a reader takes back.
fn written(q: (f64, f64)) -> (f64, f64) {
    (num(q.0).parse().unwrap_or(0.0), num(q.1).parse().unwrap_or(0.0))
}

/// Twice the area a ring of points closes.
fn area2(r: &[(f64, f64)]) -> f64 {
    r.iter().zip(r.iter().cycle().skip(1)).map(|(a, b)| a.0 * b.1 - b.0 * a.1).sum()
}

/// A number as DXF writes it: finite, no exponent a reader might not take.
fn num(v: f64) -> String {
    let v = if v.is_finite() { v + 0.0 } else { 0.0 };
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".to_string() } else { s.to_string() }
}

/// The file as it is written: its text and the next handle.
struct Writer {
    out: String,
    next: u32,
}

impl Writer {
    fn pair(&mut self, code: i32, value: impl std::fmt::Display) {
        let _ = write!(self.out, "{code}\n{value}\n");
    }

    fn handle(&mut self) -> String {
        self.next += 1;
        format!("{:X}", self.next)
    }

    /// An entity's start: its kind, handle, owner (model space's record) and common groups.
    fn entity(&mut self, kind: &str, owner: &str, layer: &str, linetype: Option<&str>, color: Rgb, lw: Option<i32>) {
        let h = self.handle();
        self.pair(0, kind);
        self.pair(5, h);
        self.pair(330, owner);
        self.pair(100, "AcDbEntity");
        self.pair(8, layer);
        if let Some(lt) = linetype {
            self.pair(6, lt);
        }
        self.pair(62, index(color));
        if let Some(lw) = lw {
            self.pair(370, lw);
        }
        self.pair(420, (u32::from(color.0) << 16) | (u32::from(color.1) << 8) | u32::from(color.2));
    }

    /// A symbol table's start, and its handle for its entries' owner.
    fn table(&mut self, name: &str, count: usize) -> String {
        let h = self.handle();
        self.pair(0, "TABLE");
        self.pair(2, name);
        self.pair(5, &h);
        self.pair(330, 0);
        self.pair(100, "AcDbSymbolTable");
        self.pair(70, count);
        h
    }

    /// A table entry's start.
    fn record(&mut self, kind: &str, owner: &str, subclass: &str, name: &str) -> String {
        let h = self.handle();
        self.pair(0, kind);
        self.pair(if kind == "DIMSTYLE" { 105 } else { 5 }, &h);
        self.pair(330, owner);
        self.pair(100, "AcDbSymbolTableRecord");
        self.pair(100, subclass);
        self.pair(2, name);
        self.pair(70, 0);
        h
    }
}

/// `p` as the drawing's place in its file.
type Place<'a> = &'a dyn Fn((f32, f32)) -> (f64, f64);

/// The DXF text of `paged`, in the colours and lines `options` asks for.
fn dxf(paged: &Paged, options: &Options, ctx: &Ctx) -> Result<String, String> {
    let restyled = styled(&paged.drawing, &options.page);
    let d = restyled.as_ref().unwrap_or(&paged.drawing);
    // Where a pixel is: the model's mm for a view, the paper's for a sheet.
    let view: View = paged.world.unwrap_or(d.view);
    let unit = 1.0 / view.scale;
    let place = |p: (f32, f32)| {
        let q = view.mm(p);
        (q.0, q.1)
    };
    let place: Place = &place;
    // The layers and linetypes the entities name, found first: the tables come before them.
    let layer_of = |i: usize| {
        paged.drawn.get(i).copied().flatten().and_then(|id| paged.categories.get(&id)).map_or_else(|| "0".to_string(), |c| table_name(c))
    };
    // An imported drawing the plan shows goes out from its own entities, not from what the plan draws of it.
    let cad_owners: std::collections::BTreeSet<crate::ids::Id> = paged.cad.iter().map(|c| c.owner).collect();
    let skip = |i: usize| !cad_owners.is_empty() && paged.drawn.get(i).copied().flatten().is_some_and(|id| cad_owners.contains(&id));
    let mut layers: BTreeMap<String, ()> = BTreeMap::from([("0".to_string(), ())]);
    let mut linetypes: BTreeMap<Vec<i64>, String> = BTreeMap::new();
    for c in &paged.cad {
        layers.insert(table_name(&c.layer), ());
        if let super::super::CadShape::Path { dash: Some(dash), .. } = c.shape {
            let key = mm_key(dash);
            if !key.is_empty() && !linetypes.contains_key(&key) {
                let name = format!("BIMCRAFT_{}", linetypes.len() + 1);
                linetypes.insert(key, name);
            }
        }
    }
    for (i, p) in d.paint_prims() {
        if i.is_multiple_of(ASK_EVERY) && (ctx.cancelled)() {
            return Err("cancelled".to_string());
        }
        if skip(i) {
            continue;
        }
        layers.insert(layer_of(i), ());
        if let Prim::Line { dash: Some(dash), .. } = p {
            let key = pattern_key(dash, unit);
            if !key.is_empty() && !linetypes.contains_key(&key) {
                let name = format!("BIMCRAFT_{}", linetypes.len() + 1);
                linetypes.insert(key, name);
            }
        }
    }
    let mut w = Writer { out: String::new(), next: 0 };
    // CLASSES, TABLES, BLOCKS and OBJECTS name handles; the header, written last, says the next one.
    let model_record = tables(&mut w, &layers, &linetypes);
    entities(&mut w, d, &model_record, place, unit, &layer_of, &linetypes, &skip, &paged.cad, ctx)?;
    let body = std::mem::take(&mut w.out);
    // The extents, for a reader that zooms to them.
    let pts: Vec<(f64, f64)> = d
        .prims
        .iter()
        .flat_map(Prim::paint)
        .flat_map(|p| match p {
            Prim::Line { pts, .. } => pts.clone(),
            Prim::Fill { rings, .. } => rings.iter().flatten().copied().collect(),
            Prim::Image { .. } | Prim::Text { .. } => Vec::new(),
        })
        .map(place)
        .collect();
    let lo = pts.iter().fold((f64::MAX, f64::MAX), |a, p| (a.0.min(p.0), a.1.min(p.1)));
    let hi = pts.iter().fold((f64::MIN, f64::MIN), |a, p| (a.0.max(p.0), a.1.max(p.1)));
    let (lo, hi) = if pts.is_empty() { ((0.0, 0.0), (0.0, 0.0)) } else { (lo, hi) };
    let mut head = Writer { out: String::new(), next: w.next };
    head.pair(0, "SECTION");
    head.pair(2, "HEADER");
    for (name, code, value) in [
        ("$ACADVER", 1, "AC1018".to_string()),
        ("$DWGCODEPAGE", 3, "ANSI_1252".to_string()),
        ("$INSUNITS", 70, "4".to_string()),
        ("$MEASUREMENT", 70, "1".to_string()),
        ("$LUNITS", 70, "2".to_string()),
        ("$LWDISPLAY", 290, "1".to_string()),
        ("$LTSCALE", 40, "1".to_string()),
        ("$PSLTSCALE", 70, "0".to_string()),
    ] {
        head.pair(9, name);
        head.pair(code, value);
    }
    for (name, p) in [("$EXTMIN", lo), ("$EXTMAX", hi)] {
        head.pair(9, name);
        head.pair(10, num(p.0));
        head.pair(20, num(p.1));
        head.pair(30, 0);
    }
    head.pair(9, "$HANDSEED");
    head.pair(5, format!("{:X}", w.next + 1));
    head.pair(0, "ENDSEC");
    head.pair(0, "SECTION");
    head.pair(2, "CLASSES");
    head.pair(0, "ENDSEC");
    Ok(head.out + &body + "0\nEOF\n")
}

/// A dash pattern's lengths, drawing units, as whole hundredths: what makes two the same linetype. Empty when it has no
/// length (drawn solid).
fn pattern_key(dash: &[f32], unit: f64) -> Vec<i64> {
    let key: Vec<i64> = dash.iter().map(|v| (f64::from(*v) * unit * 100.0).round() as i64).collect();
    if key.iter().all(|v| *v <= 0) || key.len() > 12 { Vec::new() } else { key }
}

/// The TABLES and BLOCKS sections, writing out the layers and linetypes: model space's block record's handle, which every
/// entity is owned by.
fn tables(w: &mut Writer, layers: &BTreeMap<String, ()>, linetypes: &BTreeMap<Vec<i64>, String>) -> String {
    w.pair(0, "SECTION");
    w.pair(2, "TABLES");
    // The active viewport, which a reader opens on.
    let t = w.table("VPORT", 1);
    w.record("VPORT", &t, "AcDbViewportTableRecord", "*Active");
    for (code, v) in [(10, 0.0), (20, 0.0), (11, 1.0), (21, 1.0), (12, 0.0), (22, 0.0), (40, 1000.0), (41, 1.5)] {
        w.pair(code, num(v));
    }
    w.pair(0, "ENDTAB");
    // Linetypes: the three every file has, and the drawing's dashes.
    let t = w.table("LTYPE", 3 + linetypes.len());
    for name in ["ByBlock", "ByLayer", "Continuous"] {
        w.record("LTYPE", &t, "AcDbLinetypeTableRecord", name);
        w.pair(3, if name == "Continuous" { "Solid line" } else { "" });
        w.pair(72, 65);
        w.pair(73, 0);
        w.pair(40, 0);
    }
    for (key, name) in linetypes {
        w.record("LTYPE", &t, "AcDbLinetypeTableRecord", name);
        w.pair(3, "BIMCraft");
        w.pair(72, 65);
        w.pair(73, key.len());
        w.pair(40, num(key.iter().map(|v| v.abs() as f64 / 100.0).sum()));
        for (k, v) in key.iter().enumerate() {
            // On, then off: a gap is negative.
            let len = *v as f64 / 100.0;
            w.pair(49, num(if k % 2 == 0 { len } else { -len }));
            w.pair(74, 0);
        }
    }
    w.pair(0, "ENDTAB");
    let t = w.table("LAYER", layers.len());
    for name in layers.keys() {
        w.record("LAYER", &t, "AcDbLayerTableRecord", name);
        w.pair(62, 7);
        w.pair(6, "Continuous");
        w.pair(370, -3);
    }
    w.pair(0, "ENDTAB");
    let t = w.table("STYLE", 1);
    w.record("STYLE", &t, "AcDbTextStyleTableRecord", "Standard");
    for (code, v) in [(40, "0"), (41, "1"), (50, "0"), (71, "0"), (42, "2.5"), (3, "txt"), (4, "")] {
        w.pair(code, v);
    }
    w.pair(0, "ENDTAB");
    for name in ["VIEW", "UCS"] {
        w.table(name, 0);
        w.pair(0, "ENDTAB");
    }
    let t = w.table("APPID", 1);
    w.record("APPID", &t, "AcDbRegAppTableRecord", "ACAD");
    w.pair(0, "ENDTAB");
    let t = w.table("DIMSTYLE", 1);
    w.pair(100, "AcDbDimStyleTable");
    w.pair(71, 0);
    w.record("DIMSTYLE", &t, "AcDbDimStyleTableRecord", "Standard");
    w.pair(0, "ENDTAB");
    let t = w.table("BLOCK_RECORD", 2);
    let model = w.record("BLOCK_RECORD", &t, "AcDbBlockTableRecord", "*Model_Space");
    let paper = w.record("BLOCK_RECORD", &t, "AcDbBlockTableRecord", "*Paper_Space");
    w.pair(0, "ENDTAB");
    w.pair(0, "ENDSEC");
    // The two spaces' blocks, empty: model space's entities are in ENTITIES.
    w.pair(0, "SECTION");
    w.pair(2, "BLOCKS");
    for (name, record, paper_space) in [("*Model_Space", &model, false), ("*Paper_Space", &paper, true)] {
        let h = w.handle();
        w.pair(0, "BLOCK");
        w.pair(5, h);
        w.pair(330, record);
        w.pair(100, "AcDbEntity");
        if paper_space {
            w.pair(67, 1);
        }
        w.pair(8, "0");
        w.pair(100, "AcDbBlockBegin");
        w.pair(2, name);
        w.pair(70, 0);
        for code in [10, 20, 30] {
            w.pair(code, 0);
        }
        w.pair(3, name);
        w.pair(1, "");
        let h = w.handle();
        w.pair(0, "ENDBLK");
        w.pair(5, h);
        w.pair(330, record);
        w.pair(100, "AcDbEntity");
        if paper_space {
            w.pair(67, 1);
        }
        w.pair(8, "0");
        w.pair(100, "AcDbBlockEnd");
    }
    w.pair(0, "ENDSEC");
    model
}

/// The ENTITIES and OBJECTS sections: how many entities were written.
/// A line or a polyline of points as written (model or paper mm); false, written not at all, when it has no length.
#[allow(clippy::too_many_arguments)]
fn path(w: &mut Writer, owner: &str, layer: &str, lt: Option<&str>, color: Rgb, lw: i32, pts: &[(f64, f64)], closed: bool) -> bool {
    // A line of no length as it is written, which a reader takes for an empty entity, is left out.
    if pts.len() < 2 || pts.iter().all(|q| *q == pts[0]) {
        return false;
    }
    if pts.len() == 2 && !closed {
        w.entity("LINE", owner, layer, lt, color, Some(lw));
        w.pair(100, "AcDbLine");
        for (code, v) in [(10, pts[0].0), (20, pts[0].1), (30, 0.0), (11, pts[1].0), (21, pts[1].1), (31, 0.0)] {
            w.pair(code, num(v));
        }
    } else {
        w.entity("LWPOLYLINE", owner, layer, lt, color, Some(lw));
        w.pair(100, "AcDbPolyline");
        w.pair(90, pts.len());
        w.pair(70, if closed { 1 } else { 0 } | 128);
        for q in pts {
            w.pair(10, num(q.0));
            w.pair(20, num(q.1));
        }
    }
    true
}

/// A solid hatch of rings as written; false, written not at all, when none of them closes an area.
fn hatch(w: &mut Writer, owner: &str, layer: &str, color: Rgb, rings: Vec<Vec<(f64, f64)>>) -> bool {
    // A ring that closes no area as it is written, likewise.
    let rings: Vec<Vec<(f64, f64)>> = rings.into_iter().filter(|r| r.len() >= 3 && area2(r) != 0.0).collect();
    if rings.is_empty() {
        return false;
    }
    w.entity("HATCH", owner, layer, None, color, None);
    w.pair(100, "AcDbHatch");
    for (code, v) in [(10, 0), (20, 0), (30, 0), (210, 0), (220, 0), (230, 1)] {
        w.pair(code, v);
    }
    w.pair(2, "SOLID");
    w.pair(70, 1);
    w.pair(71, 0);
    w.pair(91, rings.len());
    for r in &rings {
        w.pair(92, 2);
        w.pair(72, 0);
        w.pair(73, 1);
        w.pair(93, r.len());
        for q in r {
            w.pair(10, num(q.0));
            w.pair(20, num(q.1));
        }
        w.pair(97, 0);
    }
    w.pair(75, 0);
    w.pair(76, 1);
    w.pair(98, 0);
    true
}

/// A text's words as a DXF 2004 string in ANSI_1252: what isn't ASCII as `\U+XXXX` (past four digits, `?`), no control.
fn text_value(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().filter(|c| !c.is_control()).take(MAX_TEXT) {
        match u32::from(c) {
            0x20..=0x7e => out.push(c),
            n if n <= 0xFFFF => {
                let _ = write!(out, "\\U+{n:04X}");
            }
            _ => out.push('?'),
        }
    }
    out
}

/// One line of an imported drawing's lettering as TEXT, at `at` in its alignment.
#[allow(clippy::too_many_arguments)]
fn text(
    w: &mut Writer,
    owner: &str,
    layer: &str,
    color: Rgb,
    at: (f64, f64),
    height: f64,
    angle: f64,
    words: &str,
    align: crate::dxf::Align,
) {
    use crate::dxf::{Horizontal, Vertical};
    let h = match align.h {
        Horizontal::Left => 0,
        Horizontal::Center => 1,
        Horizontal::Right => 2,
    };
    let v = match align.v {
        Vertical::Baseline => 0,
        Vertical::Bottom => 1,
        Vertical::Middle => 2,
        Vertical::Top => 3,
    };
    w.entity("TEXT", owner, layer, None, color, None);
    w.pair(100, "AcDbText");
    for (code, x) in [(10, at.0), (20, at.1), (30, 0.0), (40, height)] {
        w.pair(code, num(x));
    }
    w.pair(1, text_value(words));
    w.pair(50, num(angle.rem_euclid(360.0)));
    w.pair(72, h);
    // Aligned other than at its baseline's left, a reader places it by its alignment point.
    for (code, x) in [(11, at.0), (21, at.1), (31, 0.0)] {
        w.pair(code, num(x));
    }
    w.pair(100, "AcDbText");
    w.pair(73, v);
}

/// A dash and gap in mm as a linetype's key, as [`pattern_key`] makes one.
fn mm_key([a, b]: [f64; 2]) -> Vec<i64> {
    let key = vec![(a * 100.0).round() as i64, (b * 100.0).round() as i64];
    if key.iter().all(|v| *v <= 0) { Vec::new() } else { key }
}

#[allow(clippy::too_many_arguments)]
fn entities(
    w: &mut Writer,
    d: &crate::render::Drawing,
    owner: &str,
    place: Place,
    unit: f64,
    layer_of: &dyn Fn(usize) -> String,
    linetypes: &BTreeMap<Vec<i64>, String>,
    skip: &dyn Fn(usize) -> bool,
    cad: &[super::super::CadDrawn],
    ctx: &Ctx,
) -> Result<usize, String> {
    use super::super::CadShape;
    w.pair(0, "SECTION");
    w.pair(2, "ENTITIES");
    let mut n = 0;
    let finite = |q: &(f64, f64)| q.0.is_finite() && q.1.is_finite();
    for (i, p) in d.paint_prims() {
        if i.is_multiple_of(ASK_EVERY) && (ctx.cancelled)() {
            return Err("cancelled".to_string());
        }
        // An imported drawing is written from its own entities, below.
        if skip(i) {
            continue;
        }
        let layer = layer_of(i);
        match p {
            Prim::Line { pts, width, color, dash, closed } => {
                let pts: Vec<(f64, f64)> = pts.iter().map(|q| place(*q)).filter(finite).map(written).collect();
                let lt = dash.as_ref().and_then(|dash| linetypes.get(&pattern_key(dash, unit))).map(String::as_str);
                if path(w, owner, &layer, lt, *color, weight(f64::from(*width) / DENSITY), &pts, *closed) {
                    n += 1;
                }
            }
            Prim::Fill { rings, color } if *color != Rgb(255, 255, 255) => {
                let rings = rings.iter().map(|r| r.iter().map(|q| place(*q)).filter(finite).map(written).collect()).collect();
                if hatch(w, owner, &layer, *color, rings) {
                    n += 1;
                }
            }
            Prim::Fill { .. } | Prim::Image { .. } | Prim::Text { .. } => {}
        }
    }
    // The imported drawings, on their own layers, already in the model's mm.
    for (i, c) in cad.iter().enumerate() {
        if i.is_multiple_of(ASK_EVERY) && (ctx.cancelled)() {
            return Err("cancelled".to_string());
        }
        let layer = table_name(&c.layer);
        let at = |p: &crate::geom::P2| written((p.0, p.1));
        let wrote = match &c.shape {
            CadShape::Path { pts, closed, pen, dash } => {
                let pts: Vec<(f64, f64)> = pts.iter().map(at).filter(finite).collect();
                let lt = dash.and_then(|d| linetypes.get(&mm_key(d))).map(String::as_str);
                path(w, owner, &layer, lt, c.color, weight(*pen), &pts, *closed)
            }
            CadShape::Fill { rings } => {
                hatch(w, owner, &layer, c.color, rings.iter().map(|r| r.iter().map(at).filter(finite).collect()).collect())
            }
            CadShape::Text { at: p, height, angle, text: words, align } => {
                let q = at(p);
                let ok = finite(&q) && height.is_finite() && angle.is_finite();
                if ok {
                    text(w, owner, &layer, c.color, q, *height, *angle, words, *align);
                }
                ok
            }
        };
        if wrote {
            n += 1;
        }
    }
    w.pair(0, "ENDSEC");
    // The root dictionary, and the groups' it names.
    w.pair(0, "SECTION");
    w.pair(2, "OBJECTS");
    let root = w.handle();
    let groups = w.handle();
    w.pair(0, "DICTIONARY");
    w.pair(5, &root);
    w.pair(330, 0);
    w.pair(100, "AcDbDictionary");
    w.pair(281, 1);
    w.pair(3, "ACAD_GROUP");
    w.pair(350, &groups);
    w.pair(0, "DICTIONARY");
    w.pair(5, &groups);
    w.pair(330, &root);
    w.pair(100, "AcDbDictionary");
    w.pair(281, 1);
    w.pair(0, "ENDSEC");
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dxf::{Color, Shape, read_text};
    use crate::engine::Session;
    use crate::export::prepare;
    use crate::geom::P2;

    /// A box of walls with a door, and a sheet with the Level 1 plan on it.
    fn project() -> Session {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
               door {"at": [2000, 0]}
               sheet {"name": "Plans"}
               place {"sheet": "A101", "view": "Level 1"}"#,
        )
        .unwrap();
        s
    }

    fn write(s: &Session, view: &str) -> crate::dxf::Dxf {
        let m = crate::link::seen(&s.model);
        let id = m.view_named(view).or_else(|| m.sheet_numbered(view)).unwrap();
        let p = prepare(&s.model, &m, Some(id), Takes::Drawing).unwrap();
        let out = Dxf.write(&[p], &Options::default(), &Ctx::quiet()).unwrap();
        let text = String::from_utf8(out[0].bytes.clone()).unwrap();
        // Every handle once, under the seed the header gives (after the header, whose seed is a 5 too).
        let mut lines = text.split_once("0\nENDSEC\n").map_or("", |(_, rest)| rest).lines();
        let mut handles = Vec::new();
        while let (Some(code), Some(value)) = (lines.next(), lines.next()) {
            if matches!(code.trim(), "5" | "105") {
                handles.push(u32::from_str_radix(value, 16).unwrap());
            }
        }
        let seed = text.split("$HANDSEED\n5\n").nth(1).and_then(|r| r.lines().next()).unwrap();
        let seed = u32::from_str_radix(seed, 16).unwrap();
        assert!(handles.iter().all(|h| *h < seed), "handles under the seed {seed}");
        let mut unique = handles.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), handles.len(), "no handle twice");
        read_text(&text).unwrap()
    }

    #[test]
    fn a_plan_is_written_in_model_millimetres_where_the_model_has_it() {
        let f = write(&project(), "Level 1");
        assert_eq!((f.version.as_str(), f.unit.as_str()), ("AC1018", "mm"));
        // The walls' lines, on their layer, where the walls are: their outside faces at x 0 and 8000.
        let walls: Vec<&crate::dxf::Entity> = f.entities.iter().filter(|e| e.layer == "wall").collect();
        assert!(!walls.is_empty(), "{:?}", f.layers);
        let [lo, hi] = crate::dxf::union(walls.iter().map(|e| e.shape.bounds())).unwrap();
        assert!(lo.0 < 0.0 && lo.0 > -400.0 && hi.0 > 8000.0 && hi.0 < 8400.0, "{lo:?} {hi:?}");
        assert!(lo.1 < 0.0 && hi.1 > 6000.0 && hi.1 < 6400.0, "{lo:?} {hi:?}");
        // Cut walls are filled solid, a door has a layer of its own, and the lines carry their weights.
        assert!(walls.iter().any(|e| matches!(e.shape, Shape::Fill { solid: true, .. })));
        assert!(f.layer("door").is_some());
        assert!(f.entities.iter().any(|e| e.weight.is_some_and(|w| w > 0.0)));
        assert!(f.entities.iter().all(|e| matches!(e.color, Color::True(..))));
    }

    #[test]
    fn a_sheet_is_written_on_its_paper() {
        let f = write(&project(), "A101");
        let [lo, hi] = f.bounds().unwrap();
        // A3 landscape, 420 by 297 mm, the border inside it.
        assert!(lo.0 >= -0.5 && lo.1 >= -0.5 && hi.0 <= 420.5 && hi.1 <= 297.5, "{lo:?} {hi:?}");
        assert!(hi.0 > 380.0, "{hi:?}");
        // Its title block on the sheet's layer, the plan's walls on theirs.
        assert!(f.entities.iter().any(|e| e.layer == "sheet"));
        assert!(f.entities.iter().any(|e| e.layer == "wall"));
        let _ = P2(0.0, 0.0);
    }

    #[test]
    fn colours_and_weights_are_the_files_own() {
        assert_eq!(index(Rgb(0, 0, 0)), 7);
        assert_eq!(index(Rgb(250, 5, 5)), 1);
        assert_eq!(index(Rgb(128, 128, 128)), 8);
        assert_eq!(weight(0.35), 35);
        assert_eq!(weight(0.52), 53);
        assert_eq!(weight(9.0), 211);
        assert_eq!(weight(-1.0), 0);
        assert_eq!(table_name("generic model"), "generic_model");
        assert_eq!(num(1e-9), "0");
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(f64::NAN), "0");
        assert_eq!(num(1234.5), "1234.5");
    }

    #[test]
    fn a_name_in_any_script_is_written_as_the_files_code_page_reads_it() {
        // The header says ANSI_1252: what isn't ASCII goes as `\U+XXXX`, and our reader takes it back whole.
        assert_eq!(table_name("Elétrica 电"), "El\\U+00E9trica_\\U+7535");
        assert_eq!(table_name("MOBILIÁRIO"), "MOBILI\\U+00C1RIO");
        // A character past the four digits `\U+` holds, and those a table refuses, are `_`.
        assert_eq!(table_name("a🙂b/c"), "a_b_c");
        let text = format!(
            "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\n{0}\n70\n0\n62\n7\n0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n{0}\n10\n0\n20\n0\n11\n10\n21\n0\n0\nENDSEC\n0\nEOF\n",
            table_name("Elétrica 电")
        );
        let f = read_text(&text).unwrap();
        assert!(f.layer("Elétrica_电").is_some(), "{:?}", f.layers);
        assert_eq!(f.entities[0].layer, "Elétrica_电");
    }

    #[test]
    fn a_line_of_no_length_and_a_ring_of_no_area_are_left_out() {
        let mut d = crate::render::Drawing::new(View { scale: 1.0, cx: 0.0, cy: 0.0, width: 100, height: 100, angle: 0.0 });
        let line = |pts: Vec<(f32, f32)>| Prim::Line { pts, width: 1.0, color: Rgb(0, 0, 0), dash: None, closed: false };
        d.prims.push(line(vec![(0.0, 0.0), (10.0, 0.0)]));
        // The same point twice, and a polyline whose points round to one.
        d.prims.push(line(vec![(5.0, 5.0), (5.0, 5.0)]));
        d.prims.push(line(vec![(1.0, 1.0), (1.000_000_1, 1.0), (1.0, 1.000_000_1)]));
        // A fill with one ring of no area, beside one that has some; and a fill of no area at all.
        let ring = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        let flat = vec![(0.0, 0.0), (5.0, 0.0), (10.0, 0.0)];
        d.prims.push(Prim::Fill { rings: vec![ring, flat.clone()], color: Rgb(0, 0, 0) });
        d.prims.push(Prim::Fill { rings: vec![flat], color: Rgb(0, 0, 0) });
        let mut w = Writer { out: String::new(), next: 0 };
        let place = |p: (f32, f32)| (f64::from(p.0), f64::from(p.1));
        let n = entities(&mut w, &d, "1F", &place, 1.0, &|_| "0".to_string(), &BTreeMap::new(), &|_| false, &[], &Ctx::quiet()).unwrap();
        assert_eq!(n, 2, "{}", w.out);
        let count = |kind: &str| w.out.matches(&format!("0\n{kind}\n")).count();
        assert_eq!((count("LINE"), count("LWPOLYLINE"), count("HATCH")), (1, 0, 1), "{}", w.out);
        // The hatch keeps its ring with an area, only.
        assert!(w.out.contains("91\n1\n"), "{}", w.out);
    }

    #[test]
    fn an_imported_drawing_goes_out_on_its_own_layers_with_its_text_as_text() {
        // A drawing in mm with a line on a layer named with an accent, written as a 2004 file writes it, and a turned text.
        let source = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1018\n9\n$DWGCODEPAGE\n3\nANSI_1252\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n\
            0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\nMOBILI\\U+00C1RIO\n70\n0\n62\n1\n0\nLAYER\n2\nTEXTO\n70\n0\n62\n3\n0\nENDTAB\n0\nENDSEC\n\
            0\nSECTION\n2\nENTITIES\n0\nLINE\n8\nMOBILI\\U+00C1RIO\n10\n0\n20\n0\n11\n1000\n21\n0\n\
            0\nTEXT\n8\nTEXTO\n10\n100\n20\n200\n40\n250\n50\n30\n1\nSala \\U+00E9\n0\nENDSEC\n0\nEOF\n";
        let dir = std::env::temp_dir().join(format!("bimcraft-test-{}-cad-own-layers", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("planta.dxf");
        std::fs::write(&path, source).unwrap();
        let mut s = Session::new();
        s.execute("import_cad", &serde_json::json!({"path": path, "position": "origin", "at": [5000, 3000]})).unwrap();
        let f = write(&s, "Level 1");
        let _ = std::fs::remove_dir_all(&dir);
        // Its own layers, the accent whole, and nothing of it on one `cadimport` layer.
        assert!(f.layer("MOBILIÁRIO").is_some() && f.layer("TEXTO").is_some(), "{:?}", f.layers);
        assert!(f.entities.iter().all(|e| e.layer != "cadimport"), "{:?}", f.entities);
        // The line where the drawing is placed.
        let lines: Vec<&crate::dxf::Entity> = f.entities.iter().filter(|e| e.layer == "MOBILIÁRIO").collect();
        assert_eq!(lines.len(), 1, "{lines:?}");
        let [lo, hi] = lines[0].shape.bounds();
        assert!(lo.dist(P2(5000.0, 3000.0)) < 0.01 && hi.dist(P2(6000.0, 3000.0)) < 0.01, "{lo:?} {hi:?}");
        // Its text as TEXT: its words, height and angle, where it was.
        let texts: Vec<&crate::dxf::Entity> = f.entities.iter().filter(|e| e.layer == "TEXTO").collect();
        let [Shape::Text { at, height, angle, text, .. }] = texts.iter().map(|e| &e.shape).collect::<Vec<_>>()[..] else {
            panic!("one text: {texts:?}")
        };
        assert_eq!((text.as_str(), *height, *angle), ("Sala é", 250.0, 30.0));
        assert!(at.dist(P2(5100.0, 3200.0)) < 0.01, "{at:?}");
    }

    #[test]
    fn an_imported_line_crossing_the_view_with_both_ends_outside_is_kept() {
        use crate::export::CadShape;
        let view = [P2(0.0, 0.0), P2(1000.0, 1000.0)];
        let path = |pts: Vec<P2>| CadShape::Path { pts, closed: false, pen: 0.25, dash: None };
        assert!(path(vec![P2(-500.0, 500.0), P2(1500.0, 500.0)]).within(view), "a gridline across the view");
        assert!(CadShape::Fill { rings: vec![vec![P2(-1.0, -1.0), P2(2000.0, -1.0), P2(2000.0, 2000.0)]] }.within(view));
        assert!(!path(vec![P2(1500.0, 0.0), P2(1500.0, 900.0)]).within(view), "a line beside the view");
        assert!(!path(vec![P2(-500.0, 1500.0), P2(1500.0, 1500.0)]).within(view), "a line above the view");
    }
}
