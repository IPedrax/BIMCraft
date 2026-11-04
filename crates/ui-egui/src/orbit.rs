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
use bimcraft_core::model::{Id, Model};
use bimcraft_core::render::{self, Rgb};
use bimcraft_core::view3d::gpu::{Hit, Mesh};
use bimcraft_core::view3d::{AZIMUTH, Camera, ELEVATION, Image, MAX_SIDE, P3, Scene, box_grips, move_box_face};
use egui::{
    Align, Color32, ColorImage, Layout, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, UiBuilder,
    pos2, vec2,
};
use serde_json::json;

use crate::canvas::{click_modifiers, color};
use crate::icons::Icon;
use crate::style::{canvas_pal, pal};
use crate::{BimApp, Status, Tool, icon_button};

/// What a picture is of: the model, the camera, the elements tinted, whether the theme is dark, whether the card drew it, and
/// the section box it is cut to while one is dragged.
type Shot = (Arc<Model>, Camera, Vec<(Id, Rgb)>, bool, bool, Option<(P3, P3)>);

/// A model's mesh for the card, or why the card can't draw it.
type Meshed = (Arc<Model>, Result<Arc<Mesh>, String>);

struct SceneJob {
    model: Arc<Model>,
    scene: Option<Arc<Scene>>,
    /// The scene of a model before this one, which an edit brings up to date instead of making the whole scene again.
    from: Option<(Arc<Model>, Arc<Scene>)>,
    width: u32,
    height: u32,
    lens: Option<f64>,
    card: bool,
}

struct SceneDone {
    model: Arc<Model>,
    scene: Arc<Scene>,
    fitted: Camera,
    mesh: Option<Result<Arc<Mesh>, String>>,
}

fn build_scene(job: SceneJob, cancel: &crate::view_worker::Cancel) -> Option<SceneDone> {
    let scene = match (job.scene, job.from) {
        (Some(scene), _) => scene,
        (None, Some((model, scene))) => Arc::new(scene.update_cancellable(&model, &job.model, &|| cancel.cancelled())?),
        (None, None) => Arc::new(Scene::new_cancellable(&job.model, &|| cancel.cancelled())?),
    };
    if cancel.cancelled() {
        return None;
    }
    let look = Look::of_3d(&job.model);
    let fitted = scene.fit_from(Camera { lens: job.lens, ..scene.fit_in(AZIMUTH, ELEVATION, job.width, job.height, &look) }, &look);
    if cancel.cancelled() {
        return None;
    }
    let mesh = if job.card {
        match Mesh::new_cancellable(&scene, &look, &|| cancel.cancelled()) {
            Ok(Some(mesh)) => Some(Ok(Arc::new(mesh))),
            Ok(None) => return None,
            Err(error) => Some(Err(error)),
        }
    } else {
        None
    };
    if let Some(Ok(mesh)) = &mesh
        && !mesh.prepare_picking_cancellable(&|| cancel.cancelled())
    {
        return None;
    }
    if cancel.cancelled() {
        return None;
    }
    Some(SceneDone { model: job.model, scene, fitted, mesh })
}

struct FitJob {
    model: Arc<Model>,
    scene: Arc<Scene>,
    from: Camera,
    camera: Camera,
}

struct FitDone {
    model: Arc<Model>,
    scene: Arc<Scene>,
    from: Camera,
    target: Camera,
    camera: Camera,
}

fn fit_scene(job: FitJob, cancel: &crate::view_worker::Cancel) -> Option<FitDone> {
    if cancel.cancelled() {
        return None;
    }
    let look = Look::of_3d(&job.model);
    let camera = job.scene.fit_from(job.camera, &look);
    (!cancel.cancelled()).then_some(FitDone { model: job.model, scene: job.scene, from: job.from, target: job.camera, camera })
}

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
    pub(crate) scene: Option<(Arc<Model>, Arc<Scene>)>,
    /// The scene of a model before, kept while the scene of the model now is made from it.
    last: Option<(Arc<Model>, Arc<Scene>)>,
    /// What picks in the picture on screen, and what the picture is of.
    picker: Option<Picker>,
    shown: Option<Shot>,
    /// The processor's picture as a texture; and whether the card's is the one on screen.
    texture: Option<TextureHandle>,
    pub(crate) on_card: bool,
    /// The mesh of the scene shown last, for the card: or why it can't draw it.
    mesh: Option<Meshed>,
    worker: Option<crate::view_worker::Latest<SceneJob, SceneDone>>,
    requested: Option<Arc<Model>>,
    requested_card: bool,
    loading_error: Option<String>,
    fit_worker: Option<crate::view_worker::Latest<FitJob, FitDone>>,
    fit_target: Option<Camera>,
    fit_requested: Option<Camera>,
    fit_from: Option<Camera>,
    /// A drag under way: whether it orbits (else it pans), and where the pointer was last.
    drag: Option<(bool, Pos2)>,
    /// The camera as that drag found it, for a drag that turns out a click.
    before: Option<Camera>,
    /// A selected level's grip being dragged: the level, and which edge of its plane (as
    /// `geom::edge_grips` orders them).
    grip: Option<(Id, usize)>,
    /// A section box's grip being dragged: which face (as `view3d::box_grips` orders them), and where the drag began.
    boxing: Option<(usize, Pos2)>,
    /// Each room's solid as the model shown last has it: its outline, bottom and top. Found once for each model.
    rooms: Option<(Arc<Model>, std::collections::BTreeMap<Id, RoomSolid>)>,
}

