//! The window's state and what its handlers do: the hub of this front end, and the one place it meets the core. It reads
//! `session.model`, draws it with `render::draw`, picks with `Drawing::pick`, and changes the model only through
//! `Session::execute`. It holds no widget: the elements are `ui`'s, so everything here is tested without a window.

use bimcraft_core::engine::Session;
use bimcraft_core::geom::P2;
use bimcraft_core::ids::Id;
use bimcraft_core::model::MAX_POINTS;
use bimcraft_core::render::{self, Drawing};
use rune::{Live, Pixels};
use serde_json::{Value, json};

use crate::camera::Camera;
use crate::orbit::Orbit;
use crate::paper::Paper;
use crate::profile::Profile;
use crate::project::{self, Start};
use crate::sched::Grid;
use crate::section::Cut;
use crate::select::{self, How, Rubber};
use crate::snapping::{Landing, Snapper};
use crate::status;
use crate::tooling::Tooling;
use crate::views::{self, ViewKind, Views};

pub mod dialogs;
mod export;
pub mod library;
mod open;
mod orbit;
mod paper;
mod schedule;
mod section;
pub mod settings;
pub mod start;
pub mod types;

/// How near a click or a hover must be to a line to take it, points.
const REACH: f32 = 4.0;
/// How far a press may travel and still be a click, points.
const CLICK: f32 = 4.0;
/// The canvas's size before the first layout says it, points.
const FIRST_SIZE: (f32, f32) = (900.0, 640.0);
/// The steps Previous Pan/Zoom keeps.
const STEPS: usize = 20;
/// How long the camera must be still, seconds, before the plan on screen, moved through the live offset and scale, is drawn exactly.
pub const SETTLE: f64 = 0.15;

/// How the plan reaches the screen: as Rune shapes (strokes and polygons the renderer draws), or as the core's own raster of it
/// (`render::raster`, tiny-skia on the CPU) shown as one image. `BIMCRAFT_RUNE_PLAN=pixels` picks the second: it is what a machine
/// with no graphics card may do better at, and the measurement in docs/areas/rune-ui.md compares the two.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlanMode {
    #[default]
    Shapes,
    Pixels,
}

impl PlanMode {
    pub fn from_env() -> PlanMode {
        match std::env::var("BIMCRAFT_RUNE_PLAN").as_deref() {
            Ok("pixels") => PlanMode::Pixels,
            _ => PlanMode::Shapes,
        }
    }
}

/// How the plan follows the camera. `Exact` asks the core to draw it again for every wheel notch and pan step. `Live` moves the last
/// drawing through Rune's live offset and scale and draws it exactly once the camera is still. Measured with no graphics card
/// (docs/areas/rune-ui.md): on Windows' software adapter, over the house, `Live` costs a twenty-fifth of `Exact` on a wheel notch and two
/// fifths on a pan step, and over the tower the two are level; on Linux's it is mixed (the house's wheel notch twice as dear, its pan step
/// half, the tower's wheel notch 0.7 and pan step 1.3), and worse on every move on the software Vulkan path. So Windows opens in `Live`,
/// the rest in `Exact`, and `BIMCRAFT_RUNE_CAMERA=live` or `exact` chooses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraMode {
    #[default]
    Exact,
    Live,
}

impl CameraMode {
    pub fn from_env() -> CameraMode {
        CameraMode::pick(std::env::var("BIMCRAFT_RUNE_CAMERA").ok().as_deref(), cfg!(windows))
    }

    /// What `BIMCRAFT_RUNE_CAMERA` says, else the platform's default.
    pub fn pick(asked: Option<&str>, windows: bool) -> CameraMode {
        match asked {
            Some("live") => CameraMode::Live,
            Some("exact") => CameraMode::Exact,
            _ if windows => CameraMode::Live,
            _ => CameraMode::Exact,
        }
    }
}

