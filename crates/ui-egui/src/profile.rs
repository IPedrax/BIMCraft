//! Edit Profile, as Revit's: a straight, upright wall's shape in elevation, re-sketched on the wall's face. In a view that looks at
//! the wall (a section or an elevation whose line runs along it, or the 3D view) it is sketch mode there at once: the wall's
//! outline and holes as sketch lines on its face (`sketch::Of::Profile`, an upright `sketch3d::Surface::Profile`), deleted, moved and drawn
//! again with the Draw panel and Modify, until Finish Edit Mode sets the profile in one step. A plan can't show the shape, so
//! there Revit's Go To View asks for such a view first, and opening it starts the sketch. Reset Profile gives the wall its
//! rectangle back. The wall's ends and base stay where they are, so it still joins and its openings are measured as before
//! (`Model::check` says why a shape doesn't fit).

use std::sync::Arc;

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Id, Model};
use egui::{Button, Context, Id as UiId};

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::{BimApp, Sheet};

/// The wall's own rectangle, from its start to its end and its base to its top.
pub(crate) fn rectangle(length: f64, height: f64) -> Vec<P2> {
    vec![P2(0.0, 0.0), P2(length, 0.0), P2(length, height), P2(0.0, height)]
}

/// What a wall's profile snaps to besides its sketch's lines, in its face's axes (`[along, up]`): its base, its two ends and
/// the levels across it, each a line running past the wall.
pub(crate) fn traces(m: &Model, id: Id) -> Vec<(P2, P2)> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    let (length, base) = (w.start.dist(w.end), m.own_base(w));
    let (lo, hi) = (-length - 10_000.0, 2.0 * length + 10_000.0);
    let mut out = vec![(P2(lo, 0.0), P2(hi, 0.0)), (P2(0.0, lo), P2(0.0, hi)), (P2(length, lo), P2(length, hi))];
    out.extend(
        m.levels()
            .into_iter()
            .map(|(_, l)| l.elevation - base)
            .filter(|up| up.abs() > 1e-6 && up.abs() < 1e6)
            .map(|up| (P2(lo, up), P2(hi, up))),
    );
    out
}

/// Whether wall `id` can have its profile edited: straight, upright, unattached, not a curtain wall or in one.
pub(crate) fn profile_editable(m: &Model, id: Id) -> bool {
    m.wall(id).is_some_and(|w| {
        w.arc == 0.0
            && w.slant == 0.0
            && w.attach_top.is_none()
            && w.attach_base.is_none()
            && w.panel_of.is_none()
            && bimcraft_core::curtain::pattern(m, w).is_none()
    })
}

/// Whether view `sheet` looks at wall `id`'s face, so its profile can be sketched there: the 3D view from anywhere; a section or
/// an elevation when its line runs along the wall.
pub(crate) fn faces_wall(m: &Model, sheet: Sheet, id: Id) -> bool {
    let Some(w) = m.wall(id) else { return false };
    match sheet {
        Sheet::View3D => true,
        Sheet::Section(v) => m.section(v).is_some_and(|s| (s.end - s.start).unit().cross((w.end - w.start).unit()).abs() < 0.5),
        _ => false,
    }
}

/// Revit's Go To View for Edit Profile begun in a plan: the wall, the views that look at it, and the one picked.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GoToView {
    pub(crate) wall: Id,
    pub(crate) views: Vec<(Sheet, String)>,
    pub(crate) picked: usize,
}

/// The views wall `id`'s profile can be sketched in, by name: the sections and elevations along it, then the 3D view.
pub(crate) fn views_facing(m: &Model, id: Id) -> Vec<(Sheet, String)> {
    let mut out: Vec<(Sheet, String)> = m
        .elements
        .iter()
        .filter_map(|(v, _)| m.section(*v).map(|s| (Sheet::Section(*v), s.name.clone())))
        .filter(|(sheet, _)| faces_wall(m, *sheet, id))
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out.push((Sheet::View3D, "{3D}".to_string()));
    out
}

impl BimApp {
    /// Edit Profile on wall `id`: in a view that looks at it, sketch mode on its face with its outline and holes as lines; in a
    /// plan, Go To View first.
    pub(crate) fn edit_profile(&mut self, id: Id) {
        let m = Arc::clone(&self.session.model);
        if !profile_editable(&m, id) {
            return;
        }
        if !faces_wall(&m, self.sheet, id) {
            self.go_to_view = Some(GoToView { wall: id, views: views_facing(&m, id), picked: 0 });
            return;
        }
        self.sketch_profile(id);
    }

    /// Sketch mode on wall `id`'s face in the view now open: its profile's loops, or its rectangle, as lines.
    fn sketch_profile(&mut self, id: Id) {
        let m = Arc::clone(&self.session.model);
        let Some(w) = m.wall(id) else { return };
        let loops = if w.profile.is_empty() { vec![rectangle(w.start.dist(w.end), m.top_z(w) - m.base_z(w))] } else { w.profile.clone() };
        let lines = loops
            .iter()
            .flat_map(|l| (0..l.len()).map(move |i| bimcraft_core::sketch::SketchLine::new(l[i], l[(i + 1) % l.len()])))
            .collect();
        self.start_sketch(crate::sketch::Of::Profile(id), lines);
        self.status = crate::Status {
            text: t("Edit Profile: sketch the wall's outline, and any holes, on its face; then Finish Edit Mode").into(),
            error: false,
        };
    }

    /// Go To View, while it's open: Open View opens the view picked and sketches the wall's profile there.
    pub(crate) fn go_to_view_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.go_to_view.take() else { return };
        let (mut open, mut cancel) = (false, false);
        let r = Modal::new(UiId::new("go-to-view"), t("Go To View")).width(420.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(t("The wall's profile is sketched in a view that looks at it. Pick one to open:"));
                ui.add_space(6.0);
                for (i, (_, name)) in d.views.iter().enumerate() {
                    ui.radio_value(&mut d.picked, i, name.as_str());
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    cancel = ui.add(Button::new(t("Cancel")).min_size(egui::vec2(76.0, 0.0))).clicked();
                    open = ui.add(Button::new(t("Open View")).min_size(egui::vec2(76.0, 0.0))).clicked();
                });
            });
        });
        if open && let Some((sheet, name)) = d.views.get(d.picked).cloned() {
            self.open_sheet(sheet);
            self.sketch_profile(d.wall);
            if self.sketch.is_none() {
                self.status = crate::Status { text: tf("Couldn't sketch the profile in {}", &[&name]), error: true };
            }
            return;
        }
        if !(cancel || r.should_close()) {
            self.go_to_view = Some(d);
        }
    }
}
