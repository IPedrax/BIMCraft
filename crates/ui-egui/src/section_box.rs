//! The 3D view's section box (fork issue 1): Properties > Extents > Section Box and BX turn it on and off, and its faces are
//! typed there; in the view, a frame round it and, in Modify, a grip on each face that a drag moves along its axis, the cut
//! shown as it goes (`orbit.rs`). What the box keeps, and where its grips are, is the core's (`view3d::box_grips`). Linked 3D
//! View floats the 3D view beside a plan and cuts it, for now, to what the plan shows.

use bimcraft_i18n::msg;

use bimcraft_core::model::{Element, Id, MAX_COORD, MIN_SECTION_BOX, Model, SectionBox};
use bimcraft_core::render::View;
use bimcraft_core::view3d::{Camera, P3, box_edges, box_grips};
use egui::{Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};
use serde_json::json;

use crate::dock::VIEW_SIZE;
use crate::lang::t;
use crate::style::pal;
use crate::{BimApp, Sheet};

/// Each face of the box as `box_grips` orders them, and the name its field has in Properties.
const FACES: [&str; 6] = ["West", "East", "South", "North", "Bottom", "Top"];

/// How near a grip, in points, a press takes it.
pub(crate) const GRIP: f32 = 8.0;

/// Where the 3D view was before Linked 3D View floated it beside the plan, so it goes back there when the link ends: closed, a
/// tab at this place, or floating already.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Was {
    Closed,
    Tab(usize),
    Floating,
}

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
        crate::properties::grid(ui, "section-box-properties", |ui| {
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
                        None => self.status = crate::Status { text: crate::lang::tf("`{}` is not a length", &[typed.trim()]), error: true },
                    }
                }
                ui.end_row();
            }
        });
        if let Some(p) = set {
            self.run("section_box", p);
        }
    }

    /// Linked 3D View, on a plan's view control bar: the 3D view floats beside the plan, cut to what the plan shows
    /// ([`BimApp::follow_plan`]); again, it goes back where it was (closed, its tab, floating) and is drawn as the project has
    /// it, whose own section box the link never changed.
    pub(crate) fn toggle_linked_3d(&mut self) {
        let three_d = Sheet::View3D;
        if let Some(was) = self.linked_3d.take() {
            if let Some(t) = self.temporary.get_mut(&three_d) {
                t.section_box = None;
            }
            let back = self.sheet;
            match was {
                Was::Closed => self.close_view(three_d),
                Was::Tab(at) => {
                    self.dock_view(three_d, at);
                    self.open_sheet(back);
                }
                Was::Floating => {}
            }
            return;
        }
        let plan @ Sheet::Plan(_) = self.sheet else { return };
        let was = match self.views.iter().position(|v| *v == three_d) {
            _ if self.floats(three_d) => Was::Floating,
            Some(at) => Was::Tab(at),
            None => Was::Closed,
        };
        if was != Was::Floating {
            // Over the plan's top right corner, clear of its tabs; the plan stays the view the tools work in.
            let r = self.view_rect;
            self.open_sheet(three_d);
            self.float_view(three_d, pos2((r.right() - VIEW_SIZE.x - 16.0).max(r.left()), r.top() + 40.0));
            self.open_sheet(plan);
        }
        self.linked_3d = Some(was);
    }

    /// While Linked 3D View is on and a plan is the active view, the 3D view cut to what the plan shows ([`plan_box`]), set
    /// when the hand rests (no button down, no wheel turning), so a pan is followed when it ends, not at each of its frames.
    /// The box is the window's, as Temporary Hide/Isolate is: a `section_box` on the copy of the model the 3D view draws
    /// (`footer.rs`), never the project's, saved or undone. Closing the 3D view ends the link.
    pub(crate) fn follow_plan(&mut self, ui: &Ui) {
        let three_d = Sheet::View3D;
        if self.linked_3d.is_none() || self.passive {
            return;
        }
        if !self.floats(three_d) && !self.views.contains(&three_d) {
            self.linked_3d = None;
            if let Some(t) = self.temporary.get_mut(&three_d) {
                t.section_box = None;
            }
            return;
        }
        let Sheet::Plan(key) = self.sheet else { return };
        if ui.input(|i| i.pointer.any_down() || i.smooth_scroll_delta != Vec2::ZERO) {
            return;
        }
        if let Some(b) = plan_box(&self.session.model, key, self.canvas.view(key)) {
            self.temporary.entry(three_d).or_default().section_box = Some(b);
        }
    }
}

