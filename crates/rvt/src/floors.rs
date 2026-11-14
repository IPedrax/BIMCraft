//! Floors and ceilings: a sketched boundary with openings, at a level, of a type.
//!
//! A floor's **boundary** is its sketch (see [`crate::sketch`]): the sketch lines that name it as their owner, closed into one
//! outer loop and the voids in it. Its **type** is the one floor type its reference list names, its layers from the top down
//! and its thickness the layers' sum. Its **level** is the one level the list names, else the highest level at or below the
//! top of its box, and its **offset** is how far the top of its box is above that level. A floor is flat only where its box is
//! as tall as its type is thick. A floor edited with Revit's shape tools holds its top's vertices in its data
//! ([`shape_vertices`]): it comes in shaped by them, its top at the height most of them have. A taller box without them is a
//! floor sloped another way (by an arrow), whose slope is not read, so it comes in level at the bottom of its box, and the report
//! says so. A sketch of several separate loops makes several floors. A sketch whose ends do not meet, or whose loops cross, is
//! read again without the lines that cannot bound it (past its box, or drawn twice), and still makes a floor — its ends joined
//! by a straight line, or else the footprint of its own box
//! — and the report says which. A type with no layers has no thickness of its own: the box's height is taken for it. A ceiling is
//! read the same way, its level and its offset those of the bottom of its box, its underside.

use std::collections::{BTreeMap, BTreeSet};

use bimcraft_core::geom;
use bimcraft_core::geom::P2;
use bimcraft_core::model::{Ceiling, CeilingType, Element, Floor, FloorType, Layer, MAX_SHAPE_POINTS, Shape, ShapePoint};

use crate::data::newest;
use crate::import::{Facts, Import, kind, micron, mm, point};
use crate::inventory::Gathered;
use crate::records::Record;
use crate::records::f64_at;
use crate::sketch::{Edge, Piece, edge_of, from_boxes, from_edges, from_edges_closed, from_edges_open, from_edges_snapped, own_edges};
use crate::types::unique_type;
use crate::walls::{LayeredType, layered_type, level_at_or_below, named_levels};

/// `OST_Floors`.
pub(crate) const OST_FLOORS: i64 = -2_000_032;
/// `OST_Ceilings`.
pub(crate) const OST_CEILINGS: i64 = -2_000_038;
/// `OST_SketchLines`.
pub(crate) const OST_SKETCH_LINES: i64 = -2_000_045;

/// The sketch lines of each sketched element: the records that name it in their second reference list. A file puts that id first,
/// in the middle or last, and may name what else the line meets after it, so the owner is the one id there that is a floor, a
/// ceiling or a roof.
pub(crate) fn sketch_lines(f: &Facts) -> BTreeMap<u32, Vec<&Record>> {
    let sketched: BTreeSet<u32> =
        [OST_FLOORS, OST_CEILINGS, crate::roofs::OST_ROOFS].into_iter().flat_map(|c| f.of_category(c).map(|r| r.id)).collect();
    let mut out: BTreeMap<u32, Vec<&Record>> = BTreeMap::new();
    for r in f.of_category(OST_SKETCH_LINES) {
        let owner = r.refs2.iter().rev().find_map(|&v| u32::try_from(v).ok().filter(|id| sketched.contains(id)));
        if let Some(owner) = owner {
            out.entry(owner).or_default().push(r);
        }
    }
    out
}

/// The ids whose data floors need: the floors', their types' and their sketch lines'.
pub(crate) fn wanted(f: &Facts, out: &mut BTreeSet<u32>) {
    let sketches = sketch_lines(f);
    for r in f.of_category(OST_FLOORS) {
        out.insert(r.id);
        out.extend(sketches.get(&r.id).into_iter().flatten().map(|l| l.id));
    }
    out.extend(f.types.get(&OST_FLOORS).into_iter().flatten());
    for r in f.of_category(OST_CEILINGS) {
        out.insert(r.id);
        out.extend(sketches.get(&r.id).into_iter().flatten().map(|l| l.id));
    }
    out.extend(f.types.get(&OST_CEILINGS).into_iter().flatten());
}