/// A room as the 3D view draws it: its outline in plan (an unbounded one's square), swept from `bottom` to `top`.
type RoomSolid = (Vec<P2>, f64, f64);

impl Default for Orbit {
    fn default() -> Orbit {
        Orbit {
            camera: None,
            rect: Rect::ZERO,
            ppp: 1.0,
            scene: None,
            last: None,
            picker: None,
            shown: None,
            texture: None,
            on_card: false,
            mesh: None,
            worker: None,
            requested: None,
            requested_card: false,
            loading_error: None,
            fit_worker: None,
            fit_target: None,
            fit_requested: None,
            fit_from: None,
            drag: None,
            before: None,
            grip: None,
            boxing: None,
            rooms: None,
        }
    }
}

/// A room as a see-through solid, drawn over the 3D view's picture in the selection's colour: its outline swept from its
/// bottom to its top (`Model::room_span`), its faces filled faintly and its edges drawn. Nothing of it behind the eye.
/// A selected one stronger than one only shown.
fn room_solid(p: &egui::Painter, (outline, bottom, top): &RoomSolid, selected: bool, screen: &dyn Fn(P3) -> Pos2) {
    let (bottom, top) = (*bottom, *top);
    let n = outline.len();
    if n < 3 || top.partial_cmp(&bottom) != Some(std::cmp::Ordering::Greater) {
        return;
    }
    let low: Vec<Pos2> = outline.iter().map(|q| screen([q.0, q.1, bottom])).collect();
    let high: Vec<Pos2> = outline.iter().map(|q| screen([q.0, q.1, top])).collect();
    if low.iter().chain(&high).any(|q| !q.is_finite()) {
        return;
    }
    let accent = canvas_pal().accent;
    let (alpha, width) = if selected { (34, 1.5) } else { (14, 0.75) };
    let fill = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), alpha);
    let mut mesh = egui::Mesh::default();
    let mut tri = |a: Pos2, b: Pos2, c: Pos2| {
        let i = mesh.vertices.len() as u32;
        for q in [a, b, c] {
            mesh.colored_vertex(q, fill);
        }
        mesh.add_triangle(i, i + 1, i + 2);
    };
    for [a, b, c] in geom::triangulate(outline) {
        if a.max(b).max(c) < n {
            tri(low[a], low[b], low[c]);
            tri(high[a], high[b], high[c]);
        }
    }
    for i in 0..n {
        let j = (i + 1) % n;
        tri(low[i], low[j], high[j]);
        tri(low[i], high[j], high[i]);
    }
    p.add(egui::Shape::mesh(mesh));
    let edge = Stroke::new(width, accent);
    p.add(egui::Shape::closed_line(low.clone(), edge));
    p.add(egui::Shape::closed_line(high.clone(), edge));
    for (a, b) in low.iter().zip(&high) {
        p.line_segment([*a, *b], edge);
    }
}

/// Where model point `p` shows on screen, in a view in `rect` through `cam`, `ppp` pixels a point.
fn on_screen(cam: &Camera, rect: Rect, ppp: f32, p: [f64; 3]) -> Pos2 {
    let (x, y, _) = cam.project(p);
    rect.min + vec2(x as f32 / ppp, y as f32 / ppp)
}

pub(crate) fn rgb(c: Color32) -> Rgb {
    Rgb(c.r(), c.g(), c.b())
}

/// The scene of `model` in `slot`, built again when the model changed.
pub(crate) fn scene_of<'a>(slot: &'a mut Option<(Arc<Model>, Arc<Scene>)>, model: &Arc<Model>) -> &'a Scene {
    if let Some((before, scene)) = slot.take_if(|(m, _)| !Arc::ptr_eq(m, model)) {
        *slot = Some((Arc::clone(model), Arc::new(scene.update(&before, model))));
    }
    &slot.get_or_insert_with(|| (Arc::clone(model), Arc::new(Scene::new(model)))).1
}

/// The lens the default 3D view is seen through while it's in perspective.
pub(crate) fn lens_of(model: &Model) -> Option<f64> {
    match model.default_3d().and_then(|v| model.elements.get(v)) {
        Some(bimcraft_core::model::Element::View3D(v)) => v.lens(),
        _ => None,
    }
}

/// The mesh of `scene` for the card, built again when the model changed; or why the card can't draw it.
impl Orbit {
    pub(crate) fn suspend(&mut self) {
        self.worker = None;
        self.requested = None;
        self.fit_worker = None;
        self.fit_target = None;
        self.fit_requested = None;
        self.fit_from = None;
    }

