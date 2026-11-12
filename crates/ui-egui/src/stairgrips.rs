//! A selected stair's grips on the plan: its foot (the stair moves), its top (it turns about its foot), the end of each
//! run before a turn (the run takes or gives risers), its sides (its width) and each landing's far edge (its length).
//! Where they are and what a drag makes of the stair is the core's (`stairs::grips`, `stairs::dragged`); a drag shows
//! the stair as it would be, with what its building code would say of it, and on release runs `modify`.

use bimcraft_core::codes;
use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, Model, Stair};
use bimcraft_core::stairs::{self, Grip};
use egui::{Align2, FontId, PointerButton, Pos2, Rect, Response, Shape, Stroke, StrokeKind, Ui, vec2};
use serde_json::json;

use crate::lang::{t, tf};
use crate::style::pal;
use crate::{BimApp, Tool};
use bimcraft_core::render::View;

fn pos(origin: Pos2, (x, y): (f32, f32)) -> Pos2 {
    origin + vec2(x, y)
}

/// How near a grip, in pixels, a press takes hold of it.
const REACH: f32 = 8.0;

impl BimApp {
    /// The stair selected alone, with Modify, on the plan of the level it stands on.
    fn gripped_stair(&self, m: &Model, level: Id) -> Option<(Id, Stair)> {
        match (self.selection.as_slice(), &self.tool) {
            (&[id], Tool::Modify) => match m.elements.get(id) {
                Some(Element::Stair(st)) if st.level == m.guid(level) => Some((id, st.clone())),
                _ => None,
            },
            _ => None,
        }
    }

    /// The stair with the grip held dragged to pixel `q`, snapped as a point picked is.
    #[allow(clippy::too_many_arguments)]
    fn stair_dragged(&self, m: &Model, level: Id, v: &View, st: &Stair, grip: Grip, q: (f32, f32), square: bool) -> Stair {
        // A width, a run's end and a landing's edge follow the cursor itself; the foot and the top land on what is there.
        let to = match grip {
            // The foot goes where it is put; the top is aimed from the foot, with its angle guides.
            Grip::Foot | Grip::Riser(..) | Grip::Centre | Grip::Vertex(_) => self.snap(m, level, v, q, &[], square).at,
            Grip::Top => self.snap(m, level, v, q, &[st.start], square).at,
            _ => v.mm(q),
        };
        stairs::dragged(m, st, grip, to)
    }

    /// A drag from a grip of the selected stair; on release, `modify` with what the drag changed.
    pub(crate) fn stair_grip_drag(&mut self, ui: &Ui, response: &Response, rect: Rect, view: &View, level: Id) {
        let m = std::sync::Arc::clone(&self.session.model);
        let Some((id, st)) = self.gripped_stair(&m, level) else {
            self.canvas.stair_grip = None;
            return;
        };
        let at = |p: P2| pos(rect.min, view.px(p));
        if response.drag_started_by(PointerButton::Primary)
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
        {
            let near = |p: P2| at(p).distance(o);
            let nearest =
                stairs::grips(&m, &st).into_iter().filter(|(_, p)| near(*p) <= REACH).min_by(|a, b| near(a.1).total_cmp(&near(b.1)));
            self.canvas.stair_grip = nearest.map(|(g, _)| (id, g));
        }
        let Some((held, grip)) = self.canvas.stair_grip else { return };
        if held != id {
            self.canvas.stair_grip = None;
            return;
        }
        if response.drag_stopped() {
            self.canvas.stair_grip = None;
            if let Some(q) = ui.input(|i| i.pointer.latest_pos()) {
                let square = ui.input(|i| i.modifiers.shift);
                let after = self.stair_dragged(&m, level, view, &st, grip, (q.x - rect.min.x, q.y - rect.min.y), square);
                self.snap_once = None;
                if after != st {
                    let p = match grip {
                        // One drawn along a path: its points, where it starts and how wide it is.
                        _ if !after.path.is_empty() => {
                            json!({"id": id, "start": [after.start.0, after.start.1], "angle": after.angle, "path": after.path, "width": after.width})
                        }
                        // One drawn by hand: its risers, and where it starts with them.
                        _ if !after.sketch.is_empty() => {
                            json!({"id": id, "start": [after.start.0, after.start.1], "angle": after.angle, "sketch": after.sketch})
                        }
                        Grip::Riser(..) | Grip::Vertex(_) => return,
                        Grip::Centre => {
                            json!({"id": id, "start": [after.start.0, after.start.1], "angle": after.angle, "spiral": after.spiral})
                        }
                        Grip::Foot => json!({"id": id, "start": [after.start.0, after.start.1]}),
                        Grip::Top => json!({"id": id, "angle": after.angle}),
                        Grip::Width(_) => json!({"id": id, "width": after.width}),
                        Grip::Turn(_) | Grip::Landing(_) => json!({"id": id, "landings": after.landings}),
                    };
                    self.run("modify", p);
                }
            }
        }
    }

