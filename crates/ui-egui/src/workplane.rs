//! The 3D view's work plane (`bimcraft_core::workplane`): the view control bar's Work Plane button names it and opens a dialog
//! that sets it, on a level with an offset or through three points, typed or picked on the model (Pick in View closes the
//! dialog for the picks and brings it back with them). The Floor, Ceiling, Roof and Shaft tools picked in the 3D view
//! sketch a new one on it at once (`sketch3d.rs`), and the plane is drawn as a grid while it's being set or sketched on. As in
//! ArchiCAD, the plane has a patch of its own in the view, a small square of it by the model's south-west corner, that a click
//! in Modify opens the dialog from; the right-click menu and the Architecture tab's Work Plane > Set do too.

use bimcraft_i18n::msg;

use bimcraft_core::geom::P2;
use bimcraft_core::model::Id;
use bimcraft_core::view3d::Camera;
use bimcraft_core::view3d::P3;
use bimcraft_core::walls::Plane;
use bimcraft_core::workplane;
use egui::{Align, Context, Id as UiId, Layout, Modal, Painter, Pos2, Rect, RichText, Stroke, TextEdit, Ui, vec2};
use serde_json::json;

use crate::lang::{t, tf};
use crate::sketch::Of;
use crate::style::pal;
use crate::{BimApp, Sheet, Status, Tool};

/// The Work Plane dialog's draft.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    /// Through three points, else on a level.
    pub by_points: bool,
    pub level: Id,
    pub offset: String,
    /// Each point's x, y and z, as typed.
    pub points: [[String; 3]; 3],
    /// While Pick in View takes the points on the model: those clicked so far. The dialog is closed meanwhile.
    pub picking: Option<Vec<P3>>,
    /// Why OK was refused.
    pub error: Option<String>,
}

const AXES: [&str; 3] = ["X", "Y", "Z"];

/// How wide the work plane's patch is, in points: the same on screen at any zoom.
const PATCH: f64 = 30.0;

impl BimApp {
    /// What the 3D view sketches on, for the button: the level and its offset, or three points and their slope.
    pub(crate) fn work_plane_name(&self) -> String {
        let m = &self.session.model;
        let wp = m.work_plane(m.default_3d());
        match wp.level.and_then(|g| m.level(g)) {
            Some(l) if wp.offset == 0.0 => l.name.clone(),
            Some(l) => format!("{} + {}", l.name, m.units.length(wp.offset)),
            None => tf("Three points, {}°", &[&format!("{:.1}", wp.slope(m))]),
        }
    }

    /// The work plane's patch as the 3D view in `rect` shows it through `cam`, `ppp` pixels a point: its corners on screen.
    /// A square of the plane by the model's south-west corner, half its width clear of it; none without a camera or a plane.
    pub(crate) fn work_plane_ring(&self, cam: Option<Camera>, rect: Rect, ppp: f32) -> Option<[Pos2; 4]> {
        let cam = cam?;
        let m = &self.session.model;
        let surface = m.work_plane(m.default_3d()).resolve(m).ok()?;
        let corner = m.model_box().map_or(P2(0.0, 0.0), |(lo, _)| P2(lo[0], lo[1]));
        let side = PATCH * f64::from(ppp) / cam.scale.max(1e-12);
        let at = P2(corner.0 - side * 1.5, corner.1 - side * 1.5);
        let screen = |q: P2| {
            let (x, y, _) = cam.project([q.0, q.1, surface.at(q)]);
            rect.min + vec2(x as f32 / ppp, y as f32 / ppp)
        };
        let ring = [at, P2(at.0 + side, at.1), P2(at.0 + side, at.1 + side), P2(at.0, at.1 + side)].map(screen);
        ring.iter().all(|p| p.x.is_finite() && p.y.is_finite()).then_some(ring)
    }

