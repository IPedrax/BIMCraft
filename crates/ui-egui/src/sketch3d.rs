//! Sketch mode in the 3D view, as Revit's Edit Boundary there: a floor, ceiling, roof or shaft selected in 3D is sketched
//! where it is, its lines drawn over the picture on the element's own plane (`Model::sketch_plane`); a new one is sketched on the
//! view's work plane (`workplane.rs`). Modify picks a line
//! where it shows and drags a picked line's end over the plane; the Draw panel's shapes, Pick Walls and the sketch's Modify
//! tools click points on it, snapped as in its level's plan. Any other drag still turns the view. The sketch belongs to the
//! 3D view as one begun in a plan belongs to the plan (`BimApp::sketch_plan`).

use bimcraft_i18n::msg;

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Id, Model};
use bimcraft_core::snap::{self, Reach};
use bimcraft_core::view3d::Camera;
use bimcraft_core::walls;
use egui::{PointerButton, Pos2, Rect, Response, Shape, Stroke, Ui, vec2};

use crate::canvas::{PICK, SNAP, click_modifiers};
use crate::sketch::{Drag, Of, Sketch};
use crate::style::pal;
use crate::{BimApp, Sheet, Status, Tool, draw};

/// How near a grip, in points, a press takes it.
const GRIP: f32 = 8.0;

/// The sketch's plane as the 3D view shows it: the camera, where the view is on screen and the pixels a point holds, and the
/// plane over the plan the lines lie on.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Plane {
    pub cam: Camera,
    pub rect: Rect,
    pub ppp: f32,
    pub surface: walls::Plane,
}

impl Plane {
    /// Where point `q` of the plane shows on screen.
    pub fn screen(&self, q: P2) -> Pos2 {
        let (x, y, _) = self.cam.project([q.0, q.1, self.surface.at(q)]);
        self.rect.min + vec2(x as f32 / self.ppp, y as f32 / self.ppp)
    }

    /// The plane's point under `at` on screen; none where the view is edge-on to it.
    pub fn at(&self, at: Pos2) -> Option<P2> {
        let px = (f64::from((at.x - self.rect.min.x) * self.ppp), f64::from((at.y - self.rect.min.y) * self.ppp));
        self.cam.on_slope(px, self.surface).map(|p| P2(p[0], p[1])).filter(|p| p.0.is_finite() && p.1.is_finite())
    }

    /// The mm `points` on screen span, as the view is scaled: a reach on the plane, foreshortening aside.
    pub fn mm(&self, points: f64) -> f64 {
        points * f64::from(self.ppp) / self.cam.scale.max(1e-12)
    }

    /// The line of `sk` that shows nearest `at`, within `PICK` points of it.
    fn line_at(&self, sk: &Sketch, at: Pos2) -> Option<usize> {
        sk.lines
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let shown: Vec<Pos2> = l.points().into_iter().map(|q| self.screen(q)).collect();
                (i, shown.windows(2).map(|w| to_segment(at, w[0], w[1])).fold(f32::INFINITY, f32::min))
            })
            .filter(|(_, d)| *d <= PICK)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// The end of a picked line of `sk` that shows within `GRIP` points of `at`: a grip.
    fn grip_at(&self, sk: &Sketch, at: Pos2) -> Option<P2> {
        sk.picked
            .iter()
            .filter_map(|&i| sk.lines.get(i))
            .flat_map(|l| l.curve.handles())
            .map(|q| (q, self.screen(q).distance(at)))
            .filter(|(_, d)| *d <= GRIP)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(q, _)| q)
    }
}

/// How far `p` is from the segment from `a` to `b`, on screen.
fn to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + ab * t)
}

impl BimApp {
    /// The plane the sketch lies on while it is sketched in the 3D view: an Edit Boundary begun there, of what still has a
    /// plane, or a new one on the work plane it began on.
    pub(crate) fn sketch_surface(&self) -> Option<walls::Plane> {
        let sk = self.sketch.as_ref().filter(|sk| sk.plan == Sheet::View3D)?;
        match sk.of {
            Of::Edit(id) => self.session.model.sketch_plane(id),
            _ => sk.surface,
        }
    }

    /// Where a point picked at `raw` on the plane lands, with the points `from` already picked: on what the sketch's level's
    /// plan has there or the sketch's own lines, as Manage > Snaps says. `square` keeps the step from the last horizontal or
    /// vertical.
    fn snap3(&self, m: &Model, plane: &Plane, raw: P2, from: &[P2], square: bool) -> P2 {
        let Some(sk) = &self.sketch else { return raw };
        let (level, editing) = match sk.of {
            Of::Edit(id) => (m.elements.get(id).and_then(|e| m.level_of(e)).and_then(|g| m.elements.id(g)), Some(id)),
            Of::Floor(level) | Of::Ceiling(level) | Of::Roof(level) | Of::Shaft(level) => (Some(level), None),
            Of::Region { .. } => (None, None),
        };
        let Some(level) = level else { return raw };
        let reach = Reach { point: plane.mm(SNAP), line: plane.mm(f64::from(PICK)) };
        let sketch = snap::Sketch { view: level, lines: &sk.lines, editing };
        snap::in_sketch(m, sketch, raw, reach, from, square, &self.snaps_now()).at
    }

