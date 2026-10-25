// Portions of this file are ported from rvt-rs (https://github.com/DrunkOnJava/rvt-rs), Apache-2.0,
// Copyright 2026 Griffin Long, at commit b6086d41b9758fd9b92f42f0a47b8345b9c58056: src/element_record_level_refs.rs (the
// levels a record's reference list names, RE-27, RE-59), src/element_record_wall_joins.rs (what the record box of a wall
// is, RE-26, RE-29) and src/partition_compound_structure.rs (a wall's location line and which side is its exterior, RE-54).
// Changed: rewritten for BIMCraft, which builds a parametric wall from them instead of a solid.
//! Walls: straight and curved, with their type's layers.
//!
//! A wall's **location line** is the first curve record in its data (see [`crate::curves`]): its centreline whatever its
//! location-line setting, so BIMCraft's wall has `location: centre` and the line's ends. Its **record box** is the untrimmed
//! wall (its line to its raw ends, inflated by half its thickness): joins cut it back in Revit's own export, and BIMCraft cuts
//! walls where they meet by itself, so the ends are the line's. Its **height** is the box's, and its **levels** are the two its
//! reference list names (the base is the higher of the two at or below the box's bottom); the wall is *up to* the other one
//! only where the box's top is at that level, else it is as tall as its box. Its **type** is the one type of the wall category
//! the reference list names; the type's layers are its compound structure, **exterior first**, which is BIMCraft's order for a
//! wall drawn so that its exterior is to the left. A wall whose exterior is to the right of its line is turned round.

use std::collections::{BTreeMap, BTreeSet};

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Function, Id, Layer, Location, Wall, WallType};

use crate::curves::{BoundedArc, BoundedLine, first_arc, first_line};
use crate::data::{Copy, newest};
use crate::import::{Facts, Import, kind, micron, mm, point};
use crate::inventory::Gathered;
use crate::records::Record;
use crate::types::{CompoundLayer, find_layers, first_name, layer_layout, unique_type, wall_orientation};

/// `OST_Walls`.
pub(crate) const OST_WALLS: i64 = -2_000_011;

/// The ids whose data walls need: the walls' and their types'.
pub(crate) fn wanted(f: &Facts, out: &mut BTreeSet<u32>) {
    out.extend(f.of_category(OST_WALLS).map(|r| r.id));
    out.extend(f.types.get(&OST_WALLS).into_iter().flatten());
}

/// A wall's location line or arc.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Curve {
    Line(BoundedLine),
    Arc(BoundedArc),
}

fn curve(data: &[u8]) -> Option<Curve> {
    first_line(data).map(Curve::Line).or_else(|| first_arc(data).map(Curve::Arc))
}

/// The ends of a curve in plan, and how far it turns (degrees, counter-clockwise positive), or why it cannot be a wall's.
fn plan(c: Curve) -> Result<(P2, P2, f64), &'static str> {
    match c {
        Curve::Line(l) => {
            let (s, e) = (l.start(), l.end());
            if (s[2] - e[2]).abs() > 1e-6 {
                return Err("its location line is not level");
            }
            Ok((point(s[0], s[1]), point(e[0], e[1]), 0.0))
        }
        Curve::Arc(a) => {
            if a.x_axis[2].abs() > 1e-9 || a.y_axis[2].abs() > 1e-9 {
                return Err("its location arc is not in plan");
            }
            let (s, e) = (a.point(a.start_angle), a.point(a.end_angle));
            let sweep = (a.end_angle - a.start_angle).to_degrees();
            let arc = if a.counter_clockwise() { sweep } else { -sweep };
            Ok((point(s[0], s[1]), point(e[0], e[1]), (arc * 1e6).round() / 1e6))
        }
    }
}

/// The distinct project levels a record's reference list names, in list order.
pub(crate) fn named_levels(refs: &[u64], elevations: &BTreeMap<u32, f64>) -> Vec<u32> {
    let mut named = Vec::new();
    for id in refs.iter().filter_map(|&v| u32::try_from(v).ok()) {
        if elevations.contains_key(&id) && !named.contains(&id) {
            named.push(id);
        }
    }
    named
}

