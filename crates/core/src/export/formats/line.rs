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

/// A name a DXF table takes: no character it refuses.
fn table_name(s: &str) -> String {
    let n: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '$') { c } else { '_' }).collect();
    if n.is_empty() { "0".to_string() } else { n }
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
    let mut layers: BTreeMap<String, ()> = BTreeMap::from([("0".to_string(), ())]);
    let mut linetypes: BTreeMap<Vec<i64>, String> = BTreeMap::new();
    for (i, p) in d.prims.iter().enumerate() {
        if i.is_multiple_of(ASK_EVERY) && (ctx.cancelled)() {
            return Err("cancelled".to_string());
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
    entities(&mut w, d, &model_record, place, unit, &layer_of, &linetypes, ctx)?;
    let body = std::mem::take(&mut w.out);
    // The extents, for a reader that zooms to them.
    let pts: Vec<(f64, f64)> = d
        .prims
        .iter()
        .flat_map(|p| match p {
            Prim::Line { pts, .. } => pts.clone(),
            Prim::Fill { rings, .. } => rings.iter().flatten().copied().collect(),
            Prim::Image { .. } => Vec::new(),
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
#[allow(clippy::too_many_arguments)]
fn entities(
    w: &mut Writer,
    d: &crate::render::Drawing,
    owner: &str,
    place: Place,
    unit: f64,
    layer_of: &dyn Fn(usize) -> String,
    linetypes: &BTreeMap<Vec<i64>, String>,
    ctx: &Ctx,
) -> Result<usize, String> {
    w.pair(0, "SECTION");
    w.pair(2, "ENTITIES");
    let mut n = 0;
    for (i, p) in d.prims.iter().enumerate() {
        if i.is_multiple_of(ASK_EVERY) && (ctx.cancelled)() {
            return Err("cancelled".to_string());
        }
        let layer = layer_of(i);
        match p {
            Prim::Line { pts, width, color, dash, closed } => {
                let pts: Vec<(f64, f64)> = pts.iter().map(|q| place(*q)).filter(|q| q.0.is_finite() && q.1.is_finite()).collect();
                if pts.len() < 2 {
                    continue;
                }
                let lt = dash.as_ref().and_then(|dash| linetypes.get(&pattern_key(dash, unit))).map(String::as_str);
                let lw = weight(f64::from(*width) / DENSITY);
                if pts.len() == 2 && !closed {
                    w.entity("LINE", owner, &layer, lt, *color, Some(lw));
                    w.pair(100, "AcDbLine");
                    for (code, v) in [(10, pts[0].0), (20, pts[0].1), (30, 0.0), (11, pts[1].0), (21, pts[1].1), (31, 0.0)] {
                        w.pair(code, num(v));
                    }
                } else {
                    w.entity("LWPOLYLINE", owner, &layer, lt, *color, Some(lw));
                    w.pair(100, "AcDbPolyline");
                    w.pair(90, pts.len());
                    w.pair(70, if *closed { 1 } else { 0 } | 128);
                    for q in &pts {
                        w.pair(10, num(q.0));
                        w.pair(20, num(q.1));
                    }
                }
                n += 1;
            }
            Prim::Fill { rings, color } if *color != Rgb(255, 255, 255) => {
                let rings: Vec<Vec<(f64, f64)>> = rings
                    .iter()
                    .map(|r| r.iter().map(|q| place(*q)).filter(|q| q.0.is_finite() && q.1.is_finite()).collect::<Vec<_>>())
                    .filter(|r| r.len() >= 3)
                    .collect();
                if rings.is_empty() {
                    continue;
                }
                w.entity("HATCH", owner, &layer, None, *color, None);
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
                n += 1;
            }
            Prim::Fill { .. } | Prim::Image { .. } => {}
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
}