pub struct App {
    pub session: Session,
    pub profile: Profile,
    /// The ribbon's tab, an index of `tools::TOOLS`.
    pub tab: usize,
    /// The model's levels, by id and name: what the browser lists.
    pub levels: Vec<(Id, String)>,
    pub mode: PlanMode,
    /// Whether the camera moves the last drawing or has the core draw it again (`CameraMode`).
    pub cam: CameraMode,
    /// The plan as an image, in [`PlanMode::Pixels`]: replaced whenever the plan is drawn again, shown without the view re-running.
    pub pixels: Pixels,
    /// The view on screen (`views`); `level` is the level a plan view stands on, or the last one shown while another kind of view is up.
    pub view: ViewKind,
    /// What the Project Browser lists, and the name of the view shown.
    pub views: Views,
    /// The section or elevation shown, with the thread that makes its picture (`section.rs`).
    pub cut: Cut,
    /// The 3D view shown, with the thread that makes its picture (`orbit.rs`).
    pub orbit: Orbit,
    /// The sheet shown, with the thread that paints it (`paper.rs`).
    pub paper: Paper,
    /// The schedule shown, as a grid (`sched.rs`).
    pub grid: Grid,
    pub level: Option<Id>,
    pub level_name: String,
    pub camera: Camera,
    /// The camera `drawing` was made for. The plan on screen is that drawing, shown through `live_offset` and `live_scale` while the
    /// camera moves, and drawn again (exactly) once the camera has been still for [`SETTLE`].
    drawn: Camera,
    /// Where the drawn plan is on the canvas now: offset in points from where it was drawn, and scale. Rune moves it without running
    /// the view again (`El::live_offset`, `live_scale`, bound by the plan). Rune makes a `Live` of 128-bit atomics, which every
    /// platform it runs on has.
    pub live_offset: Live<[f32; 2]>,
    pub live_scale: Live<f32>,
    /// The window's clock when the camera last moved without drawing again; none while the plan on screen is exact.
    live_since: Option<f64>,
    /// The window's clock as the last handler saw it, seconds (`tick`).
    now: f64,
    /// The plan canvas's size, points: what `render::draw` is asked for, taken from the layout.
    pub size: (f32, f32),
    fitted: bool,
    /// The camera has been moved by a person: a resize no longer frames the plan again.
    moved: bool,
    pub drawing: Option<Drawing>,
    /// Counts the drawings made: what the canvas's kept shapes are keyed by.
    pub revision: u64,
    pub hovered: Option<Id>,
    /// Everything drawn under the pointer in the order `Drawing::pick_all` prefers it, and which of them Tab has reached: the one
    /// that is hovered, and that a click takes.
    pub under: Vec<Id>,
    cycle: usize,
    /// The box being dragged out over the plan, while the press that makes it is held.
    pub rubber: Option<Rubber>,
    /// Zoom In Region is armed: the next box dragged out over the plan is zoomed to, and selects nothing.
    pub region: bool,
    /// How the plan was looked at before each zoom command (not the wheel or a pan), the newest last: what Previous Pan/Zoom goes back to.
    history: Vec<Camera>,
    /// The tool picked in the Tools palette, by its id in `tools::TOOLS`.
    pub tool: Option<&'static str>,
    /// What snaps are in force, and the guide held (`snapping`).
    pub snapper: Snapper,
    /// Where a pick at the pointer would land, while a tool is in hand: what the plan has there, and the guides that hold the point.
    pub landing: Option<Landing>,
    /// The points a tool in hand has had picked, in model millimetres: the chain the plan shows, which guides and angle steps run from.
    /// The tool takes each as it is picked (`app/tooling.rs`, which says what it makes and when the points go).
    pub picked: Vec<P2>,
    /// Shift is held as the pointer moves: a pick keeps its step from the last point horizontal or vertical.
    pub square: bool,
    /// What the Draw panel has set for the tools that take a shape (`tooling.rs`).
    pub tooling: Tooling,
    pub selection: Vec<Id>,
    /// The pointer over the plan, points from its top left.
    pub cursor: Option<(f32, f32)>,
    /// The Properties palette: what it shows for the selection, and the text being typed into it (`app/properties.rs`).
    pub palette: Palette,
    /// The temporary dimensions of the element selected alone, each with the number being typed into it (`app/tempdim.rs`).
    pub temp: Vec<tempdim::TempField>,
    /// A command's refusal, as the core says it.
    pub message: Option<String>,
    /// Find and the generic command form, the modal dialogs (`app/dialogs.rs`).
    pub dialogs: dialogs::Dialogs,
    /// The user's `settings.json`: what it holds of ours and where it is (`app/settings.rs`).
    pub prefs: settings::Prefs,
    /// How far the press now dragging has been followed, points.
    followed: (f32, f32),
}