    /// Install a whole snapshot at once, so the picture and its picker cannot refer to different models.
    fn prepare(&mut self, ctx: &egui::Context, model: &Arc<Model>, width: u32, height: u32, card: bool) -> bool {
        if let Some(done) = self.worker.as_ref().and_then(crate::view_worker::Latest::take)
            && Arc::ptr_eq(&done.model, model)
        {
            if self.camera.is_none() {
                self.camera = Some(done.fitted);
            }
            self.mesh = done.mesh.map(|m| (Arc::clone(model), m));
            self.scene = Some((done.model, done.scene));
            self.last = None;
        }
        let current = self.scene.as_ref().is_some_and(|(m, _)| Arc::ptr_eq(m, model));
        let needs_mesh = card && !self.mesh.as_ref().is_some_and(|(m, _)| Arc::ptr_eq(m, model));
        if !current || needs_mesh || self.camera.is_none() {
            if !self.requested.as_ref().is_some_and(|m| Arc::ptr_eq(m, model)) || (needs_mesh && !self.requested_card) {
                let job = SceneJob {
                    model: Arc::clone(model),
                    scene: self.scene.as_ref().filter(|(m, _)| Arc::ptr_eq(m, model)).map(|(_, s)| Arc::clone(s)),
                    from: if current { None } else { self.scene.clone().or_else(|| self.last.clone()) },
                    width,
                    height,
                    lens: lens_of(model),
                    card,
                };
                if !current {
                    self.last = self.scene.take().or_else(|| self.last.take());
                    self.mesh = None;
                    self.picker = None;
                    self.shown = None;
                    self.fit_worker = None;
                    self.fit_target = None;
                    self.fit_requested = None;
                    self.fit_from = None;
                }
                self.loading_error = None;
                if cfg!(test) {
                    if let Some(done) = build_scene(job, &crate::view_worker::Cancel::default()) {
                        if self.camera.is_none() {
                            self.camera = Some(done.fitted);
                        }
                        self.mesh = done.mesh.map(|m| (Arc::clone(model), m));
                        self.scene = Some((done.model, done.scene));
                        self.last = None;
                    }
                } else {
                    let wake = ctx.clone();
                    let worker =
                        self.worker.get_or_insert_with(|| crate::view_worker::Latest::new(build_scene, move || wake.request_repaint()));
                    worker.request(job);
                }
                self.requested = Some(Arc::clone(model));
                self.requested_card = card;
            }
            if let Some(worker) = &self.worker {
                if let Err(error) = worker.start() {
                    self.loading_error = Some(error);
                }
                if let Some(error) = worker.take_error() {
                    self.loading_error = Some(error);
                }
                if worker.busy() {
                    ctx.request_repaint_after(std::time::Duration::from_millis(16));
                }
            }
        }
        self.camera.is_some() && self.scene.as_ref().is_some_and(|(m, _)| Arc::ptr_eq(m, model))
    }

