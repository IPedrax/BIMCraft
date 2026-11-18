//! Revit's zoom commands, for any view whatever its camera: Zoom In Region (the rectangle dragged fitted to the view), Zoom Out
//! (2x) about the view's middle, and Previous Pan/Zoom, a short history of what the commands left. A 3D view's camera is the
//! core's `view3d::Camera`; a 2D view's is the scale and middle of a `render::View`. The family document's views use them
//! (`editor::zoom`).
use bimcraft_core::geom::P2;
use bimcraft_core::render::View;
use bimcraft_core::view3d::Camera;
use egui::{CursorIcon, Id, Pos2, Rect, Sense, Ui};

/// Zoom Out (2x): half as close.
pub(crate) const OUT: f64 = 0.5;

/// The steps Previous Pan/Zoom keeps of each view.
pub(crate) const STEPS: usize = 20;

/// A rectangle smaller than this, points across, is a click rather than a region.
const LEAST: f32 = 4.0;

/// How much closer the rectangle from pixel `a` to pixel `b` has to come to fill a picture `width` by `height` as far as it can
/// without being cut, and the pixel at its middle; none for a rectangle too small to be one.
fn fill(a: (f64, f64), b: (f64, f64), width: f64, height: f64) -> Option<(f64, (f64, f64))> {
    let (dx, dy) = ((b.0 - a.0).abs(), (b.1 - a.1).abs());
    let f = (width / dx.max(1e-9)).min(height / dy.max(1e-9));
    (f.is_finite() && f > 0.0 && dx.max(dy) >= f64::from(LEAST)).then_some((f, ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)))
}

/// A 3D camera after Zoom In Region over the rectangle from pixel `a` to pixel `b` of its picture: its middle brought to the
/// picture's, and as close as fills the picture with it.
pub(crate) fn region_3d(cam: Camera, a: (f64, f64), b: (f64, f64)) -> Option<Camera> {
    let (w, h) = (f64::from(cam.width), f64::from(cam.height));
    let (f, m) = fill(a, b, w, h)?;
    let centre = (w / 2.0, h / 2.0);
    Some(cam.pan(m, centre).zoom(f, centre))
}

/// A 3D camera after Zoom Out (2x), about its picture's middle, as the project's navigation bar zooms its 3D view.
pub(crate) fn out_3d(cam: Camera) -> Camera {
    cam.zoom(OUT, (f64::from(cam.width) / 2.0, f64::from(cam.height) / 2.0))
}

/// A 2D view after Zoom In Region over the rectangle from pixel `a` to pixel `b`: its scale (pixels a mm) and its middle (mm).
pub(crate) fn region_2d(view: &View, a: (f32, f32), b: (f32, f32)) -> Option<(f64, P2)> {
    let wide = |p: (f32, f32)| (f64::from(p.0), f64::from(p.1));
    let (f, m) = fill(wide(a), wide(b), f64::from(view.width), f64::from(view.height))?;
    Some((view.scale * f, view.mm((m.0 as f32, m.1 as f32))))
}

/// What Previous Pan/Zoom steps back through in one view: how it was looked at before each zoom command, newest last.
#[derive(Clone, Debug)]
pub(crate) struct Previous<T> {
    steps: Vec<T>,
}

impl<T> Default for Previous<T> {
    fn default() -> Self {
        Previous { steps: Vec::new() }
    }
}

impl<T: PartialEq> Previous<T> {
    /// How the view was before a zoom command: a step, unless it is the last one already. The oldest go past [`STEPS`].
    pub(crate) fn push(&mut self, was: T) {
        if self.steps.last() != Some(&was) {
            self.steps.push(was);
        }
        if self.steps.len() > STEPS {
            self.steps.remove(0);
        }
    }

    /// The last step, taken: how the view goes back.
    pub(crate) fn pop(&mut self) -> Option<T> {
        self.steps.pop()
    }
}

/// Zoom In Region's drag over `pane`, while it is armed: `from` holds where the drag began. Over the view's own interaction (call
/// it after the view is drawn) so the drag neither turns, pans nor selects; the rectangle drawn as a window selection's. The
/// rectangle once let go, in points.
pub(crate) fn drag(ui: &Ui, pane: Rect, from: &mut Option<Pos2>) -> Option<Rect> {
    let response = ui.interact(pane, Id::new("zoom-in-region"), Sense::drag());
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
    }
    if response.drag_started_by(egui::PointerButton::Primary) {
        *from = ui.input(|i| i.pointer.press_origin());
    }
    let at = ui.input(|i| i.pointer.latest_pos());
    if response.drag_stopped() {
        return from.take().zip(at).map(|(a, b)| Rect::from_two_pos(a, b));
    }
    if let (Some(a), Some(b)) = (*from, at) {
        // Drawn left to right whichever way it goes: a region has no crossing.
        let r = Rect::from_two_pos(a, b);
        let local = |p: Pos2| (p.x - pane.min.x, p.y - pane.min.y);
        ui.painter_at(pane).extend(crate::canvas::box_shapes(pane, local(r.min), local(r.max)));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_region_fills_the_picture_as_far_as_it_can_about_its_middle() {
        let view = View { scale: 2.0, cx: 0.0, cy: 0.0, width: 400, height: 200, angle: 0.0 };
        // 100 by 100 pixels at the picture's top left quarter: its height fills the picture's, twice as close.
        let (scale, middle) = region_2d(&view, (50.0, 0.0), (150.0, 100.0)).unwrap();
        assert!((scale - 4.0).abs() < 1e-9);
        assert_eq!(middle, view.mm((100.0, 50.0)));
        assert!(region_2d(&view, (10.0, 10.0), (11.0, 12.0)).is_none(), "a click");
    }

    #[test]
    fn previous_keeps_each_step_once_and_the_last_ones() {
        let mut p = Previous::default();
        for i in 0..STEPS + 5 {
            p.push(i);
            p.push(i);
        }
        assert_eq!(p.pop(), Some(STEPS + 4));
        assert_eq!(p.steps.len(), STEPS - 1);
        assert_eq!(p.steps[0], 5);
    }
}