/// The highest level whose elevation is at or below `z` feet (within 0.001 ft), the lowest when there is none, and none
/// where two levels tie.
pub(crate) fn level_at_or_below(elevations: &BTreeMap<u32, f64>, z: f64) -> Option<u32> {
    let mut best: Option<(u32, f64)> = None;
    let mut tied = false;
    for (&id, &at) in elevations.iter().filter(|(_, at)| **at <= z + 1e-3) {
        match best {
            Some((_, seen)) if at < seen => {}
            Some((_, seen)) if at == seen => tied = true,
            _ => {
                best = Some((id, at));
                tied = false;
            }
        }
    }
    match best {
        Some((id, _)) if !tied => Some(id),
        Some(_) => None,
        None => elevations.iter().min_by(|a, b| a.1.total_cmp(b.1)).map(|(&id, _)| id),
    }
}

/// The base and the top constraint of an element whose box is from `bottom` to `top` feet: of two named levels, the base is the
/// higher one at or below the bottom (else the lower), the top the other. One named level is the base; none, the level at or
/// below the bottom.
pub(crate) fn constraints(r: &Record, elevations: &BTreeMap<u32, f64>) -> (Option<u32>, Option<u32>) {
    let named = named_levels(&r.refs, elevations);
    let (bottom, _) = (r.bbox[2], r.bbox[5]);
    match named.as_slice() {
        [a, b] => {
            let (ea, eb) = (elevations[a], elevations[b]);
            if ea == eb {
                return (level_at_or_below(elevations, bottom), None);
            }
            let (low, high, high_at) = if ea < eb { (*a, *b, eb) } else { (*b, *a, ea) };
            if high_at <= bottom + 1e-3 { (Some(high), Some(low)) } else { (Some(low), Some(high)) }
        }
        [a] => (Some(*a), None),
        _ => (level_at_or_below(elevations, bottom), None),
    }
}

/// A Revit layer function as BIMCraft's.
pub(crate) fn function(revit: u32) -> Function {
    match revit {
        1 | 200 => Function::Structure,
        3 => Function::Thermal,
        4 | 5 => Function::Finish,
        100 => Function::Membrane,
        _ => Function::Substrate,
    }
}

/// A type read from the file: its name, its layers (width in millimetres, function, material name) and so its thickness.
pub(crate) struct LayeredType {
    pub name: Option<String>,
    pub layers: Vec<Layer>,
    pub thickness: f64,
}

/// The type `id` of a category with layers: name, layers and thickness, from the newest copy of its data. None where its data
/// holds no layers.
pub(crate) fn layered_type(f: &Facts, g: &Gathered, id: u32, category: i64) -> Option<LayeredType> {
    let copies: &[Copy] = g.data.get(&id)?;
    let layout = layer_layout(f.release)?;
    let tag = f.tags.vertical_regions?.to_le_bytes();
    let (layers, _) = newest(copies, |b| find_layers(b, tag, layout, &f.materials, &f.ids))?;
    let header = f.tags.data_header();
    let name = newest(copies, |b| first_name(b, &header, f.release)).map(|(n, _)| n);
    Some(layered(name, &layers, g, category))
}

/// BIMCraft's layers for a type's, and the thickness they add up to.
pub(crate) fn layered(name: Option<String>, layers: &[CompoundLayer], g: &Gathered, category: i64) -> LayeredType {
    let layers: Vec<Layer> = layers
        .iter()
        // A structural deck may take its thickness from the layer above it and write none of its own; a layer of no thickness
        // that is not a membrane is not a layer BIMCraft's model can hold, and it adds nothing to the type.
        .filter(|l| l.width_feet > 0.0 || l.function == 100)
        .map(|l| Layer {
            function: function(l.function),
            // A layer with no material of its own takes its category's, where the document's object styles give one.
            material: l
                .material
                .or_else(|| g.category.get(&category).copied())
                .and_then(|m| g.materials.get(&m))
                .cloned()
                .unwrap_or_default(),
            thickness: mm(l.width_feet),
            ..Layer::default()
        })
        .collect();
    let thickness = layers.iter().map(|l| l.thickness).sum::<f64>();
    LayeredType { name, layers, thickness: micron(thickness) }
}