/// What a sketched element's lines close.
pub(crate) struct Regions {
    pub pieces: Vec<Piece>,
    /// The sketch lines that close nothing and are left out: a line floating inside a floor's outline, as the samples have.
    pub dangling: Vec<u32>,
    /// Whether the sketch was left open and its ends joined by a straight line.
    pub closed: bool,
    /// Whether its lines close no boundary at all, so that it comes in as the footprint of its own box.
    pub boxed: bool,
}

/// The regions a sketched element's lines close, read from their data, and where a line's data holds no curve, from the lines
/// that do: a line the reader cannot read bounds nothing, and the loops its neighbours make still close.
pub(crate) fn regions(lines: &[&Record], g: &Gathered, bbox: &[f64; 6]) -> Option<Regions> {
    let mut edges: Vec<Edge> = Vec::with_capacity(lines.len());
    // The line each edge is read from.
    let mut read: Vec<u32> = Vec::with_capacity(lines.len());
    let mut unread: Vec<u32> = Vec::new();
    for l in lines {
        match g.data.get(&l.id).and_then(|c| newest(c, |b| edge_of(b, &l.bbox))) {
            Some((e, _)) => {
                edges.push(e);
                read.push(l.id);
            }
            None => unread.push(l.id),
        }
    }
    if let Some(pieces) = from_edges(&edges).or_else(|| from_edges_snapped(&edges)) {
        return Some(Regions { pieces, dangling: unread, closed: false, boxed: false });
    }
    // Without the lines that cannot be its edges (another sketch's, past its box, and one drawn twice) its sketch may close.
    let own = own_edges(&edges, bbox);
    if own.len() < edges.len() {
        let kept: Vec<Edge> = own.iter().filter_map(|&k| edges.get(k).cloned()).collect();
        if let Some(pieces) = from_edges(&kept).or_else(|| from_edges_snapped(&kept)) {
            let mut dangling = unread;
            dangling.extend(read.iter().enumerate().filter(|(k, _)| !own.contains(k)).map(|(_, id)| *id));
            return Some(Regions { pieces, dangling, closed: false, boxed: false });
        }
    }
    // The boxes of the lines make the boundary where their data does not: nothing of them is left out then.
    let boxes: Vec<[f64; 4]> = lines.iter().map(|l| [l.bbox[0], l.bbox[1], l.bbox[3], l.bbox[4]]).collect();
    if let Some(pieces) = from_boxes(&boxes) {
        return Some(Regions { pieces, dangling: Vec::new(), closed: false, boxed: false });
    }
    // A curve that closes nothing is not a boundary: the loops it stands in still close.
    if let Some((pieces, dropped)) = from_edges_open(&edges) {
        let mut dangling = unread;
        dangling.extend(dropped.iter().filter_map(|i| read.get(*i).copied()));
        return Some(Regions { pieces, dangling, closed: false, boxed: false });
    }
    // A sketch left open (a gap no snap closes): its ends are joined by a straight line, and the note says so.
    if let Some(pieces) = from_edges_closed(&edges) {
        return Some(Regions { pieces, dangling: unread, closed: true, boxed: false });
    }
    // No boundary at all: the footprint of the element's own box, which is what the file says of it and all that is left to read.
    let (lo, hi) = ((bbox[0], bbox[1]), (bbox[3], bbox[4]));
    if !(hi.0 > lo.0 && hi.1 > lo.1) {
        return None;
    }
    let outer = vec![(lo.0, lo.1), (hi.0, lo.1), (hi.0, hi.1), (lo.0, hi.1)];
    Some(Regions { pieces: vec![Piece { outer, holes: Vec::new() }], dangling: unread, closed: false, boxed: true })
}

