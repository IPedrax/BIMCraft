//! An adaptive component in a plan, as Revit places and edits one: with Component and an adaptive family, each click places
//! the next adaptive point (on the level, or on another adaptive instance's point within reach) and the last places the
//! instance (`adaptive_place`, its frames the project's); Escape forgets the points clicked. An adaptive instance selected
//! alone has a grip on each adaptive point, which a drag moves over the plan (`adaptive_move`, one undo step).
use bimcraft_core::adaptive::Frame;
use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, Model};
use bimcraft_core::render::View;
use egui::{Align2, FontId, PointerButton, Pos2, Rect, Response, Stroke, StrokeKind, Ui, vec2};
use serde_json::json;

use crate::lang::tf;
use crate::style::pal;
use crate::{BimApp, Status, Tool};

/// How near, px, a click must be to another instance's adaptive point to snap to it, and to a grip to hold it.
const REACH: f32 = 10.0;

impl BimApp {
    /// How many adaptive points the family the Component tool places has; none for a family that isn't adaptive.
    pub(crate) fn adaptive_count(&self) -> Option<u32> {
        if !matches!(self.tool, Tool::Component) {
            return None;
        }
        let (family, _) = self.placing()?;
        let d = self.session.model.families.get(&family)?.adaptive.as_ref()?;
        Some(d.next_number() - 1).filter(|n| *n > 0)
    }

    /// Every adaptive instance's adaptive points in the project: what a click snaps to.
    fn adaptive_snaps(m: &Model) -> Vec<[f64; 3]> {
        m.elements
            .values()
            .filter_map(|e| match e {
                Element::Component(c) => Some(m.adaptive_points(c)),
                _ => None,
            })
            .flatten()
            .map(|(_, f)| f.origin)
            .collect()
    }

    /// The next adaptive point at pixel `at` of plan `v` on `level`: another instance's point within reach, else where the
    /// plan's snaps put it, at the level's height. The last of the family's points places the instance.
    pub(crate) fn adaptive_click(&mut self, m: &Model, level: Id, v: &View, at: (f32, f32)) {
        let Some(n) = self.adaptive_count() else { return };
        let Some((family, ty)) = self.placing() else { return };
        let near = Self::adaptive_snaps(m)
            .into_iter()
            .map(|p| (p, v.px(P2(p[0], p[1]))))
            .map(|(p, q)| (p, (q.0 - at.0).hypot(q.1 - at.1)))
            .filter(|(_, d)| *d <= REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(p, _)| p);
        let p = near.unwrap_or_else(|| {
            let q = self.snap_point(m, level, v, at, &[], false);
            [q.0, q.1, m.elevation(level)]
        });
        self.adaptive_clicks.push(p);
        let done = self.adaptive_clicks.len() as u32;
        if done < n {
            self.status = Status {
                text: tf("Adaptive point {} placed: click point {} of {}.", &[&done.to_string(), &(done + 1).to_string(), &n.to_string()]),
                error: false,
            };
            return;
        }
        let frames: serde_json::Map<String, serde_json::Value> = std::mem::take(&mut self.adaptive_clicks)
            .into_iter()
            .enumerate()
            .map(|(k, p)| ((k + 1).to_string(), json!(Frame::at(p))))
            .collect();
        self.run("adaptive_place", json!({"family": family, "type": ty, "placements": [{"level": level, "adaptive": {"frames": frames}}]}));
    }

    /// The adaptive instance selected alone, with Modify: the one whose adaptive points are grips.
    fn adaptive_gripped(&self) -> Option<Id> {
        match (self.selection.as_slice(), &self.tool) {
            (&[id], Tool::Modify) => match self.session.model.elements.get(id) {
                Some(Element::Component(c)) if !self.session.model.adaptive_points(c).is_empty() => Some(id),
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether a drag from pixel `origin` would take hold of a grip of the adaptive instance selected.
    pub(crate) fn adaptive_grip_at(&self, v: &View, rect: Rect, origin: Pos2) -> Option<(Id, u32)> {
        let id = self.adaptive_gripped()?;
        let Some(Element::Component(c)) = self.session.model.elements.get(id) else { return None };
        let at = |p: [f64; 3]| crate::canvas::pos(rect.min, v.px(P2(p[0], p[1])));
        self.session.model.adaptive_points(c).into_iter().find(|(_, f)| at(f.origin).distance(origin) <= REACH).map(|(n, _)| (id, n))
    }

    /// A drag from a grip of the adaptive instance selected moves that point over the plan, at its height; on release,
    /// `adaptive_move`.
    pub(crate) fn adaptive_grip_drag(&mut self, ui: &Ui, response: &Response, rect: Rect, v: &View, level: Id) {
        if self.adaptive_gripped().is_none() {
            self.adaptive_grip = None;
            return;
        }
        if response.drag_started_by(PointerButton::Primary)
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
        {
            self.adaptive_grip = self.adaptive_grip_at(v, rect, o);
        }
        let Some((id, n)) = self.adaptive_grip else { return };
        if response.drag_stopped() {
            self.adaptive_grip = None;
            let m = std::sync::Arc::clone(&self.session.model);
            let Some(Element::Component(c)) = m.elements.get(id) else { return };
            let Some(z) = m.adaptive_points(c).into_iter().find(|(k, _)| *k == n).map(|(_, f)| f.origin[2]) else { return };
            if !self.canvas.quick()
                && let Some(q) = ui.input(|i| i.pointer.latest_pos())
            {
                let to = self.snap_point(&m, level, v, (q.x - rect.min.x, q.y - rect.min.y), &[], false);
                self.run("adaptive_move", json!({"id": id, "number": n, "to": [to.0, to.1, z]}));
            }
        }
    }

    /// The points clicked so far for an adaptive instance, numbered, and the grips of the one selected: the one held where
    /// the pointer is.
    pub(crate) fn adaptive_overlay(&self, ui: &Ui, rect: Rect, v: &View) {
        let p = ui.painter_at(rect);
        let at = |q: [f64; 3]| crate::canvas::pos(rect.min, v.px(P2(q[0], q[1])));
        let ink = pal().accent;
        for (k, q) in self.adaptive_clicks.iter().enumerate() {
            p.circle_filled(at(*q), 4.0, ink);
            p.text(at(*q) + vec2(6.0, -6.0), Align2::LEFT_BOTTOM, (k + 1).to_string(), FontId::proportional(13.0), ink);
        }
        let Some(id) = self.adaptive_gripped() else { return };
        let Some(Element::Component(c)) = self.session.model.elements.get(id) else { return };
        let held = ui.input(|i| i.pointer.latest_pos());
        for (n, f) in self.session.model.adaptive_points(c) {
            let q = match (self.adaptive_grip, held) {
                (Some((g, k)), Some(now)) if g == id && k == n => now,
                _ => at(f.origin),
            };
            let r = Rect::from_center_size(q, vec2(9.0, 9.0));
            p.rect_filled(r, 1.0, pal().surface);
            p.rect_stroke(r, 1.0, Stroke::new(1.5, ink), StrokeKind::Middle);
            p.text(q + vec2(7.0, -7.0), Align2::LEFT_BOTTOM, n.to_string(), FontId::proportional(12.0), ink);
        }
    }
}
