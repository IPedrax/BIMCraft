//! What a pick shows over the plan, in the canvas's points: the points picked so far and the segment to the pointer, the guides that
//! hold the point (a dashed run through it, and a square on the reference it lines up with), and the glyph of what it landed on. After
//! egui's classic look: a square for an end, a triangle for a middle, a cross for a crossing, a circle for a centre.

use bimcraft_core::alignment::{AlignmentAxis, AlignmentResult};
use bimcraft_core::geom::P2;
use bimcraft_core::render::View;
use bimcraft_core::snap::Kind;
use rune::{Color, LineCap, Shape};

use super::{Painted, Palette, dashes, points, reaches};
use crate::snapping::Landing;

/// How far a guide's dashes run behind the point it holds and past it, points.
const BEHIND: f32 = 24.0;
const PAST: f32 = 100.0;
/// A snap's own angle guide runs this far past the point, points; back to the point it runs from, and a little more.
const PAST_ANGLE: f32 = 28.0;
const BEHIND_ANGLE: f32 = 24.0;
/// The most a guide from the last point is drawn back, points: far off it cannot be seen, and a dash for every ten would be made.
const MOST_BACK: f32 = 4000.0;

/// Strokes and fills in one colour at a time, counted.
struct Pen {
    shape: Shape,
    pieces: usize,
    color: Color,
    width: f32,
}

impl Pen {
    fn new(color: Color, width: f32) -> Pen {
        Pen { shape: Shape::new(), pieces: 0, color, width }
    }

    fn line(&mut self, pts: &[(f32, f32)], closed: bool) {
        self.shape = std::mem::take(&mut self.shape).stroke_with(points(pts), closed, self.width, LineCap::Butt, self.color);
        self.pieces += 1;
    }

    fn ring(&mut self, c: (f32, f32), r: f32) {
        self.line(&circle(c, r), true);
    }

    fn dot(&mut self, c: (f32, f32), r: f32) {
        self.shape = std::mem::take(&mut self.shape).fill(points(&circle(c, r)), self.color);
        self.pieces += 1;
    }

    /// A dashed run, six on and four off.
    fn dashed(&mut self, run: &[(f32, f32)]) {
        for d in dashes(run, false, &[6.0, 4.0]) {
            self.line(&d, false);
        }
    }

    fn done(self) -> Painted {
        Painted { shape: self.shape, pieces: self.pieces, skipped: 0 }
    }
}

fn circle((x, y): (f32, f32), r: f32) -> Vec<(f32, f32)> {
    (0..16)
        .map(|k| {
            let a = f32::from(k as u8) * std::f32::consts::TAU / 16.0;
            (x + r * a.cos(), y + r * a.sin())
        })
        .collect()
}

fn finite((x, y): (f32, f32)) -> bool {
    x.is_finite() && y.is_finite()
}

/// The glyph of what a point landed on, centred on `at`. A place that is no number draws nothing.
fn glyph(pen: &mut Pen, kind: Kind, at: (f32, f32)) {
    if !finite(at) {
        return;
    }
    let (x, y) = at;
    let p = |dx: f32, dy: f32| (x + dx, y + dy);
    match kind {
        Kind::Endpoint => pen.line(&[p(-5.5, -5.5), p(5.5, -5.5), p(5.5, 5.5), p(-5.5, 5.5)], true),
        Kind::Midpoint => pen.line(&[p(0.0, -6.0), p(6.0, 5.0), p(-6.0, 5.0)], true),
        Kind::Intersection => {
            pen.line(&[p(-6.0, -6.0), p(6.0, 6.0)], false);
            pen.line(&[p(-6.0, 6.0), p(6.0, -6.0)], false);
        }
        Kind::Centre => pen.ring(at, 6.0),
        // A right angle's foot: a line, and one standing square on it.
        Kind::Perpendicular => {
            pen.line(&[p(-7.0, 5.0), p(7.0, 5.0)], false);
            pen.line(&[p(0.0, 5.0), p(0.0, -6.0)], false);
        }
        Kind::Nearest => pen.line(&[p(0.0, -6.0), p(6.0, 0.0), p(0.0, 6.0), p(-6.0, 0.0)], true),
        // A scan's point: an upright cross round a dot.
        Kind::Point => {
            pen.line(&[p(-7.0, 0.0), p(7.0, 0.0)], false);
            pen.line(&[p(0.0, -7.0), p(0.0, 7.0)], false);
            pen.dot(at, 2.5);
        }
        // A quarter of a round, its corner on the point.
        Kind::Quadrant => {
            let arc: Vec<(f32, f32)> = (0..=8)
                .map(|k| f32::from(k as u8) * std::f32::consts::FRAC_PI_2 / 8.0)
                .map(|a| p(-9.0 + 12.0 * a.cos(), 6.0 - 12.0 * a.sin()))
                .collect();
            pen.line(&arc, false);
            pen.line(&[p(-9.0, 6.0), p(3.0, 6.0)], false);
            pen.line(&[p(-9.0, 6.0), p(-9.0, -6.0)], false);
        }
        // A round, and the line touching it on top.
        Kind::Tangent => {
            pen.ring(p(0.0, 5.0), 5.0);
            pen.line(&[p(-8.0, 0.0), p(8.0, 0.0)], false);
        }
        Kind::Anchor => {
            pen.ring(at, 6.0);
            pen.dot(at, 2.0);
        }
        Kind::Angle => pen.dot(at, 3.0),
        Kind::Free => pen.line(&[p(-4.0, -4.0), p(4.0, -4.0), p(4.0, 4.0), p(-4.0, 4.0)], true),
    }
}

