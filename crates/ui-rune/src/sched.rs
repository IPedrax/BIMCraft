//! A schedule on screen: the core's `schedule::Table` made into what a grid shows, its cells as text in the project's units and
//! language, and the elements each row lists (a click on a row picks them, and a row is lit while they are selected). Pure but for the
//! grid's own state (`rune_kit::TableState`, which keeps the keyboard cursor), so what a row says and picks is tested with no window.
//! Egui's `schedule` view (`ui-egui/src/lib.rs`) is the reference.

use std::sync::Arc;

use bimcraft_core::ids::Id;
use bimcraft_core::model::{Element, Model};
use bimcraft_core::schedule::{self, Column, Kind};
use rune_kit::TableState;
use serde_json::Value;

use crate::lang::{t, tf};
use crate::views::{self, ViewKind};

/// The most rows the grid holds: a hostile schedule of a hundred thousand rows is cut here, and the grid says so.
pub const MAX_ROWS: usize = 50_000;

/// A column's heading as the project's language and units say it, and whether its cells are numbers (set at the end).
#[derive(Clone, Debug, PartialEq)]
pub struct Head {
    pub title: String,
    pub numeric: bool,
}

/// What the grid shows of the schedule on screen.
#[derive(Default)]
pub struct Shown {
    pub name: String,
    pub columns: Vec<Head>,
    pub rows: Vec<Vec<String>>,
    /// The elements each row lists: one, or a group's.
    pub ids: Vec<Vec<Id>>,
    /// The totals under the columns that have one, empty under the rest.
    pub totals: Option<Vec<String>>,
    /// Why there is no grid: the core refuses the schedule, it has no rows yet, or the view is not a schedule this window draws.
    pub note: Option<String>,
    /// The rows were cut at [`MAX_ROWS`].
    pub cut: bool,
    /// The model it was made of, to know it again.
    model: Option<Arc<Model>>,
    kind: ViewKind,
}

/// The grid: what it shows, and its state.
pub struct Grid {
    pub shown: Shown,
    pub table: TableState,
}

impl Default for Grid {
    fn default() -> Grid {
        Grid { shown: Shown::default(), table: TableState::new().key("schedule") }
    }
}

impl Grid {
    /// Makes the grid of `kind` for `model`, unless it is the one it has. Another schedule starts with a clean state.
    pub fn rebuild(&mut self, model: &Arc<Model>, kind: ViewKind) {
        if self.shown.kind == kind && self.shown.model.as_ref().is_some_and(|m| Arc::ptr_eq(m, model)) {
            return;
        }
        if self.shown.kind != kind {
            self.table = TableState::new().key("schedule");
        }
        self.shown = make(model, kind);
    }

    /// Another kind of view is shown: the grid is let go.
    pub fn clear(&mut self) {
        if self.shown.model.is_some() {
            *self = Grid::default();
        }
    }
}

/// Whether the row `i` lists something selected.
pub fn lists_selected(shown: &Shown, i: usize, selection: &[Id]) -> bool {
    shown.ids.get(i).is_some_and(|ids| ids.iter().any(|id| selection.contains(id)))
}

/// The grid of `kind` for `model`.
pub fn make(model: &Arc<Model>, kind: ViewKind) -> Shown {
    let m: &Model = model;
    let name = views::name(m, kind);
    let mut shown = Shown { name: name.clone(), model: Some(Arc::clone(model)), kind, ..Shown::default() };
    let found = match kind {
        ViewKind::Schedule(cat) => Some(schedule::standard(m, cat)),
        ViewKind::Table(id) => match m.elements.get(id) {
            Some(Element::Schedule(sc)) => Some((Some(id), sc.clone())),
            _ => None,
        },
        _ => None,
    };
    let Some((id, sc)) = found else {
        // A panel schedule or a reinforcement detail: their own views, later.
        shown.note = Some(t("This view is not drawn in this window yet.").to_string());
        return shown;
    };
    let table = match schedule::table_kept(m, id, &sc) {
        Ok(table) => table,
        Err(why) => {
            shown.note = Some(why);
            return shown;
        }
    };
    if table.rows.is_empty() {
        shown.note = Some(tf("No rows yet: the {} is empty.", &[&name.to_lowercase()]));
        return shown;
    }
    let (units, decimals) = (m.units, m.decimals);
    shown.columns = table
        .columns
        .iter()
        .map(|c| {
            let heading = Column { heading: t(&c.heading).to_string(), kind: c.kind };
            Head { title: schedule::heading(units, &heading), numeric: matches!(c.kind, Kind::Number | Kind::Length | Kind::Area) }
        })
        .collect();
    let said = |c: &Column, v: &Value| {
        let text = schedule::shown_to(units, decimals, c.kind, v);
        if v.is_boolean() { t(&text).to_string() } else { text }
    };
    shown.cut = table.rows.len() > MAX_ROWS;
    shown.rows = table.rows.iter().take(MAX_ROWS).map(|row| row.iter().zip(&table.columns).map(|(v, c)| said(c, v)).collect()).collect();
    shown.ids = table.ids.iter().take(MAX_ROWS).cloned().collect();
    shown.totals = table.totals.as_ref().map(|totals| {
        totals
            .iter()
            .zip(&table.columns)
            .enumerate()
            .map(|(i, (v, c))| match v {
                Value::Null if i == 0 => t("Total").to_string(),
                Value::Null => String::new(),
                v => said(c, v),
            })
            .collect()
    });
    shown
}

#[cfg(test)]
mod tests;