/// A loop of feet as millimetres, to the micron, with the points that fall together dropped.
pub(crate) fn ring(points: &[(f64, f64)]) -> Vec<P2> {
    let mut out: Vec<P2> = Vec::with_capacity(points.len());
    for &(x, y) in points {
        let p = point(x, y);
        if out.last() != Some(&p) {
            out.push(p);
        }
    }
    if out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

/// The level an element stands on, from its reference list and the height `z` (feet) its box is measured at: the one level
/// named, the highest named at or below `z`, or the highest level at or below it.
pub(crate) fn level_of(r: &Record, elevations: &BTreeMap<u32, f64>, z: f64) -> Option<u32> {
    let named = named_levels(&r.refs, elevations);
    let named_map: BTreeMap<u32, f64> = named.iter().map(|id| (*id, elevations[id])).collect();
    match named.as_slice() {
        [one] => Some(*one),
        [] => level_at_or_below(elevations, z),
        _ => level_at_or_below(&named_map, z).or_else(|| named.iter().copied().min_by(|a, b| elevations[a].total_cmp(&elevations[b]))),
    }
}

/// What a floor and a ceiling have in common: a sketched boundary, a layered type and a level.
pub(crate) struct Sketched {
    pub pieces: Vec<Piece>,
    pub type_name: String,
    pub level: bimcraft_core::ids::Guid,
    /// How far above its level the box's top (bottom, for a ceiling or a roof) is.
    pub offset: f64,
    /// Whether its box is as tall as its type is thick. A sloped or stepped one is not: it comes in level, a stand-in for what it is.
    pub flat: bool,
    /// A shape-edited floor's vertices, feet, and its top they are over (the height most of them have): none for another.
    pub vertices: Vec<[f64; 3]>,
    pub top: f64,
}

/// The bytes after a vertex's id, and after its point.
const VERTEX: [u8; 4] = [0x04, 0x00, 0x08, 0x00];
const VERTEX_TAIL: [u8; 8] = [6, 0, 0, 0, 2, 0, 0, 0];

/// The vertices of a shape-edited floor's top (Revit's Modify Sub Elements) in its data: each eight `ff`, its own id, `04 00 08 00`,
/// the point (`f64` feet) and `06 00 00 00 02 00 00 00`, the point the height of the top there. Those inside its box `bbox`, each
/// place once, and at most one past [`MAX_SHAPE_POINTS`], so that a top of too many says so.
pub(crate) fn shape_vertices(data: &[u8], bbox: &[f64; 6]) -> Vec<[f64; 3]> {
    let mut out: Vec<[f64; 3]> = Vec::new();
    for at in memchr::memmem::find_iter(data, &VERTEX) {
        if out.len() > MAX_SHAPE_POINTS {
            break;
        }
        let framed = at.checked_sub(16).and_then(|s| data.get(s..s + 8)) == Some(&[0xff; 8][..])
            && data.get(at + 28..at + 36) == Some(&VERTEX_TAIL[..]);
        let [Some(x), Some(y), Some(z)] = [4, 12, 20].map(|o| f64_at(data, at + o).filter(|v| v.is_finite())) else { continue };
        let inside = [x, y, z].iter().enumerate().all(|(k, v)| *v >= bbox[k] - 0.01 && *v <= bbox[k + 3] + 0.01);
        if framed && inside && !out.iter().any(|q| (q[0] - x).abs() < 1e-6 && (q[1] - y).abs() < 1e-6) {
            out.push([x, y, z]);
        }
    }
    out
}

/// Piece `p`'s shape from a floor's `vertices` (feet), each `z` mm over its `top` (feet): those in or on its outline and in none of
/// its holes, a millimetre or more apart, those at 0 on a corner of its outline left out (a corner is at 0 already).
fn shape_of(vertices: &[[f64; 3]], top: f64, p: &Piece) -> Shape {
    let (outer, holes): (Vec<P2>, Vec<Vec<P2>>) = (ring(&p.outer), p.holes.iter().map(|h| ring(h)).collect());
    let near = |r: &[P2], q: P2| geom::ring_edges(r).any(|(a, b)| geom::dist_to_segment(q, a, b) <= 1.0);
    let mut points: Vec<ShapePoint> = Vec::new();
    for v in vertices {
        let (at, z) = (point(v[0], v[1]), micron(mm(v[2]) - mm(top)));
        let on = geom::contains(&outer, at) || near(&outer, at);
        let holed = holes.iter().any(|h| geom::contains(h, at) && !near(h, at));
        let corner = outer.iter().any(|c| c.dist(at) <= 1.0);
        if on && !holed && !(z == 0.0 && corner) && !points.iter().any(|q| q.at.dist(at) <= 1.0) {
            points.push(ShapePoint { at, z, ..ShapePoint::default() });
        }
    }
    Shape { points, lines: Vec::new() }
}

/// Which sketched element is read: its category, and whether its level and offset are those of the box's top (a floor) or of its
/// bottom (a ceiling's underside).
#[derive(Clone, Copy)]
pub(crate) struct Which {
    pub category: i64,
    pub at_top: bool,
}

const FLOOR: Which = Which { category: OST_FLOORS, at_top: true };
const CEILING: Which = Which { category: OST_CEILINGS, at_top: false };
/// A roof: its underside is the bottom of its box, as a ceiling's.
pub(crate) const ROOF: Which = Which { category: crate::roofs::OST_ROOFS, at_top: false };

/// Reads a floor or a ceiling `r`: its sketch, its type (made by `type_named`, once for each) and its level.
pub(crate) fn sketched(
    cx: &mut Import,
    f: &Facts,
    g: &Gathered,
    r: &Record,
    which: Which,
    sketches: &BTreeMap<u32, Vec<&Record>>,
    mut type_named: impl FnMut(&mut Import, u32, &crate::walls::LayeredType) -> String,
) -> Result<Sketched, String> {
    let Which { category, at_top } = which;
    let types = f.types.get(&category).cloned().unwrap_or_default();
    let what = match category {
        OST_FLOORS => "floor",
        OST_CEILINGS => "ceiling",
        _ => "roof",
    };
    let lines = sketches.get(&r.id).ok_or("it has no sketch lines")?;
    let Regions { pieces, dangling, closed, boxed } = regions(lines, g, &r.bbox).ok_or("its sketch lines do not close into a boundary")?;
    if !dangling.is_empty() {
        let ids: Vec<String> = dangling.iter().map(u32::to_string).collect();
        let plural = if ids.len() == 1 { "line" } else { "lines" };
        cx.note(what, r.id, &format!("{} sketch {plural} close nothing and are left out (ids {})", ids.len(), ids.join(", ")));
    }
    if boxed {
        cx.note(what, r.id, "its sketch lines close no boundary the reader can follow: it comes in as the footprint of its own box");
    }
    if closed {
        cx.note(what, r.id, "its sketch lines do not close: the ends that do not meet are joined by a straight line");
    }
    let tid = unique_type(&r.refs, &types).ok_or_else(|| format!("its record does not name one {what} type"))?;
    let tall = mm(r.extent()[2]);
    let typ = match layered_type(f, g, tid, category).filter(|t| t.thickness > 0.0) {
        Some(t) => t,
        // A type with no compound structure says nothing of its thickness; its own box does, a flat one being its thickness.
        None => {
            cx.note(what, r.id, "its type has no layers the reader can read: it comes in as thick as its own box");
            LayeredType { name: None, layers: vec![Layer { thickness: tall, ..Layer::default() }], thickness: tall }
        }
    };
    let flat = (tall - typ.thickness).abs() <= 1.0;
    // A floor of shape editing: its top is at its vertices' heights, over the one most of them have (the lower of two as common).
    let mut vertices = match category {
        OST_FLOORS => g.data.get(&r.id).and_then(|c| newest(c, |b| Some(shape_vertices(b, &r.bbox)).filter(|v| !v.is_empty()))),
        _ => None,
    }
    .map(|(v, _)| v)
    .unwrap_or_default();
    let (lo, hi) = vertices.iter().fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v[2]), hi.max(v[2])));
    if vertices.len() > MAX_SHAPE_POINTS {
        cx.note(what, r.id, &format!("its top has more than {MAX_SHAPE_POINTS} points of shape editing: they are not read"));
        vertices.clear();
    } else if mm(hi - lo) < 0.5 {
        vertices.clear();
    }
    let mut counts: BTreeMap<i64, usize> = BTreeMap::new();
    for v in &vertices {
        *counts.entry((v[2] * 1e6).round() as i64).or_default() += 1;
    }
    let top = counts.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))).map_or(r.bbox[5], |(k, _)| *k as f64 / 1e6);
    if !vertices.is_empty() {
        cx.note(
            what,
            r.id,
            &format!(
                "its top is shaped by the {} points of Revit's shape editing; BIMCraft draws its own creases between them",
                vertices.len()
            ),
        );
        if (mm(r.bbox[2]) - (mm(lo) - typ.thickness)).abs() > 1.0 {
            cx.note(what, r.id, "its underside is level in the file (a variable layer, which is not read): it comes in following its top");
        }
    }
    if !flat && vertices.is_empty() {
        // A sloped or stepped one: it comes in level, as thick as its type, at the box's bottom, and the note says so.
        cx.note(
            what,
            r.id,
            &format!(
                "its box is {tall} mm tall and its type {} mm thick: it is sloped or stepped, and its slopes are not read: it comes in level",
                typ.thickness
            ),
        );
    }
    let z = match (at_top, vertices.is_empty()) {
        (true, true) => r.bbox[5],
        (true, false) => top,
        (false, _) => r.bbox[2],
    };
    let level = level_of(r, &f.elevations, z).ok_or("it stands on no level")?;
    let model_level = cx.levels.get(&level).copied().ok_or("it stands on no level")?;
    let offset = micron(mm(z) - mm(f.elevations.get(&level).copied().unwrap_or(0.0)));
    let type_name = type_named(cx, tid, &typ);
    Ok(Sketched { pieces, type_name, level: model_level, offset, flat, vertices, top })
}