impl App {
    pub fn new(session: Session) -> App {
        let mut app = App {
            session,
            profile: Profile::default(),
            tab: 0,
            levels: Vec::new(),
            mode: PlanMode::from_env(),
            cam: CameraMode::from_env(),
            pixels: Pixels::new(),
            view: ViewKind::default(),
            views: Views::default(),
            cut: Cut::new(),
            orbit: Orbit::new(),
            paper: Paper::new(),
            grid: Grid::default(),
            level: None,
            level_name: String::new(),
            camera: Camera { cx: 0.0, cy: 0.0, scale: 0.1, angle: 0.0 },
            drawn: Camera { cx: 0.0, cy: 0.0, scale: 0.1, angle: 0.0 },
            live_offset: Live::new([0.0, 0.0]),
            live_scale: Live::new(1.0),
            live_since: None,
            now: 0.0,
            size: FIRST_SIZE,
            fitted: false,
            moved: false,
            drawing: None,
            revision: 0,
            hovered: None,
            under: Vec::new(),
            cycle: 0,
            rubber: None,
            region: false,
            history: Vec::new(),
            tool: None,
            snapper: Snapper::default(),
            landing: None,
            picked: Vec::new(),
            square: false,
            tooling: Tooling::default(),
            selection: Vec::new(),
            cursor: None,
            palette: Palette::new(),
            temp: Vec::new(),
            message: None,
            dialogs: dialogs::Dialogs::default(),
            prefs: settings::Prefs::default(),
            followed: (0.0, 0.0),
        };
        app.settle();
        app
    }

    /// The window as the arguments ask: the project, and the profile it is drawn in.
    pub fn start(start: &Start) -> App {
        let (session, why) = project::load(&start.source);
        let mut app = App::new(session);
        app.profile = start.profile;
        app.message = why;
        app
    }

    /// The sample house (the engine's own script).
    pub fn sample() -> App {
        App::start(&Start::default())
    }

    /// After the model may have changed: the level it draws is one the model has, what is selected still exists, and the plan is
    /// drawn again.
    fn settle(&mut self) {
        let model = &self.session.model;
        let levels = model.levels();
        let kept = self.level.filter(|l| levels.iter().any(|(id, _)| id == l));
        let first = kept.or_else(|| levels.first().map(|(id, _)| *id));
        // The view on screen stays while the model has it; a plan view says its level.
        let view = views::keep(model, self.view, first);
        let (level, name) = match views::level_of(model, view).or(first) {
            Some(id) => (Some(id), levels.iter().find(|(l, _)| *l == id).map(|(_, l)| l.name.clone()).unwrap_or_default()),
            None => (None, String::new()),
        };
        if level != self.level || view != self.view {
            (self.fitted, self.moved) = (false, false);
        }
        (self.level, self.level_name, self.view) = (level, name, view);
        self.views.rebuild(&self.session.model, view);
        self.levels = levels.iter().map(|(id, l)| (*id, l.name.clone())).collect();
        self.selection.retain(|id| self.session.model.elements.get(*id).is_some());
        self.hovered = self.hovered.filter(|id| self.session.model.elements.get(*id).is_some());
        self.under.retain(|id| self.session.model.elements.get(*id).is_some());
        self.cycle = self.cycle.min(self.under.len().saturating_sub(1));
        self.refresh();
        self.fit();
        self.redraw();
        self.snapper.forget();
        self.reland();
    }

    /// Frames the view when it is first shown, or shown again at another size.
    fn fit(&mut self) {
        if !self.fitted
            && let Some(view) = views::fit(&self.session.model, self.view, self.size.0 as u32, self.size.1 as u32)
        {
            self.camera = Camera::from_view(view);
            self.fitted = true;
        }
    }

