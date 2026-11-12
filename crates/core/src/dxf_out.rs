//! A view as a DXF, written from the public DXF reference: what the view draws (its display list, [`render::Drawing`]), in
//! model millimetres, as an ASCII DXF of the AutoCAD 2004 format (AC1018), the oldest that holds true colours, with the
//! structure every reader since 2000 knows: the header, the tables (with a layer, a linetype and the records a reader
//! expects to find), the model and paper space blocks, the entities and the objects that name the layouts. The file's
//! skeleton follows CADCraft's writer (MIT OR Apache-2.0), cut down to what a view needs.
//!
//! Each element's lines go on the layer of its category (`wall`, `door`, `dooropening`), cut ones on `<category>-cut`,
//! patterns on `<category>-pattern` and lettering on `<category>-text`; what no element draws on `annotation`. A line
//! keeps its colour as a true colour, its weight on paper as the nearest standard line weight, and its dashes as a
//! linetype in model millimetres; a fill is a solid hatch. Lettering is drawn as the lines our own font draws, as the PDF
//! does; a picture can't be written and is left out, counted.
//!
//! An imported CAD drawing the view shows is written from its own entities, not from what the view draws of it: each on
//! its own layer, its text as TEXT, so a DWG brought in goes back out with the layers and lettering it came with.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::dxf::{Horizontal, Shape, Vertical};
use crate::geom::P2;
use crate::graphics;
use crate::graphics::Look;
use crate::model::{Element, Model};
use crate::render::{Drawing, Part, Prim, Rgb};

/// The standard line weights a DXF may give an entity, hundredths of a millimetre.
const WEIGHTS: [u16; 24] = [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211];

/// What [`write`] made: the file's text, and what it holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Written {
    pub text: String,
    /// The entities in model space.
    pub entities: usize,
    /// The layers, `0` left out, by name.
    pub layers: Vec<String>,
    /// What could not be written, by what it was, with its count.
    pub left_out: BTreeMap<String, usize>,
}

/// The standard line weight nearest `mm` on paper, hundredths of a millimetre.
pub fn lineweight(mm: f64) -> u16 {
    let h = if mm.is_finite() { (mm * 100.0).max(0.0) } else { 0.0 };
    WEIGHTS.into_iter().min_by(|a, b| (f64::from(*a) - h).abs().total_cmp(&(f64::from(*b) - h).abs())).unwrap_or(0)
}

/// The layer primitive `i` of `d` goes on: its element's category (or the subcategory it is drawn in), and what of it.
fn layer_of(m: &Model, d: &Drawing, i: usize) -> String {
    let element = d.owners.get(i).copied().flatten().and_then(|id| m.elements.get(id));
    let Some(e) = element else { return "annotation".into() };
    let category = d.subs.get(i).copied().flatten().unwrap_or_else(|| graphics::category_of(m, e));
    match d.parts.get(i).copied().unwrap_or_default() {
        Part::Projection => category.to_string(),
        Part::Cut => format!("{category}-cut"),
        // A text note is lettering only: its layer is `text`, not `text-text`.
        Part::Text if category == "text" => category.to_string(),
        Part::Text => format!("{category}-text"),
        Part::Pattern | Part::LinePattern => format!("{category}-pattern"),
    }
}

/// An ASCII DXF being written: its groups and the next handle.
struct W {
    t: String,
    next: u64,
}

impl W {
    fn g(&mut self, code: i32, v: impl std::fmt::Display) {
        let _ = write!(self.t, "{code:>3}\n{v}\n");
    }
    fn f(&mut self, code: i32, v: f64) {
        // Short and exact enough: a hundred-thousandth of a millimetre.
        let v = if v.is_finite() { (v * 1e5).round() / 1e5 } else { 0.0 };
        self.g(code, if v == 0.0 { 0.0 } else { v });
    }
    fn p(&mut self, code: i32, p: P2) {
        self.f(code, p.0);
        self.f(code + 10, p.1);
    }
    fn p3(&mut self, code: i32, p: P2) {
        self.p(code, p);
        self.f(code + 20, 0.0);
    }
    /// A new handle.
    fn h(&mut self) -> String {
        let h = self.next;
        self.next += 1;
        format!("{h:X}")
    }
    fn table(&mut self, name: &str, count: usize) -> String {
        let h = self.h();
        self.g(0, "TABLE");
        self.g(2, name);
        self.g(5, &h);
        self.g(330, 0);
        self.g(100, "AcDbSymbolTable");
        self.g(70, count);
        h
    }
    fn record(&mut self, kind: &str, owner: &str, subclass: &str) -> String {
        let h = self.h();
        self.g(0, kind);
        self.g(5, &h);
        self.g(330, owner);
        self.g(100, "AcDbSymbolTableRecord");
        self.g(100, subclass);
        h
    }
    fn end_table(&mut self) {
        self.g(0, "ENDTAB");
    }
    fn rgb(&mut self, c: Rgb) {
        self.g(420, (u32::from(c.0) << 16) | (u32::from(c.1) << 8) | u32::from(c.2));
    }
}