/// The segments a chain of points is drawn as, those that reach the canvas.
fn chain(pen: &mut Pen, pts: &[(f32, f32)], size: (f32, f32)) {
    for w in pts.windows(2) {
        if reaches(w, size, 2.0) {
            pen.line(w, false);
        }
    }
}

/// A guide through the point it holds: a dashed run from behind the point to past it, along the line it follows, and, unless it is a
/// direction borrowed from a line, a square on the reference point it lines up with.
fn guide(pen: &mut Pen, res: &AlignmentResult, view: &View, pal: &Palette) {
    let (at, source) = (view.px(res.snapped_point), view.px(res.reference_point));
    let (dx, dy) = view.along(res.axis.direction());
    let len = dx.hypot(dy);
    let d = if len.is_finite() && len > 1e-9 { ((dx / len) as f32, (dy / len) as f32) } else { (1.0, 0.0) };
    if !finite(at) || !finite(source) {
        return;
    }
    pen.color = pal.guide;
    pen.width = 1.0;
    pen.dashed(&[(at.0 - d.0 * BEHIND, at.1 - d.1 * BEHIND), (at.0 + d.0 * PAST, at.1 + d.1 * PAST)]);
    if !matches!(res.axis, AlignmentAxis::Inferred { .. }) {
        pen.color = pal.anchor;
        pen.width = 1.5;
        pen.line(
            &[
                (source.0 - 3.5, source.1 - 3.5),
                (source.0 + 3.5, source.1 - 3.5),
                (source.0 + 3.5, source.1 + 3.5),
                (source.0 - 3.5, source.1 + 3.5),
            ],
            true,
        );
    }
}

/// A snap's own angle guide: dashed from the point it runs from, on past the point that landed on it.
fn angle_guide(pen: &mut Pen, from: P2, at: (f32, f32), view: &View, pal: &Palette) {
    let a = view.px(from);
    if !finite(a) || !finite(at) {
        return;
    }
    let reach = (at.0 - a.0).hypot(at.1 - a.1);
    if reach <= 1.0 {
        return;
    }
    let u = ((at.0 - a.0) / reach, (at.1 - a.1) / reach);
    let back = reach.min(MOST_BACK) + BEHIND_ANGLE;
    pen.color = pal.accent;
    pen.width = 1.0;
    pen.dashed(&[(at.0 - u.0 * back, at.1 - u.1 * back), (at.0 + u.0 * PAST_ANGLE, at.1 + u.1 * PAST_ANGLE)]);
}

/// Everything a pick shows: the points `picked` so far with the segment on to the landing, the guides, and the glyph of what it landed
/// on, for the plan seen through `view`.
pub fn over(picked: &[P2], landing: Option<&Landing>, view: &View, pal: &Palette) -> Painted {
    let mut pen = Pen::new(pal.accent, 1.5);
    let size = (view.width as f32, view.height as f32);
    let mut pts: Vec<(f32, f32)> = picked.iter().map(|p| view.px(*p)).filter(|p| finite(*p)).collect();
    for p in &pts {
        if reaches(&[*p], size, 4.0) {
            pen.dot(*p, 2.5);
        }
    }
    if let Some(l) = landing {
        let at = view.px(l.at);
        if finite(at) {
            pts.push(at);
        }
    }
    chain(&mut pen, &pts, size);
    let Some(l) = landing else { return pen.done() };
    let at = view.px(l.at);
    if let Some(g) = l.found.guide {
        angle_guide(&mut pen, g.from, at, view, pal);
    }
    for res in &l.guides {
        guide(&mut pen, res, view, pal);
    }
    pen.color = pal.accent;
    pen.width = 1.5;
    glyph(&mut pen, l.found.kind, at);
    pen.done()
}

#[cfg(test)]
mod tests {
    use bimcraft_core::alignment::{GuideLine, Inference, ReferenceType};
    use bimcraft_core::snap::Found;

