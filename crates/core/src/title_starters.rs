//! Starter title blocks: ready-made stamps a project loads with `title_block {starter: …}`, each listing the papers it is for
//! (`TitleBlock::sizes`), so a sheet that names no title block takes the one for its paper. They are original layouts: a
//! ruled grid of cells, each a small label over a field, built from the data fields that ISO 7200, ASME Y14.1 and AIA's sheet
//! practice, and NBR 16752 ask a title block to carry. The numbers of those standards are unchecked against their texts, and
//! nothing here comes from any CAD vendor's own title blocks or templates. Each comes in a small size (A4 and A3, ANSI A and B)
//! and a large one (A2 to A0, ANSI C to E), the same grid taller and wider with bigger lettering.
//!
//! Every field a starter draws is an automatic one (`titleblock::AUTO`, filled from Manage > Project Information and the sheet)
//! except Checked by and Approved by, which are each sheet's own. Labels go through the project's language.

use bimcraft_i18n::msg;

use crate::geom::P2;
use crate::language::Language;
use crate::pen::Pen;
use crate::titleblock::{Align, Field, Scope, Stroke, Text, TitleBlock};

/// One cell of a grid: its width, the label small at its top, and the field text under it at `size` mm capitals.
struct Cell {
    w: f64,
    label: &'static str,
    text: &'static str,
    size: f64,
}

const fn c(w: f64, label: &'static str, text: &'static str, size: f64) -> Cell {
    Cell { w, label, text, size }
}

/// A row of the grid, top down: its height and its cells, left to right.
type Row = (f64, &'static [Cell]);

/// The grids, each 40 mm tall and 180 mm wide (NBR's 178), which a spec stretches to its size.
const ISO: &[Row] = &[
    (12.0, &[c(120.0, msg!("Project"), "{PROJECT}", 4.5), c(60.0, msg!("Client"), "{CLIENT}", 3.5)]),
    (12.0, &[c(120.0, msg!("Title"), "{SHEET_NAME}", 4.0), c(60.0, msg!("Organization"), "{ORGANIZATION}", 3.5)]),
    (
        16.0,
        &[
            c(30.0, msg!("Sheet"), "{SHEET_NUMBER}", 6.0),
            c(30.0, msg!("Scale"), "{SCALE}", 3.0),
            c(35.0, msg!("Date"), "{DATE}", 2.8),
            c(15.0, msg!("Rev."), "{REVISION}", 4.0),
            c(25.0, msg!("Created by"), "{AUTHOR}", 3.0),
            c(25.0, msg!("Approved by"), "{APPROVED}", 3.0),
            c(20.0, msg!("Page"), "{SHEET_INDEX}/{SHEET_COUNT}", 3.5),
        ],
    ),
];

const ASME: &[Row] = &[
    (
        12.0,
        &[
            c(100.0, msg!("Project"), "{PROJECT}", 4.5),
            c(40.0, msg!("Project no."), "{PROJECT_NUMBER}", 3.5),
            c(40.0, msg!("Client"), "{CLIENT}", 3.5),
        ],
    ),
    (12.0, &[c(140.0, msg!("Sheet title"), "{SHEET_NAME}", 4.5), c(40.0, msg!("Date"), "{DATE}", 2.8)]),
    (
        16.0,
        &[
            c(25.0, msg!("Drawn by"), "{AUTHOR}", 3.0),
            c(25.0, msg!("Checked by"), "{CHECKED}", 3.0),
            c(35.0, msg!("Scale"), "{SCALE}", 3.0),
            c(15.0, msg!("Rev."), "{REVISION}", 4.0),
            c(50.0, msg!("Sheet no."), "{SHEET_NUMBER}", 7.0),
            c(30.0, msg!("Page"), "{SHEET_INDEX}/{SHEET_COUNT}", 3.5),
        ],
    ),
];

const NBR: &[Row] = &[
    (12.0, &[c(118.0, msg!("Project"), "{PROJECT}", 4.5), c(60.0, msg!("Client"), "{CLIENT}", 3.5)]),
    (12.0, &[c(118.0, msg!("Title"), "{SHEET_NAME}", 4.0), c(60.0, msg!("Address"), "{PROJECT_ADDRESS}", 2.8)]),
    (
        16.0,
        &[
            c(30.0, msg!("Sheet"), "{SHEET_NUMBER}", 6.0),
            c(30.0, msg!("Scale"), "{SCALE}", 3.0),
            c(33.0, msg!("Date"), "{DATE}", 2.8),
            c(15.0, msg!("Rev."), "{REVISION}", 4.0),
            c(25.0, msg!("Drawn by"), "{AUTHOR}", 3.0),
            c(25.0, msg!("Checked by"), "{CHECKED}", 3.0),
            c(20.0, msg!("Page"), "{SHEET_INDEX}/{SHEET_COUNT}", 3.5),
        ],
    ),
];

/// A starter: its name, the grid it is drawn from, its size in mm, how much bigger its lettering is than the grid's, and the
/// papers it is for.
struct Spec {
    name: &'static str,
    rows: &'static [Row],
    width: f64,
    height: f64,
    k: f64,
    sizes: &'static [&'static str],
}

