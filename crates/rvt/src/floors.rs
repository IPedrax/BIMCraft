//! Floors and ceilings: a sketched boundary with openings, at a level, of a type.
//!
//! A floor's **boundary** is its sketch (see [`crate::sketch`]): the sketch lines that name it as their owner, closed into one
//! outer loop and the voids in it. Its **type** is the one floor type its reference list names, its layers from the top down
//! and its thickness the layers' sum. Its **level** is the one level the list names, else the highest level at or below the
//! top of its box, and its **offset** is how far the top of its box is above that level. A floor is flat only where its box is
//! as tall as its type is thick: a taller box is a sloped or a stepped floor, which the reader does not describe, so it is
//! left out and listed. A sketch of several separate loops makes several floors. A ceiling is read the same way, its level and
//! its offset those of the bottom of its box, its underside.

use std::collections::{BTreeMap, BTreeSet};

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Ceiling, CeilingType, Element, Floor, FloorType};

use crate::data::newest;
use crate::import::{Facts, Import, kind, micron, mm, point};
use crate::inventory::Gathered;
use crate::records::Record;
use crate::sketch::{Piece, edge_of, from_boxes, from_edges, from_edges_open, from_edges_snapped};
use crate::types::unique_type;
use crate::walls::{layered_type, level_at_or_below, named_levels};

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
}

/// The regions a sketched element's lines close, read from their data where every line has a curve and else from their boxes.
pub(crate) fn regions(lines: &[&Record], g: &Gathered) -> Option<Regions> {
    let edges: Option<Vec<_>> =
        lines.iter().map(|l| g.data.get(&l.id).and_then(|c| newest(c, |b| edge_of(b, &l.bbox))).map(|(e, _)| e)).collect();
    if let Some(pieces) = edges.as_ref().and_then(|e| from_edges(e)).or_else(|| edges.as_ref().and_then(|e| from_edges_snapped(e))) {
        return Some(Regions { pieces, dangling: Vec::new() });
    }
    let boxes: Vec<[f64; 4]> = lines.iter().map(|l| [l.bbox[0], l.bbox[1], l.bbox[3], l.bbox[4]]).collect();
    if let Some(pieces) = from_boxes(&boxes) {
        return Some(Regions { pieces, dangling: Vec::new() });
    }
    // A curve that closes nothing is not a boundary: the loops it stands in still close.
    let (pieces, dropped) = from_edges_open(edges.as_ref()?)?;
    let dangling = dropped.iter().filter_map(|i| lines.get(*i).map(|l| l.id)).collect();
    Some(Regions { pieces, dangling })
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
    let Regions { pieces, dangling } = regions(lines, g).ok_or("its sketch lines do not close into a boundary")?;
    if !dangling.is_empty() {
        let ids: Vec<String> = dangling.iter().map(u32::to_string).collect();
        let plural = if ids.len() == 1 { "line" } else { "lines" };
        cx.note(what, r.id, &format!("{} sketch {plural} close nothing and are left out (ids {})", ids.len(), ids.join(", ")));
    }
    let tid = unique_type(&r.refs, &types).ok_or_else(|| format!("its record does not name one {what} type"))?;
    let typ = layered_type(f, g, tid, category).filter(|t| t.thickness > 0.0).ok_or("its type has no layers the reader can read")?;
    let tall = mm(r.extent()[2]);
    if (tall - typ.thickness).abs() > 1.0 {
        return Err(format!(
            "its box is {tall} mm tall and its type {} mm thick: a sloped or stepped {what}, which is not described",
            typ.thickness
        ));
    }
    let z = if at_top { r.bbox[5] } else { r.bbox[2] };
    let level = level_of(r, &f.elevations, z).ok_or("it stands on no level")?;
    let model_level = cx.levels.get(&level).copied().ok_or("it stands on no level")?;
    let offset = micron(mm(z) - mm(f.elevations.get(&level).copied().unwrap_or(0.0)));
    let type_name = type_named(cx, tid, &typ);
    Ok(Sketched { pieces, type_name, level: model_level, offset })
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
                    };
                    let id = cx.model.insert(Element::Ceiling(ceiling));
                    cx.from.insert(id, format!("Ceiling {}", r.id));
                }
            }
            Err(why) => cx.left_out("ceiling", r.id, &why),
        }
    }
}
