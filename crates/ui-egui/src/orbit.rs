//! The 3D view: the core's picture of the model (`view3d::Scene`), orbited, panned and zoomed as
//! in Revit. Shift and the middle button orbit, and so does a drag with the primary button (for
//! a hand without a middle button); the middle button pans; the wheel zooms about the cursor. A
//! click picks the element the picture shows there, and so does a press let go quickly, however
//! far the hand went with it (`Canvas::quick_press`): the camera it turned goes back. A level
//! selected alone has a grip at the middle of each edge of its plane, which a drag moves over it. A section box is drawn
//! round what it keeps, with a grip on each face in Modify that a drag moves along its axis (`section_box.rs`).

use std::sync::Arc;

use bimcraft_core::geom::{self, P2};
use bimcraft_core::graphics::{Look, VisualStyle};
use bimcraft_core::model::{Component, Element, Id, Model};
use bimcraft_core::render::{self, Rgb};
use bimcraft_core::view3d::gpu::{Hit, Mesh};
use bimcraft_core::view3d::{AZIMUTH, Camera, ELEVATION, Image, MAX_SIDE, P3, Scene, SurfaceHit, move_box_face};
use egui::{
    Align, Color32, ColorImage, Layout, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, UiBuilder,
    pos2, vec2,
};
use serde_json::json;

use crate::canvas::{click_modifiers, color};
use crate::icons::Icon;
use crate::style::{canvas_pal, pal};
use crate::{BimApp, Sheet, Status, Tool, icon_button};

/// What a picture is of: the model, the camera, the elements tinted, whether the theme is dark, whether the card drew it, and
/// the section box it is cut to while one is dragged.
type Shot = (Arc<Model>, Camera, Vec<(Id, Rgb)>, bool, bool, Option<(P3, P3)>);

/// A model's mesh for the card, or why the card can't draw it.
type Meshed = (Arc<Model>, Result<Arc<Mesh>, String>);

/// What picks in the picture on screen: the processor's own picture, or the mesh the card drew it from (with its tints).
enum Picker {
    Image(Image),
    Mesh(Arc<Mesh>, Vec<(Id, Rgb)>),
}

impl Picker {
    /// What the picture shows at pixel `(x, y)`, taken through `cam`.
    fn at(&self, cam: &Camera, x: f64, y: f64) -> Option<Hit> {
        match self {
            Picker::Image(i) => Some(Hit { id: i.pick(x, y)?, face: i.face(x, y).unwrap_or("") }),
            Picker::Mesh(m, tints) => m.pick(cam, tints, x, y),
        }
    }
}

/// The 3D view between frames.
pub struct Orbit {
    /// `None` until the view first shows, which fits it to the model.
    pub(crate) camera: Option<Camera>,
    /// Where the view showed last, in points, and the pixels a point holds.
    pub(crate) rect: Rect,
    ppp: f32,
    /// The scene of the model shown last, in the 3D view or a section's.
    pub(crate) scene: Option<(Arc<Model>, Scene)>,
    /// What picks in the picture on screen, and what the picture is of.
    picker: Option<Picker>,
    shown: Option<Shot>,
    /// The processor's picture as a texture; and whether the card's is the one on screen.
    texture: Option<TextureHandle>,
    pub(crate) on_card: bool,
    /// The mesh of the scene shown last, for the card: or why it can't draw it.
    mesh: Option<Meshed>,
    /// A drag under way: whether it orbits (else it pans), and where the pointer was last.
    drag: Option<(bool, Pos2)>,
    mount_drag: Option<Id>,
    /// The camera as that drag found it, for a drag that turns out a click.
    before: Option<Camera>,
    /// A selected level's grip being dragged: the level, and which edge of its plane (as
    /// `geom::edge_grips` orders them).
    grip: Option<(Id, usize)>,
    /// A section box's grip being dragged: which face (as `view3d::box_grips` orders them), and where the drag began.
    boxing: Option<(usize, Pos2)>,
}

impl Default for Orbit {
    fn default() -> Orbit {
        Orbit {
            camera: None,
            rect: Rect::ZERO,
            ppp: 1.0,
            scene: None,
            picker: None,
            shown: None,
            texture: None,
            on_card: false,
            mesh: None,
            drag: None,
            mount_drag: None,
            before: None,
            grip: None,
            boxing: None,
        }
    }
}

/// Where model point `p` shows on screen, in a view in `rect` through `cam`, `ppp` pixels a point.
fn on_screen(cam: &Camera, rect: Rect, ppp: f32, p: [f64; 3]) -> Pos2 {
    let (x, y, _) = cam.project(p);
    rect.min + vec2(x as f32 / ppp, y as f32 / ppp)
}

fn micron(mm: f64) -> f64 {
    (mm * 1000.0).round() / 1000.0
}