const ARCH_SMALL: &[&str] = &["ANSI A", "ANSI B", "ARCH A", "ARCH B"];
const ARCH_LARGE: &[&str] = &["ANSI C", "ANSI D", "ANSI E", "ARCH C", "ARCH D", "ARCH E", "ARCH E1", "ARCH E2", "ARCH E3"];

static SPECS: &[Spec] = &[
    Spec { name: "ISO 7200 A4-A3", rows: ISO, width: 180.0, height: 40.0, k: 1.0, sizes: &["A4", "A3"] },
    Spec { name: "ISO 7200 A2-A0", rows: ISO, width: 240.0, height: 60.0, k: 1.25, sizes: &["A2", "A1", "A0"] },
    Spec { name: "ASME Y14.1 A-B", rows: ASME, width: 180.0, height: 40.0, k: 1.0, sizes: ARCH_SMALL },
    Spec { name: "ASME Y14.1 C-E", rows: ASME, width: 240.0, height: 60.0, k: 1.25, sizes: ARCH_LARGE },
    Spec { name: "NBR 16752 A4-A3", rows: NBR, width: 178.0, height: 40.0, k: 1.0, sizes: &["A4", "A3"] },
    Spec { name: "NBR 16752 A2-A0", rows: NBR, width: 178.0, height: 60.0, k: 1.1, sizes: &["A2", "A1", "A0"] },
];

/// The starters' names, as `title_block` lists them.
pub fn names() -> Vec<&'static str> {
    SPECS.iter().map(|s| s.name).collect()
}

/// The starter called `name` (in any case), its labels in `language`.
pub fn make(name: &str, language: Language) -> Option<TitleBlock> {
    let spec = SPECS.iter().find(|s| s.name.eq_ignore_ascii_case(name.trim()))?;
    Some(build(spec, language))
}

/// Each sheet's own fields a starter's lettering may name, with their labels.
const OWN: [(&str, &str); 2] = [("CHECKED", msg!("Checked by")), ("APPROVED", msg!("Approved by"))];

fn build(spec: &Spec, language: Language) -> TitleBlock {
    let (w, h, k) = (spec.width, spec.height, spec.k);
    let across: f64 = spec.rows.first().map_or(w, |r| r.1.iter().map(|c| c.w).sum());
    let down: f64 = spec.rows.iter().map(|r| r.0).sum();
    let (sx, sy) = (w / across, h / down);
    let rect = |x: f64, y: f64, w: f64, h: f64| vec![P2(x, y), P2(x + w, y), P2(x + w, y + h), P2(x, y + h)];
    let text = |at: P2, text: String, size: f64, width_factor: f64| Text {
        at,
        text,
        size,
        angle: 0.0,
        align: Align::Left,
        color: None,
        width_factor,
    };
    let (mut lines, mut texts) = (Vec::new(), Vec::new());
    let mut top = h;
    for (height, cells) in spec.rows {
        let (rh, mut x) = (height * sy, 0.0);
        let y = top - rh;
        for cell in *cells {
            let cw = cell.w * sx;
            lines.push(Stroke { points: rect(x, y, cw, rh), closed: true, pen: Pen::PROJECTION, color: None });
            texts.push(text(P2(x + 1.5 * k, y + rh - 3.0 * k), language.word(cell.label).to_string(), 1.8 * k, 1.0));
            // A long name is condensed a little, as the built-in title box fits one into its cell.
            let narrow = if cell.size >= 4.0 { 0.85 } else { 1.0 };
            texts.push(text(P2(x + 1.5 * k, y + 2.2 * k), cell.text.to_string(), cell.size * k, narrow));
            x += cw;
        }
        top = y;
    }
    lines.push(Stroke { points: rect(0.0, 0.0, w, h), closed: true, pen: Pen::BORDER, color: None });
    let mut tb = TitleBlock {
        width: w,
        height: h,
        sizes: spec.sizes.iter().map(|s| (*s).to_string()).collect(),
        lines,
        texts,
        ..TitleBlock::default()
    };
    let undefined = tb.undefined();
    tb.fields = OWN
        .iter()
        .filter(|(key, _)| undefined.contains(*key))
        .map(|(key, label)| Field {
            key: (*key).to_string(),
            label: language.word(label).to_string(),
            scope: Scope::Sheet,
            default: String::new(),
        })
        .collect();
    tb
}

#[cfg(test)]
mod tests;
