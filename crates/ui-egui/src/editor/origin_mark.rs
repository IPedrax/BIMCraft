//! The family's insertion point in its views (O1): a small mark where the planes that define its origin cross. In its plans
//! and its elevations it is a ring with a cross; in {3D}, a point in space: three short axes, dropped to Ref. Level by dashed
//! lines. Each is an overlay drawn through the view's own `project` (a point of the family's axes to where it shows on
//! screen), so it keeps its size whatever the zoom. Defines Origin on a plane's Properties moves it; an adaptive family, whose
//! points place it, has none.

use bimcraft_core::model::Guid;
use egui::{Color32, Painter, Stroke, vec2};

use super::FamilyEditor;
use super::connector_marks::Project;

/// The mark's radius on screen, points; its cross reaches past it.
const R: f32 = 5.0;
const CROSS: f32 = 11.0;
/// Its ink: a crimson apart from the connectors' amber, the planes' green and the selection's blue, read on a light canvas and
/// a dark one alike over the white backing it is drawn on.
const INK: Color32 = Color32::from_rgb(200, 30, 90);

impl FamilyEditor {
    /// The insertion point in the family's axes: where its planes that define the origin cross; none in an adaptive family.
    pub(crate) fn origin_mark(&self) -> Option<[f64; 3]> {
        if self.adaptive.is_some() {
            return None;
        }
        let f = bimcraft_core::family::Family { planes: self.planes.clone(), ..Default::default() };
        Some(f.origin()).filter(|o| o.iter().all(|v| v.is_finite()))
    }

    /// Defines Origin ticked (`on`) or cleared on plane `g`: it defines the origin in its direction and no other plane does;
    /// cleared, the datum of that direction takes it back. A plane square to none of x, y and z can't.
    pub(crate) fn define_origin(&mut self, g: Guid, on: bool) {
        let Some(axis) = self.planes.iter().find(|p| p.guid == g).and_then(|p| p.direction()) else { return };
        for p in self.planes.iter_mut().filter(|p| p.direction() == Some(axis)) {
            p.defines_origin = false;
        }
        let datum = self.planes.iter().position(|p| p.datum() && p.direction() == Some(axis));
        let plane = self.planes.iter().position(|p| p.guid == g).filter(|_| on);
        if let Some(i) = plane.or(datum) {
            self.planes[i].defines_origin = true;
        }
    }
}

/// How long the {3D} mark's axes are on screen, points.
const TRIPOD: f32 = 26.0;

/// The insertion point `at` in {3D}, as a point in space: three short axes along the family's x, y and z in the view's triad
/// colours, so the view's turn shows in them, and, off Ref. Level or off the datums' crossing, thin dashed lines dropping
/// from it to Ref. Level and running there to the datums' crossing, so where it is in depth reads against what is drawn.
pub(crate) fn paint_3d(painter: &Painter, project: Project, at: [f64; 3]) {
    let Some(c) = project(at) else { return };
    // The view's millimetres to points about the point: the longest axis of a metre, on screen.
    let step = |k: usize, mm: f64| project(std::array::from_fn(|i| at[i] + if i == k { mm } else { 0.0 }));
    let metre = (0..3).filter_map(|k| step(k, 1000.0)).map(|q| (q - c).length()).fold(0.0f32, f32::max);
    let dash = Stroke::new(1.0, INK);
    let foot = [at[0], at[1], 0.0];
    if at[2] != 0.0
        && let Some(f) = project(foot)
    {
        painter.extend(egui::Shape::dashed_line(&[c, f], dash, 4.0, 3.0));
    }
    if (at[0] != 0.0 || at[1] != 0.0)
        && let (Some(f), Some(o)) = (project(foot), project([0.0; 3]))
    {
        painter.extend(egui::Shape::dashed_line(&[f, o], dash, 4.0, 3.0));
        // The datums' crossing it runs to, a dot.
        painter.circle_filled(o, 2.0, INK);
    }
    painter.circle_filled(c, 3.5, Color32::from_white_alpha(170));
    if metre > 1e-3 {
        let mm = f64::from(TRIPOD / metre) * 1000.0;
        for k in 0..3 {
            if let Some(q) = step(k, mm) {
                painter.line_segment([c, q], Stroke::new(2.0, crate::triad::axis_color(k)));
            }
        }
    }
    painter.circle_filled(c, 2.0, INK);
}

/// The insertion point `at` (the family's axes) drawn where the view shows it: a ring with a cross through it.
pub(crate) fn paint(painter: &Painter, project: Project, at: [f64; 3]) {
    let Some(c) = project(at) else { return };
    let stroke = Stroke::new(1.6, INK);
    painter.circle(c, R, Color32::from_white_alpha(150), stroke);
    for d in [vec2(CROSS, 0.0), vec2(0.0, CROSS)] {
        painter.line_segment([c - d, c + d], stroke);
    }
    painter.circle_filled(c, 1.6, INK);
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::epaint::Shape as S;

    /// The shapes `paint_3d` draws for an insertion point at `at`, seen in an oblique projection (z up the screen, y off to the
    /// upper right), 0.1 points to a millimetre.
    fn drawn_3d(at: [f64; 3]) -> Vec<S> {
        let project = |q: [f64; 3]| Some(egui::pos2((300.0 + q[0] * 0.1 + q[1] * 0.05) as f32, (300.0 - q[2] * 0.1 - q[1] * 0.05) as f32));
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let p = ui.ctx().layer_painter(egui::LayerId::background());
            paint_3d(&p, &project, at);
        });
        out.textures_delta.clear();
        out.shapes.into_iter().map(|c| c.shape).collect()
    }

    /// The colours of the solid segments drawn, in order.
    fn segments(shapes: &[S]) -> Vec<Color32> {
        shapes.iter().filter_map(|s| if let S::LineSegment { stroke, .. } = s { Some(stroke.color) } else { None }).collect()
    }

    #[test]
    fn in_3d_the_insertion_point_is_a_tripod_in_the_axis_colours_dropped_to_ref_level() {
        let shapes = drawn_3d([-400.0, 200.0, 500.0]);
        // Its three axes, x red, y green, z blue, as the view's triad colours them.
        let axes: Vec<Color32> = segments(&shapes).into_iter().filter(|c| *c != INK).collect();
        assert_eq!(axes, (0..3).map(crate::triad::axis_color).collect::<Vec<_>>());
        // Above Ref. Level, a dashed line drops to it, and runs on it to the datums' crossing: dashes are segments in its ink.
        let dashes = segments(&shapes).into_iter().filter(|c| *c == INK).count();
        assert!(dashes > 6, "{dashes} dashes");
        // On Ref. Level at the datums' crossing there is nothing to drop: the tripod alone.
        let shapes = drawn_3d([0.0; 3]);
        assert_eq!(segments(&shapes).into_iter().filter(|c| *c == INK).count(), 0);
        assert_eq!(segments(&shapes).len(), 3);
    }
}