    /// Draws the level for the camera and canvas as they are.
    pub fn redraw(&mut self) {
        self.drawn = self.camera;
        self.live_offset.set([0.0, 0.0]);
        self.live_scale.set(1.0);
        self.live_since = None;
        self.drawing = views::draw(&self.session.model, self.view, self.camera.view(self.size.0, self.size.1));
        self.revision += 1;
        if let (PlanMode::Pixels, Some(d)) = (self.mode, &self.drawing) {
            let view = self.camera.view(self.size.0, self.size.1);
            if let Ok(rgba) = render::raster(d, view.width, view.height, 1.0, (0.0, 0.0), None) {
                self.pixels.set(view.width, view.height, rgba);
            }
        }
    }

    fn refresh(&mut self) {
        self.refresh_panel();
        self.refresh_dims();
        // What is selected is tinted in a section's picture, and a command may have changed what a view shows.
        self.follow_view();
    }

    /// The canvas has laid out at `w` x `h` points.
    pub fn resize(&mut self, w: f32, h: f32) {
        if !(w.is_finite() && h.is_finite()) || w < 2.0 || h < 2.0 || ((w - self.size.0).abs() < 0.5 && (h - self.size.1).abs() < 0.5) {
            return;
        }
        self.size = (w, h);
        // The first real size frames the plan; once a person has moved the camera a resize keeps what it looks at.
        if !self.moved {
            self.fitted = false;
        }
        self.fit();
        self.redraw();
    }

    /// How much bigger the plan on screen is than the drawing: 1 while it is exact.
    fn live_k(&self) -> f32 {
        if self.live_since.is_some() { (self.camera.scale / self.drawn.scale) as f32 } else { 1.0 }
    }

    /// A point of the canvas as the drawing has it: the same while the plan on screen is exact; while the camera moves, the drawing is
    /// shown scaled about the canvas's middle and moved so its old middle is where the camera has it now.
    fn drawn_at(&self, (x, y): (f32, f32)) -> (f32, f32) {
        if self.live_since.is_none() {
            return (x, y);
        }
        let (w, h) = self.size;
        let at = self.camera.view(w, h).px(P2(self.drawn.cx, self.drawn.cy));
        let k = self.live_k();
        ((x - at.0) / k + w / 2.0, (y - at.1) / k + h / 2.0)
    }

    /// The element nearest a point of the canvas, within [`REACH`].
    pub fn pick(&self, x: f32, y: f32) -> Option<Id> {
        self.drawing.as_ref()?.pick(self.drawn_at((x, y)), REACH / self.live_k())
    }

    /// The pointer is at `x`, `y` over the plan. A press and its release at the same spot report the pointer again: it has not moved, so
    /// what Tab chose among what lies there stays chosen.
    pub fn hover(&mut self, x: f32, y: f32) {
        let at = (x.is_finite() && y.is_finite()).then_some((x, y));
        if at.is_some() && at == self.cursor {
            return;
        }
        self.cursor = at;
        self.rehover();
    }

    /// What lies under the pointer, as the plan on screen has it now: the nearest is hovered until Tab goes on.
    fn rehover(&mut self) {
        self.under = match (self.cursor, &self.drawing) {
            (Some(at), Some(d)) => d.pick_all(self.drawn_at(at), REACH / self.live_k()),
            _ => Vec::new(),
        };
        self.cycle = 0;
        self.hovered = self.under.first().copied();
        self.reland();
    }

    /// Where a pick at the pointer would land, with a tool in hand: nowhere without one, or with the pointer off the plan.
    fn reland(&mut self) {
        let (Some(_), Some(at), Some(level)) = (self.tool, self.cursor, self.level) else {
            self.landing = None;
            return;
        };
        let view = self.camera.view(self.size.0, self.size.1);
        self.landing = self.snapper.land(&self.session.model, level, &view, at, &self.picked, self.square);
    }

    pub fn leave(&mut self) {
        (self.cursor, self.hovered, self.cycle, self.landing) = (None, None, 0, None);
        self.under.clear();
    }

    /// Tab (`back` for Shift+Tab): on to the next element under the pointer, which a click then takes. False when fewer than two lie
    /// there, so the key is free for something else.
    pub fn step(&mut self, back: bool) -> bool {
        let n = self.under.len();
        if n < 2 {
            return false;
        }
        self.cycle = if back { (self.cycle + n - 1) % n } else { (self.cycle + 1) % n };
        self.hovered = self.under.get(self.cycle).copied();
        true
    }