    /// The selected stair's grips; while one is dragged, the stair as it would be, and beside the cursor what changes
    /// and how many rules of the building code it would be outside of.
    pub(crate) fn stair_grip_overlay(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect, level: Id) {
        let Some((id, st)) = self.gripped_stair(m, level) else { return };
        let p = ui.painter_at(rect);
        let at = |q: P2| pos(rect.min, v.px(q));
        let held = self.canvas.stair_grip.filter(|(g, _)| *g == id).map(|(_, g)| g);
        let mut shown = st.clone();
        if let (Some(grip), Some(q)) = (held, ui.input(|i| i.pointer.latest_pos())) {
            let square = ui.input(|i| i.modifiers.shift);
            shown = self.stair_dragged(m, level, v, &st, grip, (q.x - rect.min.x, q.y - rect.min.y), square);
            let l = stairs::stair(m, &shown);
            let rings = l.runs.iter().map(|r| r.ring()).chain(l.pads.iter().map(|pad| pad.ring.clone()));
            for ring in rings {
                let ring: Vec<Pos2> = ring.into_iter().map(&at).collect();
                p.add(Shape::closed_line(ring, Stroke::new(1.0, pal().accent)));
            }
            // What the drag is changing, and what the code would say of the stair.
            let (n, _, _) = m.flight(&shown);
            let what = match grip {
                Grip::Riser(i, _) => tf("Riser {}", &[&(i + 1).to_string()]),
                Grip::Centre => m.units.with_unit(shown.spiral.map_or(0.0, |sp| sp.radius.abs())),
                Grip::Vertex(i) => {
                    let from = if i == 0 { shown.start } else { shown.path.get(i - 1).map_or(shown.start, |s| s.to) };
                    m.units.with_unit(shown.path.get(i).map_or(0.0, |s| s.to.dist(from)))
                }
                Grip::Foot => m.units.with_unit(st.start.dist(shown.start)),
                Grip::Top => format!("{:.1}°", shown.angle),
                Grip::Width(_) => m.units.with_unit(shown.width),
                Grip::Turn(i) => {
                    shown.landings.get(i).map_or_else(String::new, |t| tf("{} of {} risers", &[&t.at.to_string(), &n.to_string()]))
                }
                Grip::Landing(i) => shown.landings.get(i).map_or_else(String::new, |t| m.units.with_unit(t.length)),
            };
            let outside = codes::stair_findings(m, &shown).len();
            let (said, color) = match outside {
                0 => (t("Inside the rules").to_string(), pal().muted),
                1 => (t("Outside 1 rule").to_string(), crate::codes::OUTSIDE),
                n => (tf("Outside {} rules", &[&n.to_string()]), crate::codes::OUTSIDE),
            };
            p.text(q + vec2(14.0, -22.0), Align2::LEFT_BOTTOM, what, FontId::proportional(12.5), pal().text);
            p.text(q + vec2(14.0, -8.0), Align2::LEFT_BOTTOM, said, FontId::proportional(11.5), color);
        }
        for (grip, q) in stairs::grips(m, &shown) {
            let fill = if held == Some(grip) { pal().accent } else { egui::Color32::WHITE };
            let r = Rect::from_center_size(at(q), vec2(8.0, 8.0));
            p.rect_filled(r, 0.0, fill);
            p.rect_stroke(r, 0.0, Stroke::new(2.0, pal().accent), StrokeKind::Middle);
        }
    }
}
