//! The plan canvas's camera: where the view is looking and how close, kept apart from the pixels it is shown in. The core's
//! [`View`] is what `render::draw` takes; a camera makes one for the canvas's size, and pans and zooms by moving its centre and
//! scale. Every number that comes in is a pointer's or a wheel's, so none is trusted: a camera never holds a non-finite value.

use bimcraft_core::geom::P2;
use bimcraft_core::render::View;

/// Pixels (points, in Rune) to a millimetre, at the farthest in and out: a plan of a whole country and a hair of a wall.
pub const MIN_SCALE: f64 = 1e-5;
pub const MAX_SCALE: f64 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// The model point (mm) in the middle of the canvas.
    pub cx: f64,
    pub cy: f64,
    /// Points to a millimetre.
    pub scale: f64,
    /// Degrees the model is turned on screen (a plan oriented to true north).
    pub angle: f64,
}

impl Camera {
    /// Looking where `view` looks.
    pub fn from_view(view: View) -> Camera {
        let at = |v: f64, fallback: f64| if v.is_finite() { v } else { fallback };
        Camera {
            cx: at(view.cx, 0.0),
            cy: at(view.cy, 0.0),
            scale: at(view.scale, 0.1).clamp(MIN_SCALE, MAX_SCALE),
            angle: at(view.angle, 0.0),
        }
    }

    /// The view of a canvas `width` x `height` points: never smaller than a point, so the core never draws into nothing.
    pub fn view(&self, width: f32, height: f32) -> View {
        let side = |v: f32| if v.is_finite() { v.clamp(1.0, 16384.0) as u32 } else { 1 };
        View { scale: self.scale, cx: self.cx, cy: self.cy, width: side(width), height: side(height), angle: self.angle }
    }

    /// Zooms by `factor` (above 1 closer) keeping the model point under `at` (points from the canvas's top left) where it is.
    pub fn zoom_about(&mut self, at: (f32, f32), size: (f32, f32), factor: f64) {
        if !factor.is_finite() || factor <= 0.0 || !at.0.is_finite() || !at.1.is_finite() {
            return;
        }
        let before = self.view(size.0, size.1);
        let held = before.mm(at);
        let scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        // The same view looking at the origin names how far `at` is from the centre in the model, at the new scale.
        let from_centre = View { scale, cx: 0.0, cy: 0.0, ..before }.mm(at);
        self.scale = scale;
        self.cx = held.0 - from_centre.0;
        self.cy = held.1 - from_centre.1;
    }

