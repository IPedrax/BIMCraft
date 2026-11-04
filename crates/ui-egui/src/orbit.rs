//! The 3D view: the core's picture of the model (`view3d::Scene`), orbited, panned and zoomed as
//! in Revit. Shift and the middle button orbit, and so does a drag with the primary button (for a hand without a middle
//! button), round the point under the pointer where the drag began; the middle button pans; the wheel zooms toward the
//! point under the cursor. Shift and a primary drag in Modify are a selection box, as in a plan (`Scene::in_box`: from the
//! left what lies wholly inside, from the right what it touches; the sketch's lines while sketching). A
//! click picks the element the picture shows there, and so does a press let go quickly, however
//! far the hand went with it (`Canvas::quick_press`): the camera it turned goes back. A level
//! selected alone has a grip at the middle of each edge of its plane, which a drag moves over it. A section box is drawn
//! round what it keeps, with a grip on each face in Modify that a drag moves along its axis (`section_box.rs`). An Edit
//! Boundary begun here is sketched here, on the element's plane (`sketch3d.rs`): its clicks are the sketch's.
//! With Shape Editing's tools, the selected floor's points and corners have grips that drag their heights, and clicks
//! land on its top (`shape.rs`). The drawing tools (walls, runs, columns, components) take their points on the level face under
//! the cursor, snapped as that level's plan snaps; a run's ends may lie on two faces, and climb from one to the other.

use std::sync::Arc;

use bimcraft_i18n::msg;

use bimcraft_core::dims3d;
use bimcraft_core::geom::{self, P2};
use bimcraft_core::graphics::{Look, VisualStyle};
use bimcraft_core::model::{Dimension3D, Id, Model, Ref3, TOL, ViewCamera};
use bimcraft_core::render::{self, Rgb};
use bimcraft_core::snap::{self, Found, Kind, Reach, Snaps};
use bimcraft_core::view3d::gpu::{Hit, Mesh};
use bimcraft_core::view3d::{
    AZIMUTH, Camera, ELEVATION, Image, MAX_SIDE, P3, Scene, Surface, box_grips, move_box_face, on_floor_top, shape_handles, shape_pick_3d,
    shape_snap_3d,
};
use bimcraft_core::workplane::{CursorPlane, Frame, cursor_plane};
use egui::{
    Align, Color32, ColorImage, Layout, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, UiBuilder,
    pos2, vec2,
};
use serde_json::json;

use crate::canvas::{PICK, SNAP, click_modifiers, color};
use crate::icons::Icon;
use crate::lang::{t, tf};
use crate::sketch3d::Plane;
use crate::style::{canvas_pal, pal};
use crate::workplane::Picking;
use crate::{BimApp, Status, Tool, icon_button};

/// What a picture is of: the model, the camera, the elements tinted, the ground it is drawn on (which says whether it is dark
/// too), whether the card drew it, and the section box it is cut to while one is dragged.
type Shot = (Arc<Model>, Camera, Vec<(Id, Rgb)>, Color32, bool, Option<(P3, P3)>);

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

    /// The surface the picture shows at pixel `at`, taken through `cam`, and the point on it there; the processor's picture
    /// finds it in `scene`, the one it was drawn from (none while the picture on screen is of another model).
    fn surface(&self, scene: Option<&Scene>, cam: &Camera, at: (f64, f64)) -> Option<Surface> {
        match self {
            Picker::Image(i) => scene?.surface_in(i, cam, at),
            Picker::Mesh(m, tints) => m.surface(cam, tints, at.0, at.1),
        }
    }
}

/// The 3D view between frames.
pub struct Orbit {
    /// `None` until the view first shows, which fits it to the model.
    pub(crate) camera: Option<Camera>,
    /// Where the view showed last, in points, and the pixels a point holds.
    pub(crate) rect: Rect,
    pub(crate) ppp: f32,
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
    /// A drag under way: whether it orbits (else it pans), where the pointer was last, and the model point it orbits about,
    /// the one under the pointer where it began (none: the target).
    drag: Option<(bool, Pos2, Option<P3>)>,
    /// A selection box being dragged with Shift and the primary button: where it began.
    select: Option<Pos2>,
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
    /// The view and the camera its model held when the picture on screen was built from it or written back as: a difference
    /// means the camera changed elsewhere (dragged on a plan, a command, undo), so the picture stands from the model anew.
    pub(crate) built: Option<(Id, ViewCamera, Option<f64>)>,
    /// Each room's solid as the model shown last has it: its outline, bottom and top. Found once for each model.
    rooms: Option<(Arc<Model>, std::collections::BTreeMap<Id, RoomSolid>)>,
    /// The last point a drawing tool clicked here, in plan, and the plane it went on: a wall chain under way stays on it, and
    /// a run's end falls back to it where nothing is under the cursor.
    plane: Option<(P2, CursorPlane)>,
    /// While the Wall tool places a point in the 3D view, the base offset its walls take there: the height of the face they
    /// stand on over their level, which `BimApp::wall_params` puts in.
    pub(crate) wall_base: Option<f64>,
    /// What the Aligned Dimension tool has picked in the 3D view so far: its references, before a click places its line.
    pub(crate) dim3: Vec<Ref3>,
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
            picker: None,
            shown: None,
            texture: None,
            on_card: false,
            mesh: None,
            drag: None,
            select: None,
            before: None,
            grip: None,
            boxing: None,
            lift: None,
            built: None,
            rooms: None,
            plane: None,
            wall_base: None,
            dim3: Vec::new(),
        }
    }
}

