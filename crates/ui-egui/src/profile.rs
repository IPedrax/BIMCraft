//! Edit Profile: a straight, upright wall's shape in elevation, as Revit's. The wall is drawn to scale as seen from its exterior,
//! its doors and windows in it; its outline's points are dragged, a click on an edge adds one there, and each point's place along
//! the wall and up from its base can be typed. A hole is a loop of its own. OK puts the shape on the wall (`modify {profile}`),
//! one undo step; Reset Profile gives the wall its rectangle back. The wall's ends and base stay where they are, so it still joins
//! and its openings are measured as before (`Model::check` says why a shape doesn't fit).

use std::sync::Arc;

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, Model};
use bimcraft_core::units::Units;
use bimcraft_core::walls;
use egui::{Button, Context, Id as UiId, Pos2, Rect, Sense, Stroke, TextEdit, Ui, WidgetInfo, WidgetType, pos2, vec2};
use serde_json::json;

use crate::BimApp;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::{canvas_pal, pal};
use crate::typeprops;

/// What a dragged point lands on: the nearest 10 mm.
const SNAP: f64 = 10.0;

/// Edit Profile on one wall, while it's open.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProfileEditor {
    pub(crate) wall: Id,
    /// The loops being edited: the outline, then any holes, `[along, up]` mm.
    pub(crate) loops: Vec<Vec<P2>>,
    /// The point picked: its loop and its place in it.
    pub(crate) picked: Option<(usize, usize)>,
    /// The picked point's two places as typed, while they're typed.
    typed: [String; 2],
    length: f64,
    height: f64,
    /// The doors' and windows' rectangles, `[s0, s1, bottom, top]` up from the wall's base, to draw round.
    openings: Vec<[f64; 4]>,
    pub(crate) error: Option<String>,
}

/// The wall's own rectangle, from its start to its end and its base to its top.
pub(crate) fn rectangle(length: f64, height: f64) -> Vec<P2> {
    vec![P2(0.0, 0.0), P2(length, 0.0), P2(length, height), P2(0.0, height)]
}

/// Whether wall `id` can have its profile edited: straight, upright, unattached, not a curtain wall or in one.
pub(crate) fn editable(m: &Model, id: Id) -> bool {
    m.wall(id).is_some_and(|w| {
        w.arc == 0.0
            && w.slant == 0.0
            && w.attach_top.is_none()
            && w.attach_base.is_none()
            && w.panel_of.is_none()
            && bimcraft_core::curtain::pattern(m, w).is_none()
    })
}

fn snap(x: f64) -> f64 {
    (x / SNAP).round() * SNAP + 0.0
}

impl ProfileEditor {
    pub(crate) fn new(m: &Model, wall: Id) -> Option<ProfileEditor> {
        let w = m.wall(wall)?;
        let length = w.start.dist(w.end);
        let base = m.own_base(w);
        let (_, top) = (m.base_z(w), m.top_z(w));
        let height = top - base;
        let loops = if w.profile.is_empty() { vec![rectangle(length, height)] } else { w.profile.clone() };
        let guid = m.guid(wall);
        let openings = m
            .elements
            .values()
            .filter_map(|e| match e {
                Element::Door(o) | Element::Window(o) if o.host == guid => {
                    let h = walls::hole(m, w, e, o)?;
                    Some([h.s0, h.s1, h.bottom - base, h.top - base])
                }
                _ => None,
            })
            .collect();
        Some(ProfileEditor { wall, loops, picked: None, typed: Default::default(), length, height, openings, error: None })
    }

    /// Point `i` of loop `k`, picked, its places typed afresh.
    fn pick(&mut self, k: usize, i: usize, units: Units) {
        self.picked = Some((k, i));
        if let Some(q) = self.loops.get(k).and_then(|l| l.get(i)) {
            self.typed = [units.length(q.0), units.length(q.1)];
        }
    }

    /// A point on loop `k` after its point `i`, halfway along the edge from it.
    pub(crate) fn add_point(&mut self, k: usize, i: usize, units: Units) {
        let Some(l) = self.loops.get_mut(k) else { return };
        let n = l.len();
        if n == 0 {
            return;
        }
        let (a, b) = (l[i % n], l[(i + 1) % n]);
        l.insert(i % n + 1, P2(snap((a.0 + b.0) / 2.0), snap((a.1 + b.1) / 2.0)));
        self.pick(k, i % n + 1, units);
    }

