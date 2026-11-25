//! The section or elevation shown: what its canvas's handlers do to `App`, which asks the section's thread (`section::Cut`) for the
//! picture and takes it in. The selection and the hovered element are the model's, as in a plan: a click in a section selects what the
//! picture shows there, and the plan shows it selected.

use super::{App, CLICK};
use crate::select::{self, How, Rubber};
use crate::views::ViewKind;

impl App {
    /// The section canvas has laid out at `w` x `h` points.
    pub fn section_resize(&mut self, w: f32, h: f32) {
        if self.cut.resize(w, h) {
            self.ask_section();
        }
    }

    /// Asks for the picture of the section shown, for its camera, the model and what is selected: nothing is asked of another kind of
    /// view, which lets the thread and the picture go. Called after anything that may have changed one of them.
    pub(super) fn ask_section(&mut self) {
        match self.view {
            ViewKind::Section(id) => {
                self.cut.show(id, self.size);
                self.cut.ask(&self.session.model, &self.selection);
            }
            _ => self.cut.stop(),
        }
    }

    /// The pointer is at `x`, `y` over the section: what is under it is hovered.
    pub fn section_hover(&mut self, x: f32, y: f32) {
        let at = (x.is_finite() && y.is_finite()).then_some((x, y));
        self.cut.cursor = at;
        self.hovered = at.and_then(|(x, y)| self.cut.pick(x, y));
    }

    pub fn section_leave(&mut self) {
        self.cut.cursor = None;
        self.hovered = None;
    }

    /// The wheel: closer (`factor` above 1) about the pointer.
    pub fn section_zoom(&mut self, x: f32, y: f32, factor: f32) {
        self.cut.zoom((x, y), f64::from(factor));
        self.ask_section();
    }

    /// The zoom buttons: closer or farther about the middle of the canvas.
    pub fn section_zoom_by(&mut self, factor: f64) {
        let (w, h) = self.cut.size;
        self.cut.zoom((w / 2.0, h / 2.0), factor);
        self.ask_section();
    }

    /// Zoom to Fit: the section framed again.
    pub fn section_fit(&mut self) {
        self.cut.fit();
        self.ask_section();
    }

    /// A press on the section starts a drag.
    pub fn section_grab(&mut self) {
        self.cut.followed = (0.0, 0.0);
    }

    /// The pointer has travelled `dx`, `dy` points since the press: the view follows it.
    pub fn section_pan(&mut self, dx: f32, dy: f32) {
        self.cut.pan(dx, dy);
        self.ask_section();
    }

    /// The primary button is dragging, from `from` to `to`: it drags out a box over the section (a window whichever way it goes, when
    /// Zoom In Region is armed).
    pub fn section_sweep(&mut self, from: (f32, f32), to: (f32, f32)) {
        if ![from.0, from.1, to.0, to.1].iter().all(|v| v.is_finite()) || (to.0 - from.0).hypot(to.1 - from.1) <= CLICK {
            return;
        }
        self.rubber = Some(if self.region {
            Rubber { from: (from.0.min(to.0), from.1.min(to.1)), to: (from.0.max(to.0), from.1.max(to.1)) }
        } else {
            Rubber { from, to }
        });
    }

    /// The press that began at `from` ended at `to`: a click if it hardly moved, which picks; else the box it dragged out takes what
    /// it holds, or what it touches when it went right to left (or zooms to it, with Zoom In Region armed).
    pub fn section_release(&mut self, from: (f32, f32), to: (f32, f32), how: How) {
        let band = self.rubber.take();
        if ![from.0, from.1, to.0, to.1].iter().all(|v| v.is_finite()) {
            return;
        }
        if self.region {
            if band.is_some() && self.cut.zoom_region(from, to) {
                self.region = false;
                self.ask_section();
            }
        } else if (to.0 - from.0).hypot(to.1 - from.1) <= CLICK {
            let found = self.cut.pick(to.0, to.1);
            select::apply(&mut self.selection, found.into_iter().collect(), how);
            self.refresh();
        } else if band.is_some() {
            let r = Rubber { from, to };
            let found = self.cut.in_box(r.from, r.to, r.crossing());
            select::apply(&mut self.selection, found, how);
            self.refresh();
        }
    }

    /// Every frame: takes in the picture the section's thread has made. True when what is on screen changed.
    pub fn poll_views(&mut self) -> bool {
        self.cut.poll()
    }

    /// Whether a view's thread is at work, so the window must come back for its picture.
    pub fn views_busy(&self) -> bool {
        self.cut.busy()
    }
}

#[cfg(test)]
mod tests;
