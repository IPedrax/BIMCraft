//! Shape Editing's actions (MOD-16, [shape editing](../../../docs/shape-editing.md)): what a click or a grip's drag does to
//! the selected floor's or flat roof's shape, the same from a plan and from the 3D view, each one command. The views find
//! the plan point; this runs it.

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Floor, Id, Model, Shape, TOL};
use serde_json::{Value, json};

use egui::{Align, Align2, FontId, Key, Pos2, Rect, Sense, TextEdit, Ui, WidgetInfo, WidgetType, vec2};

use crate::lang::{t, tf};
use crate::style::pal;
use crate::{BimApp, Status, Tool};

/// Where a height label with `text` lies, its bottom left at `anchor`, with its margin.
pub(crate) fn label_rect(ui: &Ui, anchor: Pos2, text: &str) -> Rect {
    let size = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(12.0), pal().accent).size();
    Align2::LEFT_BOTTOM.anchor_size(anchor, size).expand2(vec2(3.0, 1.0))
}

fn point(p: P2) -> Value {
    json!([p.0, p.1])
}

/// Floor `f`'s shape with its point or corner at `at` raised `by` mm, to the millimetre; none when that leaves it as it is.
pub(crate) fn raised(m: &Model, f: &Floor, at: P2, by: f64) -> Option<Shape> {
    let now = m.shape_height(f, at);
    let z = (now + by).round();
    ((z - now).abs() >= 0.5).then(|| f.shape.raised(at, z))
}

impl BimApp {
    /// The floor or flat roof being shaped: the one selected, while a Shape Editing tool is on.
    pub(crate) fn shaping(&self) -> Option<Id> {
        self.selected_shapeable().filter(|_| matches!(self.tool, Tool::ModifySubElements | Tool::AddPoint | Tool::AddSplitLine(_)))
    }

    /// The selected floor or flat roof as shape editing sees it now (`Model::shapeable`: a roof's top as a floor's).
    pub(crate) fn floor_selected(&self) -> Option<(Id, Floor)> {
        let id = self.selected_shapeable()?;
        self.session.model.shapeable(id).ok().map(|f| (id, f))
    }

    /// Floor `id`'s shape given anew, its points' and lines' ids kept: one `edit_shape`.
    pub(crate) fn edit_shape(&mut self, id: Id, shape: &Shape) -> Option<Value> {
        let points: Vec<Value> = shape.points.iter().map(|p| json!({"id": p.id, "at": point(p.at), "z": p.z})).collect();
        let lines: Vec<Value> = shape.lines.iter().map(|l| json!({"id": l.id, "a": point(l.a), "b": point(l.b)})).collect();
        self.run("edit_shape", json!({"id": id, "points": points, "lines": lines}))
    }

    /// Add Point at `p` on the selected floor's top: at the Options Bar's elevation, or the top's height there.
    pub(crate) fn shape_add_point(&mut self, p: P2) {
        let Some((floor, f)) = self.floor_selected() else { return };
        let m = &self.session.model;
        let mut q = json!({"id": floor, "at": point(p)});
        // `add_point` reads an absolute elevation from the project's zero; the Options Bar's, from the floor's level.
        let level = if self.shape_absolute { m.elevation(f.level) } else { 0.0 };
        if !self.shape_z.trim().is_empty() {
            match m.units.parse(&self.shape_z) {
                Some(z) => q["z"] = json!(z + level),
                None => {
                    self.status = Status { text: tf("add point: `{}` is not an elevation", &[&self.shape_z]), error: true };
                    return;
                }
            }
        }
        if self.shape_absolute {
            q["absolute"] = json!(true);
        }
        self.run("add_point", q);
    }

    /// Modify Sub Elements: the points and corners `picked` (a point, a corner, or an edge's two ends) each to the Options
    /// Bar's elevation, in one `edit_shape`; a corner becomes a point.
    pub(crate) fn shape_modify(&mut self, picked: impl FnOnce(&Floor) -> Vec<P2>) {
        let typed = self.shape_z.clone();
        self.shape_heights(picked, &typed);
    }

