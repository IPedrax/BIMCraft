//! Columns, architectural and structural: a section standing from a level up.
//!
//! A column's **place** is its instance's box in plan: its centre, and its width and depth, are the box's. A column turned off the
//! axes by some other angle than a right angle has a box bigger than its section, so its section is its family symbol's
//! envelope, turned by the angle of its placement (the instance's own data). Its **levels** and **height** are read as a wall's
//! are: the two levels its list names, the base the higher at or below the bottom of the box, and up to the other level only
//! where the box ends at it. Its **type** is the one family symbol its list names that has a name entry.
//!
//! A box that begins at the ground while its column starts higher is corrected from the column's geometry node. The
//! section's *shape* is not read: the file's data read here does not say whether a column is rectangular, round, an I or an
//! angle, so every column comes in as the rectangle of its box and the notes say so. A round column or a rolled steel section
//! is therefore drawn square and solid.

use bimcraft_core::model::{Column, ColumnType, Element};

use crate::data::newest;
use crate::import::{Facts, Import, kind, micron, mm, point};
use crate::inventory::{Gathered, Wants};
use crate::openings::{symbol_name, symbol_of};
use crate::placement::transform;
use crate::walls::constraints;

/// `OST_Columns`.
pub(crate) const OST_COLUMNS: i64 = -2_000_100;
/// `OST_StructuralColumns`.
pub(crate) const OST_STRUCTURAL_COLUMNS: i64 = -2_001_330;

pub(crate) fn wanted(f: &Facts, out: &mut Wants) {
    for category in [OST_COLUMNS, OST_STRUCTURAL_COLUMNS] {
        for r in f.of_category(category) {
            out.data.insert(r.id);
            out.columns.insert(r.id, r.bbox);
        }
    }
}

pub(crate) fn import(cx: &mut Import, f: &Facts, g: &Gathered) {
    for (category, structural) in [(OST_COLUMNS, false), (OST_STRUCTURAL_COLUMNS, true)] {
        let symbols = f.symbols.get(&category).cloned().unwrap_or_default();
        for r in f.of_category(category) {
            let made = (|| -> Result<(Column, bool), String> {
                // A box that begins at the ground while the column starts higher: the geometry node has the truer bottom.
                let mut r = r.clone();
                if let Some(&bottom) = g.column_bottoms.get(&r.id) {
                    r.bbox[2] = bottom;
                }
                let r = &r;
                let (base, top) = constraints(r, &f.elevations);
                let level = base.and_then(|b| cx.levels.get(&b).copied()).ok_or("it stands on no level")?;
                let elevation = base.and_then(|b| f.elevations.get(&b).copied()).unwrap_or(0.0);
                let top_level = top
                    .filter(|t| f.elevations.get(t).is_some_and(|e| (e - r.bbox[5]).abs() * 304.8 <= 1.0 && *e > r.bbox[2]))
                    .and_then(|t| cx.levels.get(&t).copied());
                // Turned by an angle that is no right angle: the symbol's envelope, which is the section before it is turned.
                let mut fell_back = false;
                let placed = g.data.get(&r.id).and_then(|c| newest(c, transform)).map(|(t, _)| t);
                let angle = placed.and_then(|t| t.plan_angle()).map(f64::to_degrees).unwrap_or(0.0);
                let quarter = (angle / 90.0 - (angle / 90.0).round()).abs() < 1e-6;
                let (width, depth, at, angle) = match placed.filter(|_| !quarter) {
                    Some(t) => {
                        // A turned column's box is the one round it, so a symbol whose size the file does not give comes in as its box.
                        match symbol_of(r, &symbols, g, category).and_then(|(id, _)| f.symbol_boxes.get(&id)) {
                            Some(sym) => (mm(sym[3] - sym[0]), mm(sym[4] - sym[1]), point(t.origin[0], t.origin[1]), micron(angle)),
                            None => {
                                fell_back = true;
                                (
                                    mm(r.bbox[3] - r.bbox[0]),
                                    mm(r.bbox[4] - r.bbox[1]),
                                    point((r.bbox[0] + r.bbox[3]) / 2.0, (r.bbox[1] + r.bbox[4]) / 2.0),
                                    micron(angle),
                                )
                            }
                        }
                    }
                    None => (
                        mm(r.bbox[3] - r.bbox[0]),
                        mm(r.bbox[4] - r.bbox[1]),
                        point((r.bbox[0] + r.bbox[3]) / 2.0, (r.bbox[1] + r.bbox[4]) / 2.0),
                        0.0,
                    ),
                };
                let name = symbol_name(r, &symbols, g, category).unwrap_or_else(|| "Imported column".to_string());
                let types = if structural { &mut cx.model.types.structural_column } else { &mut cx.model.types.column };
                let type_name = kind(
                    types,
                    &name,
                    |t| (t.width - width).abs() < 0.05 && (t.depth - depth).abs() < 0.05 && !t.round,
                    ColumnType { width, depth, round: false },
                );
                Ok((
                    Column {
                        type_name,
                        level,
                        at,
                        angle,
                        top_level,
                        height: mm(r.bbox[5] - r.bbox[2]),
                        base_offset: micron(mm(r.bbox[2]) - mm(elevation)),
                        params: Default::default(),
                    },
                    fell_back,
                ))
            })();
            match made {
                Ok((column, fell_back)) => {
                    let (element, label) = if structural {
                        (Element::StructuralColumn(column), "Structural column")
                    } else {
                        (Element::Column(column), "Column")
                    };
                    let id = cx.model.insert(element);
                    cx.from.insert(id, format!("{label} {}", r.id));
                    cx.note("column", r.id, "comes in as the rectangle of its box: the shape of its section is not read");
                    if fell_back {
                        cx.note("column", r.id, "it is turned, and its family symbol's size is not known: it comes in as its box, turned");
                    }
                }
                Err(why) => cx.left_out("column", r.id, &why),
            }
        }
    }
}
