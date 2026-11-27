//! Underlays in the window: another view drawn under (or over) the plan on screen, as Archicad's Trace & Reference, after egui's
//! `underlay.rs`. What it is and how it is drawn are the core's (`bimcraft_core::underlay`; the `underlay` command sets it, saved with the
//! view); this asks for it (Show as Underlay in the Project Browser's menu), and draws it through the camera when the plan is drawn
//! (`App::redraw`), as the plan's own drawing is. Plans only: a section's or an elevation's underlay is the core's, not drawn here yet.
//! A child of `app` so it reaches the private handlers.

use bimcraft_core::ids::Id;
use bimcraft_core::render::Drawing;
use bimcraft_core::underlay;
use serde_json::{Value, json};

use super::App;
use crate::lang::t;
use crate::views::{self, ViewKind};

impl App {
    /// The plan on screen when it can have an underlay: a level's plan or a plan view.
    pub fn underlay_target(&self) -> Option<Id> {
        match self.view {
            ViewKind::Plan(id) => underlay::kind(&self.session.model, id).map(|_| id),
            _ => None,
        }
    }

    /// Whether the view `kind` can be shown under the plan on screen: another plan. A section is the core's to draw as a picture, which is
    /// a worker's job (egui's), not a redraw's.
    pub fn can_underlay(&self, kind: ViewKind) -> bool {
        let m = &self.session.model;
        if !matches!(kind, ViewKind::Plan(_)) {
            return false;
        }
        let (Some(target), Some(source)) = (self.underlay_target(), views::element_id(m, kind)) else { return false };
        source != target && underlay::kind(m, source).is_some()
    }

    /// Whether the plan on screen has an underlay (on or off).
    pub fn has_underlay(&self) -> bool {
        self.underlay_target().is_some_and(|id| underlay::of(&self.session.model, id).is_some())
    }

    /// Show as Underlay: `source` under the plan on screen, on, as an underlay starts. False when there is no plan to put it under.
    pub fn show_as_underlay(&mut self, source: Id) -> bool {
        self.set_underlay(json!(source))
    }

    /// Takes the underlay off the plan on screen; its settings go with it.
    pub fn remove_underlay(&mut self) -> bool {
        self.set_underlay(Value::Null)
    }

    fn set_underlay(&mut self, source: Value) -> bool {
        let Some(view) = self.underlay_target() else {
            self.message = Some(t("An underlay is shown under a plan: open one first.").to_string());
            return false;
        };
        self.run("underlay", &json!({"view": view, "source": source}))
    }

    /// The underlay of the plan on screen as it is drawn through the camera, and whether it is over the plan's lines. None when the plan
    /// shows none. Drawn with the plan, so one more drawing of a view while it is on (a worker's, as egui's is, when that is too slow).
    pub(super) fn underlay_drawing(&self) -> Option<(Drawing, bool)> {
        let id = self.underlay_target()?;
        let u = underlay::draw(&self.session.model, id, self.camera.view(self.size.0, self.size.1))?;
        let front = u.front;
        Some((u.into_drawing(), front))
    }
}

#[cfg(test)]
#[path = "underlay_tests.rs"]
mod tests;
