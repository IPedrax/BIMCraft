//! What the next click would make, drawn over the plan: the curves a Draw panel shape makes of the points picked and the pointer, as
//! thin strokes in the accent colour, each as the core samples it (a closed one comes back to its start). In the canvas's points.

use bimcraft_core::curves::Curve;
use bimcraft_core::render::View;
use rune::{LineCap, Shape};

use super::{Painted, Palette, points, reaches};

/// How thick a ghost is drawn, points.
const WIDTH: f32 = 1.5;

/// The strokes of `curves` seen through `view`; one that cannot reach the canvas is left out (`draw::preview` makes no curve of a point
/// that is no number).
pub fn ghost(curves: &[Curve], view: &View, pal: &Palette) -> Painted {
    let size = (view.width as f32, view.height as f32);
    let (mut shape, mut pieces) = (Shape::new(), 0);
    for c in curves {
        let pts: Vec<(f32, f32)> = c.points().into_iter().map(|p| view.px(p)).collect();
        if pts.len() < 2 || !reaches(&pts, size, 2.0) {
            continue;
        }
        shape = shape.stroke_with(points(&pts), false, WIDTH, LineCap::Butt, pal.accent);
        pieces += 1;
    }
    Painted { shape, pieces, skipped: 0 }
}

#[cfg(test)]
mod tests {
    use bimcraft_core::geom::P2;

    use super::*;

    fn view() -> View {
        View { scale: 0.1, cx: 0.0, cy: 0.0, width: 800, height: 600, angle: 0.0 }
    }

    #[test]
    fn each_curve_is_one_stroke_and_one_that_cannot_reach_the_canvas_is_left_out() {
        let (v, pal) = (view(), Palette::dark());
        assert_eq!(ghost(&[], &v, &pal).pieces, 0);
        let line = Curve::line(P2(0.0, 0.0), P2(1000.0, 0.0));
        let ring = Curve::Circle { centre: P2(0.0, 0.0), radius: 500.0 };
        assert_eq!(ghost(&[line.clone(), ring], &v, &pal).pieces, 2);
        let far = Curve::line(P2(1e9, 1e9), P2(2e9, 1e9));
        assert_eq!(ghost(&[far, line.clone()], &v, &pal).pieces, 1, "off the canvas: not drawn");
    }
}
