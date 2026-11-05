//! The 3D view: the core's picture of the model (`view3d::Scene`), orbited, panned and zoomed as
//! in Revit. Shift and the middle button orbit, round the point under the pointer where the drag began; the middle button
//! pans; the wheel zooms toward the point under the cursor. A primary drag in Modify is a selection box, as in a plan, and
//! never turns the view (`Scene::in_box`: from the left what lies wholly inside, from the right what it touches; the sketch's
//! lines while sketching); with Shift held it adds to the selection. A
//! click picks the element the picture shows there, and so does a press let go quickly, however
//! far the hand went with it (`Canvas::quick_press`): the camera it turned goes back. A level
//! selected alone has a grip at the middle of each edge of its plane, which a drag moves over it. A section box is drawn
//! round what it keeps, with a grip on each face in Modify that a drag moves along its axis (`section_box.rs`). An Edit
//! Boundary begun here is sketched here, on the element's plane (`sketch3d.rs`): its clicks are the sketch's.
//! With Shape Editing's tools, the selected floor's points and corners have grips that drag their heights, and clicks
//! land on its top (`shape.rs`). The drawing tools (walls, runs, columns, components) take their points on the level face under
//! the cursor, snapped as that level's plan snaps; a run's ends may lie on two faces, and climb from one to the other.
//! The scene and the card's mesh are made on a worker (`view_worker.rs`), an edit's from the scene before; meanwhile the view
//! shows the last picture, then faded with what it waits for.
//!
//! What any 3D view needs is the `Orbit`'s: `input` reads the frame (the triad's widget first), `zoom` and `navigate` move the
//! camera, `paint` draws the picture on the card or the processor with what picks in it, and the ViewCube is drawn from a camera
//! alone. The project's tools (grips, the section box, shape editing, sketching, dimensions, the work plane) stay in
//! `BimApp::view3d`, between them.

use std::sync::Arc;

use bimcraft_i18n::msg;

use bimcraft_core::handles::{self, Handle};
use bimcraft_core::dims3d;
use bimcraft_core::geom::{self, P2};
use bimcraft_core::graphics::{Look, VisualStyle};
use bimcraft_core::model::{Dimension3D, Id, Model, Ref3, TOL, ViewCamera};
use bimcraft_core::render::{self, Rgb};
use bimcraft_core::snap::{self, Found, Kind, Reach, Snaps};
use bimcraft_core::view3d::gpu::{Hit, Mesh};
use bimcraft_core::view3d::{
    AZIMUTH, Camera, ELEVATION, Image, MAX_SIDE, P3, PlaneMark, Scene, Surface, box_grips, move_box_face, on_floor_top, shape_handles,
    shape_pick_3d, shape_snap_3d,
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
use crate::triad::Ask;
use crate::workplane::Picking;
use crate::{BimApp, Status, Tool, icon_button};

/// What a picture is of: the model, the camera, the elements tinted, the ground it is drawn on (which says whether it is dark
/// too), whether the card drew it, and the section box it is cut to while one is dragged.
type Shot = (Arc<Model>, Camera, Vec<(Id, Rgb)>, Color32, bool, Option<(P3, P3)>);

/// A model's mesh for the card, or why the card can't draw it.
pub(crate) type Meshed = (Arc<Model>, Result<Arc<Mesh>, String>);

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

/// What picks in the picture on screen: the processor's own picture, or the mesh the card drew it from (with its tints and the
/// section box it was cut to).
enum Picker {
    Image(Image),
    Mesh(Arc<Mesh>, Vec<(Id, Rgb)>, Option<(P3, P3)>),
}

impl Picker {
    /// What the picture shows at pixel `(x, y)`, taken through `cam`.
    fn at(&self, cam: &Camera, x: f64, y: f64) -> Option<Hit> {
        match self {
            Picker::Image(i) => Some(Hit { id: i.pick(x, y)?, face: i.face(x, y).unwrap_or("") }),
            Picker::Mesh(m, tints, cut) => m.pick_cut(cam, tints, *cut, x, y),
        }
    }

    /// The surface the picture shows at pixel `at`, taken through `cam`, and the point on it there; the processor's picture
    /// finds it in `scene`, the one it was drawn from (none while the picture on screen is of another model).
    fn surface(&self, scene: Option<&Scene>, cam: &Camera, at: (f64, f64)) -> Option<Surface> {
        match self {
            Picker::Image(i) => scene?.surface_in(i, cam, at),
            Picker::Mesh(m, tints, cut) => m.surface_cut(cam, tints, *cut, at.0, at.1),
        }
    }
}

/// How the view shows while its scene or mesh is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Waiting {
    /// The last picture as it was, as a short wait shouldn't flicker.
    Quiet,
    /// The last picture faded, with the words over it.
    Faded,
    /// No picture yet: the words on the paper.
    Bare,
}

impl Waiting {
    /// How a wait `elapsed` long shows: with a last `picture` or none, and whether it has something to say `now`.
    pub(crate) fn after(elapsed: std::time::Duration, picture: bool, now: bool) -> Waiting {
        match (picture, now || elapsed >= WAIT_QUIET) {
            (false, _) => Waiting::Bare,
            (true, false) => Waiting::Quiet,
            (true, true) => Waiting::Faded,
        }
    }
}

/// How long a wait shows the last picture as it was, before it fades and says why.
const WAIT_QUIET: std::time::Duration = std::time::Duration::from_millis(200);
/// How much of the paper lies over the last picture in a longer wait.
const WAIT_FADE: u8 = 150;

/// What the hand, the wheel and the navigation triad did over a 3D view in a frame, read as the frame begins.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Input {
    /// Where the view is, in points, and the pixels a point holds; the picture's size, pixels.
    pub(crate) rect: Rect,
    pub(crate) ppp: f32,
    pub(crate) size: (u32, u32),
    /// The pointer over the view (none off it, or over the triad), the wheel's and a pinch's zoom, Shift held, and where the
    /// pointer was last, on the view or off it.
    pub(crate) hover: Option<Pos2>,
    pub(crate) wheel: f32,
    pub(crate) pinch: f32,
    pub(crate) shift: bool,
    pub(crate) latest: Option<Pos2>,
    /// What a click on the triad asked for.
    pub(crate) asked: Option<Ask>,
    /// The 3D mouse's cap off rest, while the window has focus (the wheel is left alone meanwhile: 3DxWare may make wheel turns
    /// of the cap), and how long a frame lasts, seconds.
    pub(crate) motion: Option<crate::spacemouse::Motion>,
    pub(crate) frame: f64,
}

impl Input {
    /// The frame's input over a view in `rect` that draws in points, a pixel a point (the adaptive modelling view's), its widget's
    /// `response`; with no triad.
    pub(crate) fn in_points(ui: &Ui, rect: Rect, response: &egui::Response) -> Input {
        Input::read(ui, rect, response, 1.0, None, None)
    }

    /// The frame's input over a view in `rect`, `ppp` pixels a point, its widget's `response`; `asked`, what the triad was; and
    /// `device`, the 3D mouse's cap, which counts only while the window has focus.
    fn read(
        ui: &Ui,
        rect: Rect,
        response: &egui::Response,
        ppp: f32,
        asked: Option<Ask>,
        device: Option<crate::spacemouse::Motion>,
    ) -> Input {
        let side = |v: f32| ((v * ppp).round().max(1.0) as u32).min(MAX_SIDE);
        let (hover, wheel, pinch, shift, latest, focused, frame) = ui.input(|i| {
            let hover = i.pointer.hover_pos().filter(|_| response.hovered());
            (hover, i.smooth_scroll_delta.y, i.zoom_delta(), i.modifiers.shift, i.pointer.latest_pos(), i.focused, f64::from(i.stable_dt))
        });
        let motion = device.filter(|_| focused);
        let wheel = if motion.is_some() { 0.0 } else { wheel };
        Input { rect, ppp, size: (side(rect.width()), side(rect.height())), hover, wheel, pinch, shift, latest, asked, motion, frame }
    }

