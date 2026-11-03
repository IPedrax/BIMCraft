//! The 3D view: the core's picture of the model (`view3d::Scene`), orbited, panned and zoomed as
//! in Revit. Shift and the middle button orbit, and so does a drag with the primary button (for
//! a hand without a middle button); the middle button pans; the wheel zooms about the cursor. A
//! click picks the element the picture shows there, and so does a press let go quickly, however
//! far the hand went with it (`Canvas::quick_press`): the camera it turned goes back. A level
//! selected alone has a grip at the middle of each edge of its plane, which a drag moves over it. A section box is drawn
//! round what it keeps, with a grip on each face in Modify that a drag moves along its axis (`section_box.rs`). With Shape Editing's tools, the
//! selected floor's points and corners have grips that drag their heights, and clicks land on its top (`shape.rs`).

use std::sync::Arc;

use bimcraft_core::geom::{self, P2};
use bimcraft_core::graphics::{Look, VisualStyle};
use bimcraft_core::model::{Id, Model, TOL};
use bimcraft_core::render::{self, Rgb};
use bimcraft_core::view3d::gpu::{Hit, Mesh};
use bimcraft_core::view3d::{
    AZIMUTH, Camera, ELEVATION, Image, MAX_SIDE, P3, Scene, box_grips, move_box_face, on_floor_top, shape_handles, shape_pick_3d,
    shape_snap_3d,
};
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
    /// The camera as that drag found it, for a drag that turns out a click.
    before: Option<Camera>,
    /// A selected level's grip being dragged: the level, and which edge of its plane (as
    /// `geom::edge_grips` orders them).
    grip: Option<(Id, usize)>,
    /// A section box's grip being dragged: which face (as `view3d::box_grips` orders them), and where the drag began.
    boxing: Option<(usize, Pos2)>,
    /// A shaped floor's point or corner being dragged up or down: where it is in plan and in the model, and where the drag
    /// began.
    lift: Option<(Vec<P2>, P3, Pos2)>,
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
            before: None,
            grip: None,
            boxing: None,
            lift: None,
        }
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
pub(crate) fn scene_of<'a>(slot: &'a mut Option<(Arc<Model>, Scene)>, model: &Arc<Model>) -> &'a Scene {
    if !slot.as_ref().is_some_and(|(m, _)| Arc::ptr_eq(m, model)) {
        *slot = None;
    }
    &slot.get_or_insert_with(|| (Arc::clone(model), Scene::new(model))).1
}

/// The lens the default 3D view is seen through while it's in perspective.
pub(crate) fn lens_of(model: &Model) -> Option<f64> {
    match model.default_3d().and_then(|v| model.elements.get(v)) {
        Some(bimcraft_core::model::Element::View3D(v)) => v.lens(),
        _ => None,
    }
}