    /// Escape: disarms Zoom In Region, else closes the palette's open list, else gives up the points picked, else puts down the tool in hand, else clears the selection. False
    /// when there was nothing to do.
    pub fn escape(&mut self) -> bool {
        if std::mem::take(&mut self.region) {
            return true;
        }
        // An open list of the palette closes first.
        if self.palette.open.take().is_some() {
            return true;
        }
        if !self.picked.is_empty() {
            self.picked.clear();
            self.reland();
            return true;
        }
        if self.tool.take().is_some() {
            self.landing = None;
            return true;
        }
        let some = !self.selection.is_empty();
        self.selection.clear();
        self.refresh();
        some
    }

    /// The pointer's place on the plan in model millimetres.
    pub fn cursor_mm(&self) -> Option<(f64, f64)> {
        let (x, y) = self.cursor?;
        let p = self.camera.view(self.size.0, self.size.1).mm((x, y));
        (p.0.is_finite() && p.1.is_finite()).then_some((p.0, p.1))
    }

    /// The wheel: closer (`factor` above 1) about the pointer.
    pub fn zoom(&mut self, x: f32, y: f32, factor: f32) {
        self.camera.zoom_about((x, y), self.size, f64::from(factor));
        self.moved = true;
        self.glide();
    }

    /// The window's clock, seconds, as a handler or `poll` sees it: what a camera move is stamped with.
    pub fn tick(&mut self, now: f64) {
        if now.is_finite() {
            self.now = now;
        }
    }

    /// The camera moved (a wheel notch, a pan): the plan on screen follows through the live offset and scale, which costs no drawing, and
    /// is drawn exactly once the camera is still (`settle_if_still`).
    fn glide(&mut self) {
        if self.cam == CameraMode::Exact {
            return self.redraw();
        }
        let (w, h) = self.size;
        let k = self.camera.scale / self.drawn.scale;
        let at = self.camera.view(w, h).px(P2(self.drawn.cx, self.drawn.cy));
        if !(k.is_finite() && k > 0.0 && at.0.is_finite() && at.1.is_finite()) {
            return self.redraw();
        }
        self.live_scale.set(k as f32);
        self.live_offset.set([at.0 - w / 2.0, at.1 - h / 2.0]);
        self.live_since = Some(self.now);
    }

    /// Draws the plan exactly now, if the camera has moved since it was drawn.
    pub fn settle_view(&mut self) {
        if self.live_since.is_some() {
            self.redraw();
            self.rehover();
        }
    }

    /// Draws the plan exactly if the camera has been still for [`SETTLE`]; true when it did.
    pub fn settle_if_still(&mut self) -> bool {
        let due = self.live_since.is_some_and(|since| self.now - since >= SETTLE);
        if due {
            self.redraw();
            self.rehover();
        }
        due
    }

    /// Seconds until the plan on screen must be drawn exactly, when it waits for the camera to be still.
    pub fn settle_in(&self) -> Option<f64> {
        self.live_since.map(|since| (since + SETTLE - self.now).max(0.0))
    }

    /// Notes how the plan is looked at, before a zoom command changes it: a step, unless it is the last one already.
    fn remember(&mut self) {
        if self.history.last() != Some(&self.camera) {
            self.history.push(self.camera);
        }
        if self.history.len() > STEPS {
            self.history.remove(0);
        }
    }

    /// Zoom to Fit: the level framed again.
    pub fn zoom_fit(&mut self) {
        match self.view {
            ViewKind::Section(_) => return self.section_fit(),
            ViewKind::Sheet(_) => return self.paper_fit(),
            ViewKind::View3D(_) => return self.orbit_fit(),
            _ => {}
        }
        self.remember();
        (self.fitted, self.moved) = (false, false);
        self.fit();
        self.redraw();
    }

    /// The zoom buttons: closer (`factor` above 1) or farther, about the middle of the canvas.
    pub fn zoom_by(&mut self, factor: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        match self.view {
            ViewKind::Section(_) => return self.section_zoom_by(factor),
            ViewKind::Sheet(_) => return self.paper_zoom_by(factor),
            ViewKind::View3D(_) => return self.orbit_zoom_by(factor),
            _ => {}
        }
        self.remember();
        self.camera.zoom_about((self.size.0 / 2.0, self.size.1 / 2.0), self.size, factor);
        self.moved = true;
        self.redraw();
    }