    /// Screen point `p` as a pixel of the picture.
    pub(crate) fn px(&self, p: Pos2) -> (f64, f64) {
        (f64::from((p.x - self.rect.min.x) * self.ppp), f64::from((p.y - self.rect.min.y) * self.ppp))
    }
}

/// What a frame's navigation left: the camera, whether the hand and the wheel let it be, and a selection box let go (where it
/// began and ended).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Nav {
    pub(crate) cam: Camera,
    pub(crate) settled: bool,
    pub(crate) selected_box: Option<(Pos2, Pos2)>,
}

/// The mesh a picture is drawn from on the card, once the card holds it.
pub(crate) enum OnCard {
    /// The mesh (none: the picture is the processor's), and why the card was given up this frame, if it was.
    Ready(Option<Arc<Mesh>>, Option<Status>),
    /// Still going up to the card: the view waits.
    Loading,
}

/// What a picture shows: the model and its scene, the camera, the tints, the work plane's marks, and the section box it is cut
/// to: `dragged` while one of its grips is (a picture tells by that too), else `cut`, the model's.
pub(crate) struct Shown<'a> {
    pub(crate) model: &'a Arc<Model>,
    pub(crate) scene: &'a Scene,
    pub(crate) cam: Camera,
    pub(crate) tints: &'a [(Id, Rgb)],
    pub(crate) marks: &'a [PlaneMark],
    pub(crate) dragged: Option<(P3, P3)>,
    pub(crate) cut: Option<(P3, P3)>,
}

/// The card's failure to draw the 3D view, as the status bar says it.
fn card_failed(e: &str) -> Status {
    crate::updates::failed(crate::lang::tf("The graphics card couldn't draw the 3D view ({}): it is drawn on the processor", &[e]))
}

/// The 3D view between frames.
pub struct Orbit {
    /// `None` until the view first shows, which fits it to the model.
    pub(crate) camera: Option<Camera>,
    /// Where the view showed last, in points, and the pixels a point holds.
    pub(crate) rect: Rect,
    pub(crate) ppp: f32,
    /// The scene of the model shown last, in the 3D view or a section's.
    pub(crate) scene: Option<(Arc<Model>, Arc<Scene>)>,
    /// The scene of a model before, kept while the scene of the model now is made from it.
    last: Option<(Arc<Model>, Arc<Scene>)>,
    /// What picks in the picture on screen, and what the picture is of.
    picker: Option<Picker>,
    shown: Option<Shot>,
    /// The processor's picture as it is shown before the work plane is laid over it, and the marks the picture on screen has:
    /// a change of marks alone lays them again, without drawing the picture anew.
    unmarked: Option<Vec<u8>>,
    marked: Option<Vec<PlaneMark>>,
    /// The processor's picture as a texture; and whether the card's is the one on screen.
    texture: Option<TextureHandle>,
    pub(crate) on_card: bool,
    /// The mesh of the scene shown last, for the card: or why it can't draw it.
    pub(crate) mesh: Option<Meshed>,
    worker: Option<crate::view_worker::Latest<SceneJob, SceneDone>>,
    pub(crate) requested: Option<Arc<Model>>,
    requested_card: bool,
    /// Whether a job asked of the worker for the model shown is still to come back with a camera fitted to it.
    fitting: bool,
    pub(crate) loading_error: Option<String>,
    /// How big the last picture was drawn, points, which a wait shows it at.
    picture_size: Option<egui::Vec2>,
    fit_worker: Option<crate::view_worker::Latest<FitJob, FitDone>>,
    fit_target: Option<Camera>,
    fit_requested: Option<Camera>,
    fit_from: Option<Camera>,
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
    /// The section box's grip under the pointer, in Modify: which face.
    pub(crate) hot_grip: Option<usize>,
    /// What the view is showing while its scene or mesh is made, and since when it has been waiting.
    pub(crate) waiting: Option<Waiting>,
    pub(crate) waiting_since: Option<std::time::Instant>,
    /// Holds a scene being made back, for a test to see the view wait.
    pub(crate) hold_build: bool,
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
    /// Where the pointer was over the navigation triad in the last frame (`triad.rs`).
    pub(crate) triad: crate::triad::Over,
    /// What the Aligned Dimension tool has picked in the 3D view so far: its references, before a click places its line.
    pub(crate) dim3: Vec<Ref3>,
    /// The picture's size the camera was fitted to, while nothing has moved it since: a view that fits itself again as it is
    /// resized (a family's {3D}) looks at it.
    pub(crate) fitted: Option<(u32, u32)>,
    /// Which view this is to the card, which keeps one mesh of another view's beside its own (`Gpu::hold_for`).
    slot: u64,
    /// What its processor picture's texture is called.
    name: &'static str,
    /// The handle of the selected wall or column being dragged: the element, which, its place and where the drag began.
    pub(crate) handle: Option<(Id, Handle, P3, Pos2)>,
    /// The handle under the pointer.
    pub(crate) hot_handle: Option<Handle>,
    /// The Measure tool in the 3D view: its first point, once clicked; then the ruler laid.
    pub(crate) ruler_start: Option<P3>,
    pub(crate) ruler: Option<(P3, P3)>,
}

/// The next view's slot on the card.
static NEXT_SLOT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

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
            unmarked: None,
            marked: None,
            texture: None,
            on_card: false,
            mesh: None,
            worker: None,
            requested: None,
            requested_card: false,
            fitting: false,
            loading_error: None,
            picture_size: None,
            fit_worker: None,
            fit_target: None,
            fit_requested: None,
            fit_from: None,
            drag: None,
            select: None,
            before: None,
            grip: None,
            hot_grip: None,
            waiting: None,
            waiting_since: None,
            hold_build: false,
            boxing: None,
            lift: None,
            built: None,
            rooms: None,
            plane: None,
            wall_base: None,
            triad: Default::default(),
            dim3: Vec::new(),
            fitted: None,
            slot: NEXT_SLOT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            name: "view3d",
            handle: None,
            hot_handle: None,
            ruler_start: None,
            ruler: None,
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

/// Whether Attach Top/Base takes element `id`: a roof or a floor.
fn attach_target(m: &Model, id: Id) -> bool {
    matches!(m.elements.get(id), Some(bimcraft_core::model::Element::Roof(_) | bimcraft_core::model::Element::Floor(_)))
}

/// A handle of the wall or column selected, as a section box's are drawn: an arrow up or down the screen for its top or base, a
/// square for its ends or place. In the accent while held, in the hover colour under the pointer.
fn draw_handle(p: &egui::Painter, cam: &Camera, h: Handle, q: P3, held: Option<Handle>, hot: Option<Handle>, screen: impl Fn(P3) -> Pos2) {
    let at = screen(q);
    if !at.is_finite() {
        return;
    }
    let (k, held, hot) = (0, (held == Some(h)).then_some(0), (hot == Some(h)).then_some(0));
    let (fill, outline) = crate::section_box::grip_colours(k, held, hot);
    let stroke = Stroke::new(1.5, outline);
    if h.vertical() {
        let mut out = q;
        out[2] += if h == Handle::Top { 1.0 } else { -1.0 } / cam.scale.max(1e-12);
        let d = screen(out) - at;
        if d.length() >= 0.25 {
            crate::section_box::arrow(p, at, d.normalized(), fill, stroke);
            return;
        }
    }
    let r = Rect::from_center_size(at, vec2(9.0, 9.0));
    p.rect_filled(r, 1.0, fill);
    p.rect_stroke(r, 1.0, stroke, StrokeKind::Middle);
}

/// Where the Measure tool lands at pixel `at`: what a dimension would pick there, or the middle of the edge when the cursor is
/// that near it. Any point of the model, whatever the work plane.
fn measure_point(scene: &Scene, cam: &Camera, at: (f64, f64), under: Option<Surface>, ppp: f32, look: &Look) -> Option<Ref3> {
    let mut r = dim_ref(scene, cam, at, under, ppp, look)?;
    if let Some([a, b]) = r.edge {
        let mid = [0, 1, 2].map(|k| f64::midpoint(a[k], b[k]));
        let (x, y, _) = cam.project(mid);
        if (x - at.0).hypot(y - at.1) <= SNAP * f64::from(ppp) {
            r.at = mid;
        }
    }
    Some(r)
}

