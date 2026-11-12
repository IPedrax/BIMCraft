//! F7d in the family document: a reference line selected in a plan or an elevation ("Modify | Reference Lines"), Pivot on
//! it or on a selected plane and the click that pins it (a crossing of planes, a line's end, a sketch's point, else where
//! the click is), the pivot's marker, its swing range and its rotate grip, a line's end grips (T18), and the pivot's rows in
//! Properties: what it is pinned to, its Swing Angle with "=", its limits and Remove Pivot.
use super::*;
use crate::adaptive_editor::{Associating, Field, Tied, associating, equals};
use bimcraft_core::family::Kind;
use bimcraft_core::family::constraints::Edit;
use bimcraft_core::family::views::{Staged, Which};
use bimcraft_core::family::{Pivot, PivotMark, PivotPoint, Spot, Val};
use bimcraft_core::geom::P2;
use bimcraft_core::ids::Guid;
use egui::{Painter, Pos2};

/// What a grip of the family's views drags: a pivot's rotate grip (its element's GUID), or a reference line's end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Grip {
    Rotate(Guid),
    End(Guid, Spot),
}

/// How near a grip or a pivot's point a click or a press is, in pixels.
pub(super) const REACH: f32 = 9.0;

/// Pivot's and the grips' colour.
fn ink() -> Color32 {
    pal().accent
}

/// Pivots `marks` painted (`screen` puts a point of the view on screen): each a small circle with a cross; a limited one's
/// swing range a thin dashed arc; the selected element's rotate grip a round handle at its arm's end.
pub(super) fn paint(painter: &Painter, marks: &[PivotMark], screen: &dyn Fn(P2) -> Pos2, selected: Option<Guid>, turning: Option<Guid>) {
    for m in marks {
        let c = screen(m.centre);
        let stroke = Stroke::new(1.2, ink());
        if m.range.len() > 1 {
            let pts: Vec<Pos2> = m.range.iter().map(|p| screen(*p)).collect();
            painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.0, ink().gamma_multiply(0.85)), 6.0, 4.0));
        }
        painter.circle_stroke(c, 5.5, stroke);
        painter.line_segment([c - vec2(7.5, 0.0), c + vec2(7.5, 0.0)], stroke);
        painter.line_segment([c - vec2(0.0, 7.5), c + vec2(0.0, 7.5)], stroke);
        if Some(m.element) == selected
            && let Some(g) = m.grip
        {
            let at = screen(g);
            painter.line_segment([c, at], Stroke::new(1.0, ink().gamma_multiply(0.6)));
            painter.circle_filled(at, 6.0, ink());
            painter.circle_stroke(at, 6.0, Stroke::new(1.5, Color32::WHITE));
            // A small arrow round it, square to the arm: what the grip does.
            let arm = (at - c).normalized();
            let base = arm.y.atan2(arm.x);
            let r = 12.0;
            let on = |a: f32| at + vec2(a.cos(), a.sin()) * r;
            let arc: Vec<Pos2> = (0..=12).map(|k| on(base - 0.9 + 1.8 * k as f32 / 12.0)).collect();
            painter.add(egui::Shape::line(arc, Stroke::new(1.3, ink())));
            let tip = on(base + 0.9);
            let along = vec2(-(base + 0.9).sin(), (base + 0.9).cos());
            let side = vec2((base + 0.9).cos(), (base + 0.9).sin());
            painter.add(egui::Shape::convex_polygon(vec![tip + along * 4.5, tip - side * 3.5, tip + side * 3.5], ink(), Stroke::NONE));
            // Its swing, beside it while it is dragged.
            if turning == Some(m.element) {
                let text = format!("{}°", number_text(m.swing));
                let place = at + arm * 46.0;
                let galley = painter.layout_no_wrap(text, egui::FontId::proportional(13.0), ink());
                let rect = Rect::from_center_size(place, galley.size() + vec2(8.0, 4.0));
                painter.rect_filled(rect, 3.0, pal().panel);
                painter.rect_stroke(rect, 3.0, Stroke::new(1.0, ink()), egui::StrokeKind::Inside);
                painter.galley(rect.center() - galley.size() / 2.0, galley, ink());
            }
        }
    }
}

/// A reference line's end grips (squares) on screen.
pub(super) fn paint_ends(painter: &Painter, ends: &[(Grip, Pos2)]) {
    for (_, p) in ends {
        let r = Rect::from_center_size(*p, vec2(8.0, 8.0));
        painter.rect_filled(r, 1.0, ink());
        painter.rect_stroke(r, 1.0, Stroke::new(1.2, Color32::WHITE), egui::StrokeKind::Inside);
    }
}

