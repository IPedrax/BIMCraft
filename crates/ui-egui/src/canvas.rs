//! The plan canvas: a level's floor plan as `render::draw` lays it out (its reflected ceiling plan as
//! `render::draw_ceiling` does), panned and zoomed. Over it:
//! selection (click, Ctrl adds, Shift takes away, Tab for what's underneath, boxes), dragging,
//! the tools' points, temporary dimensions, and previews of what a click would do.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Weak};

use bimcraft_core::geom::{self, P2};
use bimcraft_core::model::{
    Clearance, Column, Component, DEFAULT_HEIGHT, DimKind, Dimension, Element, Extrusion, Floor, Id, Landing, Line, MAX_COORD, Mass, Model,
    RISER, Ramp, Reference, Roof, Section, Spot, Stair, TOL, TREAD, Tag, Wall,
};
use bimcraft_core::render::{self, Drawing, Prim, Rgb, View};
use bimcraft_core::snap::{self, Found, Kind, Reach};
use bimcraft_core::view3d::shape_snap_plan;
use bimcraft_core::walls::{self, Axis};
use bimcraft_core::{floor_outline, opening_of, roof_footprint};
use egui::{
    Align, Align2, Color32, ColorImage, Event, FontId, Key, Layout, Mesh, Modifiers, Painter, PointerButton, Pos2, Rect, Response, Sense,
    Shape, Stroke, StrokeKind, TextEdit, TextureHandle, TextureId, TextureOptions, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use crate::draw;
use crate::icons::Icon;
use crate::lang::{t, tf};
use crate::properties::number;
use crate::style::{canvas_pal, pal};
use crate::{BimApp, Sheet, Status, Tool, icon_button, top_params};

/// How near the cursor an element is picked, in px.
pub(crate) const PICK: f32 = 6.0;
/// How near the cursor a click snaps to a wall's end or a point of the chain, in px.
pub(crate) const SNAP: f64 = 10.0;
/// How long the arms of the origin marker are, in points.
pub(crate) const ORIGIN_ARM: f32 = 34.0;
/// Zoom limits, px per mm: from 1000 km to 2 cm across 1000 px.
const SCALE: (f64, f64) = (1e-6, 50.0);
/// How long a press may last and still be a click, wherever the hand went meanwhile, in seconds: a
/// click is over in a tenth of a second or so, and a drag that means to move something takes longer.
// ponytail: one number for every hand; make it a setting if it needs tuning.
const QUICK: f64 = 0.2;

/// A level's view: px per mm and the model point at the canvas centre.
#[derive(Clone, Copy, Debug)]
struct Camera {
    scale: f64,
    cx: f64,
    cy: f64,
}

/// A drag with the primary button.
#[derive(Clone, Copy, Debug)]
enum Drag {
    /// Moving the selection, from the model point first pressed.
    Move(P2),
    /// A selection box, from the pixel first pressed.
    Box((f32, f32)),
}

/// Edit Dimension References: the dimension being edited, its references as they are being picked, and where its line goes.
#[derive(Clone, Debug)]
pub(crate) struct DimensionRefsEdit {
    pub(crate) id: Id,
    pub(crate) refs: Vec<Reference>,
    pub(crate) at: P2,
}

/// How a dimension's number is drawn: its size, and the ink of the numbers a click turns into a field.
#[derive(Clone, Copy)]
struct NumberInk {
    size: f32,
    color: Color32,
}

/// Geometry does not change when the pointer, selection, tool or theme changes.
/// Keep one drawing per canvas, bounded to its latest model and framing.
#[derive(Default, Clone)]
pub(crate) struct PlanCache {
    shown: Option<PlanShot>,
    /// The pictures (a scan's) of the drawing shown, on the graphics card.
    pub(crate) pictures: Option<(Weak<Drawing>, Vec<TextureHandle>)>,
    /// Its triangles on the graphics card.
    pub(crate) retained: crate::retained::Retained,
    /// Who makes the next drawing while a view moves.
    work: PlanWork,
    /// What each room lit so far covers, of the model it was found in.
    outlines: Option<(Weak<Model>, HashMap<Id, RoomOutline>)>,
}

/// What a room covers in plan, and whether it is bounded.
type RoomOutline = (Vec<P2>, bool);

/// A plan's drawing and triangles asked of a worker: what a view that moved shows next.
struct PlanJob {
    model: Arc<Model>,
    level: Id,
    view: View,
    ceiling: bool,
    pipeline: Arc<crate::retained::Pipeline>,
    look: crate::retained::Look,
    selected: Vec<Id>,
    hovered: Option<Id>,
}

struct PlanDone {
    model: Weak<Model>,
    level: Id,
    view: View,
    ceiling: bool,
    drawing: Arc<Drawing>,
    made: Option<crate::retained::Made>,
}

fn plan_job(job: PlanJob, cancel: &crate::view_worker::Cancel) -> Option<PlanDone> {
    let PlanJob { model, level, view, ceiling, pipeline, look, selected, hovered } = job;
    let drawing = Arc::new(if ceiling { render::draw_ceiling(&model, level, view) } else { render::draw(&model, level, view) });
    if cancel.cancelled() {
        return None;
    }
    let made = crate::retained::make(&pipeline, Arc::clone(&drawing), look, &selected, hovered, &|| cancel.cancelled());
    if cancel.cancelled() {
        return None;
    }
    Some(PlanDone { model: Arc::downgrade(&model), level, view, ceiling, drawing, made })
}

/// A copy of a canvas asks for its own.
#[derive(Default)]
struct PlanWork(Option<crate::view_worker::Latest<PlanJob, PlanDone>>);

impl Clone for PlanWork {
    fn clone(&self) -> Self {
        PlanWork::default()
    }
}

type PlanShot = (Weak<Model>, Id, View, bool, Arc<Drawing>);

impl PlanCache {
    pub(crate) fn drawing(&mut self, model: &Arc<Model>, level: Id, view: View, ceiling: bool) -> Arc<Drawing> {
        if let Some((m, id, v, up, drawing)) = &self.shown
            && m.ptr_eq(&Arc::downgrade(model))
            && *id == level
            && *v == view
            && *up == ceiling
        {
            return Arc::clone(drawing);
        }
        let drawing = Arc::new(if ceiling { render::draw_ceiling(model, level, view) } else { render::draw(model, level, view) });
        self.shown = Some((Arc::downgrade(model), level, view, ceiling, Arc::clone(&drawing)));
        drawing
    }

    /// The drawing to show of a view at `view` on a card: the last whole one, while a worker makes the one for `view`, so that moving never
    /// waits for a plan. Only what the view moved over waits: another level, or an edit, is made here, and shows at once.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn moving(
        &mut self,
        ctx: &egui::Context,
        model: &Arc<Model>,
        level: Id,
        view: View,
        ceiling: bool,
        pipeline: &Arc<crate::retained::Pipeline>,
        look: crate::retained::Look,
    ) -> Arc<Drawing> {
        // Whether it was idle first: then what it finished is all there is, and nothing can come after asking.
        let idle = !self.work.0.as_ref().is_some_and(crate::view_worker::Latest::busy);
        if let Some(done) = self.work.0.as_ref().and_then(crate::view_worker::Latest::take)
            && done.model.ptr_eq(&Arc::downgrade(model))
            && (done.level, done.ceiling) == (level, ceiling)
        {
            self.shown = Some((done.model, level, done.view, ceiling, done.drawing));
            match done.made {
                Some(made) => self.retained.adopt(made),
                None => self.retained = Default::default(),
            }
        }
        let Some(drawing) = self
            .shown
            .as_ref()
            .filter(|(m, id, _, up, _)| m.ptr_eq(&Arc::downgrade(model)) && *id == level && *up == ceiling)
            .map(|(.., drawing)| Arc::clone(drawing))
        else {
            return self.drawing(model, level, view, ceiling);
        };
        if drawing.view == view {
            return drawing;
        }
        // One at a time, each for where the view is when it starts: a view that never stops still gets drawings.
        let wake = ctx.clone();
        let worker = self.work.0.get_or_insert_with(|| crate::view_worker::Latest::new(plan_job, move || wake.request_repaint()));
        if worker.take_error().is_some() {
            return self.drawing(model, level, view, ceiling);
        }
        if idle {
            let (selected, hovered) = self.retained.tints();
            worker.request(PlanJob {
                model: Arc::clone(model),
                level,
                view,
                ceiling,
                pipeline: Arc::clone(pipeline),
                look,
                selected,
                hovered,
            });
        }
        if worker.start().is_err() {
            return self.drawing(model, level, view, ceiling);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
        drawing
    }

    /// The drawing shown now.
    #[cfg(test)]
    pub(crate) fn shown_drawing(&self) -> Option<Arc<Drawing>> {
        self.shown.as_ref().map(|(.., drawing)| Arc::clone(drawing))
    }

    /// What the rooms among `ids` cover, each found once for a model, however many frames it stays lit. Those not known yet are found
    /// together, as finding a level's loops is what costs, and it is the same for all its rooms.
    pub(crate) fn room_outlines(&mut self, model: &Arc<Model>, ids: &[Id]) -> &HashMap<Id, RoomOutline> {
        if !self.outlines.as_ref().is_some_and(|(m, _)| m.ptr_eq(&Arc::downgrade(model))) {
            self.outlines = Some((Arc::downgrade(model), HashMap::new()));
        }
        let known = &mut self.outlines.get_or_insert_with(Default::default).1;
        let rooms = bimcraft_core::rooms::Rooms::new(model);
        for id in ids {
            if !known.contains_key(id)
                && let Some(Element::Room(r)) = model.elements.get(*id)
            {
                known.insert(*id, rooms.outline(r));
            }
        }
        known
    }

    /// Leaving the plans: nothing more is made, and what was is let go.
    pub(crate) fn suspend(&mut self) {
        self.work = PlanWork::default();
    }

    /// The pictures drawing `d` holds, in order, loaded once for as long as it is the one shown.
    pub(crate) fn pictures(&mut self, ctx: &egui::Context, d: &Arc<Drawing>) -> Vec<TextureId> {
        if !matches!(&self.pictures, Some((shown, _)) if shown.ptr_eq(&Arc::downgrade(d))) {
            let loaded = d
                .prims
                .iter()
                .filter_map(|p| match p {
                    Prim::Image { width, height, rgba, .. } => {
                        let pixels = ColorImage::from_rgba_unmultiplied([*width as usize, *height as usize], rgba);
                        Some(ctx.load_texture("plan picture", pixels, TextureOptions::NEAREST))
                    }
                    _ => None,
                })
                .collect();
            self.pictures = Some((Arc::downgrade(d), loaded));
        }
        self.pictures.as_ref().map(|(_, t)| t.iter().map(TextureHandle::id).collect()).unwrap_or_default()
    }
}

/// The drawing shown, and where it lies on a canvas at a view that may have moved on from it: what's under a pixel of the canvas is
/// what the drawing shows there.
pub(crate) struct Shown {
    drawing: Arc<Drawing>,
    /// Where the drawing's own pixels go.
    pub(crate) place: Place,
    /// Its top left, from the canvas's.
    offset: Vec2,
}

impl Shown {
    pub(crate) fn new(drawing: Arc<Drawing>, view: View, origin: Pos2) -> Shown {
        let (k, offset) = if drawing.view == view {
            (1.0, Vec2::ZERO)
        } else {
            let (x, y) = view.px(drawing.view.mm((0.0, 0.0)));
            ((view.scale / drawing.view.scale) as f32, vec2(x, y))
        };
        Shown { drawing, place: Place { k, ..Place::at(origin + offset) }, offset }
    }

    fn own(&self, (x, y): (f32, f32)) -> (f32, f32) {
        ((x - self.offset.x) / self.place.k, (y - self.offset.y) / self.place.k)
    }

    fn pick(&self, at: (f32, f32), tol: f32) -> Option<Id> {
        self.drawing.pick(self.own(at), tol / self.place.k)
    }

    pub(crate) fn pick_all(&self, at: (f32, f32), tol: f32) -> Vec<Id> {
        self.drawing.pick_all(self.own(at), tol / self.place.k)
    }

    fn in_box(&self, a: (f32, f32), b: (f32, f32), crossing: bool) -> Vec<Id> {
        self.drawing.in_box(self.own(a), self.own(b), crossing)
    }
}

impl std::ops::Deref for Shown {
    type Target = Drawing;
    fn deref(&self) -> &Drawing {
        &self.drawing
    }
}

/// What an alignment service was built from: the level, the model's counters and address, and so the model it is for.
struct AlignmentCache {
    level: Id,
    next_id: Id,
    len: usize,
    at: usize,
    service: bimcraft_core::alignment::AlignmentGuideService,
}

pub struct Canvas {
    pub(crate) drawing: PlanCache,
    cams: BTreeMap<Id, Camera>,
    /// Where the plan was last shown, in points.
    rect: Rect,
    /// How much of its height the cards floating over the view (its tabs at the top, its view control bar at the foot) take
    /// between them, points: a view is fitted into the rest.
    pub(crate) room: f32,
    /// The level shown last.
    level: Option<Id>,
    /// The model point under the cursor.
    pub cursor: Option<P2>,
    /// The model point a middle-button drag holds under the cursor.
    grab: Option<P2>,
    drag: Option<Drag>,
    /// What's under the cursor, as `Drawing::pick_all` orders it, and how far Tab has gone.
    under: Vec<Id>,
    cycle: usize,
    /// A crop region's grip being dragged: the view, and which edge (see [`grips`]).
    crop_drag: Option<(Id, usize)>,
    /// A selected wall's end being dragged: the wall, and whether it's its start.
    grip: Option<(Id, bool)>,
    /// A part of the selected section or elevation being dragged in plan: the view, the part (its line, an end, its far
    /// clip) and the model point it was taken at.
    view_grip: Option<(Id, bimcraft_core::elevation::Part, P2)>,
    /// A selected level's end being dragged in a section or an elevation: the level, and whether
    /// it's its far end.
    level_grip: Option<(Id, bool)>,
    /// Where the primary button last went down on a view, and when.
    press: Option<(Pos2, f64)>,
    /// The primary button's release this frame: where it went down, and whether it came up within
    /// [`QUICK`] of that, so it was a click whatever the hand did between.
    lifted: Option<(Pos2, bool)>,
    /// The selection a drag from an element replaced, for a drag that turns out a click.
    prior: Option<Vec<Id>>,
    /// What the last point a tool previewed snapped to this frame, if one did.
    pub(crate) found: Cell<Option<Found>>,
    /// The cached alignment service (see [`AlignmentCache`]). A command gives the
    /// session a model of its own to change while the history keeps the one before, so a moved element, an undo and a redo
    /// change the address though they change neither count. shortcut: an address freed and taken again by a model of the same
    /// counts would hold stale guides until the next command; a revision counter on the model would close that.
    alignment_cache: RefCell<Option<AlignmentCache>>,
}

impl Default for Canvas {
    fn default() -> Canvas {
        Canvas {
            drawing: PlanCache::default(),
            cams: BTreeMap::new(),
            rect: Rect::ZERO,
            room: 0.0,
            level: None,
            cursor: None,
            grab: None,
            drag: None,
            under: Vec::new(),
            cycle: 0,
            crop_drag: None,
            grip: None,
            view_grip: None,
            level_grip: None,
            press: None,
            lifted: None,
            prior: None,
            found: Cell::new(None),
            alignment_cache: RefCell::new(None),
        }
    }
}

impl Canvas {
    /// View `level` (a plan's key, a section or an elevation) as it was last shown: its zoom, its middle and its size.
    pub(crate) fn view(&self, level: Id) -> Option<View> {
        let c = self.cams.get(&level)?;
        Some(View {
            scale: c.scale,
            cx: c.cx,
            cy: c.cy,
            width: self.rect.width().max(1.0) as u32,
            height: self.rect.height().max(1.0) as u32,
        })
    }

    /// Shows view `level` with its middle at `cx`, `cy` and `scale` pixels a millimetre: following someone in Co-Work.
    pub(crate) fn set_view(&mut self, level: Id, cx: f64, cy: f64, scale: f64) {
        self.cams.insert(level, Camera { scale, cx, cy });
    }

    /// Where model point `p` shows on screen.
    pub fn screen(&self, p: P2) -> Pos2 {
        match self.level.and_then(|l| self.view(l)) {
            Some(v) => self.rect.min + Vec2::from(v.px(p)),
            None => self.rect.center(),
        }
    }

    /// Whether the cursor was over the plan last frame.
    pub fn hovering(&self) -> bool {
        self.cursor.is_some()
    }

    /// Where the primary button went down, if it came up this frame within [`QUICK`] of that: a
    /// click, whatever the hand did between, and no drag it began is to be finished.
    pub(crate) fn quick_press(&self) -> Option<Pos2> {
        self.lifted.filter(|(_, quick)| *quick).map(|(at, _)| at)
    }

    /// A copy for a view drawn as it is, not the active one: its cameras, and nothing a hand is doing (a press, a
    /// release, a drag, a grip), which is the active view's.
    pub(crate) fn at_rest(&self) -> Canvas {
        Canvas {
            drawing: self.drawing.clone(),
            cams: self.cams.clone(),
            rect: self.rect,
            room: self.room,
            level: self.level,
            under: self.under.clone(),
            ..Canvas::default()
        }
    }

    /// Whether the primary button came up quickly this frame ([`Canvas::quick_press`]).
    pub(crate) fn quick(&self) -> bool {
        self.quick_press().is_some()
    }

    /// Note this frame's primary press or release over a view's `response`: where it went down and
    /// when, and on a release, whether it came up within [`QUICK`].
    pub(crate) fn track(&mut self, ui: &Ui, response: &Response) {
        let (pressed, released, now) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.time));
        if pressed && response.is_pointer_button_down_on() {
            self.press = ui.input(|i| i.pointer.press_origin()).map(|at| (at, now));
        }
        self.lifted = if released { self.press.take().map(|(at, began)| (at, now - began <= QUICK)) } else { None };
    }

    /// Show the rectangle `lo` to `hi` (mm) of view `level`, with room round it, zoomed no closer than a 2 m square fills it.
    pub fn show(&mut self, level: Id, lo: P2, hi: P2) {
        let (w, h) = if self.rect.width() > 1.0 { (f64::from(self.rect.width()), f64::from(self.rect.height())) } else { (800.0, 600.0) };
        let (dx, dy) = ((hi.0 - lo.0).max(2000.0), (hi.1 - lo.1).max(2000.0));
        let scale = (0.7 * (w / dx).min(h / dy)).clamp(SCALE.0, SCALE.1);
        let (cx, cy) = ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0);
        if scale.is_finite() && cx.is_finite() && cy.is_finite() {
            self.cams.insert(level, Camera { scale, cx: cx.clamp(-MAX_COORD, MAX_COORD), cy: cy.clamp(-MAX_COORD, MAX_COORD) });
        }
    }

    #[cfg(test)]
    pub(crate) fn current_view(&self, id: Id) -> Option<View> {
        self.view(id)
    }

    /// Worker-computed initial framing, installed only if navigation has not supplied one.
    pub(crate) fn has_view(&self, id: Id) -> bool {
        self.cams.contains_key(&id)
    }

    pub(crate) fn fitted(&mut self, id: Id, view: View) {
        self.cams.entry(id).or_insert(Camera { scale: view.scale, cx: view.cx, cy: view.cy });
    }

    /// Show all of `level` again, the next time it's drawn.
    pub fn fit(&mut self, level: Id) {
        self.cams.remove(&level);
    }

    /// View `id` (a plan's level, a section or an elevation) in `rect`, fitted by `fit` the first
    /// time it shows: the wheel zooms about the cursor and the middle button pans. The view, and the
    /// cursor's pixel in it while it's over it.
    pub(crate) fn navigate(
        &mut self,
        ui: &Ui,
        response: &Response,
        rect: Rect,
        id: Id,
        fit: impl FnOnce(u32, u32) -> View,
    ) -> Option<(View, Option<(f32, f32)>)> {
        self.track(ui, response);
        self.rect = rect;
        self.level = Some(id);
        self.cams.entry(id).or_insert_with(|| {
            // Clear of the cards floating over its top and foot, which take as much of each.
            let v = fit(rect.width().max(1.0) as u32, (rect.height() - self.room).max(1.0) as u32);
            Camera { scale: v.scale, cx: v.cx, cy: v.cy }
        });
        let (hover, wheel, pinch, middle) = ui.input(|i| {
            (
                i.pointer.hover_pos().filter(|_| response.hovered()),
                i.smooth_scroll_delta.y,
                i.zoom_delta(),
                (i.pointer.button_pressed(PointerButton::Middle), i.pointer.button_down(PointerButton::Middle)),
            )
        });
        if let (Some(at), Some(v)) = (hover, self.view(id)) {
            let at = at - rect.min;
            // A wheel notch (about 50 points) zooms by a fifth.
            let factor = (f64::from(wheel) * 0.004).exp() * f64::from(pinch);
            if factor.is_finite() && factor != 1.0 {
                self.hold(id, v.mm(at.into()), at, v.scale * factor.clamp(0.2, 5.0));
            }
            if middle.0 {
                self.grab = Some(v.mm(at.into()));
            }
        }
        match (self.grab, ui.input(|i| i.pointer.hover_pos()), self.view(id)) {
            (Some(g), Some(at), Some(v)) if middle.1 => self.hold(id, g, at - rect.min, v.scale),
            _ => self.grab = None,
        }
        Some((self.view(id)?, hover.map(|p| (p.x - rect.min.x, p.y - rect.min.y))))
    }

    /// View `id` moved `dx` mm along, to keep what's on screen in place when its coordinates move
    /// under it (a section's start moving along its line).
    pub(crate) fn slide(&mut self, id: Id, dx: f64) {
        if let Some(c) = self.cams.get_mut(&id) {
            c.cx += dx;
        }
    }

    /// Model point `p` in the middle of view `id`, at the zoom it has: where the minimap is clicked or dragged to.
    pub(crate) fn centre(&mut self, id: Id, p: P2) {
        if let Some(v) = self.view(id) {
            self.hold(id, p, self.rect.size() / 2.0, v.scale);
        }
    }

    /// Zoom by `factor` about the middle of the view.
    pub(crate) fn zoom(&mut self, level: Id, factor: f64) {
        if let Some(v) = self.view(level) {
            let mid = self.rect.size() / 2.0;
            self.hold(level, v.mm(mid.into()), mid, v.scale * factor);
        }
    }

    /// Keep model point `p` at pixel `at` (canvas coordinates) with `scale` px per mm.
    fn hold(&mut self, level: Id, p: P2, at: Vec2, scale: f64) {
        let scale = scale.clamp(SCALE.0, SCALE.1);
        let (w, h) = (f64::from(self.rect.width()), f64::from(self.rect.height()));
        let cx = (p.0 - (f64::from(at.x) - w / 2.0) / scale).clamp(-MAX_COORD, MAX_COORD);
        let cy = (p.1 + (f64::from(at.y) - h / 2.0) / scale).clamp(-MAX_COORD, MAX_COORD);
        if cx.is_finite() && cy.is_finite() {
            self.cams.insert(level, Camera { scale, cx, cy });
        }
    }
}

/// A drawing's colour on screen: as drawn, or in the dark theme with its lightness turned over,
/// so ink reads light on a dark ground and the grid stays blue. Its paper is the user's canvas colour when there is one.
pub(crate) fn color(c: Rgb) -> Color32 {
    if let Some(ground) = crate::style::canvas_ground().filter(|_| c == render::PAPER) {
        return ground;
    }
    if !crate::style::canvas_dark() {
        return Color32::from_rgb(c.0, c.1, c.2);
    }
    let (r, g, b) = (i32::from(c.0), i32::from(c.1), i32::from(c.2));
    let shift = 255 - r.max(g).max(b) - r.min(g).min(b);
    let over = |v: i32| (26 + (v + shift).clamp(0, 255) * 206 / 255) as u8;
    Color32::from_rgb(over(r), over(g), over(b))
}

/// Whether a ring turns one way all round.
fn convex(r: &[(f32, f32)]) -> bool {
    let n = r.len();
    let turns: Vec<f32> = (0..n)
        .map(|i| {
            let (a, b, c) = (r[i], r[(i + 1) % n], r[(i + 2) % n]);
            (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0)
        })
        .filter(|t| t.abs() > 1e-3)
        .collect();
    turns.iter().all(|t| *t > 0.0) || turns.iter().all(|t| *t < 0.0)
}

pub(crate) fn pos(origin: Pos2, (x, y): (f32, f32)) -> Pos2 {
    origin + vec2(x, y)
}

/// Where a drawing's pixels go on screen: its (0, 0) at `origin`, `k` screen px a drawing px (a
/// sheet is drawn at paper density, and scaled as its view zooms). On `paper`, its colours stay
/// as drawn in either theme.
#[derive(Clone, Copy)]
pub(crate) struct Place {
    pub origin: Pos2,
    pub k: f32,
    pub paper: bool,
    /// What of the drawing is left out: a note being written, which its editor draws.
    pub hide: Option<Id>,
}

impl Place {
    pub(crate) fn at(origin: Pos2) -> Place {
        Place { origin, k: 1.0, paper: false, hide: None }
    }
    pub(crate) fn pos(self, (x, y): (f32, f32)) -> Pos2 {
        self.origin + vec2(x, y) * self.k
    }
}

/// How many screen pixels wide a line `width` px wide in a drawing shown `k` times its size draws: as wide as its pen
/// on the paper the view stands for, at least a pixel, so a hairline (0) is one at any zoom; with Thin Lines, one.
pub(crate) fn on_screen(width: f32, k: f32, thin: bool) -> f32 {
    if thin { 1.0 } else { (width * k).max(1.0) }
}

/// The display list, with the selected and hovered elements in the accent colours, but for what `at` leaves out; with
/// `thin`, every line one pixel wide. Its pictures are `pictures`, in order.
pub(crate) fn paint(p: &Painter, at: Place, d: &Drawing, selected: &[Id], hovered: Option<Id>, thin: bool, pictures: &[TextureId]) {
    paint_to(
        &mut |shape| {
            p.add(shape);
        },
        p.clip_rect(),
        at,
        d,
        selected,
        hovered,
        thin,
        pictures,
    );
}

/// Whether `prim` draws nothing inside `clip`, however it is drawn: all of it, past its widest line, to one side.
fn outside(prim: &Prim, at: Place, thin: bool, clip: Rect) -> bool {
    let (mut lo, mut hi) = (Pos2::new(f32::INFINITY, f32::INFINITY), Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY));
    let mut grow = |q: &(f32, f32)| {
        let q = at.pos(*q);
        (lo, hi) = (lo.min(q), hi.max(q));
    };
    // A mitred corner reaches past its line by some widths, and the edge is feathered: more than either could ever take.
    let margin = match prim {
        Prim::Fill { rings, .. } => {
            rings.iter().flatten().for_each(&mut grow);
            4.0
        }
        Prim::Line { pts, width, .. } => {
            pts.iter().for_each(&mut grow);
            on_screen(*width, at.k, thin) * 8.0 + 4.0
        }
        // A picture's place has more to it; it is few, and drawn.
        Prim::Image { .. } => return false,
    };
    // Comparisons a number that isn't finite fails say nothing is outside.
    hi.x + margin < clip.min.x || lo.x - margin > clip.max.x || hi.y + margin < clip.min.y || lo.y - margin > clip.max.y
}

/// [`paint`], its shapes handed to `out`, those that can't reach `clip` left out.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_to(
    out: &mut dyn FnMut(Shape),
    clip: Rect,
    at: Place,
    d: &Drawing,
    selected: &[Id],
    hovered: Option<Id>,
    thin: bool,
    pictures: &[TextureId],
) {
    let mut pictures = pictures.iter();
    // Looked up for each primitive: not searched through.
    let set: HashSet<Id> = selected.iter().copied().collect();
    for ((prim, owner), part) in d.prims.iter().zip(&d.owners).zip(&d.parts) {
        paint_prim(out, clip, at, (prim, *owner, *part), &set, hovered, thin, &mut pictures);
    }
}