    /// The picked point taken off its loop, while it keeps three; a hole down to none of its own goes.
    pub(crate) fn remove_point(&mut self) {
        let Some((k, i)) = self.picked else { return };
        if let Some(l) = self.loops.get_mut(k)
            && l.len() > 3
            && i < l.len()
        {
            l.remove(i);
        }
        self.picked = None;
    }

    /// A hole: a square a fifth of the wall's height, in the middle.
    pub(crate) fn add_hole(&mut self) {
        let side = snap((self.height / 5.0).min(self.length / 5.0).max(SNAP));
        let (cx, cy) = (snap(self.length / 2.0), snap(self.height / 2.0));
        let h = side / 2.0;
        self.loops.push(vec![P2(cx - h, cy - h), P2(cx + h, cy - h), P2(cx + h, cy + h), P2(cx - h, cy + h)]);
        self.picked = None;
    }

    /// The loop the picked point is on, if it's a hole.
    fn remove_hole(&mut self) {
        if let Some((k, _)) = self.picked.filter(|(k, _)| *k > 0) {
            self.loops.remove(k);
            self.picked = None;
        }
    }

    /// What's typed for the picked point put on it.
    fn read(&mut self, units: Units) -> Result<(), String> {
        let Some((k, i)) = self.picked else { return Ok(()) };
        let along = units.parse(&self.typed[0]).ok_or_else(|| tf("`{}` is not a length", &[&self.typed[0]]))?;
        let up = units.parse(&self.typed[1]).ok_or_else(|| tf("`{}` is not a length", &[&self.typed[1]]))?;
        if let Some(q) = self.loops.get_mut(k).and_then(|l| l.get_mut(i)) {
            *q = P2(along, up);
        }
        Ok(())
    }