    /// The points and corners `picked` each to elevation `typed`, as the Options Bar's Elevation reads (above the unshaped
    /// top, or with Absolute above the level), in one `edit_shape`; a corner becomes a point.
    pub(crate) fn shape_heights(&mut self, picked: impl FnOnce(&Floor) -> Vec<P2>, typed: &str) {
        let Some((floor, f)) = self.floor_selected() else { return };
        let m = &self.session.model;
        let Some(z) = m.units.parse(typed) else {
            let text = if typed.trim().is_empty() {
                t("type the elevation first").to_string()
            } else {
                tf("modify sub elements: `{}` is not an elevation", &[typed])
            };
            self.status = Status { text, error: true };
            return;
        };
        let picked = picked(&f);
        if picked.is_empty() {
            return;
        }
        // Above the top as it would be without the shape, or with Absolute, above the level.
        let (base, level) = (m.floor_top(&f), m.elevation(f.level));
        let lift = |at: P2| if self.shape_absolute { z - (base.at(at) - level) } else { z };
        let mut shape = f.shape.clone();
        for at in picked {
            match shape.points.iter_mut().find(|p| p.at.dist(at) <= TOL) {
                Some(p) => p.z = lift(at),
                None => shape.points.push(bimcraft_core::model::ShapePoint { at, z: lift(at), ..Default::default() }),
            }
        }
        self.edit_shape(floor, &shape);
    }

    /// Add Split Line: its start at `p`, or its end there; then ready for another. Refused, it keeps its start, to end it
    /// elsewhere.
    pub(crate) fn shape_split(&mut self, p: P2) {
        let Tool::AddSplitLine(start) = self.tool else { return };
        match (start, self.selected_shapeable()) {
            (None, _) => self.tool = Tool::AddSplitLine(Some(p)),
            (Some(a), Some(floor))
                if a.dist(p) > TOL && self.run("add_split_line", json!({"id": floor, "a": point(a), "b": point(p)})).is_some() =>
            {
                self.tool = Tool::AddSplitLine(None);
            }
            _ => {}
        }
    }

    /// A shape point's height label, its bottom left at `anchor`: `text`, which a click turns into a field to type the height
    /// in (as the Options Bar's Elevation reads it); Enter sets it, Esc or leaving it unchanged keeps it. Where it is.
    pub(crate) fn shape_label(&mut self, ui: &mut Ui, anchor: Pos2, q: P2, text: &str) -> Rect {
        let at = label_rect(ui, anchor, text);
        let field = ui.id().with(("shape height", (q.0.to_bits(), q.1.to_bits())));
        let mut done = None;
        match &mut self.shape_label_edit {
            Some((p, typed)) if p.dist(q) <= TOL => {
                let edit = ui.put(at.expand2(vec2(18.0, 2.0)), TextEdit::singleline(typed).id(field).horizontal_align(Align::Center));
                if edit.lost_focus() {
                    done = Some(ui.input(|i| !i.key_pressed(Key::Escape)).then(|| typed.clone()));
                } else if !edit.has_focus() {
                    edit.request_focus();
                }
            }
            _ => {
                let r = ui.interact(at, field, Sense::click());
                r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tf("Point height {}", &[text])));
                let p = ui.painter();
                p.rect_filled(at, 2.0, if r.hovered() { pal().accent } else { pal().surface.gamma_multiply(0.85) });
                p.text(
                    at.center(),
                    Align2::CENTER_CENTER,
                    text,
                    FontId::proportional(12.0),
                    if r.hovered() { pal().surface } else { pal().accent },
                );
                if r.clicked() {
                    self.shape_label_edit = Some((q, text.to_string()));
                }
            }
        }
        if let Some(typed) = done {
            self.shape_label_edit = None;
            if let Some(typed) = typed.filter(|t| t.trim() != text) {
                self.shape_heights(|_| vec![q], &typed);
            }
        }
        at.expand2(vec2(18.0, 2.0))
    }

    /// The selected floor's point or corner at `at` raised `by` mm (lowered, negative), to the millimetre: a grip dragged
    /// up or down in the 3D view. A corner becomes a point.
    pub(crate) fn shape_raise(&mut self, at: P2, by: f64) {
        let Some((floor, f)) = self.floor_selected() else { return };
        if let Some(shape) = raised(&self.session.model, &f, at, by) {
            self.edit_shape(floor, &shape);
        }
    }
}