/// One primitive of a drawing, as [`paint`] draws it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_prim(
    out: &mut dyn FnMut(Shape),
    clip: Rect,
    at: Place,
    (prim, owner, part): (&Prim, Option<Id>, render::Part),
    selected: &HashSet<Id>,
    hovered: Option<Id>,
    thin: bool,
    pictures: &mut std::slice::Iter<TextureId>,
) {
    let color = |c: Rgb| if at.paper { Color32::from_rgb(c.0, c.1, c.2) } else { color(c) };
    if owner.is_some() && owner == at.hide {
        return;
    }
    if outside(prim, at, thin, clip) {
        return;
    }
    let accent = match owner {
        Some(id) if selected.contains(&id) => Some((canvas_pal().accent, canvas_pal().select_fill)),
        Some(id) if Some(id) == hovered => Some((canvas_pal().hover, canvas_pal().hover_fill)),
        _ => None,
    };
    match prim {
        // Notes and compound pattern glyphs use the non-zero fill rule, preserving counters and overlapping contours.
        // Single-contour pattern fills (including dots) keep their ordinary polygon rendering below.
        Prim::Fill { rings, color: c } if part == render::Part::Text || (part == render::Part::LinePattern && rings.len() > 1) => {
            let fill = accent.map_or(color(*c), |a| a.0);
            let mut mesh = Mesh::default();
            for tri in letter(rings).iter() {
                let first = mesh.vertices.len() as u32;
                for q in tri {
                    mesh.colored_vertex(at.pos(*q), fill);
                }
                mesh.add_triangle(first, first + 1, first + 2);
            }
            out(Shape::mesh(mesh));
            for r in rings {
                out(Shape::closed_line(r.iter().map(|q| at.pos(*q)).collect(), Stroke::new(0.5, fill)));
            }
        }
        // Ordinary fills are convex rings or independent polygons, triangulated without seams.
        Prim::Fill { rings, color: c } => {
            let fill = accent.map_or(color(*c), |a| a.1);
            match rings.as_slice() {
                [r] if convex(r) => {
                    out(Shape::convex_polygon(r.iter().map(|q| at.pos(*q)).collect(), fill, Stroke::NONE));
                }
                _ => {
                    let mut mesh = Mesh::default();
                    for r in rings {
                        let ring: Vec<P2> = r.iter().map(|q| P2(f64::from(q.0), f64::from(q.1))).collect();
                        let first = mesh.vertices.len() as u32;
                        for q in r {
                            mesh.colored_vertex(at.pos(*q), fill);
                        }
                        for [a, b, c] in geom::triangulate(&ring) {
                            mesh.add_triangle(first + a as u32, first + b as u32, first + c as u32);
                        }
                    }
                    out(Shape::mesh(mesh));
                }
            }
        }
        Prim::Image { at: corner, width, height, ink, .. } => {
            if let Some(texture) = pictures.next() {
                let rect = Rect::from_min_size(at.pos(*corner), vec2(*width as f32, *height as f32) * at.k);
                // On a dark canvas a plan's picture (a scan's) is dimmed, so the ink lightened over it still reads; a sheet's
                // paper stays white, and so does a scan the view fills with a colour of its own, which is ink itself.
                let plain = if !at.paper && !*ink && crate::style::canvas_dark() { Color32::from_gray(120) } else { Color32::WHITE };
                let tint = accent.map_or(plain, |a| a.1);
                out(Shape::image(*texture, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint));
            }
        }
        Prim::Line { pts, width, color: c, dash, closed } => {
            let stroke = Stroke::new(on_screen(*width, at.k, thin), accent.map_or(color(*c), |a| a.0));
            let mut pts: Vec<Pos2> = pts.iter().map(|q| at.pos(*q)).collect();
            match dash {
                Some(dash) => {
                    // A dashed ring: its closing side too.
                    if *closed {
                        pts.extend(pts.first().copied());
                    }
                    // Its on and off lengths, as big as its width is drawn.
                    let k = at.k.max(f32::EPSILON);
                    let on: Vec<f32> = dash.iter().step_by(2).map(|x| x * k).collect();
                    let off: Vec<f32> = dash.iter().skip(1).step_by(2).map(|x| x * k).collect();
                    Shape::dashed_line_with_offset(&pts, stroke, &on, &off, 0.0).into_iter().for_each(&mut *out);
                }
                None if *closed => {
                    out(Shape::closed_line(pts, stroke));
                }
                None => {
                    out(Shape::line(pts, stroke));
                }
            }
        }
    }
}

type Triangles = std::sync::Arc<Vec<[(f32, f32); 3]>>;

/// The triangles that fill a letter's rings, kept for the frames that draw it again (hover, selection, the caret).
fn letter(rings: &[Vec<(f32, f32)>]) -> Triangles {
    use std::hash::{Hash, Hasher};
    thread_local! {
        static KEPT: std::cell::RefCell<std::collections::HashMap<u64, Triangles>> = Default::default();
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for r in rings {
        r.len().hash(&mut h);
        for q in r {
            (q.0.to_bits(), q.1.to_bits()).hash(&mut h);
        }
    }
    let key = h.finish();
    if let Some(t) = KEPT.with(|k| k.borrow().get(&key).cloned()) {
        return t;
    }
    let t: Triangles = std::sync::Arc::new(bimcraft_core::text::triangles(rings));
    KEPT.with(|k| {
        let mut k = k.borrow_mut();
        if k.len() > 50_000 {
            k.clear();
        }
        k.insert(key, t.clone());
    });
    t
}

/// The selected elements' outlines taken by `f` (on screen): where a move, a turn or a mirror
/// puts them.
fn ghost(p: &Painter, place: Place, d: &Drawing, selected: &[Id], f: &dyn Fn(Pos2) -> Pos2) {
    let stroke = Stroke::new(1.0, pal().accent);
    let at = |q: &(f32, f32)| f(place.pos(*q));
    let selected: HashSet<Id> = selected.iter().copied().collect();
    for (prim, owner) in d.prims.iter().zip(&d.owners) {
        if !owner.is_some_and(|id| selected.contains(&id)) {
            continue;
        }
        match prim {
            Prim::Fill { rings, .. } => {
                for r in rings {
                    p.add(Shape::closed_line(r.iter().map(at).collect(), stroke));
                }
            }
            Prim::Line { pts, closed: true, .. } => {
                p.add(Shape::closed_line(pts.iter().map(at).collect(), stroke));
            }
            Prim::Line { pts, .. } => {
                p.add(Shape::line(pts.iter().map(at).collect(), stroke));
            }
            Prim::Image { .. } => {}
        }
    }
}

/// A short reading drawn where a tool previews, on a soft background so it stays legible over
/// what's drawn: `text` anchored at `at` as `align` says.
pub(crate) fn tag(p: &Painter, at: Pos2, align: Align2, text: impl Into<String>) {
    let galley = p.layout_no_wrap(text.into(), FontId::proportional(12.0), pal().accent);
    let rect = align.anchor_size(at, galley.size());
    p.rect_filled(rect.expand2(vec2(3.0, 1.0)), 2.0, pal().surface.gamma_multiply(0.85));
    p.galley(rect.min, galley, pal().accent);
}

/// The origin marker at the screen point `at`, the project's 0, 0: an arrow east for x in the canvas's red and one north (up the
/// screen) for y in its green, each lettered at its tip. Its arrows keep their size at any zoom. Not in the drawing: a PNG, a sheet
/// or a printout is made without it.
fn origin_marker(p: &Painter, at: Pos2) {
    // Not drawn where neither it nor its arms are on the screen: the plan may be anywhere.
    if !p.clip_rect().expand(2.0).intersects(Rect::from_min_max(at - vec2(4.0, ORIGIN_ARM + 16.0), at + vec2(ORIGIN_ARM + 16.0, 4.0))) {
        return;
    }
    let colours = canvas_pal();
    for (arm, colour, letter, align, off) in [
        (vec2(ORIGIN_ARM, 0.0), colours.error, "X", Align2::LEFT_CENTER, vec2(4.0, 0.0)),
        (vec2(0.0, -ORIGIN_ARM), colours.context, "Y", Align2::CENTER_BOTTOM, vec2(0.0, -4.0)),
    ] {
        let (stroke, tip) = (Stroke::new(1.5, colour), at + arm);
        let (along, across) = (arm / ORIGIN_ARM, vec2(-arm.y, arm.x) / ORIGIN_ARM);
        p.line_segment([at, tip], stroke);
        for side in [-1.0, 1.0] {
            p.line_segment([tip, tip - along * 7.0 + across * (3.5 * side)], stroke);
        }
        p.text(tip + off, align, letter, FontId::proportional(11.0), colour);
    }
    p.circle_filled(at, 2.5, colours.text);
}

impl BimApp {
    /// The view's name in the bottom left corner of the canvas `rect`, over the view control bar where that floats over the corner.
    fn corner_name(&self, ctx: &egui::Context, p: &Painter, rect: Rect, name: &str) {
        let galley = p.layout_no_wrap(name.to_string(), FontId::proportional(12.0), pal().text);
        let at = rect.left_bottom() + vec2(12.0, -(self.bar_cover(ctx) + 8.0));
        let box_ = Align2::LEFT_BOTTOM.anchor_size(at, galley.size());
        p.rect_filled(box_.expand2(vec2(5.0, 2.0)), 3.0, pal().surface.gamma_multiply(0.85));
        p.galley(box_.min, galley, pal().text);
    }
}

/// A dimension line between the screen points `a` and `b`, held `off` px out to the side `n` of them, with a tick at each
/// end as a temporary dimension has; where its number goes.
fn dimension_line(p: &Painter, a: Pos2, b: Pos2, n: Vec2, off: f32) -> Pos2 {
    let (along, stroke) = (b - a, Stroke::new(1.0, pal().accent));
    let u = along / along.length().max(f32::EPSILON);
    p.line_segment([a + n * off, b + n * off], stroke);
    for e in [a, b] {
        p.line_segment([e + n * (off - 10.0).max(2.0), e + n * (off + 4.0)], stroke);
        p.line_segment([e + n * off - (u + n) * 3.0, e + n * off + (u + n) * 3.0], stroke);
    }
    a + along / 2.0 + n * (off + 11.0)
}

/// A padlock at `at` in `ink`: locked, its body filled and its shackle shut; open, its body hollow and its shackle swung up and
/// out about the foot it keeps on the body, so it can't be read as locked.
fn padlock(p: &Painter, at: Pos2, locked: bool, ink: Color32) {
    let body = Rect::from_center_size(at + vec2(0.0, 3.0), vec2(10.0, 8.0));
    p.rect_filled(body, 1.5, if locked { ink } else { pal().surface });
    p.rect_stroke(body, 1.5, Stroke::new(1.5, ink), StrokeKind::Middle);
    let top = body.top();
    let mut shackle: Vec<Pos2> = (0..=8)
        .map(|k| {
            let t = std::f32::consts::PI * k as f32 / 8.0;
            pos2(body.center().x - 3.0 * t.cos(), top - 4.0 * t.sin())
        })
        .collect();
    if !locked {
        // An open shackle, swung up and out about the foot it keeps on the body: clearly not shut.
        let pivot = pos2(body.center().x - 3.0, top);
        let turn = -0.7_f32;
        for pt in &mut shackle {
            let d = *pt - pivot;
            *pt = pivot + vec2(d.x * turn.cos() - d.y * turn.sin(), d.x * turn.sin() + d.y * turn.cos());
        }
    }
    p.add(Shape::line(shackle, Stroke::new(1.5, ink)));
}

/// An opening's temporary dimensions, drawn on `p` beside its host wall: from the face of the wall meeting its wall
/// before it (or the wall's start) to the middle of the opening, and from there to the next face (or the wall's end)
/// (`Model::opening_span`), each as where its number goes, its text, and its length in mm. None on a curved wall, and for
/// a span too short to draw.
pub(crate) fn opening_dimensions(p: &Painter, m: &Model, v: &View, o: &bimcraft_core::model::Opening) -> Vec<(Pos2, String, f64)> {
    let (Some(w), Some((start, end))) = (m.wall(o.host).filter(|w| w.arc == 0.0), m.opening_span(o)) else { return Vec::new() };
    let axis = Axis::of(w);
    let (origin, c) = (p.clip_rect().min, axis.at(o.offset, 0.0));
    let (from, to) = (axis.at(start, 0.0), axis.at(end, 0.0));
    let screen = |q: P2| pos(origin, v.px(q));
    let (a, b) = (screen(w.start), screen(w.end));
    let chord = b - a;
    if chord.length() < 1.0 {
        return Vec::new();
    }
    // On the side away from the middle of the view, where the door swings or the window faces out.
    let n = vec2(chord.y, -chord.x).normalized();
    let mid = a + chord / 2.0;
    let n = if (mid + n - p.clip_rect().center()).length() < (mid - n - p.clip_rect().center()).length() { -n } else { n };
    let off = (m.thickness(w) / 2.0 * v.scale) as f32 + 18.0;
    [(from, c), (c, to)]
        .into_iter()
        .filter(|(s, e)| screen(*s).distance(screen(*e)) >= 24.0)
        .map(|(s, e)| (dimension_line(p, screen(s), screen(e), n, off), m.units.length(s.dist(e)), s.dist(e)))
        .collect()
}

/// A straight wall's temporary dimensions to the nearest parallel wall or grid on its left and its right
/// (`Model::wall_clearances`), drawn on `p` square to it a little along it from its middle: which side, where its number
/// goes, its text and the clearance. None for a curved wall, a side with no such line, or a span too short to draw.
pub(crate) fn wall_dimensions(p: &Painter, m: &Model, v: &View, id: Id) -> Vec<(usize, Pos2, String, Clearance)> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    let origin = p.clip_rect().min;
    let screen = |q: P2| pos(origin, v.px(q));
    let along = screen(w.end) - screen(w.start);
    if along.length() < 24.0 {
        return Vec::new();
    }
    let side = along / along.length();
    m.wall_clearances(id)
        .into_iter()
        .enumerate()
        .filter_map(|(n, c)| Some((n, c?)))
        .filter(|(_, c)| screen(c.from).distance(screen(c.from + c.toward * c.distance)) >= 24.0)
        .map(|(n, c)| {
            let (a, b) = (screen(c.from), screen(c.from + c.toward * c.distance));
            (n, dimension_line(p, a, b, side, 36.0), m.units.length(c.distance), c)
        })
        .collect()
}

/// A column's or component's temporary dimensions, drawn on `p` beside it: how far it stands from the nearest wall centre
/// line or grid east, north, west and south (`Model::clearances`), each as which of the four, where its number goes, its
/// text and the clearance. None for any other element, and for a way with no line or a span too short to draw.
pub(crate) fn standing_dimensions(p: &Painter, m: &Model, v: &View, e: &Element) -> Vec<(usize, Pos2, String, Clearance)> {
    let (level, at, reach) = match e {
        Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c) => (c.level, c.at, m.column_outline(e)),
        Element::Component(c) => (c.level, c.at, m.component_pieces(c).into_iter().flat_map(|piece| piece.outline).collect()),
        _ => return Vec::new(),
    };
    let origin = p.clip_rect().min;
    let screen = |q: P2| pos(origin, v.px(q));
    // Clear of the element, out to its farthest corner.
    let off = (reach.iter().map(|q| q.dist(at)).fold(0.0, f64::max) * v.scale) as f32 + 18.0;
    m.clearances(level, at)
        .into_iter()
        .enumerate()
        .filter_map(|(n, c)| Some((n, c?)))
        .filter(|(_, c)| screen(at).distance(screen(at + c.toward * c.distance)) >= 24.0)
        .map(|(n, c)| {
            // East and west dimensions lie north of it, north and south ones east of it.
            let side = if n % 2 == 0 { vec2(0.0, -1.0) } else { vec2(1.0, 0.0) };
            (n, dimension_line(p, screen(at), screen(at + c.toward * c.distance), side, off), m.units.length(c.distance), c)
        })
        .collect()
}

/// A ruler from `a` to `b`: its line, and its graduations at round lengths at least 10 px apart,
/// every fifth longer.
fn ruler_shapes(sc: &Scene, a: P2, b: P2) -> Vec<Shape> {
    let (pa, pb) = (sc.screen(a), sc.screen(b));
    let stroke = Stroke::new(1.0, pal().accent);
    let mut shapes = vec![Shape::line_segment([pa, pb], Stroke::new(2.0, pal().accent))];
    let len = pa.distance(pb);
    if len < 30.0 {
        return shapes;
    }
    let (u, step) = ((pb - pa) / len, (snap::graduation(sc.v.scale) * sc.v.scale) as f32);
    let n = vec2(u.y, -u.x);
    for k in 1..=((len / step) as usize).min(500) {
        let (at, half) = (pa + u * (step * k as f32), if k % 5 == 0 { 7.0 } else { 3.5 });
        shapes.push(Shape::line_segment([at - n * half, at + n * half], stroke));
    }
    shapes
}

/// The angle at `last` between the line back to `prev` and the line on to `next`, as a small arc.
fn corner_arc(sc: &Scene, prev: P2, last: P2, next: P2) -> Option<Shape> {
    let (c, a, b) = (sc.screen(last), sc.screen(prev) - sc.screen(last), sc.screen(next) - sc.screen(last));
    if a.length() < 30.0 || b.length() < 30.0 {
        return None;
    }
    let (a0, a1) = (a.y.atan2(a.x), b.y.atan2(b.x));
    // The way round that's the smaller angle.
    let mut sweep = a1 - a0;
    while sweep > std::f32::consts::PI {
        sweep -= std::f32::consts::TAU;
    }
    while sweep < -std::f32::consts::PI {
        sweep += std::f32::consts::TAU;
    }
    let pts = (0..=12).map(|i| a0 + sweep * i as f32 / 12.0).map(|t| c + vec2(t.cos(), t.sin()) * 22.0).collect();
    Some(Shape::line(pts, Stroke::new(1.0, pal().accent)))
}

/// What a preview draws on: the canvas, the model and its drawing, the level and the view.
struct Scene<'a> {
    p: &'a Painter,
    m: &'a Model,
    d: &'a Drawing,
    /// Where the drawing's own pixels go.
    place: Place,
    /// The level whose model the plan shows, and the plan's own key (the level's, or a plan view's): what annotation is in.
    level: Id,
    key: Id,
    v: &'a View,
}

impl Scene<'_> {
    fn screen(&self, pt: P2) -> Pos2 {
        pos(self.p.clip_rect().min, self.v.px(pt))
    }
}

/// Of the lines Align, Mirror and dimensions use on an element (a grid's, a wall's centre line,
/// faces and ends, a door's or a window's centre and jambs), the one nearest `p`.
fn nearest_line(m: &Model, id: Id, p: P2) -> Option<(P2, P2)> {
    m.reference_at(id, p, |_| true).and_then(|(r, _)| m.reference_line(&r).ok())
}

/// The same candidate for hover and a click: a model edge/axis, or, on the second pick,
/// a model pattern line when it is closer than the element's boundary.
fn alignment_line(m: &Model, key: Id, id: Id, at: P2, reach: f64, pattern: bool) -> Option<(Line, bool)> {
    let edge = bimcraft_core::align::nearest(m, id, at).filter(|(_, d)| *d <= reach);
    let fill = pattern.then(|| render::pattern_line_at(m, key, id, at, reach)).flatten();
    match (edge, fill) {
        (Some((_, distance)), Some(fill)) if geom::dist_to_segment(at, fill.0, fill.1) + 1e-6 < distance => Some((fill, true)),
        (Some((edge, _)), _) => Some((edge, false)),
        (None, Some(fill)) => Some((fill, true)),
        _ => None,
    }
}

/// While sketching, references can come from either scene; only the active sketch is editable.
/// Give a coincident draft line priority over the element whose boundary is being edited.
fn alignment_pick(
    m: &Model,
    key: Id,
    sketch: Option<&crate::sketch::Sketch>,
    reference: Option<Line>,
    hovered: Option<Id>,
    at: P2,
    reach: f64,
) -> Option<(draw::Picked, Line, bool)> {
    let draft = sketch.and_then(|sk| sk.alignment_at(at, reach));
    let model = hovered
        .filter(|_| sketch.is_none() || reference.is_none())
        .and_then(|id| alignment_line(m, key, id, at, reach, reference.is_some()).map(|(line, pattern)| (id, line, pattern)));
    match (draft, model) {
        (Some((_, _, d)), Some((id, other, pattern))) if geom::dist_to_segment(at, other.0, other.1) + 1e-6 < d => {
            Some((draw::Picked::Element(id), other, pattern))
        }
        (Some((i, line, _)), _) => Some((draw::Picked::Sketch(i), line, false)),
        (None, Some((id, line, pattern))) => Some((draw::Picked::Element(id), line, pattern)),
        _ => None,
    }
}

/// Degrees from the ray `centre`→`from` to the ray `centre`→`to`, counter-clockwise, in
/// (-180, 180]; each ray onto the nearest multiple of `inc` degrees when it's within a few
/// degrees of one.
fn angle_between(centre: P2, from: P2, to: P2, inc: f64) -> f64 {
    let (a, b) = (snap_angle(from - centre, inc), snap_angle(to - centre, inc));
    let d = (b.1.atan2(b.0) - a.1.atan2(a.0)).to_degrees();
    if d > 180.0 {
        d - 360.0
    } else if d <= -180.0 {
        d + 360.0
    } else {
        d
    }
}

/// A turn of `degrees` counter-clockwise in the model, about `c`, as the screen sees it (its y
/// runs down, so the turn runs the other way).
fn turned(c: Pos2, degrees: f64) -> impl Fn(Pos2) -> Pos2 {
    let (s, k) = (-degrees.to_radians()).sin_cos();
    let (s, k) = (s as f32, k as f32);
    move |q| {
        let d = q - c;
        c + vec2(d.x * k - d.y * s, d.x * s + d.y * k)
    }
}

/// The mirror across the screen line through `a` and `b`.
fn mirrored(a: Pos2, b: Pos2) -> impl Fn(Pos2) -> Pos2 {
    let u = (b - a).normalized();
    move |q| {
        let d = q - a;
        a + u * (2.0 * d.dot(u)) - d
    }
}

/// `p` to the nearest 10 mm.
fn round(p: P2) -> P2 {
    P2((p.0 / 10.0).round() * 10.0, (p.1 / 10.0).round() * 10.0)
}

fn point(p: P2) -> Value {
    json!([p.0, p.1])
}

/// `p` to the micron: what a point meant to be exact is, without float noise.
fn micron(p: P2) -> P2 {
    P2((p.0 * 1e3).round() / 1e3, (p.1 * 1e3).round() / 1e3)
}

/// `d`'s direction, turned onto the nearest multiple of `inc` degrees when it's within 3° of one
/// (less where they lie closer), as Revit's angle snaps do; `inc` outside 1 to 180 for none.
fn snap_angle(d: P2, inc: f64) -> P2 {
    let a = d.1.atan2(d.0);
    if !(1.0..=180.0).contains(&inc) {
        return P2(a.cos(), a.sin());
    }
    let step = inc.to_radians();
    let near = (a / step).round() * step;
    let a = if (a - near).abs() <= 3f64.to_radians().min(0.4 * step) { near } else { a };
    P2(a.cos(), a.sin())
}

/// `a` degrees turned into [0, 360).
fn norm_turn(a: f64) -> f64 {
    let a = a.rem_euclid(360.0);
    if a >= 360.0 { 0.0 } else { a }
}

/// An angle as a ruler reads it: to the hundredth of a degree.
fn degrees(a: f64) -> String {
    format!("{}°", (a * 100.0).round() / 100.0 + 0.0)
}

/// The point a tool picked first, while it has one.
fn start_of(tool: &Tool) -> &[P2] {
    match tool {
        Tool::Grid(start)
        | Tool::ReferencePlane(start)
        | Tool::RoomSeparator(start)
        | Tool::Move(start)
        | Tool::Copy(start)
        | Tool::Section(start)
        | Tool::Measure(start)
        | Tool::Run(_, start) => start.as_slice(),
        _ => &[],
    }
}

/// A selection box from pixel `from` to `q` of a view in `rect`: Revit draws a crossing box (right
/// to left) dashed, a window box solid.
pub(crate) fn box_shapes(rect: Rect, from: (f32, f32), q: (f32, f32)) -> Vec<Shape> {
    let r = Rect::from_two_pos(pos(rect.min, from), pos(rect.min, q));
    let fill = Shape::rect_filled(r, 0.0, pal().accent.gamma_multiply(0.08));
    if q.0 < from.0 {
        let ring = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        let mut out = vec![fill];
        out.extend(Shape::dashed_line(&ring, Stroke::new(1.0, pal().accent), 6.0, 4.0));
        out
    } else {
        vec![fill, Shape::rect_stroke(r, 0.0, Stroke::new(1.0, pal().accent), StrokeKind::Middle)]
    }
}

/// What the Dimension tool takes of element `id` at `p`, after `refs`: an end of one of its lines within `near` mm (a wall's
/// end or corner, a line's or a grid's end, a jamb's), as Archicad and Revit snap to them, else one of its lines parallel
/// to the first line picked.
fn dimension_pick(m: &Model, refs: &[Reference], id: Id, p: P2, near: f64) -> Option<Reference> {
    if let Some((r, _)) = m.endpoint_at(id, p).filter(|(_, d)| *d <= near) {
        return Some(r);
    }
    reference_for(m, DimKind::Aligned, refs.iter().find(|r| r.point.is_none()), id, p, near)
}