/// Where a tool drawing in the 3D view aims for pixel `at`, through `cam`, before it snaps: on `held`, the plane of a wall
/// chain under way; else on the work plane of `under`, the surface the view shows there; with nothing there, on `fallback`
/// (the plane a run's start went on), else on the ground.
fn aim(
    m: &Model,
    cam: &Camera,
    at: (f64, f64),
    held: Option<CursorPlane>,
    fallback: Option<CursorPlane>,
    under: Option<Surface>,
) -> Option<(P3, CursorPlane)> {
    let plane = match (held, fallback, under) {
        (Some(p), ..) => p,
        (None, Some(p), None) => p,
        (None, _, under) => cursor_plane(m, cam, at, under)?,
    };
    Some((cam.on_plane(at, plane.z)?, plane))
}

/// Where a point aimed at `q` on `plane`, at pixel `at` (`ppp` pixels a point), lands: snapped as its level's plan snaps
/// (`snap::snap`: ends, middles, crossings, guides from `from`, else the nearest 10 mm), within as many mm of it as the snap's
/// points cover on the plane there.
#[allow(clippy::too_many_arguments)]
fn snap3d(m: &Model, cam: &Camera, at: (f64, f64), ppp: f32, q: P3, plane: CursorPlane, from: &[P2], square: bool, o: &Snaps) -> Found {
    let per_px = cam
        .on_plane((at.0 + 1.0, at.1), plane.z)
        .map(|r| (r[0] - q[0]).hypot(r[1] - q[1]))
        .filter(|d| d.is_finite() && *d > 0.0)
        .unwrap_or(1.0 / cam.scale.max(1e-9));
    let reach = Reach { point: SNAP * f64::from(ppp) * per_px, line: f64::from(PICK) * f64::from(ppp) * per_px };
    snap::snap(m, plane.level, P2(q[0], q[1]), reach, from, square, o)
}

/// What the Aligned Dimension tool picks at pixel `at` of the 3D view through `cam` (`ppp` pixels a point): a corner seen within
/// the snap's reach, else an edge within a pick's (the point of it nearest), else the point of `under`, the face there.
fn dim_ref(scene: &Scene, cam: &Camera, at: (f64, f64), under: Option<Surface>, ppp: f32, look: &Look) -> Option<Ref3> {
    let micron = |q: P3| q.map(|v| (v * 1e3).round() / 1e3 + 0.0);
    if let Some(c) = scene.corner_near(cam, at, SNAP * f64::from(ppp), look) {
        return Some(Ref3 { at: micron(c), edge: None });
    }
    if let Some(([a, b], q)) = scene.edge_near(cam, at, f64::from(PICK) * f64::from(ppp), look) {
        return Some(Ref3 { at: q.map(f64::round), edge: Some([micron(a), micron(b)]) });
    }
    under.map(|s| Ref3 { at: s.at.map(f64::round), edge: None })
}

/// Whether `tool` draws in the 3D view on the face under the cursor.
fn draws_on_faces(tool: &Tool) -> bool {
    matches!(tool, Tool::Wall(_) | Tool::Run(..) | Tool::Column | Tool::Stand(_) | Tool::Component | Tool::Fixture(_))
}