    /// The points the shape in hand has picked, that the next one snaps from.
    fn sketch_from(&self) -> Vec<P2> {
        match &self.tool {
            Tool::SketchLine(chain) if self.shape() == draw::Shape::Line || chain.len() < 2 => chain.clone(),
            _ => Vec::new(),
        }
    }

    /// Before the view turns: a primary drag in Modify from a picked line's end holds that grip, not the camera. True while
    /// one is held.
    pub(crate) fn sketch3d_grab(&mut self, ui: &Ui, response: &Response, plane: Option<Plane>) -> bool {
        let Some(plane) = plane else { return false };
        if self.tool != Tool::Modify {
            return false;
        }
        let Some(sk) = &mut self.sketch else { return false };
        if response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            sk.drag = plane.grip_at(sk, origin).map(Drag::Grip);
        }
        sk.drag.is_some()
    }

    /// The sketch over the 3D view's picture, and what the hand does to it: a held grip let go moves the ends there; a click
    /// (`click`, with the element `under` it, for Pick Walls and Pick Lines) picks a line in Modify, or is the next point of
    /// the tool in hand.
    pub(crate) fn sketch3d(
        &mut self,
        ui: &Ui,
        response: &Response,
        p: &egui::Painter,
        plane: Plane,
        click: Option<Pos2>,
        under: Option<Id>,
    ) {
        let m = std::sync::Arc::clone(&self.session.model);
        let (hover, latest, shift) =
            ui.input(|i| (i.pointer.hover_pos().filter(|_| response.hovered()), i.pointer.latest_pos(), i.modifiers.shift));
        // A held grip let go: every end there moves to where it went, snapped; a quick one was a click, and moves nothing.
        if response.drag_stopped()
            && let Some(Drag::Grip(from)) = self.sketch.as_mut().and_then(|sk| sk.drag.take())
            && !self.canvas.quick()
            && let Some(raw) = latest.and_then(|at| plane.at(at))
        {
            let to = self.snap3(&m, &plane, raw, &[], false);
            if let Some(sk) = &mut self.sketch {
                sk.move_end(from, to);
            }
        }
        if let Some(at) = click {
            self.sketch3d_click(&m, &plane, at, under, click_modifiers(ui));
        }
        let Some(sk) = &self.sketch else { return };
        let picking = matches!(self.tool, Tool::Modify | Tool::Trim(_) | Tool::TrimTo { .. }) && sk.drag.is_none();
        let over = hover.filter(|_| picking).and_then(|at| plane.line_at(sk, at));
        // The snapped cursor is for a grip being dragged and for what a sketch tool shows.
        let wanted = sk.drag.is_some() || matches!(self.tool, Tool::SketchLine(_));
        let cursor =
            hover.filter(|_| wanted).and_then(|at| plane.at(at)).map(|raw| self.snap3(&m, &plane, raw, &self.sketch_from(), shift));
        sk.paint(p, |q| plane.screen(q), over, cursor, &self.sketch_lit());
        if let (Tool::SketchLine(_), Some(c)) = (&self.tool, cursor) {
            let stroke = Stroke::new(2.0, pal().accent);
            for curve in self.shape_preview(c) {
                p.add(Shape::line(curve.points().into_iter().map(|q| plane.screen(q)).collect(), stroke));
            }
            p.circle_stroke(plane.screen(c), 4.0, Stroke::new(1.5, pal().accent));
        }
    }

    /// A click at `at` on screen while sketching in the 3D view, as the plan takes one at the point of the plane under it.
    fn sketch3d_click(&mut self, m: &Model, plane: &Plane, at: Pos2, under: Option<Id>, mods: egui::Modifiers) {
        let Some(raw) = plane.at(at) else {
            self.status =
                Status { text: crate::lang::t("The view is edge-on to the sketch's plane: turn it to pick on it.").into(), error: true };
            return;
        };
        let reach = plane.mm(f64::from(PICK));
        match &self.tool {
            // Picked where it shows, as foreshortened as the view draws it.
            Tool::Modify => {
                let Some(sk) = &mut self.sketch else { return };
                let hit = plane.line_at(sk, at);
                if !(mods.shift || mods.command || mods.ctrl) {
                    sk.picked.clear();
                }
                if let Some(i) = hit
                    && !sk.picked.remove(&i)
                {
                    sk.picked.insert(i);
                }
            }
            Tool::SketchLine(_) | Tool::SketchPick => {
                let p = self.snap3(m, plane, raw, &self.sketch_from(), mods.shift);
                self.sketch_click(m, p, raw, reach, under, mods);
            }
            &Tool::Vertex { delete } => self.vertex_click(None, raw, reach, delete),
            Tool::Split | Tool::Trim(_) | Tool::TrimTo { .. } | Tool::Offset => {
                let p = self.snap3(m, plane, raw, &[], false);
                self.sketch_modify(p, raw, reach);
            }
            Tool::Align(_) => {
                self.status = Status { text: crate::lang::t(ALIGN).into(), error: true };
            }
            _ => {}
        }
    }
}

const ALIGN: &str = msg!("Align lines up with what a plan shows: in the 3D view, drag a line's end or draw it again.");