/// A primitive as written: its layer, colour, weight, linetype and points in model millimetres.
enum Out {
    Line {
        layer: String,
        color: Rgb,
        weight: u16,
        linetype: Option<String>,
        pts: Vec<P2>,
        closed: bool,
    },
    Fill {
        layer: String,
        color: Rgb,
        rings: Vec<Vec<P2>>,
    },
    /// One line of an imported drawing's lettering: `height` mm, `angle` degrees, `h` and `v` its DXF justification.
    Text {
        layer: String,
        color: Rgb,
        at: P2,
        height: f64,
        angle: f64,
        text: String,
        h: u8,
        v: u8,
    },
}

/// `pts` without a point that repeats the one before it; none when fewer than `least` are left, a line or a ring of no
/// length or area, which a reader takes as an empty entity.
fn distinct(pts: Vec<P2>, least: usize) -> Option<Vec<P2>> {
    let mut out: Vec<P2> = Vec::with_capacity(pts.len());
    for p in pts {
        if out.last().is_none_or(|q| (q.0 - p.0).abs() > 1e-6 || (q.1 - p.1).abs() > 1e-6) {
            out.push(p);
        }
    }
    while out.len() > 1 && out.first().zip(out.last()).is_some_and(|(a, b)| (a.0 - b.0).abs() <= 1e-6 && (a.1 - b.1).abs() <= 1e-6) {
        out.pop();
    }
    (out.len() >= least).then_some(out)
}

/// A layer name a DXF takes: the drawing's own, unless it is empty or holds a character a layer's name can't.
fn layer_name(name: &str) -> String {
    let clean: String = name.chars().filter(|c| !c.is_control() && !"<>/\\\":;?*|=`".contains(*c)).take(255).collect();
    if clean.trim().is_empty() { "cadimport".into() } else { clean }
}

/// `text` as a DXF string of a 2004 file: what isn't ASCII as `\U+XXXX`, controls dropped.
fn dxf_text(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars().filter(|c| !c.is_control()).take(1000) {
        match u32::from(c) {
            0x20..=0x7e => out.push(c),
            n if n <= 0xffff => {
                let _ = write!(out, "\\U+{n:04X}");
            }
            _ => out.push('?'),
        }
    }
    out
}

