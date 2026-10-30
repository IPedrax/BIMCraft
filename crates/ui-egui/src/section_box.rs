//! The 3D view's section box (fork issue 1): Properties > Extents > Section Box and BX turn it on and off, and its faces are
//! typed there; in the view, a frame round it and, in Modify, a grip on each face that a drag moves along its axis, the cut
//! shown as it goes (`orbit.rs`). What the box keeps, and where its grips are, is the core's (`view3d::box_grips`).

use bimcraft_i18n::msg;

use bimcraft_core::model::{Element, SectionBox};
use bimcraft_core::view3d::{Camera, P3, box_edges, box_grips};
use egui::{Grid, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Ui, WidgetInfo, WidgetType, vec2};
use serde_json::json;

use crate::lang::t;
use crate::style::pal;
use crate::{BimApp, Sheet};

/// Each face of the box as `box_grips` orders them, and the name its field has in Properties.
const FACES: [&str; 6] = ["West", "East", "South", "North", "Bottom", "Top"];

/// How near a grip, in points, a press takes it.
pub(crate) const GRIP: f32 = 8.0;

impl BimApp {
    /// The default 3D view's section box as the project holds it, on or off.
    pub(crate) fn section_box(&self) -> Option<SectionBox> {
        let m = &self.session.model;
        match m.default_3d().and_then(|v| m.elements.get(v)) {
            Some(Element::View3D(v)) => v.section_box,
            _ => None,
        }
    }

    /// BX and the checkbox: the box turned off if it's on, else on (fitted round the model the first time), in the 3D view.
    pub(crate) fn toggle_section_box(&mut self) {
        let on = self.section_box().is_some_and(|b| b.on);
        if self.sheet != Sheet::View3D {
            self.open_sheet(Sheet::View3D);
        }
        self.run("section_box", json!({"on": !on}));
    }

    /// The 3D view's Extents in Properties: Section Box, and while it's on where each of its faces is.
    pub(crate) fn section_box_properties(&mut self, ui: &mut Ui) {
        let group = "Extents";
        let open = !self.collapsed.contains(group);
        if crate::properties::group_header(ui, t(group), open) && !self.collapsed.remove(group) {
            self.collapsed.insert(group);
        }
        if !open {
            return;
        }
        let b = self.section_box();
        let units = self.session.model.units;
        let view = self.session.model.default_3d().unwrap_or_default();
        let mut set = None;
        egui::Frame::new().inner_margin(egui::Margin { left: 8, right: 0, top: 2, bottom: 6 }).show(ui, |ui| {
            let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            Grid::new("section-box-properties").num_columns(2).min_col_width(half).max_col_width(half).show(ui, |ui| {
                let name = t("Section Box");
                let label = ui.label(name);
                let mut on = b.is_some_and(|b| b.on);
                let r = ui.checkbox(&mut on, "");
                // A checkbox without text names itself "" for screen readers: give it its row's name.
                r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, name));
                if r.labelled_by(label.id).on_hover_text(t(TIP)).changed() {
                    set = Some(json!({"on": on}));
                }
                ui.end_row();
                let Some(b) = b.filter(|b| b.on) else { return };
                for (k, face) in FACES.into_iter().enumerate() {
                    let (axis, max) = (k / 2, !k.is_multiple_of(2));
                    let now = if max { b.max[axis] } else { b.min[axis] };
                    let label = ui.label(t(face));
                    if let Some(typed) = self.field(ui, view, &format!("section_box.{k}"), units.length(now), label.id, "") {
                        match units.parse(&typed) {
                            Some(mm) => {
                                let (mut lo, mut hi) = (b.min, b.max);
                                if max {
                                    hi[axis] = mm;
                                } else {
                                    lo[axis] = mm;
                                }
                                set = Some(json!({"min": lo, "max": hi}));
                            }
                            None => {
                                self.status = crate::Status { text: crate::lang::tf("`{}` is not a length", &[typed.trim()]), error: true }
                            }
                        }
                    }
                    ui.end_row();
                }
            });
        });
        if let Some(p) = set {
            self.run("section_box", p);
        }
    }
}

const TIP: &str = msg!("Section Box: cut the 3D view to a box, each face moved by its grip in the view or typed here (BX)");

/// The grip of section box `b` within [`GRIP`] of `at` on screen, if any, as `box_grips` orders them.
pub(crate) fn grip_at(b: (P3, P3), at: Pos2, screen: impl Fn(P3) -> Pos2) -> Option<usize> {
    box_grips(b).iter().position(|g| screen(*g).distance(at) <= GRIP)
}

/// Section box `b` drawn over the 3D view: its edges, and with `grips` an arrow on each face pointing out along its axis (a
/// square for a face square to the screen). `held` is the grip being dragged.
pub(crate) fn draw(p: &Painter, cam: &Camera, b: (P3, P3), grips: bool, held: Option<usize>, screen: impl Fn(P3) -> Pos2) {
    let edge = Stroke::new(1.0, if held.is_some() { pal().accent } else { pal().muted });
    // In perspective, what of each edge is in front of the eye.
    for [a, z] in box_edges(b).into_iter().filter_map(|[a, z]| cam.in_front(a, z)) {
        p.line_segment([screen(a), screen(z)], edge);
    }
    if !grips {
        return;
    }
    for (k, g) in box_grips(b).into_iter().enumerate() {
        let at = screen(g);
        if !at.is_finite() {
            continue;
        }
        let mut out = g;
        out[k / 2] += if k.is_multiple_of(2) { -1.0 } else { 1.0 } / cam.scale.max(1e-12);
        let d = screen(out) - at;
        let fill = if held == Some(k) { pal().accent } else { pal().surface };
        let stroke = Stroke::new(1.5, pal().accent);
        if d.length() < 0.25 {
            let r = Rect::from_center_size(at, vec2(9.0, 9.0));
            p.rect_filled(r, 1.0, fill);
            p.rect_stroke(r, 1.0, stroke, StrokeKind::Middle);
            continue;
        }
        let (u, n) = (d.normalized(), d.normalized().rot90());
        let arrow = vec![at + u * 12.0, at - u * 2.0 + n * 6.0, at - u * 2.0 - n * 6.0];
        p.add(Shape::convex_polygon(arrow, fill, stroke));
    }
}