    /// The wall to scale, from its exterior: its loops, their points to drag, an edge clicked for a new point.
    fn preview(&mut self, ui: &mut Ui, units: Units) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 340.0), Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, crate::canvas::color(bimcraft_core::render::PAPER));
        p.rect_stroke(rect, 0.0, Stroke::new(1.0, pal().border), egui::StrokeKind::Inside);
        let pts = || self.loops.iter().flatten();
        let lo = pts().fold(P2(0.0, 0.0), |a, q| P2(a.0.min(q.0), a.1.min(q.1)));
        let hi = pts().fold(P2(self.length, self.height), |a, q| P2(a.0.max(q.0), a.1.max(q.1)));
        let (w, h) = ((hi.0 - lo.0).max(1.0), (hi.1 - lo.1).max(1.0));
        let k = ((rect.width() - 60.0) as f64 / w).min((rect.height() - 60.0) as f64 / h);
        let origin = pos2(rect.center().x - (w * k / 2.0) as f32, rect.center().y + (h * k / 2.0) as f32);
        let screen = |q: P2| pos2(origin.x + ((q.0 - lo.0) * k) as f32, origin.y - ((q.1 - lo.1) * k) as f32);
        let model = |s: Pos2| P2(lo.0 + f64::from(s.x - origin.x) / k, lo.1 + f64::from(origin.y - s.y) / k);
        // The level line, and where the wall's ends are: the shape keeps to them.
        let ink = canvas_pal().text;
        p.line_segment([screen(P2(lo.0 - w * 0.05, 0.0)), screen(P2(hi.0 + w * 0.05, 0.0))], Stroke::new(1.0, canvas_pal().muted));
        for s in [0.0, self.length] {
            p.line_segment([screen(P2(s, lo.1)), screen(P2(s, hi.1))], Stroke::new(1.0, canvas_pal().muted));
        }
        for o in &self.openings {
            let r = Rect::from_two_pos(screen(P2(o[0], o[2])), screen(P2(o[1], o[3])));
            p.rect_stroke(r, 0.0, Stroke::new(1.0, canvas_pal().muted), egui::StrokeKind::Middle);
        }
        // Its outline heavier than its holes; edges only, as an outline may be concave.
        for (n, l) in self.loops.iter().enumerate() {
            let ring: Vec<Pos2> = l.iter().map(|q| screen(*q)).collect();
            p.add(egui::Shape::closed_line(ring, Stroke::new(if n == 0 { 2.0 } else { 1.5 }, ink)));
        }
        let hint = t("Drag a point; click an edge to add one there.");
        p.text(rect.center_top() + vec2(0.0, 4.0), egui::Align2::CENTER_TOP, hint, egui::FontId::proportional(11.0), canvas_pal().muted);
        // An edge clicked away from its points: under the handles, which take their own clicks.
        let edges = ui.interact(rect, UiId::new("profile-edges"), Sense::click());
        // Each point a handle, named for tests and screen readers.
        let mut picked = None;
        let mut on_handle = false;
        let mut moved = None;
        for (n, l) in self.loops.iter().enumerate() {
            for (i, q) in l.iter().enumerate() {
                let at = screen(*q);
                let r = Rect::from_center_size(at, vec2(12.0, 12.0));
                let name = format!("{} {} {}", t("Point"), n + 1, i + 1);
                let response = ui.interact(r, UiId::new(("profile-point", n, i)), Sense::click_and_drag());
                response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &name));
                on_handle |= response.hovered() || response.contains_pointer();
                let on = self.picked == Some((n, i));
                p.rect_filled(Rect::from_center_size(at, vec2(8.0, 8.0)), 0.0, if on { canvas_pal().accent } else { ink });
                if response.clicked() || response.drag_started() {
                    picked = Some((n, i));
                }
                if response.dragged()
                    && let Some(to) = response.interact_pointer_pos()
                {
                    let q = model(to);
                    moved = Some((n, i, P2(snap(q.0), snap(q.1))));
                }
            }
        }
        // An edge clicked away from its points: a point there.
        if picked.is_none()
            && !on_handle
            && edges.clicked()
            && let Some(at) = edges.interact_pointer_pos()
        {
            let near = self
                .loops
                .iter()
                .enumerate()
                .flat_map(|(n, l)| {
                    let len = l.len();
                    (0..len).map(move |i| (n, i, l[i], l[(i + 1) % len]))
                })
                .map(|(n, i, a, b)| (n, i, bimcraft_core::geom::dist_to_segment(model(at), a, b) * k))
                .min_by(|a, b| a.2.total_cmp(&b.2));
            if let Some((n, i, d)) = near
                && d < 8.0
                && let Some(l) = self.loops.get_mut(n)
            {
                let q = model(at);
                l.insert(i + 1, P2(snap(q.0), snap(q.1)));
                self.pick(n, i + 1, units);
            }
        }
        if let Some((n, i)) = picked {
            self.pick(n, i, units);
        }
        if let Some((n, i, q)) = moved
            && let Some(at) = self.loops.get_mut(n).and_then(|l| l.get_mut(i))
        {
            *at = q;
            self.typed = [units.length(q.0), units.length(q.1)];
        }
    }

    /// The picked point's places, typed; and the buttons that add and take away.
    fn side(&mut self, ui: &mut Ui, units: Units) {
        match self.picked {
            Some((n, i)) => {
                ui.strong(tf("Point {} of loop {}", &[&(i + 1).to_string(), &(n + 1).to_string()]));
                for (label, k) in [(t("Along the Wall"), 0), (t("Up from Base"), 1)] {
                    ui.horizontal(|ui| {
                        let name = ui.add_sized(vec2(110.0, 20.0), egui::Label::new(label));
                        ui.add(TextEdit::singleline(&mut self.typed[k]).desired_width(90.0)).labelled_by(name.id);
                        ui.label(units.name());
                    });
                }
            }
            None => {
                ui.label(t("Click a point to type where it goes."));
            }
        }
        ui.add_space(6.0);
        let (n, i) = self.picked.unwrap_or((0, 0));
        if ui.add(Button::new(t("Add Point"))).clicked() {
            self.add_point(n, i, units);
        }
        let removable = self.picked.is_some_and(|(n, _)| self.loops.get(n).is_some_and(|l| l.len() > 3));
        if ui.add_enabled(removable, Button::new(t("Remove Point"))).clicked() {
            self.remove_point();
        }
        if ui.add(Button::new(t("Add Hole"))).clicked() {
            self.add_hole();
        }
        if ui.add_enabled(self.picked.is_some_and(|(n, _)| n > 0), Button::new(t("Remove Hole"))).clicked() {
            self.remove_hole();
        }
        if ui.add(Button::new(t("Reset Profile"))).clicked() {
            self.loops = vec![rectangle(self.length, self.height)];
            self.picked = None;
        }
    }
}

