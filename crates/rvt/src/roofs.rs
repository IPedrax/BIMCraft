//! Roofs: by footprint, a sketched boundary, flat, at a level, of a type; and by extrusion, a profile swept across.
//!
//! A footprint roof (class `ProfileRoof`) is read as a floor or a ceiling is: the loops its sketch lines close, the one roof type
//! its reference list names, the level it stands on and its underside, which is the bottom of its box. A roof that rises, whether
//! its edges set a slope, its type is thick across a slope or it is sloped some other way, comes in level at the bottom of its box,
//! and the report says that its slopes are not read: the slope blocks of the edges (rvt-rs RE-56) are not read, since no roof of the
//! samples this crate was checked against sets one.
//!
//! A roof by extrusion (class `Roof`) is its sketch's lines, which are the profile of its top: each line's data holds its two ends
//! in model space (the bounded line of RE-49), so a profile in an upright plane square to an axis is read whole, and the box says how
//! far it is swept. Its type is as thick as the type says square to each slope, cut plumb at the ends, which the box checks: a roof
//! comes in only where the box its profile and its type make is the box the record holds (to a millimetre). Nothing of this is in
//! rvt-rs (its notes list the roof by extrusion as not read); it is read from the bytes of Autodesk's sample, and the check is its
//! record box. A roof of another class (by face, over a mass) comes in as the footprint of its own box, and the report says so.

use std::collections::{BTreeMap, BTreeSet};

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Extrusion, Layer, Roof, RoofType};

use crate::curves::first_line;
use crate::data::newest;
use crate::floors::{ROOF, level_of, ring, sketch_lines, sketched};
use crate::import::{Facts, Import, kind, micron, mm};
use crate::inventory::Gathered;
use crate::records::Record;
use crate::types::unique_type;
use crate::walls::{LayeredType, layered_type};

/// `OST_Roofs`.
pub(crate) const OST_ROOFS: i64 = -2_000_035;

pub(crate) fn wanted(f: &Facts, out: &mut BTreeSet<u32>) {
    let sketches = sketch_lines(f);
    for r in f.of_category(OST_ROOFS) {
        out.insert(r.id);
        out.extend(sketches.get(&r.id).into_iter().flatten().map(|l| l.id));
    }
    out.extend(f.types.get(&OST_ROOFS).into_iter().flatten());
}

/// One foot in a micron's tolerance, and a millimetre: how close the corners of what is read must be to the record's box.
const FEET_MM: f64 = 1.0 / 304.8;

/// The most lines a roof's profile has: its chain is found by comparing them in pairs, so a hostile file's thousands are not tried.
const MAX_LINES: usize = 256;

pub(crate) fn import(cx: &mut Import, f: &Facts, g: &Gathered) {
    let sketches = sketch_lines(f);
    let mut made: BTreeMap<u32, String> = BTreeMap::new();
    for r in f.of_category(OST_ROOFS) {
        let name = |cx: &mut Import, tid: u32, typ: &LayeredType| {
            made.entry(tid)
                .or_insert_with(|| {
                    let base = typ.name.clone().unwrap_or_else(|| format!("Imported - {}mm", typ.thickness));
                    kind(
                        &mut cx.model.types.roof,
                        &base,
                        |k| (k.thickness - typ.thickness).abs() < 0.05 && k.layers == typ.layers,
                        RoofType { thickness: typ.thickness, layers: typ.layers.clone() },
                    )
                })
                .clone()
        };
        if f.tags.profile_roof != Some(r.class_tag) {
            if f.tags.roof == Some(r.class_tag) {
                if let Err(why) = extruded(cx, f, g, r, &sketches, name) {
                    cx.left_out("roof", r.id, &why);
                }
            } else {
                // A roof by face - over a mass, say - has no sketch of floor-plan lines to close and no profile to sweep. It comes
                // in as the footprint of its own box, and the note says that its face is not read.
                match boxed(cx, f, g, r, name) {
                    Ok(()) => cx.note(
                        "roof",
                        r.id,
                        "it is a roof by face (over a mass, say), whose face is not read: it comes in as the footprint of its own box",
                    ),
                    Err(why) => cx.left_out("roof", r.id, &why),
                }
            }
            continue;
        }
        match sketched(cx, f, g, r, ROOF, &sketches, name) {
            Ok(s) => {
                if s.pieces.len() > 1 {
                    cx.note("roof", r.id, "its sketch is several separate loops: one roof for each");
                }
                for p in &s.pieces {
                    let roof = Roof {
                        type_name: s.type_name.clone(),
                        level: s.level,
                        boundary: ring(&p.outer),
                        courtyards: p.holes.iter().map(|h| ring(h)).collect(),
                        offset: s.offset,
                        ..Roof::default()
                    };
                    let id = cx.model.insert(Element::Roof(roof));
                    cx.from.insert(id, format!("Roof {}", r.id));
                    cx.roofs.insert(r.id, id);
                }
            }
            Err(why) => cx.left_out(
                "roof",
                r.id,
                &why.replace("sloped or stepped roof, which is not described", "sloped or stepped roof, whose slopes are not read"),
            ),
        }
    }
}

