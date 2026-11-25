//! Showing another view. What comes with a view is dropped (the pointer, the box, the points a tool had picked, the camera's
//! history, the frame); what belongs to the model stays, and the selection is the model's, not a view's, so opening any view
//! keeps it, as egui's `open_sheet` does.

use super::App;
use crate::views::{self, ViewKind};

impl App {
    /// Shows `kind`, framed. False when it is already on screen or the model has no such view, and nothing changes.
    pub fn open_view(&mut self, kind: ViewKind) -> bool {
        if kind == self.view || !views::exists(&self.session.model, kind) {
            return false;
        }
        self.leave();
        (self.fitted, self.moved) = (false, false);
        (self.rubber, self.region) = (None, false);
        self.history.clear();
        self.picked.clear();
        self.view = kind;
        if let Some(level) = views::level_of(&self.session.model, kind) {
            self.level = Some(level);
        }
        self.settle();
        true
    }
}

#[cfg(test)]
mod tests;