/// Whether view `sheet` looks square at wall `id`'s face, so its profile can be sketched there: the 3D view does from anywhere it
/// isn't edge-on; a section or an elevation does when its line runs along the wall.
pub(crate) fn faces_wall(m: &Model, sheet: crate::Sheet, id: Id) -> bool {
    let Some(w) = m.wall(id) else { return false };
    match sheet {
        crate::Sheet::View3D => true,
        crate::Sheet::Section(v) => m.section(v).is_some_and(|s| (s.end - s.start).unit().cross((w.end - w.start).unit()).abs() < 0.5),
        _ => false,
    }
}

impl BimApp {
    /// Edit Profile on wall `id`: in a section or an elevation looking at it, or in the 3D view, its outline and holes as sketch
    /// lines on its face, edited with the Draw panel's tools and Modify's grips until Finish Edit Mode; in a plan, which can't
    /// show its shape, the dialog.
    pub(crate) fn edit_profile(&mut self, id: Id) {
        let m = Arc::clone(&self.session.model);
        if !profile_editable(&m, id) {
            return;
        }
        if !faces_wall(&m, self.sheet, id) {
            self.open_profile_editor(id);
            return;
        }
        let Some(e) = ProfileEditor::new(&m, id) else { return };
        let lines = e
            .loops
            .iter()
            .flat_map(|l| (0..l.len()).map(move |i| bimcraft_core::sketch::SketchLine::new(l[i], l[(i + 1) % l.len()])))
            .collect();
        self.start_sketch(crate::sketch::Of::Profile(id), lines);
        self.status = crate::Status {
            text: t("Edit Profile: drag the outline's points, draw lines for its shape and any holes, then Finish Edit Mode").into(),
            error: false,
        };
    }

    /// Edit Profile on wall `id` as a dialog, if it can have one.
    pub(crate) fn open_profile_editor(&mut self, id: Id) {
        if profile_editable(&self.session.model, id) {
            self.profile_editor = ProfileEditor::new(&self.session.model, id);
        }
    }

    /// Edit Profile, while it's open.
    pub(crate) fn profile_editor_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.profile_editor.take() else { return };
        let model = Arc::clone(&self.session.model);
        if model.wall(d.wall).is_none() {
            return;
        }
        let (mut ok, mut cancel, mut apply) = (false, false, false);
        let r = Modal::new(UiId::new("wall-profile"), t("Edit Profile")).width(860.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(250.0);
                        d.side(ui, model.units);
                    });
                    ui.vertical(|ui| {
                        // What's typed shows as it's typed; what isn't a length yet waits, and OK says so.
                        let _ = d.read(model.units);
                        d.preview(ui, model.units);
                        ui.label(tf(
                            "Wall {}, {} long; the profile runs from its start to its end and stands on its base.",
                            &[&d.wall.to_string(), &model.units.length(d.length)],
                        ));
                    });
                });
                typeprops::error(ui, d.error.as_deref());
            });
            crate::popup::footer(ui, |ui| {
                (ok, cancel, apply) = typeprops::foot(ui);
            });
        });
        if ok || apply {
            let done = match d.read(model.units) {
                Ok(()) => {
                    // The wall's own rectangle is no profile at all.
                    let plain = d.loops.len() == 1 && d.loops[0] == rectangle(d.length, d.height);
                    let profile = if plain { json!([]) } else { json!(d.loops) };
                    self.run_for_type("modify", json!({"id": d.wall, "profile": profile}), &mut d.error)
                }
                Err(e) => {
                    d.error = Some(e);
                    false
                }
            };
            ok &= done;
        }
        if !(ok || cancel || r.should_close()) {
            self.profile_editor = Some(d);
        }
    }
}

/// Whether Edit Profile is offered for wall `id`: [`editable`], under the name the ribbon asks by.
pub(crate) fn profile_editable(m: &Model, id: Id) -> bool {
    editable(m, id)
}