/// The references of element `id` a dimension after `refs` may take at `p`, nearest first, apart from those it has already:
/// its corners and ends, and its lines parallel to the first line picked. What Tab steps through as the pointer is over an
/// element, so the one a click would take is seen before it is.
fn dimension_references(m: &Model, refs: &[Reference], id: Id, p: P2, near: f64) -> Vec<Reference> {
    let first = refs.iter().find(|r| r.point.is_none()).and_then(|r| m.reference_line(r).ok());
    let mut out: Vec<(f64, Reference)> = Vec::new();
    for (r, q) in m.reference_points(id) {
        if refs.contains(&r) {
            continue;
        }
        let d = q.dist(p);
        if d <= near {
            out.push((d, r));
        }
    }
    for (r, l) in m.reference_lines(id) {
        if refs.contains(&r) {
            continue;
        }
        if let Some((a, b)) = first
            && !geom::parallel(a, b, l.0, l.1)
        {
            continue;
        }
        let d = geom::dist_to_segment(p, l.0, l.1);
        if d <= near {
            out.push((d, r));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.into_iter().map(|(_, r)| r).collect()
}

/// Of element `id`'s lines that a dimension of `kind` can take after `first` (for an aligned one,
/// parallel to it; for an angular one, across it), the nearest `p`, if it's within `near` mm: a
/// wall's far end isn't picked by a click on its middle.
fn reference_for(m: &Model, kind: DimKind, first: Option<&Reference>, id: Id, p: P2, near: f64) -> Option<Reference> {
    let first = first.and_then(|r| m.reference_line(r).ok());
    let keep = |l: Line| first.is_none_or(|(a, b)| geom::parallel(a, b, l.0, l.1) == (kind == DimKind::Aligned));
    m.reference_at(id, p, keep).filter(|(_, d)| *d <= near).map(|(r, _)| r)
}

/// The modifier keys held at the last primary-button release (else now): Ctrl adds to the
/// selection, Shift takes away.
pub(crate) fn click_modifiers(ui: &Ui) -> Modifiers {
    ui.input(|i| {
        i.events
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::PointerButton { button: PointerButton::Primary, pressed: false, modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or(i.modifiers)
    })
}

impl BimApp {
    /// Where `response` was clicked this frame: a press and release a few points apart, as egui has
    /// it. A press that travels further before the button comes up is no click to egui (a drag if
    /// the frames let it be, which Modify moves or boxes with); it is a click where it began for
    /// every other tool, since fast drawing clicks on the move and no tool has a use for a drag,
    /// and for Modify when the button came up quickly ([`QUICK`]).
    pub(crate) fn clicked_at(&self, response: &Response) -> Option<Pos2> {
        // A view drawn as it is takes no clicks: the first makes it the active view.
        if self.passive {
            return None;
        }
        if response.clicked() {
            return response.interact_pointer_pos();
        }
        self.canvas.lifted.filter(|(_, quick)| *quick || self.tool != Tool::Modify || self.sketch.is_some()).map(|(at, _)| at)
    }

    /// A drag has begun on `id`: it is selected, and the selection it replaces is kept for
    /// [`BimApp::finish_drag`].
    pub(crate) fn grab(&mut self, id: Id) {
        self.canvas.prior = None;
        if !self.selection.contains(&id) {
            self.canvas.prior = Some(std::mem::replace(&mut self.selection, vec![id]));
            self.raise_context();
        }
    }

    /// A drag has stopped: whether it is to be finished (moved, boxed). It isn't if the hand was
    /// clicking on the move, and then the selection the drag's start replaced comes back, for the
    /// click to pick from.
    pub(crate) fn finish_drag(&mut self) -> bool {
        let prior = self.canvas.prior.take();
        let quick = self.canvas.quick();
        if let (true, Some(prior)) = (quick, prior) {
            self.selection = prior;
        }
        !quick
    }

    /// Where a point picked at pixel `at` lands, with the points `from` already picked: on what
    /// the plan has there, a guide at an angle from the last of them, a wall's centre line, or the
    /// nearest 10 mm, as Manage > Snaps says. `square` keeps the step from the last horizontal or
    /// vertical.
    pub(crate) fn snap(&self, m: &Model, level: Id, v: &View, at: (f32, f32), from: &[P2], square: bool) -> Found {
        let reach = Reach { point: SNAP / v.scale, line: f64::from(PICK) / v.scale };
        let snaps = self.snaps_now();
        // A camera's mark is a symbol, not a set of snap points: anywhere on it lands on its eye.
        if snaps.endpoint
            && let Some((id, eye)) = render::camera_at(m, level, v, at)
        {
            return Found { at: eye, kind: Kind::Endpoint, guide: None, source: Some(id) };
        }
        let view = self.plan_key().unwrap_or(level);
        let found = match &self.sketch {
            Some(sk) => {
                let editing = match sk.of {
                    crate::sketch::Of::Edit(id) => Some(id),
                    _ => None,
                };
                snap::in_sketch(m, snap::Sketch { view, lines: &sk.lines, editing }, v.mm(at), reach, from, square, &snaps)
            }
            None => snap::snap(m, view, v.mm(at), reach, from, square, &snaps),
        };
        if matches!(found.kind, Kind::Free | Kind::Angle)
            && !square
            && self.snap_once.is_none()
            && self.grid_snapping
            && let Some(at) = self.background_grid().nearest(v.mm(at))
        {
            return Found { at, kind: Kind::Intersection, guide: None, source: None };
        }
        found
    }

    /// Finds alignment guides (smart snapping) to reference points of existing elements when drawing.
    pub(crate) fn alignments(
        &self,
        m: &Model,
        level: Id,
        v: &View,
        cursor: P2,
        start: Option<P2>,
    ) -> Vec<bimcraft_core::alignment::AlignmentResult> {
        let snaps = self.snaps_now();
        if !snaps.alignment_guides {
            return Vec::new();
        }
        let tol = f64::from(snaps.alignment_tolerance) / v.scale;
        let config = bimcraft_core::alignment::AlignmentConfig {
            enabled: true,
            tolerance: tol,
            endpoints: snaps.endpoint,
            midpoints: snaps.midpoint,
            faces: true,
            intersections: snaps.intersection,
            angles: false,
        };
        let mut cache = self.canvas.alignment_cache.borrow_mut();
        let at = std::ptr::from_ref(m) as usize;
        let reuse = cache.as_ref().is_some_and(|c| {
            c.level == level && c.next_id == m.next_id && c.len == m.elements.len() && c.at == at && c.service.config() == &config
        });
        if !reuse {
            let s = bimcraft_core::alignment::AlignmentGuideService::from_model(m, level, config);
            *cache = Some(AlignmentCache { level, next_id: m.next_id, len: m.elements.len(), at, service: s });
        }
        cache.as_ref().map_or_else(Vec::new, |c| c.service.find_alignments(cursor, start, tol))
    }

    /// Finds alignment guides (smart snapping) to reference points of existing walls when drawing a wall.
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn wall_alignments(
        &self,
        m: &Model,
        level: Id,
        v: &View,
        cursor: P2,
        start: Option<P2>,
    ) -> Vec<bimcraft_core::alignment::AlignmentResult> {
        self.alignments(m, level, v, cursor, start)
    }

    /// Renders visual alignment guide lines, reference markers, and alignment tags.
    fn show_alignments(&self, sc: &Scene, alignments: &[bimcraft_core::alignment::AlignmentResult]) {
        let magenta = Color32::from_rgb(255, 0, 255);
        let magenta_stroke = Stroke::new(1.0, magenta);
        let marker_stroke = Stroke::new(1.5, Color32::from_rgb(0, 160, 255));
        for res in alignments {
            let a = sc.screen(res.reference_point);
            let b = sc.screen(res.snapped_point);
            if a.distance(b) > 0.5 {
                let dir = (b - a).normalized();
                let ext_a = a - dir * 24.0;
                let ext_b = b + dir * 24.0;
                sc.p.extend(Shape::dashed_line(&[ext_a, ext_b], magenta_stroke, 6.0, 4.0));
            }
            sc.p.rect_stroke(Rect::from_center_size(a, vec2(7.0, 7.0)), 0.0, marker_stroke, StrokeKind::Middle);
        }
        if let Some(first) = alignments.first() {
            // Why it lined up: with which wall, by its id, as Select by ID finds it.
            let why = first.wall_id.map_or_else(|| t("align").to_string(), |w| tf("Lines up with wall {}", &[&w.to_string()]));
            tag(sc.p, sc.screen(first.snapped_point) + vec2(12.0, 14.0), Align2::LEFT_TOP, &why);
        }
    }

    /// The alignment guides a pick that landed at `f` takes: a free point any, a point on an angle guide one that keeps it on that
    /// guide (a wall drawn east whose end lines up with another wall's end).
    fn alignments_for(&self, m: &Model, level: Id, v: &View, f: &Found, from: &[P2]) -> Vec<bimcraft_core::alignment::AlignmentResult> {
        if !self.snaps_now().alignment_guides || from.is_empty() || !matches!(f.kind, Kind::Free | Kind::Angle) {
            return Vec::new();
        }
        let on_guide = |p: P2| {
            f.guide.is_none_or(|g| {
                let (u, d) = (P2(g.angle.to_radians().cos(), g.angle.to_radians().sin()), p - g.from);
                (u.0 * d.1 - u.1 * d.0).abs() <= 0.5
            })
        };
        self.alignments(m, level, v, f.at, from.last().copied()).into_iter().filter(|a| on_guide(a.snapped_point)).collect()
    }

    /// Where a point picked at pixel `at` lands, including temporary alignment guides.
    pub(crate) fn snap_point(&self, m: &Model, level: Id, v: &View, at: (f32, f32), from: &[P2], square: bool) -> P2 {
        let f = self.snap(m, level, v, at, from, square);
        self.alignments_for(m, level, v, &f, from).first().map_or(f.at, |a| a.snapped_point)
    }

    /// What a pick at pixel `q` would land on, shown: its marker and what it is, and the guide it
    /// follows. The point.
    /// Where Add Point or Add Split Line lands at `q` on the selected floor or roof: a point or corner of its shape within
    /// reach, else the nearest point on an edge within reach (`shape_snap_plan`), else where the ordinary snaps put it.
    pub(crate) fn shape_landing(&self, m: &Model, level: Id, v: &View, q: (f32, f32), from: &[P2], square: bool) -> (P2, Option<(P2, P2)>) {
        let shaped = self.floor_selected().and_then(|(_, f)| shape_snap_plan(m, &f, v.mm(q), f64::from(PICK) / v.scale));
        shaped.map_or_else(|| (self.snap(m, level, v, q, from, square).at, None), |(p, edge, _)| (p, edge))
    }

    /// [`shape_landing`](Self::shape_landing) as a tool's preview shows it: the edge it snaps to lit, and its marker.
    fn shape_found(&self, sc: &Scene, q: (f32, f32), from: &[P2], square: bool) -> P2 {
        let shaped = self.floor_selected().and_then(|(_, f)| shape_snap_plan(sc.m, &f, sc.v.mm(q), f64::from(PICK) / sc.v.scale));
        let Some((p, edge, kind)) = shaped else { return self.found(sc, q, from, square) };
        if let Some((a, b)) = edge {
            sc.p.line_segment([sc.screen(a), sc.screen(b)], Stroke::new(3.0, pal().accent));
        }
        crate::shape::snap_marker(sc.p, sc.screen(p), kind);
        p
    }

    fn found(&self, sc: &Scene, q: (f32, f32), from: &[P2], square: bool) -> P2 {
        let mut f = self.snap(sc.m, sc.level, sc.v, q, from, square);
        let alignments = self.alignments_for(sc.m, sc.level, sc.v, &f, from);
        if let Some(first) = alignments.first() {
            f.at = first.snapped_point;
            self.show_alignments(sc, &alignments);
        }
        self.show_snap(sc, &f);
        self.canvas.found.set(Some(f));
        f.at
    }

    /// A snap's guide, dashed through the point it runs from and on past the snapped one; its
    /// marker, by what it snapped to; and its name.
    fn show_snap(&self, sc: &Scene, f: &Found) {
        // And of which element, by its id, as Select by ID finds it.
        let name = match f.source.and_then(|id| Some((id, sc.m.elements.get(id)?))) {
            Some((id, e)) => {
                let of = crate::lang::category(e.category(), false);
                // TRANSLATORS: what a point snapped to, then the category and id of its element: "Endpoint of Wall 100231".
                tf("{} of {} {}", &[t(f.kind.name()), &of, &id.to_string()])
            }
            None => t(f.kind.name()).to_string(),
        };
        snap_mark(sc.p, sc.screen(f.at), f.guide.map(|g| sc.screen(g.from)), f.kind, name);
    }

    /// The plan `key` names, filling `ui`: a level's own, or a plan view of one. What is built is on the plan's level; what is
    /// dimensioned, tagged and noted is in the plan itself.
    pub(crate) fn canvas(&mut self, ui: &mut Ui, key: Id) {
        // Clicks and drags; never the keyboard, which the shortcuts need.
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::CLICK | Sense::DRAG);
        let model = self.view_model();
        let level = model.plan_level(key).unwrap_or(key);
        let up = matches!(self.sheet, Sheet::Ceiling(_));
        let fit = |w, h| if up { render::ceiling_plan(&model, key, w, h).view } else { render::plan(&model, key, w, h).view };
        let Some((view, local)) = self.canvas.navigate(ui, &response, rect, key, fit) else { return };
        // Where the pointer is, for the others in a Co-Work session.
        self.cowork_seen.pointer = local.map(|q| (key, view.mm(q)));
        let mods = ui.input(|i| i.modifiers);
        let shape = self.shape();
        let pipeline = self.card.canvas();
        let look = crate::retained::Look::now(ui.ctx(), &ui.painter_at(rect), rect.min, self.writing(), self.thin);
        let c = &mut self.canvas;
        c.cursor = local.map(|q| view.mm(q));
        let d = match &pipeline {
            Some(pipeline) => c.drawing.moving(ui.ctx(), &model, key, view, up, pipeline, look),
            None => c.drawing.drawing(&model, key, view, up),
        };
        let d = Shown::new(d, view, rect.min);

        // What's under the cursor; Tab steps through it. (Without the app's input hook, as in
        // tests, egui moved the keyboard to a widget on that Tab: give it back.)
        let sketching = self.sketch.is_some();
        let under = match (&self.tool, local) {
            (Tool::Modify, Some(q)) if !sketching => d.pick_all(q, PICK),
            _ => Vec::new(),
        };
        // A dimension under the cursor: Tab steps through its segments before its element.
        let dim_under = under.iter().copied().find(|id| {
            matches!(model.elements.get(*id), Some(Element::Dimension(d))
                if matches!(d.kind, DimKind::Aligned | DimKind::Linear) && !d.alignment)
        });
        if under != c.under {
            (c.under, c.cycle) = (under, 0);
        }
        let tab = ui.input(|i| i.key_pressed(Key::Tab)) && local.is_some();
        if tab {
            ui.ctx().memory_mut(|m| {
                if let Some(f) = m.focused() {
                    m.surrender_focus(f);
                }
            });
        }
        let tabbed = std::mem::take(&mut self.tab_cycle) || tab;
        // A dimension being picked or edited: Tab steps through the references of the element under the cursor, so the one a
        // click takes is seen first: its corners and ends, and its faces and centre lines.
        let reference_refs: Option<Vec<Reference>> = match (&self.dimension_refs_edit, &self.tool) {
            (Some(edit), _) => Some(edit.refs.clone()),
            (None, Tool::Dimension(_, refs)) => Some(refs.clone()),
            _ => None,
        };
        let element_under =
            local.and_then(|q| d.pick_all(q, PICK).into_iter().find(|id| !matches!(model.elements.get(*id), Some(Element::Dimension(_)))));
        let reference_context: Option<(Id, Vec<Reference>)> = match (local, reference_refs, element_under) {
            (Some(q), Some(refs), Some(id)) => {
                let candidates = dimension_references(&model, &refs, id, view.mm(q), 2.0 * f64::from(PICK) / view.scale);
                (!candidates.is_empty()).then_some((id, candidates))
            }
            _ => None,
        };
        if self
            .dimension_reference_hover
            .as_ref()
            .is_some_and(|(id, r)| !reference_context.as_ref().is_some_and(|(held, cands)| held == id && cands.contains(r)))
        {
            self.dimension_reference_hover = None;
        }
        let mut reference_tabbed = false;
        if tabbed && let Some((id, candidates)) = &reference_context {
            let next = match &self.dimension_reference_hover {
                Some((held, r)) if held == id => {
                    let i = candidates.iter().position(|x| x == r).unwrap_or(0);
                    candidates[(i + 1) % candidates.len()]
                }
                _ => candidates[0],
            };
            self.dimension_reference_hover = Some((*id, next));
            reference_tabbed = true;
        }
        if self.dimension_segment_hover.is_some_and(|(id, _)| Some(id) != dim_under) {
            self.dimension_segment_hover = None;
        }
        let mut segment_tabbed = false;
        if !reference_tabbed
            && tabbed
            && let Some(id) = dim_under
            && let Some(Element::Dimension(d)) = model.elements.get(id)
        {
            let (_, st) = model.stations(d);
            let count = st.len().saturating_sub(1);
            if count > 0 {
                let at = local.map(|q| model.dimension_segment_at(d, view.mm(q))).unwrap_or(0).min(count - 1);
                let next = match self.dimension_segment_hover {
                    Some((held, i)) if held == id => (i + 1) % count,
                    _ => at,
                };
                self.dimension_segment_hover = Some((id, next));
                segment_tabbed = true;
            }
        }
        if tabbed && !reference_tabbed && !segment_tabbed && !c.under.is_empty() {
            c.cycle = (c.cycle + 1) % c.under.len();
        }
        // While a rectangle is drawn, Tab moves between its width and its height; a line's or a wall's, its length and its angle.
        if tabbed && (self.typed_fields() == 2 || self.line_angle_field()) {
            std::mem::swap(&mut self.typed_length, &mut self.drawing.other);
            self.drawing.field = 1 - self.drawing.field.min(1);
        }
        let c = &mut self.canvas;
        // How near the cursor a dimension's reference line must be, mm.
        let near = 2.0 * f64::from(PICK) / view.scale;
        // What the tool in hand would take at pixel `q`: under the cursor for the highlight, under
        // the press for a click, which the hand may have left by the time the button came up.
        // (Modify, here, is what is drawn nearest; Tab cycles through what is under the cursor.)
        let tool = self.tool.clone();
        let under_at = |q: (f32, f32)| match &tool {
            Tool::Modify if sketching => None,
            Tool::Modify => d.pick(q, PICK),
            Tool::SketchPick => d.pick(q, PICK).filter(|id| model.wall(*id).is_some()),
            // A line to draw along, or to round.
            Tool::DetailLine(_) | Tool::Wall(_) | Tool::SketchLine(_) if shape == draw::Shape::PickLines => {
                d.pick_all(q, PICK).into_iter().find(|id| draw::element_curve(&model, *id).is_some())
            }
            Tool::Wall(_) if shape == draw::Shape::Fillet => {
                d.pick_all(q, PICK).into_iter().find(|id| model.wall(*id).is_some_and(|w| w.arc == 0.0))
            }
            Tool::DetailLine(_) if shape == draw::Shape::Fillet => {
                d.pick_all(q, PICK).into_iter().find(|id| matches!(model.elements.get(id), Some(Element::DetailLine(_))))
            }
            Tool::Split => {
                d.pick_all(q, PICK).into_iter().find(|id| matches!(model.elements.get(id), Some(Element::Wall(_) | Element::DetailLine(_))))
            }
            Tool::Trim(_) | Tool::Offset => {
                d.pick_all(q, PICK).into_iter().find(|id| matches!(model.elements.get(id), Some(Element::Wall(_) | Element::DetailLine(_))))
            }
            Tool::Vertex { .. } => {
                d.pick_all(q, PICK).into_iter().find(|id| matches!(model.elements.get(id), Some(Element::DetailLine(_))))
            }
            Tool::TrimTo { boundary: None, .. } => d.pick_all(q, PICK).into_iter().find(|id| draw::element_curve(&model, *id).is_some()),
            Tool::TrimTo { .. } => {
                d.pick_all(q, PICK).into_iter().find(|id| matches!(model.elements.get(id), Some(Element::DetailLine(_))))
            }
            Tool::Door | Tool::Window => d.pick(q, PICK).filter(|id| model.wall(*id).is_some()),
            Tool::Demolish | Tool::Linework => d.pick(q, PICK),
            // Search all candidates: a non-alignable annotation must not hide a model edge.
            Tool::Align(Some(_)) if sketching => None,
            Tool::Align(reference) => d.pick_all(q, PICK).into_iter().find(|id| {
                (reference.is_none() || !bimcraft_core::link::is_linked(*id))
                    && alignment_line(&model, key, *id, view.mm(q), near, reference.is_some()).is_some()
            }),
            Tool::Mirror { draw: false, .. } => d.pick(q, PICK).filter(|id| !model.reference_lines(*id).is_empty()),
            // Something to measure to: with a line parallel to the first one picked, or for an angle,
            // one across it.
            Tool::Dimension(_, refs) => {
                d.pick_all(q, PICK).into_iter().find(|id| dimension_pick(&model, refs, *id, view.mm(q), near).is_some())
            }
            Tool::Angular(refs) if refs.len() < 2 => d
                .pick_all(q, PICK)
                .into_iter()
                .find(|id| reference_for(&model, DimKind::Angular, refs.first(), *id, view.mm(q), near).is_some()),
            Tool::Round(kind, picked) if picked.is_empty() => {
                d.pick_all(q, PICK).into_iter().find(|id| bimcraft_core::dims::round_at(&model, *id, view.mm(q), *kind).is_some())
            }
            // An arc length's points, on its arc.
            Tool::Round(DimKind::ArcLength, picked) => picked.first().and_then(|arc| {
                d.pick_all(q, PICK).into_iter().find(|id| bimcraft_core::dims::arc_point(&model, arc, *id, view.mm(q), near).is_some())
            }),
            // What an elevation would look at: a straight wall, a line, a slab's edge, the project's or a link's.
            Tool::Elevation => {
                d.pick_all(q, PICK).into_iter().find(|id| bimcraft_core::elevation::reference(&model, *id, view.mm(q)).is_some())
            }
            // What a spot elevation would read.
            Tool::Spot => model.spot_host(level, view.mm(q)),
            // The floor or roof a spot slope would read.
            Tool::SpotSlope => model
                .spot_host(level, view.mm(q))
                .filter(|id| matches!(model.elements.get(*id), Some(Element::Floor(_) | Element::Roof(_)))),
            Tool::Tag => d.pick(q, PICK).filter(|id| model.anchor(*id).is_some()),
            Tool::Attach => {
                d.pick_all(q, PICK).into_iter().find(|id| matches!(model.elements.get(id), Some(Element::Roof(_) | Element::Floor(_))))
            }
            Tool::Dormer => model.roof_at(level, view.mm(q)),
            Tool::GableEdge => model.roof_edge(level, view.mm(q), f64::from(PICK) / view.scale).map(|(id, _)| id),
            Tool::Railing(path) if path.is_empty() => {
                d.pick_all(q, PICK).into_iter().find(|id| matches!(model.elements.get(id), Some(Element::Stair(_) | Element::Ramp(_))))
            }
            _ => None,
        };
        let hovered = match (&tool, local) {
            (Tool::Modify, Some(_)) => c.under.get(c.cycle).copied(),
            (_, Some(q)) => under_at(q),
            _ => None,
        };

        // While a note is written here, the pointer is its editor's.
        let writing = self.writing_in(key);
        let crop = self.crop_grips(ui, &response, rect, &view, key);
        let dragged = self.sketch_drag(ui, &response, rect, &view, level);
        self.canvas.found.set(None);
        self.grip_drag(ui, &response, rect, &view, level);
        self.line_grip_drag(ui, &response, rect, &view, level);
        self.adaptive_grip_drag(ui, &response, rect, &view, level);
        self.view_grip_drag(ui, &response, rect, &view);
        let dragged = self.shape_grip_drag(ui, &response, rect, &view, level) || dragged;
        // A drag from an element moves the selection; from empty floor, it draws a selection box.
        if response.drag_started_by(PointerButton::Primary)
            && self.tool == Tool::Modify
            && !sketching
            && !writing
            && self.canvas.crop_drag.is_none()
            && self.canvas.grip.is_none()
            && self.canvas.view_grip.is_none()
            && self.drawing.grip.is_none()
            && self.adaptive_grip.is_none()
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
        {
            let o = (o.x - rect.min.x, o.y - rect.min.y);
            self.canvas.prior = None;
            self.canvas.drag = Some(match d.pick(o, PICK) {
                Some(id) => {
                    self.grab(id);
                    Drag::Move(view.mm(o))
                }
                None => Drag::Box(o),
            });
        }
        let at = ui.input(|i| i.pointer.latest_pos()).map(|p| (p.x - rect.min.x, p.y - rect.min.y));
        if response.drag_stopped()
            && let (Some(drag), Some(q)) = (self.canvas.drag.take(), at)
            && self.finish_drag()
        {
            match drag {
                Drag::Move(from) => {
                    let by = self.dragged(&model, &view, from, view.mm(q));
                    if by.length() > 0.0 {
                        // An elevation taken alone is taken by its mark, which goes apart from its line.
                        match self.selection.as_slice() {
                            &[id] if matches!(model.elements.get(id), Some(Element::Elevation(_))) => {
                                self.run("move", json!({"ids": [id], "by": point(by), "end": "mark"}))
                            }
                            _ => self.run("move", json!({"ids": self.selection, "by": point(by)})),
                        };
                    }
                }
                Drag::Box(from) => self.pick_up(d.in_box(from, q, q.0 < from.0), click_modifiers(ui)),
            }
        }
        // A double click on a section's or an elevation's mark opens its view, as in Revit; on a note, writes in it.
        if response.double_clicked()
            && let Some(id) = hovered.filter(|id| model.section(*id).is_some())
        {
            self.open_sheet(Sheet::Section(id));
            return;
        }
        // A double click on a camera's mark opens its 3D view.
        if response.double_clicked()
            && let Some(id) = hovered.filter(|id| matches!(model.elements.get(*id), Some(Element::View3D(_))))
        {
            self.open_3d(Some(id));
            return;
        }
        if response.double_clicked()
            && !writing
            && let Some(id) = hovered.filter(|id| crate::notes::is_note(&model, *id))
        {
            self.edit_note(id);
            return;
        }
        // On a dimension, its text: the number under the cursor's, as Revit's Dimension Text.
        if response.double_clicked()
            && !writing
            && let (Some(q), Some(Element::Dimension(dim))) = (local, hovered.and_then(|id| model.elements.get(id)))
            && let Some(id) = hovered
        {
            let segment = model.dimension_segment_at(dim, view.mm(q));
            self.dimension_text_dialog = Some(crate::dimtypes::DimensionTextDialog::new(&model, id, segment));
            return;
        }
        // A click on a shape point's height label is the label's.
        let on_label = |at: &Pos2| self.shape_label_rects.iter().any(|r| r.contains(*at));
        if let Some(at) = self.clicked_at(&response).filter(|at| !dragged && !writing && !on_label(at)) {
            let q = (at.x - rect.min.x, at.y - rect.min.y);
            // What lay under the press, which the hand may have left by the time the button came up.
            let found = if Some(q) == local { hovered } else { under_at(q) };
            // A dimension's control keeps a click over it (its number, EQ, padlock or text button), but a mark that
            // opens a view or a note, or is written in, is the click's: a double click there must not be held back.
            let opens = found.is_some_and(|id| {
                model.section(id).is_some()
                    || matches!(model.elements.get(id), Some(Element::View3D(_)))
                    || crate::notes::is_note(&model, id)
            });
            if self.over_dimension_control && !opens {
                self.snap_once = None;
            } else {
                // Tab has picked a segment of the dimension under the cursor: a click selects that segment.
                if let Some((dim, segment)) = self.dimension_segment_hover
                    && (found == Some(dim) || d.pick_all(q, PICK).contains(&dim))
                {
                    self.selection = vec![dim];
                    self.dimension_segment = Some((dim, segment));
                    self.dimension_segment_hover = None;
                } else {
                    self.dimension_segment = None;
                    self.dimension_segment_hover = None;
                    self.click(&model, (level, key), &view, q, found, click_modifiers(ui));
                }
                // A snap override typed was for that point.
                self.snap_once = None;
            }
        }
        // A double click ends a spline or a polyline at the point clicked, as Enter does.
        if response.double_clicked() && !dragged && !writing && self.finishable() {
            self.finish_open_shape();
        }

        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, color(render::PAPER));
        self.paint_background_grid(&p, rect, view);
        // The floor or roof being shaped is drawn as it is, not in the selection's colour: edit mode's marks are what's lit.
        let shaped = self.shaping();
        let mut marked: Vec<Id> = self.selection.iter().copied().filter(|id| Some(*id) != shaped).collect();
        if let Tool::Trim(Some((first, _))) | Tool::TrimTo { boundary: Some(first), .. } = self.tool {
            marked.push(first);
        }
        let pictures = self.canvas.drawing.pictures(ui.ctx(), &d.drawing);
        // Fillet Arc's lines picked so far, in the selection's colour.
        marked.extend(self.drawing.fillet.iter().filter_map(|(l, _)| match l {
            draw::Picked::Element(id) => Some(*id),
            draw::Picked::Sketch(_) => None,
        }));
        let sketch_trim = self.sketch.is_some() && matches!(self.tool, Tool::Trim(_) | Tool::TrimTo { .. });
        let painted_hover = if matches!(self.tool, Tool::Align(_)) || sketch_trim { None } else { hovered };
        let place = Place { hide: self.writing(), ..d.place };
        let kept = pipeline.as_ref().is_some_and(|pipeline| {
            self.canvas.drawing.retained.paint(ui.ctx(), pipeline, &p, place, rect.min, &d.drawing, &marked, painted_hover, self.thin)
        });
        if !kept {
            paint(&p, place, &d, &marked, painted_hover, self.thin, &pictures);
        }
        let kind = if up { "ceiling" } else { "plan" };
        crate::cowork::paint_plan(&p, &self.cowork, &self.cowork_seen, &model, (kind, key), rect, &view, (&d.drawing, d.place));
        // The working aids: where the project's 0, 0 is, and which view this is. On the screen only; no drawing is made of them.
        if self.show_origin {
            let (x, y) = view.px(P2(0.0, 0.0));
            origin_marker(&p, pos(rect.min, (x, y)));
        }
        if self.show_view_name {
            let name = model.view_name(key).unwrap_or_default();
            let name = if up { format!("{name} ({})", t("Ceiling Plan")) } else { name.to_string() };
            self.corner_name(ui.ctx(), &p, rect, &name);
        }
        if writing {
            self.note_canvas(ui, &response, &p, d.place, &d);
        }
        if let Some(sk) = &self.sketch {
            let over = local
                .filter(|_| matches!(self.tool, Tool::Modify | Tool::Trim(_) | Tool::TrimTo { .. }) && sk.drag.is_none())
                .and_then(|q| sk.line_at(view.mm(q), f64::from(PICK) / view.scale));
            let from: &[P2] = match &self.tool {
                Tool::SketchLine(chain) if self.shape() == draw::Shape::Line || chain.len() < 2 => chain,
                _ => &[],
            };
            let shift = ui.input(|i| i.modifiers.shift);
            // The snapped cursor is for a grip being dragged and for what a sketch tool shows; Modify's idle frames need none.
            let wanted = sk.drag.is_some() || matches!(self.tool, Tool::SketchLine(_));
            let cursor = local.filter(|_| wanted).map(|q| self.snap(&model, level, &view, q, from, shift).at);
            sk.paint(&p, |q| pos(rect.min, view.px(q)), over, cursor, &self.sketch_lit());
            if let (Some(q), Some(c)) = (local, cursor) {
                self.sketch_preview(&p, &view, rect.min, q, c);
            }
        }
        p.extend(crop);
        let lit: Vec<Id> = hovered.iter().chain(&self.selection).copied().collect();
        let outlines = self.canvas.drawing.room_outlines(&model, &lit);
        let hover_lit = usize::from(hovered.is_some());
        for (i, id) in lit.iter().enumerate() {
            // An unbounded room: the square it sits as, dashed.
            if let Some((outline, bounded)) = outlines.get(id) {
                let accent = if i < hover_lit { canvas_pal().hover } else { canvas_pal().accent };
                let mut ring: Vec<Pos2> = outline.iter().map(|q| pos(rect.min, view.px(*q))).collect();
                if *bounded {
                    p.add(Shape::closed_line(ring, Stroke::new(2.0, accent)));
                } else {
                    ring.extend(ring.first().copied());
                    p.extend(Shape::dashed_line(&ring, Stroke::new(2.0, accent), 6.0, 4.0));
                }
            }
        }
        match (self.canvas.drag, local) {
            (Some(Drag::Move(from)), Some(q)) => {
                let by = self.dragged(&model, &view, from, view.mm(q));
                let (a, b) = (view.px(from), view.px(from + by));
                // An elevation dragged alone: its mark as it will stand, turned round past the line it looks at.
                let turned = match self.selection.as_slice() {
                    &[id] => match model.elements.get(id) {
                        Some(Element::Elevation(e)) => bimcraft_core::elevation::dragged(e, true, bimcraft_core::elevation::Part::Mark, by)
                            .ok()
                            .map(Element::Elevation),
                        _ => None,
                    },
                    _ => None,
                };
                match turned {
                    Some(e) => {
                        let id = model.next_id;
                        ghost(&p, Place::at(p.clip_rect().min), &render::sketch(&model, view, id, &e), &[id], &|pt| pt);
                    }
                    None => ghost(&p, d.place, &d, &self.selection, &|q| q + vec2(b.0 - a.0, b.1 - a.1)),
                }
            }
            (Some(Drag::Box(from)), Some(q)) => p.extend(box_shapes(rect, from, q)),
            _ => {}
        }
        if let Some(q) = local {
            let scene = Scene { p: &p, m: &model, d: &d, place: d.place, level, key, v: &view };
            self.preview(&scene, q, hovered, mods.shift);
        }
        // The Measure tool's ruler, as laid.
        if let (Tool::Measure(_), Some((a, b))) = (&self.tool, self.measured) {
            let scene = Scene { p: &p, m: &model, d: &d, place: d.place, level, key, v: &view };
            p.extend(ruler_shapes(&scene, a, b));
            let r = snap::ruler(a, b);
            let units = model.units;
            let text = format!(
                "{}  {}  dx {}  dy {}",
                units.with_unit(r.length),
                degrees(r.angle),
                units.with_unit(r.dx.abs()),
                units.with_unit(r.dy.abs())
            );
            let (pa, pb) = (scene.screen(a), scene.screen(b));
            let n = (pb - pa).normalized().rot90();
            tag(&p, pa + (pb - pa) / 2.0 + n * 20.0, Align2::CENTER_CENTER, text);
            for q in [pa, pb] {
                p.rect_stroke(Rect::from_center_size(q, vec2(8.0, 8.0)), 0.0, Stroke::new(2.0, pal().accent), StrokeKind::Middle);
            }
        }
        self.grip_overlay(ui, &model, &view, rect, level, &d, d.place);
        self.adaptive_overlay(ui, rect, &view);
        self.view_extent_overlay(ui, &model, &view, rect);
        // Where the cursor snaps is worked out only while a grip is held to it.
        let cursor = self
            .drawing
            .grip
            .and_then(|_| ui.input(|i| i.pointer.latest_pos()))
            .map(|q| self.snap(&model, level, &view, (q.x - rect.min.x, q.y - rect.min.y), &[], false).at);
        self.line_grip_overlay(&ui.painter_at(rect), rect, &view, cursor);
        self.shape_overlay(ui, &model, &view, rect, level);
        self.temporary_dimension(ui, &model, &view, rect);
        self.dimension_overlays(ui, &model, &view, rect, key);
        self.locks(ui, &model, &view, rect, key);
        self.context_menu(&response);
        self.nav_bar(ui, rect, key);
    }

    /// A selection box in a section or an elevation, where nothing moves by dragging (in a plan, a
    /// drag from an element moves it): any drag with Modify but a crop region's grip's; on release,
    /// what `inside` finds between its corners (from the left, what's wholly inside; from the right,
    /// what it crosses too) is picked up. What to paint while it's dragged.
    pub(crate) fn drag_box(
        &mut self,
        ui: &Ui,
        response: &Response,
        rect: Rect,
        inside: impl Fn((f32, f32), (f32, f32), bool) -> Vec<Id>,
    ) -> Vec<Shape> {
        let local = |p: Pos2| (p.x - rect.min.x, p.y - rect.min.y);
        if response.drag_started_by(PointerButton::Primary)
            && self.tool == Tool::Modify
            && self.canvas.crop_drag.is_none()
            && self.canvas.level_grip.is_none()
            && let Some(o) = ui.input(|i| i.pointer.press_origin()).map(local)
        {
            self.canvas.prior = None;
            self.canvas.drag = Some(Drag::Box(o));
        }
        let at = ui.input(|i| i.pointer.latest_pos()).map(local);
        if response.drag_stopped()
            && let (Some(Drag::Box(from)), Some(q)) = (self.canvas.drag.take(), at)
            && self.finish_drag()
        {
            self.pick_up(inside(from, q, q.0 < from.0), click_modifiers(ui));
        }
        match (self.canvas.drag, at) {
            (Some(Drag::Box(from)), Some(q)) => box_shapes(rect, from, q),
            _ => Vec::new(),
        }
    }

    /// The navigation bar of view `id`, at its top right as in Revit: fit, zoom in, zoom out.
    pub(crate) fn nav_bar(&mut self, ui: &mut Ui, rect: Rect, id: Id) {
        let bar = Rect::from_min_size(self.nav_corner(rect), vec2(32.0, 80.0));
        ui.painter_at(rect).rect(bar, 6.0, pal().surface, Stroke::new(1.0, pal().border), StrokeKind::Inside);
        ui.scope_builder(UiBuilder::new().max_rect(bar.shrink(3.0)).layout(Layout::top_down(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if icon_button(ui, Icon::Fit, "Zoom to Fit").clicked() {
                self.canvas.fit(id);
            }
            if icon_button(ui, Icon::ZoomIn, "Zoom In").clicked() {
                self.canvas.zoom(id, 1.25);
            }
            if icon_button(ui, Icon::ZoomOut, "Zoom Out").clicked() {
                self.canvas.zoom(id, 0.8);
            }
        });
    }

    /// View `id`'s crop region over it, in `rect` through `view`: its outline, and with Modify, a
    /// grip at the middle of each edge. A drag from a grip moves that edge; on release, `crop`.
    /// What to paint over the view once it's drawn.
    pub(crate) fn crop_grips(&mut self, ui: &Ui, response: &Response, rect: Rect, view: &View, id: Id) -> Vec<Shape> {
        let Some(region) = render::crop_region(&self.session.model, id) else { return Vec::new() };
        let at = |p: P2| pos(rect.min, view.px(p));
        let modify = self.tool == Tool::Modify;
        if modify
            && response.drag_started_by(PointerButton::Primary)
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
            && let Some(k) = geom::edge_grips(region).iter().position(|g| at(*g).distance(o) <= 8.0)
        {
            self.canvas.crop_drag = Some((id, k));
        }
        let held = self.canvas.crop_drag.filter(|(v, _)| *v == id).map(|(_, k)| k);
        let now = ui.input(|i| i.pointer.latest_pos()).map(|q| view.mm((q.x - rect.min.x, q.y - rect.min.y)));
        let shown = match (held, now) {
            (Some(k), Some(p)) => geom::move_edge(region, k, round(p)),
            _ => region,
        };
        if response.drag_stopped()
            && let Some(k) = held
        {
            self.canvas.crop_drag = None;
            let moved = !self.canvas.quick()
                && shown != region
                && self.run("crop", json!({"view": id, "region": [point(shown[0]), point(shown[1])]})).is_some();
            // A section's start went along its line: what's on screen stays where it was.
            if moved && k == 0 && self.session.model.section(id).is_some() {
                self.canvas.slide(id, -shown[0].0);
            }
        }
        let [lo, hi] = shown;
        let ring = [lo, P2(hi.0, lo.1), hi, P2(lo.0, hi.1)].map(at).to_vec();
        let mut shapes = vec![Shape::closed_line(ring, Stroke::new(1.0, if held.is_some() { pal().accent } else { canvas_pal().muted }))];
        if modify {
            for g in geom::edge_grips(shown) {
                let r = Rect::from_center_size(at(g), vec2(9.0, 9.0));
                shapes.push(Shape::rect_filled(r, 1.0, pal().surface));
                shapes.push(Shape::rect_stroke(r, 1.0, Stroke::new(1.5, pal().accent), StrokeKind::Middle));
            }
        }
        shapes
    }

    /// The level selected alone, with Modify, in section or elevation `id` in `rect` through `view`:
    /// a grip at each end of its line. A drag from one moves that end along the line, and its extent
    /// with it (`Model::level_extent_along`); on release, `modify`. What to paint over the view.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn level_grips(
        &mut self,
        ui: &Ui,
        response: &Response,
        rect: Rect,
        view: &View,
        id: Id,
        ends: Option<(f64, f64)>,
    ) -> Vec<Shape> {
        let m = std::sync::Arc::clone(&self.session.model);
        let level = match self.selection.as_slice() {
            &[l] if self.tool == Tool::Modify => m.level(l).map(|lv| (l, lv.elevation)),
            _ => None,
        };
        let Some(((level, z), (a, b))) = level.and_then(|(l, z)| Some(((l, z), ends?))) else {
            self.canvas.level_grip = None;
            return Vec::new();
        };
        let at = |along: f64| pos(rect.min, view.px(P2(along, z)));
        if response.drag_started_by(PointerButton::Primary)
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
            && let Some(far) = [(false, a), (true, b)].into_iter().find(|(_, e)| at(*e).distance(o) <= 8.0).map(|(far, _)| far)
        {
            self.canvas.level_grip = Some((level, far));
        }
        let held = self.canvas.level_grip.filter(|(l, _)| *l == level).map(|(_, far)| far);
        let now = ui.input(|i| i.pointer.latest_pos()).map(|q| round(view.mm((q.x - rect.min.x, q.y - rect.min.y))).0);
        let ends = match (held, now) {
            (Some(true), Some(to)) => (a, to.max(a + 100.0)),
            (Some(false), Some(to)) => (to.min(b - 100.0), b),
            _ => (a, b),
        };
        if response.drag_stopped()
            && let (Some(far), Some(to)) = (held, now)
        {
            self.canvas.level_grip = None;
            if !self.canvas.quick()
                && let Some([lo, hi]) = m.level_extent_along(level, id, far, to)
            {
                self.run("modify", json!({"id": level, "extent": [point(lo), point(hi)]}));
            }
        }
        let mut shapes = Vec::new();
        if held.is_some() {
            shapes.push(Shape::line_segment([at(ends.0), at(ends.1)], Stroke::new(1.5, pal().accent)));
        }
        for e in [ends.0, ends.1] {
            let r = Rect::from_center_size(at(e), vec2(9.0, 9.0));
            shapes.push(Shape::rect_filled(r, 1.0, pal().surface));
            shapes.push(Shape::rect_stroke(r, 1.0, Stroke::new(1.5, pal().accent), StrokeKind::Middle));
        }
        shapes
    }

    /// Add `found` to the selection with Ctrl, take it away with Shift, else select just it.
    pub(crate) fn pick_up(&mut self, found: Vec<Id>, mods: Modifiers) {
        if mods.shift {
            self.selection.retain(|id| !found.contains(id));
        } else if mods.command {
            for id in found {
                if !self.selection.contains(&id) {
                    self.selection.push(id);
                }
            }
        } else {
            self.selection = found;
        }
        if !self.selection.is_empty() {
            self.raise_context();
        }
    }

    /// The parameters a click puts down for `cat`, with the type (and a door's or window's family) picked for the tool.
    pub(crate) fn typed(&self, cat: &str, mut p: Value) -> Value {
        if matches!(cat, "door" | "window") {
            // Of a family, by default the project's usual door or window of one; else one of the project's own types.
            let (family, ty) = self.opening_pick(cat);
            if let Some(f) = family {
                p["family"] = json!(f);
            }
            if let Some(t) = ty {
                p["type"] = json!(t);
            }
        } else if let Some(t) = self.types.get(cat) {
            p["type"] = json!(t);
        }
        p
    }

    /// The opening both the ghost and the next click place. Measure facing from the physical centre
    /// of the resolved host, even for an arc or a wall drawn on a face rather than its centre line.
    fn opening_params(&self, m: &Model, v: &View, level: Id, at: (f32, f32), door: bool) -> Value {
        let cursor = v.mm(at);
        let mut params = self.typed(if door { "door" } else { "window" }, json!({"at": point(round(cursor)), "level": level}));
        if let Ok(Element::Door(o) | Element::Window(o)) = opening_of(m, &params, door) {
            if let Some(middle) = m.centred(&o, SNAP / v.scale) {
                params["offset"] = json!(middle);
            }
            if door
                && m.single_swing_door(o.family.as_deref(), &o.type_name)
                && let Some(w) = m.wall(o.host)
            {
                params["flip_hand"] = json!(self.door_flip_hand);
                params["flip_facing"] = json!(Axis::of(w).project(cursor).1 < walls::centre(m, w));
            }
        }
        params
    }

    /// How far dragging the selection from `from` to `to` moves it: a lone door or window slides to the middle of its
    /// stretch of wall when it comes within `SNAP` px of it.
    fn dragged(&self, m: &Model, v: &View, from: P2, to: P2) -> P2 {
        let by = round(to - from);
        let &[id] = self.selection.as_slice() else { return by };
        // A dimension's line moves only square to itself, as far as the drag goes that way: along it, it stays put.
        if let Some(Element::Dimension(d)) = m.elements.get(id) {
            let (n, _) = m.crossings(d);
            let off = n.perp();
            return if off.length() > 1e-9 { off * by.dot(off) } else { P2(0.0, 0.0) };
        }
        let Some(Element::Door(o) | Element::Window(o)) = m.elements.get(id) else { return by };
        let Some(w) = m.wall(o.host).filter(|w| w.arc == 0.0) else { return by };
        let u = (w.end - w.start).unit();
        let moved = bimcraft_core::model::Opening { offset: o.offset + by.dot(u), ..o.clone() };
        m.centred(&moved, SNAP / v.scale).map_or(by, |middle| by + u * (middle - moved.offset))
    }

    fn click(&mut self, m: &Model, (level, key): (Id, Id), v: &View, at: (f32, f32), hovered: Option<Id>, mods: Modifiers) {
        // Edit Dimension References: picks take references out or put them in, a click away fixes the line.
        if self.dimension_refs_edit.is_some() {
            self.dimension_refs_click(m, key, v, at, hovered);
            return;
        }
        match &self.tool {
            &Tool::Vertex { delete } if self.sketch.is_some() => self.vertex_click(None, v.mm(at), f64::from(PICK) / v.scale, delete),
            Tool::Split | Tool::Trim(_) | Tool::TrimTo { .. } | Tool::Offset if self.sketch.is_some() => {
                let p = self.snap(m, level, v, at, &[], false).at;
                self.sketch_modify(p, v.mm(at), f64::from(PICK) / v.scale);
            }
            Tool::Modify | Tool::SketchLine(_) | Tool::SketchPick if self.sketch.is_some() => {
                let from = match &self.tool {
                    Tool::SketchLine(chain) if self.shape() == draw::Shape::Line => chain.clone(),
                    Tool::SketchLine(chain) if chain.len() < 2 => chain.clone(),
                    _ => Vec::new(),
                };
                let p = self.snap(m, level, v, at, &from, mods.shift).at;
                self.sketch_click(m, p, v.mm(at), f64::from(PICK) / v.scale, hovered, mods);
            }
            // A sketch tool with the sketch gone: nothing to draw into.
            Tool::SketchLine(_) | Tool::SketchPick => self.tool = Tool::Modify,
            Tool::Modify => self.pick_up(hovered.into_iter().collect(), mods),
            Tool::Door | Tool::Window => {
                let door = matches!(self.tool, Tool::Door);
                let cat = if door { "door" } else { "window" };
                let p = self.opening_params(m, v, level, at, door);
                self.run(cat, p);
            }
            Tool::Room => {
                let p = self.typed("room", json!({"point": point(round(v.mm(at))), "level": level}));
                self.run("room", p);
            }
            Tool::Level => {}
            Tool::Elevation => match hovered {
                Some(id) => {
                    self.run("elevation", json!({"on": id, "at": point(round(v.mm(at)))}));
                }
                None => {
                    self.status = Status {
                        text: t("Click a straight wall, a line or a slab's edge for the elevation to look at.").into(),
                        error: false,
                    }
                }
            },
            Tool::Wall(_) if self.shape() == draw::Shape::Fillet => {
                let picked = hovered.filter(|id| m.wall(*id).is_some());
                self.fillet_pick(picked.map(draw::Picked::Element), v.mm(at));
            }
            Tool::Wall(_) if self.shape() == draw::Shape::PickLines => {
                if let Some(c) = hovered.and_then(|id| self.picked_curve(id, v.mm(at))) {
                    self.make_walls(level, &[c]);
                }
            }
            Tool::Wall(_) if self.shape() == draw::Shape::PickFaces => {
                match bimcraft_core::massing::face_at(m, v.mm(at), 2.0 * f64::from(PICK) / v.scale) {
                    Some((mass, face)) => {
                        let mut p = json!({"mass": mass, "faces": [face]});
                        if let Some(t) = self.types.get("wall") {
                            p["type"] = json!(t);
                        }
                        self.run("wall_by_face", p);
                    }
                    None => self.status = Status { text: t("Pick Faces: click a face of a mass").into(), error: true },
                }
            }
            Tool::Wall(_)
            | Tool::Grid(_)
            | Tool::ReferencePlane(_)
            | Tool::RoomSeparator(_)
            | Tool::Move(_)
            | Tool::Copy(_)
            | Tool::Section(_)
            | Tool::Camera(_)
            | Tool::Run(..) => {
                let chain: Vec<P2> = match &self.tool {
                    Tool::Wall(chain) => chain.clone(),
                    Tool::Grid(start)
                    | Tool::ReferencePlane(start)
                    | Tool::RoomSeparator(start)
                    | Tool::Move(start)
                    | Tool::Copy(start)
                    | Tool::Section(start)
                    | Tool::Camera(start)
                    | Tool::Run(_, start) => start.iter().copied().collect(),
                    _ => Vec::new(),
                };
                // A shape's points past its second follow no guide from the points before.
                let chain = match (&self.tool, self.shape()) {
                    (Tool::Wall(_), draw::Shape::Line) => chain,
                    (Tool::Wall(_), _) if chain.len() >= 2 => Vec::new(),
                    (Tool::Wall(_), _) => chain.last().copied().into_iter().collect(),
                    _ => chain,
                };
                let square = mods.shift || (self.constrain && matches!(self.tool, Tool::Move(_) | Tool::Copy(_)));
                let pt = self.snap_point(m, level, v, at, &chain, square);
                self.place(level, pt);
            }
            &Tool::Rotate { centre, start, place } => {
                self.tool = match (place, start) {
                    (true, _) => Tool::Rotate { centre: self.snap_point(m, level, v, at, &[], false), start: None, place: false },
                    (false, None) => Tool::Rotate { centre, start: Some(v.mm(at)), place: false },
                    (false, Some(from)) => return self.turn(centre, angle_between(centre, from, v.mm(at), self.snaps.angle)),
                };
            }
            // A ruler: its start, then its end, which stays on show until Esc or another start.
            &Tool::Measure(first) => {
                let p = self.snap_point(m, level, v, at, first.as_slice(), mods.shift);
                match first {
                    None => {
                        self.measured = None;
                        self.tool = Tool::Measure(Some(p));
                    }
                    Some(a) if a.dist(p) > TOL => {
                        self.measured = Some((a, p));
                        self.tool = Tool::Measure(None);
                    }
                    Some(_) => {}
                }
            }
            Tool::Mirror { draw: false, .. } => {
                if let Some(axis) = hovered.and_then(|id| nearest_line(m, id, v.mm(at))) {
                    self.reflect(axis);
                }
            }
            &Tool::Mirror { draw: true, start } => {
                let p = self.snap_point(m, level, v, at, start.as_slice(), mods.shift);
                match start {
                    None => self.tool = Tool::Mirror { draw: true, start: Some(p) },
                    Some(a) => self.reflect((a, p)),
                }
            }
            &Tool::Align(reference) => match (
                reference,
                alignment_pick(m, key, self.sketch.as_ref(), reference, hovered, v.mm(at), 2.0 * f64::from(PICK) / v.scale),
            ) {
                (None, Some((picked, line, _))) => {
                    self.drawing.align_reference = Some(picked);
                    self.drawing.align_lock = None;
                    self.tool = Tool::Align(Some(line));
                }
                (Some(r), Some((draw::Picked::Sketch(i), line, _))) => self.align_sketch(r, i, line),
                (Some(r), Some((draw::Picked::Element(id), line, true))) => self.align_pattern(m, r, id, line),
                (Some(r), Some((draw::Picked::Element(id), line, false))) => self.align(r, id, line),
                _ => {}
            },
            Tool::Trim(_) | Tool::TrimTo { .. } => self.trim_click(hovered, v.mm(at)),
            Tool::Split => {
                if let Some(id) = hovered {
                    self.run("split", json!({"id": id, "at": point(self.snap(m, level, v, at, &[], false).at)}));
                }
            }
            Tool::Offset => {
                if let Some(id) = hovered.filter(|id| matches!(m.elements.get(*id), Some(Element::DetailLine(_)))) {
                    match m.units.parse(&self.offset).filter(|d| *d > 0.0) {
                        Some(d) => {
                            self.run("offset", json!({"id": id, "distance": d, "toward": point(v.mm(at)), "copy": self.offset_copy}));
                        }
                        None => self.status = Status { text: tf("offset: `{}` is not a distance", &[&self.offset]), error: true },
                    }
                } else if let Some(id) = hovered
                    && let Some(by) = self.offset_by(m, id, v.mm(at))
                {
                    self.run(if self.offset_copy { "copy" } else { "move" }, json!({"ids": [id], "by": point(by)}));
                }
            }
            &Tool::Vertex { delete } => {
                let p = self.snap(m, level, v, at, &[], false).at;
                self.vertex_click(hovered, if delete { v.mm(at) } else { p }, f64::from(PICK) / v.scale, delete);
            }
            Tool::Demolish => {
                if let Some(id) = hovered {
                    self.demolish(id);
                }
            }
            Tool::Linework => {
                if let Some(id) = hovered {
                    let mut p = json!({"view": key, "id": id, "at": point(v.mm(at))});
                    match self.linework_style.as_str() {
                        crate::linestyles::INVISIBLE => p["invisible"] = json!(true),
                        crate::linestyles::BY_CATEGORY => p["remove"] = json!(true),
                        style => p["style"] = json!(style),
                    }
                    self.run("linework", p);
                }
            }
            // Paint works in the 3D view, which it opens; a plan's tool goes back to Modify.
            Tool::Paint | Tool::Unpaint => {}
            // An endpoint or a reference under the cursor joins the dimension; a click away from them places it, laid as
            // its mode says.
            Tool::Dimension(mode, refs) => {
                let (mode, mut refs) = (*mode, refs.clone());
                let picked = self
                    .dimension_reference_hover
                    .filter(|(id, _)| Some(*id) == hovered)
                    .map(|(_, r)| r)
                    .or_else(|| hovered.and_then(|id| dimension_pick(m, &refs, id, v.mm(at), 2.0 * f64::from(PICK) / v.scale)));
                self.dimension_reference_hover = None;
                match picked {
                    Some(r) => {
                        // A reference picked again is taken out, as Revit's dimension tool does.
                        if let Some(i) = refs.iter().position(|x| *x == r) {
                            refs.remove(i);
                        } else {
                            refs.push(r);
                        }
                        self.tool = Tool::Dimension(mode, refs);
                    }
                    None if refs.len() >= 2 => {
                        let at = round(v.mm(at));
                        let (kind, orient) = mode.kind(m, &refs, at);
                        let mut p = json!({"kind": kind, "refs": refs, "at": point(at), "level": key});
                        if let Some(o) = orient {
                            p["orient"] = json!(o);
                        }
                        if let Some(t) = self.dimension_type_for(kind) {
                            p["type"] = json!(t);
                        }
                        if self.run("dimension", p).is_some() {
                            self.tool = Tool::Dimension(mode, Vec::new());
                        }
                    }
                    None => {}
                }
            }
            // Two lines that meet, then where the arc goes.
            Tool::Angular(refs) => {
                let mut refs = refs.clone();
                match hovered.and_then(|id| reference_for(m, DimKind::Angular, refs.first(), id, v.mm(at), 2.0 * f64::from(PICK) / v.scale))
                {
                    Some(r) if refs.len() < 2 && !refs.contains(&r) => {
                        refs.push(r);
                        self.tool = Tool::Angular(refs);
                    }
                    _ if refs.len() == 2 => {
                        let p = json!({"kind": "angular", "refs": refs, "at": point(round(v.mm(at))), "level": key});
                        if self.run("dimension", p).is_some() {
                            self.tool = Tool::Angular(Vec::new());
                        }
                    }
                    _ => {}
                }
            }
            // What is round, then (an arc length's) two points on its arc if it measures part of it, then where it goes.
            Tool::Round(kind, picked) => {
                let (kind, mut picked) = (*kind, picked.clone());
                let near = 2.0 * f64::from(PICK) / v.scale;
                let on_arc = match (kind, picked.first()) {
                    (DimKind::ArcLength, Some(arc)) => hovered.and_then(|id| bimcraft_core::dims::arc_point(m, arc, id, v.mm(at), near)),
                    _ => None,
                };
                match (picked.len(), on_arc) {
                    (0, _) => {
                        if let Some(r) = hovered.and_then(|id| bimcraft_core::dims::round_at(m, id, v.mm(at), kind)) {
                            self.tool = Tool::Round(kind, vec![r]);
                        }
                    }
                    (1 | 2, Some(r)) if !picked.contains(&r) => {
                        picked.push(r);
                        self.tool = Tool::Round(kind, picked);
                    }
                    (2, _) => {}
                    _ => {
                        let mut p = json!({"kind": kind, "refs": picked, "at": point(round(v.mm(at))), "level": key});
                        if let Some(t) = self.dimension_type_for(kind) {
                            p["type"] = json!(t);
                        }
                        if self.run("dimension", p).is_some() {
                            self.tool = Tool::Round(kind, Vec::new());
                        }
                    }
                }
            }
            Tool::Text => self.write_note(key, round(v.mm(at))),
            Tool::DetailGroup => self.place_group(key, self.snap(m, level, v, at, &[], false).at),
            &Tool::RepeatingDetail(start) => {
                let p = self.snap(m, level, v, at, start.as_slice(), mods.shift).at;
                match start {
                    None => self.tool = Tool::RepeatingDetail(Some(p)),
                    Some(a) => {
                        if self.repeat(key, a, p) {
                            self.tool = Tool::RepeatingDetail(None);
                        }
                    }
                }
            }
            Tool::Spot => {
                self.run("spot", json!({"at": point(round(v.mm(at))), "view": key}));
            }
            Tool::SpotSlope => {
                self.run("spot_slope", json!({"at": point(round(v.mm(at))), "view": key}));
            }
            Tool::Tag => {
                if let Some(id) = hovered {
                    let label = self.tag_label.trim();
                    self.run("tag", json!({"ids": [id], "leader": self.tag_leader, "label": (!label.is_empty()).then_some(label)}));
                }
            }
            // The selected walls, onto the roof or floor picked; then Modify, the walls still selected.
            Tool::Attach => {
                if let Some(id) = hovered {
                    self.attach_to(id);
                }
            }
            Tool::Component | Tool::Fixture(_) => {
                if self.adaptive_count().is_some() {
                    self.adaptive_click(m, level, v, at);
                } else if let Some((family, ty)) = self.placing() {
                    let at = self.snap_point(m, level, v, at, &[], false);
                    let z = m.elevation(level);
                    self.place_component(
                        json!({"family": family, "type": ty, "at": point(at), "level": level, "angle": self.component_angle}),
                        z,
                    );
                }
            }
            &Tool::Stand(category) => {
                let at = self.snap_point(m, level, v, at, &[], false);
                self.place_stand(level, at, category);
            }
            Tool::Column => {
                let at = self.snap_point(m, level, v, at, &[], false);
                self.place_column(level, at, 0.0);
            }
            // Its start, where its first run ends, and which way the next turns: done when it climbs all
            // the way.
            Tool::Stair(pts) | Tool::Ramp(pts) => {
                let ramp = matches!(self.tool, Tool::Ramp(_));
                let mut pts = pts.clone();
                let p = self.snap_point(m, level, v, at, &pts, mods.shift);
                if pts.last().is_some_and(|l| l.dist(p) <= TOL) {
                    return;
                }
                pts.push(p);
                let next = match (pts.len(), self.flight(m, level, &pts, ramp)) {
                    (1, _) | (_, Ok((_, false))) => pts,
                    (_, Ok((e, true))) => {
                        let mut params = serde_json::to_value(&e).unwrap_or_default();
                        params["railings"] = json!(self.railings);
                        self.run(e.category(), params);
                        Vec::new()
                    }
                    (_, Err(e)) => {
                        self.status = Status { text: e, error: true };
                        pts.truncate(1);
                        pts
                    }
                };
                self.tool = if ramp { Tool::Ramp(next) } else { Tool::Stair(next) };
            }
            // A stair or a ramp picked: one down each side; else the path's next point.
            Tool::Railing(path) => match (path.is_empty(), hovered) {
                (true, Some(host)) => {
                    let p = self.typed("railing", json!({"host": host}));
                    self.run("railing", p);
                }
                _ => {
                    let p = self.snap_point(m, level, v, at, path, mods.shift);
                    if let Tool::Railing(path) = &mut self.tool
                        && path.last().is_none_or(|l| l.dist(p) > TOL)
                    {
                        path.push(p);
                    }
                }
            },
            // The footprint's next corner; back on the first, it's done.
            Tool::Mass(path) => {
                let p = self.snap_point(m, level, v, at, path, mods.shift);
                if path.len() >= 3 && path.first().is_some_and(|f| f.dist(p) <= TOL) {
                    self.finish_mass(level);
                } else if let Tool::Mass(path) = &mut self.tool
                    && path.last().is_none_or(|l| l.dist(p) > TOL)
                {
                    path.push(p);
                }
            }
            Tool::DetailLine(path) => {
                let guide: Vec<P2> = match self.shape() {
                    draw::Shape::Line | draw::Shape::Polyline => path.clone(),
                    _ if path.len() >= 2 => Vec::new(),
                    _ => path.last().copied().into_iter().collect(),
                };
                let p = self.snap(m, level, v, at, &guide, mods.shift).at;
                match self.shape() {
                    draw::Shape::Fillet => {
                        let picked = hovered.filter(|id| matches!(m.elements.get(*id), Some(Element::DetailLine(_))));
                        self.fillet_pick(picked.map(draw::Picked::Element), v.mm(at));
                    }
                    draw::Shape::PickLines => {
                        if let Some(c) = hovered.and_then(|id| self.picked_curve(id, v.mm(at))) {
                            self.make_detail_lines(level, &[c]);
                        }
                    }
                    _ => self.place_detail_line(level, p),
                }
            }
            // On the level above, over this plan's walls; once made, selected, as a finished sketch is.
            Tool::Roof => match (number(&self.roof_slope), m.units.parse(&self.roof_overhang)) {
                (Some(slope), Some(overhang)) => {
                    let on = m.level_above(level).unwrap_or(m.guid(level));
                    let p = self.typed("roof", json!({"level": on, "slope": slope, "overhang": overhang, "gable": self.roof_gable}));
                    if let Some(made) = self.run("roof", p) {
                        self.selection = made["id"].as_u64().into_iter().collect();
                        self.tool = Tool::Modify;
                    }
                }
                _ => {
                    let text = tf("roof: the slope `{}` and the overhang `{}` must be numbers", &[&self.roof_slope, &self.roof_overhang]);
                    self.status = Status { text, error: true };
                }
            },
            // Its line's start and end, then how far across it's swept; once made, selected.
            Tool::ExtrusionRoof(pts) => {
                let mut pts = pts.clone();
                let p = self.snap_point(m, level, v, at, &pts, mods.shift);
                if pts.last().is_some_and(|l| l.dist(p) <= TOL) {
                    return;
                }
                pts.push(p);
                if pts.len() < 3 {
                    self.tool = Tool::ExtrusionRoof(pts);
                    return;
                }
                match self.extrusion_roof(m, level, &pts) {
                    Ok(r) => {
                        let p = self.typed("roof", json!({"level": r.level, "extrusion": r.extrusion}));
                        if let Some(made) = self.run("roof", p) {
                            self.selection = made["id"].as_u64().into_iter().collect();
                            self.tool = Tool::Modify;
                        }
                    }
                    Err(e) => {
                        self.status = Status { text: e, error: true };
                        self.tool = Tool::ExtrusionRoof(Vec::new());
                    }
                }
            }
            // On the roof's slope under the cursor, as the Options Bar says.
            Tool::Dormer => {
                match (hovered, m.units.parse(&self.dormer_width), m.units.parse(&self.dormer_height), number(&self.roof_slope)) {
                    (None, ..) => {}
                    (Some(host), Some(width), Some(height), Some(slope)) => {
                        let at = point(round(v.mm(at)));
                        let mut p = self.typed("wall", json!({"host": host, "at": at, "width": width, "height": height, "slope": slope}));
                        // The window type picked last, else the smallest: a dormer's front is short.
                        if self.dormer_window {
                            let smallest =
                                m.types.window.iter().min_by(|a, b| (a.1.width * a.1.height).total_cmp(&(b.1.width * b.1.height)));
                            p["window"] = json!(self.types.get("window").cloned().or_else(|| smallest.map(|(n, _)| n.clone())));
                        }
                        self.run("dormer", p);
                    }
                    _ => {
                        let what = [self.dormer_width.as_str(), &self.dormer_height, &self.roof_slope];
                        self.status = Status {
                            text: tf("dormer: the width `{}`, the height `{}` and the slope `{}` must be numbers", &what),
                            error: true,
                        };
                    }
                }
            }
            // The roof edge picked stands upright, or slopes again.
            Tool::GableEdge => {
                if let Some((id, i)) = m.roof_edge(level, v.mm(at), f64::from(PICK) / v.scale)
                    && let Some(Element::Roof(r)) = m.elements.get(id)
                {
                    match m.roof_uprights(r) {
                        Ok(mut up) => {
                            if let Some(u) = up.get_mut(i) {
                                *u = !*u;
                            }
                            let gables: Vec<usize> = up.iter().enumerate().filter(|(_, u)| **u).map(|(i, _)| i).collect();
                            self.run("modify", json!({"id": id, "gables": gables, "gable": false}));
                        }
                        Err(e) => self.status = Status { text: e, error: true },
                    }
                }
            }
            // Under the walls of this plan's level; once made, selected, as a finished sketch is.
            Tool::Floor => {
                let p = self.typed("floor", json!({"level": level, "structural": self.floor_structural}));
                if let Some(made) = self.run("floor", p) {
                    self.selection = made["id"].as_u64().into_iter().collect();
                    self.tool = Tool::Modify;
                }
            }
            Tool::Ceiling => match m.units.parse(&self.ceiling_height) {
                Some(offset) => {
                    let p = self.typed("ceiling", json!({"point": point(round(v.mm(at))), "level": level, "offset": offset}));
                    self.run("ceiling", p);
                }
                None => self.status = Status { text: tf("ceiling: `{}` is not a height", &[&self.ceiling_height]), error: true },
            },
            // A custom fill pattern's tile: two opposite corners, the pattern made of the lines picked on the second.
            &Tool::PatternCorner(first) => {
                let p = self.snap(m, level, v, at, first.as_slice(), false).at;
                match first {
                    None => self.tool = Tool::PatternCorner(Some(p)),
                    Some(a) => self.pattern_corners(a, p),
                }
            }
            // Two opposite corners of a rectangle.
            &Tool::Shaft(start) => {
                let p = self.snap(m, level, v, at, start.as_slice(), false).at;
                match start {
                    None => self.tool = Tool::Shaft(Some(p)),
                    Some(a) if (a.0 - p.0).abs() > TOL && (a.1 - p.1).abs() > TOL => {
                        match top_params(m, level, self.shaft_top, &self.shaft_height) {
                            Ok(mut q) => {
                                q["boundary"] = json!([point(a), point(P2(p.0, a.1)), point(p), point(P2(a.0, p.1))]);
                                q["level"] = json!(level);
                                if self.run("shaft", q).is_some() {
                                    self.tool = Tool::Shaft(None);
                                }
                            }
                            Err(e) => self.status = Status { text: tf("shaft: {}", &[&e]), error: true },
                        }
                    }
                    Some(_) => {}
                }
            }
            // The selected floor's slope arrow, from its tail to its head; then Modify, the floor
            // still selected.
            &Tool::SlopeArrow(tail) => {
                let p = self.snap(m, level, v, at, tail.as_slice(), mods.shift).at;
                match (tail, self.selected_floor(), number(&self.floor_slope)) {
                    (None, ..) => self.tool = Tool::SlopeArrow(Some(p)),
                    (Some(a), Some(floor), Some(slope)) if a.dist(p) > TOL => {
                        if self.run("modify", json!({"id": floor, "arrow": [point(a), point(p)], "slope": slope})).is_some() {
                            self.tool = Tool::Modify;
                        }
                    }
                    (Some(_), _, None) => {
                        self.status = Status { text: tf("slope arrow: `{}` is not a slope", &[&self.floor_slope]), error: true };
                    }
                    _ => {}
                }
            }
            // A point on the selected floor's top: at the Options Bar's elevation, or the top's height there; the tool stays
            // for the next one.
            Tool::AddPoint => {
                let (p, _) = self.shape_landing(m, level, v, at, &[], mods.shift);
                self.shape_add_point(p);
            }
            // A point of the selected floor's shape or a corner picked, or else an edge (a split line's, the outline's or an
            // opening's): selected, or with Shift or Ctrl added to the selection; the Options Bar's Elevation then reads it.
            Tool::ModifySubElements => {
                let (q, reach) = (v.mm(at), 2.0 * f64::from(PICK) / v.scale);
                self.shape_select(|f| bimcraft_core::view3d::shape_pick_plan(m, f, q, reach), mods.shift || mods.command);
            }
            // A split line across the selected floor's top, from one click to the next; then ready for another.
            &Tool::AddSplitLine(start) => {
                let (p, _) = self.shape_landing(m, level, v, at, start.as_slice(), mods.shift);
                self.shape_split(p);
            }
        }
    }

    /// The roof by extrusion the points clicked make, on the level above: its profile along the line
    /// from the first to the second, rising from it at the Options Bar's slope to a ridge over its
    /// middle (or a shed's, to its end), swept across to the third.
    pub(crate) fn extrusion_roof(&self, m: &Model, level: Id, pts: &[P2]) -> Result<Roof, String> {
        let (Some(&a), Some(&b), Some(&c)) = (pts.first(), pts.get(1), pts.get(2)) else { return Err(t("click its line's end").into()) };
        let slope = number(&self.roof_slope).filter(|s| *s > 0.0).ok_or_else(|| tf("roof: `{}` is not a slope", &[&self.roof_slope]))?;
        let (len, tan) = (a.dist(b), slope.to_radians().tan());
        let profile = if self.roof_shed {
            vec![P2(0.0, 0.0), P2(len, len * tan)]
        } else {
            vec![P2(0.0, 0.0), P2(len / 2.0, len / 2.0 * tan), P2(len, 0.0)]
        };
        let across = (c - a).dot((b - a).unit().perp());
        let type_name = self.types.get("roof").cloned().or_else(|| m.default_type("roof").map(String::from)).unwrap_or_default();
        Ok(Roof {
            type_name,
            level: m.level_above(level).unwrap_or(m.guid(level)),
            extrusion: Some(Extrusion { line: [a, b], profile, from: across.min(0.0), to: across.max(0.0) }),
            ..Roof::default()
        })
    }

    /// The stair or the ramp the points clicked make (its start, where its first run ends, and a
    /// point the next run goes toward from the landing), as the Options Bar sets it; and whether it's
    /// done: its first run climbs all the way, or its next one has turned.
    pub(crate) fn flight(&self, m: &Model, level: Id, pts: &[P2], ramp: bool) -> Result<(Element, bool), String> {
        let (start, end) = match pts {
            [a, b, ..] => (*a, *b),
            _ => return Err(t("click where its run ends").into()),
        };
        let (u, run) = ((end - start).unit(), start.dist(end));
        let angle = (u.1.atan2(u.0).to_degrees() * 1e9).round() / 1e9;
        // Ahead of the landing, beside it, or back behind it, to its left or right.
        let turn = pts.get(2).map_or(0.0, |&p| {
            let d = p - end;
            let a = d.dot(u.perp()).atan2(d.dot(u)).to_degrees();
            if a.abs() < 45.0 {
                0.0
            } else if a.abs() < 135.0 {
                90f64.copysign(a)
            } else {
                180f64.copysign(a)
            }
        });
        let done = |landings: &[Landing]| landings.is_empty() || pts.len() > 2;
        if ramp {
            let slope = number(&self.ramp_slope).ok_or_else(|| tf("ramp: `{}` is not a slope", &[&self.ramp_slope]))?;
            let width = m.units.parse(&self.ramp_width).ok_or_else(|| tf("ramp: `{}` is not a width", &[&self.ramp_width]))?;
            let top = top_params(m, level, self.ramp_top, &self.ramp_height).map_err(|e| tf("ramp: {}", &[&e]))?;
            let (top_level, height) = (top["top_level"].as_u64().map(|t| m.guid(t)), top["height"].as_f64().unwrap_or(0.0));
            let (bottom, top) = m.span(level, 0.0, top_level, height);
            let up = (run * slope / 100.0 * 1000.0).round() / 1000.0;
            let landings = if top - bottom - up > 1.0 { vec![Landing { at: up, turn }] } else { Vec::new() };
            let done = done(&landings);
            Ok((
                Element::Ramp(Ramp {
                    params: Default::default(),
                    level: m.guid(level),
                    top_level,
                    height,
                    start,
                    angle,
                    width,
                    slope,
                    landings,
                }),
                done,
            ))
        } else {
            let width = m.units.parse(&self.stair_width).ok_or_else(|| tf("stair: `{}` is not a width", &[&self.stair_width]))?;
            let mut st = Stair {
                params: Default::default(),
                level: m.guid(level),
                top_level: m.level_above(level),
                height: DEFAULT_HEIGHT,
                start,
                angle,
                width,
                riser: RISER,
                tread: TREAD,
                landings: Vec::new(),
            };
            let (n, _, _) = m.flight(&st);
            // As many risers as the run clicked holds, a tread between each two.
            let first = ((run / TREAD).round() + 1.0).max(2.0);
            if f64::from(n) - first >= 2.0 {
                st.landings.push(Landing { at: first, turn });
            }
            let done = done(&st.landings);
            Ok((Element::Stair(st), done))
        }
    }

    /// The Railing tool's Enter: a railing along the path clicked, on the plan's level.
    pub(crate) fn finish_railing(&mut self, level: Id) {
        let Tool::Railing(path) = &mut self.tool else { return };
        let path = std::mem::take(path);
        if path.len() >= 2 {
            let mut p = json!({"path": path.iter().map(|q| point(*q)).collect::<Vec<_>>(), "level": level});
            if let Some(t) = self.types.get("railing") {
                p["type"] = json!(t);
            }
            self.run("railing", p);
        }
    }

    /// The Detail Line tool's next point. A line a click, as walls are: each click after the first makes a line from the
    /// one before, and with Chain goes on from there until back on the first point. With Polyline, the points gather into
    /// one line, closed when back on the first, else finished with Enter.
    fn place_detail_line(&mut self, level: Id, p: P2) {
        if !matches!(self.shape(), draw::Shape::Line | draw::Shape::Polyline) {
            if let Some(made) = self.shape_click(p) {
                self.make_detail_lines(level, &made);
            }
            return;
        }
        let polyline = self.one_line();
        let Tool::DetailLine(path) = &mut self.tool else { return };
        let (Some(first), Some(last)) = (path.first().copied(), path.last().copied()) else {
            path.push(p);
            return;
        };
        if last.dist(p) <= TOL {
            return;
        }
        if polyline {
            if path.len() >= 2 && first.dist(p) <= TOL {
                self.finish_detail_line_closed(level, true);
            } else {
                path.push(p);
            }
            return;
        }
        if self.options_refused() {
            return;
        }
        // Beside the points clicked, by the Options Bar's Offset.
        let line = draw::offset_line(last, p, self.draw_options().offset);
        let mut params = json!({"start": point(line[0]), "end": point(line[1]), "view": level});
        self.line_extras(&mut params);
        if self.run("detail_line", params).is_some() {
            let Tool::DetailLine(path) = &mut self.tool else { return };
            if !self.line_chain || first.dist(p) <= TOL {
                path.clear();
            } else {
                path.push(p);
            }
        }
    }

    /// The Detail Line tool's Enter: with Polyline, a detail line through the points clicked, in the plan of `level`;
    /// else the chain ends, its lines made as it went.
    pub(crate) fn finish_detail_line(&mut self, level: Id) {
        if self.one_line() {
            self.finish_detail_line_closed(level, false);
        } else if self.shape().open_ended() {
            if let Some(made) = self.finish_shape() {
                self.make_detail_lines(level, &made);
            }
        } else if let Tool::DetailLine(path) = &mut self.tool {
            path.clear();
        }
    }

    /// A detail line through the points clicked, round to the first again where `closed`, in the Options Bar's line
    /// style; fewer than two points make none.
    fn finish_detail_line_closed(&mut self, level: Id, closed: bool) {
        let Tool::DetailLine(path) = &mut self.tool else { return };
        let path = std::mem::take(path);
        if path.len() < 2 {
            return;
        }
        let mut p = json!({"points": path.iter().map(|q| point(*q)).collect::<Vec<_>>(), "view": level, "closed": closed});
        self.line_extras(&mut p);
        self.run("detail_line", p);
    }

    /// The Mass tool's Enter, or its first corner clicked again: a mass over the footprint clicked, on the plan's level,
    /// up to the Options Bar's top. Fewer than three corners make none.
    pub(crate) fn finish_mass(&mut self, level: Id) {
        let Tool::Mass(path) = &mut self.tool else { return };
        let path = std::mem::take(path);
        if path.len() < 3 {
            return;
        }
        match top_params(&self.session.model, level, self.mass_top, &self.mass_height) {
            Ok(mut q) => {
                q["boundary"] = json!(path.iter().map(|p| point(*p)).collect::<Vec<_>>());
                q["level"] = json!(level);
                self.run("mass", q);
            }
            Err(e) => self.status = Status { text: tf("mass: {}", &[&e]), error: true },
        }
    }

    /// The Text tool's click: a note saying what the Options Bar says, its top left at `at` in
    /// view `view` (a plan's level, a section, an elevation or a sheet).
    pub(crate) fn write_note(&mut self, view: Id, at: P2) {
        // With Leader, the first click is the point the leader reaches, the next where the note goes.
        let leader = match self.note_arrow.take() {
            Some((v, arrow)) if self.note_leader && v == view => Some(arrow),
            _ if self.note_leader => {
                self.note_arrow = Some((view, at));
                self.status = Status { text: t("Click where the note goes: its top left corner.").into(), error: false };
                return;
            }
            _ => None,
        };
        // Typed in the Options Bar, it goes straight there; else it is written on the canvas.
        if self.note_text.trim().is_empty() {
            self.start_note(view, at);
            if let Some(e) = &mut self.note_edit {
                e.note.leader = leader;
            }
            return;
        }
        // `\n` typed in the bar breaks the line.
        let text = self.note_text.replace("\\n", "\n");
        let mut p = json!({"text": text, "at": point(at), "view": view});
        if let Some(arrow) = leader {
            p["leader"] = point(arrow);
        }
        self.run("text", p);
    }

    /// Rotate the selection about `centre` by `angle` degrees, counter-clockwise; with Copy, a
    /// turned copy.
    pub(crate) fn turn(&mut self, centre: P2, angle: f64) {
        let p = json!({"ids": self.selection, "about": point(centre), "angle": angle, "copy": self.rotate_copy});
        let made = self.run("rotate", p);
        self.edited(made, self.rotate_copy);
    }

    /// Mirror the selection across the line through `a` and `b`; with Copy (Revit's default), a
    /// mirrored copy.
    fn reflect(&mut self, (a, b): (P2, P2)) {
        if a.dist(b) > TOL {
            let made = self.run("mirror", json!({"ids": self.selection, "axis": [point(a), point(b)], "copy": self.mirror_copy}));
            self.edited(made, self.mirror_copy);
        }
    }

    /// After Rotate or Mirror: back to Modify, with the copies selected when there are copies.
    fn edited(&mut self, made: Option<Value>, copied: bool) {
        let Some(made) = made else { return };
        if copied {
            self.selection = made["ids"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
        }
        self.tool = Tool::Modify;
    }

    /// Align's second pick: turn and move the element so its picked line lies on the reference.
    /// Its padlock is offered then, on the line where it went.
    fn align(&mut self, reference: (P2, P2), id: Id, line: (P2, P2)) {
        let to = match self.drawing.align_reference {
            Some(draw::Picked::Element(r)) => Some(r),
            _ => None,
        };
        if let Some(made) = self
            .run("align", json!({"id": id, "reference": [point(reference.0), point(reference.1)], "line": [point(line.0), point(line.1)]}))
        {
            self.tool = Tool::Align(None);
            self.drawing.align_reference = None;
            let at = |k: usize| made["line"][k].as_array().and_then(|q| Some(P2(q.first()?.as_f64()?, q.get(1)?.as_f64()?)));
            self.drawing.align_lock = at(0).zip(at(1)).map(|line| draw::AlignLock { id, to, reference, line });
        }
    }

    /// Align on a pattern: element `id`'s pattern moved across so its `line` lies on `reference`. The core says where the
    /// pattern then starts (`Model::align_pattern`); the window only sends that through `modify`.
    fn align_pattern(&mut self, m: &Model, reference: (P2, P2), id: Id, line: (P2, P2)) {
        match m.align_pattern(id, reference, line) {
            Ok(to) => {
                if self.run("modify", json!({"id": id, "pattern_origin": point(to)})).is_some() {
                    self.tool = Tool::Align(None);
                    self.drawing.align_reference = None;
                }
            }
            Err(e) => self.status = Status { text: t(&e).into(), error: true },
        }
    }

    /// Where Offset takes wall `id` for the cursor at `p`: across it, toward `p`, by the Options
    /// Bar's distance.
    fn offset_by(&mut self, m: &Model, id: Id, p: P2) -> Option<P2> {
        let w = m.wall(id)?;
        if w.arc != 0.0 {
            self.status = Status { text: t("offset: it offsets straight walls").into(), error: true };
            return None;
        }
        let Some(d) = m.units.parse(&self.offset).filter(|d| *d > 0.0) else {
            self.status = Status { text: tf("offset: `{}` is not a distance", &[&self.offset]), error: true };
            return None;
        };
        let n = (w.end - w.start).unit().perp();
        Some(micron(n * if (p - w.start).dot(n) >= 0.0 { d } else { -d }))
    }

    /// The tool's next point: of a wall chain, a grid's ends, a move's or a copy's base point and
    /// destination.
    pub(crate) fn place(&mut self, level: Id, p: P2) {
        // A shape other than a line: its points, then the walls along what they make.
        if matches!(self.tool, Tool::Wall(_)) && self.shape() != draw::Shape::Line {
            if matches!(self.shape(), draw::Shape::PickLines | draw::Shape::PickFaces) {
                return;
            }
            if let Some(made) = self.shape_click(p) {
                self.make_walls(level, &made);
            }
            return;
        }
        if matches!(self.tool, Tool::Run(..)) {
            return self.place_run(level, p);
        }
        if matches!(self.tool, Tool::DetailLine(_)) {
            return self.place_detail_line(level, p);
        }
        match &mut self.tool {
            // A line: each click after the first builds the wall from the one before it.
            Tool::Wall(chain) => match (chain.first().copied(), chain.last().copied()) {
                (Some(first), Some(last)) if last.dist(p) > TOL => {
                    if self.options_refused() {
                        return;
                    }
                    if self.build_walls(level, &draw::offset_line(last, p, self.draw_options().offset), false, 0.0) {
                        let Tool::Wall(chain) = &mut self.tool else { return };
                        // Back on its start, the chain closes; without Chain, every wall stands alone.
                        if !self.chain || first.dist(p) <= TOL {
                            chain.clear();
                        } else {
                            chain.push(p);
                        }
                    }
                }
                (Some(_), _) => {}
                (None, _) => chain.push(p),
            },
            // Each click past the first a line from the one before, the chain going on from there.
            Tool::RoomSeparator(start) => match start.take() {
                Some(a) if a.dist(p) <= TOL => *start = Some(a),
                Some(a) => {
                    self.run("room_separator", json!({"points": [point(a), point(p)], "level": level}));
                    if let Tool::RoomSeparator(start) = &mut self.tool {
                        *start = Some(p);
                    }
                }
                None => *start = Some(p),
            },
            Tool::Grid(start) => match start.take() {
                None => *start = Some(p),
                Some(a) if a.dist(p) > TOL => {
                    self.run("grid", json!({"start": point(a), "end": point(p)}));
                }
                Some(a) => *start = Some(a),
            },
            // An upright plane through the two points, on the plan's level.
            Tool::ReferencePlane(start) => match start.take() {
                None => *start = Some(p),
                Some(a) if a.dist(p) > TOL => {
                    self.run("reference_plane", json!({"start": point(a), "end": point(p), "level": level}));
                }
                Some(a) => *start = Some(a),
            },
            Tool::Section(start) => match start.take() {
                None => *start = Some(p),
                Some(a) if a.dist(p) > TOL => {
                    self.run("section", json!({"start": point(a), "end": point(p)}));
                }
                Some(a) => *start = Some(a),
            },
            Tool::Camera(start) => match start.take() {
                None => *start = Some(p),
                Some(a) if a.dist(p) > TOL => {
                    // The eye stands at the first point, the camera looks at the second: the Options Bar's heights over the
                    // plan's level, and its lens.
                    let z0 = self.session.model.elevation(level);
                    let eye = z0 + self.units.parse(&self.camera_eye_height).unwrap_or(1600.0);
                    let target = z0 + self.units.parse(&self.camera_target_height).unwrap_or(1200.0);
                    let lens = self.camera_lens.trim().parse::<f64>().unwrap_or(35.0);
                    let params = json!({"eye": [a.0, a.1, eye], "target": [p.0, p.1, target], "lens": lens});
                    if let Some(made) = self.run("camera", params) {
                        self.open_3d(made["id"].as_u64());
                    }
                }
                Some(a) => *start = Some(a),
            },
            Tool::Move(base) | Tool::Copy(base) => match base.take() {
                None => *base = Some(p),
                Some(from) => self.shift_selection(from, p),
            },
            _ => {}
        }
    }

    /// The Move and Copy tools' second point: the selection moved or copied by `to - from`. A copy
    /// is selected afterwards; with Multiple, the tool goes on copying from the same base point.
    fn shift_selection(&mut self, from: P2, to: P2) {
        let copy = matches!(self.tool, Tool::Copy(_));
        let again = if copy { Tool::Copy(Some(from)) } else { Tool::Move(Some(from)) };
        if from.dist(to) <= TOL {
            self.tool = again;
            return;
        }
        let params = json!({"ids": self.selection, "by": point(to - from)});
        match self.run(if copy { "copy" } else { "move" }, params) {
            None => self.tool = again,
            Some(_) if copy && self.multiple => self.tool = again,
            Some(made) => {
                if copy {
                    self.selection = made["ids"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
                }
                self.tool = Tool::Modify;
            }
        }
    }

    /// The typed length, from the tool's base point toward the cursor: along the guide the cursor
    /// snapped to, else onto the nearest snap angle when close. Enter while a length is typed.
    pub(crate) fn place_typed(&mut self, level: Id) {
        if self.filleting() {
            self.fillet_typed();
            return;
        }
        let Some(cursor) = self.canvas.cursor else { return };
        if self.place_numbers(level, self.canvas.found.get().map_or(cursor, |f| f.at)) {
            return;
        }
        let typed = std::mem::take(&mut self.typed_length);
        let Some(base) = self.base() else { return };
        // The length and the angle, whichever Tab left in `drawing.other`.
        let other = std::mem::take(&mut self.drawing.other);
        let (typed, angle) = if std::mem::take(&mut self.drawing.field) == 1 { (other, typed) } else { (typed, other) };
        let toward = match (angle.trim().trim_end_matches('°').trim().parse::<f64>(), self.canvas.found.get()) {
            (Ok(a), _) if a.is_finite() => P2(a.to_radians().cos(), a.to_radians().sin()),
            (Err(_), _) if !angle.trim().is_empty() => {
                self.status = Status { text: tf("`{}` is not an angle", &[angle.trim()]), error: true };
                return;
            }
            (_, Some(f)) if f.at.dist(base) > TOL => (f.at - base).unit(),
            _ => snap_angle(cursor - base, self.snaps.angle),
        };
        // An angle with no length: as far along it as the cursor is.
        let typed = if typed.trim().is_empty() {
            let d = cursor - base;
            self.session.model.units.length(d.0 * toward.0 + d.1 * toward.1)
        } else {
            typed
        };
        match self.session.model.units.parse(&typed) {
            Some(len) if len > 0.0 && len <= MAX_COORD => self.put(level, micron(base + toward * len)),
            _ => self.status = Status { text: tf("`{}` is not a length", &[typed.trim()]), error: true },
        }
    }

    /// End the wall chain, its walls built as it went: the tool's Enter. An arc not finished yet
    /// is dropped.
    pub(crate) fn finish_wall(&mut self) {
        if let Tool::Wall(chain) = &mut self.tool {
            chain.clear();
        }
    }

    /// Walls along `points`, each turning `arc` degrees, as the Options Bar says: their type,
    /// their top, the line they stand on. Whether they were made.
    fn build_walls(&mut self, level: Id, points: &[P2], closed: bool, arc: f64) -> bool {
        match self.wall_params(level, points, closed, arc) {
            Some(p) => self.run("wall", p).is_some(),
            None => false,
        }
    }

    /// The Wall tool's params for walls through `points`: the type and the Options Bar's location line and height.
    pub(crate) fn wall_params(&mut self, level: Id, points: &[P2], closed: bool, arc: f64) -> Option<Value> {
        let mut p = json!({
            "points": points.iter().map(|q| point(*q)).collect::<Vec<_>>(), "level": level, "closed": closed,
            "arc": arc, "location": self.wall_location.name(),
        });
        if let Some(t) = self.types.get("wall") {
            p["type"] = json!(t);
        }
        // Drawn in the 3D view, on the face their base stands on.
        if let Some(offset) = self.orbit.wall_base {
            p["base_offset"] = json!(offset);
        }
        // The Options Bar's Height: up to a level, or unconnected at a height.
        match top_params(&self.session.model, level, self.wall_top, &self.wall_height) {
            Ok(Value::Object(top)) => p.as_object_mut().into_iter().for_each(|p| p.extend(top.clone())),
            Ok(_) => {}
            Err(e) => {
                self.status = Status { text: tf("wall: {}", &[&e]), error: true };
                return None;
            }
        }
        Some(p)
    }

    /// The section or elevation selected alone, with Modify: the one whose extent a plan shows, with its grips. With it,
    /// whether it is an elevation.
    fn view_gripped(&self) -> Option<(Id, Section, bool)> {
        match (self.selection.as_slice(), &self.tool) {
            (&[id], Tool::Modify) => match self.session.model.elements.get(id)? {
                Element::Section(v) => Some((id, v.clone(), false)),
                Element::Elevation(v) => Some((id, v.clone(), true)),
                _ => None,
            },
            _ => None,
        }
    }

    /// A drag from a grip of the selected section or elevation moves that part: an end of its line along it (while the
    /// view is cropped), its far clip to and from it; from an elevation's line, the line the way it looks, its mark staying.
    /// On release, `move` with the part as its `end`.
    fn view_grip_drag(&mut self, ui: &Ui, response: &Response, rect: Rect, view: &View) {
        use bimcraft_core::elevation::{self, Part};
        let Some((id, v, is_elevation)) = self.view_gripped() else {
            self.canvas.view_grip = None;
            return;
        };
        let at = |p: P2| pos(rect.min, view.px(p));
        if response.drag_started_by(PointerButton::Primary)
            && self.canvas.crop_drag.is_none()
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
        {
            let near = |p: P2| at(p).distance(o);
            let from = view.mm((o.x - rect.min.x, o.y - rect.min.y));
            let grip = elevation::grips(&v).into_iter().filter(|(_, p)| near(*p) <= 9.0).min_by(|a, b| near(a.1).total_cmp(&near(b.1)));
            // An elevation's line is taken anywhere along it, off its mark.
            let on_line = || {
                let (a, b) = (at(v.start), at(v.end));
                let (ab, ao) = (b - a, o - a);
                let t = (ao.dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                is_elevation && (a + ab * t).distance(o) <= 5.0 && near(elevation::mark(&v)) > 18.0
            };
            if let Some((part, _)) = grip {
                self.canvas.view_grip = Some((id, part, from));
            } else if on_line() {
                self.canvas.view_grip = Some((id, Part::Line, from));
            }
        }
        let Some((held, part, from)) = self.canvas.view_grip else { return };
        if held != id {
            self.canvas.view_grip = None;
            return;
        }
        if response.drag_stopped() {
            self.canvas.view_grip = None;
            if !self.canvas.quick()
                && let Some(q) = ui.input(|i| i.pointer.latest_pos())
            {
                let by = view.mm((q.x - rect.min.x, q.y - rect.min.y)) - from;
                if by.length() > 1e-6 {
                    self.run("move", json!({"ids": [id], "by": point(by), "end": part.name()}));
                }
            }
        }
    }

    /// The selected section's or elevation's extent in plan, as Revit shows it: the line the view starts at; with a far
    /// clip, the line it sees as far as and the sides between them; a round grip at each end of the line while the view
    /// is cropped, and a pair of arrows on the far clip. A part being dragged shows where it would come to.
    fn view_extent_overlay(&self, ui: &Ui, m: &Model, view: &View, rect: Rect) {
        use bimcraft_core::elevation;
        let Some((id, v, is_elevation)) = self.view_gripped() else { return };
        if m.section(id).is_none() {
            return;
        }
        let v = match (self.canvas.view_grip, ui.input(|i| i.pointer.latest_pos())) {
            (Some((held, part, from)), Some(q)) if held == id => {
                let by = view.mm((q.x - rect.min.x, q.y - rect.min.y)) - from;
                elevation::dragged(&v, is_elevation, part, by).unwrap_or(v)
            }
            _ => v,
        };
        let p = ui.painter_at(rect);
        let at = |q: P2| pos(rect.min, view.px(q));
        let blue = canvas_pal().accent;
        let (a, b) = (at(v.start), at(v.end));
        // The line it starts at: long dashes with a dot between, as a cut line is drawn.
        p.extend(Shape::dashed_line_with_offset(&[a, b], Stroke::new(1.5, blue), &[12.0, 2.0], &[4.0, 4.0], 0.0));
        if let Some((fa, fb)) = elevation::far(&v) {
            let faint = Stroke::new(1.0, blue.gamma_multiply(0.7));
            p.extend(Shape::dashed_line(&[a, at(fa), at(fb), b], faint, 5.0, 4.0));
        }
        for (part, g) in elevation::grips(&v) {
            let c = at(g);
            if part == elevation::Part::Far {
                // Two arrows, to and from the view's line.
                let l = v.looking();
                let (n, t) = (vec2(l.0 as f32, -l.1 as f32), vec2(-l.1 as f32, -l.0 as f32));
                for k in [1.0, -1.0] {
                    let tip = c + n * (9.0 * k);
                    let base = c + n * (1.5 * k);
                    p.add(Shape::convex_polygon(vec![tip, base + t * 4.5, base - t * 4.5], blue, Stroke::new(1.0, pal().surface)));
                }
            } else {
                p.circle(c, 4.5, blue.gamma_multiply(0.75), Stroke::new(1.0, blue));
            }
        }
    }

    /// The wall or run selected alone, with Modify: the one whose ends are grips.
    fn gripped(&self) -> Option<Id> {
        let m = &self.session.model;
        match (self.selection.as_slice(), &self.tool) {
            (&[id], Tool::Modify) if m.wall(id).is_some() || m.run_of(id).is_some() => Some(id),
            _ => None,
        }
    }

    /// The ends of the element whose ends are grips, `(start, end)`: a wall's or a run's line.
    fn grip_ends(m: &Model, id: Id) -> Option<(P2, P2)> {
        match m.elements.get(id)? {
            Element::Wall(w) => Some((w.start, w.end)),
            e => Some((e.run()?.start, e.run()?.end)),
        }
    }

    /// Where the gripped end (else `start`, the line's other end) goes for a drag to pixel `q`:
    /// what that snaps to from the fixed end; with Ctrl, along that line at the line's own length,
    /// so it turns about the fixed end.
    fn grip_target(&self, m: &Model, level: Id, v: &View, (ends, start): ((P2, P2), bool), q: (f32, f32), mods: Modifiers) -> (Found, P2) {
        let (mine, fixed) = if start { (ends.0, ends.1) } else { (ends.1, ends.0) };
        let found = self.snap(m, level, v, q, &[fixed], mods.shift);
        let at = if mods.command && fixed.dist(found.at) > TOL {
            micron(fixed + (found.at - fixed).unit() * mine.dist(fixed))
        } else {
            found.at
        };
        (found, at)
    }

    /// A drag from an end of the selected wall or run moves that end: its angle and length follow
    /// the cursor, with the snaps and angle guides from the other end (Ctrl keeps the length); on
    /// release, `move`, and the walls joined there follow.
    fn grip_drag(&mut self, ui: &Ui, response: &Response, rect: Rect, view: &View, level: Id) {
        let Some(id) = self.gripped() else {
            self.canvas.grip = None;
            return;
        };
        let m = std::sync::Arc::clone(&self.session.model);
        let Some(ends) = Self::grip_ends(&m, id) else { return };
        let at = |p: P2| pos(rect.min, view.px(p));
        if response.drag_started_by(PointerButton::Primary)
            && self.canvas.crop_drag.is_none()
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
        {
            let near = |p: P2| at(p).distance(o);
            let nearest = [(true, ends.0), (false, ends.1)]
                .into_iter()
                .filter(|(_, p)| near(*p) <= 8.0)
                .min_by(|a, b| near(a.1).total_cmp(&near(b.1)));
            if let Some((start, _)) = nearest {
                self.canvas.grip = Some((id, start));
            }
        }
        let Some((held, start)) = self.canvas.grip else { return };
        if held != id {
            self.canvas.grip = None;
            return;
        }
        if response.drag_stopped() {
            self.canvas.grip = None;
            if !self.canvas.quick()
                && let Some(q) = ui.input(|i| i.pointer.latest_pos())
            {
                let mods = ui.input(|i| i.modifiers);
                let (_, to) = self.grip_target(&m, level, view, (ends, start), (q.x - rect.min.x, q.y - rect.min.y), mods);
                self.snap_once = None;
                let by = to - if start { ends.0 } else { ends.1 };
                if by.length() > 1e-6 {
                    self.run("move", json!({"ids": [id], "by": point(by), "end": if start { "start" } else { "end" }}));
                }
            }
        }
    }

    /// The selected wall's or run's grips at its ends and, beside each for a wall, its angle to
    /// click and type: the wall turns about that end. Dragging one, the element as it would stand,
    /// with its ruler.
    #[allow(clippy::too_many_arguments)]
    fn grip_overlay(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect, level: Id, d: &Drawing, place: Place) {
        let Some(id) = self.gripped() else {
            self.angle_edit = None;
            return;
        };
        let Some(ends) = Self::grip_ends(m, id) else { return };
        let wall = m.wall(id).cloned();
        let p = ui.painter_at(rect);
        let at = |q: P2| pos(rect.min, v.px(q));
        let held = self.canvas.grip.filter(|(g, _)| *g == id).map(|(_, start)| start);
        let mut ends_drawn = [ends.0, ends.1];
        if let (Some(start), Some(q)) = (held, ui.input(|i| i.pointer.latest_pos())) {
            let (mods, q) = (ui.input(|i| i.modifiers), (q.x - rect.min.x, q.y - rect.min.y));
            let (found, to) = self.grip_target(m, level, v, (ends, start), q, mods);
            let sc = Scene { p: &p, m, d, place, level, key: level, v };
            // The element as it would stand: its outline, and the ruler from the end that stays.
            let moved_ends = if start { (to, ends.1) } else { (ends.0, to) };
            let ring: Vec<P2> = match &wall {
                // A wall's faces, where its location line puts them.
                Some(w) => {
                    let moved = Wall { start: moved_ends.0, end: moved_ends.1, ..w.clone() };
                    let (ext, int) = walls::faces(m, &moved);
                    let axis = Axis::of(&moved);
                    let (left, right) = (axis.points(0.0, axis.length(), ext), axis.points(0.0, axis.length(), int));
                    left.into_iter().chain(right.into_iter().rev()).collect()
                }
                // A run's footprint across its axis, in the state it would take.
                None => m
                    .elements
                    .get(id)
                    .and_then(|e| {
                        let mut e = e.clone();
                        let r = e.run_mut()?;
                        (r.start, r.end) = moved_ends;
                        m.run_geom(&e).map(|g| g.footprint())
                    })
                    .unwrap_or_default(),
            };
            let ring: Vec<Pos2> = ring.into_iter().map(&at).collect();
            p.add(Shape::closed_line(ring, Stroke::new(1.0, pal().accent)));
            let fixed = if start { ends.1 } else { ends.0 };
            p.extend(ruler_shapes(&sc, fixed, to));
            self.show_snap(&sc, &found);
            self.canvas.found.set(Some(found));
            let r = snap::ruler(fixed, to);
            let reading = format!("{}  {}", m.units.with_unit(r.length), degrees(if start { norm_turn(r.angle + 180.0) } else { r.angle }));
            tag(&p, at(to) + vec2(12.0, -12.0), Align2::LEFT_BOTTOM, reading);
            ends_drawn[usize::from(!start)] = to;
        }
        for (k, e) in ends_drawn.into_iter().enumerate() {
            let r = Rect::from_center_size(at(e), vec2(9.0, 9.0));
            let live = held.is_some_and(|s| s == (k == 0));
            p.rect_filled(r, 1.0, if live { pal().accent } else { pal().surface });
            p.rect_stroke(r, 1.0, Stroke::new(1.5, pal().accent), StrokeKind::Middle);
        }
        // Angles to type are a wall's: a run turns with its other commands.
        if held.is_none()
            && let Some(w) = wall
        {
            self.wall_angles(ui, &p, id, &w, v, rect);
        }
    }

    /// A selected wall's angle beside each end, to click and type: the wall turns about that end,
    /// to the angle typed (degrees from east, counter-clockwise).
    fn wall_angles(&mut self, ui: &mut Ui, p: &Painter, id: Id, w: &Wall, v: &View, rect: Rect) {
        let (a, b) = (pos(rect.min, v.px(w.start)), pos(rect.min, v.px(w.end)));
        if a.distance(b) < 80.0 {
            return;
        }
        let u = (b - a).normalized();
        let angle = snap::ruler(w.start, w.end).angle;
        let text = degrees(angle);
        let font = FontId::proportional(12.0);
        let (mut done, mut clicked) = (None, None);
        for start in [true, false] {
            let anchor = if start { a - u * 32.0 } else { b + u * 32.0 };
            let size = p.layout_no_wrap(text.clone(), font.clone(), pal().accent).size() + vec2(10.0, 4.0);
            let at = Rect::from_center_size(anchor, size);
            let field = ui.id().with(("wall angle", id, start));
            match &mut self.angle_edit {
                Some((wall, which, typed)) if *wall == id && *which == start => {
                    let edit = ui.put(at.expand2(vec2(18.0, 2.0)), TextEdit::singleline(typed).id(field).horizontal_align(Align::Center));
                    if edit.lost_focus() {
                        done = Some((start, ui.input(|i| !i.key_pressed(Key::Escape)).then(|| typed.clone())));
                    } else if !edit.has_focus() {
                        edit.request_focus();
                    }
                }
                _ => {
                    let r = ui.interact(at, field, Sense::click());
                    let which = t(if start { "start" } else { "end" });
                    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tf("Turn the wall about its {}: {}", &[which, &text])));
                    p.rect_filled(at, 2.0, pal().surface);
                    p.text(at.center(), Align2::CENTER_CENTER, &text, font.clone(), if r.hovered() { pal().hover } else { pal().accent });
                    if r.clicked() {
                        clicked = Some(start);
                    }
                }
            }
        }
        if let Some(start) = clicked {
            self.angle_edit = Some((id, start, text));
        }
        if let Some((start, typed)) = done {
            self.angle_edit = None;
            let Some(typed) = typed else { return };
            match number(typed.trim().trim_end_matches('°')) {
                Some(deg) => {
                    let mut by = (deg - angle).rem_euclid(360.0);
                    if by > 180.0 {
                        by -= 360.0;
                    }
                    // The label reads to the hundredth: what it said, typed back, turns nothing.
                    if by.abs() > 1e-9 && (deg - (angle * 100.0).round() / 100.0).abs() > 1e-9 {
                        let about = if start { w.start } else { w.end };
                        self.run("rotate", json!({"ids": [id], "about": point(about), "angle": by}));
                    }
                }
                None => self.status = Status { text: tf("`{}` is not an angle", &[typed.trim()]), error: true },
            }
        }
    }

    /// A temporary dimension's number at `at`, which a click turns into a field to type a new one: what was typed once
    /// the field is left (None inside it for Esc). `key` is the element and which of its dimensions.
    fn dimension_number(&mut self, ui: &mut Ui, at: Rect, key: (Id, usize), text: &str) -> Option<Option<String>> {
        let look = NumberInk { size: 12.0, color: pal().accent };
        self.number_field(ui, at, key, text, bimcraft_i18n::msg!("Temporary dimension {}"), look)
    }

    /// A dimension's number at `at`, as [`dimension_number`](Self::dimension_number) shows one, named `name` (with the number
    /// in it) for the screen reader. A field opens empty: what was typed once it is left (None for Esc, an empty one, or a
    /// click elsewhere), and a null for a number left empty.
    fn number_field(&mut self, ui: &mut Ui, at: Rect, key: (Id, usize), text: &str, name: &str, look: NumberInk) -> Option<Option<String>> {
        let font = FontId::proportional(look.size);
        let field = ui.id().with(("temporary dimension", key));
        let (mut done, mut start, mut escaped) = (None, false, false);
        match &mut self.dim_edit {
            Some((id, n, typed)) if (*id, *n) == key => {
                self.over_dimension_control = true;
                let edit = ui.put(at.expand2(vec2(18.0, 2.0)), TextEdit::singleline(typed).id(field).horizontal_align(Align::Center));
                if edit.lost_focus() {
                    escaped = ui.input(|i| i.key_pressed(Key::Escape));
                    let typed = typed.trim().to_string();
                    done = Some((!escaped && !typed.is_empty()).then_some(typed));
                } else if !edit.has_focus() {
                    edit.request_focus();
                }
            }
            _ => {
                let r = ui.interact(at, field.with("number"), Sense::click());
                if r.hovered() {
                    self.over_dimension_control = true;
                }
                r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tf(name, &[text])));
                ui.painter().rect_filled(at, 2.0, pal().surface);
                let ink = if r.hovered() { pal().hover } else { look.color };
                ui.painter().text(at.center(), Align2::CENTER_CENTER, text, font, ink);
                start = r.clicked();
            }
        }
        if start {
            self.dim_edit = Some((key.0, key.1, String::new()));
        }
        if escaped {
            self.dim_edit_escaped = true;
        }
        if done.is_some() {
            self.dim_edit = None;
        }
        done
    }

    /// A selected aligned or linear dimension's constraints, as Revit shows them: a padlock under each segment, which a
    /// click locks at its length or unlocks, and past its end, over a string of two segments or more, EQ, which a click
    /// makes equal or not (crossed out while it isn't).
    fn dimension_constraints(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect, id: Id, d: &Dimension) {
        if !matches!(d.kind, DimKind::Aligned | DimKind::Linear) {
            return;
        }
        let (n, cross) = m.crossings(d);
        if cross.len() < 2 {
            return;
        }
        let screen = |q: P2| pos(rect.min, v.px(q));
        let on = |s: f64| screen(d.at + n * s);
        // On screen: off the line to the side its numbers aren't.
        let side = (screen(d.at - n.perp()) - screen(d.at)).normalized();
        let p = ui.painter_at(rect);
        for (i, pair) in cross.windows(2).enumerate() {
            let [(s0, _), (s1, _)] = pair else { continue };
            let at = on((s0 + s1) / 2.0) + side * 14.0;
            let locked = d.locked.iter().any(|l| l.segment == i);
            let r = ui.interact(Rect::from_center_size(at, vec2(16.0, 18.0)), ui.id().with(("dimension lock", id, i)), Sense::click());
            if r.hovered() {
                self.over_dimension_control = true;
            }
            let name = tf("Lock segment {}", &[&(i + 1).to_string()]);
            r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, locked, name.clone()));
            let ink = if r.hovered() {
                pal().hover
            } else if locked {
                pal().accent
            } else {
                pal().muted
            };
            padlock(&p, at, locked, ink);
            let r = r.on_hover_text(if locked {
                tf("Segment {} is locked at its length: what it measures to moves together. Click to unlock it", &[&(i + 1).to_string()])
            } else {
                tf("Lock segment {} at its length, so what it measures to moves together", &[&(i + 1).to_string()])
            });
            if r.clicked() {
                let mut segments: Vec<usize> = d.locked.iter().map(|l| l.segment).filter(|&s| s != i).collect();
                if !locked {
                    segments.push(i);
                }
                self.run("dimension_constraint", json!({"id": id, "locked": segments}));
            }
        }
        // Each segment's number, which a click turns into a field: a length typed there moves what the segment ends at
        // (`dimension_value`), as a temporary dimension's does; under it, a small T opens the number's Dimension Text.
        if m.dimension_type(d).string == bimcraft_core::dims::DimString::Continuous {
            for (i, (pair, label)) in cross.windows(2).zip(m.dimension_labels(d)).enumerate() {
                let [(s0, _), (s1, _)] = pair else { continue };
                let size = p.layout_no_wrap(label.clone(), FontId::proportional(12.0), pal().accent).size() + vec2(10.0, 4.0);
                let at = Rect::from_center_size(on((s0 + s1) / 2.0) - side * 12.0, size);
                // A larger button over the number, which opens the same Dimension Text the double click does.
                let icon = Rect::from_center_size(at.center() - vec2(0.0, at.height() / 2.0 + 12.0), vec2(24.0, 20.0));
                let r = ui.interact(icon, ui.id().with(("dimension text", id, i)), Sense::click());
                if r.hovered() {
                    self.over_dimension_control = true;
                }
                r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, t("Dimension text")));
                let ink = if r.hovered() { pal().hover } else { pal().accent_ink };
                p.rect_filled(icon, 3.0, pal().surface);
                p.rect_stroke(icon, 3.0, Stroke::new(1.0, ink), StrokeKind::Inside);
                p.text(icon.center(), Align2::CENTER_CENTER, "Aa", FontId::proportional(12.0), ink);
                if r.clicked() {
                    self.dimension_text_dialog = Some(crate::dimtypes::DimensionTextDialog::new(m, id, i));
                }
                let look = NumberInk { size: 12.0, color: pal().accent };
                let Some(Some(typed)) = self.number_field(ui, at, (id, i), &label, bimcraft_i18n::msg!("Dimension {}"), look) else {
                    continue;
                };
                match m.units.parse(&typed) {
                    Some(value) => {
                        self.run("dimension_value", json!({"id": id, "segment": i, "value": value}));
                    }
                    None => self.status = Status { text: tf("`{}` is not a length", &[typed.trim()]), error: true },
                }
            }
        }
    }

    /// A selected element's dimensions, whose numbers are picked to type a value into: shown smaller and in the accent's
    /// ink (Revit's own), a click opening an empty field. What is typed moves the selected element, the far reference
    /// staying where it is; an equal dimension takes its value at the end opposite the element.
    fn dimension_highlights(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect, key: Id) {
        if self.tool != Tool::Modify || self.canvas.drag.is_some() || self.canvas.grip.is_some() || self.drawing.grip.is_some() {
            return;
        }
        let &[sel] = self.selection.as_slice() else { return };
        if matches!(m.elements.get(sel), Some(Element::Dimension(_))) {
            return;
        }
        let Some(sel_guid) = m.elements.guid(sel) else { return };
        let screen = |q: P2| pos(rect.min, v.px(q));
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let p = ui.painter_at(rect);
        let dims: Vec<(Id, Dimension)> = m
            .elements
            .iter()
            .filter_map(|(&id, e)| match e {
                Element::Dimension(d)
                    if matches!(d.kind, DimKind::Aligned | DimKind::Linear)
                        && !d.alignment
                        && m.annotation_in(e, key)
                        && d.refs.iter().any(|r| r.id == sel_guid) =>
                {
                    Some((id, d.clone()))
                }
                _ => None,
            })
            .collect();
        for (id, d) in dims {
            let (n, cross) = m.crossings(&d);
            if cross.len() < 2 {
                continue;
            }
            let side = (screen(d.at - n.perp()) - screen(d.at)).normalized();
            let on = |s: f64| screen(d.at + n * s);
            let labels = m.dimension_labels(&d);
            for (i, (pair, label)) in cross.windows(2).zip(labels).enumerate() {
                let [(s0, _), (s1, _)] = pair else { continue };
                let size = p.layout_no_wrap(label.clone(), FontId::proportional(11.0), pal().accent_ink).size() + vec2(10.0, 4.0);
                let at = Rect::from_center_size(on((s0 + s1) / 2.0) - side * 12.0, size);
                let editing = matches!(&self.dim_edit, Some((eid, ei, _)) if (*eid, *ei) == (id, i));
                if pointer.is_some_and(|q| at.expand2(vec2(18.0, 2.0)).contains(q)) || editing {
                    self.over_dimension_control = true;
                }
                let look = NumberInk { size: 11.0, color: pal().accent_ink };
                let Some(Some(typed)) = self.number_field(ui, at, (id, i), &label, bimcraft_i18n::msg!("Dimension {}"), look) else {
                    continue;
                };
                let Some(value) = m.units.parse(&typed) else {
                    self.status = Status { text: tf("`{}` is not a length", &[typed.trim()]), error: true };
                    continue;
                };
                // The far reference stays: the clicked segment's other station, or an equal dimension's opposite end.
                let (_, st) = m.stations(&d);
                let hold = if d.equal {
                    let k = st.len().saturating_sub(1);
                    let near = st.iter().position(|(_, _, r)| r.id == sel_guid).unwrap_or(0);
                    st.get(if near * 2 <= k { k } else { 0 }).map(|(_, _, r)| *r)
                } else {
                    match (st.get(i), st.get(i + 1)) {
                        (Some((_, _, a)), Some((_, _, b))) if a.id == sel_guid => Some(*b),
                        (Some((_, _, a)), Some((_, _, b))) if b.id == sel_guid => Some(*a),
                        _ => None,
                    }
                };
                let mut params = json!({"id": id, "segment": i, "value": value});
                if let Some(hold) = hold {
                    params["hold"] = json!(hold);
                }
                self.run("dimension_value", params);
            }
        }
    }

    /// A selected element's dimensions, each number picked as [`dimension_highlights`](Self::dimension_highlights) says.
    fn dimension_overlays(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect, key: Id) {
        self.dimension_reference_highlight(ui, m, v, rect);
        if let Some(edit) = &self.dimension_refs_edit {
            // The references picked so far, each marked where it crosses the line being edited.
            if let Some(Element::Dimension(old)) = m.elements.get(edit.id) {
                let d = Dimension { refs: edit.refs.clone(), ..old.clone() };
                let (_, st) = m.stations(&d);
                let p = ui.painter_at(rect);
                for (_, _, r) in st {
                    if let Ok((a, b)) = m.reference_geometry(&r) {
                        let c = pos(rect.min, v.px((a + b) * 0.5));
                        p.circle_stroke(c, 7.0, Stroke::new(2.0, pal().accent));
                    }
                }
            }
            return;
        }
        self.dimension_segment_highlight(ui, m, v, rect);
        self.dimension_eq_hint(ui, m, v, rect);
        self.dimension_highlights(ui, m, v, rect, key);
    }

    /// The equality control over the middle of a dimension's chain: over an equal chain while the pointer is near it, and
    /// always while the dimension is selected. A click makes the chain equal or not, as Revit's EQ; one that isn't equal
    /// wears it struck through. Screen only: never drawn in a plan, a PNG or on paper.
    fn dimension_eq_hint(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect) {
        let cursor = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p));
        let q = cursor.map(|p| v.mm((p.x - rect.min.x, p.y - rect.min.y)));
        let near = 25.0 / v.scale;
        let mut toggle: Option<(Id, bool)> = None;
        let p = ui.painter_at(rect);
        for (&id, e) in m.elements.iter() {
            let Element::Dimension(d) = e else { continue };
            if d.alignment || !matches!(d.kind, DimKind::Aligned | DimKind::Linear) {
                continue;
            }
            let (n, st) = m.stations(d);
            if st.len() < 3 {
                continue;
            }
            let (first, last) = (st[0].0, st[st.len() - 1].0);
            let selected = matches!(self.selection.as_slice(), [one] if *one == id) && self.tool == Tool::Modify;
            let hovered = q.is_some_and(|q| {
                let s = (q - d.at).dot(n);
                s >= first - near && s <= last + near && (q - (d.at + n * s)).length() <= near
            });
            if !selected && !(hovered && d.equal) {
                continue;
            }
            let at = pos(rect.min, v.px(d.at + n * ((first + last) / 2.0))) + vec2(0.0, -30.0);
            let text = m.dimension_type(d).equality_text;
            let size = p.layout_no_wrap(text.clone(), FontId::proportional(12.0), pal().accent_ink).size() + vec2(10.0, 4.0);
            let chip = Rect::from_center_size(at, size);
            let r = ui.interact(chip, ui.id().with(("dimension eq", id)), Sense::click());
            if r.hovered() {
                self.over_dimension_control = true;
            }
            r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, d.equal, t("Equal")));
            let ink = if r.hovered() {
                pal().hover
            } else if d.equal {
                pal().accent
            } else {
                pal().muted
            };
            p.rect_filled(chip, 3.0, pal().surface);
            p.rect_stroke(chip, 3.0, Stroke::new(1.0, ink), StrokeKind::Inside);
            p.text(at, Align2::CENTER_CENTER, text, FontId::proportional(12.0), ink);
            if !d.equal {
                p.line_segment([chip.left_bottom() + vec2(4.0, -3.0), chip.right_top() + vec2(-4.0, 3.0)], Stroke::new(1.2, ink));
            }
            if r.clicked() {
                toggle = Some((id, !d.equal));
            }
        }
        if let Some((id, equal)) = toggle {
            self.run("dimension_constraint", json!({"id": id, "equal": equal}));
        }
    }

    /// The reference Tab picked on the element under the cursor, drawn over the plan: a line brightened, or a corner ringed.
    fn dimension_reference_highlight(&self, ui: &Ui, m: &Model, v: &View, rect: Rect) {
        let Some((_, r)) = self.dimension_reference_hover else { return };
        let Ok((a, b)) = m.reference_geometry(&r) else { return };
        let p = ui.painter_at(rect);
        if a.dist(b) > 1e-9 {
            p.line_segment([pos(rect.min, v.px(a)), pos(rect.min, v.px(b))], Stroke::new(3.5, pal().accent));
        } else {
            p.circle_stroke(pos(rect.min, v.px(a)), 6.0, Stroke::new(2.0, pal().accent));
        }
    }

    /// The segment Tab picked, or a click selected, drawn over the dimension line: brighter while it is the picked one.
    fn dimension_segment_highlight(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect) {
        let Some((id, segment)) = self.dimension_segment.or(self.dimension_segment_hover) else { return };
        let Some(Element::Dimension(d)) = m.elements.get(id) else {
            self.dimension_segment = None;
            self.dimension_segment_hover = None;
            return;
        };
        let (n, st) = m.stations(d);
        let (Some(a), Some(b)) = (st.get(segment), st.get(segment + 1)) else { return };
        let on = |s: f64| pos(rect.min, v.px(d.at + n * s));
        let p = ui.painter_at(rect);
        let (ink, width) = if self.dimension_segment == Some((id, segment)) { (pal().accent, 5.0) } else { (pal().hover, 3.0) };
        p.line_segment([on(a.0), on(b.0)], Stroke::new(width, ink));
        p.circle_filled(on((a.0 + b.0) / 2.0), 5.0, ink);
    }

    /// Starts Edit Dimension References on the selected dimension, if it is one.
    pub(crate) fn start_dimension_refs_edit(&mut self) {
        let &[id] = self.selection.as_slice() else { return };
        let Some(Element::Dimension(d)) = self.session.model.elements.get(id) else { return };
        if d.alignment {
            return;
        }
        self.tool = Tool::Modify;
        self.dimension_reference_hover = None;
        self.dimension_refs_edit = Some(DimensionRefsEdit { id, refs: d.refs.clone(), at: d.at });
    }

    /// Confirms Edit Dimension References: one command, one undo step; nothing while fewer than two references are left.
    pub(crate) fn commit_dimension_refs(&mut self) {
        let Some(edit) = self.dimension_refs_edit.take() else { return };
        self.dimension_reference_hover = None;
        if edit.refs.len() < 2 {
            self.status = Status { text: bimcraft_i18n::msg!("A dimension has two references at least.").to_string(), error: true };
            return;
        }
        // The line may have been dragged while its references were picked: commit where the dimension is now.
        let (at, same) = match self.session.model.elements.get(edit.id) {
            Some(Element::Dimension(d)) => (d.at, d.refs == edit.refs),
            _ => (edit.at, false),
        };
        // Nothing picked differently: leaving the mode is enough, and makes no undo step.
        if same {
            return;
        }
        if self.run("dimension", json!({"id": edit.id, "refs": edit.refs, "at": point(at)})).is_none() {
            self.dimension_refs_edit = Some(edit);
        }
    }

    /// A click while editing a dimension's references: one picked again goes, another is added; a click in the open confirms
    /// and leaves the mode. The line moves only by dragging.
    fn dimension_refs_click(&mut self, m: &Model, _key: Id, v: &View, at: (f32, f32), hovered: Option<Id>) {
        let Some(mut edit) = self.dimension_refs_edit.take() else { return };
        let q = v.mm(at);
        // The reference Tab picked, else the one nearest the cursor: a face, an axis or a corner.
        let picked = self
            .dimension_reference_hover
            .filter(|(id, _)| Some(*id) == hovered)
            .map(|(_, r)| r)
            .or_else(|| hovered.and_then(|id| dimension_pick(m, &edit.refs, id, q, 2.0 * f64::from(PICK) / v.scale)));
        self.dimension_reference_hover = None;
        if let Some(r) = picked {
            if let Some(i) = edit.refs.iter().position(|x| *x == r) {
                edit.refs.remove(i);
            } else {
                edit.refs.push(r);
            }
            self.dimension_refs_edit = Some(edit);
            return;
        }
        // A click in the open confirms the edit and leaves the mode; the line itself is moved by dragging, not here.
        self.dimension_refs_edit = Some(edit);
        self.commit_dimension_refs();
    }

    /// The locks of the dimensions in plan `key`, shown whether they're selected or not, as Revit shows them: a shut padlock
    /// under each locked segment, and where Align left a lock, a padlock a click on which unlocks it (deletes the lock). The
    /// dimension selected alone shows its own, which a click toggles (`dimension_constraints`); and while Align's last two
    /// lines are lined up, an open padlock a click on which locks them (`align` with `lock`).
    fn locks(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect, key: Id) {
        let screen = |q: P2| pos(rect.min, v.px(q));
        let p = ui.painter_at(rect);
        let alone = match (self.selection.as_slice(), &self.tool) {
            (&[id], Tool::Modify) => Some(id),
            _ => None,
        };
        let mut unlock = None;
        let mut unlocked: Vec<(Id, usize)> = Vec::new();
        for (&id, e) in m.elements.iter() {
            let Element::Dimension(d) = e else { continue };
            if d.locked.is_empty() || Some(id) == alone || !m.annotation_in(e, key) {
                continue;
            }
            if d.alignment {
                let at = screen(d.at) + vec2(12.0, -12.0);
                let r = ui.interact(Rect::from_center_size(at, vec2(16.0, 18.0)), ui.id().with(("alignment lock", id)), Sense::click());
                if r.hovered() {
                    self.over_dimension_control = true;
                }
                r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, true, t("Unlock alignment")));
                padlock(&p, at, true, if r.hovered() { pal().hover } else { pal().accent });
                if r.clicked() {
                    unlock = Some(id);
                }
                continue;
            }
            let (n, cross) = m.crossings(d);
            let on = |s: f64| screen(d.at + n * s);
            let side = (screen(d.at - n.perp()) - screen(d.at)).normalized();
            for (i, pair) in cross.windows(2).enumerate() {
                let [(s0, _), (s1, _)] = pair else { continue };
                if d.locked.iter().any(|l| l.segment == i) {
                    let at = on((s0 + s1) / 2.0) + side * 14.0;
                    let r = ui.interact(Rect::from_center_size(at, vec2(18.0, 20.0)), ui.id().with(("shown lock", id, i)), Sense::click());
                    if r.hovered() {
                        self.over_dimension_control = true;
                    }
                    let name = tf("Locked segment {}", &[&(i + 1).to_string()]);
                    r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, true, name.clone()));
                    padlock(&p, at, true, if r.hovered() { pal().hover } else { pal().muted });
                    if r.clicked() {
                        unlocked.push((id, i));
                    }
                }
            }
        }
        for (id, i) in unlocked {
            let Some(Element::Dimension(d)) = self.session.model.elements.get(id) else { continue };
            let segments: Vec<usize> = d.locked.iter().map(|l| l.segment).filter(|&s| s != i).collect();
            self.run("dimension_constraint", json!({"id": id, "locked": segments}));
        }
        if let Some(id) = unlock {
            self.run("delete", json!({"ids": [id]}));
        }
        // Align's offer, while its tool is on.
        let Some(lock) = self.drawing.align_lock.filter(|_| matches!(self.tool, Tool::Align(_))) else { return };
        let at = screen((lock.line.0 + lock.line.1) * 0.5) + vec2(12.0, -12.0);
        let r = ui.interact(Rect::from_center_size(at, vec2(16.0, 18.0)), ui.id().with("align lock"), Sense::click());
        r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, false, t("Lock alignment")));
        padlock(&p, at, false, if r.hovered() { pal().hover } else { pal().accent });
        if r.clicked() {
            let (a, b) = (lock.reference, lock.line);
            let mut params = json!({"id": lock.id, "reference": [point(a.0), point(a.1)], "line": [point(b.0), point(b.1)], "lock": true});
            if let Some(to) = lock.to {
                params["to"] = json!(to);
            }
            if self.run("align", params).is_some() {
                self.drawing.align_lock = None;
            }
        }
    }

    /// The selected wall's length as a temporary dimension beside it, as Revit shows one; a door's or a window's two,
    /// to the ends of its wall. A click on a number types a new length: a wall's end moves (the walls joined there
    /// follow), an opening slides along its wall.
    fn temporary_dimension(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect) {
        self.over_dimension_control = false;
        let &[id] = self.selection.as_slice() else {
            self.dim_edit = None;
            return;
        };
        if self.tool != Tool::Modify || self.canvas.drag.is_some() || self.canvas.grip.is_some() || self.drawing.grip.is_some() {
            return;
        }
        match m.elements.get(id) {
            Some(Element::Door(o) | Element::Window(o)) => return self.opening_dimension(ui, m, v, rect, id, o),
            Some(e @ (Element::Column(_) | Element::StructuralColumn(_) | Element::Footing(_) | Element::Component(_))) => {
                return self.standing_dimension(ui, m, v, rect, id, e);
            }
            Some(Element::Dimension(d)) => return self.dimension_constraints(ui, m, v, rect, id, d),
            _ => {}
        }
        let Some(w) = m.wall(id) else { return };
        let (a, b) = (pos(rect.min, v.px(w.start)), pos(rect.min, v.px(w.end)));
        let along = b - a;
        if along.length() < 24.0 {
            return;
        }
        let u = along / along.length();
        // On the side away from the middle of the view: outside, for an outside wall.
        let n = vec2(u.y, -u.x);
        let n = if (a + along / 2.0 + n - rect.center()).length() < (a + along / 2.0 - n - rect.center()).length() { -n } else { n };
        let off = (m.thickness(w) / 2.0 * v.scale) as f32 + 18.0;
        let stroke = Stroke::new(1.0, pal().accent);
        let p = ui.painter_at(rect);
        let axis = Axis::of(w);
        let length = axis.length();
        // Along a curved wall, an arc round its outside; its number at the arc's middle.
        let middle = if axis.centre().is_some() {
            // Turning left, its left is in toward the centre: outside, to its right.
            let side = w.arc.signum();
            let along_arc = |px: f32| axis.points(0.0, length, -side * f64::from(px) / v.scale).into_iter().map(|q| pos(rect.min, v.px(q)));
            p.add(Shape::line(along_arc(off).collect(), stroke));
            for s in [0.0, length] {
                let (i, o) = (axis.at(s, -side * f64::from(off - 10.0) / v.scale), axis.at(s, -side * f64::from(off + 4.0) / v.scale));
                p.line_segment([pos(rect.min, v.px(i)), pos(rect.min, v.px(o))], stroke);
            }
            pos(rect.min, v.px(axis.at(length / 2.0, -side * f64::from(off + 11.0) / v.scale)))
        } else {
            dimension_line(&p, a, b, n, off)
        };
        let text = m.units.length(length);
        let size = p.layout_no_wrap(text.clone(), FontId::proportional(12.0), pal().accent).size() + vec2(10.0, 4.0);
        let number = Rect::from_center_size(middle, size);
        // Under the number, a small dimension-line button makes the temporary dimension a real one: a dimension between
        // the wall's ends, where the temporary line is. A curved wall's length along its arc is no aligned dimension's.
        if w.arc == 0.0 {
            let icon = Rect::from_center_size(middle + vec2(0.0, size.y / 2.0 + 10.0), vec2(22.0, 16.0));
            let r = ui.interact(icon, ui.id().with(("permanent dimension", id)), Sense::click());
            if r.hovered() {
                self.over_dimension_control = true;
            }
            r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, t("Make Dimension Permanent")));
            let ink = if r.hovered() { pal().hover } else { pal().accent_ink };
            p.rect_filled(icon, 3.0, pal().surface);
            p.rect_stroke(icon, 3.0, Stroke::new(1.0, ink), StrokeKind::Inside);
            let y = icon.center().y;
            let (left, right) = (icon.left() + 4.0, icon.right() - 4.0);
            p.line_segment([pos2(left, y), pos2(right, y)], Stroke::new(1.0, ink));
            for x in [left, right] {
                p.line_segment([pos2(x, y - 3.0), pos2(x, y + 3.0)], Stroke::new(1.0, ink));
            }
            if r.clicked() {
                let at = v.mm((icon.center().x - rect.min.x, icon.center().y - rect.min.y));
                // A perpendicular wall joined at an end is what the dimension is measured to there: changing it moves
                // that whole wall, not just its joined end (which would leave it out of square).
                let start = walls::joined_perpendicular(m, id, false);
                let end = walls::joined_perpendicular(m, id, true);
                let refs = match (start, end) {
                    (Some(a), Some(b)) if a != b => json!([{"id": a}, {"id": b}]),
                    (Some(a), None) => json!([{"id": a}, {"id": id, "point": "end"}]),
                    (None, Some(b)) => json!([{"id": id, "point": "start"}, {"id": b}]),
                    _ => json!([{"id": id, "point": "start"}, {"id": id, "point": "end"}]),
                };
                self.run("dimension", json!({"refs": refs, "at": point(at)}));
            }
        }
        if let Some(typed) = self.dimension_number(ui, number, (id, 0), &text) {
            match typed.map(|t| (m.units.parse(&t), t)) {
                Some((Some(len), _)) if len > 0.0 => {
                    self.run("wall_length", json!({"id": id, "value": len}));
                }
                Some((_, typed)) => self.status = Status { text: tf("`{}` is not a length", &[typed.trim()]), error: true },
                None => {}
            }
        }
        // To the nearest parallel wall or grid either side: a length typed there moves the wall square to itself.
        for (n, middle, text, c) in wall_dimensions(&p, m, v, id) {
            let size = p.layout_no_wrap(text.clone(), FontId::proportional(12.0), pal().accent).size() + vec2(10.0, 4.0);
            let Some(typed) = self.dimension_number(ui, Rect::from_center_size(middle, size), (id, n + 1), &text) else { continue };
            match typed.map(|t| (m.units.parse(&t), t)) {
                Some((Some(len), _)) if len > 0.0 => {
                    let by = c.toward * (c.distance - len);
                    if by.length() > 1e-6 {
                        self.run("move", json!({"ids": [id], "by": point(by)}));
                    }
                }
                Some((_, typed)) => self.status = Status { text: tf("`{}` is not a length", &[typed.trim()]), error: true },
                None => {}
            }
        }
    }

    /// The listening dimensions of a shape being drawn, with the cursor at `next`: a rectangle's width and height by
    /// their sides, a radius, an angle or a reach by the cursor, each as typed once something is; else the length
    /// from the last point, as a line's. The number Tab is on says Enter.
    fn shape_dimensions(&self, sc: &Scene, chain: &[P2], next: P2) {
        for (at, text) in self.shape_texts(sc.m, chain, next) {
            tag(sc.p, sc.screen(at) + vec2(12.0, -12.0), Align2::LEFT_BOTTOM, text);
        }
    }

    /// What [`BimApp::shape_dimensions`] says, and where (a model point each).
    pub(crate) fn shape_texts(&self, m: &Model, chain: &[P2], next: P2) -> Vec<(P2, String)> {
        let shape = self.shape();
        let names = bimcraft_core::draw::numbers(shape, chain.len());
        // The radius by the cursor, or as typed.
        if shape == draw::Shape::Fillet {
            return match self.fillet_radius_at(next).filter(|_| self.filleting()) {
                _ if !self.typed_length.is_empty() && self.filleting() => vec![(next, format!("R {} (Enter)", self.typed_length.trim()))],
                Some(r) => vec![(next, format!("R {}", m.units.with_unit(r)))],
                None => Vec::new(),
            };
        }
        let Some(&last) = chain.last() else { return Vec::new() };
        if names.is_empty() {
            // The length and, once Tab has been pressed, the angle: the one being typed in brackets.
            let angled = self.drawing.field == 1 || !self.drawing.other.is_empty();
            let text = match (self.typed_length.is_empty(), angled) {
                (true, false) => m.units.with_unit(last.dist(next)),
                (false, false) => format!("{} (Enter)", self.typed_length.trim()),
                _ => {
                    let (len, ang) = if self.drawing.field == 1 {
                        (&self.drawing.other, &self.typed_length)
                    } else {
                        (&self.typed_length, &self.drawing.other)
                    };
                    let d = next - last;
                    let len = if len.is_empty() { m.units.with_unit(last.dist(next)) } else { len.trim().to_string() };
                    let ang = if ang.is_empty() {
                        format!("{:.0}°", d.1.atan2(d.0).to_degrees().rem_euclid(360.0))
                    } else {
                        format!("{}°", ang.trim())
                    };
                    match self.drawing.field {
                        1 => format!("{len}  [{ang}] (Enter)"),
                        _ => format!("[{len}]  {ang} (Enter)"),
                    }
                }
            };
            return vec![(next, text)];
        }
        let now = bimcraft_core::draw::reading(shape, chain, next);
        let field = self.drawing.field.min(names.len() - 1);
        let places: Vec<P2> = match (shape, chain) {
            (draw::Shape::Rectangle, &[a]) => vec![(a + P2(next.0, a.1)) * 0.5, (P2(next.0, a.1) + next) * 0.5],
            _ => vec![next],
        };
        let mut out = Vec::new();
        for (i, name) in names.iter().enumerate() {
            let typed = if i == field { &self.typed_length } else { &self.drawing.other };
            let shown = match now.get(i) {
                _ if !typed.is_empty() => typed.trim().to_string(),
                Some(v) if *name == "Angle" => degrees(*v),
                Some(v) if *name == "Radius" => format!("R {}", m.units.with_unit(*v)),
                Some(v) => m.units.with_unit(*v),
                None => continue,
            };
            let text = if i == field && (!typed.is_empty() || names.len() > 1) { format!("{shown} (Enter)") } else { shown };
            out.push((places.get(i).copied().unwrap_or(next), text));
        }
        out
    }

    /// A selected door's or window's two temporary dimensions: how far its middle stands from the faces of the walls
    /// meeting its wall either side of it, or its wall's ends. Typing one moves it along the wall; the other follows.
    fn opening_dimension(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect, id: Id, o: &bimcraft_core::model::Opening) {
        let Some((start, end)) = m.opening_span(o) else { return };
        let p = ui.painter_at(rect);
        for (n, (middle, text, _)) in opening_dimensions(&p, m, v, o).into_iter().enumerate() {
            let size = p.layout_no_wrap(text.clone(), FontId::proportional(12.0), pal().accent).size() + vec2(10.0, 4.0);
            let Some(typed) = self.dimension_number(ui, Rect::from_center_size(middle, size), (id, n), &text) else { continue };
            match typed.map(|t| (m.units.parse(&t), t)) {
                Some((Some(len), _)) if (0.0..=end - start).contains(&len) => {
                    let offset = if n == 0 { start + len } else { end - len };
                    if (offset - o.offset).abs() > 1e-6 {
                        self.run("modify", json!({"id": id, "offset": offset}));
                    }
                }
                Some((Some(_), _)) => self.status = Status { text: t("the opening has to stay on its wall").into(), error: true },
                Some((None, typed)) => self.status = Status { text: tf("`{}` is not a length", &[typed.trim()]), error: true },
                None => {}
            }
        }
    }

    /// A selected column's or component's temporary dimensions: how far it stands from the nearest wall or grid on each
    /// side along x and y. Typing one moves it, the others follow.
    fn standing_dimension(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect, id: Id, e: &Element) {
        let p = ui.painter_at(rect);
        for (n, middle, text, c) in standing_dimensions(&p, m, v, e) {
            let size = p.layout_no_wrap(text.clone(), FontId::proportional(12.0), pal().accent).size() + vec2(10.0, 4.0);
            let Some(typed) = self.dimension_number(ui, Rect::from_center_size(middle, size), (id, n), &text) else { continue };
            match typed.map(|t| (m.units.parse(&t), t)) {
                Some((Some(len), _)) if len >= 0.0 => {
                    let by = c.toward * (c.distance - len);
                    if by.length() > 1e-6 {
                        self.run("move", json!({"ids": [id], "by": point(by)}));
                    }
                }
                Some((_, typed)) => self.status = Status { text: tf("`{}` is not a length", &[typed.trim()]), error: true },
                None => {}
            }
        }
    }

    /// What the tool would do at pixel `q`: a chain's next segment, the room a click would bound,
    /// where a move, turn or mirror puts the selection, the line Align or Offset would use.
    fn preview(&self, sc: &Scene, q: (f32, f32), hovered: Option<Id>, shift: bool) {
        let (p, m, v, level, key) = (sc.p, sc.m, sc.v, sc.level, sc.key);
        let origin = p.clip_rect().min;
        let cursor = pos(origin, q);
        let stroke = Stroke::new(2.0, pal().accent);
        let thin = Stroke::new(1.0, pal().accent);
        let label = |at: Pos2, text: String| tag(p, at + vec2(12.0, -12.0), Align2::LEFT_BOTTOM, text);
        let dashed = |a: Pos2, b: Pos2| p.extend(Shape::dashed_line(&[a, b], thin, 8.0, 4.0));
        // An element a click would add, drawn as the plan would draw it.
        // What a dimension or a tag made here says it is in: the plan view, when the plan on screen is one.
        let in_view = || m.plan_view(key).map(|_| m.guid(key));
        let sketched = |e: &Element| ghost(p, Place::at(origin), &render::sketch(m, *v, m.next_id, e), &[m.next_id], &|pt| pt);
        let ghost = |f: &dyn Fn(Pos2) -> Pos2| ghost(p, sc.place, sc.d, &self.selection, f);
        let chain = match &self.tool {
            Tool::Wall(chain) | Tool::Railing(chain) | Tool::Mass(chain) | Tool::DetailLine(chain) | Tool::SketchLine(chain) => {
                chain.as_slice()
            }
            Tool::SketchPick => &[],
            // The group a click would place.
            Tool::DetailGroup => {
                if let Some(e) = self.group_at(m, key, self.found(sc, q, &[], false)) {
                    sketched(&e);
                }
                return;
            }
            &Tool::RepeatingDetail(Some(a)) => {
                if let Some(e) = self.repeat_to(m, key, a, self.found(sc, q, &[a], shift)) {
                    sketched(&e);
                }
                return;
            }
            Tool::RepeatingDetail(None) => &[],
            Tool::ExtrusionRoof(pts) if pts.len() < 2 => pts.as_slice(),
            // The roof a click would make, swept across to the cursor.
            Tool::ExtrusionRoof(pts) => {
                let mut with = pts.clone();
                with.push(self.found(sc, q, pts, false));
                if let Ok(r) = self.extrusion_roof(m, level, &with) {
                    sketched(&Element::Roof(r));
                }
                return;
            }
            // The edge a click would stand up, or slope.
            Tool::GableEdge => {
                if let Some((id, i)) = m.roof_edge(level, v.mm(q), f64::from(PICK) / v.scale)
                    && let Some(Element::Roof(r)) = m.elements.get(id)
                    && let Some((a, b)) = r.edges().nth(i)
                {
                    p.line_segment([sc.screen(a), sc.screen(b)], Stroke::new(4.0, pal().accent));
                }
                return;
            }
            Tool::Stair(pts) | Tool::Ramp(pts) if pts.is_empty() => &[],
            // The stair or the ramp a click would make, and how far up its first run goes.
            Tool::Stair(pts) | Tool::Ramp(pts) => {
                let ramp = matches!(self.tool, Tool::Ramp(_));
                let mut with = pts.clone();
                with.push(self.found(sc, q, pts, shift));
                if let Ok((e, _)) = self.flight(m, level, &with, ramp) {
                    sketched(&e);
                    let first = match &e {
                        Element::Stair(st) => {
                            st.landings.first().map(|l| tf("{} of {} risers", &[&l.at.to_string(), &m.flight(st).0.to_string()]))
                        }
                        Element::Ramp(r) => r.landings.first().map(|l| tf("{} up", &[&m.units.with_unit(l.at)])),
                        _ => None,
                    };
                    if let Some(text) = first.filter(|_| with.len() == 2) {
                        label(cursor, text);
                    }
                }
                return;
            }
            Tool::Grid(_)
            | Tool::ReferencePlane(_)
            | Tool::RoomSeparator(_)
            | Tool::Run(_, None)
            | Tool::Move(_)
            | Tool::Copy(_)
            | Tool::Section(None)
            | Tool::Shaft(None)
            | Tool::PatternCorner(None)
            | Tool::SlopeArrow(None)
            | Tool::Measure(_) => start_of(&self.tool),
            // The point a click would add, and its elevation.
            // The point a click would add, and how high above the level it would be.
            Tool::AddPoint => {
                let to = self.shape_found(sc, q, &[], shift);
                p.circle_stroke(sc.screen(to), 4.0, stroke);
                if let Some(z) = self.landing_height(to) {
                    label(cursor, m.units.length(z));
                }
                return;
            }
            // The split line a click would make, and how high its end would be.
            &Tool::AddSplitLine(Some(start)) => {
                let to = self.shape_found(sc, q, &[start], shift);
                p.line_segment([sc.screen(start), sc.screen(to)], stroke);
                if let Some(z) = self.landing_height(to) {
                    label(cursor, m.units.length(z));
                }
                return;
            }
            // Where a split line would start, and how high.
            Tool::AddSplitLine(None) => {
                let to = self.shape_found(sc, q, &[], shift);
                p.circle_stroke(sc.screen(to), 4.0, stroke);
                if let Some(z) = self.landing_height(to) {
                    label(cursor, m.units.length(z));
                }
                return;
            }
            // The section a click would end, and the elevation a click would place.
            &Tool::Section(Some(start)) => {
                let end = self.found(sc, q, &[start], shift);
                sketched(&Element::Section(Section {
                    name: m.free_view_name("Section", true),
                    start,
                    end,
                    depth: None,
                    crop: None,
                    reference: None,
                    mark: None,
                    graphics: Default::default(),
                }));
                return;
            }
            Tool::Elevation => {
                let at = round(v.mm(q));
                if let Some(line) = hovered.and_then(|id| bimcraft_core::elevation::reference(m, id, at))
                    && let Some((start, end)) = bimcraft_core::elevation::new_line(line, bimcraft_core::elevation::place(line, at))
                {
                    let name = m.free_view_name(m.language.word("Elevation"), true);
                    let reference = Some([line.0, line.1]);
                    // The edge it would look at, and the mark where it would stand.
                    p.line_segment([sc.screen(line.0), sc.screen(line.1)], stroke);
                    sketched(&Element::Elevation(Section {
                        name,
                        start,
                        end,
                        depth: None,
                        crop: None,
                        reference,
                        mark: None,
                        graphics: Default::default(),
                    }));
                }
                return;
            }
            Tool::Room => {
                if let Some(g) = geom::region_at(&m.room_lines(level, m.room_area), v.mm(q), TOL) {
                    p.add(Shape::closed_line(g.outer.iter().map(|pt| sc.screen(*pt)).collect(), stroke));
                    let (area, unit) = m.units.area(g.area());
                    label(cursor, format!("{area:.2} {unit}"));
                }
                return;
            }
            &Tool::Rotate { centre, start, place } => {
                // Placing the centre, the cross sits where a click would put it, with its snap's marker.
                let at = if place { self.found(sc, q, &[], false) } else { centre };
                let c = sc.screen(at);
                p.circle_stroke(c, 5.0, stroke);
                p.line_segment([c - vec2(8.0, 0.0), c + vec2(8.0, 0.0)], thin);
                p.line_segment([c - vec2(0.0, 8.0), c + vec2(0.0, 8.0)], thin);
                if let (Some(from), false) = (start, place) {
                    let angle = angle_between(centre, from, v.mm(q), self.snaps.angle);
                    p.line_segment([c, sc.screen(from)], thin);
                    p.line_segment([c, cursor], stroke);
                    ghost(&turned(c, angle));
                    label(cursor, format!("{}°", (angle * 100.0).round() / 100.0));
                }
                return;
            }
            &Tool::Mirror { draw, start } => {
                let axis = match (draw, start) {
                    (false, _) => hovered.and_then(|id| nearest_line(m, id, v.mm(q))).map(|(a, b)| (sc.screen(a), sc.screen(b))),
                    (true, Some(a)) => Some((sc.screen(a), sc.screen(self.found(sc, q, &[a], shift)))),
                    (true, None) => None,
                };
                if let Some((a, b)) = axis.filter(|(a, b)| a.distance(*b) > 1.0) {
                    dashed(a, b);
                    ghost(&mirrored(a, b));
                }
                return;
            }
            Tool::Align(reference) => {
                if let Some((a, b)) = reference {
                    // The reference, drawn on past its ends.
                    let (a, b) = (sc.screen(*a), sc.screen(*b));
                    let u = (b - a).normalized() * 2000.0;
                    dashed(a - u, b + u);
                }
                if let Some((_, line, _)) =
                    alignment_pick(m, key, self.sketch.as_ref(), *reference, hovered, v.mm(q), 2.0 * f64::from(PICK) / v.scale)
                {
                    p.line_segment([sc.screen(line.0), sc.screen(line.1)], Stroke::new(4.0, canvas_pal().hover));
                }
                return;
            }
            Tool::Split => {
                if hovered.is_some() {
                    let at = sc.screen(self.found(sc, q, &[], false));
                    p.line_segment([at - vec2(0.0, 9.0), at + vec2(0.0, 9.0)], stroke);
                    p.line_segment([at - vec2(9.0, 0.0), at + vec2(9.0, 0.0)], stroke);
                }
                return;
            }
            // The copy a click would make, on the cursor's side: of a wall, a detail line or a sketch line.
            Tool::Offset => {
                let d = m.units.parse(&self.offset).filter(|d| *d > 0.0);
                let at = v.mm(q);
                let line = match &self.sketch {
                    Some(sk) => sk.line_at(at, f64::from(PICK) / v.scale).and_then(|i| sk.lines.get(i)).map(|l| l.curve.clone()),
                    None => hovered
                        .filter(|id| matches!(m.elements.get(*id), Some(Element::DetailLine(_))))
                        .and_then(|id| draw::element_curve(m, id)),
                };
                if let (Some(c), Some(d)) = (line, d) {
                    if let Some(copy) = bimcraft_core::draw::pick(&c, at, d) {
                        let pts: Vec<Pos2> = copy.points().into_iter().map(|pt| sc.screen(pt)).collect();
                        p.extend(Shape::dashed_line(&pts, stroke, 8.0, 4.0));
                    }
                } else if let (Some(w), Some(d)) = (hovered.and_then(|id| m.wall(id)).filter(|w| w.arc == 0.0), d) {
                    let n = (w.end - w.start).unit().perp();
                    let by = n * if (at - w.start).dot(n) >= 0.0 { d } else { -d };
                    p.extend(Shape::dashed_line(&[sc.screen(w.start + by), sc.screen(w.end + by)], stroke, 8.0, 4.0));
                }
                return;
            }
            // The references and endpoints picked, the one under the cursor, else the dimension a click places.
            Tool::Dimension(mode, refs) => {
                let candidate = self
                    .dimension_reference_hover
                    .filter(|(id, _)| Some(*id) == hovered)
                    .map(|(_, r)| r)
                    .or_else(|| hovered.and_then(|id| dimension_pick(m, refs, id, v.mm(q), 2.0 * f64::from(PICK) / v.scale)));
                for r in refs.iter().chain(&candidate) {
                    match (r.point, m.reference_geometry(r)) {
                        (Some(_), Ok((a, _))) => {
                            let c = sc.screen(a);
                            p.rect_stroke(Rect::from_center_size(c, vec2(9.0, 9.0)), 0.0, stroke, StrokeKind::Middle);
                        }
                        (None, Ok((a, b))) => dashed(sc.screen(a), sc.screen(b)),
                        _ => {}
                    }
                }
                if candidate.is_none() && refs.len() >= 2 {
                    let at = round(v.mm(q));
                    let (kind, orient) = mode.kind(m, refs, at);
                    let type_name = self.dimension_type_for(kind).unwrap_or_default();
                    sketched(&Element::Dimension(Dimension {
                        level: m.guid(level),
                        view: in_view(),
                        kind,
                        orient,
                        refs: refs.clone(),
                        at,
                        type_name,
                        ..Dimension::default()
                    }));
                }
                return;
            }
            Tool::Tag => {
                if let Some(id) = hovered {
                    let label = Some(self.tag_label.trim().to_string()).filter(|l| !l.is_empty());
                    sketched(&Element::Tag(Tag {
                        host: m.guid(id),
                        view: in_view(),
                        offset: m.tag_offset(id),
                        leader: self.tag_leader,
                        label,
                    }));
                }
                return;
            }
            // The lines picked, the one under the cursor, else the arc a click places.
            Tool::Angular(refs) => {
                let candidate =
                    hovered.and_then(|id| reference_for(m, DimKind::Angular, refs.first(), id, v.mm(q), 2.0 * f64::from(PICK) / v.scale));
                for r in refs.iter().chain(candidate.as_ref().filter(|_| refs.len() < 2)) {
                    if let Ok((a, b)) = m.reference_line(r) {
                        dashed(sc.screen(a), sc.screen(b));
                    }
                }
                if refs.len() == 2 {
                    sketched(&Element::Dimension(Dimension {
                        level: m.guid(level),
                        view: in_view(),
                        kind: DimKind::Angular,
                        orient: None,
                        refs: refs.clone(),
                        at: round(v.mm(q)),
                        type_name: bimcraft_core::dims::default_type(&m.dimension_types, DimKind::Angular).cloned().unwrap_or_default(),
                        ..Dimension::default()
                    }));
                }
                return;
            }
            Tool::Round(kind, picked) if picked.len() == 1 || picked.len() == 3 => {
                sketched(&Element::Dimension(Dimension {
                    level: m.guid(level),
                    view: in_view(),
                    kind: *kind,
                    refs: picked.clone(),
                    at: round(v.mm(q)),
                    type_name: self.dimension_type_for(*kind).unwrap_or_default(),
                    ..Dimension::default()
                }));
                return;
            }
            Tool::Text if !self.note_text.trim().is_empty() => {
                let text = self.note_text.replace("\\n", "\n");
                let mut n = crate::notes::NoteEdit::new(m, key, round(v.mm(q)), self.note_type()).note;
                n.text = text;
                n.leader = self.note_arrow.filter(|(view, _)| *view == key).map(|(_, arrow)| arrow);
                sketched(&Element::Text(n));
                return;
            }
            Tool::Spot => {
                let at = round(v.mm(q));
                sketched(&Element::Spot(Spot {
                    view: m.guid(key),
                    at,
                    host: m.spot_host(level, at).map(|h| m.guid(h)),
                    slope: false,
                    coordinate: false,
                    origin: Default::default(),
                }));
                return;
            }
            Tool::SpotSlope => {
                let at = round(v.mm(q));
                sketched(&Element::Spot(Spot {
                    view: m.guid(key),
                    at,
                    host: m.spot_host(level, at).map(|h| m.guid(h)),
                    slope: true,
                    coordinate: false,
                    origin: Default::default(),
                }));
                return;
            }
            // The door or window a click would put in the wall, as the command makes it, and how far it stands from
            // the wall's ends.
            Tool::Door | Tool::Window => {
                let door = matches!(self.tool, Tool::Door);
                let params = self.opening_params(m, v, level, q, door);
                if let Ok(e) = opening_of(m, &params, door) {
                    sketched(&e);
                    if let Element::Door(o) | Element::Window(o) = &e {
                        for (at, text, _) in opening_dimensions(p, m, v, o) {
                            tag(p, at, Align2::CENTER_CENTER, text);
                        }
                    }
                }
                return;
            }
            Tool::Component | Tool::Fixture(_) => {
                // An adaptive instance shows only the points clicked for it (`adaptive_overlay`), until its last places it.
                if self.adaptive_count().is_none()
                    && let Some((family, type_name)) = self.placing()
                {
                    let at = self.found(sc, q, &[], false);
                    let angle = self.component_angle;
                    let c = Component {
                        adaptive: None,
                        family,
                        type_name,
                        level: m.guid(level),
                        at,
                        angle,
                        offset: 0.0,
                        mirrored: false,
                        params: Default::default(),
                        host: None,
                        panel: None,
                    };
                    let e = Element::Component(c);
                    sketched(&e);
                    for (_, at, text, _) in standing_dimensions(p, m, v, &e) {
                        tag(p, at, Align2::CENTER_CENTER, text);
                    }
                }
                return;
            }
            // The beam or wall footing a click would make, and how long it is.
            &Tool::Run(category, Some(start)) => {
                let end = self.found(sc, q, &[start], shift);
                for e in self.run_sketch(m, category, level, start, end) {
                    sketched(&e);
                }
                label(cursor, m.units.length(start.dist(end)));
                return;
            }
            // The structural column or footing a click would put down, and how far it stands from the walls.
            &Tool::Stand(category) => {
                let at = self.found(sc, q, &[], false);
                if let Some(e) = self.stand_sketch(m, category, level, at) {
                    sketched(&e);
                    for (_, at, text, _) in standing_dimensions(p, m, v, &e) {
                        tag(p, at, Align2::CENTER_CENTER, text);
                    }
                }
                return;
            }
            Tool::Column => {
                let at = self.found(sc, q, &[], false);
                let type_name =
                    self.types.get("column").cloned().or_else(|| m.default_type("column").map(String::from)).unwrap_or_default();
                let e = Element::Column(Column {
                    room_bounding: true,
                    params: Default::default(),
                    type_name,
                    level: m.guid(level),
                    at,
                    angle: 0.0,
                    top_level: None,
                    height: 0.0,
                    base_offset: 0.0,
                    top_offset: 0.0,
                    attach_top: None,
                    attach_base: None,
                });
                sketched(&e);
                for (_, at, text, _) in standing_dimensions(p, m, v, &e) {
                    tag(p, at, Align2::CENTER_CENTER, text);
                }
                return;
            }
            // The floor a click would make, under the walls, open over their courtyards.
            Tool::Floor => {
                if let Some((ring, courtyards)) = floor_outline(m, level) {
                    for r in std::iter::once(&ring).chain(&courtyards) {
                        p.add(Shape::closed_line(r.iter().map(|pt| sc.screen(*pt)).collect(), stroke));
                    }
                }
                return;
            }
            // The ceiling a click would make: the room's outline, out to its walls' faces, round the
            // walls standing inside it.
            Tool::Ceiling => {
                if let Some(g) = geom::region_at(&m.face_lines(level), v.mm(q), TOL) {
                    for r in std::iter::once(&g.outer).chain(&g.holes) {
                        p.add(Shape::closed_line(r.iter().map(|pt| sc.screen(*pt)).collect(), stroke));
                    }
                    let (area, unit) = m.units.area(g.area().abs());
                    label(cursor, format!("{area:.2} {unit}"));
                }
                return;
            }
            // The tile a click would close: the rectangle to the cursor.
            &Tool::PatternCorner(Some(a)) => {
                let b = self.found(sc, q, &[a], false);
                let c = [a, P2(b.0, a.1), b, P2(a.0, b.1)].map(|pt| sc.screen(pt));
                p.add(Shape::closed_line(c.to_vec(), stroke));
                return;
            }
            // The shaft a click would make: the rectangle to the cursor, crossed.
            &Tool::Shaft(Some(a)) => {
                let b = self.found(sc, q, &[a], false);
                let c = [a, P2(b.0, a.1), b, P2(a.0, b.1)].map(|pt| sc.screen(pt));
                p.add(Shape::closed_line(c.to_vec(), stroke));
                p.line_segment([c[0], c[2]], thin);
                p.line_segment([c[1], c[3]], thin);
                return;
            }
            // The slope arrow a click would make, and its slope.
            &Tool::SlopeArrow(Some(tail)) => {
                let (a, b) = (sc.screen(tail), sc.screen(self.found(sc, q, &[tail], shift)));
                p.line_segment([a, b], stroke);
                if a.distance(b) > 1.0 {
                    let (u, n) = ((b - a).normalized() * 10.0, (b - a).normalized().rot90() * 4.0);
                    p.add(Shape::convex_polygon(vec![b, b - u + n, b - u - n], pal().accent, Stroke::NONE));
                }
                label(cursor, format!("{}°", self.floor_slope.trim()));
                return;
            }
            // The roof a click would make, with its ridge and hips.
            Tool::Roof => {
                let on = m.level_above(level).unwrap_or(m.guid(level));
                if let (Some(slope), Some(overhang)) = (number(&self.roof_slope), m.units.parse(&self.roof_overhang))
                    && let Ok((boundary, courtyards)) = roof_footprint(m, on, overhang)
                {
                    let type_name =
                        self.types.get("roof").cloned().or_else(|| m.default_type("roof").map(String::from)).unwrap_or_default();
                    sketched(&Element::Roof(Roof {
                        type_name,
                        level: on,
                        boundary,
                        courtyards,
                        slope,
                        overhang,
                        gable: self.roof_gable,
                        ..Roof::default()
                    }));
                }
                return;
            }
            _ => return,
        };
        let square = shift || (self.constrain && matches!(self.tool, Tool::Move(_) | Tool::Copy(_)));
        // A shape other than a line: guided from its last point only, and its third point from none.
        let shape = self.shape();
        let shaped = self.target().is_some() && !matches!(shape, draw::Shape::Line | draw::Shape::Polyline);
        let guide = match (shaped, chain) {
            (false, _) => chain,
            (true, [.., last]) if chain.len() < 2 || shape.open_ended() => std::slice::from_ref(last),
            (true, _) => &[],
        };
        let next = self.found(sc, q, guide, square);
        if shaped {
            for c in self.shape_preview(next) {
                p.add(Shape::line(c.points().into_iter().map(|pt| sc.screen(pt)).collect(), stroke));
            }
            if let (Some(&last), true) =
                (chain.last(), matches!(shape, draw::Shape::Circle | draw::Shape::Inscribed | draw::Shape::Circumscribed))
            {
                p.extend(ruler_shapes(sc, last, next));
            }
            self.shape_dimensions(sc, chain, next);
            return;
        }
        if let Tool::Move(Some(base)) | Tool::Copy(Some(base)) = &self.tool {
            let delta = sc.screen(next) - sc.screen(*base);
            ghost(&|pt| pt + delta);
        }
        // A line's walls are built as it goes: only the next one is to come.
        // Walls, and detail lines a click, are made as they go: only the next one is still a preview.
        let one_by_one = matches!(self.tool, Tool::Wall(_) | Tool::DetailLine(_)) && shape == draw::Shape::Line && !self.one_line();
        let built = if one_by_one { chain.len().saturating_sub(1) } else { 0 };
        let mut pts: Vec<Pos2> = chain.iter().skip(built).map(|pt| sc.screen(*pt)).collect();
        pts.push(sc.screen(next));
        p.add(Shape::line(pts, stroke));
        // The mass a click would close: its outline as the plan draws it, and its area.
        if let (Tool::Mass(_), true) = (&self.tool, chain.len() >= 2) {
            let ring: Vec<P2> = chain.iter().copied().chain([next]).collect();
            sketched(&Element::Mass(Mass {
                name: String::new(),
                level: m.guid(level),
                boundary: ring.clone(),
                top: None,
                top_level: None,
                height: 1.0,
                base_offset: 0.0,
                params: Default::default(),
            }));
            let (area, unit) = m.units.area(geom::area(&ring).abs());
            let middle = ring.iter().fold(P2(0.0, 0.0), |a, q| a + *q * (1.0 / ring.len() as f64));
            label(sc.screen(middle), format!("{area:.2} {unit}"));
        }
        // The ruler along the new line, and an arc at the corner it turns from the line before.
        if let Some(&last) = chain.last() {
            p.extend(ruler_shapes(sc, last, next));
            if let [.., prev, l] = chain
                && let Some(a) = corner_arc(sc, *prev, *l, next)
            {
                p.add(a);
            }
        }
        // The listening dimension: the length so far and the angle (at the corner it turns, else from
        // east), or the length being typed. (An arc's radius is its shape's: `shape_texts`.)
        let reading = |last: P2| {
            let angle = match chain {
                [.., prev, l] => snap::corner(*prev, *l, next),
                _ => None,
            };
            format!("{}  {}", m.units.with_unit(last.dist(next)), degrees(angle.unwrap_or_else(|| snap::ruler(last, next).angle)))
        };
        match (chain.last(), self.typed_length.is_empty()) {
            (Some(_), false) => label(sc.screen(next), format!("{} (Enter)", self.typed_length.trim())),
            (Some(last), true) => label(sc.screen(next), reading(*last)),
            (None, _) => {}
        }
    }
}