/// Why a wall's type gives no layers, naming the type where its data does: curtain walls, storefronts and the like have none.
fn no_layers(f: &Facts, g: &Gathered, tid: u32) -> String {
    let header = f.tags.data_header();
    match g.data.get(&tid).and_then(|copies| newest(copies, |b| first_name(b, &header, f.release))) {
        Some((name, _)) => format!("its type '{name}' has no layers the reader can read"),
        None => "its type has no layers the reader can read".to_string(),
    }
}

pub(crate) fn import(cx: &mut Import, f: &Facts, g: &Gathered) {
    let types = f.types.get(&OST_WALLS).cloned().unwrap_or_default();
    let mut read: BTreeMap<u32, Option<LayeredType>> = BTreeMap::new();
    let mut made: BTreeMap<u32, String> = BTreeMap::new();
    for r in f.of_category(OST_WALLS) {
        let who = |why: &str| (r.id, why.to_string());
        let wall = (|| -> Result<(Wall, String), (u32, String)> {
            let copies = g.data.get(&r.id).map(Vec::as_slice).unwrap_or_default();
            let (curve, _) = newest(copies, curve).ok_or_else(|| who("no location line the reader can read"))?;
            let (start, end, arc) = plan(curve).map_err(who)?;
            let flip = newest(copies, |b| wall_orientation(b, f.release)).map(|(o, _)| o.flip);
            let tid = unique_type(&r.refs, &types).ok_or_else(|| who("its record does not name one wall type"))?;
            let typ = read.entry(tid).or_insert_with(|| layered_type(f, g, tid, OST_WALLS));
            let typ = typ.as_ref().filter(|t| t.thickness > 0.0).ok_or_else(|| who(&no_layers(f, g, tid)))?;
            let (base, top) = constraints(r, &f.elevations);
            let level = base.and_then(|b| cx.levels.get(&b).copied()).ok_or_else(|| who("it stands on no level"))?;
            let elevation = base.and_then(|b| f.elevations.get(&b).copied()).unwrap_or(0.0);
            let top_level = top
                .filter(|t| f.elevations.get(t).is_some_and(|e| (e - r.bbox[5]).abs() * 304.8 <= 1.0 && *e > r.bbox[2]))
                .and_then(|t| cx.levels.get(&t).copied());
            let type_name = match made.get(&tid) {
                Some(n) => n.clone(),
                None => {
                    let base = typ.name.clone().unwrap_or_else(|| format!("Imported - {}mm", typ.thickness));
                    let n = kind(
                        &mut cx.model.types.wall,
                        &base,
                        |k| (k.thickness - typ.thickness).abs() < 0.05 && k.layers == typ.layers,
                        WallType { thickness: typ.thickness, layers: typ.layers.clone() },
                    );
                    made.insert(tid, n.clone());
                    n
                }
            };
            // The exterior is to the left of a BIMCraft wall; to the right of a Revit wall whose flip is set.
            let (start, end, arc) = if flip == Some(true) { (end, start, -arc) } else { (start, end, arc) };
            let wall = Wall {
                params: Default::default(),
                type_name,
                level,
                start,
                end,
                top_level,
                height: mm(r.bbox[5] - r.bbox[2]),
                base_offset: micron(mm(r.bbox[2]) - mm(elevation)),
                arc,
                location: Location::Centre,
                attach_top: None,
                attach_base: None,
            };
            let note = if flip.is_none() { "side".to_string() } else { String::new() };
            Ok((wall, note))
        })();
        match wall {
            Ok((wall, note)) => {
                let id: Id = cx.model.insert(Element::Wall(wall));
                cx.from.insert(id, format!("Wall {}", r.id));
                cx.walls.insert(r.id, id);
                if !note.is_empty() {
                    cx.note("wall", r.id, "the data does not say which side is the exterior: kept as drawn");
                }
            }
            Err((id, why)) => {
                // A wall whose type has no compound layers is a curtain wall (or the like): BIMCraft has no element for it, and a
                // door set in it is set in no wall.
                if why.contains("no layers") {
                    cx.curtain.insert(id);
                }
                cx.left_out("wall", id, &why)
            }
        }
    }
}