fn equipment_pose(model: &Model, mut component: Component, hit: &SurfaceHit) -> Component {
    component.at = P2(micron(hit.point[0]), micron(hit.point[1]));
    let [nx, ny, _] = hit.normal;
    let local_z = match hit.face {
        "exterior" | "interior" => {
            component.angle = nx.atan2(-ny).to_degrees();
            model
                .families
                .get(&component.family)
                .and_then(|family| family.values(&component.type_name, &component.params).ok())
                .and_then(|values| match values.get(model.language.word("Mounting Height")) {
                    Some(bimcraft_core::family::Val::Number(height)) => Some(*height),
                    _ => None,
                })
                .unwrap_or_default()
        }
        "top" => model.component_local_pieces(&component).iter().map(|p| p.bottom).min_by(f64::total_cmp).unwrap_or_default(),
        _ => model.component_local_pieces(&component).iter().map(|p| p.top).max_by(f64::total_cmp).unwrap_or_default(),
    };
    component.offset = micron(hit.point[2] - model.elevation(component.level) - local_z);
    component
}

pub(crate) fn rgb(c: Color32) -> Rgb {
    Rgb(c.r(), c.g(), c.b())
}

/// The scene of `model` in `slot`, built again when the model changed.
pub(crate) fn scene_of<'a>(slot: &'a mut Option<(Arc<Model>, Scene)>, model: &Arc<Model>) -> &'a Scene {
    if !slot.as_ref().is_some_and(|(m, _)| Arc::ptr_eq(m, model)) {
        *slot = None;
    }
    &slot.get_or_insert_with(|| (Arc::clone(model), Scene::new(model))).1
}

/// The mesh of `scene` for the card, built again when the model changed; or why the card can't draw it.
fn mesh_of<'a>(slot: &'a mut Option<Meshed>, model: &Arc<Model>, scene: &Scene) -> &'a Result<Arc<Mesh>, String> {
    if !slot.as_ref().is_some_and(|(m, _)| Arc::ptr_eq(m, model)) {
        *slot = None;
    }
    &slot.get_or_insert_with(|| (Arc::clone(model), Mesh::new(scene, &Look::of_3d(model)).map(Arc::new))).1
}

impl BimApp {
    pub(crate) fn toggle_design_view(&mut self) {
        if self.sheet == Sheet::View3D {
            if let Some(level) = self.placement_level() {
                self.open_sheet(Sheet::Plan(level));
            }
        } else {
            if let Some(level) = self.plan_level() {
                self.work_level = Some(self.session.model.guid(level));
            }
            self.open_sheet(Sheet::View3D);
        }
    }

    pub(crate) fn placement_level(&self) -> Option<Id> {
        let m = &self.session.model;
        self.plan_level()
            .or_else(|| {
                self.work_level
                    .and_then(|g| m.elements.id(g))
                    .filter(|id| matches!(m.elements.get(*id), Some(bimcraft_core::model::Element::Level(_))))
            })
            .or_else(|| {
                self.views.iter().rev().find_map(|view| match view {
                    Sheet::Plan(key) | Sheet::Ceiling(key) => m.plan_level(*key),
                    _ => None,
                })
            })
            .or_else(|| m.levels().first().map(|(id, _)| *id))
    }

    pub(crate) fn workplane_options(&mut self, ui: &mut Ui) {
        if self.sheet == Sheet::View3D
            && matches!(self.tool, Tool::Move(_) | Tool::Run("wire" | "cablecarrier", _) | Tool::Split | Tool::SplitRoute3D { .. })
        {
            let label = ui.label(crate::lang::t(bimcraft_i18n::msg!("Work Plane")));
            let planes = ["YZ", "XZ", "XY"];
            egui::ComboBox::from_id_salt("3d-move-plane")
                .selected_text(planes[self.work_axis])
                .show_ui(ui, |ui| {
                    for (axis, plane) in planes.into_iter().enumerate() {
                        if ui.selectable_value(&mut self.work_axis, axis, plane).changed() {
                            self.tool = match self.tool {
                                Tool::Run(category, _) => Tool::Run(category, None),
                                Tool::Move(_) => Tool::Move(None),
                                _ => Tool::Split,
                            };
                        }
                    }
                })
                .response
                .labelled_by(label.id);
            if let Tool::Run(category, _) = self.tool {
                if self.work_axis != 2 {
                    let label = ui.label(crate::lang::t(bimcraft_i18n::msg!("Plane Coordinate")));
                    if ui.add(egui::TextEdit::singleline(&mut self.work_coordinate).desired_width(64.0)).labelled_by(label.id).changed() {
                        self.tool = Tool::Run(category, None);
                    }
                }
            } else {
                return;
            }
        }
        if self.sheet != Sheet::View3D || !matches!(self.tool, Tool::Run(..) | Tool::Component | Tool::Fixture(_)) {
            return;
        }
        if matches!(self.tool, Tool::Component | Tool::Fixture(_)) {
            ui.checkbox(&mut self.place_on_face, crate::lang::t(bimcraft_i18n::msg!("Place on Face")));
        }
        let selected = self.placement_level();
        let levels: Vec<_> = self.session.model.levels().into_iter().map(|(id, level)| (id, level.name.clone())).collect();
        let name = levels.iter().find(|(id, _)| Some(*id) == selected).map_or("", |(_, name)| name.as_str());
        let label = ui.label(crate::lang::t(bimcraft_i18n::msg!("Work Level")));
        egui::ComboBox::from_id_salt("3d-work-level")
            .selected_text(name)
            .show_ui(ui, |ui| {
                for (id, name) in &levels {
                    if ui.selectable_label(Some(*id) == selected, name).clicked() {
                        self.work_level = Some(self.session.model.guid(*id));
                        if let Tool::Run(category, _) = self.tool {
                            self.tool = Tool::Run(category, None);
                        }
                    }
                }
            })
            .response
            .labelled_by(label.id);
    }

