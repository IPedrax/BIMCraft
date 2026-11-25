//! Vertical drawing tools share the core's plane and reference resolver. Model XY never receives
//! a section cursor position; all mutations use the same commands as CLI/MCP.
use super::*;
use crate::canvas::{Place, paint};
use bimcraft_core::{
    draw::Shape,
    drawing_plane,
    model::{Dimension, Element},
};

impl BimApp {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn vertical_tools(
        &mut self,
        ui: &Ui,
        p: &egui::Painter,
        rect: Rect,
        view: View,
        id: Id,
        local: Option<(f32, f32)>,
        clicked: Option<(f32, f32)>,
        hovered: Option<Id>,
    ) -> bool {
        // Move with a curtain grid picked moves that grid, on the wall's face (`curtain_face_tool`).
        if !matches!(
            self.tool,
            Tool::Dimension(..)
                | Tool::Move(_)
                | Tool::Scale(_)
                | Tool::DetailLine(_)
                | Tool::DetailView(_)
                | Tool::Trim(_)
                | Tool::TrimTo { .. }
        ) || self.curtain_face_tool()
        {
            return false;
        }
        let model = Arc::clone(&self.session.model);
        let m = &model;
        let guid = m.guid(id);
        let near = 2.0 * f64::from(PICK) / view.scale;
        let screen = |q: P2| {
            let (x, y) = view.px(q);
            rect.min + egui::vec2(x, y)
        };
        let stroke = egui::Stroke::new(1.5, canvas_pal().accent);
        let raw = local.map(|q| view.mm(q));
        let mut cursor = raw.map(|q| {
            let mut candidates = hovered.map(|h| drawing_plane::references(m, guid, h)).unwrap_or_default();
            candidates.sort_by_key(|(r, _)| r.point.is_none());
            if let Some(q) = candidates.into_iter().find_map(|(r, (a, b))| {
                let on = if r.point.is_some() { a } else { a + (b - a).unit() * (q - a).dot((b - a).unit()) };
                (q.dist(on) <= near).then_some(on)
            }) {
                return q;
            }
            if self.grid_snapping
                && let Some(q) = self.background_grid().nearest(q)
            {
                return q;
            }
            P2((q.0 / 10.0).round() * 10.0, (q.1 / 10.0).round() * 10.0)
        });
        if let (Some(base), Some(q)) = (self.base(), cursor)
            && (ui.input(|i| i.modifiers.shift) || self.constrain && matches!(self.tool, Tool::Move(_)))
        {
            let d = q - base;
            cursor = Some(base + if d.0.abs() > d.1.abs() { P2(d.0, 0.0) } else { P2(0.0, d.1) });
        }
        self.canvas.cursor = cursor;
        // The plan's cached snap/guide belongs to a different coordinate system.
        self.canvas.found.set(None);
        self.tab_cycle = false;
        match self.tool.clone() {
            Tool::Dimension(mode, mut refs) => {
                let candidate = raw.and_then(|q| hovered.and_then(|h| drawing_plane::pick(m, guid, h, q, near, &refs)));
                for r in refs.iter().chain(candidate.iter()) {
                    if let Ok((a, b)) = drawing_plane::reference(m, Some(guid), r) {
                        if r.point.is_some() {
                            p.circle_stroke(screen(a), 5.0, stroke);
                        } else {
                            let u = (b - a).unit();
                            let centre = raw.unwrap_or(a);
                            let on = a + u * (centre - a).dot(u);
                            p.line_segment([screen(on - u * near * 8.0), screen(on + u * near * 8.0)], stroke);
                        }
                    }
                }
                if candidate.is_none()
                    && refs.len() >= 2
                    && let Some(at) = cursor
                {
                    let (kind, orient) = mode.kind_in(m, Some(guid), &refs, at);
                    let dm = Dimension {
                        level: guid,
                        view: Some(guid),
                        kind,
                        orient,
                        refs: refs.clone(),
                        at,
                        type_name: self.dimension_type_for(kind).unwrap_or_default(),
                        ..Dimension::default()
                    };
                    let drawing = render::sketch(m, view, 0, &Element::Dimension(dm));
                    paint(p, Place::at(rect.min), &drawing, &[], None, self.thin, &[]);
                }
                if clicked.is_some() {
                    if let Some(r) = candidate {
                        if let Some(i) = refs.iter().position(|a| *a == r) {
                            refs.remove(i);
                        } else {
                            refs.push(r);
                        }
                        self.tool = Tool::Dimension(mode, refs);
                    } else if refs.len() >= 2
                        && let Some(at) = cursor
                    {
                        let (kind, orient) = mode.kind_in(m, Some(guid), &refs, at);
                        let mut params = json!({"view":id,"refs":refs,"at":[at.0,at.1],"kind":kind});
                        if let Some(o) = orient {
                            params["orient"] = json!(o);
                        }
                        if let Some(t) = self.dimension_type_for(kind) {
                            params["type"] = json!(t);
                        }
                        if self.run("dimension", params).is_some() {
                            self.tool = Tool::Dimension(mode, Vec::new());
                        }
                    } else {
                        self.status = Status {
                            text: crate::lang::t("Pick levels, perpendicular grids or detail lines in this view.").into(),
                            error: true,
                        };
                    }
                }
            }
            Tool::Move(base) => {
                if let (Some(a), Some(b)) = (base, cursor) {
                    p.line_segment([screen(a), screen(b)], stroke);
                    p.text(
                        screen(b) + egui::vec2(12.0, -12.0),
                        egui::Align2::LEFT_BOTTOM,
                        m.length(a.dist(b)),
                        egui::FontId::proportional(13.0),
                        stroke.color,
                    );
                }
                if clicked.is_some()
                    && let Some(at) = cursor
                {
                    self.vertical_point(id, at);
                }
            }
            // Scale: the point that stays, or two points and the line between them, in the view's own terms.
            Tool::Scale(step) => {
                use crate::scale::Step;
                let cross = |c: egui::Pos2| {
                    p.circle_stroke(c, 5.0, stroke);
                    p.line_segment([c - egui::vec2(8.0, 0.0), c + egui::vec2(8.0, 0.0)], stroke);
                    p.line_segment([c - egui::vec2(0.0, 8.0), c + egui::vec2(0.0, 8.0)], stroke);
                };
                let to = |a: P2, b: P2| {
                    p.line_segment([screen(a), screen(b)], stroke);
                    p.text(
                        screen(b) + egui::vec2(12.0, -12.0),
                        egui::Align2::LEFT_BOTTOM,
                        m.length(a.dist(b)),
                        egui::FontId::proportional(13.0),
                        stroke.color,
                    );
                };
                match (step, cursor) {
                    (Step::Base(_) | Step::First, Some(at)) => cross(screen(at)),
                    (Step::Second(a), Some(b)) => {
                        cross(screen(a));
                        to(a, b);
                    }
                    (Step::Length(a, b), _) => {
                        cross(screen(a));
                        to(a, b);
                    }
                    _ => {}
                }
                if clicked.is_some()
                    && let Some(at) = cursor
                {
                    self.scale_point(at);
                }
            }
            Tool::DetailLine(ref path) => {
                if self.shape() == Shape::Line
                    && let (Some(a), Some(b)) = (path.last(), cursor)
                {
                    p.line_segment([screen(*a), screen(b)], stroke);
                }
                if let Some(cursor) = cursor {
                    for curve in self.shape_preview(cursor) {
                        p.add(egui::Shape::line(curve.points().into_iter().map(screen).collect(), stroke));
                    }
                }
                if clicked.is_some()
                    && let Some(at) = cursor
                {
                    if matches!(self.shape(), Shape::PickLines | Shape::Fillet) {
                        self.status = Status {
                            text: crate::lang::t("Use drawn detail shapes in this view; Pick Lines and Fillet are not supported yet.")
                                .into(),
                            error: true,
                        };
                    } else {
                        self.place_detail_line(id, at);
                    }
                }
            }
            Tool::DetailView(first) => {
                if let (Some(a), Some(b)) = (first, cursor) {
                    let c = [a, P2(b.0, a.1), b, P2(a.0, b.1)].map(screen);
                    p.add(egui::Shape::closed_line(c.to_vec(), stroke));
                }
                if clicked.is_some()
                    && let Some(at) = cursor
                {
                    match first {
                        None => self.tool = Tool::DetailView(Some(at)),
                        Some(a) if a.dist(at) > 1e-6 && self.make_callout(id, a, at) => self.tool = Tool::DetailView(None),
                        Some(_) => {}
                    }
                }
            }
            Tool::Trim(_) | Tool::TrimTo { .. } => {
                if let Some(at) = clicked.map(|q| view.mm(q)) {
                    let own = hovered.filter(|h| matches!(m.elements.get(*h),Some(Element::DetailLine(l)) if m.same(l.view,id)));
                    if own.is_some() {
                        self.vertical_trim(own, at);
                    } else {
                        self.status = Status {
                            text: crate::lang::t("Trim in this view supports its detail lines; model edges are not supported yet.").into(),
                            error: true,
                        };
                    }
                }
            }
            _ => {}
        }
        true
    }

