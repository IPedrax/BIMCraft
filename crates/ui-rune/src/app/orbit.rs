//! The 3D view shown: what its canvas's handlers do to `App`, which asks the view's thread (`orbit::Orbit`) for the picture and takes
//! it in. As in a section, the selection and the hovered element are the model's, and the selected elements are tinted in the picture.
//! Revit's navigation, as egui has it: the wheel zooms about the point under the pointer, the middle button pans, Shift with it orbits
//! round the point of the surface under the pointer where the drag began; the primary button picks, or drags out a box.

use super::{App, CLICK};
use crate::select::{self, How, Rubber};
use crate::views::ViewKind;

impl App {
    /// The 3D canvas has laid out at `w` x `h` points.
    pub fn orbit_resize(&mut self, w: f32, h: f32) {
        if self.orbit.resize(w, h) {
            self.ask_orbit();
        }
    }

    /// Asks for the picture of the 3D view shown, for its camera, the model and what is selected; another kind of view lets the thread,
    /// the picture and the camera go.
    pub(super) fn ask_orbit(&mut self) {
        match self.view {
            ViewKind::View3D(view) => {
                self.orbit.show(view, self.size);
                self.orbit.ask(&self.session.model, &self.selection);
            }
            _ => self.orbit.stop(),
        }
    }

    /// The pointer is at `x`, `y` over the 3D view: what the picture shows under it is hovered.
    pub fn orbit_hover(&mut self, x: f32, y: f32) {
        let at = (x.is_finite() && y.is_finite()).then_some((x, y));
        self.orbit.cursor = at;
        self.hovered = at.and_then(|(x, y)| self.orbit.pick(x, y));
    }

    pub fn orbit_leave(&mut self) {
        self.orbit.cursor = None;
        self.hovered = None;
    }

    /// The wheel: closer (`factor` above 1) about the point under the pointer.
    pub fn orbit_zoom(&mut self, x: f32, y: f32, factor: f32) {
        self.orbit.zoom((x, y), f64::from(factor));
        self.ask_orbit();
    }

    /// The zoom buttons: closer or farther about the middle of the canvas.
    pub fn orbit_zoom_by(&mut self, factor: f64) {
        let (w, h) = self.orbit.size;
        self.orbit.zoom((w / 2.0, h / 2.0), factor);
        self.ask_orbit();
    }

    /// Zoom to Fit: the model framed again from where the camera looks from.
    pub fn orbit_fit(&mut self) {
        self.orbit.fit();
        self.ask_orbit();
    }

    /// A press of the middle button at `at` begins to move the camera: it orbits with Shift, else pans.
    pub fn orbit_grab(&mut self, at: (f32, f32), orbit: bool) {
        self.orbit.grab(at, orbit);
    }

    /// The pointer has travelled `dx`, `dy` points since the press: the camera follows.
    pub fn orbit_drag(&mut self, dx: f32, dy: f32) {
        self.orbit.drag(dx, dy);
        self.ask_orbit();
    }

    pub fn orbit_release(&mut self) {
        self.orbit.release();
    }

    /// The primary button is dragging, from `from` to `to`: it drags out a box over the picture.
    pub fn orbit_sweep(&mut self, from: (f32, f32), to: (f32, f32)) {
        if ![from.0, from.1, to.0, to.1].iter().all(|v| v.is_finite()) || (to.0 - from.0).hypot(to.1 - from.1) <= CLICK {
            return;
        }
        self.rubber = Some(Rubber { from, to });
    }

    /// The press that began at `from` ended at `to`: a click if it hardly moved, which picks what the picture shows there; else the box
    /// it dragged out takes what the picture shows only inside it, or anything of it when it went right to left.
    pub fn orbit_select(&mut self, from: (f32, f32), to: (f32, f32), how: How) {
        let band = self.rubber.take();
        if ![from.0, from.1, to.0, to.1].iter().all(|v| v.is_finite()) {
            return;
        }
        if (to.0 - from.0).hypot(to.1 - from.1) <= CLICK {
            let found = self.orbit.pick(to.0, to.1);
            select::apply(&mut self.selection, found.into_iter().collect(), how);
            self.refresh();
        } else if band.is_some() {
            let r = Rubber { from, to };
            let found = self.orbit.in_box(r.from, r.to, r.crossing());
            select::apply(&mut self.selection, found, how);
            self.refresh();
        }
    }
}

#[cfg(test)]
mod tests;
