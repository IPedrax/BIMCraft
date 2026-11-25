//! The sheet shown: what its canvas's handlers do to `App`, which asks the sheet's thread (`paper::Paper`) for the picture and takes it
//! in. As in a section, the selection and the hovered element are the model's: a click on a view of the sheet selects its viewport.

use super::{App, CLICK};
use crate::select::{self, How, Rubber};
use crate::views::ViewKind;

impl App {
    /// The sheet canvas has laid out at `w` x `h` points.
    pub fn paper_resize(&mut self, w: f32, h: f32) {
        if self.paper.resize(w, h) {
            self.ask_paper();
        }
    }

    /// Asks for the picture of the sheet shown, for its camera and the model; another kind of view lets the thread and the picture go.
    pub(super) fn ask_paper(&mut self) {
        match self.view {
            ViewKind::Sheet(id) => {
                self.paper.show(id, self.size);
                self.paper.ask(&self.session.model);
            }
            _ => self.paper.stop(),
        }
    }

    /// The pointer is at `x`, `y` over the sheet: what is under it is hovered.
    pub fn paper_hover(&mut self, x: f32, y: f32) {
        let at = (x.is_finite() && y.is_finite()).then_some((x, y));
        self.paper.cursor = at;
        self.hovered = at.and_then(|(x, y)| self.paper.pick(x, y));
    }

    pub fn paper_leave(&mut self) {
        self.paper.cursor = None;
        self.hovered = None;
    }

    /// The wheel: closer (`factor` above 1) about the pointer.
    pub fn paper_zoom(&mut self, x: f32, y: f32, factor: f32) {
        self.paper.zoom((x, y), f64::from(factor));
        self.ask_paper();
    }

    /// The zoom buttons: closer or farther about the middle of the canvas.
    pub fn paper_zoom_by(&mut self, factor: f64) {
        let (w, h) = self.paper.size;
        self.paper.zoom((w / 2.0, h / 2.0), factor);
        self.ask_paper();
    }

    /// Zoom to Fit: the whole paper framed again.
    pub fn paper_fit(&mut self) {
        self.paper.fit();
        self.ask_paper();
    }

    pub fn paper_grab(&mut self) {
        self.paper.followed = (0.0, 0.0);
    }

    /// The pointer has travelled `dx`, `dy` points since the press: the sheet follows it.
    pub fn paper_pan(&mut self, dx: f32, dy: f32) {
        self.paper.pan(dx, dy);
        self.ask_paper();
    }

    /// The primary button is dragging, from `from` to `to`: it drags out a box (a window whichever way, with Zoom In Region armed).
    pub fn paper_sweep(&mut self, from: (f32, f32), to: (f32, f32)) {
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
    pub fn paper_release(&mut self, from: (f32, f32), to: (f32, f32), how: How) {
        let band = self.rubber.take();
        if ![from.0, from.1, to.0, to.1].iter().all(|v| v.is_finite()) {
            return;
        }
        if self.region {
            if band.is_some() && self.paper.zoom_region(from, to) {
                self.region = false;
                self.ask_paper();
            }
        } else if (to.0 - from.0).hypot(to.1 - from.1) <= CLICK {
            let found = self.paper.pick(to.0, to.1);
            select::apply(&mut self.selection, found.into_iter().collect(), how);
            self.refresh();
        } else if band.is_some() {
            let r = Rubber { from, to };
            let found = self.paper.in_box(r.from, r.to, r.crossing());
            select::apply(&mut self.selection, found, how);
            self.refresh();
        }
    }
}

#[cfg(test)]
mod tests;