impl BimApp {
    /// Under Modify Sub Elements, a drag from one of the selected floor's shape points moves it in plan, snapped, at its
    /// height, the split lines ending there going with it; on release, one `edit_shape`. Whether it took the release, which is
    /// then no click.
    pub(crate) fn shape_grip_drag(
        &mut self,
        ui: &Ui,
        response: &Response,
        rect: Rect,
        view: &bimcraft_core::render::View,
        level: Id,
    ) -> bool {
        let floor = self.selected_shapeable().filter(|_| self.tool == Tool::ModifySubElements);
        let Some(f) = floor.and_then(|id| self.session.model.shapeable(id).ok()) else {
            if self.shape_grip.take().is_some() {
                self.shape_preview = None;
            }
            return false;
        };
        if response.drag_started_by(PointerButton::Primary)
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
        {
            self.shape_grip =
                f.shape.point_near(view.mm((o.x - rect.min.x, o.y - rect.min.y)), f64::from(crate::section_box::GRIP) / view.scale);
        }
        let Some(from) = self.shape_grip else { return false };
        if !response.drag_stopped() {
            // While it's dragged, the floor as it would be with the point where the cursor snaps.
            if let Some(q) = ui.input(|i| i.pointer.latest_pos()) {
                let m = std::sync::Arc::clone(&self.session.model);
                let to = self.snap(&m, level, view, (q.x - rect.min.x, q.y - rect.min.y), &[], false).at;
                self.shape_preview = Some(Floor { shape: f.shape.moved(from, to), ..f });
            }
            return false;
        }
        (self.shape_grip, self.shape_preview) = (None, None);
        let Some(q) = ui.input(|i| i.pointer.latest_pos()) else { return true };
        let m = std::sync::Arc::clone(&self.session.model);
        let to = self.snap(&m, level, view, (q.x - rect.min.x, q.y - rect.min.y), &[], false).at;
        self.snap_once = None;
        if to.dist(from) <= TOL {
            return true;
        }
        if let Some(id) = floor {
            self.edit_shape(id, &f.shape.moved(from, to));
        }
        true
    }

