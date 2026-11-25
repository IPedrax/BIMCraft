//! The schedule shown: its grid is made (`sched::Grid`) whenever the model or the view changes, and a click on a row picks what the
//! row lists. The selection is the model's, so the plan and the section have it selected too.

use super::App;
use crate::sched;
use crate::select::{self, How};
use crate::views::ViewKind;

impl App {
    /// Makes the grid of the schedule shown, for the model as it is; another kind of view lets the grid go.
    pub(super) fn make_grid(&mut self) {
        match self.view {
            ViewKind::Schedule(_) | ViewKind::Table(_) => self.grid.rebuild(&self.session.model, self.view),
            _ => self.grid.clear(),
        }
    }

    /// A click on row `row` of the grid with `how` (Ctrl adds what it lists, Shift takes it away): picks what the row lists.
    pub fn pick_row(&mut self, row: usize, how: How) {
        let Some(ids) = self.grid.shown.ids.get(row) else { return };
        select::apply(&mut self.selection, ids.clone(), how);
        self.refresh();
    }

    /// Whether row `row` lists something selected: the grid lights it.
    pub fn row_lit(&self, row: usize) -> bool {
        sched::lists_selected(&self.grid.shown, row, &self.selection)
    }
}

#[cfg(test)]
mod tests;
