//! The grips of the element selected alone, in the canvas's points: a square at each end and, while an end is dragged, the line the
//! element would take with that end filled. After the egui window's `grip_overlay`, which also draws the element's outline as it
//! would stand and a ruler with graduations; here the line and the reading beside the end (`App::grip_reading`).

use bimcraft_core::geom::P2;
use bimcraft_core::render::View;
use rune::{LineCap, Shape};

use super::{Painted, Palette, points, reaches};

/// Half a grip's side, points: nine across.
const HALF: f32 = 4.5;
const WIDTH: f32 = 1.5;

/// The grips at `ends` seen through `view`; `live` is the index of the one held. A grip off the canvas or that is no number is left out.
pub fn handles(ends: [P2; 2], live: Option<usize>, view: &View, pal: &Palette) -> Painted {
    let size = (view.width as f32, view.height as f32);
    let px: Vec<(f32, f32)> = ends.iter().map(|p| view.px(*p)).collect();
    let (mut shape, mut pieces) = (Shape::new(), 0);
    if px.iter().any(|(x, y)| !x.is_finite() || !y.is_finite()) {
        return Painted { shape, pieces, skipped: 0 };
    }
    if live.is_some() {
        shape = shape.stroke_with(points(&px), false, WIDTH, LineCap::Butt, pal.accent);
        pieces += 1;
    }
    for (k, &(x, y)) in px.iter().enumerate() {
        let square = [(x - HALF, y - HALF), (x + HALF, y - HALF), (x + HALF, y + HALF), (x - HALF, y + HALF)];
        if !reaches(&square, size, 0.0) {
            continue;
        }
        shape = shape.fill(points(&square), if live == Some(k) { pal.accent } else { pal.paper });
        shape = shape.stroke_with(points(&square), true, WIDTH, LineCap::Butt, pal.accent);
        pieces += 2;
    }
    Painted { shape, pieces, skipped: 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> View {
        View { scale: 0.1, cx: 0.0, cy: 0.0, width: 800, height: 600, angle: 0.0 }
    }

    #[test]
    fn a_square_for_each_end_in_view_and_while_one_is_held_the_line_between() {
        let (v, pal) = (view(), Palette::dark());
        let ends = [P2(-1000.0, 0.0), P2(1000.0, 0.0)];
        assert_eq!(handles(ends, None, &v, &pal).pieces, 4, "two squares, each filled and outlined");
        assert_eq!(handles(ends, Some(1), &v, &pal).pieces, 5, "and the line the held end draws");
        assert_eq!(handles([P2(-1000.0, 0.0), P2(1e9, 1e9)], None, &v, &pal).pieces, 2, "an end off the canvas has no square");
        assert_eq!(handles([P2(f64::NAN, 0.0), P2(0.0, 0.0)], Some(0), &v, &pal).pieces, 0, "an end that is no number draws nothing");
    }
}
