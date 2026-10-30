//! A panel schedule on a sheet: its title, the board's header, its slots (in two columns for the branch panel's template), its load
//! summary and its notes, as ruled tables from the top left corner, the size its lettering makes it (times its `scale`). A space (a
//! slot no circuit takes) is struck through and a spare (a breaker with no device) is lettered in grey.

use super::{CELL_TEXT, INK, PAD, ROW, TITLE_ROW, TITLE_TEXT};
use crate::font;
use crate::model::Model;
use crate::panel_schedule::{Kind, Layout, PanelSchedule, Row, layout};
use crate::pen::Pen;
use crate::render::{Drawing, Rgb};

/// The lettering of a spare, which it has in place of the ink.
const GREY: Rgb = Rgb(128, 128, 128);

/// What a panel schedule is, laid out in px: each band's height, and the widths of the columns of its tables.
struct Plan {
    layout: Layout,
    k: f32,
    /// A side's column widths in the slots' table.
    cols: Vec<f32>,
    /// The summary tables' column widths.
    summary: Vec<Vec<f32>>,
    /// The whole width.
    width: f32,
}

fn text_width(text: &str, k: f32) -> f32 {
    font::width(text, CELL_TEXT * k)
}

/// Column widths to fit `rows` (the headings among them): the widest cell of each and room either side.
fn widths(rows: &[&Vec<String>], n: usize, k: f32) -> Vec<f32> {
    (0..n).map(|i| rows.iter().filter_map(|r| r.get(i)).map(|c| text_width(c, k)).fold(0.0, f32::max) + 2.0 * PAD * k).collect()
}

/// The rows of a side of the slots' table, in order: pairs for a branch panel (odd slots on the left, even on the right), else one.
type Pair<'a> = (Option<&'a Row>, Option<&'a Row>);

fn pairs(l: &Layout) -> Vec<Pair<'_>> {
    if l.sides < 2 {
        return l.rows.iter().map(|r| (Some(r), None)).collect();
    }
    let mut out: Vec<Pair<'_>> = Vec::new();
    for r in &l.rows {
        let line = (r.slot.saturating_sub(1) / 2) as usize;
        let key = |p: &Pair<'_>| p.0.or(p.1).map(|r| (r.slot.saturating_sub(1) / 2) as usize);
        match out.last_mut() {
            Some(last) if key(last) == Some(line) => last.1 = Some(r),
            _ => out.push(if r.slot % 2 == 1 { (Some(r), None) } else { (None, Some(r)) }),
        }
    }
    out
}

fn plan(m: &Model, ps: &PanelSchedule) -> Option<Plan> {
    let layout = layout(m, ps).ok()?;
    let k = ps.scale as f32;
    let n = layout.columns.len();
    let mut all: Vec<&Vec<String>> = vec![&layout.columns];
    all.extend(layout.rows.iter().map(|r| &r.cells));
    let cols = widths(&all, n, k);
    let summary: Vec<Vec<f32>> = layout
        .summary_tables
        .iter()
        .map(|(_, rows)| widths(&rows.iter().collect::<Vec<_>>(), rows.first().map_or(0, Vec::len), k))
        .collect();
    let grid = cols.iter().sum::<f32>() * layout.sides.max(1) as f32;
    let header = layout.header.iter().map(|(a, b)| text_width(&format!("{a}: {b}"), k) + 2.0 * PAD * k).fold(0.0, f32::max) * 2.0;
    let notes = layout.notes.lines().map(|l| text_width(l, k) + 2.0 * PAD * k).fold(0.0, f32::max);
    let title = font::width(&layout.title, TITLE_TEXT * k) + 2.0 * PAD * k;
    let width = [grid, header, notes, title].into_iter().chain(summary.iter().map(|w| w.iter().sum())).fold(0.0, f32::max);
    Some(Plan { layout, k, cols, summary, width })
}

/// How many rows a band takes.
fn rows_of(p: &Plan) -> usize {
    let header = p.layout.header.len().div_ceil(2);
    let grid = 1 + pairs(&p.layout).len();
    // A table's title above it and a row of room under it.
    let summary: usize = p.layout.summary_tables.iter().map(|(_, rows)| 2 + rows.len()).sum();
    let notes = p.layout.notes.lines().count();
    header + grid + summary + notes
}

/// The size of the panel schedule `ps`, px, as its tables make it.
pub fn size(m: &Model, ps: &PanelSchedule) -> Option<(u32, u32)> {
    let p = plan(m, ps)?;
    let height = TITLE_ROW * p.k + ROW * p.k * rows_of(&p) as f32;
    Some((p.width.ceil() as u32, height.ceil() as u32))
}

