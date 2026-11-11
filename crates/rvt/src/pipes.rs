//! Pipes: a straight run between the origins of the two connectors in its data, as wide as its box says.
//!
//! A pipe's data holds a record for each of its connectors: the word 1, the connector's origin and one more number (four `f64`, feet),
//! the word 3, the connector's index and an unset id. The pipe's line runs from the origin of connector 0 to that of connector 1,
//! and its heights are theirs (a pipe's are the centre line's). Its **diameter** is the outside diameter its record's box gives:
//! the box of a cylinder of diameter `d` along the unit vector `u` and of length `l` is `l|u| + d sqrt(1 - u^2)` across on each
//! axis, so `d` is worked out on the axes the pipe does not lie along, and averaged. Its **level** is the highest at or below its
//! start. Its system, the name of its type and its material are not read, and the notes say so.
//!
//! Found in the data of a Revit 2018 file; no other release's was seen, so a pipe whose data does not hold the two connectors
//! is left out and listed.

use std::collections::BTreeSet;

use bimcraft_core::model::{Element, Justify, Run, RunType};

use crate::data::newest;
use crate::import::{Facts, Import, kind, level_at, micron, mm, point};
use crate::inventory::Gathered;
use crate::records::{MAX_FEET, f64_at, u32_at};

/// `OST_PipeCurves`.
pub(crate) const OST_PIPES: i64 = -2_008_044;

pub(crate) fn wanted(f: &Facts, out: &mut BTreeSet<u32>) {
    out.extend(f.of_category(OST_PIPES).map(|r| r.id));
}

/// The origins, in feet, of connectors 0 and 1 in an element's data.
fn connectors(data: &[u8]) -> Option<([f64; 3], [f64; 3])> {
    let mut found: [Option<[f64; 3]>; 2] = [None, None];
    for at in memchr::memmem::find_iter(data, &[3, 0, 0, 0]) {
        let Some(start) = at.checked_sub(36) else { continue };
        if data.get(start..start + 4) != Some(&[1, 0, 0, 0]) || data.get(at + 8..at + 12) != Some(&[0xff; 4]) {
            continue;
        }
        let Some(index) = u32_at(data, at + 4).filter(|&i| i < 2) else { continue };
        let origin: Option<Vec<f64>> =
            (0..3).map(|k| f64_at(data, start + 4 + 8 * k).filter(|v| v.is_finite() && v.abs() <= MAX_FEET)).collect();
        if let Some(o) = origin
            && found[index as usize].is_none()
        {
            found[index as usize] = Some([o[0], o[1], o[2]]);
        }
    }
    Some((found[0]?, found[1]?))
}

/// The outside diameter, in feet, of the round pipe from `a` to `b` whose box is `b`.
fn diameter(a: [f64; 3], b: [f64; 3], bbox: &[f64; 6]) -> Option<f64> {
    let v = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if length < 1e-6 {
        return None;
    }
    let across: Vec<f64> = (0..3)
        .map(|k| (k, v[k] / length))
        .filter(|(_, u)| u.abs() <= 0.8)
        .map(|(k, u)| (bbox[k + 3] - bbox[k] - length * u.abs()) / (1.0 - u * u).sqrt())
        .collect();
    let d = across.iter().sum::<f64>() / across.len() as f64;
    (d.is_finite() && d > 1e-3 && d < 20.0).then_some(d)
}

pub(crate) fn import(cx: &mut Import, f: &Facts, g: &Gathered) {
    for r in f.of_category(OST_PIPES) {
        let made = (|| -> Result<Run, String> {
            let copies = g.data.get(&r.id).ok_or("its two connectors are not in the record")?;
            let (a, b) = newest(copies, connectors).map(|(c, _)| c).ok_or("its two connectors are not in the record")?;
            let width = diameter(a, b, &r.bbox).ok_or("its diameter cannot be worked out from its box")?;
            let (level, elevation) = level_at(cx, f, a[2])?;
            let width = micron(mm(width));
            let types = cx.model.types.runs.entry("pipe".to_string()).or_default();
            let type_name = kind(
                types,
                &format!("Pipe {width:.1} mm"),
                |t| (t.width - width).abs() < 0.05 && t.round,
                RunType { width, depth: width, round: true, material: String::new(), profile: None },
            );
            let offset = micron(mm(a[2]) - mm(elevation));
            let end = micron(mm(b[2]) - mm(elevation));
            Ok(Run {
                type_name,
                level,
                start: point(a[0], a[1]),
                end: point(b[0], b[1]),
                offset,
                end_offset: (end != offset).then_some(end),
                justify: Justify::Centre,
                join: true,
                system: String::new(),
                params: Default::default(),
            })
        })();
        match made {
            Ok(run) => {
                let id = cx.model.insert(Element::Pipe(run));
                cx.from.insert(id, format!("Pipe {}", r.id));
                cx.note(
                    "pipe",
                    r.id,
                    "its diameter is the outside diameter its box gives; its system, type name and material are not read",
                );
            }
            Err(why) => cx.left_out("pipe", r.id, why.as_str()),
        }
    }
}