/// The mesh of `scene` for the card, built again when the model changed; or why the card can't draw it.
fn mesh_of<'a>(slot: &'a mut Option<Meshed>, model: &Arc<Model>, scene: &Scene) -> &'a Result<Arc<Mesh>, String> {
    if !slot.as_ref().is_some_and(|(m, _)| Arc::ptr_eq(m, model)) {
        *slot = None;
    }
    &slot.get_or_insert_with(|| (Arc::clone(model), Mesh::new(scene, &Look::of_3d(model)).map(Arc::new))).1
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
        let model = Arc::clone(&self.session.model);
        let look = Look::of_3d(&model);
        let o = &mut self.orbit;
        if let (Some(c), Some((_, scene))) = (o.camera, &o.scene) {
            o.camera = Some(scene.fit_from(c, &look));
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
        // The floor being shaped, with Shape Editing's tools: its points and corners where its top is, which Modify Sub
        // Elements' grips drag up or down; Add Point and Add Split Line click on its top.
        let shaping = self.shaping().and_then(|id| model.shapeable(id).ok());
        let lifting = shaping.is_some() && self.tool == Tool::ModifySubElements;
        let split_start = match self.tool {
            Tool::AddSplitLine(start) => start,
            _ => None,
        };
        let adding = shaping.is_some() && matches!(self.tool, Tool::AddPoint | Tool::AddSplitLine(_));
        let handles = shaping.as_ref().map(|f| shape_handles(&model, f).0).unwrap_or_default();
        let absolute = self.shape_absolute;
        let picked = self.shape_picked.clone();
        let shaped = self.shaping();
        // Where the floor's top is over plan point `q`; an edge's middle, between its ends.
        let top_at = |q: P2| -> Option<P3> {
            let f = shaping.as_ref()?;
            Some([q.0, q.1, model.floor_top(f).at(q) + model.shape_height(f, q)])
        };
        let anchor = |ends: &[P2]| -> Option<P3> {
            let tops: Vec<P3> = ends.iter().filter_map(|q| top_at(*q)).collect();
            let n = tops.len().max(1) as f64;
            (!tops.is_empty()).then(|| [0, 1, 2].map(|k| tops.iter().map(|t| t[k]).sum::<f64>() / n))
        };
        let o = &mut self.orbit;
        (o.rect, o.ppp) = (rect, ppp);
        let scene = scene_of(&mut o.scene, &model);
        // Through the view's lens while it's in perspective: the eye stays where it stood as the lens or the picture changes.
        let lens = lens_of(&model);
        let mut cam = match o.camera {
            Some(c) => c.sized(w, h, lens),
            None => {
                let look = Look::of_3d(&model);
                scene.fit_from(Camera { lens, ..scene.fit_in(AZIMUTH, ELEVATION, w, h, &look) }, &look)
            }
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
        // A drag from a grip of the floor being shaped raises or lowers that point or corner, to the millimetre; the command
        // runs when the drag ends.
        if lifting
            && o.grip.is_none()
            && o.boxing.is_none()
            && response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            let off = |p: &P3| on_screen(&cam, rect, ppp, *p).distance(origin);
            // A selected point's or edge's ▲ or ▼ takes all of it; else the grip there.
            let arrows = picked.iter().find_map(|s| {
                let at = anchor(&s.ends())?;
                let c = on_screen(&cam, rect, ppp, at);
                let near = [c + vec2(-12.0, -9.0), c + vec2(-12.0, 9.0)].iter().any(|g| g.distance(origin) <= 9.0);
                near.then(|| (s.ends(), at, origin))
            });
            o.lift = arrows.or_else(|| {
                handles
                    .iter()
                    .filter(|(_, p)| off(p) <= 8.0)
                    .min_by(|a, b| off(&a.1).total_cmp(&off(&b.1)))
                    .map(|(q, p)| (vec![*q], *p, origin))
            });
        }
        if !lifting {
            o.lift = None;
        }
        let lift_held = o.lift.is_some();
        let lifted_by = match (&o.lift, latest) {
            (Some((_, at, from)), Some(now)) => cam.along(2, *at, px(*from), px(now)).map(f64::round),
            _ => None,
        };
        // While it's dragged, the floor as it would be with those points or corners there.
        let preview = match (&shaping, &o.lift, lifted_by) {
            (Some(f), Some((qs, ..)), Some(by)) => {
                crate::shape::raised_all(&model, f, qs, by).map(|shape| bimcraft_core::model::Floor { shape, ..f.clone() })
            }
            _ => None,
        };
        let mut raise = None;
        if response.drag_stopped()
            && let Some((qs, ..)) = o.lift.take()
            && !self.canvas.quick()
        {
            raise = lifted_by.map(|by| (qs, by));
        }
        let still = o.lift.is_some();
        // The primary button, or Shift and the middle button, orbit, 0.4° a point; the middle
        // button alone pans.
        if response.drag_started()
            && o.grip.is_none()
            && o.boxing.is_none()
            && o.lift.is_none()
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
        let hovered =
            hover.filter(|_| (modify || demolish || paint || unpaint) && o.drag.is_none()).and_then(|at| pick(&o.picker, at)).map(|h| h.id);
        // The floor or roof being shaped is drawn as it is, not in the selection's colour: edit mode's marks are what's lit.
        let tints: Vec<(Id, Rgb)> = selection
            .iter()
            .filter(|id| Some(**id) != shaped)
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
        // Where a click lands on the floor being shaped, in plan: a point or corner within reach of it, else on an edge within
        // reach (the edge, to light up), else its top there.
        let snapped = |at: Pos2| -> Option<(P2, Option<[P3; 2]>)> {
            let f = shaping.as_ref()?;
            shape_snap_3d(&model, f, &cam, px(at), 8.0 * f64::from(ppp))
                .or_else(|| on_floor_top(&model, f, &cam, px(at)).map(|q| (q, None)))
        };
        let landing = |at: Pos2| snapped(at).map(|(q, _)| q);
        // The grip under the cursor, lit.
        let lit = hover.filter(|_| lifting && o.lift.is_none()).and_then(|h| {
            let off = |p: &P3| on_screen(&cam, rect, ppp, *p).distance(h);
            handles.iter().filter(|(_, p)| off(p) <= 8.0).min_by(|a, b| off(&a.1).total_cmp(&off(&b.1))).map(|(q, _)| *q)
        });
        let mut labels: Vec<(Pos2, P2, Vec<P2>, String)> = Vec::new();
        // Where the floor's top is over plan point `q`.
        let on_top = |q: P2| -> Option<P3> {
            let f = shaping.as_ref()?;
            Some([q.0, q.1, model.floor_top(f).at(q) + model.shape_height(f, q)])
        };
        if let Some(f) = &shaping {
            // The creases the drag would leave, dashed over the ones there are.
            for [a, b] in preview.as_ref().map(|f| model.shape_creases(f)).unwrap_or_default() {
                let (a, b) = (screen(a), screen(b));
                if a.is_finite() && b.is_finite() {
                    p.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(1.5, pal().accent), 6.0, 4.0));
                }
            }
            // Its points, edges, split lines and creases as edit mode colours them; the grip under the cursor lit, the
            // selection thick.
            crate::shape::draw_marks(&p, &crate::shape::marks(&model, f), screen, &picked, lit);
            let held: Vec<P2> = o.lift.as_ref().map(|(qs, ..)| qs.clone()).unwrap_or_default();
            let by = lifted_by.unwrap_or(0.0);
            // What's held, where the drag takes it.
            for at in held.iter().filter_map(|q| top_at(*q)) {
                let (from, to) = (screen(at), screen([at[0], at[1], at[2] + by]));
                if from.is_finite() && to.is_finite() {
                    p.line_segment([from, to], Stroke::new(1.5, pal().accent));
                    p.circle_filled(to, 5.5, pal().accent);
                }
            }
            // Each point's height beside it, and the selection's (an edge's at its middle, ▲/▼ beside it): a click on one
            // types it; a held one's follows the drag.
            let mut items: Vec<(P2, Vec<P2>)> = f.shape.points.iter().map(|x| (x.at, vec![x.at])).collect();
            for s in &picked {
                if !items.iter().any(|(q, _)| q.dist(s.at()) <= TOL) {
                    items.push((s.at(), s.ends()));
                }
            }
            for (q, ends) in items {
                let Some(at) = anchor(&ends) else { continue };
                let is_held = !held.is_empty() && ends.iter().all(|e| held.iter().any(|h| h.dist(*e) <= TOL));
                let lift = if is_held { by } else { 0.0 };
                let to = screen([at[0], at[1], at[2] + lift]);
                if !to.is_finite() {
                    continue;
                }
                let text = crate::shape::heights_text(&model, f, &ends, absolute, lift);
                if is_held {
                    crate::canvas::tag(&p, to + vec2(8.0, -8.0), egui::Align2::LEFT_BOTTOM, text);
                } else {
                    labels.push((to + vec2(8.0, -8.0), q, ends.clone(), text));
                }
                if lifting && picked.iter().any(|s| s.at().dist(q) <= TOL) {
                    crate::shape::arrows(&p, to);
                }
            }
            // Where a click would add a point, or end the split line begun; the edge it snaps to lit.
            if adding && let Some((q, edge)) = hover.and_then(snapped) {
                if let Some([a, b]) = edge {
                    let (a, b) = (screen(a), screen(b));
                    if a.is_finite() && b.is_finite() {
                        p.line_segment([a, b], Stroke::new(3.0, pal().accent));
                    }
                }
                if let Some(at) = on_top(q) {
                    let to = screen(at);
                    match split_start.and_then(on_top) {
                        Some(start) => {
                            p.line_segment([screen(start), to], Stroke::new(1.5, pal().accent));
                        }
                        None => {
                            p.circle_stroke(to, 4.0, Stroke::new(1.5, pal().accent));
                        }
                    }
                }
            }
        }
        // A click on a grip picks nothing behind it.
        let click = response.clicked().then(|| response.interact_pointer_pos()).flatten().or(self.canvas.quick_press());
        let click = click.filter(|at| !boxed.is_some_and(|b| modify && crate::section_box::grip_at(b, *at, screen).is_some()));
        // Shape Editing's clicks are the floor's, not the picture's; one on a height label is the label's.
        let label_rects: Vec<Rect> =
            labels.iter().map(|(at, _, _, text)| crate::shape::label_rect(ui, *at, text).expand2(vec2(18.0, 2.0))).collect();
        let shape_click = click.filter(|at| shaping.is_some() && !label_rects.iter().any(|r| r.contains(*at)));
        let shape_reach = 8.0 * f64::from(ppp);
        let landed = shape_click.map(|at| (landing(at), px(at)));
        let click = click.filter(|_| shaping.is_none());
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
        // This view's drag's preview, while it lasts; a plan's is its own.
        if lift_held {
            self.shape_preview = preview.filter(|_| still);
        }
        if shaping.is_none() {
            self.shape_label_edit = None;
        }
        for (at, q, ends, text) in labels {
            self.shape_label(ui, at, q, &ends, &text);
        }
        if let Some((qs, by)) = raise {
            self.shape_raise_all(&qs, by);
        }
        if let Some((landed, at)) = landed {
            match (&self.tool, landed) {
                (Tool::ModifySubElements, _) => {
                    let mods = click_modifiers(ui);
                    self.shape_select(|f| shape_pick_3d(&model, f, &cam, at, shape_reach), mods.shift || mods.command);
                }
                (Tool::AddPoint, Some(q)) => self.shape_add_point(q),
                (Tool::AddSplitLine(_), Some(q)) => self.shape_split(q),
                _ => self.status = Status { text: crate::lang::t("click on the floor's top").into(), error: true },
            }
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
