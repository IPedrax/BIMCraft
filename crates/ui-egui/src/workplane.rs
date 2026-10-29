//! The 3D view's work plane (`bimcraft_core::workplane`): the view control bar's Work Plane button names it and opens a dialog
//! that sets it three ways, each with an offset along its normal: a level picked from the list, a surface picked on the model
//! (a wall's face: the face under the pointer lights up), or three points picked on it, each snapping to a corner seen near
//! the pointer as a dimension's ends do in plan. A pick closes the dialog while it's made and brings it back with what was
//! picked. The plane may lie at any angle; the Floor, Ceiling, Roof and Shaft tools picked in the 3D view sketch a new one on
//! it at once (`sketch3d.rs`) when it lies level, and say it must otherwise. The plane is drawn as a grid while it's being set
//! or sketched on. As in ArchiCAD, it has a patch of its own in the view, a small square of it (by the model's south-west
//! corner for a level one, where it was picked for another), that a click in Modify opens the dialog from; the right-click
//! menu and the Architecture tab's Datum > Set do too.

use bimcraft_i18n::msg;

use bimcraft_core::geom::P2;
use bimcraft_core::graphics::Look;
use bimcraft_core::model::Id;
use bimcraft_core::view3d::{Camera, Face, P3, Scene};
use bimcraft_core::workplane::{self, Frame, Surface};
use egui::{Align, Context, Id as UiId, Layout, Modal, Painter, Pos2, Rect, RichText, Stroke, TextEdit, Ui, vec2};
use serde_json::json;

use crate::lang::{t, tf};
use crate::sketch::Of;
use crate::style::pal;
use crate::{BimApp, Sheet, Status, Tool};

/// How the Work Plane dialog sets the plane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum By {
    /// A level, picked from the list.
    #[default]
    Level,
    /// A surface picked on the model.
    Surface,
    /// Three points picked on it.
    Points,
}

/// What Pick in View is taking on the model while the dialog makes way.
#[derive(Clone, Debug, PartialEq)]
pub enum Picking {
    Surface,
    /// The points clicked so far.
    Points(Vec<P3>),
}

/// The Work Plane dialog's draft.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    pub by: By,
    pub level: Id,
    pub offset: String,
    /// The surface and the points picked, as last picked (or as the plane was set).
    pub surface: Option<Surface>,
    pub points: Option<[P3; 3]>,
    /// While a pick is made on the model; the dialog is closed meanwhile.
    pub picking: Option<Picking>,
    /// Why OK was refused.
    pub error: Option<String>,
}

/// What a click would pick for the work plane where the pointer is: a face, or a point (a corner's, when it snapped to one).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Aim {
    Face(Face),
    Point(P3, bool),
}

/// How wide the work plane's patch is, in points: the same on screen at any zoom.
const PATCH: f64 = 30.0;

/// What pick `picking` takes at pixel `at` of the 3D view through `cam` (`ppp` pixels a point): the face there, or the
/// corner seen within the snap's reach, else the point of the surface there.
pub(crate) fn aim(scene: &Scene, cam: &Camera, at: (f64, f64), picking: &Picking, look: &Look, ppp: f32) -> Option<Aim> {
    match picking {
        Picking::Surface => scene.face_at(cam, at, look).map(Aim::Face),
        Picking::Points(_) => match scene.corner_near(cam, at, crate::canvas::SNAP * f64::from(ppp), look) {
            Some(corner) => Some(Aim::Point(corner, true)),
            None => scene.face_at(cam, at, look).map(|f| Aim::Point(f.at.map(f64::round), false)),
        },
    }
}

/// What `aim` would pick, drawn: the face lit, or the point marked (a square on a corner, as a snap in plan).
pub(crate) fn draw_aim(painter: &Painter, aim: &Aim, screen: impl Fn(P3) -> Pos2) {
    match aim {
        Aim::Face(face) => {
            let fill = pal().accent.gamma_multiply(0.3);
            for t in &face.tris {
                painter.add(egui::Shape::convex_polygon(t.map(&screen).to_vec(), fill, Stroke::NONE));
            }
            painter.circle_filled(screen(face.at), 3.0, pal().accent);
        }
        Aim::Point(p, true) => {
            let r = Rect::from_center_size(screen(*p), vec2(10.0, 10.0));
            painter.rect_stroke(r, 0.0, Stroke::new(2.0, pal().accent), egui::StrokeKind::Middle);
        }
        Aim::Point(p, false) => {
            painter.circle_stroke(screen(*p), 4.0, Stroke::new(1.5, pal().accent));
        }
    }
}

