//! A family's connectors in its views, as Revit's family editor shows them: a small symbol where each is (a circle for a pipe's,
//! a square for a power, switch or data connector's, an earthing mark for an earthing one's) and an arrow the way it faces, in
//! {3D}, its plans and its elevations, lit when selected or under the pointer, and picked by a click.
//!
//! An overlay over the view's picture, drawn and picked through `project`: a point of the family's axes to where it shows on
//! screen (none, behind the eye). The symbols keep their size on screen whatever the zoom, so a view hands over its camera and
//! nothing else, and the {3D} preview or the project's 3D view can draw them alike.

use bimcraft_core::family::ConnectorKind;
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, vec2};

use super::FamilyEditor;

/// A connector where the type previewed puts it: its place in the list, its kind, where it is and which way it faces, in the
/// family's axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Mark {
    pub(crate) index: usize,
    pub(crate) kind: ConnectorKind,
    pub(crate) at: [f64; 3],
    pub(crate) direction: Option<[f64; 3]>,
}

/// Half a symbol's width on screen, points.
const R: f32 = 7.0;
/// How far its arrow reaches past the symbol, points.
const ARROW: f32 = 22.0;
/// How near a click comes to a symbol's middle to pick it, points.
pub(crate) const REACH: f32 = 11.0;
/// An unlit connector's colour: an amber apart from the selection's and the hover's blues and the planes' green, which reads
/// on a light canvas and a dark one alike.
const INK: Color32 = Color32::from_rgb(196, 98, 16);

/// A point of the family's axes to where it shows on screen; none, behind the eye.
pub(crate) type Project<'a> = &'a dyn Fn([f64; 3]) -> Option<Pos2>;

impl FamilyEditor {
    /// The connectors that have a place, where the type previewed (and its instance values) puts them; none in an adaptive
    /// family's views, or while the family doesn't work out.
    pub(crate) fn connector_marks(&self) -> Vec<Mark> {
        if self.adaptive.is_some() || self.connectors.is_empty() {
            return Vec::new();
        }
        let (Ok(f), Some((ty, _))) = (self.family(), self.types.get(self.picked)) else { return Vec::new() };
        let Ok(values) = f.connector_values(ty, &self.preview_params) else { return Vec::new() };
        values
            .into_iter()
            .enumerate()
            .filter_map(|(index, c)| Some(Mark { index, kind: c.kind, at: c.at?, direction: c.direction }))
            .filter(|m| m.at.iter().all(|v| v.is_finite()))
            .collect()
    }
}