    /// While a floor is shaped, each of its points' heights beside it, as the Options Bar's Elevation reads them (relative,
    /// or with Absolute above the level); under Modify Sub Elements, a grip on each, the one held at the cursor.
    pub(crate) fn shape_overlay(&mut self, ui: &mut Ui, m: &Model, view: &bimcraft_core::render::View, rect: Rect, level: Id) {
        self.shape_label_rects.clear();
        let Some(f) = self.shaping().and_then(|id| m.shapeable(id).ok()) else {
            self.shape_label_edit = None;
            self.shape_picked.clear();
            return;
        };
        let f = &f;
        let p = ui.painter_at(rect);
        let at = |q: P2| {
            let (x, y) = view.px(q);
            rect.min + vec2(x, y)
        };
        let cursor = self
            .shape_grip
            .and_then(|_| ui.input(|i| i.pointer.latest_pos()))
            .map(|q| self.snap(m, level, view, (q.x - rect.min.x, q.y - rect.min.y), &[], false).at);
        // Its points, edges, split lines and creases as edit mode colours them; the grip under the cursor lit, the selection
        // thick.
        let marks = crate::shape::marks(m, f);
        let hover = ui.input(|i| i.pointer.hover_pos()).filter(|h| rect.contains(*h));
        // Under Modify Sub Elements, what a click there would pick, a point or an edge; under the others, the point.
        let lit = hover.filter(|_| self.shape_grip.is_none()).and_then(|h| {
            if self.tool == Tool::ModifySubElements {
                let q = view.mm((h.x - rect.min.x, h.y - rect.min.y));
                crate::shape::Sub::of(&bimcraft_core::view3d::shape_pick_plan(m, f, q, 2.0 * f64::from(PICK) / view.scale))
            } else {
                let near = marks.points.iter().map(|(q, ..)| (*q, at(*q).distance(h))).filter(|(_, d)| *d <= 8.0);
                near.min_by(|a, b| a.1.total_cmp(&b.1)).map(|(q, _)| crate::shape::Sub::Point(q))
            }
        });
        // Modify Sub Elements' selection, shown only while it's on.
        let picked = if self.tool == Tool::ModifySubElements { self.shape_picked.clone() } else { Vec::new() };
        crate::shape::draw_marks(&p, &marks, |a| at(P2(a[0], a[1])), &picked, lit);
        // The creases the drag would leave, dashed over the ones there are.
        for [a, b] in self.shape_preview.as_ref().map(|f| m.shape_creases(f)).unwrap_or_default() {
            p.extend(egui::Shape::dashed_line(&[at(P2(a[0], a[1])), at(P2(b[0], b[1]))], Stroke::new(1.5, pal().accent), 6.0, 4.0));
        }
        // Each point's height beside it, and the selection's (an edge's at its middle): a click on one types it. A point held
        // follows the cursor.
        let mut labels: Vec<(P2, Vec<P2>)> = f.shape.points.iter().map(|x| (x.at, vec![x.at])).collect();
        for s in picked {
            if !labels.iter().any(|(q, _)| q.dist(s.at()) <= TOL) {
                labels.push((s.at(), s.ends()));
            }
        }
        for (q, ends) in labels {
            let text = self.heights_text(f, &ends);
            let held = self.shape_grip.is_some_and(|g| g.dist(q) <= TOL);
            if held {
                let c = at(cursor.unwrap_or(q));
                p.circle_filled(c, 5.5, pal().accent);
                tag(&p, c + vec2(8.0, -8.0), Align2::LEFT_BOTTOM, text);
            } else {
                let r = self.shape_label(ui, at(q) + vec2(8.0, -8.0), q, &ends, &text);
                self.shape_label_rects.push(r);
            }
        }
    }
}