    /// Where model point `p` shows on screen, in the 3D view as last shown.
    pub fn screen3(&self, p: P3) -> Pos2 {
        let o = &self.orbit;
        match o.camera {
            Some(c) => {
                let (x, y, _) = c.project(p);
                o.rect.min + vec2(x as f32 / o.ppp, y as f32 / o.ppp)
            }
            None => o.rect.center(),
        }
    }

    /// Zoom to Fit: all the view shows of the model, from where the camera is.
    pub(crate) fn fit3d(&mut self) {
        let model = Arc::clone(&self.session.model);
        let look = Look::of_3d(&model);
        let o = &mut self.orbit;
        if let (Some(c), Some((_, scene))) = (o.camera, &o.scene) {
            o.camera = Some(scene.fit_in(c.azimuth, c.elevation, c.width, c.height, &look));
        }
    }

    /// Paint (PT): the face clicked of element `id` takes the material in the Options Bar, or with `remove` loses its paint.
    pub(crate) fn paint_face(&mut self, id: bimcraft_core::model::Id, face: Option<&'static str>, remove: bool) {
        let Some(face) = face else {
            self.status = Status { text: crate::lang::t("there's no face to paint there").into(), error: true };
            return;
        };
        let material = match (remove, self.painting()) {
            (true, _) => serde_json::Value::Null,
            (false, Some(n)) => json!(n),
            (false, None) => return,
        };
        self.run("paint", json!({"id": id, "face": face, "material": material}));
    }