/// A roof of `r` whose face is not read, as the footprint of its box: at the level its underside stands on, as
/// thick as its type.
fn boxed(
    cx: &mut Import,
    f: &Facts,
    g: &Gathered,
    r: &Record,
    type_named: impl FnOnce(&mut Import, u32, &LayeredType) -> String,
) -> Result<(), String> {
    let types = f.types.get(&OST_ROOFS).cloned().unwrap_or_default();
    let tid = unique_type(&r.refs, &types).unwrap_or(0);
    let tall = mm(r.extent()[2]);
    let typ = match layered_type(f, g, tid, OST_ROOFS).filter(|t| t.thickness > 0.0) {
        Some(t) => t,
        None => LayeredType { name: None, layers: vec![Layer { thickness: tall, ..Layer::default() }], thickness: tall },
    };
    let level = level_of(r, &f.elevations, r.bbox[2]).ok_or("it stands on no level")?;
    let model_level = *cx.levels.get(&level).ok_or("it stands on no level")?;
    let offset = micron(mm(r.bbox[2]) - mm(f.elevations.get(&level).copied().unwrap_or(0.0)));
    let type_name = type_named(cx, tid, &typ);
    let (x0, y0, x1, y1) = (r.bbox[0], r.bbox[1], r.bbox[3], r.bbox[4]);
    if !(x1 - x0 > 0.0 && y1 - y0 > 0.0) {
        return Err("its box has no footprint".into());
    }
    let roof = Roof { type_name, level: model_level, boundary: ring(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)]), offset, ..Roof::default() };
    let id = cx.model.insert(Element::Roof(roof));
    cx.from.insert(id, format!("Roof {}", r.id));
    cx.roofs.insert(r.id, id);
    Ok(())
}

/// A roof by extrusion `r`: the chain of its sketch's lines is the profile of its top, swept across the box's width, as thick as its
/// type is square to each slope. Whether it came in; if not, why.
fn extruded(
    cx: &mut Import,
    f: &Facts,
    g: &Gathered,
    r: &Record,
    sketches: &BTreeMap<u32, Vec<&Record>>,
    type_named: impl FnOnce(&mut Import, u32, &LayeredType) -> String,
) -> Result<(), String> {
    let lines = sketches.get(&r.id).ok_or("it has no sketch lines")?;
    if lines.len() > MAX_LINES {
        return Err(format!("its profile has {} lines", lines.len()));
    }
    let mut ends = Vec::new();
    for l in lines {
        let line = g.data.get(&l.id).and_then(|c| newest(c, first_line)).map(|(line, _)| line);
        let line = line.ok_or("its profile has a line that is not a straight one the reader can read (an arc, or no curve in its data)")?;
        ends.push((line.start(), line.end()));
    }
    let mut chain = chain(&ends).ok_or("its profile's lines do not make one open chain")?;
    // It runs one way whatever order and direction its lines are listed in: east, or north where it runs square to that.
    let (dx, dy) = (chain[chain.len() - 1][0] - chain[0][0], chain[chain.len() - 1][1] - chain[0][1]);
    if dx < -1e-6 || (dx.abs() <= 1e-6 && dy < 0.0) {
        chain.reverse();
    }
    let (first, last) = (chain[0], chain[chain.len() - 1]);
    let run = (last[0] - first[0]).hypot(last[1] - first[1]);
    if run < FEET_MM {
        return Err("its profile does not run along a plan line".into());
    }
    let u = [(last[0] - first[0]) / run, (last[1] - first[1]) / run];
    if u[0].abs().min(u[1].abs()) > 1e-9 {
        return Err("its profile's plane is not square to an axis, so its box does not say how far it is swept".into());
    }
    let n = [-u[1], u[0]];
    let along = |p: &[f64; 3]| (p[0] - first[0]) * u[0] + (p[1] - first[1]) * u[1];
    let top: Vec<(f64, f64)> = chain.iter().map(|p| (along(p), p[2])).collect();
    let straight = chain.iter().all(|p| ((p[0] - first[0]) * n[0] + (p[1] - first[1]) * n[1]).abs() < 1e-6);
    if !straight || top.windows(2).any(|w| w[1].0 - w[0].0 < FEET_MM) {
        return Err("its profile is not one upright plane running one way along its line".into());
    }
    let types = f.types.get(&OST_ROOFS).cloned().unwrap_or_default();
    let tid = unique_type(&r.refs, &types).ok_or("its record does not name one roof type")?;
    let typ = layered_type(f, g, tid, OST_ROOFS).filter(|t| t.thickness > 0.0).ok_or("its type has no layers the reader can read")?;
    let thickness = typ.thickness / 304.8;
    let under = underside(&top, thickness).ok_or("its type is thicker than its profile's stretches allow")?;
    let (z0, z1) = (under.iter().map(|p| p.1).fold(f64::INFINITY, f64::min), top.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max));
    // The box the profile and the type make is the box the record holds: along the profile, across it, and up.
    let corners = (0..8).map(|k| [r.bbox[3 * (k & 1)], r.bbox[1 + 3 * ((k >> 1) & 1)], r.bbox[2 + 3 * ((k >> 2) & 1)]]);
    let (mut a_lo, mut a_hi, mut b_lo, mut b_hi) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for c in corners {
        let (a, b) = (along(&c), (c[0] - first[0]) * n[0] + (c[1] - first[1]) * n[1]);
        (a_lo, a_hi, b_lo, b_hi) = (a_lo.min(a), a_hi.max(a), b_lo.min(b), b_hi.max(b));
    }
    let length = top[top.len() - 1].0;
    let fits =
        a_lo.abs() <= FEET_MM && (a_hi - length).abs() <= FEET_MM && (r.bbox[2] - z0).abs() <= FEET_MM && (r.bbox[5] - z1).abs() <= FEET_MM;
    if !fits || b_hi - b_lo < FEET_MM {
        return Err("its box is not the one its profile and its type's thickness square to each slope make".into());
    }
    let level = level_of(r, &f.elevations, z0).ok_or("it stands on no level")?;
    let model_level = cx.levels.get(&level).copied().ok_or("it stands on no level")?;
    let offset = micron(mm(z0) - mm(f.elevations.get(&level).copied().unwrap_or(0.0)));
    let type_name = type_named(cx, tid, &typ);
    let roof = Roof {
        type_name,
        level: model_level,
        offset,
        extrusion: Some(Extrusion {
            line: [P2(mm(first[0]), mm(first[1])), P2(mm(last[0]), mm(last[1]))],
            profile: under.iter().map(|p| P2(mm(p.0), mm(p.1 - z0))).collect(),
            from: mm(b_lo),
            to: mm(b_hi),
        }),
        ..Roof::default()
    };
    let id = cx.model.insert(Element::Roof(roof));
    cx.from.insert(id, format!("Roof {}", r.id));
    cx.roofs.insert(r.id, id);
    Ok(())
}

