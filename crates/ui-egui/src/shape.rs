//! Shape Editing's actions (MOD-16, [shape editing](../../../docs/shape-editing.md)): what a click or a grip's drag does to
//! the selected floor's or flat roof's shape, the same from a plan and from the 3D view, each one command. The views find
//! the plan point; this runs it.

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Floor, Id, Model, Shape, TOL};
use serde_json::{Value, json};

use bimcraft_core::geom;
use bimcraft_core::view3d::{P3, shape_handles};
use egui::{Align, Align2, FontId, Key, Painter, Pos2, Rect, Sense, Stroke, TextEdit, Ui, WidgetInfo, WidgetType, vec2};

use crate::lang::{t, tf};
use crate::style::pal;
use crate::{BimApp, Status, Tool};

/// Where a height label with `text` lies, its bottom left at `anchor`, with its margin.
pub(crate) fn label_rect(ui: &Ui, anchor: Pos2, text: &str) -> Rect {
    let size = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(12.0), pal().accent).size();
    Align2::LEFT_BOTTOM.anchor_size(anchor, size).expand2(vec2(3.0, 1.0))
}

/// What Modify Sub Elements selects of a shape, as Revit's does: a point (a shape point or a corner of the outline or an
/// opening), or an edge (a split line, or an edge of the outline or an opening) by its two ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Sub {
    Point(P2),
    Edge(P2, P2),
}

impl Sub {
    /// What a pick of `ends` (one, a point; two, an edge's) selects.
    pub(crate) fn of(ends: &[P2]) -> Option<Sub> {
        match *ends {
            [p] => Some(Sub::Point(p)),
            [a, b] => Some(Sub::Edge(a, b)),
            _ => None,
        }
    }

    /// Its points: itself, or an edge's two ends.
    pub(crate) fn ends(self) -> Vec<P2> {
        match self {
            Sub::Point(p) => vec![p],
            Sub::Edge(a, b) => vec![a, b],
        }
    }

    /// Where its label goes: at the point, or an edge's middle.
    pub(crate) fn at(self) -> P2 {
        match self {
            Sub::Point(p) => p,
            Sub::Edge(a, b) => (a + b) * 0.5,
        }
    }

    fn same(self, o: Sub) -> bool {
        match (self, o) {
            (Sub::Point(p), Sub::Point(q)) => p.dist(q) <= TOL,
            (Sub::Edge(a, b), Sub::Edge(c, d)) => (a.dist(c) <= TOL && b.dist(d) <= TOL) || (a.dist(d) <= TOL && b.dist(c) <= TOL),
            _ => false,
        }
    }
}

fn point(p: P2) -> Value {
    json!([p.0, p.1])
}

/// Floor `f`'s shape with each of its points and corners `all` raised `by` mm from where it is, to the millimetre; none
/// when that leaves it as it is.
pub(crate) fn raised_all(m: &Model, f: &Floor, all: &[P2], by: f64) -> Option<Shape> {
    let mut shape = f.shape.clone();
    for &at in all {
        let now = m.shape_height(f, at);
        let z = (now + by).round();
        if (z - now).abs() >= 0.5 {
            shape = shape.raised(at, z);
        }
    }
    (shape != f.shape).then_some(shape)
}

/// How high `ends` lie on floor `f`'s top, raised `by` mm, as the Options Bar's Elevation reads them (above the unshaped
/// top, or with `absolute` above the level): one height, or the lowest and the highest, `-30…-10`.
pub(crate) fn heights_text(m: &Model, f: &Floor, ends: &[P2], absolute: bool, by: f64) -> String {
    let (base, level) = (m.floor_top(f), m.elevation(f.level));
    let read = |q: P2| m.shape_height(f, q) + by + if absolute { base.at(q) - level } else { 0.0 };
    let zs: Vec<f64> = ends.iter().map(|q| read(*q)).collect();
    let (lo, hi) = zs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| (lo.min(*z), hi.max(*z)));
    if hi - lo < 0.5 { m.units.length(lo) } else { format!("{}…{}", m.units.length(lo), m.units.length(hi)) }
}

/// What edit mode draws of a shaped top, in the model: its points and corners (whether on the outline or an opening's
/// edge), the outline's and openings' edges, its split lines, and the creases between its planes that are neither.
pub(crate) struct Marks {
    pub points: Vec<(P2, P3, bool)>,
    pub rims: Vec<[P3; 2]>,
    pub splits: Vec<[P3; 2]>,
    pub creases: Vec<[P3; 2]>,
}