/// What plan `key` shows, as a section box: its level, from its view range's bottom (the level, unless the range says) up to
/// its top (over the highest level, which has none, the model's top), across the part of the plan `view` shows. In usable
/// numbers, with room on each axis, so `section_box` takes it.
fn plan_box(m: &Model, key: Id, view: Option<View>) -> Option<([f64; 3], [f64; 3])> {
    let v = view?;
    let (a, b) = (v.mm((0.0, 0.0)), v.mm((v.width as f32, v.height as f32)));
    let r = m.plan_range(key);
    let top = if r.top.is_finite() { r.top } else { m.model_box().map_or(r.cut, |(_, hi)| hi[2]) };
    let sides = [(a.0.min(b.0), a.0.max(b.0)), (a.1.min(b.1), a.1.max(b.1)), (r.bottom, top)];
    if sides.iter().any(|(lo, hi)| !lo.is_finite() || !hi.is_finite()) {
        return None;
    }
    let [x, y, z] = sides.map(|(lo, hi)| {
        let lo = lo.clamp(-MAX_COORD, MAX_COORD - MIN_SECTION_BOX);
        (lo, hi.clamp(lo + MIN_SECTION_BOX, MAX_COORD))
    });
    Some(([x.0, y.0, z.0], [x.1, y.1, z.1]))
}

const TIP: &str = msg!("Section Box: cut the 3D view to a box, each face moved by its grip in the view or typed here (BX)");

/// The grip of section box `b` within [`GRIP`] of `at` on screen, if any, as `box_grips` orders them.
pub(crate) fn grip_at(b: (P3, P3), at: Pos2, screen: impl Fn(P3) -> Pos2) -> Option<usize> {
    box_grips(b).iter().position(|g| screen(*g).distance(at) <= GRIP)
}

/// The colours of grip `k`, its fill and its outline: as it is, under the pointer, and held.
pub(crate) fn grip_colours(k: usize, held: Option<usize>, hot: Option<usize>) -> (egui::Color32, egui::Color32) {
    if held == Some(k) {
        (pal().accent, pal().accent)
    } else if hot == Some(k) {
        (pal().hover, pal().hover)
    } else {
        (pal().surface, pal().accent)
    }
}

/// Section box `b` drawn over the 3D view: its edges, and with `grips` an arrow on each face pointing out along its axis (a
/// square for a face square to the screen). `held` is the grip being dragged, and `hot` the one under the pointer.
pub(crate) fn draw(
    p: &Painter,
    cam: &Camera,
    b: (P3, P3),
    grips: bool,
    (held, hot): (Option<usize>, Option<usize>),
    screen: impl Fn(P3) -> Pos2,
) {
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
        let (fill, outline) = grip_colours(k, held, hot);
        let stroke = Stroke::new(1.5, outline);
        if d.length() < 0.25 {
            let r = Rect::from_center_size(at, vec2(9.0, 9.0));
            p.rect_filled(r, 1.0, fill);
            p.rect_stroke(r, 1.0, stroke, StrokeKind::Middle);
            continue;
        }
        arrow(p, at, d.normalized(), fill, stroke);
    }
}

/// An arrow at `at` pointing `u`, a unit on screen: a section box's face's, a wall's top's.
pub(crate) fn arrow(p: &Painter, at: Pos2, u: Vec2, fill: egui::Color32, stroke: Stroke) {
    let n = u.rot90();
    p.add(Shape::convex_polygon(vec![at + u * 12.0, at - u * 2.0 + n * 6.0, at - u * 2.0 - n * 6.0], fill, stroke));
}