/// The segments, joined end to end into one open chain: its points in order, from one end. None where they do not make one (a gap,
/// a fork, a loop).
fn chain(segments: &[([f64; 3], [f64; 3])]) -> Option<Vec<[f64; 3]>> {
    let same = |a: &[f64; 3], b: &[f64; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
    let mut rest: Vec<_> = segments.iter().skip(1).copied().collect();
    let mut points = vec![segments.first()?.0, segments.first()?.1];
    while !rest.is_empty() {
        let tail = points[points.len() - 1];
        let head = points[0];
        let at = rest.iter().position(|(a, b)| same(a, &tail) || same(b, &tail) || same(a, &head) || same(b, &head))?;
        let (a, b) = rest.swap_remove(at);
        if same(&a, &tail) {
            points.push(b);
        } else if same(&b, &tail) {
            points.push(a);
        } else if same(&a, &head) {
            points.insert(0, b);
        } else {
            points.insert(0, a);
        }
    }
    let closed = same(&points[0], &points[points.len() - 1]);
    (!closed).then_some(points)
}

/// The underside of a profile `top` of (along, up) feet that is `thickness` thick square to each of its slopes and cut plumb at the
/// ends: each stretch moved down by `thickness` over the cosine of its slope, neighbours met where those lines cross. None where
/// that does not keep the points running along.
fn underside(top: &[(f64, f64)], thickness: f64) -> Option<Vec<(f64, f64)>> {
    let slopes: Vec<f64> = top.windows(2).map(|w| (w[1].1 - w[0].1) / (w[1].0 - w[0].0)).collect();
    let drops: Vec<f64> = slopes.iter().map(|m| thickness * m.hypot(1.0)).collect();
    let mut out = vec![(top[0].0, top[0].1 - drops[0])];
    for k in 1..top.len() - 1 {
        let (m0, m1, d0, d1) = (slopes[k - 1], slopes[k], drops[k - 1], drops[k]);
        let shift = if (m0 - m1).abs() < 1e-12 { 0.0 } else { (d0 - d1) / (m0 - m1) };
        out.push((top[k].0 + shift, top[k].1 + m0 * shift - d0));
    }
    out.push((top[top.len() - 1].0, top[top.len() - 1].1 - drops[drops.len() - 1]));
    out.windows(2).all(|w| w[1].0 - w[0].0 >= FEET_MM && w.iter().all(|p| p.1.is_finite())).then_some(out)
}