    /// The middle of the work plane's patch on screen, as the 3D view last showed it.
    #[cfg(test)]
    pub(crate) fn work_plane_patch(&self) -> Option<Pos2> {
        let o = &self.orbit;
        let ring = self.work_plane_ring(o.camera, o.rect, o.ppp)?;
        Some(Pos2::new(ring.iter().map(|p| p.x).sum::<f32>() / 4.0, ring.iter().map(|p| p.y).sum::<f32>() / 4.0))
    }

    /// The view control bar's Work Plane: what the 3D view sketches on; a click opens the dialog that sets it.
    pub(crate) fn work_plane_button(&mut self, ui: &mut Ui) {
        let tip = msg!(
            "Work Plane: what the 3D view sketches floors, ceilings, roofs and shafts on: a level, raised by an offset, or three points"
        );
        if crate::footer::menu_button(ui, "Work Plane", &self.work_plane_name(), tip, None).clicked() {
            self.open_work_plane();
        }
    }

    /// The Architecture tab's Set (in its Datum panel): the Work Plane dialog, in the 3D view (which it opens).
    pub(crate) fn work_plane_set(&mut self, ui: &mut Ui) {
        let tip = msg!(
            "Set Work Plane: the plane the 3D view sketches on, a level raised by an offset or three points; its patch in the view sets it too"
        );
        if crate::big_button(ui, crate::icons::Icon::WorkPlane, msg!("Set"), self.work_plane.is_some(), tip).clicked() {
            if self.sheet != Sheet::View3D {
                self.open_sheet(Sheet::View3D);
            }
            self.open_work_plane();
        }
    }

    /// The Work Plane dialog, filled with the view's plane as it is.
    pub(crate) fn open_work_plane(&mut self) {
        let m = &self.session.model;
        let wp = m.work_plane(m.default_3d());
        let units = m.units;
        let lowest = m.levels().first().map_or(0, |(id, _)| *id);
        let points = wp.points.unwrap_or_default().map(|p| p.map(|v| units.length(v)));
        self.work_plane = Some(Draft {
            by_points: wp.points.is_some(),
            level: wp.level.and_then(|g| m.elements.id(g)).unwrap_or(lowest),
            offset: units.length(wp.offset),
            points,
            picking: None,
            error: None,
        });
    }