/// The point a click at `p` (the view's mm; `screen` its place) pins a pivot to: the nearest of `targets` within reach, else
/// the click's own point.
pub(super) fn target(
    st: &Staged,
    which: Which,
    targets: &[(PivotPoint, P2)],
    q: P2,
    screen: &dyn Fn(P2) -> Pos2,
) -> (PivotPoint, [f64; 3]) {
    let at = screen(q);
    let near = targets.iter().map(|(t, v)| (t, *v, screen(*v).distance(at))).filter(|x| x.2 <= REACH).min_by(|a, b| a.2.total_cmp(&b.2));
    match near {
        Some((t, v, _)) => (*t, st.from_view(which, v)),
        None => {
            let p = st.from_view(which, q).map(|x| (x * 1000.0).round() / 1000.0);
            (PivotPoint::Fixed(p), p)
        }
    }
}

/// What a grip dragged asks: a pivot turned toward a point, or a line's end moved by so much (the family's axes).
#[derive(Clone, Copy, Debug)]
pub(super) enum GripMove {
    Rotate(Guid, [f64; 3]),
    End(Guid, Spot, [f64; 3]),
}

/// Pivot's click at `q` (the view's mm) on line or plane `g` of `flexed`: what it pins it to and where on it.
#[allow(clippy::too_many_arguments)]
pub(super) fn picked(
    flexed: &Family,
    values: &BTreeMap<String, Val>,
    st: &Staged,
    which: Which,
    g: Guid,
    q: P2,
    screen: &dyn Fn(P2) -> Pos2,
    scale: f64,
) -> Option<(PivotPoint, bimcraft_core::family::PivotAt)> {
    let targets = bimcraft_core::family::constraints::pivot_targets(flexed, values, st, which)
        .into_iter()
        .filter(|(t, _)| !t.names(g))
        .collect::<Vec<_>>();
    let (to, p) = target(st, which, &targets, q, screen);
    let at = flexed.pivot_at_near(g, p, f64::from(REACH) / scale.max(1e-9))?;
    Some((to, at))
}

impl FamilyEditor {
    /// The reference line selected, while the family has it.
    pub(crate) fn selected_line(&self) -> Option<Guid> {
        self.views.line.filter(|g| self.lines.iter().any(|l| l.guid == *g))
    }

    /// What Pivot takes: the reference line selected, or a plane of the family's own.
    pub(crate) fn pivot_candidate(&self) -> Option<Guid> {
        self.selected_line().or_else(|| self.selected_plane().filter(|g| self.planes.iter().any(|p| p.guid == *g && !p.datum())))
    }

    /// The draft's pivot on line or plane `g`.
    pub(crate) fn draft_pivot(&self, g: Guid) -> Option<&Pivot> {
        match self.lines.iter().find(|l| l.guid == g) {
            Some(l) => l.pivot.as_ref(),
            None => self.planes.iter().find(|p| p.guid == g)?.pivot.as_ref(),
        }
    }

    fn draft_pivot_slot(&mut self, g: Guid) -> Option<&mut Option<Pivot>> {
        if let Some(l) = self.lines.iter_mut().find(|l| l.guid == g) {
            return Some(&mut l.pivot);
        }
        self.planes.iter_mut().find(|p| p.guid == g).map(|p| &mut p.pivot)
    }

    /// Pivot (Modify | Reference Lines, Modify | Reference Planes): the next click in the view picks the point.
    pub(crate) fn pivot_tool(&mut self) {
        if let Some(g) = self.pivot_candidate() {
            self.views.pivoting = Some(g);
        }
    }

    /// The pivot of line or plane `g` changed by `change`, if the family takes it: else why not, nothing changed.
    pub(crate) fn change_pivot(&mut self, g: Guid, change: impl FnOnce(&mut Option<Pivot>)) -> Result<(), String> {
        let Some(slot) = self.draft_pivot_slot(g) else { return Err(t("It is no longer there.").to_string()) };
        let was = slot.clone();
        change(slot);
        let checked = self.family().and_then(|f| f.check_pivots());
        if let Err(e) = checked {
            if let Some(slot) = self.draft_pivot_slot(g) {
                *slot = was;
            }
            return Err(e);
        }
        Ok(())
    }

    /// Line or plane `g` pinned to `to` at `at` (what it held before kept: its angle, its parameter, its limits).
    pub(crate) fn set_pivot(&mut self, g: Guid, to: PivotPoint, at: bimcraft_core::family::PivotAt) -> Result<(), String> {
        self.change_pivot(g, |slot| match slot {
            Some(p) => {
                p.to = to;
                p.at = at;
            }
            None => *slot = Some(Pivot::new(to, at)),
        })
    }

    /// The pivot of line or plane `g` taken off (Remove Pivot).
    pub(crate) fn remove_pivot(&mut self, g: Guid) {
        if let Some(slot) = self.draft_pivot_slot(g) {
            *slot = None;
        }
        self.views.pivot_text = None;
    }