/// The ruler from `a` to `b`, a dot at each end: its length beside `b`, and under it how far it goes along each axis.
fn draw_ruler(p: &egui::Painter, m: &bimcraft_core::model::Model, a: P3, b: P3, screen: &dyn Fn(P3) -> Pos2) {
    let (from, to) = (screen(a), screen(b));
    if !(from.is_finite() && to.is_finite()) {
        return;
    }
    p.line_segment([from, to], Stroke::new(2.0, pal().accent));
    for end in [from, to] {
        p.circle_filled(end, 3.5, pal().accent);
    }
    let (long, along) = dims3d::reading(a, b);
    let [x, y, z] = along.map(|d| m.units.length(d));
    let text = format!("{}\nΔX {x}  ΔY {y}  ΔZ {z}", m.units.with_unit(long));
    crate::canvas::tag(p, to + vec2(12.0, -12.0), egui::Align2::LEFT_BOTTOM, text);
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

/// The ViewCube's faces, as Revit's: the direction each looks from (its outward normal), its name, and the camera's azimuth and
/// elevation that look at the model from that side.
const CUBE_FACES: [(P3, &str, f64, f64); 6] = [
    ([0., 0., 1.], msg!("TOP"), -90., 89.),
    ([0., 0., -1.], msg!("BOTTOM"), -90., -89.),
    ([0., -1., 0.], msg!("FRONT"), -90., 0.),
    ([0., 1., 0.], msg!("BACK"), 90., 0.),
    ([1., 0., 0.], msg!("RIGHT"), 0., 0.),
    ([-1., 0., 0.], msg!("LEFT"), 180., 0.),
];

/// What a click on the ViewCube asks for: Home (the house over its corner), or to look from a face's side, the camera's azimuth
/// and elevation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Cube {
    Home,
    Look(f64, f64),
}

/// Whether `p` is inside the convex polygon `poly`.
fn inside(poly: &[Pos2], p: Pos2) -> bool {
    let side = |a: Pos2, b: Pos2| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    let sides: Vec<f32> = (0..poly.len()).map(|i| side(poly[i], poly[(i + 1) % poly.len()])).collect();
    sides.iter().all(|s| *s >= 0.) || sides.iter().all(|s| *s <= 0.)
}

/// The ViewCube, which turns a view's camera: where it stands, its faces, what a click on it asks for and how it is drawn. It
/// needs only a camera and the view's rect, so any 3D view may show one.
impl Orbit {
    /// Where the ViewCube stands in view `rect`: its top right corner, 84 px across.
    pub(crate) fn cube_rect(rect: Rect) -> Rect {
        Rect::from_min_size(rect.right_top() + vec2(-98.0, 14.0), vec2(84.0, 84.0))
    }

    /// The house over the ViewCube's corner: Home, Revit's default 3D view.
    pub(crate) fn home_rect(rect: Rect) -> Rect {
        Rect::from_min_size(Self::cube_rect(rect).left_top() + vec2(-4.0, -6.0), vec2(18.0, 18.0))
    }

    /// Whether `at` is on the ViewCube or its house, in view `rect`.
    pub(crate) fn on_cube(rect: Rect, at: Pos2) -> bool {
        Self::cube_rect(rect).contains(at) || Self::home_rect(rect).contains(at)
    }

    /// The ViewCube's faces that face the camera, each on screen with its name and the view it gives.
    pub(crate) fn cube_faces(cam: &Camera, rect: Rect) -> Vec<(Vec<Pos2>, &'static str, f64, f64)> {
        let (a, e) = (cam.azimuth.to_radians(), cam.elevation.clamp(-89.0, 89.0).to_radians());
        let back = [e.cos() * a.cos(), e.cos() * a.sin(), e.sin()];
        let right = [-a.sin(), a.cos(), 0.0];
        let up =
            [back[1] * right[2] - back[2] * right[1], back[2] * right[0] - back[0] * right[2], back[0] * right[1] - back[1] * right[0]];
        let box_ = Self::cube_rect(rect);
        let (c, half) = (box_.center(), box_.width() as f64 * 0.27);
        let dot = |p: P3, q: P3| p[0] * q[0] + p[1] * q[1] + p[2] * q[2];
        let on = |p: P3| c + vec2((dot(p, right) * half) as f32, (-dot(p, up) * half) as f32);
        CUBE_FACES
            .iter()
            .filter(|(n, ..)| dot(*n, back) > 0.02)
            .map(|(n, label, az, el)| {
                // The face's two other axes, round it in turn.
                let k = n.iter().position(|v| v.abs() > 0.5).unwrap_or(2);
                let (u, v) = ((k + 1) % 3, (k + 2) % 3);
                let corner = |su: f64, sv: f64| {
                    let mut p = *n;
                    p[u] = su;
                    p[v] = sv;
                    on(p)
                };
                (vec![corner(-1., -1.), corner(1., -1.), corner(1., 1.), corner(-1., 1.)], *label, *az, *el)
            })
            .collect()
    }

    /// What a click at `at` on the ViewCube in view `rect`, seen through `cam`, asks for: Home on the house, else the side of the
    /// face there; nothing off them.
    pub(crate) fn cube_click(cam: &Camera, rect: Rect, at: Pos2) -> Option<Cube> {
        if Self::home_rect(rect).contains(at) {
            return Some(Cube::Home);
        }
        Self::cube_faces(cam, rect).into_iter().find(|(poly, ..)| inside(poly, at)).map(|(_, _, az, el)| Cube::Look(az, el))
    }

    /// The ViewCube drawn on `p` in view `rect` as `cam` turns it, as Revit's: the faces turned to the eye, named, the one under
    /// `hover` lit; the house over its corner.
    pub(crate) fn draw_cube(p: &egui::Painter, cam: &Camera, rect: Rect, hover: Option<Pos2>) {
        let cp = canvas_pal();
        let face_fill = if crate::style::canvas_dark() { Color32::from_rgb(92, 97, 106) } else { Color32::from_rgb(236, 237, 239) };
        let edge = if crate::style::canvas_dark() { Color32::from_rgb(150, 155, 165) } else { Color32::from_rgb(120, 124, 130) };
        for (poly, label, ..) in Self::cube_faces(cam, rect) {
            let lit = hover.is_some_and(|h| inside(&poly, h));
            let centre = poly.iter().fold(Pos2::ZERO, |a, q| a + q.to_vec2() / poly.len() as f32);
            p.add(egui::Shape::convex_polygon(poly, if lit { cp.hover.gamma_multiply(0.6) } else { face_fill }, Stroke::new(1.0, edge)));
            p.text(centre, egui::Align2::CENTER_CENTER, t(label), egui::FontId::proportional(9.5), cp.text.gamma_multiply(0.85));
        }
        let home = Self::home_rect(rect);
        let home_lit = hover.is_some_and(|h| home.contains(h));
        crate::icons::paint(p, crate::icons::Icon::Home, home, if home_lit { cp.hover } else { edge });
    }
}

impl Orbit {
    /// A view whose processor picture's texture is called `name`.
    pub(crate) fn named(name: &'static str) -> Orbit {
        Orbit { name, ..Orbit::default() }
    }

    /// Zoom to Fit: all the view shows of its model, from where the camera is, fitted on the worker.
    pub(crate) fn fit(&mut self) {
        if let Some(c) = self.camera {
            self.fit_target = Some(c);
            self.fit_requested = None;
            self.fit_from = None;
        }
    }