pub(crate) fn import(cx: &mut Import, f: &Facts, g: &Gathered) {
    let sketches = sketch_lines(f);
    let mut made: BTreeMap<u32, String> = BTreeMap::new();
    for r in f.of_category(OST_FLOORS) {
        let name = |cx: &mut Import, tid: u32, typ: &crate::walls::LayeredType| {
            made.entry(tid)
                .or_insert_with(|| {
                    let base = typ.name.clone().unwrap_or_else(|| format!("Imported - {}mm", typ.thickness));
                    kind(
                        &mut cx.model.types.floor,
                        &base,
                        |k| (k.thickness - typ.thickness).abs() < 0.05 && k.layers == typ.layers,
                        FloorType { thickness: typ.thickness, layers: typ.layers.clone() },
                    )
                })
                .clone()
        };
        match sketched(cx, f, g, r, FLOOR, &sketches, name) {
            Ok(s) => {
                if s.pieces.len() > 1 {
                    cx.note("floor", r.id, "its sketch is several separate loops: one floor for each");
                }
                for p in &s.pieces {
                    let floor = Floor {
                        type_name: s.type_name.clone(),
                        level: s.level,
                        boundary: ring(&p.outer),
                        openings: p.holes.iter().map(|h| ring(h)).collect(),
                        offset: s.offset,
                        slope: 0.0,
                        arrow: None,
                        structural: false,
                        params: Default::default(),
                        sketch: Vec::new(),
                        shape: shape_of(&s.vertices, s.top, p),
                        pattern_origin: Default::default(),
                        pattern_angle: 0.0,
                    };
                    let id = cx.model.insert(Element::Floor(floor));
                    cx.from.insert(id, format!("Floor {}", r.id));
                }
            }
            Err(why) => cx.left_out("floor", r.id, &why),
        }
    }
    let mut made: BTreeMap<u32, String> = BTreeMap::new();
    for r in f.of_category(OST_CEILINGS) {
        let name = |cx: &mut Import, tid: u32, typ: &crate::walls::LayeredType| {
            made.entry(tid)
                .or_insert_with(|| {
                    let base = typ.name.clone().unwrap_or_else(|| format!("Imported - {}mm", typ.thickness));
                    kind(
                        &mut cx.model.types.ceiling,
                        &base,
                        |k| (k.thickness - typ.thickness).abs() < 0.05 && k.layers == typ.layers && k.grid == 0.0,
                        CeilingType { thickness: typ.thickness, grid: 0.0, layers: typ.layers.clone() },
                    )
                })
                .clone()
        };
        match sketched(cx, f, g, r, CEILING, &sketches, name) {
            Ok(s) => {
                if s.pieces.len() > 1 {
                    cx.note("ceiling", r.id, "its sketch is several separate loops: one ceiling for each");
                }
                for p in &s.pieces {
                    let ceiling = Ceiling {
                        type_name: s.type_name.clone(),
                        level: s.level,
                        boundary: ring(&p.outer),
                        openings: p.holes.iter().map(|h| ring(h)).collect(),
                        offset: s.offset,
                        params: Default::default(),
                        sketch: Vec::new(),
                    };
                    let id = cx.model.insert(Element::Ceiling(ceiling));
                    cx.from.insert(id, format!("Ceiling {}", r.id));
                }
            }
            Err(why) => cx.left_out("ceiling", r.id, &why),
        }
    }
}