/// The connector a click at `at` picks: the nearest symbol within [`REACH`], the last drawn first where two lie together.
pub(crate) fn pick(marks: &[Mark], project: Project, at: Pos2) -> Option<usize> {
    marks
        .iter()
        .rev()
        .filter_map(|m| project(m.at).map(|p| (m.index, p.distance(at))))
        .filter(|(_, d)| *d <= REACH)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// The way `m` faces on screen, a unit vector from its middle `c`; none where it has no direction or faces the eye.
fn facing(m: &Mark, project: Project, c: Pos2) -> Option<Vec2> {
    let d = m.direction?;
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if !len.is_finite() || len <= 1e-9 {
        return None;
    }
    // A short step, so a perspective view bends it no more than at its middle.
    let step = 10.0 / len;
    let tip = project([m.at[0] + d[0] * step, m.at[1] + d[1] * step, m.at[2] + d[2] * step])?;
    let v = tip - c;
    (v.length() > 1e-3).then(|| v.normalized())
}

/// Each of `marks` drawn: its symbol and its arrow, `selected` in `lit.0` and `hovered` in `lit.1`, thicker; the rest in the
/// connectors' ink.
pub(crate) fn paint(
    painter: &Painter,
    marks: &[Mark],
    project: Project,
    selected: Option<usize>,
    hovered: Option<usize>,
    lit: (Color32, Color32),
) {
    for m in marks {
        let Some(c) = project(m.at) else { continue };
        let (colour, width) = if Some(m.index) == selected {
            (lit.0, 2.5)
        } else if Some(m.index) == hovered {
            (lit.1, 2.5)
        } else {
            (INK, 1.5)
        };
        let stroke = Stroke::new(width, colour);
        // A backing the canvas's own colour shows through, so the symbol reads over a form's faces and edges.
        let back = Color32::from_white_alpha(150);
        match m.kind {
            ConnectorKind::Pipe => {
                painter.circle(c, R, back, stroke);
            }
            ConnectorKind::Power | ConnectorKind::Switch | ConnectorKind::Data => {
                let r = Rect::from_center_size(c, vec2(2.0 * R, 2.0 * R));
                painter.rect(r, 0.0, back, stroke, egui::StrokeKind::Middle);
                // What it carries, inside: a power connector's dot, a switch's stroke, a data connector's two.
                match m.kind {
                    ConnectorKind::Power => {
                        painter.circle_filled(c, 2.0, colour);
                    }
                    ConnectorKind::Switch => {
                        painter.line_segment([c + vec2(-R * 0.5, R * 0.5), c + vec2(R * 0.5, -R * 0.5)], stroke);
                    }
                    _ => {
                        for dy in [-2.5, 2.5] {
                            painter.line_segment([c + vec2(-R * 0.55, dy), c + vec2(R * 0.55, dy)], Stroke::new(1.2, colour));
                        }
                    }
                }
            }
            ConnectorKind::Earthing => {
                // The earthing sign: a stem down to three bars, each shorter.
                painter.circle(c, R, back, Stroke::NONE);
                painter.line_segment([c + vec2(0.0, -R), c], stroke);
                for (k, half) in [R, R * 0.62, R * 0.25].into_iter().enumerate() {
                    let y = k as f32 * 3.5;
                    painter.line_segment([c + vec2(-half, y), c + vec2(half, y)], stroke);
                }
            }
        }
        if let Some(u) = facing(m, project, c) {
            let (from, tip) = (c + u * R, c + u * (R + ARROW));
            painter.line_segment([from, tip], stroke);
            let side = vec2(-u.y, u.x);
            let head = vec![tip, tip - u * 7.0 + side * 4.0, tip - u * 7.0 - side * 4.0];
            painter.add(Shape::convex_polygon(head, colour, Stroke::NONE));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::epaint::Shape as S;

    const SELECTED: Color32 = Color32::from_rgb(200, 0, 0);
    const HOVERED: Color32 = Color32::from_rgb(0, 200, 0);

    /// A pipe connector facing +x at the origin and a power one 100 east, drawn with `selected` and `hovered`: each
    /// symbol's outline colour (the pipe's circle, the power's square) and whether an arrow was drawn.
    fn drawn(selected: Option<usize>, hovered: Option<usize>) -> (Color32, Color32, bool) {
        let marks = [
            Mark { index: 0, kind: ConnectorKind::Pipe, at: [0.0, 0.0, 0.0], direction: Some([1.0, 0.0, 0.0]) },
            Mark { index: 1, kind: ConnectorKind::Power, at: [100.0, 0.0, 0.0], direction: None },
        ];
        let project = |q: [f64; 3]| Some(Pos2::new(q[0] as f32 + 50.0, 50.0));
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let p = ui.ctx().layer_painter(egui::LayerId::background());
            paint(&p, &marks, &project, selected, hovered, (SELECTED, HOVERED));
        });
        // The font atlas it uploads is no test's concern.
        out.textures_delta.clear();
        let (mut circle, mut square, mut arrow) = (Color32::TRANSPARENT, Color32::TRANSPARENT, false);
        for c in out.shapes {
            match c.shape {
                S::Circle(x) if x.stroke.width > 0.0 => circle = x.stroke.color,
                S::Rect(x) => square = x.stroke.color,
                S::Path(_) => arrow = true,
                _ => {}
            }
        }
        (circle, square, arrow)
    }

    #[test]
    fn the_selected_connector_is_drawn_in_the_selections_colour_the_hovered_in_the_hovers_the_rest_in_ink() {
        assert_eq!(drawn(None, None), (INK, INK, true), "a pipe's circle with its arrow, a power connector's square");
        assert_eq!(drawn(Some(0), Some(1)), (SELECTED, HOVERED, true));
        assert_eq!(drawn(Some(1), None), (INK, SELECTED, true));
    }

    #[test]
    fn a_click_picks_the_symbol_within_reach_and_nothing_further() {
        let marks = [
            Mark { index: 0, kind: ConnectorKind::Pipe, at: [0.0, 0.0, 0.0], direction: None },
            Mark { index: 3, kind: ConnectorKind::Earthing, at: [40.0, 0.0, 0.0], direction: None },
        ];
        let project = |q: [f64; 3]| Some(Pos2::new(q[0] as f32, 0.0));
        assert_eq!(pick(&marks, &project, Pos2::new(3.0, 4.0)), Some(0));
        assert_eq!(pick(&marks, &project, Pos2::new(36.0, 0.0)), Some(3), "the nearer");
        assert_eq!(pick(&marks, &project, Pos2::new(20.0, 0.0)), None, "beyond both symbols' reach");
        assert_eq!(pick(&marks, &|_| None, Pos2::new(0.0, 0.0)), None, "behind the eye");
    }
}