/// A snap's marker at `at` on screen, by what it snapped to (`kind`), its guide dashed from `from` (the point it runs from,
/// on screen) on past it, and `name` beside it: the plan's and a family's sketch's alike.
pub(crate) fn snap_mark(p: &Painter, at: Pos2, from: Option<Pos2>, kind: Kind, name: String) {
    let (stroke, thin) = (Stroke::new(2.0, pal().accent), Stroke::new(1.0, pal().accent));
    if let Some(a) = from
        && a.distance(at) > 1.0
    {
        // Back to the point it runs from, but no further than a screen's width: past that it can't be seen, and a point far
        // off would make a dash for every 10 px.
        let u = (at - a).normalized();
        let back = a.distance(at).min(4000.0) + 24.0;
        p.extend(Shape::dashed_line(&[at - u * back, at + u * 28.0], thin, 6.0, 4.0));
    }
    match kind {
        Kind::Endpoint => {
            p.rect_stroke(Rect::from_center_size(at, vec2(11.0, 11.0)), 0.0, stroke, StrokeKind::Middle);
        }
        Kind::Midpoint => {
            p.add(Shape::closed_line(vec![at + vec2(0.0, -6.0), at + vec2(6.0, 5.0), at + vec2(-6.0, 5.0)], stroke));
        }
        Kind::Intersection => {
            p.line_segment([at + vec2(-6.0, -6.0), at + vec2(6.0, 6.0)], stroke);
            p.line_segment([at + vec2(-6.0, 6.0), at + vec2(6.0, -6.0)], stroke);
        }
        Kind::Centre => {
            p.circle_stroke(at, 6.0, stroke);
        }
        // A right angle's foot: a line, and one standing square on it.
        Kind::Perpendicular => {
            p.line_segment([at + vec2(-7.0, 5.0), at + vec2(7.0, 5.0)], stroke);
            p.line_segment([at + vec2(0.0, 5.0), at + vec2(0.0, -6.0)], stroke);
        }
        Kind::Nearest => {
            p.add(Shape::closed_line(vec![at + vec2(0.0, -6.0), at + vec2(6.0, 0.0), at + vec2(0.0, 6.0), at + vec2(-6.0, 0.0)], stroke));
        }
        // A scan's point: a cross, upright, round a dot.
        Kind::Point => {
            p.line_segment([at + vec2(-7.0, 0.0), at + vec2(7.0, 0.0)], stroke);
            p.line_segment([at + vec2(0.0, -7.0), at + vec2(0.0, 7.0)], stroke);
            p.circle_filled(at, 2.5, pal().accent);
        }
        // A quarter of a round, its corner on the point.
        Kind::Quadrant => {
            let arc: Vec<Pos2> = (0..=8)
                .map(|k| f32::from(k as u8) * std::f32::consts::FRAC_PI_2 / 8.0)
                .map(|a| at + vec2(-9.0 + 12.0 * a.cos(), 6.0 - 12.0 * a.sin()))
                .collect();
            p.add(Shape::line(arc, stroke));
            p.line_segment([at + vec2(-9.0, 6.0), at + vec2(3.0, 6.0)], stroke);
            p.line_segment([at + vec2(-9.0, 6.0), at + vec2(-9.0, -6.0)], stroke);
        }
        // A round, and the line touching it on top.
        Kind::Tangent => {
            p.circle_stroke(at + vec2(0.0, 5.0), 5.0, stroke);
            p.line_segment([at + vec2(-8.0, 0.0), at + vec2(8.0, 0.0)], stroke);
        }
        Kind::Anchor => {
            p.circle_stroke(at, 6.0, stroke);
            p.circle_filled(at, 2.0, pal().accent);
        }
        Kind::Angle => {
            p.circle_filled(at, 3.0, pal().accent);
        }
        Kind::Free => {
            p.rect_stroke(Rect::from_center_size(at, vec2(8.0, 8.0)), 0.0, stroke, StrokeKind::Middle);
        }
    }
    if !matches!(kind, Kind::Free | Kind::Angle) {
        tag(p, at + vec2(12.0, 14.0), Align2::LEFT_TOP, name);
    }
}
