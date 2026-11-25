//! The grips in the app: which element has them, the press that takes one, the drag that follows the pointer with the snap from the
//! end that stays, and the `move` it ends in. A child of `app` so it reaches the private handlers (`run`); `app.rs` only declares it.

use bimcraft_core::geom::P2;
use bimcraft_core::ids::Id;
use bimcraft_core::snap;

use super::App;
use crate::grips::{self, Grip};

/// How near a grip a press must be to take it, points: its square is nine across.
const GRIP_REACH: f64 = 8.0;

/// The grips of the element selected alone, as the plan draws them: which element, its two ends as they stand (the one being dragged
/// where it would go), and the drag.
pub struct Shown {
    pub id: Id,
    pub ends: [P2; 2],
    pub held: Option<Grip>,
}

impl App {
    /// The element whose ends are grips: selected alone, with no tool in hand.
    pub fn gripped(&self) -> Option<Id> {
        grips::gripped(&self.session.model, &self.selection, self.tool.is_some())
    }

    /// Whether an end is being dragged.
    pub fn gripping(&self) -> bool {
        self.tooling.grip.is_some()
    }

    /// The grips to draw now, if the element selected alone has any.
    pub fn grips_shown(&self) -> Option<Shown> {
        let id = self.gripped()?;
        let (a, b) = grips::ends(&self.session.model, id)?;
        let held = self.tooling.grip.filter(|g| g.id == id);
        let mut ends = [a, b];
        if let Some(g) = held {
            ends[usize::from(!g.start)] = g.to;
        }
        Some(Shown { id, ends, held })
    }

    /// A press at canvas point `x`, `y`: on an end of the gripped element it takes that grip, and the press is the drag of it. False
    /// when it is not on one, and the press goes on as it did.
    pub fn grip_down(&mut self, x: f32, y: f32) -> bool {
        let Some(id) = self.gripped() else { return false };
        let Some(ends) = grips::ends(&self.session.model, id) else { return false };
        let p = self.camera.view(self.size.0, self.size.1).mm((x, y));
        let Some(start) = grips::end_at(ends, p, GRIP_REACH / self.camera.scale) else { return false };
        self.tooling.grip = Some(Grip { id, start, to: if start { ends.0 } else { ends.1 } });
        true
    }

    /// The pointer is at canvas point `x`, `y` with the end held: it goes where the pointer lands, from the end that stays (with
    /// `keep_length`, turning about it at the length it has). A pointer that lands nowhere leaves it where it was.
    pub fn grip_move(&mut self, x: f32, y: f32, keep_length: bool) {
        let Some(grip) = self.tooling.grip else { return };
        let (Some(ends), Some(level)) = (grips::ends(&self.session.model, grip.id), self.level) else { return };
        let fixed = if grip.start { ends.1 } else { ends.0 };
        let view = self.camera.view(self.size.0, self.size.1);
        let Some(landing) = self.snapper.land(&self.session.model, level, &view, (x, y), &[fixed], self.square) else { return };
        let to = grips::target(ends, grip.start, landing.at, keep_length);
        if to.is_finite() {
            self.tooling.grip = Some(Grip { to, ..grip });
        }
    }

    /// The press is let go: the end moves, unless the drag was cancelled.
    pub fn grip_up(&mut self, cancelled: bool) {
        let Some(grip) = self.tooling.grip.take() else { return };
        if cancelled {
            return;
        }
        if let Some((name, params)) = grips::command(&self.session.model, grip) {
            self.run(name, &params);
        }
    }

    /// What a dragged end reads: its place and a ruler's length and angle from the end that stays, as the egui window tags it.
    pub fn grip_reading(&self) -> Option<(P2, String)> {
        let g = self.tooling.grip?;
        let (a, b) = grips::ends(&self.session.model, g.id)?;
        let fixed = if g.start { b } else { a };
        let r = snap::ruler(fixed, g.to);
        Some((g.to, format!("{}  {:.1}\u{b0}", self.session.model.length(r.length), r.angle)))
    }
}

#[cfg(test)]
#[path = "grips_tests.rs"]
mod tests;
