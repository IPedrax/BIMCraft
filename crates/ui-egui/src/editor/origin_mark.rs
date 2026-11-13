//! The family's insertion point in its views (O1): a small mark where the planes that define its origin cross, in {3D}, its
//! plans and its elevations, as an overlay drawn through the view's own `project` (a point of the family's axes to where it
//! shows on screen), so it keeps its size whatever the zoom. Defines Origin on a plane's Properties moves it; an adaptive
//! family, whose points place it, has none.

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
