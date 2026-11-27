//! The right-click menu in the app: where it is open, the items the moment offers (`context::items`) and what each does. A child of
//! `app`, so it reaches the private handlers (`run`); `app.rs` only declares it.

use bimcraft_core::ids::Id;
use bimcraft_core::model::Element;
use serde_json::json;

use super::App;
use crate::context::{self, Action, Item};

impl App {
    /// A right click at window point `x`, `y` opens the menu there; a point that is no number opens nothing.
    pub fn open_menu(&mut self, x: f32, y: f32) {
        if x.is_finite() && y.is_finite() {
            self.tooling.menu = Some((x, y));
        }
    }

    pub fn close_menu(&mut self) {
        self.tooling.menu = None;
    }

    /// What the menu offers now, in groups: a tool in hand with points picked can be finished.
    pub fn menu_items(&self) -> Vec<Vec<Item>> {
        context::items(&self.session.model, &self.selection, self.tool.is_some(), self.tool.is_some() && !self.picked.is_empty())
    }

    /// Does what an item of the menu says, and closes the menu.
    pub fn act(&mut self, action: Action) {
        // The Project Browser's menu is another menu: its items are `act_browser`'s, and it reads what the click was over first.
        if action.is_browser() {
            return self.act_browser(action);
        }
        self.tooling.menu = None;
        match action {
            Action::Finish => {
                self.finish_tool();
            }
            Action::Cancel => {
                self.escape();
            }
            Action::Delete => {
                self.delete_selection();
            }
            Action::Pin => self.pin_selection(true),
            Action::Unpin => self.pin_selection(false),
            Action::DisallowJoin => self.set_joins(false),
            Action::AllowJoin => self.set_joins(true),
            Action::ZoomToFit => self.zoom_fit(),
            Action::ZoomIn => self.zoom_by(1.25),
            Action::ZoomOut => self.zoom_by(0.8),
            // The browser's items were done above.
            _ => {}
        }
    }

    /// Pins every selected element, or unpins them, as one command; those already as asked are left.
    fn pin_selection(&mut self, pinned: bool) {
        let ids: Vec<Id> = self.selection.iter().copied().filter(|id| self.session.model.is_pinned(*id) != pinned).collect();
        if !ids.is_empty() {
            self.run(if pinned { "pin" } else { "unpin" }, &json!({"ids": ids}));
        }
    }

    /// Turns the joins of every selected wall and run off or on, as one command: the flag is the whole element's, both ends alike.
    fn set_joins(&mut self, join: bool) {
        let ids: Vec<Id> = self
            .selection
            .iter()
            .copied()
            .filter(|id| self.session.model.elements.get(*id).is_some_and(|e| matches!(e, Element::Wall(_)) || e.run().is_some()))
            .collect();
        if !ids.is_empty() {
            self.run("modify", &json!({"ids": ids, "join": join}));
        }
    }
}

#[cfg(test)]
#[path = "menu_tests.rs"]
mod tests;