/// Floor `f`'s marks, each where its top is.
pub(crate) fn marks(m: &Model, f: &Floor) -> Marks {
    let height = |q: P2| m.floor_top(f).at(q) + m.shape_height(f, q);
    let lift = |q: P2| [q.0, q.1, height(q)];
    let rings: Vec<(P2, P2)> = std::iter::once(&f.boundary).chain(&f.openings).flat_map(|r| geom::ring_edges(r)).collect();
    let on = |segs: &[(P2, P2)], q: P2| segs.iter().any(|&(a, b)| geom::dist_to_segment(q, a, b) <= TOL);
    let splits: Vec<(P2, P2)> = f.shape.lines.iter().map(|l| (l.a, l.b)).collect();
    let (handles, _) = shape_handles(m, f);
    Marks {
        points: handles.into_iter().map(|(q, p)| (q, p, on(&rings, q))).collect(),
        rims: rings.iter().map(|&(a, b)| [lift(a), lift(b)]).collect(),
        splits: splits.iter().map(|&(a, b)| [lift(a), lift(b)]).collect(),
        creases: m
            .shape_creases(f)
            .into_iter()
            .filter(|[a, b]| {
                let mid = P2((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0);
                !on(&rings, mid) && !on(&splits, mid)
            })
            .collect(),
    }
}

/// `marks` drawn on `p` through `to` (the model to the screen), as Revit's edit mode colours them: the creases grey, the
/// split lines blue, the outline's and openings' edges dashed green; the points as dots, green on an edge, blue inside, `lit`
/// one larger; what's `picked` thick, or filled, in the accent.
pub(crate) fn draw_marks(p: &Painter, marks: &Marks, to: impl Fn(P3) -> Pos2, picked: &[Sub], lit: Option<P2>) {
    let seg = |[a, b]: [P3; 2]| Some([to(a), to(b)]).filter(|[a, b]| a.is_finite() && b.is_finite());
    let green = pal().context;
    for e in marks.creases.iter().filter_map(|e| seg(*e)) {
        p.line_segment(e, Stroke::new(1.0, pal().muted));
    }
    for e in marks.splits.iter().filter_map(|e| seg(*e)) {
        p.line_segment(e, Stroke::new(1.5, pal().accent));
    }
    for e in marks.rims.iter().filter_map(|e| seg(*e)) {
        p.extend(egui::Shape::dashed_line(&e, Stroke::new(1.5, green), 6.0, 4.0));
    }
    let lift = |q: P2| marks.points.iter().find(|(r, ..)| r.dist(q) <= TOL).map(|(_, at, _)| *at);
    for s in picked {
        if let Sub::Edge(a, b) = s
            && let (Some(a), Some(b)) = (lift(*a), lift(*b))
            && let Some(e) = seg([a, b])
        {
            p.line_segment(e, Stroke::new(3.0, pal().accent));
        }
    }
    for &(q, at, rim) in &marks.points {
        let c = to(at);
        if !c.is_finite() {
            continue;
        }
        let chosen = picked.iter().any(|s| matches!(s, Sub::Point(r) if r.dist(q) <= TOL));
        let lit = lit.is_some_and(|l| l.dist(q) <= TOL);
        if chosen {
            p.circle_filled(c, 5.5, pal().accent);
            p.circle_stroke(c, 5.5, Stroke::new(1.5, pal().surface));
        } else {
            p.circle_filled(c, if lit { 5.0 } else { 3.0 }, if rim { green } else { pal().accent });
        }
    }
}

/// ▲ and ▼ beside a selected point or edge at `c`, as Revit draws them to drag it up or down: where each is.
pub(crate) fn arrows(p: &Painter, c: Pos2) -> [Pos2; 2] {
    let (up, down) = (c + vec2(-12.0, -9.0), c + vec2(-12.0, 9.0));
    p.add(egui::Shape::convex_polygon(vec![up + vec2(0.0, -5.0), up + vec2(5.0, 4.0), up + vec2(-5.0, 4.0)], pal().accent, Stroke::NONE));
    p.add(egui::Shape::convex_polygon(
        vec![down + vec2(0.0, 5.0), down + vec2(-5.0, -4.0), down + vec2(5.0, -4.0)],
        pal().accent,
        Stroke::NONE,
    ));
    [up, down]
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
    pub(crate) fn shape_label(&mut self, ui: &mut Ui, anchor: Pos2, q: P2, ends: &[P2], text: &str) -> Rect {
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
                let ends = ends.to_vec();
                self.shape_heights(|_| ends, &typed);
                self.shape_refresh();
            }
        }
        at.expand2(vec2(18.0, 2.0))
    }

    /// The selected floor's points and corners `all` each raised `by` mm from where it is, in one `edit_shape`: an edge's
    /// ▲/▼ dragged in the 3D view.
    pub(crate) fn shape_raise_all(&mut self, all: &[P2], by: f64) {
        let Some((floor, f)) = self.floor_selected() else { return };
        if let Some(shape) = raised_all(&self.session.model, &f, all, by) {
            self.edit_shape(floor, &shape);
        }
        self.shape_refresh();
    }

    /// A pick under Modify Sub Elements: what it picked (a point's, or an edge's two ends) selected, or with `add` added to
    /// the selection (or taken off it, if there); nothing picked, none. The Options Bar's Elevation then reads its height.
    pub(crate) fn shape_select(&mut self, picked: impl FnOnce(&Floor) -> Vec<P2>, add: bool) {
        let Some((_, f)) = self.floor_selected() else { return };
        let sub = Sub::of(&picked(&f));
        match (sub, add) {
            (Some(s), true) => match self.shape_picked.iter().position(|o| o.same(s)) {
                Some(k) => {
                    self.shape_picked.remove(k);
                }
                None => self.shape_picked.push(s),
            },
            (Some(s), false) => self.shape_picked = vec![s],
            (None, true) => {}
            (None, false) => self.shape_picked.clear(),
        }
        self.shape_refresh();
    }

    /// How high the selection is, as the Options Bar's Elevation reads it (above the unshaped top, or with Absolute above
    /// the level): one height for all, or none when it's mixed or nothing is selected.
    pub(crate) fn shape_picked_height(&self) -> Option<f64> {
        let (_, f) = self.floor_selected()?;
        let m = &self.session.model;
        let (base, level) = (m.floor_top(&f), m.elevation(f.level));
        let read = |q: P2| m.shape_height(&f, q) + if self.shape_absolute { base.at(q) - level } else { 0.0 };
        let zs: Vec<f64> = self.shape_picked.iter().flat_map(|s| s.ends()).map(read).collect();
        let first = *zs.first()?;
        zs.iter().all(|z| (z - first).abs() < 0.5).then_some(first)
    }

    /// How high `ends` lie on floor `f`'s top, as the Options Bar's Elevation reads them: one height, or the lowest and the
    /// highest, `-30…-10`.
    pub(crate) fn heights_text(&self, f: &Floor, ends: &[P2]) -> String {
        heights_text(&self.session.model, f, ends, self.shape_absolute, 0.0)
    }

    /// The Options Bar's Elevation as the selection reads it now.
    pub(crate) fn shape_refresh(&mut self) {
        if self.tool == Tool::ModifySubElements {
            self.shape_z = self.shape_picked_height().map(|z| self.session.model.units.length(z)).unwrap_or_default();
        }
    }

    /// The Options Bar's Elevation typed, Enter: each point of the selection to it, an edge's both ends.
    pub(crate) fn shape_apply(&mut self) {
        if self.shape_picked.is_empty() {
            self.status = Status { text: t("select a point, a corner or an edge first").into(), error: true };
            return;
        }
        let ends: Vec<P2> = self.shape_picked.iter().flat_map(|s| s.ends()).collect();
        let typed = self.shape_z.clone();
        self.shape_heights(|_| ends, &typed);
        self.shape_refresh();
    }

    /// Delete under Modify Sub Elements: the selected shape points and split lines taken off, and the lines that ended at a
    /// point taken; a selected corner back at the top's height, an outline's edge left. Whether there was a selection.
    pub(crate) fn shape_delete(&mut self) -> bool {
        if self.tool != Tool::ModifySubElements || self.shape_picked.is_empty() {
            return false;
        }
        let Some((floor, f)) = self.floor_selected() else { return false };
        let corners: Vec<P2> = f.boundary.iter().chain(f.openings.iter().flatten()).copied().collect();
        let corner = |q: P2| corners.iter().any(|c| c.dist(q) <= TOL);
        let mut shape = f.shape.clone();
        let points: Vec<P2> = self.shape_picked.iter().filter_map(|s| if let Sub::Point(p) = s { Some(*p) } else { None }).collect();
        shape.points.retain(|x| !points.iter().any(|p| p.dist(x.at) <= TOL));
        // A line goes with an inner point it ended at, or when it's itself selected.
        let gone = |q: P2| !corner(q) && points.iter().any(|p| p.dist(q) <= TOL);
        let picked = self.shape_picked.clone();
        shape.lines.retain(|l| !gone(l.a) && !gone(l.b) && !picked.iter().any(|s| s.same(Sub::Edge(l.a, l.b))));
        self.shape_picked.clear();
        if shape != f.shape {
            self.edit_shape(floor, &shape);
        }
        self.shape_refresh();
        true
    }
}
