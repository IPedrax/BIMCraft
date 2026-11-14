//! The 3D view's work plane (`bimcraft_core::workplane`): the view control bar's Work Plane button names it and opens Revit's
//! Work Plane dialog (T12). Current Work Plane names it (`Level : Level 1`, `Reference Plane : Ramp`, a surface or three
//! points and how steep), with Show (its grid drawn in the 3D view while on) and Dissociate (a work plane on a reference
//! plane left where it is, no longer following it). Specify a new Work Plane: Name, a level or a named reference plane of the
//! project; Pick a plane, a face of the model (a wall's: the face under the pointer lights up); Pick a line, a wall, a beam or
//! another element drawn along a line, whose level is taken; or Pick three points (BIMCraft's own), each snapping to a corner
//! seen near the pointer as a dimension's ends do in plan; each with an offset along its normal. As Revit's, OK with a Pick
//! closes the dialog and the click (the third, for points) sets the plane; Escape brings the dialog back. The plane may lie
//! at any angle; the Floor, Ceiling, Roof and Shaft tools picked in the 3D view sketch a new one on it at once
//! (`sketch3d.rs`) when it lies level, and say it must otherwise. The plane is drawn tinted with a grid while it's being set
//! or sketched on, or Show is on, as a family's {3D} draws its own. As in ArchiCAD, it has a patch of its own in the view, a
//! small square of it (by the model's south-west corner for a level one, where it was set for another), that a click in
//! Modify opens the dialog from; the right-click menu and the Architecture tab's Datum > Set do too. Both are drawn into the
//! picture depth tested, as surfaces are (`view3d::PlaneMark`, on the processor and the card alike): what stands in front of
//! the plane hides it, a face it lies on doesn't, and a click on the patch where something hides it is that thing's.

use bimcraft_i18n::msg;

use bimcraft_core::geom::P2;
use bimcraft_core::graphics::Look;
use bimcraft_core::view3d::{Camera, Face, P3, PlaneMark, Scene};
use bimcraft_core::workplane::{self, Frame, Surface};
use egui::{Align, Color32, Context, Id as UiId, Layout, Painter, Pos2, Rect, RichText, Stroke, TextEdit, Ui, vec2};
use serde_json::json;

use crate::lang::{t, tf};
use crate::sketch::Of;
use crate::style::pal;
use crate::{BimApp, Sheet, Status, Tool};

/// How the Work Plane dialog's Specify a new Work Plane takes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum By {
    /// A level or a named reference plane, by name.
    #[default]
    Name,
    /// A face picked on the model.
    Plane,
    /// An element drawn along a line picked on the model: its level.
    Line,
    /// Three points picked on it.
    Points,
}

/// What a pick is taking on the model while the dialog makes way.
#[derive(Clone, Debug, PartialEq)]
pub enum Picking {
    Surface,
    Line,
    /// The points clicked so far.
    Points(Vec<P3>),
}

/// The Work Plane dialog's draft.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    pub by: By,
    /// What Name has chosen, as `workplane::choices` labels it.
    pub name: String,
    pub offset: String,
    /// While a pick is made on the model; the dialog is closed meanwhile.
    pub picking: Option<Picking>,
    /// Why OK was refused.
    pub error: Option<String>,
    /// Edit Work Plane: the components it puts on another plane (`edit_work_plane`); none, the 3D view's work plane.
    pub components: Vec<bimcraft_core::model::Id>,
}

/// What the dialog took for the plane: a name chosen, a level (Pick a line), a face picked (its point and way, and its
/// element), or three points.
enum Took {
    Name(String),
    Level(bimcraft_core::model::Id),
    Surface(Surface, bimcraft_core::model::Id),
    Points([P3; 3]),
}

/// What a click would pick for the work plane where the pointer is: a face, or a point (a corner's, when it snapped to one).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Aim {
    Face(Face),
    Point(P3, bool),
}

/// How wide the work plane's patch is, in points: the same on screen at any zoom.
const PATCH: f64 = 30.0;

/// Work Plane > Show's tint, in a project's 3D view, a family's {3D} and the family's plans and elevations.
pub(crate) const TINT: Color32 = Color32::from_rgba_premultiplied(30, 70, 140, 60);