    use super::*;

    fn view() -> View {
        View { scale: 0.1, cx: 0.0, cy: 0.0, width: 800, height: 600, angle: 0.0 }
    }

    fn landing(kind: Kind, at: P2) -> Landing {
        Landing { found: Found { at, kind, guide: None, source: None, face: None }, guides: Vec::new(), at }
    }

    fn pieces(kind: Kind, at: (f32, f32)) -> usize {
        let mut pen = Pen::new(Palette::dark().accent, 1.5);
        glyph(&mut pen, kind, at);
        pen.pieces
    }

    #[test]
    fn every_kind_of_snap_has_a_glyph_and_a_place_that_is_no_number_has_none() {
        let kinds = [
            Kind::Endpoint,
            Kind::Midpoint,
            Kind::Intersection,
            Kind::Centre,
            Kind::Perpendicular,
            Kind::Quadrant,
            Kind::Tangent,
            Kind::Nearest,
            Kind::Point,
            Kind::Anchor,
            Kind::Angle,
            Kind::Free,
        ];
        for kind in kinds {
            assert!(pieces(kind, (100.0, 100.0)) > 0, "{kind:?}");
            assert_eq!(pieces(kind, (f32::NAN, 1.0)), 0, "{kind:?}");
        }
        assert_eq!(pieces(Kind::Intersection, (0.0, 0.0)), 2, "a cross is two strokes");
        assert_eq!(pieces(Kind::Endpoint, (0.0, 0.0)), 1, "a square is one closed stroke");
    }

    #[test]
    fn a_pick_with_nothing_picked_and_nowhere_to_land_draws_nothing() {
        assert_eq!(over(&[], None, &view(), &Palette::dark()).pieces, 0);
    }

    #[test]
    fn the_points_picked_are_dots_joined_by_segments_on_to_the_landing() {
        let (v, pal) = (view(), Palette::dark());
        let picked = [P2(0.0, 0.0), P2(1000.0, 0.0)];
        let alone = over(&picked, None, &v, &pal);
        assert_eq!(alone.pieces, 3, "two dots and the segment between them");
        let l = landing(Kind::Free, P2(1000.0, 1000.0));
        let with = over(&picked, Some(&l), &v, &pal);
        assert_eq!(with.pieces, alone.pieces + 2, "the segment on to the landing, and its glyph");
    }

    #[test]
    fn a_segment_that_cannot_reach_the_canvas_is_left_out_and_a_hostile_point_draws_nothing() {
        let (v, pal) = (view(), Palette::dark());
        let far = over(&[P2(1e9, 1e9), P2(2e9, 1e9)], None, &v, &pal);
        assert_eq!(far.pieces, 0, "off the canvas: no dot, no segment");
        let bad = over(&[P2(f64::NAN, 0.0)], Some(&landing(Kind::Free, P2(f64::INFINITY, 0.0))), &v, &pal);
        assert_eq!(bad.pieces, 0, "{}", bad.pieces);
    }

    #[test]
    fn a_guide_is_a_dashed_run_and_a_square_on_what_it_lines_up_with_but_a_borrowed_direction_has_no_square() {
        let (v, pal) = (view(), Palette::dark());
        let result = |axis| AlignmentResult {
            snapped_point: P2(100.0, 200.0),
            guide_line: GuideLine { from: P2(100.0, 0.0), to: P2(100.0, 200.0) },
            reference_point: P2(100.0, 0.0),
            axis,
            kind: ReferenceType::Endpoint,
            wall_id: None,
        };
        let mut l = landing(Kind::Free, P2(100.0, 200.0));
        let bare = over(&[], Some(&l), &v, &pal).pieces;
        l.guides = vec![result(AlignmentAxis::X)];
        let aligned = over(&[], Some(&l), &v, &pal).pieces;
        assert!(aligned > bare + 2, "dashes and the square: {aligned} against {bare}");
        l.guides = vec![result(AlignmentAxis::Inferred { along: P2(0.0, 1.0), of: Inference::Parallel })];
        let borrowed = over(&[], Some(&l), &v, &pal).pieces;
        assert_eq!(borrowed + 1, aligned, "the same dashes, no square on a point of the line's own");
    }

    #[test]
    fn a_snaps_own_angle_guide_runs_from_the_point_it_comes_from() {
        let (v, pal) = (view(), Palette::dark());
        let mut l = landing(Kind::Angle, P2(1000.0, 0.0));
        let bare = over(&[], Some(&l), &v, &pal).pieces;
        l.found.guide = Some(bimcraft_core::snap::Guide { from: P2(0.0, 0.0), angle: 0.0, corner: None });
        assert!(over(&[], Some(&l), &v, &pal).pieces > bare, "dashes along the guide");
    }
}