/// The imported drawings `d` shows, with the layers of each its view hides, written from their own entities.
fn cad_entities(m: &Model, d: &Drawing, look: &Look, dashes: &mut Vec<Vec<f64>>) -> Vec<Out> {
    let per_paper_mm = d.paper_px().max(f32::EPSILON);
    let mut shown: Vec<crate::ids::Id> =
        d.owners.iter().flatten().copied().filter(|id| matches!(m.elements.get(id), Some(Element::CadImport(_)))).collect();
    shown.sort();
    shown.dedup();
    let mut out = Vec::new();
    for id in shown {
        let Some(Element::CadImport(ci)) = m.elements.get(id) else { continue };
        let Some(cad) = m.cads.get(&ci.cad) else { continue };
        let place = crate::model::cad_place(ci);
        let in_view = look.hidden_layers(&ci.cad);
        for ent in &cad.dxf().entities {
            if !cad.shows(ent, &ci.hidden) || in_view.is_some_and(|l| l.contains(&ent.layer)) {
                continue;
            }
            let layer = layer_name(&ent.layer);
            let color = cad.color(ent, ci.colors).unwrap_or(Rgb(0, 0, 0));
            match &ent.shape {
                Shape::Text { at, height, angle, text, align, .. } => {
                    let height = height * ci.scale;
                    if !(height.is_finite() && height > 0.0) || text.trim().is_empty() {
                        continue;
                    }
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
                    out.push(Out::Text { layer, color, at: place(*at), height, angle: angle + ci.angle, text: dxf_text(text), h, v });
                }
                Shape::Point(_) => {}
                Shape::Fill { rings, solid } => {
                    let rings: Vec<Vec<P2>> = rings.iter().filter_map(|r| distinct(r.iter().map(|p| place(*p)).collect(), 3)).collect();
                    if rings.is_empty() {
                        continue;
                    }
                    // A patterned hatch is drawn as the view draws it: filled pale.
                    let pale = |v: u8| v + (255 - v) / 4 * 3;
                    let color = if *solid { color } else { Rgb(pale(color.0), pale(color.1), pale(color.2)) };
                    out.push(Out::Fill { layer, color, rings });
                }
                shape => {
                    let linetype =
                        cad.dash(ent).map(|[a, b]| [a * ci.scale, b * ci.scale]).filter(|[a, b]| *a > 0.0 && *b > 0.0).map(|[a, b]| {
                            let pattern = vec![(a * 100.0).round() / 100.0, (b * 100.0).round() / 100.0];
                            let n = dashes.iter().position(|d| *d == pattern).unwrap_or_else(|| {
                                dashes.push(pattern);
                                dashes.len() - 1
                            });
                            format!("BIM_DASH_{}", n + 1)
                        });
                    let weight = lineweight(f64::from(d.width(cad.pen(ent)) / per_paper_mm));
                    for (pts, closed) in crate::cad::paths(shape) {
                        let Some(pts) = distinct(pts.iter().map(|p| place(*p)).collect(), 2) else { continue };
                        out.push(Out::Line { layer: layer.clone(), color, weight, linetype: linetype.clone(), pts, closed });
                    }
                }
            }
        }
    }
    out
}