    fn vertical_trim(&mut self, hovered: Option<Id>, at: P2) {
        let before = self.tool.clone();
        self.status = Status::default();
        self.trim_click(hovered, at);
        if self.status.error {
            self.tool = before;
        }
    }

    pub(crate) fn vertical_point(&mut self, view: Id, at: P2) {
        match self.tool.clone() {
            Tool::Move(None) => self.tool = Tool::Move(Some(at)),
            Tool::Move(Some(base)) if base.dist(at) > 1e-6 => {
                let by = at - base;
                if self.run("move", json!({"view":view,"ids":self.selection,"by":[by.0,by.1]})).is_some() {
                    self.tool = Tool::Modify;
                }
            }
            Tool::DetailLine(_) => self.place_detail_line(view, at),
            _ => {}
        }
    }

    pub(crate) fn vertical_enter(&mut self, view: Id) {
        if self.typed_length.is_empty() {
            if matches!(self.tool, Tool::DetailLine(_)) {
                self.finish_detail_line(view);
            }
            return;
        }
        let (Some(base), Some(cursor)) = (self.base(), self.canvas.cursor) else { return };
        let Some(length) = self.units.parse(&self.typed_length).filter(|n| n.is_finite() && *n > 0.0) else {
            self.status = Status { text: crate::lang::t("Enter a positive length.").into(), error: true };
            return;
        };
        if base.dist(cursor) <= 1e-6 {
            return;
        }
        let at = base + (cursor - base).unit() * length;
        self.vertical_point(view, at);
        self.clear_typed();
    }
}