/// Panel schedule `ps` from its top left corner at `(x, y)` px.
pub fn draw(d: &mut Drawing, m: &Model, ps: &PanelSchedule, (x, y): (f32, f32)) {
    let Some(p) = plan(m, ps) else { return };
    let Some((w, h)) = size(m, ps) else { return };
    let (x, y, w, h) = (x.round(), y.round(), w as f32, h as f32);
    let (k, row) = (p.k, ROW * p.k);
    let rule = |d: &mut Drawing, a: (f32, f32), b: (f32, f32), pen: Pen| d.stroke(vec![a, b], pen, INK, None, false);
    let word = |d: &mut Drawing, s: &str, left: f32, top: f32, color: Rgb| {
        if !s.is_empty() {
            d.text(s, (left + PAD * k + text_width(s, k) / 2.0, top + row / 2.0), CELL_TEXT * k, color);
        }
    };
    d.stroke(vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)], Pen::HEAVY, INK, None, true);
    d.text(&p.layout.title, (x + w / 2.0, y + TITLE_ROW * k / 2.0), TITLE_TEXT * k, INK);
    let mut top = y + TITLE_ROW * k;
    rule(d, (x, top), (x + w, top), Pen::PROJECTION);

    // The header: label and value, two to a row.
    for (i, pair) in p.layout.header.chunks(2).enumerate() {
        for (j, (label, value)) in pair.iter().enumerate() {
            word(d, &format!("{label}: {value}"), x + w / 2.0 * j as f32, top + row * i as f32, INK);
        }
    }
    top += row * p.layout.header.len().div_ceil(2) as f32;
    rule(d, (x, top), (x + w, top), Pen::MEDIUM);

    // The slots: headings, then a row (for a branch panel, a pair of slots) each.
    let sides = p.layout.sides.max(1);
    let side = p.cols.iter().sum::<f32>();
    let ordered = |cells: &[String], mirrored: bool| -> Vec<String> {
        let mut c = cells.to_vec();
        if mirrored {
            c.reverse();
        }
        c
    };
    let draw_cells = |d: &mut Drawing, cells: &[String], left: f32, top: f32, mirrored: bool, color: Rgb| {
        let widths: Vec<f32> = if mirrored { p.cols.iter().rev().copied().collect() } else { p.cols.clone() };
        let mut at = left;
        for (text, width) in ordered(cells, mirrored).iter().zip(&widths) {
            word(d, text, at, top, color);
            at += width;
        }
    };
    for s in 0..sides {
        draw_cells(d, &p.layout.columns, x + side * s as f32, top, s == 1, INK);
    }
    let lines = pairs(&p.layout);
    let first = top;
    for (i, (left, right)) in lines.iter().enumerate() {
        let at = top + row * (i + 1) as f32;
        rule(d, (x, at), (x + side * sides as f32, at), if i == 0 { Pen::MEDIUM } else { Pen::FINE });
        for (s, r) in [left, right].into_iter().take(sides).enumerate() {
            let Some(r) = r else { continue };
            let left_edge = x + side * s as f32;
            let color = if matches!(r.kind, Kind::Spare | Kind::Space) { GREY } else { INK };
            draw_cells(d, &r.cells, left_edge, at, s == 1, color);
            if r.kind == Kind::Space {
                rule(d, (left_edge, at + row), (left_edge + side, at), Pen::FINE);
            }
        }
    }
    top += row * (lines.len() + 1) as f32;
    let mut edge = x;
    for s in 0..sides {
        // The second side runs the other way, so its columns do too.
        let widths: Vec<f32> = if s == 1 { p.cols.iter().rev().copied().collect() } else { p.cols.clone() };
        for width in widths.iter().take(widths.len().saturating_sub(1)) {
            edge += width;
            rule(d, (edge, first), (edge, top), Pen::FINE);
        }
        edge += widths.last().copied().unwrap_or(0.0);
        if s + 1 < sides {
            rule(d, (edge, first), (edge, top), Pen::MEDIUM);
        }
    }
    rule(d, (x, top), (x + w, top), Pen::MEDIUM);

    // The summary: a table for the classifications and one for the phases.
    for ((title, rows), widths) in p.layout.summary_tables.iter().zip(&p.summary) {
        word(d, title, x, top, INK);
        top += row;
        let total = widths.iter().sum::<f32>();
        let last = rows.len();
        let first = top;
        for (i, cells) in rows.iter().enumerate() {
            let at = top + row * i as f32;
            if i > 0 {
                rule(d, (x, at), (x + total, at), if i == 1 || i + 1 == last { Pen::MEDIUM } else { Pen::FINE });
            }
            let mut left = x;
            for (text, width) in cells.iter().zip(widths) {
                word(d, text, left, at, INK);
                left += width;
            }
        }
        top += row * rows.len() as f32;
        let mut left = x;
        for width in widths.iter().take(widths.len().saturating_sub(1)) {
            left += width;
            rule(d, (left, first), (left, top), Pen::FINE);
        }
        rule(d, (x, top), (x + total, top), Pen::MEDIUM);
        top += row;
    }

    // The notes.
    for line in p.layout.notes.lines() {
        word(d, line, x, top, INK);
        top += row;
    }
}
