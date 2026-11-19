//! A selected image's grips in a plan: a square at each corner. A drag from one resizes it about the corner across from it,
//! keeping its aspect, and on release runs `modify {width, at}`.

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id};
use egui::{Painter, PointerButton, Rect, Response, Stroke, StrokeKind, Ui, vec2};
use serde_json::json;

use crate::style::{canvas_pal, pal};
use crate::{BimApp, Tool};

/// An image's corners in plan, in turn: lower left, lower right, upper right, upper left.
fn corners(lo: P2, hi: P2) -> [P2; 4] {
    [lo, P2(hi.0, lo.1), hi, P2(lo.0, hi.1)]
}

/// The width and the middle of an image resized by its corner `k` dragged to `to`, the corner across from it staying put:
/// as wide as the drag reaches across, or as tall, whichever is more, at its aspect (width over height, `aspect`).
pub(crate) fn resized(lo: P2, hi: P2, k: usize, to: P2, aspect: f64) -> Option<(f64, P2)> {
    let c = corners(lo, hi);
    let fixed = c[(k + 2) % 4];
    let (dx, dy) = ((to.0 - fixed.0).abs(), (to.1 - fixed.1).abs());
    let width = dx.max(dy * aspect);
    if !(width.is_finite() && width > 1.0 && aspect > 0.0) {
        return None;
    }
    let height = width / aspect;
    // Toward the dragged corner from the one that stays.
    let (sx, sy) = ((c[k].0 - fixed.0).signum(), (c[k].1 - fixed.1).signum());
    Some((width, P2(fixed.0 + sx * width / 2.0, fixed.1 + sy * height / 2.0)))
}

impl BimApp {
    /// The image selected alone, with Modify, and its corners: what has grips.
    pub(crate) fn image_gripped(&self) -> Option<(Id, P2, P2)> {
        let m = &self.session.model;
        match (self.selection.as_slice(), &self.tool) {
            (&[id], Tool::Modify) => match m.elements.get(id) {
                Some(Element::Image(im)) => im.bounds(m).map(|[lo, hi]| (id, lo, hi)),
                _ => None,
            },
            _ => None,
        }
    }

    /// A drag from a corner of the selected image: held while it goes, and on release the image resized to where it stopped.
    pub(crate) fn image_grip_drag(&mut self, ui: &Ui, response: &Response, rect: Rect, view: &bimcraft_core::render::View) {
        let Some((id, lo, hi)) = self.image_gripped() else {
            self.image_grip = None;
            return;
        };
        let at = |q: P2| {
            let (x, y) = view.px(q);
            rect.min + vec2(x, y)
        };
        if response.drag_started_by(PointerButton::Primary)
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
            && let Some(k) = corners(lo, hi).iter().position(|c| at(*c).distance(o) <= 8.0)
        {
            self.image_grip = Some((id, k));
        }
        let Some((held, k)) = self.image_grip else { return };
        if held != id {
            self.image_grip = None;
            return;
        }
        if response.drag_stopped() {
            self.image_grip = None;
            if let Some(q) = ui.input(|i| i.pointer.latest_pos()) {
                let to = view.mm((q.x - rect.min.x, q.y - rect.min.y));
                let aspect = (hi.0 - lo.0) / (hi.1 - lo.1);
                if let Some((width, mid)) = resized(lo, hi, k, to, aspect) {
                    self.run("modify", json!({"id": id, "width": width, "at": [mid.0, mid.1]}));
                }
            }
        }
    }

    /// The selected image's corner grips, and while one is dragged, the outline it would take.
    pub(crate) fn image_grip_overlay(&self, ui: &Ui, p: &Painter, rect: Rect, view: &bimcraft_core::render::View) {
        let Some((id, lo, hi)) = self.image_gripped() else { return };
        let at = |q: P2| {
            let (x, y) = view.px(q);
            rect.min + vec2(x, y)
        };
        let (mut lo2, mut hi2) = (lo, hi);
        let live = self.image_grip.filter(|(held, _)| *held == id).map(|(_, k)| k);
        if let (Some(k), Some(q)) = (live, ui.input(|i| i.pointer.latest_pos())) {
            let to = view.mm((q.x - rect.min.x, q.y - rect.min.y));
            if let Some((w, mid)) = resized(lo, hi, k, to, (hi.0 - lo.0) / (hi.1 - lo.1)) {
                let h = w * (hi.1 - lo.1) / (hi.0 - lo.0);
                (lo2, hi2) = (P2(mid.0 - w / 2.0, mid.1 - h / 2.0), P2(mid.0 + w / 2.0, mid.1 + h / 2.0));
                let c = corners(lo2, hi2).map(at);
                for i in 0..4 {
                    p.line_segment([c[i], c[(i + 1) % 4]], Stroke::new(1.5, pal().accent));
                }
            }
        }
        for (i, c) in corners(lo2, hi2).into_iter().enumerate() {
            let r = Rect::from_center_size(at(c), vec2(9.0, 9.0));
            let held = live == Some(i);
            p.rect_filled(r, 1.0, if held { pal().accent } else { canvas_pal().surface });
            p.rect_stroke(r, 1.0, Stroke::new(1.5, pal().accent), StrokeKind::Middle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_corner_dragged_resizes_about_the_one_across_keeping_the_aspect() {
        let (lo, hi) = (P2(0.0, 0.0), P2(4000.0, 2000.0));
        // The upper right corner dragged out to (8000, 3000): twice as wide, the lower left staying.
        let (w, mid) = resized(lo, hi, 2, P2(8000.0, 3000.0), 2.0).unwrap();
        assert_eq!((w, mid), (8000.0, P2(4000.0, 2000.0)));
        // Dragged up past its aspect: as tall as the drag, as wide as that makes it.
        let (w, _) = resized(lo, hi, 2, P2(4100.0, 5000.0), 2.0).unwrap();
        assert_eq!(w, 10000.0);
        // The lower left corner dragged in: the upper right stays.
        let (w, mid) = resized(lo, hi, 0, P2(2000.0, 1000.0), 2.0).unwrap();
        assert_eq!((w, mid), (2000.0, P2(3000.0, 1500.0)));
        assert!(resized(lo, hi, 0, P2(4000.0, 2000.0), 2.0).is_none(), "nothing left of it");
    }
}