/// How much room the project's 3D view leaves round the model on its work plane, mm.
const ROOM: f64 = 2000.0;

/// Patch `p` of a work plane as a 3D view draws it: tinted, with grid lines `step` apart and its outline (a family's {3D} and
/// the project's 3D view alike).
pub(crate) fn plane_mark(p: Patch, step: f64) -> PlaneMark {
    PlaneMark {
        origin: p.origin,
        axes: p.axes,
        lo: p.lo,
        hi: p.hi,
        step,
        fill: TINT.to_array(),
        grid: pal().muted.gamma_multiply(0.5).to_array(),
        edge: pal().accent.gamma_multiply(0.6).to_array(),
        edge_width: 1.0,
    }
}

/// What pick `picking` takes at pixel `at` of the 3D view through `cam` (`ppp` pixels a point): the face there, or the
/// corner seen within the snap's reach, else the point of the surface there.
pub(crate) fn aim(scene: &Scene, cam: &Camera, at: (f64, f64), picking: &Picking, look: &Look, ppp: f32) -> Option<Aim> {
    match picking {
        Picking::Surface | Picking::Line => scene.face_at(cam, at, look).map(Aim::Face),
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
        let name = match (&wp.level, &wp.ref_plane, &wp.surface) {
            (Some(_), ..) | (_, Some(_), _) => wp.name(m),
            (None, None, Some(_)) => tf("Surface, {}°", &[&slope()]),
            _ => tf("Three points, {}°", &[&slope()]),
        };
        if wp.offset == 0.0 { name } else { format!("{name} + {}", m.units.length(wp.offset)) }
    }

    /// The work plane's patch as the 3D view in `rect` shows it through `cam`, `ppp` pixels a point: its corners on screen.
    /// None without a camera or a plane.
    pub(crate) fn work_plane_ring(&self, cam: Option<Camera>, rect: Rect, ppp: f32) -> Option<[Pos2; 4]> {
        let cam = cam?;
        let extent = self.orbit.extent();
        let (plane, corner, side) = patch_square(&self.session.model, &cam, ppp, extent)?;
        let (across, up) = plane.axes();
        let at = |a: f64, b: f64| {
            let p: P3 = std::array::from_fn(|k| corner[k] + (across[k] * a + up[k] * b) * side);
            let (x, y, _) = cam.project(p);
            rect.min + vec2(x as f32 / ppp, y as f32 / ppp)
        };
        let ring = [at(0.0, 0.0), at(1.0, 0.0), at(1.0, 1.0), at(0.0, 1.0)];
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
            "Work Plane: what the 3D view sketches on: a level, a reference plane, a surface of the model or three points, each with an offset"
        );
        if crate::footer::menu_button(ui, "Work Plane", &self.work_plane_name(), tip, None).clicked() {
            self.open_work_plane();
        }
    }

    /// The Architecture tab's Set (in its Datum panel): the Work Plane dialog, in the 3D view (which it opens).
    pub(crate) fn work_plane_set(&mut self, ui: &mut Ui) {
        let tip = msg!(
            "Set Work Plane: the plane the 3D view sketches on, by name or picked on the model, with an offset; its patch in the view sets it too"
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
        let choices = workplane::choices(m);
        let current = wp.name(m);
        let by = match (&wp.surface, &wp.points, &wp.ref_plane) {
            (_, _, Some(_)) => By::Name,
            (Some(_), _, _) => By::Plane,
            (None, Some(_), _) => By::Points,
            _ => By::Name,
        };
        let name = choices.iter().find(|(_, l)| *l == current).or(choices.first()).map(|(_, l)| l.clone()).unwrap_or_default();
        self.work_plane = Some(Draft { by, name, offset: m.units.length(wp.offset), picking: None, error: None, components: Vec::new() });
    }

    /// The selection's components that Edit Work Plane can put on another plane: each of a work plane-based family placed by
    /// its insertion point (not an adaptive one); none unless the whole selection is.
    pub(crate) fn work_plane_components(&self) -> Vec<bimcraft_core::model::Id> {
        let m = &self.session.model;
        let ok = |id: &bimcraft_core::model::Id| match m.elements.get(*id) {
            Some(bimcraft_core::model::Element::Component(c)) => {
                m.families.get(&c.family).is_some_and(|f| f.work_plane_based) && !m.placed_by_points(c)
            }
            _ => false,
        };
        if !self.selection.is_empty() && self.selection.iter().all(ok) { self.selection.clone() } else { Vec::new() }
    }

    /// Edit Work Plane on the components `ids` (their contextual tab's Work Plane panel): the Work Plane dialog, naming the
    /// first one's, which puts them on a level, a named reference plane or a face picked (`edit_work_plane`).
    pub(crate) fn open_component_work_plane(&mut self, ids: Vec<bimcraft_core::model::Id>) {
        let m = &self.session.model;
        let Some(first) = ids.first().and_then(|id| m.elements.get(*id)) else { return };
        let current = m.work_plane_name_of(first);
        let choices = workplane::choices(m);
        let name = choices.iter().find(|(_, l)| *l == current).or(choices.first()).map(|(_, l)| l.clone()).unwrap_or_default();
        self.work_plane = Some(Draft { name, offset: String::new(), components: ids, ..Draft::default() });
    }

    /// The Work Plane dialog, while it's open and not picking.
    pub(crate) fn work_plane_window(&mut self, ctx: &Context) {
        let m = std::sync::Arc::clone(&self.session.model);
        let wp = m.work_plane(m.default_3d());
        let components = self.work_plane.as_ref().map(|d| d.components.clone()).unwrap_or_default();
        // A surface said by which way it faces, the rest as the control bar's button says it; a component's, as its
        // Properties' Work Plane does.
        let current = match (&wp.surface, &wp.ref_plane, &wp.level) {
            _ if !components.is_empty() => {
                components.first().and_then(|id| m.elements.get(*id)).map(|e| m.work_plane_name_of(e)).unwrap_or_default()
            }
            (Some(sf), None, None) => {
                let name = tf("Surface, {}", &[&facing(sf.normal)]);
                if wp.offset == 0.0 { name } else { format!("{name} + {}", m.units.length(wp.offset)) }
            }
            _ => self.work_plane_name(),
        };
        let follows = wp.ref_plane.is_some() && components.is_empty();
        let view = components.is_empty();
        let show_on = self.show_work_plane;
        let Some(d) = self.work_plane.as_mut().filter(|d| d.picking.is_none()) else { return };
        let choices = workplane::choices(&m);
        let (mut ok, mut close, mut show, mut dissociate) = (false, false, false, false);
        let r = crate::popup::Modal::new(UiId::new("work-plane"), t("Work Plane")).width(480.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(RichText::new(t("Current Work Plane")).strong());
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    let l = ui.label(t("Name:"));
                    ui.label(RichText::new(&current).strong()).labelled_by(l.id);
                });
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    let tip = msg!("Show: the work plane drawn as a grid in the 3D view, on or off");
                    show = ui.add(egui::Button::new(t("Show")).selected(show_on)).on_hover_text(t(tip)).clicked();
                    let tip = msg!("Dissociate: the work plane left where its reference plane is now, no longer following it");
                    let off = if view {
                        msg!("Dissociate: only a work plane on a reference plane follows it; this one follows nothing")
                    } else {
                        msg!("Dissociate: a component stays on its host, which it moves with")
                    };
                    dissociate = ui
                        .add_enabled(follows, egui::Button::new(t("Dissociate")))
                        .on_hover_text(t(tip))
                        .on_disabled_hover_text(t(off))
                        .clicked();
                });
                ui.add_space(8.0);
                ui.label(RichText::new(t("Specify a new Work Plane")).strong());
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.radio_value(&mut d.by, By::Name, t("Name"));
                    let mut picked = None;
                    egui::ComboBox::from_id_salt("work-plane-name").selected_text(d.name.clone()).width(300.0).show_ui(ui, |ui| {
                        for (_, label) in &choices {
                            if ui.selectable_label(d.name == *label, label).clicked() {
                                picked = Some(label.clone());
                            }
                        }
                    });
                    if let Some(n) = picked {
                        (d.name, d.by) = (n, By::Name);
                    }
                });
                for (by, label, tip) in [
                    (
                        By::Plane,
                        msg!("Pick a plane"),
                        msg!("Pick a plane: after OK, click a face of the model in the 3D view, a wall's or a roof's"),
                    ),
                    (
                        By::Line,
                        msg!("Pick a line and use the work plane it was sketched in"),
                        msg!(
                            "Pick a line: after OK, click a wall, a beam, a pipe or another element drawn along a line; its level is taken"
                        ),
                    ),
                    (
                        By::Points,
                        msg!("Pick three points"),
                        msg!("Pick three points: after OK, click three points on the model, each snapping to a corner near the pointer"),
                    ),
                ] {
                    // A component goes on a level, a reference plane or a face: no plane through three points.
                    if by == By::Points && !view {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.radio_value(&mut d.by, by, t(label)).on_hover_text(t(tip));
                    });
                }
                ui.add_space(6.0);
                // A component keeps its Offset from Host.
                if view {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        let label = ui.label(t("Offset"));
                        ui.add(TextEdit::singleline(&mut d.offset).desired_width(90.0)).labelled_by(label.id);
                        ui.label(t("along its normal: up from a level, out from a surface"));
                    });
                }
                if let Some(e) = &d.error {
                    ui.add_space(4.0);
                    ui.colored_label(pal().error, e);
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
                });
            });
        });
        if show {
            self.show_work_plane = !self.show_work_plane;
        }
        if dissociate {
            let refused = match self.run("work_plane", json!({"dissociate": true})) {
                Some(_) => None,
                None => Some(std::mem::take(&mut self.status).text),
            };
            if let Some(d) = &mut self.work_plane {
                d.error = refused;
            }
        } else if ok {
            self.apply_work_plane();
        } else if close || r.should_close() {
            self.work_plane = None;
        }
    }

    /// The offset typed, in mm: none typed, 0.
    fn typed_offset(&self) -> Result<f64, String> {
        let Some(d) = &self.work_plane else { return Ok(0.0) };
        if d.offset.trim().is_empty() {
            return Ok(0.0);
        }
        self.session.model.units.parse(&d.offset).ok_or_else(|| tf("`{}` is not a length", &[d.offset.trim()]))
    }

    /// OK: by name, the plane set at once; with a Pick, the dialog makes way for the clicks, which set it. What's wrong is
    /// said in the dialog.
    fn apply_work_plane(&mut self) {
        let Some(d) = &self.work_plane else { return };
        let (by, name) = (d.by, d.name.clone());
        if let Err(e) = self.typed_offset() {
            if let Some(d) = &mut self.work_plane {
                d.error = Some(e);
            }
            return;
        }
        let picking = match by {
            By::Name => return self.take(Took::Name(name)),
            By::Plane => Picking::Surface,
            By::Line => Picking::Line,
            By::Points => Picking::Points(Vec::new()),
        };
        let first = match picking {
            Picking::Surface => PICK_SURFACE,
            Picking::Line => PICK_LINE,
            Picking::Points(_) => PICK_FIRST,
        };
        if let Some(d) = &mut self.work_plane {
            d.picking = Some(picking);
            d.error = None;
        }
        if self.sheet != Sheet::View3D {
            self.open_sheet(Sheet::View3D);
        }
        self.status = Status { text: t(first).into(), error: false };
    }

    /// What the dialog took, set: the 3D view's work plane (`work_plane`, with the offset typed), or the components' (Edit
    /// Work Plane, `edit_work_plane`). The dialog closes when it's taken, and comes back with the reason when it isn't.
    fn take(&mut self, took: Took) {
        let offset = self.typed_offset().unwrap_or(0.0);
        let components = self.work_plane.as_ref().map(|d| d.components.clone()).unwrap_or_default();
        let (command, params) = if components.is_empty() {
            let params = match took {
                Took::Name(n) => json!({"name": n, "offset": offset}),
                Took::Level(l) => json!({"level": l, "offset": offset}),
                Took::Surface(s, _) => json!({"surface": s, "offset": offset}),
                Took::Points(p) => json!({"points": p, "offset": offset}),
            };
            ("work_plane", params)
        } else {
            let m = &self.session.model;
            let host = match took {
                Took::Name(n) => workplane::by_name(m, &n).map(|wp| match (wp.ref_plane, wp.level.and_then(|g| m.elements.id(g))) {
                    (Some(g), _) => json!({"plane": g}),
                    (None, l) => json!({"level": l}),
                }),
                Took::Level(l) => Ok(json!({"level": l})),
                Took::Surface(s, owner) => match m.face_role(owner, s.point, s.normal) {
                    Some(role) => Ok(json!({"face": {"element": owner, "face": role.name()}})),
                    None => {
                        self.status = Status { text: t(NOT_A_FACE).into(), error: true };
                        return;
                    }
                },
                Took::Points(_) => return,
            };
            match host {
                Ok(host) => ("edit_work_plane", json!({"ids": components, "host": host})),
                Err(e) => {
                    if let Some(d) = &mut self.work_plane {
                        (d.picking, d.error) = (None, Some(e));
                    }
                    return;
                }
            }
        };
        match self.run(command, params) {
            Some(_) => {
                self.work_plane = None;
                self.status = Status::default();
            }
            None => {
                let e = std::mem::take(&mut self.status).text;
                if let Some(d) = &mut self.work_plane {
                    d.picking = None;
                    d.error = Some(e);
                }
            }
        }
    }

    /// A click while a Pick waits, with what it aimed at (none where nothing of the model is under it): the face, the line's
    /// level, or the points gathered; the plane set with the offset typed once it has what it needs.
    pub(crate) fn work_plane_pick(&mut self, aimed: Option<Aim>) {
        if self.work_plane_picking().is_none() {
            return;
        }
        let Some(aimed) = aimed else {
            self.status = Status { text: t("Click on the model: a point of its surface.").into(), error: true };
            return;
        };
        let Some(d) = &mut self.work_plane else { return };
        let Some(picking) = &mut d.picking else { return };
        let took = match (picking, aimed) {
            (Picking::Surface, Aim::Face(face)) => {
                Took::Surface(Surface { point: face.at.map(f64::round), normal: face.normal }, face.owner)
            }
            (Picking::Line, Aim::Face(face)) => {
                let m = &self.session.model;
                match workplane::sketched_in(m, face.owner).and_then(|wp| wp.level).and_then(|g| m.elements.id(g)) {
                    Some(level) => Took::Level(level),
                    None => {
                        self.status = Status { text: t(NOT_A_LINE).into(), error: true };
                        return;
                    }
                }
            }
            (Picking::Points(picked), Aim::Point(at, _)) => {
                picked.push(at);
                if let [a, b, c] = picked[..] {
                    Took::Points([a, b, c])
                } else {
                    let next = if picked.len() == 1 { PICK_SECOND } else { PICK_THIRD };
                    self.status = Status { text: t(next).into(), error: false };
                    return;
                }
            }
            _ => return,
        };
        self.take(took);
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
const PICK_LINE: &str =
    msg!("Click a wall, a beam, a pipe or another element drawn along a line: its level is the work plane. Esc goes back to the dialog.");
const NOT_A_LINE: &str = msg!("Pick a line: a wall, a beam, a pipe or another element drawn along a line.");
const NOT_A_FACE: &str = msg!("Pick a plane: a component goes on a face of a wall, a floor, a ceiling or a roof.");
const LEVEL_ONLY: &str = msg!(
    "The work plane must be horizontal: a floor, a ceiling, a roof or a shaft is sketched on a level plane. Set the Work Plane (the 3D view's control bar) on a level, a level surface or three points at one height."
);

/// Whether `at` on screen is on the patch `ring`.
pub(crate) fn on_patch(ring: &[Pos2; 4], at: Pos2) -> bool {
    let ring: Vec<P2> = ring.iter().map(|p| P2(f64::from(p.x), f64::from(p.y))).collect();
    bimcraft_core::geom::contains(&ring, P2(f64::from(at.x), f64::from(at.y)))
}

/// Model `m`'s 3D view's work plane's patch, a square of the plane `PATCH` points wide through `cam` (`ppp` pixels a point):
/// on a level one by the model's south-west corner, half its width clear of it; on another, about the point it was set
/// through. The plane, and the patch's corner from which it runs `side` mm along the plane's axes. None without a plane.
/// The work plane's patch: its plane, its corner and its side. `extent` is the box round what the 3D view shows, its scene's
/// ([`crate::orbit::Orbit::extent`]): working it out from the model made every solid of it again, every frame.
fn patch_square(m: &bimcraft_core::model::Model, cam: &Camera, ppp: f32, extent: Option<(P3, P3)>) -> Option<(Frame, P3, f64)> {
    let plane = m.work_plane(m.default_3d()).resolve(m).ok()?;
    let side = PATCH * f64::from(ppp) / cam.scale.max(1e-12);
    let (across, up) = plane.axes();
    let (start, from) = if workplane::is_level(&plane) {
        let corner = extent.map_or([0.0; 3], |(lo, _)| lo);
        (plane.nearest(corner), -1.5)
    } else {
        (plane.origin, -0.5)
    };
    let corner = std::array::from_fn(|k| start[k] + (across[k] + up[k]) * from * side);
    Some((plane, corner, side))
}

/// What model `m`'s 3D view through `cam` (`ppp` pixels a point) draws of its work plane into its picture: the plane tinted
/// with its grid (with `plane`: while it's being set or sketched on, or Show is on), on `on` when a new outline is sketched
/// on another, and its patch (with `square`: in Modify), lit while the pointer is on it (`lit`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn marks(
    m: &bimcraft_core::model::Model,
    cam: &Camera,
    ppp: f32,
    plane: bool,
    on: Option<Frame>,
    square: bool,
    lit: bool,
    extent: Option<(P3, P3)>,
) -> Vec<PlaneMark> {
    let mut marks = Vec::new();
    if plane
        && let Some(extent) = extent
        && let Ok(frame) = m.work_plane(m.default_3d()).resolve(m)
    {
        let frame = on.unwrap_or(frame);
        let middle: P3 = std::array::from_fn(|k| (extent.0[k] + extent.1[k]) / 2.0);
        if let Some(p) = patch(frame, extent, ROOM, middle) {
            let step = workplane::grid_step((p.hi.0 - p.lo.0).max(p.hi.1 - p.lo.1));
            marks.push(plane_mark(p, step));
        }
    }
    if square && let Some((frame, corner, side)) = patch_square(m, cam, ppp, extent) {
        marks.push(PlaneMark {
            origin: corner,
            axes: frame.axes(),
            lo: (0.0, 0.0),
            hi: (side, side),
            step: 0.0,
            fill: pal().accent.gamma_multiply(if lit { 0.45 } else { 0.2 }).to_array(),
            grid: [0; 4],
            edge: pal().accent.to_array(),
            edge_width: 1.5 * f64::from(ppp),
        });
    }
    marks
}

