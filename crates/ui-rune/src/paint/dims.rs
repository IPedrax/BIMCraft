//! Where a temporary dimension is drawn, and its lines, in the canvas's points: the line parallel to what it measures and a little off
//! it, the two extension lines to it, and a tick at each end; the number goes at the line's middle (`ui/tempdim.rs`).

use bimcraft_core::render::View;
use rune::{LineCap, Shape};

use super::{Painted, Palette, points, reaches};
use crate::tempdim::Dim;

/// The shortest a dimension is drawn, points: shorter, and its number would lie over the element.
const MIN_LENGTH: f32 = 24.0;
/// Half a tick across the line's end, points.
const TICK: f32 = 4.0;
const WIDTH: f32 = 1.0;

type Pt = (f32, f32);

/// A dimension placed on the canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    /// The dimension line's two ends.
    pub line: [Pt; 2],
    /// The extension lines from what is measured to the ends of the line.
    pub feet: [[Pt; 2]; 2],
    /// Its middle: where the number goes.
    pub middle: Pt,
}

fn finite((x, y): Pt) -> bool {
    x.is_finite() && y.is_finite()
}

/// Where `dim` stands seen through `view`; none when what it measures is shorter than [`MIN_LENGTH`] on screen or any of it is no number.
pub fn place(dim: &Dim, view: &View) -> Option<Placed> {
    let (pa, pb, side) = (view.px(dim.a), view.px(dim.b), view.px(dim.a + dim.side));
    if ![pa, pb, side].iter().all(|p| finite(*p)) || (pb.0 - pa.0).hypot(pb.1 - pa.1) < MIN_LENGTH {
        return None;
    }
    let n = (side.0 - pa.0, side.1 - pa.1);
    let reach = n.0.hypot(n.1);
    if reach < 1e-9 {
        return None;
    }
    let off = (dim.off.mm * view.scale) as f32 + dim.off.px;
    let n = (n.0 / reach * off, n.1 / reach * off);
    let (a, b) = ((pa.0 + n.0, pa.1 + n.1), (pb.0 + n.0, pb.1 + n.1));
    let middle = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    (finite(a) && finite(b)).then_some(Placed { line: [a, b], feet: [[pa, a], [pb, b]], middle })
}

/// The strokes of the dimensions placed, those that reach the canvas.
pub fn lines(placed: &[Placed], view: &View, pal: &Palette) -> Painted {
    let size = (view.width as f32, view.height as f32);
    let (mut shape, mut pieces) = (Shape::new(), 0);
    let mut stroke = |pts: &[Pt]| {
        if reaches(pts, size, 2.0) {
            shape = std::mem::take(&mut shape).stroke_with(points(pts), false, WIDTH, LineCap::Butt, pal.accent);
            pieces += 1;
        }
    };
    for p in placed {
        stroke(&p.line);
        for foot in &p.feet {
            stroke(foot);
        }
        // A tick across each end of the line, square to it.
        let (dx, dy) = (p.line[1].0 - p.line[0].0, p.line[1].1 - p.line[0].1);
        let len = dx.hypot(dy);
        if len > 1e-6 {
            let n = (-dy / len * TICK, dx / len * TICK);
            for e in p.line {
                stroke(&[(e.0 - n.0, e.1 - n.1), (e.0 + n.0, e.1 + n.1)]);
            }
        }
    }
    Painted { shape, pieces, skipped: 0 }
}

#[cfg(test)]
mod tests {
    use bimcraft_core::geom::P2;

    use super::*;
    use crate::tempdim::{Edit, Off};

    fn view() -> View {
        View { scale: 0.1, cx: 0.0, cy: 0.0, width: 800, height: 600, angle: 0.0 }
    }

    fn dim(a: P2, b: P2) -> Dim {
        Dim { a, b, side: P2(0.0, 1.0), off: Off { mm: 100.0, px: 18.0 }, length: a.dist(b), edit: Edit::WallLength(1) }
    }

    #[test]
    fn a_dimension_stands_off_what_it_measures_on_its_side_and_its_number_is_at_the_middle() {
        let v = view();
        let p = place(&dim(P2(-2000.0, 0.0), P2(2000.0, 0.0)), &v).expect("placed");
        let (a, b) = (v.px(P2(-2000.0, 0.0)), v.px(P2(2000.0, 0.0)));
        // North is up on the screen: the line is above the measured segment by 100 mm at 0.1 plus 18 points.
        assert!((p.line[0].1 - (a.1 - 28.0)).abs() < 0.01 && (p.line[1].1 - (b.1 - 28.0)).abs() < 0.01, "{p:?} from {a:?}");
        assert!((p.middle.0 - (a.0 + b.0) / 2.0).abs() < 0.01 && (p.middle.1 - (a.1 - 28.0)).abs() < 0.01);
        assert_eq!(p.feet[0][0], a, "the extension line starts on what is measured");
    }

    #[test]
    fn what_is_too_short_on_screen_or_no_number_is_not_placed_and_each_placed_one_is_five_strokes() {
        let (v, pal) = (view(), Palette::dark());
        assert!(place(&dim(P2(0.0, 0.0), P2(100.0, 0.0)), &v).is_none(), "ten points is too short");
        assert!(place(&dim(P2(f64::NAN, 0.0), P2(5000.0, 0.0)), &v).is_none());
        let p = place(&dim(P2(-2000.0, 0.0), P2(2000.0, 0.0)), &v).expect("placed");
        assert_eq!(lines(&[p], &v, &pal).pieces, 5, "the line, its two extension lines and two ticks");
        assert_eq!(lines(&[], &v, &pal).pieces, 0);
        let far = place(&dim(P2(1e5, 1e5), P2(1e5 + 5000.0, 1e5)), &v).expect("placed off the canvas");
        assert_eq!(lines(&[far], &v, &pal).pieces, 0, "off the canvas nothing is sent");
    }
}