    /// The Work Plane dialog, while it's open and not picking.
    pub(crate) fn work_plane_window(&mut self, ctx: &Context) {
        let m = std::sync::Arc::clone(&self.session.model);
        let Some(d) = self.work_plane.as_mut().filter(|d| d.picking.is_none()) else { return };
        let (mut ok, mut close, mut pick) = (false, false, false);
        let r = Modal::new(UiId::new("work-plane")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new(t("Work Plane")).heading());
            ui.label(t("What the 3D view sketches on."));
            ui.add_space(6.0);
            ui.radio_value(&mut d.by_points, false, t("On a level"));
            ui.add_enabled_ui(!d.by_points, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    let label = ui.label(t("Level"));
                    let shown = m.level(d.level).map(|l| l.name.clone()).unwrap_or_default();
                    egui::ComboBox::from_id_salt("work-plane-level")
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            for (id, l) in m.levels() {
                                ui.selectable_value(&mut d.level, id, &l.name);
                            }
                        })
                        .response
                        .labelled_by(label.id);
                    let label = ui.label(t("Offset"));
                    ui.add(TextEdit::singleline(&mut d.offset).desired_width(90.0)).labelled_by(label.id);
                });
            });
            ui.radio_value(&mut d.by_points, true, t("Through three points"));
            ui.add_enabled_ui(d.by_points, |ui| {
                egui::Grid::new("work-plane-points").num_columns(4).show(ui, |ui| {
                    for (k, p) in d.points.iter_mut().enumerate() {
                        ui.label(tf("Point {}", &[&(k + 1).to_string()]));
                        for (axis, v) in AXES.iter().zip(p.iter_mut()) {
                            let name = tf("Point {} {}", &[&(k + 1).to_string(), axis]);
                            let r = ui.add(TextEdit::singleline(v).desired_width(80.0).hint_text(*axis));
                            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, &name));
                        }
                        ui.end_row();
                    }
                });
            });
            // Picking chooses three points, so it works from either choice.
            let tip = msg!("Pick in View: click three points on the model in the 3D view; the dialog comes back with them");
            pick = ui.button(t("Pick in View")).on_hover_text(t(tip)).clicked();
            if let Some(e) = &d.error {
                ui.add_space(4.0);
                ui.colored_label(pal().error, e);
            }
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                close = ui.button(t("Cancel")).clicked();
                ok = ui.button(t("OK")).clicked();
            });
        });
        if pick {
            d.by_points = true;
            d.picking = Some(Vec::new());
            if self.sheet != Sheet::View3D {
                self.open_sheet(Sheet::View3D);
            }
            self.status = Status { text: t(PICK_FIRST).into(), error: false };
        } else if ok {
            self.apply_work_plane();
        } else if close || r.should_close() {
            self.work_plane = None;
        }
    }

    /// OK: the plane set as the dialog says; or what's wrong said in it.
    fn apply_work_plane(&mut self) {
        let Some(d) = &self.work_plane else { return };
        let units = self.session.model.units;
        let length = |text: &str| units.parse(text).ok_or_else(|| tf("`{}` is not a length", &[text.trim()]));
        let params = if d.by_points {
            let points: Result<Vec<Vec<f64>>, String> = d.points.iter().map(|p| p.iter().map(|v| length(v)).collect()).collect();
            points.map(|points| json!({"points": points}))
        } else {
            let offset = if d.offset.trim().is_empty() { Ok(0.0) } else { length(&d.offset) };
            offset.map(|offset| json!({"level": d.level, "offset": offset}))
        };
        let refused = match params {
            Err(e) => Some(e),
            Ok(p) => match self.run("work_plane", p) {
                Some(_) => None,
                None => Some(std::mem::take(&mut self.status).text),
            },
        };
        match refused {
            None => {
                self.work_plane = None;
                self.status = Status::default();
            }
            Some(e) => {
                if let Some(d) = &mut self.work_plane {
                    d.error = Some(e);
                }
            }
        }
    }

    /// Pick in View, at the model point `at` (none where nothing is under the click): the points gathered, and with the
    /// third the dialog back with them in it.
    pub(crate) fn work_plane_pick(&mut self, at: Option<P3>) {
        let units = self.session.model.units;
        let Some(d) = &mut self.work_plane else { return };
        let Some(picked) = &mut d.picking else { return };
        let Some(at) = at else {
            self.status = Status { text: t("Click on the model: a point of its surface.").into(), error: true };
            return;
        };
        picked.push(at.map(|v| v.round()));
        if picked.len() < 3 {
            let next = if picked.len() == 1 { PICK_SECOND } else { PICK_THIRD };
            self.status = Status { text: t(next).into(), error: false };
            return;
        }
        let points = std::mem::take(picked);
        for (slot, p) in d.points.iter_mut().zip(&points) {
            *slot = p.map(|v| units.length(v));
        }
        d.picking = None;
        d.by_points = true;
        d.error = None;
        self.status = Status::default();
    }

    /// The points Pick in View has taken so far, while it is picking.
    pub(crate) fn work_plane_picks(&self) -> Option<&[P3]> {
        self.work_plane.as_ref()?.picking.as_deref()
    }

    /// Esc while Pick in View picks: the dialog back as it was.
    pub(crate) fn stop_work_plane_pick(&mut self) -> bool {
        match &mut self.work_plane {
            Some(d) if d.picking.is_some() => {
                d.picking = None;
                self.status = Status::default();
                true
            }
            _ => false,
        }
    }

    /// Floor, Ceiling, Roof or Shaft picked in the 3D view: a new one sketched on its work plane at once. Each lies level, so
    /// a sloped plane is refused, saying the plane must be horizontal.
    pub(crate) fn sketch_on_work_plane(&mut self) {
        let m = &self.session.model;
        let surface = match m.work_plane(m.default_3d()).resolve(m) {
            Ok(p) => p,
            Err(e) => {
                self.status = Status { text: e, error: true };
                self.tool = Tool::Modify;
                return;
            }
        };
        let Some((level, _)) = workplane::host(m, surface, P2(0.0, 0.0)) else {
            self.status = Status { text: t("the model has no levels").into(), error: true };
            self.tool = Tool::Modify;
            return;
        };
        let of = match self.tool {
            Tool::Floor => Of::Floor(level),
            Tool::Ceiling => Of::Ceiling(level),
            Tool::Roof => Of::Roof(level),
            Tool::Shaft(_) => Of::Shaft(level),
            _ => return,
        };
        if !workplane::is_level(&surface) {
            self.tool = Tool::Modify;
            self.status = Status { text: t(LEVEL_ONLY).into(), error: true };
            return;
        }
        self.start_sketch(of, Vec::new());
        if let Some(sk) = &mut self.sketch {
            sk.surface = Some(surface);
        }
    }
}

