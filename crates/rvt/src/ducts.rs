//! Ducts and cable trays: a straight run along the bounded line in their data, as wide and as deep as the data says.
//!
//! The data of a duct or a cable tray holds its location line as a bounded line (see `curves`) and, 66 and 74 bytes from the
//! data's header (48 and 56 after the id, where the bytes the reader gets begin), two `f64` (feet): its width and its height, which
//! are a round duct's diameter twice. The run's section is those
//! two, its width across and its height deep; a duct of one number twice is round. Its heights are the line's (the centre line's),
//! its level the highest at or below its start. Where the section is turned about the line (a tray rolled on its side) the data
//! says so elsewhere, which is not read. The name of the type and the material are not read, and the notes say so; its system is
//! read with the others' ([`crate::systems`]).
//!
//! Found in the data of a Revit 2018 file; no other release's was seen. An element whose data holds no such line and section (every
//! one of the 2025 samples') runs along the middle of what its graphics draw ([`crate::framing::centre_line`]), of the section it
//! draws: round where it draws a cylinder. One whose graphics draw no straight run of one section is left out and listed.

use std::collections::BTreeSet;

use bimcraft_core::model::{Element, Justify, Run, RunType};

use crate::curves::first_line;
use crate::data::newest;
use crate::import::{Facts, Import, kind, level_at, micron, mm, point};
use crate::inventory::Gathered;
use crate::records::f64_at;

/// `OST_DuctCurves`.
pub(crate) const OST_DUCTS: i64 = -2_008_000;
/// `OST_CableTray`.
pub(crate) const OST_CABLE_TRAYS: i64 = -2_008_130;

/// Revit's category, BIMCraft's run category and what one is called in the notes.
const KINDS: [(i64, &str, &str); 2] = [(OST_DUCTS, "duct", "duct"), (OST_CABLE_TRAYS, "cabletray", "cable tray")];

pub(crate) fn wanted(f: &Facts, out: &mut BTreeSet<u32>) {
    for (category, ..) in KINDS {
        out.extend(f.of_category(category).map(|r| r.id));
    }
}

/// The width and the height, in feet, in an element's data.
fn section(data: &[u8]) -> Option<(f64, f64)> {
    let ok = |v: f64| v.is_finite() && v > 1e-3 && v <= 30.0;
    let (width, height) = (f64_at(data, 48)?, f64_at(data, 56)?);
    (ok(width) && ok(height)).then_some((width, height))
}

/// The ends of the line, in feet, and the section, in an element's data.
type Read = ([f64; 3], [f64; 3], (f64, f64));

fn read(data: &[u8]) -> Option<Read> {
    let line = first_line(data)?;
    Some((line.start(), line.end(), section(data)?))
}

/// The ends of element `r`'s line, feet, its section, feet, and whether it is round and read from its graphics: from its data, else
/// from what its graphics draw; or why not.
fn line(g: &Gathered, r: &crate::records::Record) -> Result<(Read, Option<bool>), String> {
    let reason = "its line and section are not in the record";
    if let Some(read) = g.data.get(&r.id).and_then(|copies| newest(copies, read)).map(|(c, _)| c) {
        return Ok((read, None));
    }
    match crate::framing::centre_line(g, r) {
        Ok(c) => Ok(((c.start, c.end, (c.width, c.depth)), Some(c.round))),
        Err(why) if why == crate::generic::NO_GRAPHICS => Err(reason.to_string()),
        Err(why) => Err(format!("{reason}, nor is its line read from its graphics: {why}")),
    }
}

pub(crate) fn import(cx: &mut Import, f: &Facts, g: &Gathered) {
    for (category, run_category, label) in KINDS {
        for r in f.of_category(category) {
            let mut drawn = false;
            let made = (|| -> Result<Run, String> {
                let ((a, b, (w, h)), graphics) = line(g, r)?;
                drawn = graphics.is_some();
                let (level, elevation) = level_at(cx, f, a[2])?;
                let (width, depth) = (micron(mm(w)), micron(mm(h)));
                // A duct of one number twice is round, and one whose graphics draw a cylinder.
                let round = category == OST_DUCTS && graphics.unwrap_or((w - h).abs() < 1e-9);
                let name = if round {
                    format!("Round duct {width:.1} mm")
                } else if category == OST_DUCTS {
                    format!("Duct {width:.1} x {depth:.1} mm")
                } else {
                    format!("Cable tray {width:.1} x {depth:.1} mm")
                };
                let types = cx.model.types.runs.entry(run_category.to_string()).or_default();
                let type_name = kind(
                    types,
                    &name,
                    |t| (t.width - width).abs() < 0.05 && (t.depth - depth).abs() < 0.05 && t.round == round,
                    RunType { width, depth, round, material: String::new(), profile: None, clear_section: None },
                );
                let offset = micron(mm(a[2]) - mm(elevation));
                let end = micron(mm(b[2]) - mm(elevation));
                Ok(Run {
                    imported: None,
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
                    let element = Element::of_run(run_category, run);
                    if let Some(element) = element {
                        let id = cx.model.insert(element);
                        cx.from.insert(id, format!("{label} {}", r.id));
                        let note = if drawn {
                            "comes in along the line its graphics draw, of the section they draw: its type name and material are not \
                             read, and it is not turned about its line"
                        } else {
                            "its type name and material are not read, and it is not turned about its line"
                        };
                        cx.note(label, r.id, note);
                    }
                }
                Err(why) => cx.left_out(label, r.id, why.as_str()),
            }
        }
    }
}