    /// Zoom In Region over the box from `a` to `b`: it fills the canvas, and the region is disarmed. False for a box with no size.
    pub fn zoom_region(&mut self, a: (f32, f32), b: (f32, f32)) -> bool {
        let Some(camera) = self.camera.region(a, b, self.size) else { return false };
        self.remember();
        (self.camera, self.moved, self.region) = (camera, true, false);
        self.redraw();
        true
    }

    /// Arms Zoom In Region, or puts it down when it is armed.
    pub fn arm_region(&mut self) {
        self.region = !self.region;
    }

    /// Previous Pan/Zoom: back to how the plan was looked at before the last zoom command. False with nothing to go back to.
    pub fn previous(&mut self) -> bool {
        let Some(camera) = self.history.pop() else { return false };
        (self.camera, self.moved) = (camera, true);
        self.redraw();
        true
    }

    /// A press on the plan starts a drag.
    pub fn grab(&mut self) {
        self.followed = (0.0, 0.0);
    }

    /// The pointer has travelled `dx`, `dy` points since the press: the plan follows it.
    pub fn drag(&mut self, dx: f32, dy: f32) {
        if !(dx.is_finite() && dy.is_finite()) {
            return;
        }
        self.camera.pan(dx - self.followed.0, dy - self.followed.1, self.size);
        (self.followed, self.moved) = ((dx, dy), true);
        self.glide();
    }

    /// The primary button is dragging, from `from` to `to`: with nothing in hand it drags out a box over the plan; with a tool in
    /// hand the plan follows, as the tools do not take the button yet.
    pub fn sweep(&mut self, from: (f32, f32), to: (f32, f32)) {
        if ![from.0, from.1, to.0, to.1].iter().all(|v| v.is_finite()) {
            return;
        }
        if self.region {
            // A region has no crossing: the box is drawn as a window whichever way it is dragged.
            if (to.0 - from.0).hypot(to.1 - from.1) > CLICK {
                self.rubber = Some(Rubber { from: (from.0.min(to.0), from.1.min(to.1)), to: (from.0.max(to.0), from.1.max(to.1)) });
            }
        } else if self.tool.is_some() {
            // ponytail: shortcut: a tool in hand pans on the primary button, the middle one's job, until the tools take it (slice 3.3).
            // A press that hardly moves is a click, which picks a point: the plan must not slip under it.
            if (to.0 - from.0).hypot(to.1 - from.1) > CLICK {
                self.drag(to.0 - from.0, to.1 - from.1);
            }
        } else if (to.0 - from.0).hypot(to.1 - from.1) > CLICK {
            self.rubber = Some(Rubber { from, to });
        }
    }

    /// The press that was dragging a box is given up (Escape, or the pointer taken away): nothing is selected.
    pub fn cancel_press(&mut self) {
        self.rubber = None;
    }

    /// The press that began at `from` ended at `to`: a click if it hardly moved, which picks; else the box it dragged out takes what
    /// it holds, or what it touches when it went right to left. `how` says what is done with what was taken.
    pub fn release(&mut self, from: (f32, f32), to: (f32, f32), how: How) {
        let band = self.rubber.take();
        if ![from.0, from.1, to.0, to.1].iter().all(|v| v.is_finite()) {
            return;
        }
        self.settle_view();
        if self.region {
            // A click leaves it armed; a box zooms to what it holds.
            if band.is_some() {
                self.zoom_region(from, to);
            }
        } else if (to.0 - from.0).hypot(to.1 - from.1) <= CLICK {
            if self.tool.is_some() {
                let before = self.picked.len();
                self.pick_point(to.0, to.1);
                if self.picked.len() > before {
                    self.tool_took_point();
                }
            } else {
                self.select_at(to.0, to.1, how);
            }
        } else if band.is_some() {
            let r = Rubber { from, to };
            let found = self.drawing.as_ref().map(|d| d.in_box(r.from, r.to, r.crossing())).unwrap_or_default();
            select::apply(&mut self.selection, found, how);
            self.refresh();
        }
    }