/// The point the tool's next goes on from, in plan: a wall chain's last, a run's start.
fn base_point(tool: &Tool) -> Option<P2> {
    match tool {
        Tool::Wall(chain) => chain.last().copied(),
        Tool::Run(_, start) => *start,
        _ => None,
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

    /// A drawing tool's click in the 3D view, its point `q` on work plane `plane`: the Wall tool's next point, its walls going
    /// on the plane's level with their base on the plane (`place`, as in a plan); a run's start, then its end, each as high
    /// as the face it was clicked on (a brace, from one face up to another); a column, a structural column, a footing or a
    /// component standing on the plane.
    fn draw3d(&mut self, q: P3, plane: CursorPlane) {
        let at = P2(q[0], q[1]);
        let m = Arc::clone(&self.session.model);
        match self.tool.clone() {
            Tool::Wall(_) => {
                self.orbit.wall_base = Some(plane.offset);
                self.place(plane.level, at);
                self.orbit.wall_base = None;
            }
            Tool::Run(_, start) => {
                // Its start's plane, where it was clicked here; else the end's.
                let from = self.orbit.plane.filter(|(p, _)| Some(*p) == start).map_or(plane, |(_, p)| p);
                let micron = |v: f64| (v * 1e3).round() / 1e3 + 0.0;
                self.place_run_lifted(from.level, at, (from.offset, micron(plane.z - m.elevation(from.level))));
            }
            Tool::Column => self.place_column(plane.level, at, plane.offset),
            Tool::Stand(category) => self.place_stand_lifted(plane.level, at, category, plane.offset),
            Tool::Component | Tool::Fixture(_) => {
                if let Some((family, ty)) = self.placing() {
                    let p = json!({"family": family, "type": ty, "at": [at.0, at.1], "level": plane.level, "offset": plane.offset,
                                   "angle": self.component_angle});
                    self.run("component", p);
                }
            }
            _ => {}
        }
        self.orbit.plane = Some((at, plane));
    }

    /// The Aligned Dimension tool's click in the 3D view at pixel `at` through `cam`, `r` what it picks there: its first and
    /// second references, then where its line goes, which places it (`dimension_3d`) and says how long it is.
    fn dim3_click(&mut self, cam: &Camera, at: (f64, f64), r: Option<Ref3>) {
        let picks = std::mem::take(&mut self.orbit.dim3);
        match (picks.as_slice(), r) {
            (&[a, b], _) => {
                let (p, q) = bimcraft_core::dims3d::ends([a, b]);
                let Some(on) = bimcraft_core::dims3d::line_through(cam, p, q, at) else {
                    self.status = Status { text: t("The view runs along the dimension: turn it to place its line.").into(), error: true };
                    self.orbit.dim3 = picks;
                    return;
                };
                let mut params = json!({"refs": [a, b], "at": on.map(f64::round)});
                if let Some(v) = self.active_3d() {
                    params["view"] = json!(v);
                }
                if let Some(made) = self.run("dimension_3d", params) {
                    let long = self.session.model.units.with_unit(made["length"].as_f64().unwrap_or(0.0));
                    self.status = Status { text: tf("Aligned dimension: {}", &[&long]), error: false };
                }
            }
            (_, None) => {
                self.status = Status { text: t("Pick a corner, an edge or a face of the model to dimension.").into(), error: true };
                self.orbit.dim3 = picks;
            }
            (_, Some(r)) => {
                self.orbit.dim3 = picks;
                self.orbit.dim3.push(r);
            }
        }
    }

    /// What the tool on needs under the cursor in the 3D view and found none of there: a level face to stand on or to take
    /// its points on.
    fn needs_face(&self) -> String {
        let title = match &self.tool {
            Tool::Wall(_) => Some(msg!("Place Wall")),
            Tool::Column => Some(msg!("Place Column")),
            Tool::Run(..) | Tool::Stand(_) => crate::structure::title(&self.tool),
            Tool::Component => Some(msg!("Place Component")),
            Tool::Fixture(key) => crate::mep::fixture(key).map(|f| f.title),
            _ => None,
        };
        let title = title.map(t).unwrap_or_default();
        if matches!(self.tool, Tool::Wall(_) | Tool::Run(..)) {
            tf(
                "{}: in the 3D view its points go on a level face (the top of a floor, a column, a flat roof or a level): point at one",
                &[title],
            )
        } else {
            tf("{}: in the 3D view it stands on a level face (the top of a floor, a flat roof or a level): point at one", &[title])
        }
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
        // An Edit Boundary begun here: its lines on the element's plane, a drag from a picked one's end holding its grip.
        let sketch_z = self.sketch_surface();
        let last = self.orbit.camera.map(|c| Camera { width: w, height: h, ..c });
        let held = self.sketch3d_grab(ui, &response, sketch_z.zip(last).map(|(surface, cam)| Plane { cam, rect, ppp, surface }));
        // Pick in View, taking the work plane's surface or points: clicks are its, not the selection's.
        let picking = self.work_plane_picking().is_some();
        // The work plane's patch, in Modify while nothing is sketched: a click on it opens the Work Plane dialog.
        let patch =
            (self.tool == Tool::Modify && self.sketch.is_none() && !picking).then(|| self.work_plane_ring(last, rect, ppp)).flatten();
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
        // How high above the level a click would land: Add Point's point as the Options Bar says, a split line's end on the top.
        let (point_tool, along) = (self.tool == Tool::AddPoint, self.shape_along);
        let offset = model.units.parse(&self.shape_offset).unwrap_or(0.0);
        let typed_z = model.units.parse(&self.shape_z);
        let handles = shaping.as_ref().map(|f| shape_handles(&model, f).0).unwrap_or_default();
        let base = self.shape_base;
        // Modify Sub Elements' selection, shown only while it's on.
        let picked = if self.tool == Tool::ModifySubElements { self.shape_picked.clone() } else { Vec::new() };
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
        // The 3D view on screen: a camera view stands where its camera looks from; the default is fitted over the model,
        // and its lens is the one it is seen through.
        let active = self.active_3d();
        let three_d = active.and_then(|id| match self.session.model.elements.get(id) {
            Some(bimcraft_core::model::Element::View3D(v)) => Some((v.camera, v.lens())),
            _ => None,
        });
        let lens = three_d.and_then(|(_, l)| l);
        let stored = three_d.and_then(|(c, _)| c);
        let snaps = self.snaps_now();
        // The points a drawing tool's next snaps from (guides, perpendiculars), as in a plan.
        let from: Vec<P2> = match (&self.tool, self.shape()) {
            (Tool::Wall(chain), crate::draw::Shape::Line) => chain.clone(),
            (Tool::Wall(chain), _) if chain.len() < 2 => chain.clone(),
            (Tool::Run(_, start), _) => start.iter().copied().collect(),
            _ => Vec::new(),
        };
        let o = &mut self.orbit;
        (o.rect, o.ppp) = (rect, ppp);
        // The camera changed elsewhere than by the hand in this view (dragged on a plan, a command, undo): the picture stands
        // from the model afresh, so the old one it showed isn't written back over the change.
        let elsewhere = match (o.built, active, stored) {
            (Some((id, cam, l)), Some(active), Some(stored)) => id != active || cam != stored || l != lens,
            _ => false,
        };
        if elsewhere && o.camera.is_some() && o.drag.is_none() && wheel == 0.0 && pinch == 1.0 {
            o.camera = None;
        }
        let scene = scene_of(&mut o.scene, &model);
        // Through the view's lens while it's in perspective: the eye stays where it stood as the lens or the picture changes.
        let from_model = o.camera.is_none();
        let mut cam = match o.camera {
            Some(c) => c.sized(w, h, lens),
            None => {
                let look = Look::of_3d(&model);
                match (three_d.and_then(|(c, _)| c), lens) {
                    (Some(cam), Some(lens)) => Camera::at(cam.eye, cam.target, lens, w, h),
                    _ => scene.fit_from(Camera { lens, ..scene.fit_in(AZIMUTH, ELEVATION, w, h, &look) }, &look),
                }
            }
        };
        if from_model {
            o.built = active.zip(stored).map(|(id, cam)| (id, cam, lens));
        }
        // What the picture on screen shows at a point, as the view picks: its scene is this model's while it's of this model.
        let fresh = o.shown.as_ref().is_some_and(|s| Arc::ptr_eq(&s.0, &model));
        let surface = |picker: &Option<Picker>, cam: &Camera, at: Pos2| picker.as_ref()?.surface(fresh.then_some(scene), cam, px(at));
        if let Some(at) = hover {
            // A wheel notch (about 50 points) zooms by a fifth, toward what's under the cursor.
            let factor = (f64::from(wheel) * 0.004).exp() * f64::from(pinch);
            if factor.is_finite() && factor != 1.0 {
                let factor = factor.clamp(0.2, 5.0);
                cam = match surface(&o.picker, &cam, at) {
                    Some(s) => cam.zoom_about(factor, s.at),
                    None => cam.zoom(factor, px(at)),
                };
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
            && !held
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
        // The primary button, or Shift and the middle button, orbit, 0.4° a point, round the point under the pointer where
        // the drag began (the target, where nothing is); the middle button alone pans. Shift and the primary button box, in
        // Modify (the sketch's lines while sketching).
        if response.drag_started()
            && o.grip.is_none()
            && o.boxing.is_none()
            && !held
            && o.lift.is_none()
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            let primary = response.dragged_by(PointerButton::Primary);
            if primary && modify && shift {
                o.select = Some(origin);
            } else if primary || response.dragged_by(PointerButton::Middle) {
                let orbit = primary || shift;
                let pivot = orbit.then(|| surface(&o.picker, &cam, origin)).flatten().map(|s| s.at);
                o.drag = Some((orbit, origin, pivot));
                o.before = Some(cam);
            }
        }
        let mut selected_box = None;
        if response.drag_stopped()
            && let Some(from) = o.select.take()
            && !self.canvas.quick()
            && let Some(to) = latest
        {
            selected_box = Some((from, to));
        }
        if let (Some((orbit, last, pivot)), Some(now)) = (o.drag, latest)
            && response.dragged()
        {
            let d = now - last;
            let (around, up) = (-f64::from(d.x) * 0.4, f64::from(d.y) * 0.4);
            cam = if orbit { cam.orbit_about(around, up, pivot.unwrap_or(cam.target)) } else { cam.pan(px(last), px(now)) };
            o.drag = Some((orbit, now, pivot));
        }
        if response.drag_stopped() {
            o.drag = None;
            // A click made on the move: the camera the hand turned meanwhile goes back.
            if let (true, Some(before)) = (self.canvas.quick(), o.before.take()) {
                cam = before;
            }
        }
        o.camera = Some(cam);
        // The hand and the wheel have let the camera be: a camera view's position is written back then (below).
        let settled = o.drag.is_none() && wheel == 0.0 && pinch == 1.0;
        // A box let go: what it picks, as the picture is now.
        let boxed_ids = selected_box
            .filter(|_| sketch_z.is_none())
            .map(|(from, to)| scene.in_box(&cam, px(from), px(to), to.x < from.x, &Look::of_3d(&model)));
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
        // The view's dimensions, as the core draws them through this camera: over the picture, whichever painter drew it, and
        // picked where they are drawn.
        let look3 = Look::of_3d(&model);
        let marks = active.map(|v| dims3d::marks(&model, v, &cam, &look3, dims3d::SCREEN * f64::from(ppp))).unwrap_or_default();
        let dim_at = |at: Pos2| dims3d::pick(&marks, px(at), f64::from(PICK) * f64::from(ppp));
        let lit_dim = hover.filter(|_| modify && o.drag.is_none() && sketch_z.is_none() && !picking).and_then(dim_at);
        let hovered = hover
            .filter(|_| (modify || demolish || paint || unpaint) && o.drag.is_none() && sketch_z.is_none() && !picking)
            .filter(|_| lit_dim.is_none())
            .and_then(|at| pick(&o.picker, at))
            .map(|h| h.id);
        // The floor or roof being shaped is drawn as it is, not in the selection's colour: edit mode's marks are what's lit.
        let tints: Vec<(Id, Rgb)> = selection
            .iter()
            .filter(|id| Some(**id) != shaped)
            .map(|id| (*id, rgb(canvas_pal().accent)))
            .chain(hovered.filter(|id| !selection.contains(id)).map(|id| (id, rgb(canvas_pal().hover))))
            .collect();
        // The ground, which a canvas colour of the user's changes without making the drawing dark or light.
        let dark = color(render::PAPER);
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
        let on_view = |(x, y): (f32, f32)| rect.min + vec2(x / ppp, y / ppp);
        let draw_marks = |marks: &[dims3d::Mark], ink: &dyn Fn(Id, Rgb) -> Color32| {
            for mk in marks {
                p.add(egui::Shape::line(
                    mk.pts.iter().copied().map(on_view).collect(),
                    Stroke::new(mk.width / ppp, ink(mk.owner, mk.color)),
                ));
            }
        };
        draw_marks(&marks, &|owner, c| {
            if selection.contains(&owner) {
                canvas_pal().accent
            } else if lit_dim == Some(owner) {
                canvas_pal().hover
            } else {
                color(c)
            }
        });
        // The Aligned Dimension tool: what a click would pick, and once two are picked, the dimension its line would make.
        let dimming = matches!(self.tool, Tool::Dimension(..)) && sketch_z.is_none() && !picking;
        if !dimming {
            o.dim3.clear();
        }
        let dim_pick = |at: Pos2| dim_ref(scene, &cam, px(at), surface(&o.picker, &cam, at), ppp, &look3);
        if let Some(at) = hover.filter(|_| dimming && o.drag.is_none()) {
            let accent = Stroke::new(1.5, pal().accent);
            let screen = |q: P3| on_screen(&cam, rect, ppp, q);
            match o.dim3.as_slice() {
                &[a, b] => {
                    let (from, to) = dims3d::ends([a, b]);
                    if let Some(on) = dims3d::line_through(&cam, from, to, px(at)) {
                        let type_name = bimcraft_core::dims::default_type(&model.dimension_types, bimcraft_core::model::DimKind::Aligned);
                        let d = Dimension3D {
                            view: Default::default(),
                            refs: [a, b],
                            at: on,
                            type_name: type_name.cloned().unwrap_or_default(),
                        };
                        draw_marks(&dims3d::marks_of(&model, 0, &d, &cam, dims3d::SCREEN * f64::from(ppp)), &|_, _| pal().accent);
                    }
                }
                picked => {
                    for r in picked {
                        p.circle_filled(screen(r.at), 3.5, pal().accent);
                    }
                    if let Some(r) = dim_pick(at) {
                        if let Some([a, b]) = r.edge {
                            p.line_segment([screen(a), screen(b)], Stroke::new(3.0, pal().accent));
                        }
                        p.circle_stroke(screen(r.at), 5.0, accent);
                        if let Some(first) = picked.first() {
                            p.extend(egui::Shape::dashed_line(&[screen(first.at), screen(r.at)], accent, 6.0, 4.0));
                        }
                    }
                }
            }
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
        for (id, e) in model.elements.iter() {
            let shown = selection.contains(id) || look.as_ref().is_some_and(|l| l.shows(*id));
            if let (true, bimcraft_core::model::Element::Room(r)) = (shown, e) {
                let solid = solids.entry(*id).or_insert_with(|| {
                    let (bottom, top) = model.room_span(r);
                    (model.room_outline(r).0, bottom, top)
                });
                room_solid(&p, solid, selection.contains(id), &|q: P3| on_screen(&cam, rect, ppp, q));
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
        // The drawing tools take their points on the face under the cursor (`workplane`), snapped as its level's plan snaps:
        // a wall chain under way on the plane its first point found, a run's end on the face it is clicked on (where there is
        // none, the plane of its start). The point is shown as a plan shows a snap, and the wall or run from the point before.
        let drawing = draws_on_faces(&self.tool) && sketch_z.is_none();
        let last3 = o.plane.filter(|(q, _)| base_point(&self.tool) == Some(*q));
        let held = last3.filter(|_| matches!(self.tool, Tool::Wall(_))).map(|(_, p)| p);
        let fallback = last3.filter(|_| matches!(self.tool, Tool::Run(..))).map(|(_, p)| p);
        let aimed = |picker: &Option<Picker>, at: Pos2| -> Option<(P3, CursorPlane, Found)> {
            let under = picker.as_ref().and_then(|k| k.surface(Some(scene), &cam, px(at)));
            let (q, plane) = aim(&model, &cam, px(at), held, fallback, under)?;
            let f = snap3d(&model, &cam, px(at), ppp, q, plane, &from, shift, &snaps);
            Some(([f.at.0, f.at.1, plane.z], plane, f))
        };
        if let Some((q, plane, found)) = hover.filter(|_| drawing && o.drag.is_none()).and_then(|at| aimed(&o.picker, at))
            && let at = screen(q)
            && at.is_finite()
        {
            // The point before, at the height of the plane it went on.
            let before = base_point(&self.tool).map(|b| [b.0, b.1, last3.map_or(plane.z, |(_, p)| p.z)]);
            if let Some(b) = before
                && let from = screen(b)
                && from.is_finite()
            {
                p.line_segment([from, at], Stroke::new(1.0, pal().accent));
                let long = P2(b[0], b[1]).dist(P2(q[0], q[1])).hypot(q[2] - b[2]);
                crate::canvas::tag(&p, at + vec2(12.0, -12.0), egui::Align2::LEFT_BOTTOM, model.units.with_unit(long));
            }
            let diamond = [vec2(0.0, -6.0), vec2(6.0, 0.0), vec2(0.0, 6.0), vec2(-6.0, 0.0)].map(|d| at + d).to_vec();
            p.add(egui::Shape::closed_line(diamond, Stroke::new(2.0, pal().accent)));
            // What it snapped to, of which element; a free point, the face it's on.
            let (kind, of) = match found.kind {
                Kind::Free => (Kind::Nearest, Some(plane.on)),
                k => (k, found.source),
            };
            let name = match of.and_then(|id| Some((id, model.elements.get(id)?))) {
                Some((id, e)) => tf("{} of {} {}", &[t(kind.name()), &crate::lang::category(e.category(), false), &id.to_string()]),
                None => t(kind.name()).to_string(),
            };
            crate::canvas::tag(&p, at + vec2(12.0, 14.0), egui::Align2::LEFT_TOP, name);
        }
        // Where a click lands on the floor being shaped, in plan: a point or corner within reach of it, else on an edge within
        // reach (the edge, to light up), else its top there.
        let snapped = |at: Pos2| -> Option<(P2, Option<[P3; 2]>, Option<bimcraft_core::view3d::ShapeSnap>)> {
            let f = shaping.as_ref()?;
            shape_snap_3d(&model, f, &cam, px(at), 8.0 * f64::from(ppp))
                .map(|(q, e, k)| (q, e, Some(k)))
                .or_else(|| on_floor_top(&model, f, &cam, px(at)).map(|q| (q, None, None)))
        };
        let landing = |at: Pos2| snapped(at).map(|(q, ..)| q);
        // What a click there would pick, lit: a point or an edge.
        let lit = hover
            .filter(|_| lifting && o.lift.is_none())
            .and_then(|h| crate::shape::Sub::of(&shape_pick_3d(&model, shaping.as_ref()?, &cam, px(h), 8.0 * f64::from(ppp))));
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
            // A selected point's offset from the top as it would be without the shape, as Revit dimensions it: plumb, an
            // arrow at each end, the distance beside it.
            for s in &picked {
                let crate::shape::Sub::Point(q) = *s else { continue };
                let (flat, up) = (model.floor_top(f).at(q), model.shape_height(f, q));
                if up.abs() < 0.5 {
                    continue;
                }
                let (a, b) = (screen([q.0, q.1, flat]), screen([q.0, q.1, flat + up]));
                if !(a.is_finite() && b.is_finite()) || a.distance(b) < 12.0 {
                    continue;
                }
                // Left of its ▲/▼, clear of its label on the right.
                let (a, b) = (a + vec2(-26.0, 0.0), b + vec2(-26.0, 0.0));
                p.line_segment([a, b], Stroke::new(1.0, pal().accent));
                for (tip, from) in [(a, b), (b, a)] {
                    let u = (tip - from).normalized();
                    let n = vec2(-u.y, u.x) * 3.0;
                    p.add(egui::Shape::convex_polygon(vec![tip, tip - u * 7.0 + n, tip - u * 7.0 - n], pal().accent, Stroke::NONE));
                }
                crate::canvas::tag(&p, a + (b - a) * 0.5 + vec2(-6.0, 0.0), egui::Align2::RIGHT_CENTER, model.units.length(up.abs()));
            }
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
                let text = crate::shape::heights_text(&model, f, &ends, base, lift);
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
            if adding && let Some((q, edge, kind)) = hover.and_then(snapped) {
                if let Some([a, b]) = edge {
                    let (a, b) = (screen(a), screen(b));
                    if a.is_finite() && b.is_finite() {
                        p.line_segment([a, b], Stroke::new(3.0, pal().accent));
                    }
                }
                if let Some(at) = on_top(q) {
                    let to = screen(at);
                    let surface = at[2] - base.at(&model, f, q);
                    let z = match (point_tool, along) {
                        (true, true) => surface + offset,
                        (true, false) => typed_z.unwrap_or(surface),
                        _ => surface,
                    };
                    crate::canvas::tag(&p, to + vec2(12.0, -12.0), egui::Align2::LEFT_BOTTOM, model.units.length(z));
                    match split_start.and_then(on_top) {
                        Some(start) => {
                            p.line_segment([screen(start), to], Stroke::new(1.5, pal().accent));
                        }
                        None if kind.is_none() => {
                            p.circle_stroke(to, 4.0, Stroke::new(1.5, pal().accent));
                        }
                        None => {}
                    }
                    if let Some(kind) = kind {
                        crate::shape::snap_marker(&p, to, kind);
                    }
                }
            }
        }
        // A click on a grip picks nothing behind it.
        let click = response.clicked().then(|| response.interact_pointer_pos()).flatten().or(self.canvas.quick_press());
        let click = click.filter(|at| !boxed.is_some_and(|b| modify && crate::section_box::grip_at(b, *at, screen).is_some()));
        let on_patch = |at: Pos2| patch.is_some_and(|ring| crate::workplane::on_patch(&ring, at));
        let patched = click.is_some_and(on_patch);
        let click = click.filter(|_| !patched);
        // Shape Editing's clicks are the floor's, not the picture's; one on a height label is the label's.
        let label_rects: Vec<Rect> =
            labels.iter().map(|(at, _, _, text)| crate::shape::label_rect(ui, *at, text).expand2(vec2(18.0, 2.0))).collect();
        let shape_click = click.filter(|at| shaping.is_some() && !label_rects.iter().any(|r| r.contains(*at)));
        let shape_reach = 8.0 * f64::from(ppp);
        let landed = shape_click.map(|at| (landing(at), px(at)));
        let click = click.filter(|_| shaping.is_none());
        // While sketching here a click is the sketch's, with what's under it for Pick Walls and Pick Lines.
        let under = click.filter(|_| sketch_z.is_some()).and_then(|at| pick(&o.picker, at)).map(|h| h.id);
        // A dimension's lines are picked before what the picture shows under them.
        let clicked = click
            .filter(|_| sketch_z.is_none() && !picking)
            .map(|at| dim_at(at).filter(|_| modify).or_else(|| pick(&o.picker, at).map(|h| h.id)));
        let dim_click = click.filter(|_| dimming).map(|at| (px(at), dim_pick(at)));
        // The face of what's clicked, as paint names the faces of an element.
        let face = click.and_then(|at| pick(&o.picker, at).map(|h| h.face).filter(|f| !f.is_empty()));
        let target = click.filter(|_| drawing).and_then(|at| aimed(&o.picker, at));
        match clicked {
            Some(found) if modify => self.pick_up(found.into_iter().collect(), click_modifiers(ui)),
            Some(Some(id)) if demolish => self.demolish(id),
            Some(Some(id)) if paint || unpaint => self.paint_face(id, face, unpaint),
            // Nothing under the cursor: nothing to take down, or to paint.
            Some(None) if demolish || paint || unpaint => {}
            Some(_) if drawing => match target {
                Some((q, plane, _)) => self.draw3d(q, plane),
                None => self.status = Status { text: self.needs_face(), error: true },
            },
            Some(_) if dimming => {}
            Some(_) => {
                self.status =
                    Status { text: crate::lang::t("this tool works in plan views: open one from the Project Browser").into(), error: true }
            }
            None => {}
        }
        if let Some((at, r)) = dim_click {
            self.dim3_click(&cam, at, r);
        }
        // The selection box being dragged, and what one let go picks.
        if let (Some(from), Some(now)) = (self.orbit.select, latest) {
            let local = |q: Pos2| (q.x - rect.min.x, q.y - rect.min.y);
            p.extend(crate::canvas::box_shapes(rect, local(from), local(now)));
        }
        if let Some(found) = boxed_ids {
            self.pick_up(found, click_modifiers(ui));
        }
        if let Some(surface) = sketch_z {
            let plane = Plane { cam, rect, ppp, surface };
            if let Some((from, to)) = selected_box {
                self.sketch3d_box(&plane, from, to, click_modifiers(ui));
            }
            self.sketch3d(ui, &response, &p, plane, click, under);
        }
        if let Some(ring) = patch {
            let name = self.work_plane_name();
            crate::workplane::draw_patch(&p, ring, hover.is_some_and(on_patch), &name);
        }
        if patched {
            self.open_work_plane();
        }
        // The work plane, drawn while it's being set or a new outline is sketched on it; and what Pick in View would take.
        let m = &self.session.model;
        let new_sketch = self.sketch.as_ref().and_then(|sk| sk.surface.filter(|_| sk.plan == crate::Sheet::View3D));
        if (self.work_plane.is_some() || new_sketch.is_some())
            && let Some(extent) = m.model_box()
            && let Ok(plane) = m.work_plane(m.default_3d()).resolve(m)
        {
            let plane = new_sketch.map_or(plane, Frame::from);
            crate::workplane::draw_plane(&p, plane, extent, 2000.0, screen);
        }
        if let (Some(picking), Some((_, scene))) = (self.work_plane_picking(), &self.orbit.scene) {
            let look = Look::of_3d(m);
            let aim = |at: Pos2| crate::workplane::aim(scene, &cam, px(at), picking, &look, ppp);
            if let Picking::Points(picked) = picking {
                for q in picked {
                    p.circle_filled(screen(*q), 4.0, pal().accent);
                }
            }
            if let Some(a) = hover.and_then(aim) {
                crate::workplane::draw_aim(&p, &a, screen);
            }
            if let Some(at) = click {
                let aimed = aim(at);
                self.work_plane_pick(aimed);
            }
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
                self.orbit.built = None;
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
        // A camera view remembers where the hand and the wheel left it: once they let it be, its eye and target are written
        // back, so leaving the view and returning comes back to the same place (and Properties shows them). A view drawn
        // beside the active one (a floating window) writes nothing: the hand is working in the active view.
        if settled
            && !self.passive
            && matches!(three_d, Some((Some(_), _)))
            && let (Some(eye), Some(lens)) = (cam.eye(), lens)
            && let Some(id) = active
        {
            let stored = match self.session.model.elements.get(id) {
                Some(bimcraft_core::model::Element::View3D(v)) => v.camera,
                _ => None,
            };
            let near = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01);
            if !stored.is_some_and(|c| near(c.eye, eye) && near(c.target, cam.target))
                && self.run("camera", json!({"view": id, "eye": eye, "target": cam.target, "lens": lens})).is_some()
            {
                // What the model holds now is what the picture shows.
                self.orbit.built = Some((id, ViewCamera { eye, target: cam.target }, Some(lens)));
            }
        }
    }
}