const PICK_FIRST: &str = msg!("Click the work plane's first point on the model. Esc goes back to the dialog.");
const PICK_SECOND: &str = msg!("Click the work plane's second point.");
const PICK_THIRD: &str = msg!("Click the work plane's third point.");
const LEVEL_ONLY: &str = msg!(
    "The work plane must be horizontal: a floor, a ceiling, a roof or a shaft is sketched on a level plane. Set the Work Plane (the 3D view's control bar) on a level, or through three points at one height."
);

/// Whether `at` on screen is on the patch `ring`.
pub(crate) fn on_patch(ring: &[Pos2; 4], at: Pos2) -> bool {
    let ring: Vec<P2> = ring.iter().map(|p| P2(f64::from(p.x), f64::from(p.y))).collect();
    bimcraft_core::geom::contains(&ring, P2(f64::from(at.x), f64::from(at.y)))
}

/// The work plane's patch, `ring` on screen: lit while `hovered`, when it also says what the plane is (`name`).
pub(crate) fn draw_patch(painter: &Painter, ring: [Pos2; 4], hovered: bool, name: &str) {
    let fill = pal().accent.gamma_multiply(if hovered { 0.45 } else { 0.2 });
    painter.add(egui::Shape::convex_polygon(ring.to_vec(), fill, Stroke::new(1.5, pal().accent)));
    if hovered {
        let right = ring.iter().fold(ring[0], |a, p| if p.x > a.x { *p } else { a });
        crate::canvas::tag(painter, right + egui::vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, tf("Work Plane: {}", &[name]));
    }
}

/// Plane `p` drawn as a light grid over the plan's `extent` (its corners, mm), `screen` placing its points.
pub(crate) fn draw_plane(painter: &Painter, p: Plane, [lo, hi]: [P2; 2], screen: impl Fn(P3) -> Pos2) {
    let span = (hi.0 - lo.0).max(hi.1 - lo.1);
    if !(span.is_finite() && span > 0.0) {
        return;
    }
    // A metre apart, or as many as fit in twenty.
    let step = (span / 20.0).max(1000.0);
    let stroke = Stroke::new(1.0, pal().muted.gamma_multiply(0.5));
    let at = |x: f64, y: f64| screen([x, y, p.at(P2(x, y))]);
    let lines = |n: f64| (0..=(n.ceil() as usize).min(40)).map(move |k| k as f64 * step);
    for d in lines((hi.0 - lo.0) / step) {
        let x = (lo.0 + d).min(hi.0);
        painter.line_segment([at(x, lo.1), at(x, hi.1)], stroke);
    }
    for d in lines((hi.1 - lo.1) / step) {
        let y = (lo.1 + d).min(hi.1);
        painter.line_segment([at(lo.0, y), at(hi.0, y)], stroke);
    }
    let ring = [at(lo.0, lo.1), at(hi.0, lo.1), at(hi.0, hi.1), at(lo.0, hi.1)];
    painter.add(egui::Shape::closed_line(ring.to_vec(), Stroke::new(1.0, pal().accent.gamma_multiply(0.6))));
}