impl BimApp {
    /// What the 3D view sketches on, for the button: the level, or a surface or three points and how steep, and the offset.
    pub(crate) fn work_plane_name(&self) -> String {
        let m = &self.session.model;
        let wp = m.work_plane(m.default_3d());
        let slope = || format!("{:.0}", wp.slope(m));
        let name = match wp.level.and_then(|g| m.level(g)) {
            Some(l) => l.name.clone(),
            None if wp.surface.is_some() => tf("Surface, {}°", &[&slope()]),
            None => tf("Three points, {}°", &[&slope()]),
        };
        if wp.offset == 0.0 { name } else { format!("{name} + {}", m.units.length(wp.offset)) }
    }

    /// The work plane's patch as the 3D view in `rect` shows it through `cam`, `ppp` pixels a point: its corners on screen.
    /// A square of the plane: on a level one by the model's south-west corner, half its width clear of it; on another,
    /// about the point it was set through. None without a camera or a plane.
    pub(crate) fn work_plane_ring(&self, cam: Option<Camera>, rect: Rect, ppp: f32) -> Option<[Pos2; 4]> {
        let cam = cam?;
        let m = &self.session.model;
        let plane = m.work_plane(m.default_3d()).resolve(m).ok()?;
        let side = PATCH * f64::from(ppp) / cam.scale.max(1e-12);
        let (across, up) = plane.axes();
        let (start, from) = if workplane::is_level(&plane) {
            let corner = m.model_box().map_or([0.0; 3], |(lo, _)| lo);
            (plane.nearest(corner), -1.5)
        } else {
            (plane.origin, -0.5)
        };
        let at = |a: f64, b: f64| {
            let p: P3 = std::array::from_fn(|k| start[k] + (across[k] * a + up[k] * b) * side);
            let (x, y, _) = cam.project(p);
            rect.min + vec2(x as f32 / ppp, y as f32 / ppp)
        };
        let ring = [at(from, from), at(from + 1.0, from), at(from + 1.0, from + 1.0), at(from, from + 1.0)];
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
        let tip = msg!("Work Plane: what the 3D view sketches on: a level, a surface of the model or three points, each with an offset");
        if crate::footer::menu_button(ui, "Work Plane", &self.work_plane_name(), tip, None).clicked() {
            self.open_work_plane();
        }
    }