    /// The 3D view, filling `ui`.
    pub(crate) fn view3d(&mut self, ui: &mut Ui) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.canvas.track(ui, &response);
        let ppp = ui.ctx().pixels_per_point();
        let side = |v: f32| ((v * ppp).round().max(1.0) as u32).min(MAX_SIDE);
        let (w, h) = (side(rect.width()), side(rect.height()));
        let px = |p: Pos2| (f64::from((p.x - rect.min.x) * ppp), f64::from((p.y - rect.min.y) * ppp));
        let (hover, wheel, pinch, shift, latest) = ui.input(|i| {
            let hover = i.pointer.hover_pos().filter(|_| response.hovered());
            (hover, i.smooth_scroll_delta.y, i.zoom_delta(), i.modifiers.shift, i.pointer.latest_pos())
        });
        let model = self.view_model();
        let equipment = if self.place_on_face && matches!(self.tool, Tool::Component | Tool::Fixture(_)) {
            self.placing().zip(self.placement_level()).map(|((family, type_name), level)| Component {
                family,
                type_name,
                level: model.guid(level),
                at: P2(0.0, 0.0),
                angle: self.component_angle,
                offset: 0.0,
                mirrored: false,
                params: Default::default(),
                provision: None,
                connection_sides: Vec::new(),
            })
        } else {
            None
        };
        let work_axis = self.work_axis;
        let placement_level = self.placement_level();
        let constrain = self.constrain;
        let (u, v) = [(1, 2), (0, 2), (0, 1)][work_axis];
        let move_plane = matches!(self.tool, Tool::Move(_))
            .then(|| {
                self.selection
                    .iter()
                    .find_map(|id| model.elements.get(*id).and_then(|e| model.run_geom(e)).map(|g| (g.a[work_axis] + g.b[work_axis]) * 0.5))
                    .or_else(|| {
                        if work_axis == 2 {
                            self.placement_level().map(|id| model.elevation(id))
                        } else {
                            self.selection.first().and_then(|id| model.anchor(*id)).map(|p| if work_axis == 0 { p.0 } else { p.1 })
                        }
                    })
            })
            .flatten();
        let (modify, demolish, selection) = (self.tool == Tool::Modify, self.tool == Tool::Demolish, self.selection.clone());
        let (paint, unpaint) = (self.tool == Tool::Paint, self.tool == Tool::Unpaint);
        let o = &mut self.orbit;
        (o.rect, o.ppp) = (rect, ppp);
        let scene = scene_of(&mut o.scene, &model);
        let mut cam = match o.camera {
            Some(c) => Camera { width: w, height: h, ..c },
            None => scene.fit_in(AZIMUTH, ELEVATION, w, h, &Look::of_3d(&model)),
        };
        if let Some(at) = hover {
            // A wheel notch (about 50 points) zooms by a fifth.
            let factor = (f64::from(wheel) * 0.004).exp() * f64::from(pinch);
            if factor.is_finite() && factor != 1.0 {
                cam = cam.zoom(factor.clamp(0.2, 5.0), px(at));
            }
        }
        // A level selected alone has a grip at the middle of each edge of its plane: a drag from one
        // moves that edge over the plane, rather than turning the view.
        let level = match selection.as_slice() {
            &[id] if modify => model.level(id).and_then(|l| Some((id, l.elevation, model.level_extent(id)?))),
            _ => None,
        };
        if let Some((id, z, extent)) = level
            && response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && let Some(k) = geom::edge_grips(extent).iter().position(|g| on_screen(&cam, rect, ppp, [g.0, g.1, z]).distance(origin) <= 8.0)
        {
            o.grip = Some((id, k));
        }
        if !level.is_some_and(|(id, ..)| o.grip.is_some_and(|(g, _)| g == id)) {
            o.grip = None;
        }
        // Where the held grip's edge is now: over the level's plane, under the pointer, to 10 mm.
        let regripped = match (level, o.grip, latest) {
            (Some((_, z, extent)), Some((_, k)), Some(now)) => cam.on_plane(px(now), z).map(|p| {
                let round = |v: f64| (v / 10.0).round() * 10.0;
                geom::move_edge(extent, k, P2(round(p[0]), round(p[1])))
            }),
            _ => None,
        };
        let mut moved = None;
        if response.drag_stopped()
            && let (Some((id, _)), Some(to)) = (o.grip.take(), regripped)
            && !self.canvas.quick()
        {
            moved = Some((id, to));
        }
        // The section box, while it's on; in Modify a drag from one of its grips moves that face along its axis, and the view
        // shows the cut as it goes. The command runs when the drag ends.
        let boxed = Look::of_3d(&model).section_box();
        if let Some(b) = boxed
            && modify
            && o.grip.is_none()
            && response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && let Some(k) = crate::section_box::grip_at(b, origin, |p| on_screen(&cam, rect, ppp, p))
        {
            o.boxing = Some((k, origin));
        }
        if boxed.is_none() {
            o.boxing = None;
        }
        let reboxed = match (boxed, o.boxing, latest) {
            (Some(b), Some((k, from)), Some(now)) => Some(move_box_face(b, k, cam.along(k / 2, px(from), px(now)).unwrap_or(0.0))),
            _ => None,
        };
        let mut boxed_to = None;
        if response.drag_stopped()
            && let (Some(_), Some(to)) = (o.boxing.take(), reboxed)
            && !self.canvas.quick()
        {
            boxed_to = Some(to);
        }
        let surface_at = |at: Pos2| {
            let (x, y) = px(at);
            o.picker.as_ref().and_then(|picker| picker.at(&cam, x, y))
        };
        if shift
            && modify
            && response.drag_started_by(PointerButton::Primary)
            && o.grip.is_none()
            && o.boxing.is_none()
            && let [id] = selection.as_slice()
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && surface_at(origin).is_some_and(|hit| hit.id == *id)
            && model.elements.get(*id).is_some_and(|element| model.run_geom(element).is_some())
        {
            o.mount_drag = Some(*id);
        }
        let mounting = response
            .drag_stopped()
            .then(|| o.mount_drag.take())
            .flatten()
            .zip(latest.filter(|at| rect.contains(*at)).and_then(surface_at))
            .filter(|(_, hit)| matches!(hit.face, "interior" | "exterior" | "top" | "bottom"));
        // The primary button, or Shift and the middle button, orbit, 0.4° a point; the middle
        // button alone pans.
        if response.drag_started()
            && o.mount_drag.is_none()
            && o.grip.is_none()
            && o.boxing.is_none()
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            o.drag = Some((response.dragged_by(PointerButton::Primary) || shift, origin));
            o.before = Some(cam);
        }
        if let (Some((orbit, last)), Some(now)) = (o.drag, latest)
            && response.dragged()
        {
            let d = now - last;
            cam = if orbit { cam.orbit(-f64::from(d.x) * 0.4, f64::from(d.y) * 0.4) } else { cam.pan(px(last), px(now)) };
            o.drag = Some((orbit, now));
        }
        if response.drag_stopped() {
            o.drag = None;
            // A click made on the move: the camera the hand turned meanwhile goes back.
            if let (true, Some(before)) = (self.canvas.quick(), o.before.take()) {
                cam = before;
            }
        }
        o.camera = Some(cam);
        // On the card when the user asked for it and it can draw what the view shows; else on the processor.
        let mesh = match self.card.ready().is_some().then(|| mesh_of(&mut o.mesh, &model, scene)) {
            Some(Ok(mesh)) => Some(Arc::clone(mesh)),
            _ => None,
        };
        // Why not, where the mesh says; a card that failed keeps what it said.
        if !self.card.failed {
            let why = (self.card.wanted && mesh.is_none()).then(|| o.mesh.as_ref().and_then(|(_, m)| m.as_ref().err().cloned())).flatten();
            if self.card.note != why {
                self.card.note = why;
            }
        }
        let pick = |picker: &Option<Picker>, at: Pos2| {
            let (x, y) = px(at);
            picker.as_ref().and_then(|p| p.at(&cam, x, y))
        };
        let hovered = hover
            .filter(|_| (modify || demolish || paint || unpaint || self.tool == Tool::Split) && o.drag.is_none())
            .and_then(|at| pick(&o.picker, at))
            .map(|h| h.id);
        let tints: Vec<(Id, Rgb)> = selection
            .iter()
            .map(|id| (*id, rgb(canvas_pal().accent)))
            .chain(hovered.filter(|id| !selection.contains(id)).map(|id| (id, rgb(canvas_pal().hover))))
            .collect();
        let dark = crate::style::canvas_dark();
        if !matches!(&o.shown, Some((m, c, t, k, g, b)) if Arc::ptr_eq(m, &model) && *c == cam && *t == tints && *k == dark && *g == mesh.is_some() && *b == reboxed)
        {
            // The card's picture, if it makes one; the processor's if not, and if the card fails it for good.
            let mut drawn = false;
            if let (Some(mesh), Some(gpu)) = (&mesh, self.card.gpu.as_mut()) {
                match gpu.draw(mesh, &cam, &tints, rgb(color(render::PAPER))) {
                    Ok(()) => {
                        o.picker = Some(Picker::Mesh(Arc::clone(mesh), tints.clone()));
                        drawn = true;
                    }
                    Err(e) => {
                        self.status = crate::updates::failed(crate::lang::tf(
                            "The graphics card couldn't draw the 3D view ({}): it is drawn on the processor",
                            &[&e],
                        ));
                        (self.card.gpu, self.card.note) = (None, Some(e));
                    }
                }
            }
            o.on_card = drawn;
            if !drawn {
                // As the default 3D view says: what it hides, and the colours it draws in.
                let look = Look::of_3d(&model);
                let look = if reboxed.is_some() { look.with_section_box(reboxed) } else { look };
                let image = scene.render_in(&cam, &tints, &look);
                // The model shaded as ever, on the theme's ground: only the background is paper white.
                let ground = color(render::PAPER);
                let (rgba, _) = image.rgba.as_chunks::<4>();
                // Hidden line and wireframe are drawn as a section is, dark in the dark theme: paper and ink swap.
                let lines = look.style() != VisualStyle::Shaded;
                let pixels = rgba
                    .iter()
                    .map(|&[r, g, b, _]| match [r, g, b] {
                        _ if lines => color(Rgb(r, g, b)),
                        [255, 255, 255] => ground,
                        _ => Color32::from_rgb(r, g, b),
                    })
                    .collect();
                let pixels = ColorImage { size: [image.width as usize, image.height as usize], pixels, source_size: Default::default() };
                match &mut o.texture {
                    Some(t) => t.set(pixels, TextureOptions::NEAREST),
                    None => o.texture = Some(ui.ctx().load_texture("view3d", pixels, TextureOptions::NEAREST)),
                }
                o.picker = Some(Picker::Image(image));
            }
            o.shown = Some((Arc::clone(&model), cam, tints, dark, drawn, reboxed));
        }
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, color(render::PAPER));
        let shown = if o.on_card { self.card.gpu.as_ref().and_then(|g| g.texture()) } else { o.texture.as_ref().map(|t| t.id()) };
        if let Some(id) = shown {
            p.image(id, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        // The selected level's grips, and its plane as the held one would leave it.
        if let Some((_, z, extent)) = level {
            let shown = if o.grip.is_some() { regripped.unwrap_or(extent) } else { extent };
            let at = |q: P2| on_screen(&cam, rect, ppp, [q.0, q.1, z]);
            if o.grip.is_some() {
                let [lo, hi] = shown;
                let ring = [lo, P2(hi.0, lo.1), hi, P2(lo.0, hi.1)].map(at).to_vec();
                p.add(egui::Shape::closed_line(ring, Stroke::new(1.5, pal().accent)));
            }
            for g in geom::edge_grips(shown) {
                let r = Rect::from_center_size(at(g), vec2(9.0, 9.0));
                p.rect_filled(r, 1.0, pal().surface);
                p.rect_stroke(r, 1.0, Stroke::new(1.5, pal().accent), StrokeKind::Middle);
            }
        }
        // The section box's frame, as the held grip would leave it.
        let screen = |p: P3| on_screen(&cam, rect, ppp, p);
        let move_point = |at, from: Option<P2>, height| {
            cam.on_axis_plane(px(at), work_axis, height).map(|point| {
                let (x, y) = (point[u], point[v]);
                if let Some(from) = from.filter(|_| shift || constrain) {
                    if (x - from.0).abs() >= (y - from.1).abs() { P2(x, from.1) } else { P2(from.0, y) }
                } else {
                    P2(x, y)
                }
            })
        };
        let world = |p: P2, coordinate| {
            let mut point = [coordinate; 3];
            point[u] = p.0;
            point[v] = p.1;
            point
        };
        let split_point =
            |at, origin: P3| move_point(at, Some(P2(origin[u], origin[v])), origin[work_axis]).map(|to| world(to, origin[work_axis]));
        let vertical_run = matches!(self.tool, Tool::Run("wire" | "cablecarrier", _)) && work_axis != 2;
        let run_plane = vertical_run.then(|| model.units.parse(&self.work_coordinate)).flatten();
        if let (Tool::Run(_, Some(from)), Some(coordinate), Some(at)) = (self.tool.clone(), run_plane, hover)
            && let Some(to) = move_point(at, Some(from), coordinate)
        {
            p.line_segment([screen(world(from, coordinate)), screen(world(to, coordinate))], Stroke::new(1.5, canvas_pal().accent));
        }
        if work_axis == 2
            && let (Tool::Run("wire" | "cablecarrier", Some(from)), Some(level), Some(at)) = (&self.tool, placement_level, hover)
            && let Some(start) = model.units.parse(&self.beam_offset)
            && let Some(end) =
                model.units.parse(if self.run_end_offset.trim().is_empty() { &self.beam_offset } else { &self.run_end_offset })
        {
            let elevation = model.elevation(model.guid(level));
            let to = if self.run_vertical { Some(*from) } else { move_point(at, Some(*from), elevation + end) };
            if let Some(to) = to {
                p.line_segment(
                    [screen(world(*from, elevation + start)), screen(world(to, elevation + end))],
                    Stroke::new(1.5, canvas_pal().accent),
                );
            }
        }
        if let (Tool::Move(Some(from)), Some(height), Some(at)) = (&self.tool, move_plane, hover)
            && let Some(to) = move_point(at, Some(*from), height)
        {
            p.line_segment([screen(world(*from, height)), screen(world(to, height))], Stroke::new(1.5, canvas_pal().accent));
        }
        if let Some(b) = reboxed.or(boxed) {
            crate::section_box::draw(&p, &cam, b, modify, o.boxing.map(|(k, _)| k), screen);
        }
        if let Tool::SplitRoute3D { id, origin, .. } = self.tool
            && let Some(g) = model.elements.get(id).and_then(|e| model.run_geom(e))
            && let Some(to) = hover.and_then(|at| split_point(at, origin))
        {
            let stroke = Stroke::new(1.5, canvas_pal().accent);
            p.line_segment([screen(g.a), screen(to)], stroke);
            p.line_segment([screen(to), screen(g.b)], stroke);
        }
        // A click on a grip picks nothing behind it.
        let click = response.clicked().then(|| response.interact_pointer_pos()).flatten().or(self.canvas.quick_press());
        let click = click.filter(|at| !boxed.is_some_and(|b| modify && crate::section_box::grip_at(b, *at, screen).is_some()));
        let split_near = |at: Pos2| {
            let look = Look::of_3d(&model);
            let foreground = pick(&o.picker, at).and_then(|hit| scene.surface_at(hit.id, &cam, px(at)));
            model
                .elements
                .iter()
                .filter_map(|(&id, element)| {
                    if !look.shows(id) || !matches!(element, Element::Wire(_) | Element::CableCarrier(_)) {
                        return None;
                    }
                    let g = model.run_geom(element)?;
                    let start = screen(g.a);
                    let axis = screen(g.b) - start;
                    let t = ((at - start).dot(axis) / axis.length_sq()).clamp(0.0, 1.0);
                    let distance = (at - (start + axis * t)).length();
                    if !distance.is_finite() || distance > 4.0 {
                        return None;
                    }
                    let point = std::array::from_fn(|i| g.a[i] + (g.b[i] - g.a[i]) * f64::from(t));
                    if boxed.is_some_and(|(lo, hi)| (0..3).any(|i| point[i] < lo[i] || point[i] > hi[i]))
                        || foreground.as_ref().is_some_and(|hit| cam.project(point).2 + g.width < cam.project(hit.point).2)
                    {
                        return None;
                    }
                    Some((id, distance))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(id, _)| id)
        };
        let clicked = click.map(|at| {
            if self.tool == Tool::Split {
                // Thin wires may cover only a neighbouring raster sample.
                [vec2(0.0, 0.0), vec2(-1.0, 0.0), vec2(1.0, 0.0), vec2(0.0, -1.0), vec2(0.0, 1.0)]
                    .into_iter()
                    .filter_map(|offset| pick(&o.picker, at + offset))
                    .map(|hit| hit.id)
                    .find(|id| matches!(model.elements.get(*id), Some(Element::Wire(_) | Element::CableCarrier(_))))
                    .or_else(|| split_near(at))
            } else {
                pick(&o.picker, at).map(|h| h.id)
            }
        });
        // The face of what's clicked, as paint names the faces of an element.
        let face = click.and_then(|at| pick(&o.picker, at).map(|h| h.face).filter(|f| !f.is_empty()));
        let surface_component = |at| {
            let component = equipment.clone()?;
            let hit = pick(&o.picker, at)?;
            let element = model.elements.get(hit.id)?;
            if !matches!(
                (element, hit.face),
                (Element::Wall(_), "exterior" | "interior") | (Element::Floor(_) | Element::Ceiling(_), "top" | "bottom")
            ) {
                return None;
            }
            let surface = scene.surface_at(hit.id, &cam, px(at))?;
            (surface.face == hit.face).then(|| (equipment_pose(&model, component, &surface), surface))
        };
        if let Some((component, hit)) = hover.and_then(surface_component) {
            let tint = canvas_pal().accent;
            for triangle in scene.face_triangles(&hit) {
                p.add(egui::Shape::convex_polygon(triangle.map(screen).to_vec(), tint.gamma_multiply(0.2), Stroke::NONE));
            }
            let stroke = Stroke::new(1.0, tint);
            for piece in model.component_pieces(&component) {
                for tri in piece.tris {
                    p.add(egui::Shape::closed_line(tri.map(screen).to_vec(), stroke));
                }
            }
        }
        let equipment_on_face = click.and_then(surface_component).map(|(component, _)| component);
        match clicked {
            Some(_) if vertical_run => {
                if let (Tool::Run(category, from), Some(coordinate), Some(at), Some(level)) =
                    (self.tool.clone(), run_plane, click, self.placement_level())
                    && let Some(to) = move_point(at, from, coordinate)
                {
                    if let Some(from) = from {
                        let a = world(from, coordinate);
                        let b = world(to, coordinate);
                        let base = model.elevation(level);
                        let params = self.typed(
                            category,
                            json!({"points":[[a[0],a[1]],[b[0],b[1]]],
                            "level":level,"offset":a[2]-base,"end_offset":b[2]-base,"justify":self.run_justify}),
                        );
                        if let Some(result) = self.run(crate::structure::command(category), params) {
                            self.tool = Tool::Run(category, self.chain.then_some(to));
                            if category == "cablecarrier"
                                && let Some(id) = result["ids"][0].as_u64()
                            {
                                self.offer_carrier_attachment(id);
                            }
                        }
                    } else {
                        self.tool = Tool::Run(category, Some(to));
                    }
                }
            }
            Some(_) if matches!(self.tool, Tool::Move(_)) => {
                if let (Tool::Move(from), Some(height), Some(at)) = (&self.tool, move_plane, click)
                    && let Some(to) = move_point(at, *from, height)
                {
                    if let Some(from) = *from {
                        if work_axis == 2 {
                            self.shift_selection(from, to);
                        } else {
                            let [dx, dy, dz] = world(to - from, 0.0);
                            let params = json!({"ids": self.selection, "by": [micron(dx), micron(dy)], "dz": micron(dz)});
                            if !self.offer_carrier_move(&params) && self.run("move", params).is_some() {
                                self.tool = Tool::Modify;
                            }
                        }
                    } else {
                        self.tool = Tool::Move(Some(to));
                    }
                }
            }
            Some(found) if modify => self.pick_up(found.into_iter().collect(), click_modifiers(ui)),
            Some(Some(id)) if demolish => self.demolish(id),
            Some(Some(id)) if paint || unpaint => self.paint_face(id, face, unpaint),
            // Nothing under the cursor: nothing to take down, or to paint.
            Some(None) if demolish || paint || unpaint => {}
            Some(_) if equipment.is_some() => {
                if let Some(c) = equipment_on_face {
                    self.run(
                        "component",
                        json!({"family":c.family,"type":c.type_name,"level":c.level,
                        "at":[c.at.0,c.at.1],"angle":c.angle,"offset":c.offset}),
                    );
                }
            }
            Some(Some(id)) if self.tool == Tool::Split => {
                if let Some(at) = click
                    && let Some(e @ (Element::Wire(_) | Element::CableCarrier(_))) = model.elements.get(id)
                    && let Some(g) = model.run_geom(e)
                {
                    let a = screen(g.a);
                    let axis = screen(g.b) - a;
                    if axis.length_sq() > 0.0 {
                        let fraction = f64::from((at - a).dot(axis) / axis.length_sq()).clamp(0.0, 1.0);
                        let origin = std::array::from_fn(|i| g.a[i] + (g.b[i] - g.a[i]) * fraction);
                        self.tool = Tool::SplitRoute3D { id, fraction, origin };
                    }
                }
            }
            Some(_) if matches!(self.tool, Tool::SplitRoute3D { .. }) => {
                if let Tool::SplitRoute3D { id, fraction, origin } = self.tool
                    && let Some([x, y, z]) = click.and_then(|at| split_point(at, origin))
                    && self.run("split", json!({"id":id, "fraction":fraction, "to":[x,y], "dz":z-origin[2]})).is_some()
                {
                    self.tool = Tool::Split;
                }
            }
            Some(found) if matches!(self.tool, Tool::Run(..) | Tool::Component | Tool::Fixture(_)) => {
                if let (Some(level), Some(at)) = (self.placement_level(), click)
                    && !self.start_wire_at_device(level, found)
                {
                    let offset = match self.tool {
                        Tool::Run("wire" | "cablecarrier", Some(_)) if !self.run_end_offset.trim().is_empty() => &self.run_end_offset,
                        Tool::Run(..) => &self.beam_offset,
                        _ => &self.component_offset,
                    };
                    if let Some(offset) = self.session.model.units.parse(offset) {
                        let z = self.session.model.elevation(self.session.model.guid(level)) + offset;
                        let point = match self.tool {
                            Tool::Run("wire" | "cablecarrier", from) if work_axis == 2 => move_point(at, from, z),
                            _ => cam.on_plane(px(at), z).map(|point| P2(point[0], point[1])),
                        };
                        if let Some(point) = point {
                            if matches!(self.tool, Tool::Run(..)) {
                                self.place_run(level, point);
                            } else {
                                self.place_component(level, point);
                            }
                        }
                    } else {
                        self.status = Status {
                            text: crate::lang::t(bimcraft_i18n::msg!("Enter a valid workplane height offset.")).into(),
                            error: true,
                        };
                    }
                }
            }
            Some(_) => {
                self.status =
                    Status { text: crate::lang::t("this tool works in plan views: open one from the Project Browser").into(), error: true }
            }
            None => {}
        }
        self.context_menu(&response);
        if let Some((id, hit)) = mounting {
            self.run("run_mount", serde_json::json!({"id":id,"host":hit.id,"face":hit.face}));
        }
        if let Some((id, [lo, hi])) = moved {
            self.run("modify", json!({"id": id, "extent": [[lo.0, lo.1], [hi.0, hi.1]]}));
        }
        if let Some((lo, hi)) = boxed_to {
            self.run("section_box", json!({"min": lo, "max": hi}));
        }
        // The navigation bar, at the view's top right: Home (Revit's default orientation), fit, zoom.
        let bar = Rect::from_min_size(self.nav_corner(rect), vec2(32.0, 104.0));
        p.rect(bar, 6.0, pal().surface, Stroke::new(1.0, pal().border), StrokeKind::Inside);
        ui.scope_builder(UiBuilder::new().max_rect(bar.shrink(3.0)).layout(Layout::top_down(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if icon_button(ui, Icon::View3D, "Home View").clicked() {
                self.orbit.camera = None;
            }
            if icon_button(ui, Icon::Fit, "Zoom to Fit").clicked() {
                self.fit3d();
            }
            for (icon, label, factor) in [(Icon::ZoomIn, "Zoom In", 1.25), (Icon::ZoomOut, "Zoom Out", 0.8)] {
                if icon_button(ui, icon, label).clicked()
                    && let Some(c) = self.orbit.camera
                {
                    self.orbit.camera = Some(c.zoom(factor, (f64::from(c.width) / 2.0, f64::from(c.height) / 2.0)));
                }
            }
        });
    }
}