    /// Home: the camera turned to Revit's default orientation and fitted over the model on the worker, through `lens`.
    pub(crate) fn home(&mut self, lens: Option<f64>) {
        if let Some(c) = self.camera {
            self.fit_target = Some(Camera { azimuth: AZIMUTH, elevation: ELEVATION, lens, ..c });
            self.fit_requested = None;
            self.fit_from = None;
        }
    }

    /// What the picture on screen shows at screen point `at` (`input` the frame's), through `cam`.
    pub(crate) fn hit(&self, input: &Input, cam: &Camera, at: Pos2) -> Option<Hit> {
        let (x, y) = input.px(at);
        self.picker.as_ref()?.at(cam, x, y)
    }

    /// Whether the hand is turning or panning the view.
    pub(crate) fn turning(&self) -> bool {
        self.drag.is_some()
    }

    /// Whether the picture on screen is of `model`.
    pub(crate) fn shows(&self, model: &Arc<Model>) -> bool {
        self.shown.as_ref().is_some_and(|s| Arc::ptr_eq(&s.0, model))
    }

    /// The view in `rect` while its scene or mesh is made, on `p`: the last picture, still at first so that a short wait doesn't
    /// flicker, then faded with `words` over it. With none yet, or with something wrong (`now`), the words at once.
    pub(crate) fn wait(&mut self, ctx: &egui::Context, p: &egui::Painter, card: &crate::gpu::Card, rect: Rect, words: &str, now: bool) {
        p.rect_filled(rect, 0.0, color(render::PAPER));
        let since = *self.waiting_since.get_or_insert_with(std::time::Instant::now);
        let picture = match self.picture_size {
            Some(size) if self.on_card => card.gpu.as_ref().and_then(|g| g.texture()).map(|id| (id, size)),
            Some(size) => self.texture.as_ref().map(|t| (t.id(), size)),
            None => None,
        };
        if let Some((id, size)) = picture {
            p.image(id, Rect::from_min_size(rect.min, size), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        let shown = Waiting::after(since.elapsed(), picture.is_some(), now);
        self.waiting = Some(shown);
        if shown == Waiting::Quiet {
            ctx.request_repaint_after(WAIT_QUIET);
        } else {
            if shown == Waiting::Faded {
                let c = color(render::PAPER);
                p.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), WAIT_FADE));
            }
            let galley = p.layout_no_wrap(words.to_string(), egui::FontId::proportional(14.0), pal().text);
            let at = rect.center() - galley.size() / 2.0;
            let card = Rect::from_min_size(at, galley.size()).expand2(vec2(12.0, 7.0));
            p.rect(card, 6.0, pal().surface, Stroke::new(1.0, pal().border), StrokeKind::Inside);
            p.galley(at, galley, pal().text);
        }
    }

    pub(crate) fn suspend(&mut self) {
        self.worker = None;
        self.requested = None;
        self.fit_worker = None;
        self.fit_target = None;
        self.fit_requested = None;
        self.fit_from = None;
        self.fitting = false;
    }

    /// A model that differs from the last only in its section box, or in what the scene doesn't draw (a camera view's camera
    /// written back, a 3D dimension), has its scene and its mesh: a box is asked of them as they are drawn and picked, so there
    /// is nothing to make again, or to wait for.
    fn keep_for_a_new_box(&mut self, model: &Arc<Model>) {
        let Some((before, scene)) = &self.scene else { return };
        if Arc::ptr_eq(before, model) || !bimcraft_core::view3d::scene_kept(before, model) {
            return;
        }
        let scene = Arc::clone(scene);
        self.scene = Some((Arc::clone(model), scene));
        if let Some((held, _)) = &mut self.mesh {
            *held = Arc::clone(model);
        }
        self.requested = Some(Arc::clone(model));
    }

    /// Install a whole snapshot at once, so the picture and its picker cannot refer to different models.
    /// The scene of `model` (and its mesh, for a `card`) made or brought up to date on the worker, and a camera fitted to it
    /// through `lens` while the view has none (a view the user sent home, or opened anew): whether both are ready to draw.
    pub(crate) fn prepare(
        &mut self,
        ctx: &egui::Context,
        model: &Arc<Model>,
        (width, height): (u32, u32),
        lens: Option<f64>,
        card: bool,
    ) -> bool {
        if let Some(done) = self.worker.as_ref().and_then(crate::view_worker::Latest::take)
            && Arc::ptr_eq(&done.model, model)
        {
            self.install(done);
        }
        let current = self.scene.as_ref().is_some_and(|(m, _)| Arc::ptr_eq(m, model));
        let needs_mesh = card && !self.mesh.as_ref().is_some_and(|(m, _)| Arc::ptr_eq(m, model));
        if !current || needs_mesh || self.camera.is_none() {
            if !self.requested.as_ref().is_some_and(|m| Arc::ptr_eq(m, model))
                || (needs_mesh && !self.requested_card)
                || (self.camera.is_none() && !self.fitting)
            {
                let job = SceneJob {
                    model: Arc::clone(model),
                    scene: self.scene.as_ref().filter(|(m, _)| Arc::ptr_eq(m, model)).map(|(_, s)| Arc::clone(s)),
                    from: if current { None } else { self.scene.clone().or_else(|| self.last.clone()) },
                    width,
                    height,
                    lens,
                    card: needs_mesh,
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
                self.fitting = true;
                if self.hold_build {
                } else if cfg!(test) {
                    if let Some(done) = build_scene(job, &crate::view_worker::Cancel::default()) {
                        self.install(done);
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

    /// What the worker made for a model, in place: its scene, its mesh if it made one, and its fitted camera if the view has none.
    fn install(&mut self, done: SceneDone) {
        if self.camera.is_none() {
            self.camera = Some(done.fitted);
            self.fitted = Some((done.fitted.width, done.fitted.height));
        }
        if let Some(mesh) = done.mesh {
            self.mesh = Some((Arc::clone(&done.model), mesh));
        }
        self.scene = Some((done.model, done.scene));
        self.last = None;
        self.fitting = false;
    }

    pub(crate) fn prepare_fit(&mut self, ctx: &egui::Context, model: &Arc<Model>) -> Option<String> {
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
    /// The frame's input over the view in `rect` (its navigation bar's corner at `nav`), `response` its widget's: the triad's widget
    /// laid over the view's first, so a click or a drag on it is its own and the model behind never sees it.
    /// `device` is the 3D mouse's cap, which counts only while the window has focus.
    pub(crate) fn input(
        &mut self,
        ui: &Ui,
        rect: Rect,
        response: &egui::Response,
        nav: Pos2,
        device: Option<crate::spacemouse::Motion>,
    ) -> Input {
        let asked = self.triad_input(ui, rect, nav);
        let input = Input::read(ui, rect, response, ui.ctx().pixels_per_point(), asked, device);
        (self.rect, self.ppp) = (rect, input.ppp);
        input
    }

    /// `cam` zoomed by the wheel and a pinch, a notch (about 50 points) a fifth, toward what the picture on screen shows under the
    /// cursor (found in `scene`, the one it was drawn from; none while it is of another model), else toward the cursor.
    /// Then the 3D mouse's cap moves it, and asks for the next frame.
    pub(crate) fn zoom(&self, ctx: &egui::Context, input: &Input, mut cam: Camera, scene: Option<&Scene>) -> Camera {
        if let Some(at) = input.hover {
            let factor = (f64::from(input.wheel) * 0.004).exp() * f64::from(input.pinch);
            if factor.is_finite() && factor != 1.0 {
                let factor = factor.clamp(0.2, 5.0);
                cam = match self.picker.as_ref().and_then(|p| p.surface(scene, &cam, input.px(at))) {
                    Some(s) => cam.zoom_about(factor, s.at),
                    None => cam.zoom(factor, input.px(at)),
                };
            }
        }
        if let Some(motion) = input.motion {
            cam = crate::spacemouse::navigate(cam, motion, input.frame);
            ctx.request_repaint();
        }
        cam
    }

    /// The hand's drag, from `cam` (zoomed already): Shift and the middle button orbit, 0.4° a point, round the point under the
    /// pointer where the drag began (found in `scene` as [`zoom`](Self::zoom) finds it; the target, where nothing is); the
    /// middle button alone pans. The primary button never moves the camera: with `boxes`, it drags a selection box.
    /// A drag that a view's tool `held` this frame (a grip's) is the tool's. A drag that turns out a `quick` click puts the camera
    /// back. Then a click on the triad's end turns the camera to look from that side. The camera is the view's from here on.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn navigate(
        &mut self,
        ui: &Ui,
        response: &egui::Response,
        input: &Input,
        mut cam: Camera,
        held: bool,
        boxes: bool,
        quick: bool,
        scene: Option<&Scene>,
    ) -> Nav {
        if response.drag_started()
            && !held
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            if response.dragged_by(PointerButton::Primary) {
                if boxes {
                    self.select = Some(origin);
                }
            } else if response.dragged_by(PointerButton::Middle) {
                let orbit = input.shift;
                let pivot = orbit.then(|| self.picker.as_ref()?.surface(scene, &cam, input.px(origin))).flatten().map(|s| s.at);
                self.drag = Some((orbit, origin, pivot));
                self.before = Some(cam);
            }
        }
        let mut selected_box = None;
        if response.drag_stopped()
            && let Some(from) = self.select.take()
            && !quick
            && let Some(to) = input.latest
        {
            selected_box = Some((from, to));
        }
        if let (Some((orbit, last, pivot)), Some(now)) = (self.drag, input.latest)
            && response.dragged()
        {
            let d = now - last;
            let (around, up) = (-f64::from(d.x) * 0.4, f64::from(d.y) * 0.4);
            cam = if orbit { cam.orbit_about(around, up, pivot.unwrap_or(cam.target)) } else { cam.pan(input.px(last), input.px(now)) };
            self.drag = Some((orbit, now, pivot));
        }
        if response.drag_stopped() {
            self.drag = None;
            // A click made on the move: the camera the hand turned meanwhile goes back.
            if let (true, Some(before)) = (quick, self.before.take()) {
                cam = before;
            }
        }
        // A click on one of the triad's ends turns the camera to look from that side (or from the opposite one, if it does already).
        if let Some(Ask::Turn(end)) = input.asked {
            cam = bimcraft_core::triad::turned(cam, end);
        }
        self.camera = Some(cam);
        // The hand, the wheel and the 3D mouse have let the camera be.
        let settled = self.drag.is_none() && input.wheel == 0.0 && input.pinch == 1.0 && input.motion.is_none();
        Nav { cam, settled, selected_box }
    }

    /// The mesh the card draws the picture from, where the card is `ready` and the mesh is made: put on the card off the
    /// interface's thread first (meanwhile [`OnCard::Loading`]). One the card can't take gives the card up. The card's note says
    /// why a card wanted draws nothing, as the mesh says.
    pub(crate) fn card_mesh(&mut self, ctx: &egui::Context, card: &mut crate::gpu::Card, ready: bool) -> OnCard {
        // Another view's mesh on the card is set aside, and this one's brought back: the picture on the card is not this view's.
        if let Some(gpu) = card.gpu.as_mut()
            && gpu.hold_for(self.slot)
        {
            self.shown = None;
        }
        let mut mesh = match ready.then(|| self.mesh.as_ref().map(|(_, m)| m)).flatten() {
            Some(Ok(mesh)) => Some(Arc::clone(mesh)),
            _ => None,
        };
        let mut failed = None;
        if let (Some(mesh_to_upload), Some(gpu)) = (mesh.as_ref(), card.gpu.as_mut())
            && !cfg!(test)
        {
            match gpu.prepare_upload(ctx, mesh_to_upload) {
                Ok(true) => {}
                Ok(false) => return OnCard::Loading,
                Err(e) => {
                    failed = Some(card_failed(&e));
                    (card.gpu, card.note) = (None, Some(e));
                    mesh = None;
                }
            }
        }
        // Why not, where the mesh says; a card that failed keeps what it said.
        if !card.failed {
            let why = (card.wanted && mesh.is_none()).then(|| self.mesh.as_ref().and_then(|(_, m)| m.as_ref().err().cloned())).flatten();
            if card.note != why {
                card.note = why;
            }
        }
        OnCard::Ready(mesh, failed)
    }

    /// The picture of `shot` on `p`, over the view's ground: drawn on the card from `mesh` where there is one, else on the
    /// processor, and with it what picks in it ([`Picker`]). It is drawn again only where what it shows has changed; the work
    /// plane's marks alone are laid again over it as it was. A card that fails is given up and the processor draws: what to say.
    pub(crate) fn paint(
        &mut self,
        ctx: &egui::Context,
        p: &egui::Painter,
        card: &mut crate::gpu::Card,
        mesh: Option<&Arc<Mesh>>,
        shot: Shown<'_>,
    ) -> Option<Status> {
        let Shown { model, scene, cam, tints, marks: plane_marks, dragged: reboxed, cut: boxed } = shot;
        let mut failed = None;
        // The ground, which a canvas colour of the user's changes without making the drawing dark or light.
        let dark = color(render::PAPER);
        if !matches!(&self.shown, Some((m, c, t, k, g, b)) if Arc::ptr_eq(m, model) && *c == cam && t.as_slice() == tints && *k == dark && *g == mesh.is_some() && *b == reboxed)
        {
            // The card's picture, if it makes one; the processor's if not, and if the card fails it for good.
            let (mut drawn, mut exact) = (false, true);
            if let (Some(mesh), Some(gpu)) = (mesh, card.gpu.as_mut()) {
                gpu.swapped_ink = crate::style::canvas_dark();
                plane_marks.clone_into(&mut gpu.marks);
                // The model's box, whichever the mesh was made under.
                let cut = reboxed.or(boxed);
                match gpu.draw_cut(mesh, &cam, tints, rgb(color(render::PAPER)), cut) {
                    Ok(whole) => {
                        self.picker = Some(Picker::Mesh(Arc::clone(mesh), tints.to_vec(), cut));
                        self.marked = Some(plane_marks.to_vec());
                        (drawn, exact) = (true, whole);
                    }
                    Err(e) => {
                        failed = Some(card_failed(&e));
                        (card.gpu, card.note) = (None, Some(e));
                    }
                }
            }
            self.on_card = drawn;
            if !drawn {
                // As the default 3D view says: what it hides, and the colours it draws in.
                let look = Look::of_3d(model);
                let look = if reboxed.is_some() { look.with_section_box(reboxed) } else { look };
                let image = scene.render_in(&cam, tints, &look);
                // The model shaded as ever, on the theme's ground: only the background is paper white.
                let ground = color(render::PAPER);
                let (rgba, _) = image.rgba.as_chunks::<4>();
                // Hidden line and wireframe are drawn as a section is, dark in the dark theme: paper and ink swap.
                let lines = look.style() != VisualStyle::Shaded;
                let shown: Vec<u8> = rgba
                    .iter()
                    .flat_map(|&[r, g, b, _]| {
                        match [r, g, b] {
                            _ if lines => color(Rgb(r, g, b)),
                            [255, 255, 255] => ground,
                            _ => Color32::from_rgb(r, g, b),
                        }
                        .to_array()
                    })
                    .collect();
                // The work plane is laid over it below.
                (self.unmarked, self.marked) = (Some(shown), None);
                self.picker = Some(Picker::Image(image));
            }
            // A picture that stands in, or whose exact surfaces are on their way, is drawn again next frame until it is exact.
            self.shown = exact.then(|| (Arc::clone(model), cam, tints.to_vec(), dark, drawn, reboxed));
            if !exact {
                ctx.request_repaint();
            }
        }
        // The work plane laid over the picture again where only it has changed (the patch lit, Show turned on): over the
        // processor's picture as it was before, or by the card, which draws the surfaces it has again only if it must.
        if self.marked.as_deref() != Some(plane_marks) {
            if self.on_card {
                if let (Some(mesh), Some(gpu)) = (mesh, card.gpu.as_mut()) {
                    plane_marks.clone_into(&mut gpu.marks);
                    if let Err(e) = gpu.draw_cut(mesh, &cam, tints, rgb(color(render::PAPER)), reboxed.or(boxed)) {
                        failed = Some(card_failed(&e));
                        (card.gpu, card.note) = (None, Some(e));
                        self.shown = None;
                        ctx.request_repaint();
                    }
                }
            } else if let (Some(Picker::Image(image)), Some(base)) = (&self.picker, &self.unmarked) {
                let mut rgba = base.clone();
                image.mark(&mut rgba, &cam, plane_marks);
                let pixels = rgba.as_chunks::<4>().0.iter().map(|&[r, g, b, _]| Color32::from_rgb(r, g, b)).collect();
                let pixels = ColorImage { size: [image.width as usize, image.height as usize], pixels, source_size: Default::default() };
                match &mut self.texture {
                    Some(t) => t.set(pixels, TextureOptions::NEAREST),
                    None => self.texture = Some(ctx.load_texture(self.name, pixels, TextureOptions::NEAREST)),
                }
            }
            self.marked = Some(plane_marks.to_vec());
        }
        (self.waiting, self.waiting_since) = (None, None);
        let rect = self.rect;
        self.picture_size = Some(rect.size());
        p.rect_filled(rect, 0.0, color(render::PAPER));
        let shown = if self.on_card { card.gpu.as_ref().and_then(|g| g.texture()) } else { self.texture.as_ref().map(|t| t.id()) };
        if let Some(id) = shown {
            p.image(id, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        failed
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

    /// Where point `p` of the family open shows on screen, in its {3D} as last shown (its axes are the scratch model's).
    pub fn family_screen3(&self, p: P3) -> Option<Pos2> {
        let o = &self.active_family()?.orbit;
        o.camera.map(|c| on_screen(&c, o.rect, o.ppp, p))
    }

    /// Zoom to Fit: all the view shows of the model, from where the camera is.
    pub(crate) fn fit3d(&mut self) {
        self.orbit.fit();
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
                    self.place_component(p, plane.z);
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
        // What the hand, the wheel, the 3D mouse and the navigation triad did; what the triad asks for is done below. The 3D
        // mouse's cap, while it is held off rest (`spacemouse.rs`), moves the camera when this is the active view and the window
        // has focus; a 3D view drawn beside the active one it leaves alone, as the hand does.
        let nav_corner = self.nav_corner(rect);
        let device = self.space_mouse.as_ref().and_then(crate::spacemouse::SpaceMouse::motion).filter(|_| !self.passive);
        let input = self.orbit.input(ui, rect, &response, nav_corner, device);
        let Input { ppp, size: (w, h), hover, wheel, pinch, shift, latest, asked, .. } = input;
        let px = |p: Pos2| input.px(p);
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
        let attaching = self.tool == Tool::Attach;
        // Trim/Extend in the 3D view takes walls as a plan does, at the point of the face clicked.
        let trimming = matches!(self.tool, Tool::Trim(_) | Tool::TrimTo { .. }) && self.sketch.is_none();
        let trim_first = match self.tool {
            Tool::Trim(Some((first, _))) | Tool::TrimTo { boundary: Some(first), .. } => Some(first),
            _ => None,
        };
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
        // Place on Face: the Component tool placing a work plane-based family on the face clicked.
        let placing_on_face = matches!(self.tool, Tool::Component) && self.place_on_face && self.placing_on_work_plane();
        let o = &mut self.orbit;
        (o.rect, o.ppp) = (rect, ppp);
        // The camera changed elsewhere than by the hand in this view (dragged on a plan, a command, undo): the picture stands
        // from the model afresh, so the old one it showed isn't written back over the change.
        let elsewhere = match (o.built, active, stored) {
            (Some((id, cam, l)), Some(active), Some(stored)) => id != active || cam != stored || l != lens,
            _ => false,
        };
        if elsewhere && o.camera.is_some() && o.drag.is_none() && wheel == 0.0 && pinch == 1.0 && input.motion.is_none() {
            o.camera = None;
        }
        // The view stands from the model anew: a camera view where its camera looks from, which needs no fitting; another is
        // fitted over the model on the worker, with its scene.
        if o.camera.is_none() {
            o.built = active.zip(stored).map(|(id, cam)| (id, cam, lens));
            if let (Some(cam), Some(lens)) = (stored, lens) {
                o.camera = Some(Camera::at(cam.eye, cam.target, lens, w, h));
            }
        }
        o.keep_for_a_new_box(&model);
        if !o.scene.as_ref().is_some_and(|(m, _)| Arc::ptr_eq(m, &model))
            && let Some(gpu) = &mut self.card.gpu
        {
            gpu.forget_mesh();
        }
        let card_ready = self.card.ready().is_some();
        if !o.prepare(ui.ctx(), &model, (w, h), lens, card_ready) {
            let error = o.loading_error.clone();
            self.wait3d(ui, rect, error.as_deref().unwrap_or(crate::lang::t("Building 3D view…")), error.is_some());
            return;
        }
        if let Some(error) = o.prepare_fit(ui.ctx(), &model) {
            self.status = Status { text: crate::lang::tf("Could not fit the 3D view: {}", &[&error]), error: true };
        }
        // A prepared view has both; without them there's nothing to draw this frame.
        let (Some((_, scene)), Some(camera)) = (o.scene.as_ref(), o.camera) else { return };
        let scene = Arc::clone(scene);
        let scene: &Scene = &scene;
        // Through the view's lens while it's in perspective: the eye stays where it stood as the lens or the picture changes.
        let mut cam = camera.sized(w, h, lens);
        // What the picture on screen shows at a point, as the view picks: its scene is this model's while it's of this model.
        let fresh = o.shown.as_ref().is_some_and(|s| Arc::ptr_eq(&s.0, &model));
        let surface = |picker: &Option<Picker>, cam: &Camera, at: Pos2| picker.as_ref()?.surface(fresh.then_some(scene), cam, px(at));
        // The wheel and a pinch zoom toward what's under the cursor; the 3D mouse moves the camera.
        cam = o.zoom(ui.ctx(), &input, cam, fresh.then_some(scene));
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
        // The grip under the pointer shows it: a press there moves that face, not the view.
        o.hot_grip = boxed
            .filter(|_| modify && o.grip.is_none() && o.boxing.is_none() && o.drag.is_none())
            .and_then(|b| crate::section_box::grip_at(b, hover?, |p| on_screen(&cam, rect, ppp, p)));
        // The wall or column selected alone has handles: its ends or its place, and arrows on its top and under its base. A drag
        // from one changes that, and the command runs when the drag ends (`handles.rs`).
        let single = match selection.as_slice() {
            &[id] if modify && !lifting && sketch_z.is_none() => Some(id),
            _ => None,
        };
        let own_handles = single.map(|id| handles::of(&model, id)).unwrap_or_default();
        let handle_at = |cam: &Camera, at: Pos2| {
            own_handles.iter().find(|(_, q)| on_screen(cam, rect, ppp, *q).distance(at) <= crate::section_box::GRIP + 2.0).copied()
        };
        if let Some(id) = single
            && o.grip.is_none()
            && o.boxing.is_none()
            && !held
            && response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && let Some((h, q)) = handle_at(&cam, origin)
        {
            o.handle = Some((id, h, q, origin));
        }
        if !o.handle.is_some_and(|(id, ..)| Some(id) == single) {
            o.handle = None;
        }
        // Where the held handle is now: an arrow at its height, up or down its plumb line and on a level within a few pixels; the
        // rest over its level's plane under the pointer, snapped as a plan snaps.
        let held_to = match (o.handle, latest) {
            (Some((_, h, q, from)), Some(now)) if h.vertical() => cam.along(2, q, px(from), px(now)).map(|by| {
                let reach = f64::from(crate::section_box::GRIP) / cam.scale.max(1e-9);
                (P2(q[0], q[1]), handles::settled(&model, (q[2] + by).round(), reach))
            }),
            (Some((id, _, q, _)), Some(now)) => cam.on_plane(px(now), q[2]).map(|at| {
                let level = model.elements.get(id).and_then(|e| model.level_of(e)).and_then(|g| model.elements.id(g));
                let landed = level.map(|level| {
                    let plane = CursorPlane { on: id, level, offset: 0.0, z: q[2] };
                    snap3d(&model, &cam, px(now), ppp, at, plane, &[], shift, &snaps).at
                });
                (landed.unwrap_or(P2(at[0], at[1])), q[2])
            }),
            _ => None,
        };
        let mut handled = None;
        if response.drag_stopped()
            && let (Some((id, h, ..)), Some((to, z))) = (o.handle.take(), held_to)
            && !self.canvas.quick()
        {
            handled = handles::command(&model, id, h, to, z);
        }
        o.hot_handle = if o.grip.is_none() && o.boxing.is_none() && o.drag.is_none() && o.handle.is_none() {
            hover.and_then(|at| handle_at(&cam, at)).map(|(h, _)| h)
        } else {
            None
        };
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
        // The hand's drag turns or pans the view (Shift and the middle button, the middle button), unless a grip or the sketch
        // holds it; the primary button boxes, in Modify (the sketch's lines while sketching). A click on the triad's end turns it too.
        let grabbed = o.grip.is_some() || o.boxing.is_some() || o.handle.is_some() || held || o.lift.is_some();
        let Nav { cam, settled, selected_box } =
            o.navigate(ui, &response, &input, cam, grabbed, modify, self.canvas.quick(), fresh.then_some(scene));
        // A box let go: what it picks, as the picture is now.
        let boxed_ids = selected_box
            .filter(|_| sketch_z.is_none())
            .map(|(from, to)| scene.in_box(&cam, px(from), px(to), to.x < from.x, &Look::of_3d(&model)));
        // On the card when the user asked for it and it can draw what the view shows; else on the processor.
        let mesh = match o.card_mesh(ui.ctx(), &mut self.card, card_ready) {
            OnCard::Loading => {
                self.wait3d(ui, rect, crate::lang::t("Loading 3D view onto the card…"), false);
                return;
            }
            OnCard::Ready(mesh, failed) => {
                if let Some(status) = failed {
                    self.status = status;
                }
                mesh
            }
        };
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
            .filter(|_| {
                (modify || demolish || paint || unpaint || attaching || trimming)
                    && o.drag.is_none()
                    && o.hot_grip.is_none()
                    && o.hot_handle.is_none()
                    && sketch_z.is_none()
                    && !picking
            })
            .filter(|_| lit_dim.is_none())
            .and_then(|at| pick(&o.picker, at))
            .map(|h| h.id)
            .filter(|id| !attaching || attach_target(&model, *id))
            .filter(|id| !trimming || model.wall(*id).is_some());
        // The floor or roof being shaped is drawn as it is, not in the selection's colour: edit mode's marks are what's lit.
        let tints: Vec<(Id, Rgb)> = selection
            .iter()
            .chain(trim_first.iter())
            .filter(|id| Some(**id) != shaped)
            .map(|id| (*id, rgb(canvas_pal().accent)))
            .chain(hovered.filter(|id| !selection.contains(id)).map(|id| (id, rgb(canvas_pal().hover))))
            .collect();
        // The work plane and its patch, drawn into the picture so what stands in front of them hides them: the plane while it's
        // being set, shown (Show) or a new outline is sketched on it; the patch in Modify, lit while the pointer is on it where
        // it is seen.
        let new_sketch = self.sketch.as_ref().and_then(|sk| sk.surface.filter(|_| sk.plan == crate::Sheet::View3D));
        let planed = self.work_plane.is_some() || self.show_work_plane || new_sketch.is_some();
        let m = &self.session.model;
        let wp = m.work_plane(m.default_3d()).resolve(m).ok();
        let patch_seen = |picker: &Option<Picker>, at: Pos2| {
            let on = wp.and_then(|f| cam.on(px(at), f.origin, f.normal));
            match (on, surface(picker, &cam, at)) {
                (None, _) => false,
                (Some(q), Some(s)) => {
                    let z = cam.project(q).2;
                    cam.project(s.at).2 <= z + cam.slack(bimcraft_core::view3d::MARK_SLACK, z)
                }
                (Some(_), None) => true,
            }
        };
        let on_patch =
            |picker: &Option<Picker>, at: Pos2| patch.is_some_and(|ring| crate::workplane::on_patch(&ring, at)) && patch_seen(picker, at);
        let patch_lit = hover.is_some_and(|at| on_patch(&o.picker, at));
        let plane_marks = crate::workplane::marks(m, &cam, ppp, planed, new_sketch.map(Frame::from), patch.is_some(), patch_lit);
        // The picture, on the card or the processor, and what picks in it.
        let p = ui.painter_at(rect);
        let shot = Shown { model: &model, scene, cam, tints: &tints, marks: &plane_marks, dragged: reboxed, cut: boxed };
        if let Some(status) = o.paint(ui.ctx(), &p, &mut self.card, mesh.as_ref(), shot) {
            self.status = status;
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
        // The Measure tool: what a click would take, snapped to the model wherever that is, and the ruler, begun or laid.
        let measuring = matches!(self.tool, Tool::Measure(_)) && sketch_z.is_none() && !picking;
        let measure_pick = |at: Pos2| measure_point(scene, &cam, px(at), surface(&o.picker, &cam, at), ppp, &look3);
        if measuring {
            let screen = |q: P3| on_screen(&cam, rect, ppp, q);
            let aim = hover.filter(|_| o.drag.is_none()).and_then(measure_pick);
            if let Some((a, b)) = o.ruler {
                draw_ruler(&p, &model, a, b, &screen);
            }
            if let Some(r) = aim {
                if let Some([a, b]) = r.edge {
                    p.line_segment([screen(a), screen(b)], Stroke::new(3.0, pal().accent));
                }
                p.circle_stroke(screen(r.at), 5.0, Stroke::new(1.5, pal().accent));
                if let Some(start) = o.ruler_start {
                    draw_ruler(&p, &model, start, r.at, &screen);
                }
            } else if let Some(start) = o.ruler_start {
                p.circle_filled(screen(start), 3.5, pal().accent);
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
            crate::section_box::draw(&p, &cam, b, modify, (o.boxing.map(|(k, _)| k), o.hot_grip), screen);
        }
        // The others in a Co-Work session: what they selected, and where they look from.
        crate::cowork::paint_3d(&p, &self.cowork, &mut self.cowork_seen, &model, scene, &|q: P3| on_screen(&cam, rect, ppp, q));
        // The held handle's element as it would stand, and its height beside the arrow; then the handles, where they are now.
        if let (Some((id, h, ..)), Some((to, z))) = (o.handle, held_to) {
            for ring in handles::preview(&model, id, h, to, z) {
                let ring: Vec<Pos2> = ring.into_iter().map(&screen).filter(|q| q.is_finite()).collect();
                p.add(egui::Shape::closed_line(ring, Stroke::new(1.5, pal().accent)));
            }
            let at = screen([to.0, to.1, z]);
            if h.vertical() && at.is_finite() {
                crate::canvas::tag(&p, at + vec2(14.0, -8.0), egui::Align2::LEFT_BOTTOM, model.units.with_unit(z));
            }
        }
        // A selected wall's location line, as a plan shows it, along its base.
        if let Some(id) = single {
            let line: Vec<Pos2> = handles::location_line(&model, id).into_iter().map(&screen).filter(|q| q.is_finite()).collect();
            p.extend(egui::Shape::dashed_line(&line, Stroke::new(1.5, pal().accent), 8.0, 5.0));
        }
        for (h, q) in &own_handles {
            draw_handle(&p, &cam, *h, *q, o.handle.map(|(_, held, ..)| held), o.hot_handle, &screen);
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
        let click = click.filter(|at| handle_at(&cam, *at).is_none());
        let patched = click.is_some_and(|at| on_patch(&o.picker, at));
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
        let measure_click = click.filter(|_| measuring).map(measure_pick);
        // Where on the face a Trim click is: the side of the wall it keeps.
        let trim_at = click.filter(|_| trimming).and_then(|at| surface(&o.picker, &cam, at)).map(|s| s.at);
        // The face of what's clicked, as paint names the faces of an element.
        let face = click.and_then(|at| pick(&o.picker, at).map(|h| h.face).filter(|f| !f.is_empty()));
        // Place on Face: the face under the click, of what hosts components, by its role.
        let on_face = drawing && placing_on_face;
        let face_target = click.filter(|_| on_face).map(|at| {
            let under = o.picker.as_ref().and_then(|k| k.surface(Some(scene), &cam, px(at)));
            under.and_then(|u| Some((u.id, model.face_role(u.id, u.at, u.normal)?, u.at)))
        });
        let target = click.filter(|_| drawing && !on_face).and_then(|at| aimed(&o.picker, at));
        match clicked {
            Some(found) if modify => self.pick_up(found.into_iter().collect(), click_modifiers(ui)),
            Some(Some(id)) if demolish => self.demolish(id),
            Some(Some(id)) if paint || unpaint => self.paint_face(id, face, unpaint),
            // Nothing under the cursor: nothing to take down, or to paint.
            Some(None) if demolish || paint || unpaint => {}
            Some(_) if on_face => match face_target.flatten() {
                Some((id, face, at)) => self.place_component_on_face(id, face, at),
                None => {
                    self.status =
                        Status { text: t("Pick a face of a wall, a floor, a ceiling or a roof to place it on.").into(), error: true }
                }
            },
            Some(_) if drawing => match target {
                Some((q, plane, _)) => self.draw3d(q, plane),
                None => self.status = Status { text: self.needs_face(), error: true },
            },
            Some(_) if dimming || measuring => {}
            Some(Some(id)) if attaching && attach_target(&model, id) => self.attach_to(id),
            Some(Some(id)) if trimming && model.wall(id).is_some() => {
                let on = trim_at.map_or_else(|| model.wall(id).map_or(P2(0.0, 0.0), |w| (w.start + w.end) * 0.5), |q| P2(q[0], q[1]));
                self.trim_click(Some(id), on);
            }
            Some(_) if trimming => {}
            Some(_) if attaching => {}
            Some(_) => {
                self.status =
                    Status { text: crate::lang::t("this tool works in plan views: open one from the Project Browser").into(), error: true }
            }
            None => {}
        }
        if let Some((at, r)) = dim_click {
            self.dim3_click(&cam, at, r);
        }
        match measure_click {
            Some(Some(r)) => match self.orbit.ruler_start.take() {
                Some(start) => self.orbit.ruler = Some((start, r.at)),
                None => {
                    self.orbit.ruler = None;
                    self.orbit.ruler_start = Some(r.at);
                }
            },
            Some(None) => self.status = Status { text: t("Pick a corner, an edge or a face of the model to measure.").into(), error: true },
            None => {}
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
        if let Some(ring) = patch.filter(|_| patch_lit) {
            let name = self.work_plane_name();
            crate::workplane::name_patch(&p, ring, &name);
        }
        if patched {
            self.open_work_plane();
        }
        // What a Pick of the work plane would take.
        let m = &self.session.model;
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
        if let Some((command, params)) = handled {
            self.run(command, params);
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
        self.nav_bar3d(ui, &p, rect);
        // The navigation triad, over the picture whichever painter made it; Alt and a click on it sets the work plane.
        self.orbit.triad_draw(&p, rect, self.nav_corner(rect), &cam);
        match asked {
            Some(Ask::Plane(end)) => self.triad_plane(end, cam.target),
            Some(Ask::Level) => self.triad_level(),
            _ => {}
        }
        // A camera view remembers where the hand, the wheel and the 3D mouse left it: once they let it be, its eye and target are written
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

    /// Home View: a camera view back where its camera stands; another turned to Revit's default orientation and fitted over
    /// the model on the worker, the picture as it was meanwhile.
    pub(crate) fn home3d(&mut self) {
        let view = self.active_3d().and_then(|id| match self.session.model.elements.get(id) {
            Some(bimcraft_core::model::Element::View3D(v)) => Some((v.camera, v.lens())),
            _ => None,
        });
        match (view, self.orbit.camera) {
            (Some((Some(_), Some(_))), _) | (_, None) => {
                self.orbit.camera = None;
                self.orbit.built = None;
            }
            (view, Some(_)) => self.orbit.home(view.and_then(|(_, l)| l)),
        }
    }

    /// The view while its scene or mesh is made: the last picture, still at first so that a short wait doesn't flicker, then faded
    /// with `words` over it, and its bar as ever. With none yet, or with something wrong (`now`), the words at once.
    fn wait3d(&mut self, ui: &mut Ui, rect: Rect, words: &str, now: bool) {
        let p = ui.painter_at(rect);
        self.orbit.wait(ui.ctx(), &p, &self.card, rect, words, now);
        self.nav_bar3d(ui, &p, rect);
    }

    /// The navigation bar, at the view's top right: Home (Revit's default orientation), fit, zoom.
    fn nav_bar3d(&mut self, ui: &mut Ui, p: &egui::Painter, rect: Rect) {
        let bar = Rect::from_min_size(self.nav_corner(rect), vec2(32.0, 104.0));
        p.rect(bar, 6.0, pal().surface, Stroke::new(1.0, pal().border), StrokeKind::Inside);
        ui.scope_builder(UiBuilder::new().max_rect(bar.shrink(3.0)).layout(Layout::top_down(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if icon_button(ui, Icon::View3D, "Home View").clicked() {
                self.home3d();
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
        eprintln!(
            "on the card: shapes {} MB, copies {} MB ({}), copies' planes {} MB, texels {} MB, owners {} MB",
            (mesh.shape_vertices.len() * 44) >> 20,
            (mesh.instances.len() * 60) >> 20,
            mesh.instances.len(),
            (mesh.instance_planes.len() * 4) >> 20,
            (mesh.texels.len() * 4) >> 20,
            (mesh.owners.len() * 32) >> 20
        );
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
        // A wireframe's lines, the most of a view the card holds.
        let mut session = bimcraft_core::engine::Session::new();
        session.model = model.into();
        session.execute("visual_style", &serde_json::json!({"style": "wireframe"})).unwrap();
        let look = Look::of_3d(&session.model);
        let mesh = Arc::new(bimcraft_core::view3d::gpu::Mesh::new(&scene, &look).unwrap());
        eprintln!("wireframe: {} MB of lines", (mesh.wires.len() * 28) >> 20);
        for (chunk, room) in [(256u64, u64::MAX >> 20), (16, 64), (16, 32)] {
            let Some(mut gpu) = crate::gpu::headless() else { return };
            gpu.vertex_bytes = chunk << 20;
            gpu.card_bytes = if room == u64::MAX >> 20 { u64::MAX } else { room << 20 };
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
            eprintln!(
                "{chunk} MB buffers, room for {room} MB: moving median {:.1} ms, worst {:.1} ms",
                frames[frames.len() / 2],
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
        let storeys = std::env::var("STOREYS").ok().and_then(|s| s.parse().ok()).unwrap_or(10);
        session.model = bimcraft_core::sample::tower::annotated(bimcraft_core::sample::tower::Plate::REFERENCE, storeys).unwrap().into();
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
            if std::env::var("MESH_ONLY").is_ok() {
                let t = Instant::now();
                std::hint::black_box(bimcraft_core::view3d::gpu::Mesh::new(&scene, &look).unwrap());
                eprintln!("  mesh again, its edges kept: {:.0} ms", ms(t));
                let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
                eprintln!("  {}", status.lines().filter(|l| l.starts_with("VmHWM")).collect::<String>());
                continue;
            }
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