    /// A click with a tool in hand picks the point the pointer lands on (what the plan has there, or a guide, else the nearest 10 mm),
    /// at most [`MAX_POINTS`] of them. The pointer is taken to be where the click is.
    pub fn pick_point(&mut self, x: f32, y: f32) {
        self.hover(x, y);
        if let Some(l) = &self.landing
            && self.picked.len() < MAX_POINTS
        {
            self.picked.push(l.at);
            self.reland();
        }
    }

    /// Selects these elements (ids that no longer exist drop out when the model next changes).
    pub fn select(&mut self, ids: Vec<Id>) {
        self.selection = ids;
        self.refresh();
    }

    /// Takes what is at the point into the selection, or nothing. Where the pointer has been hovering the element Tab reached is the
    /// one a click takes, not merely the nearest.
    pub fn select_at(&mut self, x: f32, y: f32, how: How) {
        let hovering = self.cursor.is_some_and(|(cx, cy)| (cx - x).abs() < 0.5 && (cy - y).abs() < 0.5);
        let found = if hovering { self.hovered } else { self.pick(x, y) };
        select::apply(&mut self.selection, found.into_iter().collect(), how);
        self.refresh();
    }

    /// Runs a command; a refusal is kept for the status bar and changes nothing.
    fn run(&mut self, name: &str, params: &Value) -> bool {
        match self.session.execute(name, params) {
            Ok(_) => {
                self.message = None;
                self.settle();
                true
            }
            Err(refusal) => {
                self.message = Some(refusal);
                false
            }
        }
    }

    /// Shows another level's plan, framed (`open_view`: the selection stays).
    pub fn set_level(&mut self, id: Id) {
        if self.levels.iter().any(|(l, _)| *l == id) {
            self.open_view(ViewKind::Plan(id));
        }
    }

    /// Opens the ribbon's tab `i` of `tools::TOOLS`, if there is one.
    pub fn set_tab(&mut self, i: usize) {
        if i < crate::tools::TOOLS.len() {
            self.tab = i;
        }
    }

    /// Picks the tool with this id, or puts it down when it is the one in hand; an id the palette does not have picks nothing.
    pub fn set_tool(&mut self, id: &str) {
        let picked = crate::tools::id_of(id);
        self.tool = if picked == self.tool { None } else { picked };
        self.picked.clear();
        self.reland();
    }

    /// Opens the project at `path`, framed on its first level.
    pub fn open_path(&mut self, path: std::path::PathBuf) -> bool {
        let opened = self.run("open", &json!({ "path": path }));
        if opened {
            if self.session.path.is_some() {
                self.note_recent(&path);
            }
            (self.fitted, self.moved) = (false, false);
            self.history.clear();
            self.picked.clear();
            self.selection.clear();
            self.settle();
        }
        opened
    }

    /// Whether the project has a file to save to.
    pub fn has_file(&self) -> bool {
        self.session.path.is_some()
    }

    pub fn save(&mut self) -> bool {
        self.run("save", &json!({}))
    }

    pub fn save_as(&mut self, path: std::path::PathBuf) -> bool {
        let saved = self.run("save", &json!({ "path": path }));
        if saved {
            self.note_recent(&path);
        }
        saved
    }

    pub fn delete_selection(&mut self) -> bool {
        !self.selection.is_empty() && self.run("delete", &json!({"ids": self.selection}))
    }

    pub fn undo(&mut self) -> bool {
        self.run("undo", &json!({}))
    }

    pub fn redo(&mut self) -> bool {
        self.run("redo", &json!({}))
    }

    /// The status bar's line.
    pub fn status(&self) -> String {
        let hovered = self.hovered.and_then(|id| self.session.model.elements.get(id).map(|e| (e.category(), id)));
        status::line(&status::Facts {
            level: &self.views.title,
            cursor: self.cursor_mm(),
            hovered,
            selected: self.selection.len(),
            tool: self.tool.and_then(crate::tools::label),
            message: self.message.as_deref(),
        })
    }
}

mod grips;
mod menu;
mod properties;
pub use properties::Palette;
mod tempdim;
mod tooling;

#[cfg(test)]
mod tests;
