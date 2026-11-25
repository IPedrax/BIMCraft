//! The canvas of a schedule: the core's table as a grid (`rune_kit::Table`: only the rows on screen are built, so a schedule of ten
//! thousand rows costs what the visible thirty do), the totals under it. A row is lit while what it lists is selected, and a click on
//! it picks that (Ctrl adds, Shift takes away), as egui's schedule does.

use std::borrow::Cow;

use rune::tok::{Theme, type_};
use rune::{Align, El, col, row, text};
use rune_kit::{Cell, Kit, Table, TableColumn, TableState};

use crate::app::App;
use crate::lang::t;
use crate::paint::Palette;
use crate::select::How;

fn row_id(_: &App, i: usize) -> Cow<'_, str> {
    Cow::Owned(i.to_string())
}

fn state(s: &App) -> &TableState {
    &s.grid.table
}

fn state_mut(s: &mut App) -> &mut TableState {
    &mut s.grid.table
}

fn count(s: &App) -> usize {
    s.grid.shown.rows.len()
}

pub fn view(s: &App, th: &Theme) -> El {
    let shown = &s.grid.shown;
    let mut page = col()
        .key("schedule")
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .fill(th.bg)
        .padding(16.0)
        .gap(8.0)
        .child(text(shown.name.clone()).font(type_::TITLE).color(th.text));
    page = match &shown.note {
        Some(note) => page.child(text(note.clone()).key("schedule-note").font(type_::BODY).color(th.text_secondary)),
        None => page.child(grid(s)),
    };
    if shown.cut {
        page = page.child(text(t("Only the first rows are shown.")).font(type_::LABEL).color(th.text_secondary));
    }
    if let Some(totals) = &shown.totals {
        let line: Vec<String> =
            shown.columns.iter().zip(totals).filter(|(_, v)| !v.is_empty()).map(|(h, v)| format!("{}: {v}", h.title)).collect();
        page =
            page.child(row().key("schedule-totals").gap(16.0).child(text(line.join("    ")).font(type_::LABEL).color(th.text).truncate()));
    }
    page
}

/// The table: a column for each of the schedule's, a row for each of its rows.
fn grid(s: &App) -> El {
    let kit = Kit::neutral(true);
    let lit = Palette::dark().accent_fill;
    let mut table = Table::new(state, state_mut, count, row_id).name("schedule").zebra(true);
    for (i, head) in s.grid.shown.columns.iter().enumerate() {
        let mut column = TableColumn::new(i.to_string(), head.title.clone()).min(80.0);
        if head.numeric {
            column = column.align(Align::End);
        }
        table = table.column(column);
    }
    table
        .cell(|s: &App, row: usize, column: &str| {
            let i: usize = column.parse().unwrap_or(usize::MAX);
            let text = s.grid.shown.rows.get(row).and_then(|r| r.get(i)).cloned().unwrap_or_default();
            let cell = Cell::text(text);
            if s.grid.shown.columns.get(i).is_some_and(|h| h.numeric) { cell.mono() } else { cell }
        })
        .row(move |s: &App, row: usize, el: El| if s.row_lit(row) { el.fill(lit) } else { el })
        .on_select(|s: &mut App, id: &str, tx| {
            if let Ok(row) = id.parse::<usize>() {
                let m = tx.mods();
                s.pick_row(row, How::of(m.shift, m.cmd));
            }
        })
        .build(s, &kit)
}
