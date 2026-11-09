//! Structural framing and structural foundations, from their records' boxes.
//!
//! The file's record says where an element is and the box that holds it; a beam's line, slope, rotation and section, and a
//! foundation's shape, are not read here (that would take a finding in the element's data). So:
//!
//! - A **framing member** whose box runs along an axis (across it at most a third of its length, and less deep than it is long)
//!   is a beam along the box's longer horizontal side, through the middle of the box across it, its top the box's top on the
//!   level at or below it, its section the box across it and as deep as the box. A member whose box doesn't (one that runs
//!   across the axes in plan, whose box is the envelope of a diagonal; a brace or a post that stands) is left out, with why:
//!   its box would be a beam of another width and direction.
//! - A **foundation** is an isolated footing of its box: its middle in plan, its top the box's top on the level at or below it,
//!   as thick as the box, its type the box's width and depth. A wall foundation or a mat is its box too.
//!
//! The notes say each came in from its box. Nothing new is read from the file: the record's category and box, and the levels.

use bimcraft_core::model::{Column, ColumnType, Element, Justify, Run, RunType};

use crate::import::{Facts, Import, kind, level_at, micron, mm, point};

/// `OST_StructuralFraming`.
pub(crate) const OST_STRUCTURAL_FRAMING: i64 = -2_001_320;
/// `OST_StructuralFoundation`.
pub(crate) const OST_STRUCTURAL_FOUNDATION: i64 = -2_001_300;

/// A size in millimetres as a type's name shows it: to the tenth, without trailing zeros.
fn size(v: f64) -> String {
    format!("{}", (v * 10.0).round() / 10.0 + 0.0)
}

/// The run a box gives along an axis: its two ends' middles and how wide across it is, in feet; none where its box runs across
/// the axes (wider than a third of its length) or stands (as deep as it is long).
pub(crate) fn along(b: &[f64; 6]) -> Option<([f64; 2], [f64; 2], f64)> {
    let (dx, dy, dz) = (b[3] - b[0], b[4] - b[1], b[5] - b[2]);
    let (length, across) = (dx.max(dy), dx.min(dy));
    if across > length / 3.0 || dz >= length {
        return None;
    }
    let (mx, my) = ((b[0] + b[3]) / 2.0, (b[1] + b[4]) / 2.0);
    Some(if dx >= dy { ([b[0], my], [b[3], my], across) } else { ([mx, b[1]], [mx, b[4]], across) })
}

pub(crate) fn import(cx: &mut Import, f: &Facts) {
    for r in f.of_category(OST_STRUCTURAL_FRAMING) {
        let label = "structural framing member";
        let Some((a, b, across)) = along(&r.bbox) else {
            cx.left_out(
                label,
                r.id,
                "its box is no beam's: it runs across the axes in plan or stands, and its line is not read, so a beam of its box would \
                 have another width and direction",
            );
            continue;
        };
        let (level, elevation) = match level_at(cx, f, r.bbox[5]) {
            Ok(l) => l,
            Err(why) => {
                cx.left_out(label, r.id, &why);
                continue;
            }
        };
        let (width, depth) = (mm(across), mm(r.bbox[5] - r.bbox[2]));
        let types = cx.model.types.runs.entry("beam".to_string()).or_default();
        let new = RunType { width, depth, round: false, material: String::new(), profile: None };
        let same = |t: &RunType| !t.round && (t.width - width).abs() < 0.05 && (t.depth - depth).abs() < 0.05 && t.material.is_empty();
        let type_name = kind(types, &format!("Imported {} x {}", size(width), size(depth)), same, new);
        let run = Run {
            type_name,
            level,
            start: point(a[0], a[1]),
            end: point(b[0], b[1]),
            offset: micron(mm(r.bbox[5]) - mm(elevation)),
            end_offset: None,
            justify: Justify::Top,
            system: String::new(),
            params: Default::default(),
        };
        let Some(beam) = Element::of_run("beam", run) else { continue };
        let id = cx.model.insert(beam);
        cx.from.insert(id, format!("Structural framing member {}", r.id));
        cx.note(
            label,
            r.id,
            "comes in as a beam along its box's longer side, its section the box across it: its line, slope, section and type are not read",
        );
    }
    for r in f.of_category(OST_STRUCTURAL_FOUNDATION) {
        let label = "structural foundation";
        let (level, elevation) = match level_at(cx, f, r.bbox[5]) {
            Ok(l) => l,
            Err(why) => {
                cx.left_out(label, r.id, &why);
                continue;
            }
        };
        let (width, depth) = (mm(r.bbox[3] - r.bbox[0]), mm(r.bbox[4] - r.bbox[1]));
        let same = |t: &ColumnType| !t.round && (t.width - width).abs() < 0.05 && (t.depth - depth).abs() < 0.05;
        let new = ColumnType { width, depth, round: false, profile: None };
        let type_name = kind(&mut cx.model.types.footing, &format!("Imported {} x {}", size(width), size(depth)), same, new);
        let footing = Column {
            room_bounding: true,
            type_name,
            level,
            at: point((r.bbox[0] + r.bbox[3]) / 2.0, (r.bbox[1] + r.bbox[4]) / 2.0),
            angle: 0.0,
            top_level: None,
            height: mm(r.bbox[5] - r.bbox[2]),
            base_offset: micron(mm(r.bbox[2]) - mm(elevation)),
            params: Default::default(),
        };
        let id = cx.model.insert(Element::Footing(footing));
        cx.from.insert(id, format!("Structural foundation {}", r.id));
        cx.note(label, r.id, "comes in as an isolated footing of its box: its shape and type are not read");
    }
}