    fn prepare_fit(&mut self, ctx: &egui::Context, model: &Arc<Model>) -> Option<String> {
        if self.fit_from.is_some_and(|from| self.camera != Some(from)) {
            self.fit_worker = None;
            self.fit_target = None;
            self.fit_requested = None;
            self.fit_from = None;
        }
        if let Some(done) = self.fit_worker.as_ref().and_then(crate::view_worker::Latest::take)
            && self.fit_requested == Some(done.target)
        {
            if self.camera == Some(done.from)
                && Arc::ptr_eq(&done.model, model)
                && self.scene.as_ref().is_some_and(|(_, scene)| Arc::ptr_eq(scene, &done.scene))
            {
                self.camera = Some(done.camera);
            }
            self.fit_target = None;
            self.fit_requested = None;
            self.fit_from = None;
        }
        let (Some(target), Some(from), Some((_, scene))) = (self.fit_target, self.camera, &self.scene) else { return None };
        if self.fit_requested != Some(target) {
            let job = FitJob { model: Arc::clone(model), scene: Arc::clone(scene), from, camera: target };
            if cfg!(test) {
                if let Some(done) = fit_scene(job, &crate::view_worker::Cancel::default()) {
                    self.camera = Some(done.camera);
                    self.fit_target = None;
                }
                return None;
            }
            let wake = ctx.clone();
            let worker = self.fit_worker.get_or_insert_with(|| crate::view_worker::Latest::new(fit_scene, move || wake.request_repaint()));
            worker.request(job);
            self.fit_requested = Some(target);
            self.fit_from = Some(from);
        }
        if let Some(worker) = &self.fit_worker {
            if let Err(error) = worker.start() {
                return Some(error);
            }
            if let Some(error) = worker.take_error() {
                self.fit_target = None;
                self.fit_requested = None;
                self.fit_from = None;
                return Some(error);
            }
            if worker.busy() {
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
        }
        None
    }
}

impl BimApp {
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
        if let Some(c) = self.orbit.camera {
            self.orbit.fit_target = Some(c);
            self.orbit.fit_requested = None;
            self.orbit.fit_from = None;
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
        let (modify, demolish, selection) = (self.tool == Tool::Modify, self.tool == Tool::Demolish, self.selection.clone());
        let (paint, unpaint) = (self.tool == Tool::Paint, self.tool == Tool::Unpaint);
        let o = &mut self.orbit;
        (o.rect, o.ppp) = (rect, ppp);
        if !o.scene.as_ref().is_some_and(|(m, _)| Arc::ptr_eq(m, &model))
            && let Some(gpu) = &mut self.card.gpu
        {
            gpu.forget_mesh();
        }
        let card_ready = self.card.ready().is_some();
        if !o.prepare(ui.ctx(), &model, w, h, card_ready) {
            let p = ui.painter_at(rect);
            p.rect_filled(rect, 0.0, color(render::PAPER));
            p.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                o.loading_error.as_deref().unwrap_or("Building 3D view…"),
                egui::FontId::proportional(16.0),
                pal().text,
            );
            return;
        }
        if let Some(error) = o.prepare_fit(ui.ctx(), &model) {
            self.status = Status { text: crate::lang::tf("Could not fit the 3D view: {}", &[&error]), error: true };
        }
        // A prepared view has both; without them there's nothing to draw this frame.
        let (Some((_, scene)), Some(camera)) = (o.scene.as_ref(), o.camera) else { return };
        // Through the view's lens while it's in perspective: the eye stays where it stood as the lens or the picture changes.
        let lens = lens_of(&model);
        let mut cam = camera.sized(w, h, lens);
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
            (Some(b), Some((k, from)), Some(now)) => {
                let grip = box_grips(b).get(k).copied().unwrap_or_default();
                Some(move_box_face(b, k, cam.along(k / 2, grip, px(from), px(now)).unwrap_or(0.0)))
            }
            _ => None,
        };
        let mut boxed_to = None;
        if response.drag_stopped()
            && let (Some(_), Some(to)) = (o.boxing.take(), reboxed)
            && !self.canvas.quick()
        {
            boxed_to = Some(to);
        }
        // The primary button, or Shift and the middle button, orbit, 0.4° a point; the middle
        // button alone pans.
        if response.drag_started()
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
        let mut mesh = match card_ready.then(|| o.mesh.as_ref().map(|(_, m)| m)).flatten() {
            Some(Ok(mesh)) => Some(Arc::clone(mesh)),
            _ => None,
        };
        if let (Some(mesh_to_upload), Some(gpu)) = (mesh.as_ref(), self.card.gpu.as_mut())
            && !cfg!(test)
        {
            match gpu.prepare_upload(ui.ctx(), mesh_to_upload) {
                Ok(true) => {}
                Ok(false) => {
                    let p = ui.painter_at(rect);
                    p.rect_filled(rect, 0.0, color(render::PAPER));
                    p.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Loading 3D view onto the card…",
                        egui::FontId::proportional(16.0),
                        pal().text,
                    );
                    return;
                }
                Err(e) => {
                    self.status = crate::updates::failed(crate::lang::tf(
                        "The graphics card couldn't draw the 3D view ({}): it is drawn on the processor",
                        &[&e],
                    ));
                    (self.card.gpu, self.card.note) = (None, Some(e));
                    mesh = None;
                }
            }
        }
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
        let hovered =
            hover.filter(|_| (modify || demolish || paint || unpaint) && o.drag.is_none()).and_then(|at| pick(&o.picker, at)).map(|h| h.id);
        let tints: Vec<(Id, Rgb)> = selection
            .iter()
            .map(|id| (*id, rgb(canvas_pal().accent)))
            .chain(hovered.filter(|id| !selection.contains(id)).map(|id| (id, rgb(canvas_pal().hover))))
            .collect();
        let dark = crate::style::canvas_dark();
        if !matches!(&o.shown, Some((m, c, t, k, g, b)) if Arc::ptr_eq(m, &model) && *c == cam && *t == tints && *k == dark && *g == mesh.is_some() && *b == reboxed)
        {
            // The card's picture, if it makes one; the processor's if not, and if the card fails it for good.
            let (mut drawn, mut exact) = (false, true);
            if let (Some(mesh), Some(gpu)) = (&mesh, self.card.gpu.as_mut()) {
                gpu.swapped_ink = dark;
                match gpu.draw_cut(mesh, &cam, &tints, rgb(color(render::PAPER)), reboxed.or(mesh.section_box)) {
                    Ok(whole) => {
                        o.picker = Some(Picker::Mesh(Arc::clone(mesh), tints.clone()));
                        (drawn, exact) = (true, whole);
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
            // A picture that stands in, or whose exact surfaces are on their way, is drawn again next frame until it is exact.
            o.shown = exact.then(|| (Arc::clone(&model), cam, tints, dark, drawn, reboxed));
            if !exact {
                ui.ctx().request_repaint();
            }
        }
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, color(render::PAPER));
        let shown = if o.on_card { self.card.gpu.as_ref().and_then(|g| g.texture()) } else { o.texture.as_ref().map(|t| t.id()) };
        if let Some(id) = shown {
            p.image(id, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        // Rooms aren't seen in 3D but while selected: then a see-through solid in the selection's colour, from their bottom to
        // their top, over their boundary (an unbounded one's square).
        // With Rooms shown in the view's Visibility/Graphics (they are off in every new 3D view), all of them so.
        let all_rooms = model
            .default_3d()
            .and_then(|v| bimcraft_core::graphics::graphics_of(&model, v))
            .is_some_and(|g| g.resolved(&model).shows_in_3d("room"));
        let look = all_rooms.then(|| Look::of_3d(&model));
        if !o.rooms.as_ref().is_some_and(|(m, _)| Arc::ptr_eq(m, &model)) {
            o.rooms = None;
        }
        let solids = &mut o.rooms.get_or_insert_with(|| (Arc::clone(&model), std::collections::BTreeMap::new())).1;
        let selected: std::collections::HashSet<Id> = selection.iter().copied().collect();
        for (id, e) in model.elements.iter() {
            let shown = selected.contains(id) || look.as_ref().is_some_and(|l| l.shows(*id));
            if let (true, bimcraft_core::model::Element::Room(r)) = (shown, e) {
                let solid = solids.entry(*id).or_insert_with(|| {
                    let (bottom, top) = model.room_span(r);
                    (model.room_outline(r).0, bottom, top)
                });
                room_solid(&p, solid, selected.contains(id), &|q: P3| on_screen(&cam, rect, ppp, q));
            }
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
            for g in geom::edge_grips(shown).into_iter().filter(|g| at(*g).is_finite()) {
                let r = Rect::from_center_size(at(g), vec2(9.0, 9.0));
                p.rect_filled(r, 1.0, pal().surface);
                p.rect_stroke(r, 1.0, Stroke::new(1.5, pal().accent), StrokeKind::Middle);
            }
        }
        // The section box's frame, as the held grip would leave it.
        let screen = |p: P3| on_screen(&cam, rect, ppp, p);
        if let Some(b) = reboxed.or(boxed) {
            crate::section_box::draw(&p, &cam, b, modify, o.boxing.map(|(k, _)| k), screen);
        }
        // A click on a grip picks nothing behind it.
        let click = response.clicked().then(|| response.interact_pointer_pos()).flatten().or(self.canvas.quick_press());
        let click = click.filter(|at| !boxed.is_some_and(|b| modify && crate::section_box::grip_at(b, *at, screen).is_some()));
        let clicked = click.map(|at| pick(&o.picker, at).map(|h| h.id));
        // The face of what's clicked, as paint names the faces of an element.
        let face = click.and_then(|at| pick(&o.picker, at).map(|h| h.face).filter(|f| !f.is_empty()));
        match clicked {
            Some(found) if modify => self.pick_up(found.into_iter().collect(), click_modifiers(ui)),
            Some(Some(id)) if demolish => self.demolish(id),
            Some(Some(id)) if paint || unpaint => self.paint_face(id, face, unpaint),
            // Nothing under the cursor: nothing to take down, or to paint.
            Some(None) if demolish || paint || unpaint => {}
            Some(_) => {
                self.status =
                    Status { text: crate::lang::t("this tool works in plan views: open one from the Project Browser").into(), error: true }
            }
            None => {}
        }
        self.context_menu(&response);
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
            if icon_button(ui, Icon::View3D, "Home View").clicked()
                && let Some(c) = self.orbit.camera
            {
                self.orbit.fit_target = Some(Camera { azimuth: AZIMUTH, elevation: ELEVATION, lens: lens_of(&model), ..c });
                self.orbit.fit_requested = None;
                self.orbit.fit_from = None;
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

#[cfg(test)]
mod edits {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn after_an_edit_the_worker_brings_the_last_scene_up_to_date_and_draws_the_whole_scene_s_picture() {
        let mut s = bimcraft_core::engine::Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
               door {"at": [2000, 0]}
               floor {}"#,
        )
        .unwrap();
        let before = Arc::clone(&s.model);
        let scene = Arc::new(Scene::new(&before));
        let wall = s.model.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id).unwrap();
        s.execute("move", &json!({"ids": [wall], "by": [0, -500]})).unwrap();
        let after = Arc::clone(&s.model);
        let (send, receive) = mpsc::channel();
        let worker = crate::view_worker::Latest::new(build_scene, move || {
            let _ = send.send(());
        });
        worker.request(SceneJob {
            model: Arc::clone(&after),
            scene: None,
            from: Some((before, scene)),
            width: 320,
            height: 240,
            lens: None,
            card: false,
        });
        worker.start().unwrap();
        let done = loop {
            receive.recv_timeout(Duration::from_secs(60)).unwrap();
            assert!(worker.take_error().is_none());
            if let Some(done) = worker.take() {
                break done;
            }
        };
        let whole = Scene::new(&after);
        assert!(Arc::ptr_eq(&done.model, &after));
        let cam = whole.fit(AZIMUTH, ELEVATION, 320, 240);
        assert_eq!(done.scene.render(&cam, &[]).png().unwrap(), whole.render(&cam, &[]).png().unwrap());
    }
}

#[cfg(test)]
mod worker_measurements {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore]
    fn tower_on_a_card_that_holds_little() {
        let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;
        let model = bimcraft_core::sample::tower::annotated(bimcraft_core::sample::tower::Plate::REFERENCE, 10).unwrap();
        let scene = bimcraft_core::view3d::Scene::new(&model);
        let look = Look::of_3d(&model);
        let mesh = Arc::new(bimcraft_core::view3d::gpu::Mesh::new(&scene, &look).unwrap());
        let fitted = scene.fit_in(30.0, 30.0, 1920, 1080, &look);
        eprintln!("{} MB of vertices of their own, in {} groups", (mesh.vertices.len() * 44) >> 20, mesh.groups.len());
        for (chunk, room) in [(256u64, u64::MAX >> 20), (16, 64), (16, 32)] {
            let Some(mut gpu) = crate::gpu::headless() else { return };
            gpu.vertex_bytes = chunk << 20;
            gpu.card_bytes = if room == u64::MAX >> 20 { u64::MAX } else { room << 20 };
            let t = Instant::now();
            gpu.draw(&mesh, &fitted, &[], bimcraft_core::render::PAPER).unwrap();
            gpu.wait();
            eprintln!("{chunk} MB buffers, room for {room} MB: first frame {:.0} ms", ms(t));
            let mut frames = Vec::new();
            for i in 0..30 {
                let cam = Camera { azimuth: fitted.azimuth + f64::from(i) * 3.0, ..fitted };
                let t = Instant::now();
                gpu.draw(&mesh, &cam, &[], bimcraft_core::render::PAPER).unwrap();
                gpu.wait();
                if i >= 5 {
                    frames.push(ms(t));
                }
            }
            frames.sort_by(f64::total_cmp);
            eprintln!(
                "  moving: median {:.1} ms, p95 {:.1} ms, worst {:.1} ms",
                frames[frames.len() / 2],
                frames[frames.len() * 95 / 100],
                frames[frames.len() - 1]
            );
            let cam = Camera { azimuth: fitted.azimuth + 100.0, ..fitted };
            let (t, mut settle, mut worst) = (Instant::now(), 0, 0.0f64);
            loop {
                let one = Instant::now();
                let whole = gpu.draw(&mesh, &cam, &[], bimcraft_core::render::PAPER).unwrap();
                gpu.wait();
                worst = worst.max(ms(one));
                settle += 1;
                if whole {
                    break;
                }
            }
            eprintln!("  settling: {settle} frames, {:.0} ms in all, the longest {worst:.1} ms", ms(t));
        }
    }

    #[test]
    #[ignore]
    fn tower_with_a_scan_on_the_card() {
        let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;
        let mut session = bimcraft_core::engine::Session::new();
        session.model = bimcraft_core::sample::tower::annotated(bimcraft_core::sample::tower::Plate::REFERENCE, 10).unwrap().into();
        // As many points as a picture draws, on a sheet across the tower's plate.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scan.xyz");
        let mut text = String::new();
        for i in 0..2000 {
            for k in 0..1000 {
                text += &format!("{} {} {} 220 30 30\n", f64::from(i) * 0.04, f64::from(k) * 0.06, f64::from((i + k) % 7) * 0.5);
            }
        }
        std::fs::write(&path, text).unwrap();
        session.execute("point_cloud", &serde_json::json!({"path": path, "at": [0, 0]})).unwrap();
        let Some(mut gpu) = crate::gpu::headless() else { return };
        let scene = bimcraft_core::view3d::Scene::new(&session.model);
        let look = Look::of_3d(&session.model);
        let t = Instant::now();
        let mesh = Arc::new(bimcraft_core::view3d::gpu::Mesh::new(&scene, &look).unwrap());
        eprintln!("mesh {:.0} ms, {} triangles, {} points", ms(t), mesh.triangles(), mesh.dots.len());
        let t = Instant::now();
        mesh.prepare_picking();
        eprintln!("picking hierarchies: {:.0} ms", ms(t));
        let fitted = scene.fit_in(30.0, 30.0, 1920, 1080, &look);
        let t = Instant::now();
        gpu.draw(&mesh, &fitted, &[], bimcraft_core::render::PAPER).unwrap();
        gpu.wait();
        eprintln!("first frame, with the upload: {:.0} ms", ms(t));
        let inside = bimcraft_core::view3d::inside(&session.model, Camera { lens: Some(bimcraft_core::view3d::LENS), ..fitted });
        let plain = gpu.budget;
        for (name, base, budget) in
            [("whole, moving", fitted, plain), ("whole, all of it a frame", fitted, usize::MAX), ("inside", inside, plain)]
        {
            gpu.budget = budget;
            let mut frames = Vec::new();
            for i in 0..30 {
                let cam = Camera { azimuth: base.azimuth + f64::from(i) * 3.0, ..base };
                let t = Instant::now();
                gpu.draw(&mesh, &cam, &[], bimcraft_core::render::PAPER).unwrap();
                gpu.wait();
                if i >= 5 {
                    frames.push(ms(t));
                }
            }
            frames.sort_by(f64::total_cmp);
            eprintln!("frame, {name}: median {:.1} ms, p95 {:.1} ms", frames[frames.len() / 2], frames[frames.len() * 95 / 100]);
        }
        let t = Instant::now();
        for i in 0..100 {
            std::hint::black_box(mesh.pick(&fitted, &[], 400.0 + f64::from(i) * 10.0, 540.0));
        }
        eprintln!("pick: {:.3} ms each", ms(t) / 100.0);
        let t = Instant::now();
        std::hint::black_box(scene.render_in(&fitted, &[], &look));
        eprintln!("the processor's picture: {:.0} ms", ms(t));
        // The points alone.
        let mut alone = bimcraft_core::engine::Session::new();
        alone.execute("point_cloud", &serde_json::json!({"path": path, "at": [0, 0]})).unwrap();
        let scene = bimcraft_core::view3d::Scene::new(&alone.model);
        let look = Look::of_3d(&alone.model);
        let mesh = Arc::new(bimcraft_core::view3d::gpu::Mesh::new(&scene, &look).unwrap());
        let fitted = scene.fit_in(30.0, 30.0, 1920, 1080, &look);
        gpu.budget = plain;
        gpu.draw(&mesh, &fitted, &[], bimcraft_core::render::PAPER).unwrap();
        gpu.wait();
        let mut frames = Vec::new();
        for i in 0..30 {
            let cam = Camera { azimuth: fitted.azimuth + f64::from(i) * 3.0, ..fitted };
            let t = Instant::now();
            gpu.draw(&mesh, &cam, &[], bimcraft_core::render::PAPER).unwrap();
            gpu.wait();
            if i >= 5 {
                frames.push(ms(t));
            }
        }
        frames.sort_by(f64::total_cmp);
        eprintln!("frame, the points alone: median {:.1} ms, p95 {:.1} ms", frames[frames.len() / 2], frames[frames.len() * 95 / 100]);
    }

    #[test]
    #[ignore]
    fn tower_in_the_styles_of_lines() {
        let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;
        let mut session = bimcraft_core::engine::Session::new();
        session.model = bimcraft_core::sample::tower::annotated(bimcraft_core::sample::tower::Plate::REFERENCE, 10).unwrap().into();
        let Some(mut gpu) = crate::gpu::headless() else { return };
        let t = Instant::now();
        let scene = bimcraft_core::view3d::Scene::new(&session.model);
        eprintln!("scene: {:.0} ms", ms(t));
        for style in ["hidden_line", "wireframe"] {
            session.execute("visual_style", &serde_json::json!({"style": style})).unwrap();
            let look = Look::of_3d(&session.model);
            let t = Instant::now();
            let mesh = Arc::new(bimcraft_core::view3d::gpu::Mesh::new(&scene, &look).unwrap());
            eprintln!("{style}: mesh {:.0} ms, {} triangles, {} lines", ms(t), mesh.triangles(), mesh.wires.len());
            let t = Instant::now();
            mesh.prepare_picking();
            eprintln!("  picking hierarchy: {:.0} ms", ms(t));
            let fitted = scene.fit_in(30.0, 30.0, 1920, 1080, &look);
            let t = Instant::now();
            gpu.draw(&mesh, &fitted, &[], bimcraft_core::render::PAPER).unwrap();
            gpu.wait();
            eprintln!("  first frame, with the upload: {:.0} ms", ms(t));
            let inside = bimcraft_core::view3d::inside(&session.model, Camera { lens: Some(bimcraft_core::view3d::LENS), ..fitted });
            let wall = session.model.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id);
            let tinted: Vec<(Id, Rgb)> = wall.into_iter().map(|id| (id, Rgb(255, 160, 0))).collect();
            for (name, base, tints) in
                [("whole", fitted, &[][..]), ("whole, a wall selected", fitted, &tinted[..]), ("inside", inside, &[][..])]
            {
                let mut frames = Vec::new();
                for i in 0..30 {
                    let cam = Camera { azimuth: base.azimuth + f64::from(i) * 3.0, ..base };
                    let t = Instant::now();
                    while !gpu.draw(&mesh, &cam, tints, bimcraft_core::render::PAPER).unwrap() {}
                    gpu.wait();
                    if i >= 5 {
                        frames.push(ms(t));
                    }
                }
                frames.sort_by(f64::total_cmp);
                eprintln!("  frame, {name}: median {:.1} ms, p95 {:.1} ms", frames[frames.len() / 2], frames[frames.len() * 95 / 100]);
            }
            let t = Instant::now();
            for i in 0..100 {
                std::hint::black_box(mesh.pick(&fitted, &[], 400.0 + f64::from(i) * 10.0, 540.0));
            }
            eprintln!("  pick: {:.3} ms each", ms(t) / 100.0);
            let t = Instant::now();
            std::hint::black_box(scene.render_in(&fitted, &[], &look));
            eprintln!("  the processor's picture: {:.0} ms", ms(t));
            if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
                eprintln!(
                    "  {}",
                    status.lines().filter(|l| l.starts_with("VmHWM") || l.starts_with("VmRSS")).collect::<Vec<_>>().join(", ")
                );
            }
        }
    }

    #[test]
    #[ignore = "measures the annotated ten-storey tower in release"]
    fn tower_scene_and_upload_leave_the_requesting_thread_free() {
        let model = Arc::new(bimcraft_core::sample::tower::annotated(bimcraft_core::sample::tower::Plate::REFERENCE, 10).unwrap());
        let model_for_inside = Arc::clone(&model);
        let (send, receive) = mpsc::channel();
        let worker = crate::view_worker::Latest::new(build_scene, move || {
            let _ = send.send(());
        });
        let started = Instant::now();
        worker.request(SceneJob { model, scene: None, from: None, width: 1920, height: 1080, lens: None, card: true });
        worker.start().unwrap();
        eprintln!("scene request and start: {:.2} ms", started.elapsed().as_secs_f64() * 1000.0);
        let done = loop {
            receive.recv_timeout(Duration::from_secs(120)).unwrap();
            if let Some(error) = worker.take_error() {
                panic!("scene worker: {error}");
            }
            if let Some(done) = worker.take() {
                break done;
            }
        };
        eprintln!("scene, fit, mesh and picking hierarchy complete: {:.2} s", started.elapsed().as_secs_f64());
        let mesh = done.mesh.unwrap().unwrap();
        let Some(mut gpu) = crate::gpu::headless() else { return };
        let ctx = egui::Context::default();
        let started = Instant::now();
        assert!(!gpu.prepare_upload(&ctx, &mesh).unwrap());
        eprintln!("upload request and start: {:.2} ms", started.elapsed().as_secs_f64() * 1000.0);
        loop {
            if gpu.prepare_upload(&ctx, &mesh).unwrap() {
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(120), "upload did not finish");
            std::thread::sleep(Duration::from_millis(16));
        }
        eprintln!("upload complete: {:.2} s", started.elapsed().as_secs_f64());
        let unique: usize = mesh.shapes.iter().map(|s| s.vertices.len() / 3).sum();
        eprintln!(
            "{} triangles drawn: {} of their own, {} copies of {} shapes ({unique} triangles)",
            mesh.triangles(),
            mesh.vertices.len() / 3,
            mesh.instances.len(),
            mesh.shapes.len()
        );
        eprintln!("{} groups of their own triangles, and {} copies each on its own", mesh.groups.len(), mesh.instances.len());
        // Frames as an orbit draws them, timed back to back once the card is awake: the whole tower, then nearer, then from inside.
        let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;
        let fitted = done.fitted;
        let corner = [fitted.target[0] - 15000.0, fitted.target[1] - 10000.0, fitted.target[2]];
        let inside = bimcraft_core::view3d::inside(&model_for_inside, Camera { lens: Some(bimcraft_core::view3d::LENS), ..fitted });
        for (name, base, cull) in [
            ("whole, all drawn", fitted, false),
            ("4x nearer, all drawn", Camera { scale: fitted.scale * 4.0, target: corner, ..fitted }, false),
            ("inside, all drawn", inside, false),
            ("whole", fitted, true),
            ("4x nearer", Camera { scale: fitted.scale * 4.0, target: corner, ..fitted }, true),
            ("16x nearer", Camera { scale: fitted.scale * 16.0, target: corner, ..fitted }, true),
            ("inside", inside, true),
            ("nothing", Camera { target: [fitted.target[0] + 1e7, fitted.target[1], fitted.target[2]], ..fitted }, true),
        ] {
            gpu.cull = cull;
            let mut frames = Vec::new();
            let mut drawn = 0usize;
            for i in 0..40 {
                let cam = Camera { azimuth: base.azimuth + f64::from(i) * 3.0, ..base };
                let t = Instant::now();
                gpu.draw(&mesh, &cam, &[], bimcraft_core::render::PAPER).unwrap();
                gpu.wait();
                if i >= 5 {
                    frames.push(ms(t));
                }
                if i == 0 {
                    let t = Instant::now();
                    for _ in 0..20 {
                        std::hint::black_box(mesh.visible(&cam));
                    }
                    let seen = mesh.visible(&cam);
                    eprintln!("  visible: {:.2} ms, {} runs of own, {} of copies", ms(t) / 21.0, seen.vertices.len(), seen.instances.len());
                    let own: usize = seen.vertices.iter().map(|r| r.len() / 3).sum();
                    let copies: usize = seen.instances.iter().map(|r| r.len()).sum();
                    drawn = own + copies;
                }
            }
            frames.sort_by(f64::total_cmp);
            eprintln!(
                "frame, {name}: median {:.1} ms, p95 {:.1} ms; drawn {drawn} of {} triangles and copies",
                frames[frames.len() / 2],
                frames[frames.len() * 95 / 100],
                mesh.vertices.len() / 3 + mesh.instances.len()
            );
        }
        // A section box: a storey's height through the middle, half the tower across, and a face dragged a little each frame.
        gpu.cull = true;
        let (o, far) = (mesh.origin, 1e7);
        let storey = ([o[0] - far, o[1] - far, o[2]], [o[0] + far, o[1] + far, o[2] + 3700.0]);
        let half = ([o[0] - far, o[1] - far, o[2] - far], [o[0], o[1] + far, o[2] + far]);
        for (name, cut, drags) in [("a storey", storey, false), ("half the tower", half, false), ("a face dragged", half, true)] {
            let mut frames = Vec::new();
            for i in 0..40 {
                let cam = if drags { fitted } else { Camera { azimuth: fitted.azimuth + f64::from(i) * 3.0, ..fitted } };
                let at = if drags { ([cut.0[0], cut.0[1], cut.0[2]], [cut.1[0] + f64::from(i) * 200.0, cut.1[1], cut.1[2]]) } else { cut };
                let t = Instant::now();
                gpu.draw_cut(&mesh, &cam, &[], bimcraft_core::render::PAPER, Some(at)).unwrap();
                gpu.wait();
                if i >= 5 {
                    frames.push(ms(t));
                }
            }
            frames.sort_by(f64::total_cmp);
            let seen = mesh.visible_cut(&fitted, Some(cut));
            eprintln!(
                "frame, section box, {name}: median {:.1} ms, p95 {:.1} ms; drawn {} of {} triangles",
                frames[frames.len() / 2],
                frames[frames.len() * 95 / 100],
                mesh.cost(&seen),
                mesh.triangles()
            );
        }
        let t = Instant::now();
        for i in 0..100 {
            std::hint::black_box(mesh.pick(&done.fitted, &[], 400.0 + f64::from(i) * 10.0, 540.0));
        }
        eprintln!("pick: {:.3} ms each", ms(t) / 100.0);
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            eprintln!("{}", status.lines().filter(|l| l.starts_with("VmHWM") || l.starts_with("VmRSS")).collect::<Vec<_>>().join(", "));
        }
    }
}