    /// The Architecture tab's Set (in its Datum panel): the Work Plane dialog, in the 3D view (which it opens).
    pub(crate) fn work_plane_set(&mut self, ui: &mut Ui) {
        let tip = msg!(
            "Set Work Plane: the plane the 3D view sketches on, a level, a surface or three points, with an offset; its patch in the view sets it too"
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
        let lowest = m.levels().first().map_or(0, |(id, _)| *id);
        let by = match (&wp.surface, &wp.points) {
            (Some(_), _) => By::Surface,
            (None, Some(_)) => By::Points,
            _ => By::Level,
        };
        self.work_plane = Some(Draft {
            by,
            level: wp.level.and_then(|g| m.elements.id(g)).unwrap_or(lowest),
            offset: m.units.length(wp.offset),
            surface: wp.surface,
            points: wp.points,
            picking: None,
            error: None,
        });
    }

    /// The Work Plane dialog, while it's open and not picking.
    pub(crate) fn work_plane_window(&mut self, ctx: &Context) {
        let m = std::sync::Arc::clone(&self.session.model);
        let Some(d) = self.work_plane.as_mut().filter(|d| d.picking.is_none()) else { return };
        let (mut ok, mut close, mut pick) = (false, false, None);
        let r = Modal::new(UiId::new("work-plane")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new(t("Work Plane")).heading());
            ui.label(t("What the 3D view sketches on."));
            ui.add_space(6.0);
            ui.radio_value(&mut d.by, By::Level, t("Pick from list"));
            ui.add_enabled_ui(d.by == By::Level, |ui| {
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
                });
            });
            ui.radio_value(&mut d.by, By::Surface, t("Pick a surface"));
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                let tip = msg!("Pick Surface: click a face of the model in the 3D view, a wall's or a roof's; the dialog comes back with it");
                if ui.button(t("Pick Surface")).on_hover_text(t(tip)).clicked() {
                    pick = Some(Picking::Surface);
                }
                let said = d.surface.map(|s| facing(s.normal));
                ui.label(said.unwrap_or_else(|| t("none picked yet").into()));
            });
            ui.radio_value(&mut d.by, By::Points, t("Pick three points"));
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                let tip = msg!(
                    "Pick Points: click three points on the model in the 3D view, each snapping to a corner near the pointer; the dialog comes back with them"
                );
                if ui.button(t("Pick Points")).on_hover_text(t(tip)).clicked() {
                    pick = Some(Picking::Points(Vec::new()));
                }
                let said = d.points.and_then(|p| workplane::through(&p).ok()).map(|f| tf("{}° from level", &[&format!("{:.0}", f.slope())]));
                ui.label(said.unwrap_or_else(|| t("none picked yet").into()));
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let label = ui.label(t("Offset"));
                ui.add(TextEdit::singleline(&mut d.offset).desired_width(90.0)).labelled_by(label.id);
                ui.label(t("along its normal: up from a level, out from a surface"));
            });
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
        if let Some(picking) = pick {
            let first = if picking == Picking::Surface { PICK_SURFACE } else { PICK_FIRST };
            d.by = if picking == Picking::Surface { By::Surface } else { By::Points };
            d.picking = Some(picking);
            if self.sheet != Sheet::View3D {
                self.open_sheet(Sheet::View3D);
            }
            self.status = Status { text: t(first).into(), error: false };
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
        let offset = if d.offset.trim().is_empty() {
            Ok(0.0)
        } else {
            units.parse(&d.offset).ok_or_else(|| tf("`{}` is not a length", &[d.offset.trim()]))
        };
        let params = offset.and_then(|offset| match d.by {
            By::Level => Ok(json!({"level": d.level, "offset": offset})),
            By::Surface => d.surface.map(|s| json!({"surface": s, "offset": offset})).ok_or_else(|| t(NO_SURFACE).to_string()),
            By::Points => d.points.map(|p| json!({"points": p, "offset": offset})).ok_or_else(|| t(NO_POINTS).to_string()),
        });
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

    /// A click while Pick in View picks, with what it aimed at (none where nothing of the model is under it): the surface,
    /// or the points gathered, and with the third the dialog back with them in it.
    pub(crate) fn work_plane_pick(&mut self, aimed: Option<Aim>) {
        let Some(d) = &mut self.work_plane else { return };
        let Some(picking) = &mut d.picking else { return };
        let Some(aimed) = aimed else {
            self.status = Status { text: t("Click on the model: a point of its surface.").into(), error: true };
            return;
        };
        match (picking, aimed) {
            (Picking::Surface, Aim::Face(face)) => d.surface = Some(Surface { point: face.at.map(f64::round), normal: face.normal }),
            (Picking::Points(picked), Aim::Point(at, _)) => {
                picked.push(at);
                if let [a, b, c] = picked[..] {
                    d.points = Some([a, b, c]);
                } else {
                    let next = if picked.len() == 1 { PICK_SECOND } else { PICK_THIRD };
                    self.status = Status { text: t(next).into(), error: false };
                    return;
                }
            }
            _ => return,
        }
        d.picking = None;
        d.error = None;
        self.status = Status::default();
    }

    /// What Pick in View is taking, while it picks.
    pub(crate) fn work_plane_picking(&self) -> Option<&Picking> {
        self.work_plane.as_ref()?.picking.as_ref()
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
    /// a plane that doesn't is refused, saying the plane must be horizontal.
    pub(crate) fn sketch_on_work_plane(&mut self) {
        let m = &self.session.model;
        let plane = match m.work_plane(m.default_3d()).resolve(m) {
            Ok(p) => p,
            Err(e) => {
                self.status = Status { text: e, error: true };
                self.tool = Tool::Modify;
                return;
            }
        };
        let Some(surface) = plane.over_plan().filter(|_| workplane::is_level(&plane)) else {
            self.tool = Tool::Modify;
            self.status = Status { text: t(LEVEL_ONLY).into(), error: true };
            return;
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
        self.start_sketch(of, Vec::new());
        if let Some(sk) = &mut self.sketch {
            sk.surface = Some(surface);
        }
    }
}

/// Which way a surface faces, said as the dialog says it: up, down, or the compass point it looks to and how steep.
fn facing(n: P3) -> String {
    let plane = Frame { origin: [0.0; 3], normal: n };
    let slope = plane.slope();
    if slope < 0.5 {
        return t(if n[2] > 0.0 { msg!("facing up") } else { msg!("facing down") }).into();
    }
    const WAYS: [&str; 8] = [
        msg!("east"),
        msg!("north-east"),
        msg!("north"),
        msg!("north-west"),
        msg!("west"),
        msg!("south-west"),
        msg!("south"),
        msg!("south-east"),
    ];
    let turn = n[1].atan2(n[0]).to_degrees().rem_euclid(360.0);
    let way = WAYS[((turn / 45.0).round() as usize) % 8];
    tf("facing {}, {}° from level", &[t(way), &format!("{slope:.0}")])
}

const PICK_SURFACE: &str = msg!("Click a face of the model for the work plane. Esc goes back to the dialog.");
const PICK_FIRST: &str =
    msg!("Click the work plane's first point on the model; it snaps to a corner near the pointer. Esc goes back to the dialog.");
const PICK_SECOND: &str = msg!("Click the work plane's second point.");
const PICK_THIRD: &str = msg!("Click the work plane's third point.");
const NO_SURFACE: &str = msg!("Pick a surface first: Pick Surface, then a click on a face of the model.");
const NO_POINTS: &str = msg!("Pick three points first: Pick Points, then three clicks on the model.");
const LEVEL_ONLY: &str = msg!(
    "The work plane must be horizontal: a floor, a ceiling, a roof or a shaft is sketched on a level plane. Set the Work Plane (the 3D view's control bar) on a level, a level surface or three points at one height."
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

/// Plane `p` drawn as a light grid over as much of it as the model's box `(lo, hi)` spans and `room` mm round, `screen`
/// placing its points.
pub(crate) fn draw_plane(painter: &Painter, p: Frame, (lo, hi): (P3, P3), room: f64, screen: impl Fn(P3) -> Pos2) {
    let (across, up) = p.axes();
    let middle: P3 = std::array::from_fn(|k| (lo[k] + hi[k]) / 2.0);
    let o = p.nearest(middle);
    // The box's corners laid on the plane, as far across and up it as they go.
    let (mut a, mut b) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    for k in 0..8 {
        let c: P3 = std::array::from_fn(|i| if k >> i & 1 == 0 { lo[i] } else { hi[i] });
        let d: P3 = std::array::from_fn(|i| c[i] - o[i]);
        let (u, v) = (d.iter().zip(across).map(|(x, y)| x * y).sum::<f64>(), d.iter().zip(up).map(|(x, y)| x * y).sum::<f64>());
        (a, b) = ((a.0.min(u), a.1.min(v)), (b.0.max(u), b.1.max(v)));
    }
    let (a, b) = ((a.0 - room, a.1 - room), (b.0 + room, b.1 + room));
    let span = (b.0 - a.0).max(b.1 - a.1);
    if !(span.is_finite() && span > 0.0) {
        return;
    }
    // A metre apart, or as many as fit in twenty.
    let step = (span / 20.0).max(1000.0);
    let stroke = Stroke::new(1.0, pal().muted.gamma_multiply(0.5));
    let at = |u: f64, v: f64| screen(std::array::from_fn(|k| o[k] + across[k] * u + up[k] * v));
    let lines = |n: f64| (0..=(n.ceil() as usize).min(40)).map(move |k| k as f64 * step);
    for d in lines((b.0 - a.0) / step) {
        let u = (a.0 + d).min(b.0);
        painter.line_segment([at(u, a.1), at(u, b.1)], stroke);
    }
    for d in lines((b.1 - a.1) / step) {
        let v = (a.1 + d).min(b.1);
        painter.line_segment([at(a.0, v), at(b.0, v)], stroke);
    }
    let ring = [at(a.0, a.1), at(b.0, a.1), at(b.0, b.1), at(a.0, b.1)];
    painter.add(egui::Shape::closed_line(ring.to_vec(), Stroke::new(1.0, pal().accent.gamma_multiply(0.6))));
}