    /// Reference line `g` taken away; refused, saying why, while something is drawn on it. The dimensions and pivots that
    /// name it lose it.
    pub(crate) fn delete_line(&mut self, g: Guid) -> Result<(), String> {
        let mut f = self.family()?;
        let on = f.hosted_on(g);
        if !on.is_empty() {
            return Err(tf("the line can't go while {} is drawn on it", &[&on.join(", ")]));
        }
        f.lines.retain(|l| l.guid != g);
        f.forget_pivots(g);
        f.forget_reference(g);
        (self.lines, self.planes, self.dimensions) = (f.lines, f.planes, f.dimensions);
        self.views.line = None;
        Ok(())
    }

    /// A grip dragged: a pivot turned toward a point (the family's axes) through its parameter or its own angle, or a line's
    /// end moved by so much.
    pub(super) fn drag_grip(&mut self, moved: GripMove) -> Option<String> {
        let f = self.family().ok()?;
        let values = self.values();
        let edits = match moved {
            GripMove::Rotate(g, toward) => f.drag_pivot(&values, g, toward, true).map(|(_, e)| e).map_err(|e| e.message),
            GripMove::End(g, spot, by) => f.drag_line_end(&values, g, spot, by),
        };
        match edits {
            Ok(e) => {
                self.apply_edits(e);
                None
            }
            Err(e) => Some(e),
        }
    }

    /// Where the selected element's grips are on screen: a pivot's rotate grip, or an unpivoted line's ends.
    pub(super) fn grips(
        &self,
        marks: &[PivotMark],
        flexed: &Family,
        st: &Staged,
        which: Which,
        screen: &dyn Fn(P2) -> Pos2,
    ) -> Vec<(Grip, Pos2)> {
        let Some(g) = self.selected_line().or_else(|| self.selected_plane()) else { return Vec::new() };
        if let Some(m) = marks.iter().find(|m| m.element == g) {
            return m.grip.map(|p| vec![(Grip::Rotate(g), screen(p))]).unwrap_or_default();
        }
        let Some(l) = flexed.lines.iter().find(|l| l.guid == g) else { return Vec::new() };
        let (a, b) = (st.to_view(which, l.start), st.to_view(which, l.end));
        if a.dist(b) < 1e-6 {
            return Vec::new();
        }
        vec![(Grip::End(g, Spot::Start), screen(a)), (Grip::End(g, Spot::End), screen(b))]
    }

    /// What a pivot is pinned to, as Properties says it.
    fn pivot_said(&self, p: &Pivot) -> String {
        let plane = |g: Guid| -> String {
            match self.planes.iter().find(|q| q.guid == g) {
                Some(q) if !q.name.is_empty() => t(&q.name).to_string(),
                Some(_) => t("Reference Plane").to_string(),
                None => t("Reference Plane").to_string(),
            }
        };
        let line = |g: Guid| {
            self.lines.iter().position(|l| l.guid == g).map(|i| tf("Reference Line {}", &[&(i + 1).to_string()])).unwrap_or_default()
        };
        match p.to {
            PivotPoint::Crossing(a, b) => tf("{} / {}", &[&plane(a), &plane(b)]),
            PivotPoint::LineEnd(g, Spot::End) => tf("{}: End", &[&line(g)]),
            PivotPoint::LineEnd(g, _) => tf("{}: Start", &[&line(g)]),
            PivotPoint::Sketch(..) => t("Point of a sketch").to_string(),
            PivotPoint::Fixed(q) => format!("({}, {})", number_text(q[0]), number_text(q[1])),
        }
    }