/// What the work plane's patch, `ring` on screen, says while the pointer is on it: what the plane is (`name`). The patch itself
/// is drawn into the picture ([`marks`]).
pub(crate) fn name_patch(painter: &Painter, ring: [Pos2; 4], name: &str) {
    let right = ring.iter().fold(ring[0], |a, p| if p.x > a.x { *p } else { a });
    crate::canvas::tag(painter, right + egui::vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, tf("Work Plane: {}", &[name]));
}

/// The patch of a plane a work plane is drawn over: from `lo` to `hi` along its `axes` (across it and up it) from `origin`.
pub(crate) struct Patch {
    pub(crate) origin: P3,
    pub(crate) axes: (P3, P3),
    pub(crate) lo: (f64, f64),
    pub(crate) hi: (f64, f64),
}

/// The patch of plane `p` as much of it as box `(lo, hi)` spans laid on it and `room` mm round, measured from the point of it
/// nearest `about`; none for a box with no size or none at all.
pub(crate) fn patch(p: Frame, (lo, hi): (P3, P3), room: f64, about: P3) -> Option<Patch> {
    let (across, up) = p.axes();
    let o = p.nearest(about);
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
    (span.is_finite() && span > 0.0).then_some(Patch { origin: o, axes: (across, up), lo: a, hi: b })
}
