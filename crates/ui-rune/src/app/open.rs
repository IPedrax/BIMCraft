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
        self.close_menu();
        (self.fitted, self.moved) = (false, false);
        (self.rubber, self.region) = (None, false);
        self.history.clear();
        self.picked.clear();
        // A paste waiting for its click follows the pointer in a plan only.
        if !crate::clip::by_hand(kind) {
            self.pasting = None;
        }
        self.view = kind;
        if let Some(level) = views::level_of(&self.session.model, kind) {
            self.level = Some(level);
        }
        self.settle();
        true
    }

    /// Opens the first view of a kind by its short name (`section`, `elevation`, `sheet`, `schedule`, `ceiling`, `3d`): what the
    /// measurement starts a window on (`BIMCRAFT_RUNE_VIEW`). False for another name, or a project with none of that kind.
    pub fn open_named(&mut self, name: &str) -> bool {
        let first = |group: &str| self.views.groups.iter().find(|g| g.label == group).and_then(|g| g.entries.first()).map(|e| e.kind);
        let kind = match name {
            "3d" => Some(ViewKind::View3D(None)),
            "section" => first("Sections (Building Section)"),
            "elevation" => first("Elevations (Building Elevation)"),
            "sheet" => first("Sheets (all)"),
            "schedule" => first("Schedules/Quantities (all)"),
            "ceiling" => first("Ceiling Plans"),
            _ => None,
        };
        kind.is_some_and(|k| self.open_view(k))
    }

    /// After the model, the selection or the view may have changed: what the view shown is made of is made again (the section's
    /// picture is asked for, the schedule's grid is built), and what another kind of view held is let go.
    pub(super) fn follow_view(&mut self) {
        self.ask_section();
        self.ask_paper();
        self.ask_orbit();
        self.make_grid();
    }
}

#[cfg(test)]
mod tests;