    /// Properties' rows of line or plane `g`'s pivot (Constraints): what it is pinned to, its Swing Angle with "=", its
    /// minimum and maximum (blank: none) and Remove Pivot. Associate Family Parameter asked for, when "=" was pressed.
    pub(crate) fn pivot_properties(&mut self, ui: &mut Ui, g: Guid, units: bimcraft_core::units::Units) -> Option<String> {
        let _ = units;
        let mut problem = None;
        let Some(p) = self.draft_pivot(g).cloned() else {
            super::document::group(ui, msg!("Constraints"), |ui| {
                let l = ui.label(t("Pivot Point"));
                ui.label(t("None")).labelled_by(l.id);
                ui.end_row();
            });
            return None;
        };
        let values = self.values();
        let params = self.parameter_kinds();
        // What is typed, kept while this pivot is shown.
        let fresh = || {
            let n = |v: Option<f64>| v.map(number_text).unwrap_or_default();
            (g, number_text(p.angle), n(p.min), n(p.max))
        };
        if self.views.pivot_text.as_ref().is_none_or(|x| x.0 != g) {
            self.views.pivot_text = Some(fresh());
        }
        let said = self.pivot_said(&p);
        let mut text = self.views.pivot_text.clone().unwrap_or_else(fresh);
        let (mut asked, mut commit) = (None, None::<u8>);
        super::document::group(ui, msg!("Constraints"), |ui| {
            let l = ui.label(t("Pivot Point"));
            ui.label(&said).labelled_by(l.id);
            ui.end_row();
            let label = t("Swing Angle").to_string();
            let l = ui.label(&label);
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 34.0).max(40.0);
                match &p.parameter {
                    Some(name) => {
                        let mut shown = match values.get(name) {
                            Some(Val::Number(v)) => format!("{}°", number_text(*v)),
                            _ => name.clone(),
                        };
                        ui.add_enabled(false, TextEdit::singleline(&mut shown).desired_width(width)).labelled_by(l.id);
                    }
                    None => {
                        let r = ui.add(TextEdit::singleline(&mut text.1).desired_width(width)).labelled_by(l.id);
                        if r.lost_focus() {
                            commit = Some(0);
                        }
                    }
                }
                if equals(ui, &label, p.parameter.is_some()) {
                    asked = Some(associating(&g.to_string(), Field::Pivot, Kind::Angle, &label, p.parameter.as_deref(), &params));
                }
            });
            ui.end_row();
            for (k, name) in [(1u8, msg!("Minimum Angle")), (2u8, msg!("Maximum Angle"))] {
                let l = ui.label(t(name));
                let slot = if k == 1 { &mut text.2 } else { &mut text.3 };
                let r = ui.add(TextEdit::singleline(slot).desired_width(f32::INFINITY)).labelled_by(l.id);
                if r.lost_focus() {
                    commit = Some(k);
                }
                ui.end_row();
            }
        });
        self.views.pivot_text = Some(text.clone());
        if let Some(k) = commit {
            let read = |s: &str| -> Result<Option<f64>, String> {
                let s = s.trim().trim_end_matches('°').trim();
                if s.is_empty() {
                    return Ok(None);
                }
                s.parse::<f64>().map(Some).map_err(|_| tf("`{}` isn't an angle", &[s]))
            };
            let done = match k {
                0 => read(&text.1).and_then(|v| self.change_pivot(g, |slot| slot.iter_mut().for_each(|p| p.angle = v.unwrap_or(0.0)))),
                1 => read(&text.2).and_then(|v| self.change_pivot(g, |slot| slot.iter_mut().for_each(|p| p.min = v))),
                _ => read(&text.3).and_then(|v| self.change_pivot(g, |slot| slot.iter_mut().for_each(|p| p.max = v))),
            };
            if let Err(e) = done {
                problem = Some(e);
                self.views.pivot_text = None;
            }
        }
        if asked.is_some() {
            self.associating = asked;
        }
        problem
    }
}

/// Associate Family Parameter's row of a pivot: its Swing Angle, the `id` its element's GUID.
pub(super) struct PivotRows<'a> {
    pub(super) lines: &'a mut [RefLine],
    pub(super) planes: &'a mut [RefPlane],
}

impl PivotRows<'_> {
    fn pivot(&mut self, a: &Associating) -> Option<&mut Pivot> {
        let g: Guid = serde_json::from_value(json!(a.id)).ok()?;
        if let Some(l) = self.lines.iter_mut().find(|l| l.guid == g) {
            return l.pivot.as_mut();
        }
        self.planes.iter_mut().find(|p| p.guid == g)?.pivot.as_mut()
    }
}

impl Tied for PivotRows<'_> {
    fn tie(&mut self, a: &Associating, values: &BTreeMap<String, Val>) {
        let Some(p) = self.pivot(a) else { return };
        // Untied, it keeps the swing it came to as its own angle.
        if a.chosen.is_none()
            && let Some(Val::Number(v)) = p.parameter.as_ref().and_then(|n| values.get(n))
        {
            p.angle = p.clamp(*v);
        }
        p.parameter = a.chosen.clone();
    }

    fn value_now(&mut self, a: &Associating, values: &BTreeMap<String, Val>) -> String {
        let Some(p) = self.pivot(a) else { return String::new() };
        match p.parameter.as_ref().and_then(|n| values.get(n)) {
            Some(Val::Number(v)) => number_text(*v),
            _ => number_text(p.angle),
        }
    }
}

/// An edit of a pivot's own angle made to the draft.
pub(super) fn apply_angle(lines: &mut [RefLine], planes: &mut [RefPlane], e: &Edit) {
    if let Edit::PivotAngle(g, v) = e {
        let slot = match lines.iter_mut().find(|l| l.guid == *g) {
            Some(l) => l.pivot.as_mut(),
            None => planes.iter_mut().find(|p| p.guid == *g).and_then(|p| p.pivot.as_mut()),
        };
        if let Some(p) = slot {
            p.angle = *v;
        }
    }
}