/// `d`, a view of `m`, as a DXF in model millimetres.
pub fn write(m: &Model, d: &Drawing, look: &Look) -> Written {
    let mm = |p: &(f32, f32)| d.view.mm(*p);
    let per_paper_mm = d.paper_px().max(f32::EPSILON);
    let scale = d.view.scale.max(f64::EPSILON);
    let mut left_out: BTreeMap<String, usize> = BTreeMap::new();
    let mut layers: BTreeMap<String, Rgb> = BTreeMap::new();
    // Dash patterns, model mm, rounded to a hundredth, by the name of their linetype.
    let mut dashes: Vec<Vec<f64>> = Vec::new();
    let mut out: Vec<Out> = Vec::new();
    for (i, prim) in d.prims.iter().enumerate() {
        // An imported drawing is written from its own entities, below.
        if d.owners.get(i).copied().flatten().is_some_and(|id| matches!(m.elements.get(id), Some(Element::CadImport(_)))) {
            continue;
        }
        let layer = layer_of(m, d, i);
        match prim {
            Prim::Line { pts, width, color, dash, closed } => {
                let Some(pts) = distinct(pts.iter().map(mm).collect(), 2) else { continue };
                let linetype = dash.as_ref().filter(|d| d.len() >= 2).map(|px| {
                    let pattern: Vec<f64> = px.iter().map(|v| (f64::from(*v) / scale * 100.0).round() / 100.0).collect();
                    let n = dashes.iter().position(|d| *d == pattern).unwrap_or_else(|| {
                        dashes.push(pattern);
                        dashes.len() - 1
                    });
                    format!("BIM_DASH_{}", n + 1)
                });
                layers.entry(layer.clone()).or_insert(*color);
                let weight = lineweight(f64::from(*width / per_paper_mm));
                out.push(Out::Line { layer, color: *color, weight, linetype, pts, closed: *closed });
            }
            Prim::Fill { rings, color } => {
                let rings: Vec<Vec<P2>> = rings.iter().filter_map(|r| distinct(r.iter().map(mm).collect(), 3)).collect();
                if rings.is_empty() {
                    continue;
                }
                layers.entry(layer.clone()).or_insert(*color);
                out.push(Out::Fill { layer, color: *color, rings });
            }
            Prim::Image { .. } => *left_out.entry("pictures".into()).or_default() += 1,
        }
    }
    for o in cad_entities(m, d, look, &mut dashes) {
        let (Out::Line { layer, color, .. } | Out::Fill { layer, color, .. } | Out::Text { layer, color, .. }) = &o;
        if layer != "0" {
            layers.entry(layer.clone()).or_insert(*color);
        }
        out.push(o);
    }
    let all = out.iter().flat_map(|o| match o {
        Out::Line { pts, .. } => pts.clone(),
        Out::Fill { rings, .. } => rings.iter().flatten().copied().collect(),
        Out::Text { at, .. } => vec![*at],
    });
    let (lo, hi) = all.fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), p| {
        (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1)))
    });
    let (lo, hi) = if lo.0 <= hi.0 { (lo, hi) } else { (P2(0.0, 0.0), P2(0.0, 0.0)) };
    let mut w = W { t: String::new(), next: 0x20 };
    // Handles the structure names before it writes them.
    let (ms, ps) = (w.h(), w.h());
    let (root, groups, layouts, model_layout, paper_layout) = (w.h(), w.h(), w.h(), w.h(), w.h());

    // HEADER
    w.g(0, "SECTION");
    w.g(2, "HEADER");
    w.g(9, "$ACADVER");
    w.g(1, "AC1018");
    w.g(9, "$DWGCODEPAGE");
    w.g(3, "ANSI_1252");
    w.g(9, "$INSBASE");
    w.p3(10, P2(0.0, 0.0));
    w.g(9, "$EXTMIN");
    w.p3(10, lo);
    w.g(9, "$EXTMAX");
    w.p3(10, hi);
    w.g(9, "$LIMMIN");
    w.p(10, lo);
    w.g(9, "$LIMMAX");
    w.p(10, hi);
    w.g(9, "$LTSCALE");
    w.f(40, 1.0);
    w.g(9, "$INSUNITS");
    w.g(70, 4);
    w.g(9, "$MEASUREMENT");
    w.g(70, 1);
    w.g(9, "$LWDISPLAY");
    w.g(290, 1);
    w.g(9, "$HANDSEED");
    // Filled in at the end, when the last handle is known.
    let seed_at = w.t.len();
    w.g(5, "FFFFFFFF");
    w.g(0, "ENDSEC");

    w.g(0, "SECTION");
    w.g(2, "CLASSES");
    w.g(0, "ENDSEC");

    // TABLES
    w.g(0, "SECTION");
    w.g(2, "TABLES");
    let t = w.table("VPORT", 1);
    w.record("VPORT", &t, "AcDbViewportTableRecord");
    w.g(2, "*Active");
    w.g(70, 0);
    w.p(10, P2(0.0, 0.0));
    w.p(11, P2(1.0, 1.0));
    w.p(12, P2((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0));
    w.f(40, ((hi.1 - lo.1).max(hi.0 - lo.0) * 1.1).max(1.0));
    w.f(41, 1.6);
    w.end_table();

    let t = w.table("LTYPE", 3 + dashes.len());
    for name in ["ByBlock", "ByLayer", "Continuous"] {
        w.record("LTYPE", &t, "AcDbLinetypeTableRecord");
        w.g(2, name);
        w.g(70, 0);
        w.g(3, if name == "Continuous" { "Solid line" } else { "" });
        w.g(72, 65);
        w.g(73, 0);
        w.f(40, 0.0);
    }
    for (n, pattern) in dashes.iter().enumerate() {
        w.record("LTYPE", &t, "AcDbLinetypeTableRecord");
        w.g(2, format!("BIM_DASH_{}", n + 1));
        w.g(70, 0);
        w.g(3, "BIMCraft dashes");
        w.g(72, 65);
        w.g(73, pattern.len());
        w.f(40, pattern.iter().sum());
        for (k, v) in pattern.iter().enumerate() {
            // On, off, on, off: a gap is negative.
            w.f(49, if k % 2 == 0 { *v } else { -*v });
            w.g(74, 0);
        }
    }
    w.end_table();

    let t = w.table("LAYER", 1 + layers.len());
    let layer_rows = std::iter::once(("0".to_string(), None)).chain(layers.iter().map(|(n, c)| (n.clone(), Some(*c))));
    for (name, color) in layer_rows {
        w.record("LAYER", &t, "AcDbLayerTableRecord");
        w.g(2, name);
        w.g(70, 0);
        w.g(62, 7);
        if let Some(c) = color {
            w.rgb(c);
        }
        w.g(6, "Continuous");
        w.g(370, -3);
        w.g(390, 0);
    }
    w.end_table();

    let t = w.table("STYLE", 1);
    w.record("STYLE", &t, "AcDbTextStyleTableRecord");
    w.g(2, "Standard");
    w.g(70, 0);
    w.f(40, 0.0);
    w.f(41, 1.0);
    w.f(50, 0.0);
    w.g(71, 0);
    w.f(42, 2.5);
    w.g(3, "txt");
    w.g(4, "");
    w.end_table();

    for name in ["VIEW", "UCS"] {
        w.table(name, 0);
        w.end_table();
    }

    let t = w.table("APPID", 1);
    w.record("APPID", &t, "AcDbRegAppTableRecord");
    w.g(2, "ACAD");
    w.g(70, 0);
    w.end_table();

    let t = w.table("DIMSTYLE", 1);
    w.g(100, "AcDbDimStyleTable");
    w.g(71, 0);
    let h = w.h();
    w.g(0, "DIMSTYLE");
    w.g(105, &h);
    w.g(330, &t);
    w.g(100, "AcDbSymbolTableRecord");
    w.g(100, "AcDbDimStyleTableRecord");
    w.g(2, "Standard");
    w.g(70, 0);
    w.end_table();

    let t = w.table("BLOCK_RECORD", 2);
    for (h, name, layout) in [(&ms, "*Model_Space", &model_layout), (&ps, "*Paper_Space", &paper_layout)] {
        w.g(0, "BLOCK_RECORD");
        w.g(5, h);
        w.g(330, &t);
        w.g(100, "AcDbSymbolTableRecord");
        w.g(100, "AcDbBlockTableRecord");
        w.g(2, name);
        w.g(340, layout);
    }
    w.end_table();
    w.g(0, "ENDSEC");

    // BLOCKS: model and paper space, empty: their entities are in ENTITIES.
    w.g(0, "SECTION");
    w.g(2, "BLOCKS");
    for (owner, name, paper) in [(&ms, "*Model_Space", false), (&ps, "*Paper_Space", true)] {
        for (kind, subclass) in [("BLOCK", "AcDbBlockBegin"), ("ENDBLK", "AcDbBlockEnd")] {
            let h = w.h();
            w.g(0, kind);
            w.g(5, h);
            w.g(330, owner);
            w.g(100, "AcDbEntity");
            if paper {
                w.g(67, 1);
            }
            w.g(8, "0");
            w.g(100, subclass);
            if kind == "BLOCK" {
                w.g(2, name);
                w.g(70, 0);
                w.p3(10, P2(0.0, 0.0));
                w.g(3, name);
                w.g(1, "");
            }
        }
    }
    w.g(0, "ENDSEC");

    // ENTITIES
    w.g(0, "SECTION");
    w.g(2, "ENTITIES");
    let entity = |w: &mut W, kind: &str, layer: &str, color: Rgb| {
        let h = w.h();
        w.g(0, kind);
        w.g(5, h);
        w.g(330, &ms);
        w.g(100, "AcDbEntity");
        w.g(8, layer);
        w.rgb(color);
    };
    for o in &out {
        match o {
            Out::Line { layer, color, weight, linetype, pts, closed } => {
                entity(&mut w, "LWPOLYLINE", layer, *color);
                if let Some(lt) = linetype {
                    w.g(6, lt);
                }
                w.g(370, weight);
                w.g(100, "AcDbPolyline");
                w.g(90, pts.len());
                w.g(70, if *closed { 1 } else { 0 });
                w.f(43, 0.0);
                for p in pts {
                    w.p(10, *p);
                }
            }
            Out::Fill { layer, color, rings } => {
                entity(&mut w, "HATCH", layer, *color);
                w.g(100, "AcDbHatch");
                w.p3(10, P2(0.0, 0.0));
                w.f(210, 0.0);
                w.f(220, 0.0);
                w.f(230, 1.0);
                w.g(2, "SOLID");
                w.g(70, 1);
                w.g(71, 0);
                w.g(91, rings.len());
                for (k, ring) in rings.iter().enumerate() {
                    // The first is the outline, the rest its holes; each a closed polyline.
                    w.g(92, if k == 0 { 3 } else { 2 });
                    w.g(72, 0);
                    w.g(73, 1);
                    w.g(93, ring.len());
                    for p in ring {
                        w.p(10, *p);
                    }
                    w.g(97, 0);
                }
                w.g(75, 0);
                w.g(76, 1);
                w.g(98, 0);
            }
            Out::Text { layer, color, at, height, angle, text, h, v } => {
                entity(&mut w, "TEXT", layer, *color);
                w.g(100, "AcDbText");
                w.p3(10, *at);
                w.f(40, *height);
                w.g(1, text);
                w.f(50, *angle);
                w.g(7, "Standard");
                if *h != 0 || *v != 0 {
                    w.g(72, h);
                    w.p3(11, *at);
                }
                w.g(100, "AcDbText");
                if *v != 0 {
                    w.g(73, v);
                }
            }
        }
    }
    w.g(0, "ENDSEC");

    // OBJECTS: the root dictionary, the groups and the layouts, Model and one sheet.
    w.g(0, "SECTION");
    w.g(2, "OBJECTS");
    w.g(0, "DICTIONARY");
    w.g(5, &root);
    w.g(330, 0);
    w.g(100, "AcDbDictionary");
    w.g(281, 1);
    w.g(3, "ACAD_GROUP");
    w.g(350, &groups);
    w.g(3, "ACAD_LAYOUT");
    w.g(350, &layouts);
    w.g(0, "DICTIONARY");
    w.g(5, &groups);
    w.g(330, &root);
    w.g(100, "AcDbDictionary");
    w.g(281, 1);
    w.g(0, "DICTIONARY");
    w.g(5, &layouts);
    w.g(330, &root);
    w.g(100, "AcDbDictionary");
    w.g(281, 1);
    w.g(3, "Model");
    w.g(350, &model_layout);
    w.g(3, "Layout1");
    w.g(350, &paper_layout);
    for (h, name, block, order) in [(&model_layout, "Model", &ms, 0), (&paper_layout, "Layout1", &ps, 1)] {
        w.g(0, "LAYOUT");
        w.g(5, h);
        w.g(330, &layouts);
        w.g(100, "AcDbPlotSettings");
        w.g(1, "");
        w.g(4, "");
        w.g(6, "");
        w.g(40, 0.0);
        w.g(41, 0.0);
        w.g(42, 0.0);
        w.g(43, 0.0);
        w.g(44, 0.0);
        w.g(45, 0.0);
        w.g(46, 0.0);
        w.g(47, 0.0);
        w.g(48, 0.0);
        w.g(49, 0.0);
        w.g(140, 0.0);
        w.g(141, 0.0);
        w.g(142, 1.0);
        w.g(143, 1.0);
        w.g(70, if order == 0 { 1712 } else { 688 });
        w.g(72, 1);
        w.g(73, 0);
        w.g(74, 5);
        w.g(7, "");
        w.g(75, 16);
        w.g(147, 1.0);
        w.g(148, 0.0);
        w.g(149, 0.0);
        w.g(100, "AcDbLayout");
        w.g(1, name);
        w.g(70, 1);
        w.g(71, order);
        w.p(10, P2(0.0, 0.0));
        w.p(11, P2(420.0, 297.0));
        w.p3(12, P2(0.0, 0.0));
        w.p3(14, lo);
        w.p3(15, hi);
        w.f(146, 0.0);
        w.p3(13, P2(0.0, 0.0));
        w.p3(16, P2(1.0, 0.0));
        w.p3(17, P2(0.0, 1.0));
        w.g(76, 0);
        w.g(330, block);
    }
    w.g(0, "ENDSEC");
    w.g(0, "EOF");

    let seed = format!("{:>3}\n{:X}\n", 5, w.next);
    let end = seed_at + format!("{:>3}\nFFFFFFFF\n", 5).len();
    w.t.replace_range(seed_at..end, &seed);
    Written { text: w.t, entities: out.len(), layers: layers.into_keys().collect(), left_out }
}