    /// The camera after Zoom In Region over the rectangle from `a` to `b` (points of a canvas `size`): its middle brought to the
    /// canvas's and as close as fills the canvas with it. None for a rectangle with no width or no height, or a corner that is no number.
    pub fn region(&self, a: (f32, f32), b: (f32, f32), size: (f32, f32)) -> Option<Camera> {
        if ![a.0, a.1, b.0, b.1].iter().all(|v| v.is_finite()) {
            return None;
        }
        let view = self.view(size.0, size.1);
        let (dx, dy) = (f64::from((b.0 - a.0).abs()), f64::from((b.1 - a.1).abs()));
        let f = (f64::from(view.width) / dx).min(f64::from(view.height) / dy);
        if dx <= 0.0 || dy <= 0.0 || !f.is_finite() || f <= 0.0 {
            return None;
        }
        let P2(cx, cy) = view.mm(((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0));
        if !(cx.is_finite() && cy.is_finite()) {
            return None;
        }
        Some(Camera { cx, cy, scale: (self.scale * f).clamp(MIN_SCALE, MAX_SCALE), angle: self.angle })
    }

    /// Pans so the content follows a pointer that moved `dx`, `dy` points.
    pub fn pan(&mut self, dx: f32, dy: f32, size: (f32, f32)) {
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        let view = self.view(size.0, size.1);
        let P2(x, y) = view.mm((view.width as f32 / 2.0 - dx, view.height as f32 / 2.0 - dy));
        if x.is_finite() && y.is_finite() {
            (self.cx, self.cy) = (x, y);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (f32, f32) = (800.0, 600.0);

    fn cam(angle: f64) -> Camera {
        Camera { cx: 5000.0, cy: 3500.0, scale: 0.1, angle }
    }

    fn near(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer_where_it_is() {
        for angle in [0.0, 30.0, -90.0] {
            let mut c = cam(angle);
            let at = (123.0, 456.0);
            let held = c.view(SIZE.0, SIZE.1).mm(at);
            c.zoom_about(at, SIZE, 2.5);
            assert!((c.scale - 0.25).abs() < 1e-12);
            assert!(near(c.view(SIZE.0, SIZE.1).px(held), at), "angle {angle}: the point moved");
        }
    }

    #[test]
    fn panning_moves_the_content_with_the_pointer() {
        for angle in [0.0, 45.0] {
            let mut c = cam(angle);
            let p = P2(5200.0, 3300.0);
            let before = c.view(SIZE.0, SIZE.1).px(p);
            c.pan(40.0, -25.0, SIZE);
            let after = c.view(SIZE.0, SIZE.1).px(p);
            assert!(near(after, (before.0 + 40.0, before.1 - 25.0)), "angle {angle}: {before:?} -> {after:?}");
        }
    }

    #[test]
    fn zoom_stays_in_range_and_hostile_input_changes_nothing() {
        let mut c = cam(0.0);
        c.zoom_about((0.0, 0.0), SIZE, 1e12);
        assert_eq!(c.scale, MAX_SCALE);
        c.zoom_about((0.0, 0.0), SIZE, 1e-30);
        assert_eq!(c.scale, MIN_SCALE);
        let kept = c;
        c.zoom_about((f32::NAN, 0.0), SIZE, 2.0);
        c.zoom_about((0.0, 0.0), SIZE, f64::NAN);
        c.zoom_about((0.0, 0.0), SIZE, -1.0);
        c.pan(f32::INFINITY, 0.0, SIZE);
        assert_eq!(c, kept);
    }

    #[test]
    fn a_region_fills_the_canvas_as_far_as_it_can_about_its_middle() {
        for angle in [0.0, 30.0] {
            let c = Camera { cx: 0.0, cy: 0.0, scale: 2.0, angle };
            let size = (400.0, 200.0);
            // 100 by 100 points: the canvas's height is the tighter fit, so twice as close.
            let r = c.region((50.0, 0.0), (150.0, 100.0), size).expect("a region");
            assert!((r.scale - 4.0).abs() < 1e-9, "angle {angle}: {}", r.scale);
            let middle = c.view(size.0, size.1).mm((100.0, 50.0));
            assert!(near(r.view(size.0, size.1).px(middle), (200.0, 100.0)), "the region's middle is the canvas's");
            assert_eq!(
                r.region((150.0, 100.0), (50.0, 0.0), size).map(|c| c.scale),
                r.region((50.0, 0.0), (150.0, 100.0), size).map(|c| c.scale)
            );
        }
    }

    #[test]
    fn a_region_with_no_size_or_no_number_is_none_and_zoom_stays_in_range() {
        let c = cam(0.0);
        assert!(c.region((10.0, 10.0), (60.0, 10.0), SIZE).is_none(), "no height");
        assert!(c.region((10.0, 10.0), (10.0, 90.0), SIZE).is_none(), "no width");
        assert!(c.region((f32::NAN, 0.0), (5.0, 5.0), SIZE).is_none());
        assert!(c.region((0.0, 0.0), (f32::INFINITY, 5.0), SIZE).is_none());
        let tiny = c.region((10.0, 10.0), (10.001, 10.001), SIZE).expect("a speck is a region");
        assert_eq!(tiny.scale, MAX_SCALE, "never past the farthest in");
    }

    #[test]
    fn a_canvas_with_no_size_still_makes_a_view_the_core_can_draw() {
        let v = cam(0.0).view(0.0, f32::NAN);
        assert_eq!((v.width, v.height), (1, 1));
        let c = Camera::from_view(View { scale: f64::NAN, cx: f64::INFINITY, cy: 1.0, width: 10, height: 10, angle: f64::NAN });
        assert!(c.cx.is_finite() && c.scale.is_finite() && c.angle.is_finite());
    }
}
