//! The plan canvas: a level's floor plan as `render::draw` lays it out (its reflected ceiling plan as
//! `render::draw_ceiling` does), panned and zoomed. Over it:
//! selection (click, Ctrl adds, Shift takes away, Tab for what's underneath, boxes), dragging,
//! the tools' points, temporary dimensions, and previews of what a click would do.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

use bimcraft_core::geom::{self, P2};
use bimcraft_core::model::{
    Clearance, Column, Component, DEFAULT_HEIGHT, DimKind, Dimension, Element, Extrusion, Face, Id, Landing, Line, MAX_COORD, Mass, Model,
    NOTE_SIZE, Note, RISER, Ramp, Reference, Roof, Section, Spot, Stair, TOL, TREAD, Tag, Wall,
};
use bimcraft_core::render::{self, Drawing, Prim, Rgb, View};
use bimcraft_core::snap::{self, Found, Kind, Reach};
use bimcraft_core::walls::{self, Axis};
use bimcraft_core::{floor_outline, opening_of, roof_footprint};
use egui::{
    Align, Align2, Color32, Event, FontId, Key, Layout, Mesh, Modifiers, Painter, PointerButton, Pos2, Rect, Response, Sense, Shape,
    Stroke, StrokeKind, TextEdit, TextureId, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::properties::number;
use crate::style::{Icon, pal};
use crate::{BimApp, Sheet, Status, Tool, icon_button, top_params};

/// How near the cursor an element is picked, in px.
pub(crate) const PICK: f32 = 6.0;
/// How near the cursor a click snaps to a wall's end or a point of the chain, in px.
const SNAP: f64 = 10.0;
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

/// Geometry does not change when the pointer, selection, tool or theme changes.
/// Keep one drawing per canvas, bounded to its latest model and framing.
#[derive(Default, Clone)]
pub(crate) struct PlanCache {
    shown: Option<PlanShot>,
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
}

pub struct Canvas {
    drawing: PlanCache,
    cams: BTreeMap<Id, Camera>,
    /// Where the plan was last shown, in points.
    rect: Rect,
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
}

impl Default for Canvas {
    fn default() -> Canvas {
        Canvas {
            drawing: PlanCache::default(),
            cams: BTreeMap::new(),
            rect: Rect::ZERO,
            level: None,
            cursor: None,
            grab: None,
            drag: None,
            under: Vec::new(),
            cycle: 0,
            crop_drag: None,
            grip: None,
            level_grip: None,
            press: None,
            lifted: None,
            prior: None,
            found: Cell::new(None),
        }
    }
}

impl Canvas {
    fn view(&self, level: Id) -> Option<View> {
        let c = self.cams.get(&level)?;
        Some(View {
            scale: c.scale,
            cx: c.cx,
            cy: c.cy,
            width: self.rect.width().max(1.0) as u32,
            height: self.rect.height().max(1.0) as u32,
        })
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
            let v = fit(rect.width().max(1.0) as u32, rect.height().max(1.0) as u32);
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
/// so ink reads light on a dark ground and the grid stays blue.
pub(crate) fn color(c: Rgb) -> Color32 {
    if !crate::style::dark() {
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

fn pos(origin: Pos2, (x, y): (f32, f32)) -> Pos2 {
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
}

impl Place {
    pub(crate) fn at(origin: Pos2) -> Place {
        Place { origin, k: 1.0, paper: false }
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

/// The display list, with the selected and hovered elements in the accent colours; with `thin`,
/// every line one pixel wide. Its pictures are `pictures`, in order.
pub(crate) fn paint(p: &Painter, at: Place, d: &Drawing, selected: &[Id], hovered: Option<Id>, thin: bool, pictures: &[TextureId]) {
    let mut pictures = pictures.iter();
    let color = |c: Rgb| if at.paper { Color32::from_rgb(c.0, c.1, c.2) } else { color(c) };
    for (prim, owner) in d.prims.iter().zip(&d.owners) {
        let accent = match *owner {
            Some(id) if selected.contains(&id) => Some((pal().accent, pal().select_fill)),
            Some(id) if Some(id) == hovered => Some((pal().hover, pal().hover_fill)),
            _ => None,
        };
        match prim {
            // A convex ring alone, smooth-edged; else (a curved wall's outline, walls' union) cut into
            // triangles, which meet without a seam. No holes: no fill has one.
            Prim::Fill { rings, color: c } => {
                let fill = accent.map_or(color(*c), |a| a.1);
                match rings.as_slice() {
                    [r] if convex(r) => {
                        p.add(Shape::convex_polygon(r.iter().map(|q| at.pos(*q)).collect(), fill, Stroke::NONE));
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
                        p.add(Shape::mesh(mesh));
                    }
                }
            }
            Prim::Image { at: corner, width, height, .. } => {
                if let Some(texture) = pictures.next() {
                    let rect = Rect::from_min_size(at.pos(*corner), vec2(*width as f32, *height as f32) * at.k);
                    let tint = accent.map_or(Color32::WHITE, |a| a.1);
                    p.image(*texture, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
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
                        p.extend(Shape::dashed_line_with_offset(&pts, stroke, &on, &off, 0.0));
                    }
                    None if *closed => {
                        p.add(Shape::closed_line(pts, stroke));
                    }
                    None => {
                        p.add(Shape::line(pts, stroke));
                    }
                }
            }
        }
    }
}

/// The selected elements' outlines taken by `f` (on screen): where a move, a turn or a mirror
/// puts them.
fn ghost(p: &Painter, origin: Pos2, d: &Drawing, selected: &[Id], f: &dyn Fn(Pos2) -> Pos2) {
    let stroke = Stroke::new(1.0, pal().accent);
    let at = |q: &(f32, f32)| f(pos(origin, *q));
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
fn tag(p: &Painter, at: Pos2, align: Align2, text: impl Into<String>) {
    let galley = p.layout_no_wrap(text.into(), FontId::proportional(12.0), pal().accent);
    let rect = align.anchor_size(at, galley.size());
    p.rect_filled(rect.expand2(vec2(3.0, 1.0)), 2.0, pal().surface.gamma_multiply(0.85));
    p.galley(rect.min, galley, pal().accent);
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

/// An opening's temporary dimensions, drawn on `p` beside its host wall: from the wall's start to the middle of the
/// opening and from there to the wall's end, each as where its number goes, its text, and its length in mm. None on a
/// curved wall, and for a span too short to draw.
/// ponytail: measured along the wall's location line to its ends; Revit stops at the nearest wall crossing it.
pub(crate) fn opening_dimensions(p: &Painter, m: &Model, v: &View, o: &bimcraft_core::model::Opening) -> Vec<(Pos2, String, f64)> {
    let Some(w) = m.wall(o.host).filter(|w| w.arc == 0.0) else { return Vec::new() };
    let (origin, c) = (p.clip_rect().min, Axis::of(w).at(o.offset, 0.0));
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
    [(w.start, c), (c, w.end)]
        .into_iter()
        .filter(|(s, e)| screen(*s).distance(screen(*e)) >= 24.0)
        .map(|(s, e)| (dimension_line(p, screen(s), screen(e), n, off), m.units.length(s.dist(e)), s.dist(e)))
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
    level: Id,
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
        Tool::Grid(start) | Tool::Move(start) | Tool::Copy(start) | Tool::Section(start) | Tool::Measure(start) | Tool::Run(_, start) => {
            start.as_slice()
        }
        _ => &[],
    }
}

/// A selection box from pixel `from` to `q` of a view in `rect`: Revit draws a crossing box (right
/// to left) dashed, a window box solid.
fn box_shapes(rect: Rect, from: (f32, f32), q: (f32, f32)) -> Vec<Shape> {
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
        self.canvas.lifted.filter(|(_, quick)| *quick || self.tool != Tool::Modify).map(|(at, _)| at)
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
        snap::snap(m, level, v.mm(at), reach, from, square, &self.snaps_now())
    }

    /// What a pick at pixel `q` would land on, shown: its marker and what it is, and the guide it
    /// follows. The point.
    fn found(&self, sc: &Scene, q: (f32, f32), from: &[P2], square: bool) -> P2 {
        let f = self.snap(sc.m, sc.level, sc.v, q, from, square);
        self.show_snap(sc, &f);
        self.canvas.found.set(Some(f));
        f.at
    }

    /// A snap's guide, dashed through the point it runs from and on past the snapped one; its
    /// marker, by what it snapped to; and its name.
    fn show_snap(&self, sc: &Scene, f: &Found) {
        let (p, at) = (sc.p, sc.screen(f.at));
        let (stroke, thin) = (Stroke::new(2.0, pal().accent), Stroke::new(1.0, pal().accent));
        if let Some(g) = f.guide {
            let a = sc.screen(g.from);
            if a.distance(at) > 1.0 {
                // Back to the point it runs from, but no further than a screen's width: past that it
                // can't be seen, and a point far off would make a dash for every 10 px.
                let u = (at - a).normalized();
                let back = a.distance(at).min(4000.0) + 24.0;
                p.extend(Shape::dashed_line(&[at - u * back, at + u * 28.0], thin, 6.0, 4.0));
            }
        }
        match f.kind {
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
                p.add(Shape::closed_line(
                    vec![at + vec2(0.0, -6.0), at + vec2(6.0, 0.0), at + vec2(0.0, 6.0), at + vec2(-6.0, 0.0)],
                    stroke,
                ));
            }
            Kind::Angle => {
                p.circle_filled(at, 3.0, pal().accent);
            }
            Kind::Free => {
                p.rect_stroke(Rect::from_center_size(at, vec2(8.0, 8.0)), 0.0, stroke, StrokeKind::Middle);
            }
        }
        if !matches!(f.kind, Kind::Free | Kind::Angle) {
            tag(p, at + vec2(12.0, 14.0), Align2::LEFT_TOP, t(f.kind.name()));
        }
    }

    /// The plan of `level`, filling `ui`.
    pub(crate) fn canvas(&mut self, ui: &mut Ui, level: Id) {
        // Clicks and drags; never the keyboard, which the shortcuts need.
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::CLICK | Sense::DRAG);
        let model = self.view_model();
        let up = matches!(self.sheet, Sheet::Ceiling(_));
        let fit = |w, h| if up { render::ceiling_plan(&model, level, w, h).view } else { render::plan(&model, level, w, h).view };
        let Some((view, local)) = self.canvas.navigate(ui, &response, rect, level, fit) else { return };
        let mods = ui.input(|i| i.modifiers);
        let c = &mut self.canvas;
        c.cursor = local.map(|q| view.mm(q));
        let d = c.drawing.drawing(&model, level, view, up);

        // What's under the cursor; Tab steps through it. (Without the app's input hook, as in
        // tests, egui moved the keyboard to a widget on that Tab: give it back.)
        let under = match (&self.tool, local) {
            (Tool::Modify, Some(q)) => d.pick_all(q, PICK),
            _ => Vec::new(),
        };
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
        if (std::mem::take(&mut self.tab_cycle) || tab) && !c.under.is_empty() {
            c.cycle = (c.cycle + 1) % c.under.len();
        }
        // How near the cursor a dimension's reference line must be, mm.
        let near = 2.0 * f64::from(PICK) / view.scale;
        // What the tool in hand would take at pixel `q`: under the cursor for the highlight, under
        // the press for a click, which the hand may have left by the time the button came up.
        // (Modify, here, is what is drawn nearest; Tab cycles through what is under the cursor.)
        let tool = self.tool.clone();
        let under_at = |q: (f32, f32)| match &tool {
            Tool::Modify => d.pick(q, PICK),
            Tool::Door | Tool::Window | Tool::Trim(_) | Tool::Split | Tool::Offset => {
                d.pick(q, PICK).filter(|id| model.wall(*id).is_some())
            }
            Tool::Demolish => d.pick(q, PICK),
            Tool::Align(_) | Tool::Mirror { draw: false, .. } => d.pick(q, PICK).filter(|id| !model.reference_lines(*id).is_empty()),
            // Something to measure to: with a line parallel to the first one picked, or for an angle,
            // one across it.
            Tool::Dimension(refs) => d
                .pick_all(q, PICK)
                .into_iter()
                .find(|id| reference_for(&model, DimKind::Aligned, refs.first(), *id, view.mm(q), near).is_some()),
            Tool::Angular(refs) if refs.len() < 2 => d
                .pick_all(q, PICK)
                .into_iter()
                .find(|id| reference_for(&model, DimKind::Angular, refs.first(), *id, view.mm(q), near).is_some()),
            Tool::Radial(None) => d
                .pick_all(q, PICK)
                .into_iter()
                .find(|id| model.reference_arc(&Reference { id: model.guid(*id), face: Face::Centre }).is_ok()),
            // What a spot elevation would read.
            Tool::Spot => model.spot_host(level, view.mm(q)),
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

        let crop = self.crop_grips(ui, &response, rect, &view, level);
        self.canvas.found.set(None);
        self.grip_drag(ui, &response, rect, &view, level);
        // A drag from an element moves the selection; from empty floor, it draws a selection box.
        if response.drag_started_by(PointerButton::Primary)
            && self.tool == Tool::Modify
            && self.canvas.crop_drag.is_none()
            && self.canvas.grip.is_none()
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
                    let by = round(view.mm(q) - from);
                    if by.length() > 0.0 {
                        self.run("move", json!({"ids": self.selection, "by": point(by)}));
                    }
                }
                Drag::Box(from) => self.pick_up(d.in_box(from, q, q.0 < from.0), click_modifiers(ui)),
            }
        }
        // A double click on a section's or an elevation's mark opens its view, as in Revit.
        if response.double_clicked()
            && let Some(id) = hovered.filter(|id| model.section(*id).is_some())
        {
            self.open_sheet(Sheet::Section(id));
            return;
        }
        if let Some(at) = self.clicked_at(&response) {
            let q = (at.x - rect.min.x, at.y - rect.min.y);
            // What lay under the press, which the hand may have left by the time the button came up.
            let found = if Some(q) == local { hovered } else { under_at(q) };
            self.click(&model, level, &view, q, found, click_modifiers(ui));
            // A snap override typed was for that point.
            self.snap_once = None;
        }

        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, color(render::PAPER));
        let mut marked = self.selection.clone();
        if let Tool::Trim(Some((first, _))) = self.tool {
            marked.push(first);
        }
        paint(&p, Place::at(rect.min), &d, &marked, hovered, self.thin, &[]);
        p.extend(crop);
        let rooms = hovered.iter().map(|id| (*id, pal().hover)).chain(self.selection.iter().map(|id| (*id, pal().accent)));
        for (id, accent) in rooms {
            if let Some(Element::Room(r)) = model.elements.get(id)
                && let Some(g) = model.room_region(r)
            {
                p.add(Shape::closed_line(g.outer.iter().map(|q| pos(rect.min, view.px(*q))).collect(), Stroke::new(2.0, accent)));
            }
        }
        match (self.canvas.drag, local) {
            (Some(Drag::Move(from)), Some(q)) => {
                let (a, b) = (view.px(from), view.px(from + round(view.mm(q) - from)));
                ghost(&p, rect.min, &d, &self.selection, &|q| q + vec2(b.0 - a.0, b.1 - a.1));
            }
            (Some(Drag::Box(from)), Some(q)) => p.extend(box_shapes(rect, from, q)),
            _ => {}
        }
        if let Some(q) = local {
            let scene = Scene { p: &p, m: &model, d: &d, level, v: &view };
            self.preview(&scene, q, hovered, mods.shift);
        }
        // The Measure tool's ruler, as laid.
        if let (Tool::Measure(_), Some((a, b))) = (&self.tool, self.measured) {
            let scene = Scene { p: &p, m: &model, d: &d, level, v: &view };
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
        self.grip_overlay(ui, &model, &view, rect, level, &d);
        self.temporary_dimension(ui, &model, &view, rect);
        self.context_menu(&response);
        self.nav_bar(ui, rect, level);
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
        let bar = Rect::from_min_size(rect.right_top() + vec2(-46.0, 14.0), vec2(32.0, 80.0));
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
        let mut shapes = vec![Shape::closed_line(ring, Stroke::new(1.0, if held.is_some() { pal().accent } else { pal().muted }))];
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
    pub(crate) fn level_grips(&mut self, ui: &Ui, response: &Response, rect: Rect, view: &View, id: Id) -> Vec<Shape> {
        let m = std::sync::Arc::clone(&self.session.model);
        let level = match self.selection.as_slice() {
            &[l] if self.tool == Tool::Modify => m.level(l).map(|lv| (l, lv.elevation)),
            _ => None,
        };
        let Some(((level, z), (a, b))) = level.and_then(|(l, z)| Some(((l, z), render::level_ends(&m, l, id)?))) else {
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

    fn click(&mut self, m: &Model, level: Id, v: &View, at: (f32, f32), hovered: Option<Id>, mods: Modifiers) {
        match &self.tool {
            Tool::Modify => self.pick_up(hovered.into_iter().collect(), mods),
            Tool::Door | Tool::Window => {
                let cat = if matches!(self.tool, Tool::Door) { "door" } else { "window" };
                let p = self.typed(cat, json!({"at": point(round(v.mm(at))), "level": level}));
                self.run(cat, p);
            }
            Tool::Room => {
                self.run("room", json!({"point": point(round(v.mm(at))), "level": level}));
            }
            Tool::Level => {}
            Tool::Elevation => {
                self.run("elevation", json!({"at": point(round(v.mm(at)))}));
            }
            Tool::Wall(_) | Tool::Grid(_) | Tool::Move(_) | Tool::Copy(_) | Tool::Section(_) | Tool::Run(..) => {
                let chain: Vec<P2> = match &self.tool {
                    Tool::Wall(chain) => chain.clone(),
                    Tool::Grid(start) | Tool::Move(start) | Tool::Copy(start) | Tool::Section(start) | Tool::Run(_, start) => {
                        start.iter().copied().collect()
                    }
                    _ => Vec::new(),
                };
                // The point an arc passes through follows no guide from the points before it.
                let chain = if self.wall_arc && matches!(self.tool, Tool::Wall(_)) && chain.len() == 2 { Vec::new() } else { chain };
                let square = mods.shift || (self.constrain && matches!(self.tool, Tool::Move(_) | Tool::Copy(_)));
                self.place(level, self.snap(m, level, v, at, &chain, square).at);
            }
            &Tool::Rotate { centre, start, place } => {
                self.tool = match (place, start) {
                    (true, _) => Tool::Rotate { centre: self.snap(m, level, v, at, &[], false).at, start: None, place: false },
                    (false, None) => Tool::Rotate { centre, start: Some(v.mm(at)), place: false },
                    (false, Some(from)) => return self.turn(centre, angle_between(centre, from, v.mm(at), self.snaps.angle)),
                };
            }
            // A ruler: its start, then its end, which stays on show until Esc or another start.
            &Tool::Measure(first) => {
                let p = self.snap(m, level, v, at, first.as_slice(), mods.shift).at;
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
                let p = self.snap(m, level, v, at, start.as_slice(), mods.shift).at;
                match start {
                    None => self.tool = Tool::Mirror { draw: true, start: Some(p) },
                    Some(a) => self.reflect((a, p)),
                }
            }
            &Tool::Align(reference) => match (reference, hovered.and_then(|id| Some((id, nearest_line(m, id, v.mm(at))?)))) {
                (None, Some((_, line))) => self.tool = Tool::Align(Some(line)),
                (Some(r), Some((id, line))) => self.align(r, id, line),
                _ => {}
            },
            &Tool::Trim(first) => match (first, hovered) {
                (None, Some(a)) => self.tool = Tool::Trim(Some((a, v.mm(at)))),
                (Some((a, keep)), Some(b)) if b != a => {
                    self.run("corner", json!({"ids": [a, b], "keep": [point(keep), point(v.mm(at))]}));
                    self.tool = Tool::Trim(None);
                }
                _ => {}
            },
            Tool::Split => {
                if let Some(id) = hovered {
                    self.run("split", json!({"id": id, "at": point(self.snap(m, level, v, at, &[], false).at)}));
                }
            }
            Tool::Offset => {
                if let Some(id) = hovered
                    && let Some(by) = self.offset_by(m, id, v.mm(at))
                {
                    self.run(if self.offset_copy { "copy" } else { "move" }, json!({"ids": [id], "by": point(by)}));
                }
            }
            Tool::Demolish => {
                if let Some(id) = hovered {
                    self.demolish(id);
                }
            }
            // Paint works in the 3D view, which it opens; a plan's tool goes back to Modify.
            Tool::Paint | Tool::Unpaint => {}
            // A reference under the cursor joins the dimension; a click away from them places it.
            Tool::Dimension(refs) => {
                let mut refs = refs.clone();
                match hovered.and_then(|id| reference_for(m, DimKind::Aligned, refs.first(), id, v.mm(at), 2.0 * f64::from(PICK) / v.scale))
                {
                    Some(r) => {
                        if !refs.contains(&r) {
                            refs.push(r);
                        }
                        self.tool = Tool::Dimension(refs);
                    }
                    None if refs.len() >= 2 => {
                        let p = json!({"refs": refs, "at": point(round(v.mm(at))), "level": level});
                        if self.run("dimension", p).is_some() {
                            self.tool = Tool::Dimension(Vec::new());
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
                        let p = json!({"kind": "angular", "refs": refs, "at": point(round(v.mm(at))), "level": level});
                        if self.run("dimension", p).is_some() {
                            self.tool = Tool::Angular(Vec::new());
                        }
                    }
                    _ => {}
                }
            }
            &Tool::Radial(picked) => match (picked, hovered) {
                (None, Some(id)) => self.tool = Tool::Radial(Some(Reference { id: m.guid(id), face: Face::Centre })),
                (Some(r), _) => {
                    if self.run("dimension", json!({"kind": "radial", "refs": [r], "at": point(round(v.mm(at))), "level": level})).is_some()
                    {
                        self.tool = Tool::Radial(None);
                    }
                }
                (None, None) => {}
            },
            Tool::Text => self.write_note(level, round(v.mm(at))),
            Tool::Spot => {
                self.run("spot", json!({"at": point(round(v.mm(at))), "view": level}));
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
                    let field = if self.attach_base { "attach_base" } else { "attach_top" };
                    let mut p = json!({"ids": self.selected_walls()});
                    p[field] = json!(id);
                    if self.run("modify", p).is_some() {
                        self.tool = Tool::Modify;
                    }
                }
            }
            Tool::Component | Tool::Fixture(_) => {
                if let Some((family, ty)) = self.placing() {
                    let at = self.snap(m, level, v, at, &[], false).at;
                    self.run(
                        "component",
                        json!({"family": family, "type": ty, "at": point(at), "level": level, "angle": self.component_angle}),
                    );
                }
            }
            &Tool::Stand(category) => {
                let at = self.snap(m, level, v, at, &[], false).at;
                self.place_stand(level, at, category);
            }
            Tool::Column => match top_params(m, level, self.column_top, &self.column_height) {
                Ok(mut p) => {
                    p["at"] = point(self.snap(m, level, v, at, &[], false).at);
                    p["level"] = json!(level);
                    let p = self.typed("column", p);
                    self.run("column", p);
                }
                Err(e) => self.status = Status { text: tf("column: {}", &[&e]), error: true },
            },
            // Its start, where its first run ends, and which way the next turns: done when it climbs all
            // the way.
            Tool::Stair(pts) | Tool::Ramp(pts) => {
                let ramp = matches!(self.tool, Tool::Ramp(_));
                let mut pts = pts.clone();
                let p = self.snap(m, level, v, at, &pts, mods.shift).at;
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
                    let p = self.snap(m, level, v, at, path, mods.shift).at;
                    if let Tool::Railing(path) = &mut self.tool
                        && path.last().is_none_or(|l| l.dist(p) > TOL)
                    {
                        path.push(p);
                    }
                }
            },
            // The footprint's next corner; back on the first, it's done.
            Tool::Mass(path) => {
                let p = self.snap(m, level, v, at, path, mods.shift).at;
                if path.len() >= 3 && path.first().is_some_and(|f| f.dist(p) <= TOL) {
                    self.finish_mass(level);
                } else if let Tool::Mass(path) = &mut self.tool
                    && path.last().is_none_or(|l| l.dist(p) > TOL)
                {
                    path.push(p);
                }
            }
            // The line's next point; back on the first, it's closed.
            Tool::DetailLine(path) => {
                let p = self.snap(m, level, v, at, path, mods.shift).at;
                if path.len() >= 2 && path.first().is_some_and(|f| f.dist(p) <= TOL) {
                    self.finish_detail_line_closed(level, true);
                } else if let Tool::DetailLine(path) = &mut self.tool
                    && path.last().is_none_or(|l| l.dist(p) > TOL)
                {
                    path.push(p);
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
                let p = self.snap(m, level, v, at, &pts, mods.shift).at;
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

    /// The Detail Line tool's Enter: a detail line through the points clicked, in the plan of `level`.
    pub(crate) fn finish_detail_line(&mut self, level: Id) {
        self.finish_detail_line_closed(level, false);
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
        if !self.line_style.is_empty() {
            p["style"] = json!(self.line_style);
        }
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
        if self.note_text.trim().is_empty() {
            self.status = Status { text: t("text: type what the note says in the Options Bar first").into(), error: true };
            return;
        }
        // `\n` typed in the bar breaks the line.
        let text = self.note_text.replace("\\n", "\n");
        self.run("text", json!({"text": text, "at": point(at), "view": view}));
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

    /// Align's second pick: element `id` moved across so its `line` lies on the reference line.
    fn align(&mut self, reference: (P2, P2), id: Id, line: (P2, P2)) {
        if !geom::parallel(reference.0, reference.1, line.0, line.1) {
            self.status = Status { text: t("align: the lines aren't parallel").into(), error: true };
            return;
        }
        let n = (reference.1 - reference.0).unit().perp();
        let by = micron(n * (reference.0 - line.0).dot(n));
        if by.length() > 1e-6 {
            self.run("move", json!({"ids": [id], "by": point(by)}));
        }
        self.tool = Tool::Align(None);
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
        // An arc: its start, its end, then a point it passes through, which makes it.
        if self.wall_arc
            && let Tool::Wall(chain) = &mut self.tool
        {
            match chain.as_slice() {
                &[start, end] => {
                    let arc = walls::sweep_through(start, p, end);
                    if self.build_walls(level, &[start, end], false, arc) {
                        let Tool::Wall(chain) = &mut self.tool else { return };
                        *chain = if self.chain { vec![end] } else { Vec::new() };
                    }
                }
                [start] if start.dist(p) <= TOL => {}
                _ => chain.push(p),
            }
            return;
        }
        if matches!(self.tool, Tool::Run(..)) {
            return self.place_run(level, p);
        }
        match &mut self.tool {
            // A line: each click after the first builds the wall from the one before it.
            Tool::Wall(chain) => match (chain.first().copied(), chain.last().copied()) {
                (Some(first), Some(last)) if last.dist(p) > TOL => {
                    if self.build_walls(level, &[last, p], false, 0.0) {
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
            Tool::Grid(start) => match start.take() {
                None => *start = Some(p),
                Some(a) if a.dist(p) > TOL => {
                    self.run("grid", json!({"start": point(a), "end": point(p)}));
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
        let typed = std::mem::take(&mut self.typed_length);
        let (Some(base), Some(cursor)) = (self.base(), self.canvas.cursor) else { return };
        let toward = match self.canvas.found.get() {
            Some(f) if f.at.dist(base) > TOL => (f.at - base).unit(),
            _ => snap_angle(cursor - base, self.snaps.angle),
        };
        match self.session.model.units.parse(&typed) {
            Some(len) if len > 0.0 && len <= MAX_COORD => self.place(level, micron(base + toward * len)),
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
        let mut p = json!({
            "points": points.iter().map(|q| point(*q)).collect::<Vec<_>>(), "level": level, "closed": closed,
            "arc": arc, "location": self.wall_location.name(),
        });
        if let Some(t) = self.types.get("wall") {
            p["type"] = json!(t);
        }
        // The Options Bar's Height: up to a level, or unconnected at a height.
        match top_params(&self.session.model, level, self.wall_top, &self.wall_height) {
            Ok(Value::Object(top)) => p.as_object_mut().into_iter().for_each(|p| p.extend(top.clone())),
            Ok(_) => {}
            Err(e) => {
                self.status = Status { text: tf("wall: {}", &[&e]), error: true };
                return false;
            }
        }
        self.run("wall", p).is_some()
    }

    /// The wall selected alone, with Modify: the one whose ends are grips.
    fn gripped(&self) -> Option<Id> {
        match (self.selection.as_slice(), &self.tool) {
            (&[id], Tool::Modify) if self.session.model.wall(id).is_some() => Some(id),
            _ => None,
        }
    }

    /// Where wall `w`'s start (else its end) goes for a drag to pixel `q`: what that snaps to from
    /// the wall's other end; with Ctrl, along that line at the wall's own length, so it turns
    /// about its other end.
    fn grip_target(&self, m: &Model, level: Id, v: &View, (w, start): (&Wall, bool), q: (f32, f32), mods: Modifiers) -> (Found, P2) {
        let fixed = if start { w.end } else { w.start };
        let found = self.snap(m, level, v, q, &[fixed], mods.shift);
        let at = if mods.command && fixed.dist(found.at) > TOL {
            micron(fixed + (found.at - fixed).unit() * w.start.dist(w.end))
        } else {
            found.at
        };
        (found, at)
    }

    /// A drag from an end of the selected wall moves that end: its angle and length follow the
    /// cursor, with the snaps and angle guides from the other end (Ctrl keeps the length); on
    /// release, `move`, and the walls joined there follow.
    fn grip_drag(&mut self, ui: &Ui, response: &Response, rect: Rect, view: &View, level: Id) {
        let Some(id) = self.gripped() else {
            self.canvas.grip = None;
            return;
        };
        let m = std::sync::Arc::clone(&self.session.model);
        let Some(w) = m.wall(id) else { return };
        let at = |p: P2| pos(rect.min, view.px(p));
        if response.drag_started_by(PointerButton::Primary)
            && self.canvas.crop_drag.is_none()
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
        {
            let near = |p: P2| at(p).distance(o);
            let nearest = [(true, w.start), (false, w.end)]
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
                let (_, to) = self.grip_target(&m, level, view, (w, start), (q.x - rect.min.x, q.y - rect.min.y), mods);
                self.snap_once = None;
                let by = to - if start { w.start } else { w.end };
                if by.length() > 1e-6 {
                    self.run("move", json!({"ids": [id], "by": point(by), "end": if start { "start" } else { "end" }}));
                }
            }
        }
    }

    /// The selected wall's grips at its ends and, beside each, its angle to click and type: it
    /// turns about that end. Dragging one, the wall as it would stand, with its ruler.
    fn grip_overlay(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect, level: Id, d: &Drawing) {
        let Some(id) = self.gripped() else {
            self.angle_edit = None;
            return;
        };
        let Some(w) = m.wall(id) else { return };
        let p = ui.painter_at(rect);
        let at = |q: P2| pos(rect.min, v.px(q));
        let held = self.canvas.grip.filter(|(g, _)| *g == id).map(|(_, start)| start);
        let mut ends = [w.start, w.end];
        if let (Some(start), Some(q)) = (held, ui.input(|i| i.pointer.latest_pos())) {
            let (mods, q) = (ui.input(|i| i.modifiers), (q.x - rect.min.x, q.y - rect.min.y));
            let (found, to) = self.grip_target(m, level, v, (w, start), q, mods);
            let moved = Wall { start: if start { to } else { w.start }, end: if start { w.end } else { to }, ..w.clone() };
            let sc = Scene { p: &p, m, d, level, v };
            // The wall as it would stand: its outline, and the ruler from the end that stays.
            // Its faces, where its location line puts them.
            let (ext, int) = walls::faces(m, &moved);
            let axis = Axis::of(&moved);
            let (left, right) = (axis.points(0.0, axis.length(), ext), axis.points(0.0, axis.length(), int));
            let ring: Vec<Pos2> = left.into_iter().chain(right.into_iter().rev()).map(&at).collect();
            p.add(Shape::closed_line(ring, Stroke::new(1.0, pal().accent)));
            let fixed = if start { w.end } else { w.start };
            p.extend(ruler_shapes(&sc, fixed, to));
            self.show_snap(&sc, &found);
            self.canvas.found.set(Some(found));
            let r = snap::ruler(fixed, to);
            let reading = format!("{}  {}", m.units.with_unit(r.length), degrees(if start { norm_turn(r.angle + 180.0) } else { r.angle }));
            tag(&p, at(to) + vec2(12.0, -12.0), Align2::LEFT_BOTTOM, reading);
            ends[usize::from(!start)] = to;
        }
        for (k, e) in ends.into_iter().enumerate() {
            let r = Rect::from_center_size(at(e), vec2(9.0, 9.0));
            let live = held.is_some_and(|s| s == (k == 0));
            p.rect_filled(r, 1.0, if live { pal().accent } else { pal().surface });
            p.rect_stroke(r, 1.0, Stroke::new(1.5, pal().accent), StrokeKind::Middle);
        }
        if held.is_none() {
            self.wall_angles(ui, &p, id, w, v, rect);
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
        let font = FontId::proportional(12.0);
        let field = ui.id().with(("temporary dimension", key));
        let (mut done, mut start) = (None, false);
        match &mut self.dim_edit {
            Some((id, n, typed)) if (*id, *n) == key => {
                let edit = ui.put(at.expand2(vec2(18.0, 2.0)), TextEdit::singleline(typed).id(field).horizontal_align(Align::Center));
                if edit.lost_focus() {
                    done = Some(ui.input(|i| !i.key_pressed(Key::Escape)).then(|| typed.clone()));
                } else if !edit.has_focus() {
                    edit.request_focus();
                }
            }
            _ => {
                let r = ui.interact(at, field.with("number"), Sense::click());
                r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tf("Temporary dimension {}", &[text])));
                ui.painter().rect_filled(at, 2.0, pal().surface);
                ui.painter().text(at.center(), Align2::CENTER_CENTER, text, font, if r.hovered() { pal().hover } else { pal().accent });
                start = r.clicked();
            }
        }
        if start {
            self.dim_edit = Some((key.0, key.1, text.to_string()));
        }
        if done.is_some() {
            self.dim_edit = None;
        }
        done
    }

    /// The selected wall's length as a temporary dimension beside it, as Revit shows one; a door's or a window's two,
    /// to the ends of its wall. A click on a number types a new length: a wall's end moves (the walls joined there
    /// follow), an opening slides along its wall.
    fn temporary_dimension(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect) {
        let &[id] = self.selection.as_slice() else {
            self.dim_edit = None;
            return;
        };
        if self.tool != Tool::Modify || self.canvas.drag.is_some() || self.canvas.grip.is_some() {
            return;
        }
        match m.elements.get(id) {
            Some(Element::Door(o) | Element::Window(o)) => return self.opening_dimension(ui, m, v, rect, id, o),
            Some(e @ (Element::Column(_) | Element::StructuralColumn(_) | Element::Footing(_) | Element::Component(_))) => {
                return self.standing_dimension(ui, m, v, rect, id, e);
            }
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
        if let Some(typed) = self.dimension_number(ui, Rect::from_center_size(middle, size), (id, 0), &text) {
            match typed.map(|t| (m.units.parse(&t), t)) {
                // Its end moved along its chord: an arc keeps its turn, its length as typed.
                Some((Some(len), _)) if len > 0.0 => {
                    let by = (w.end - w.start) * (len / length - 1.0);
                    if by.length() > 1e-6 {
                        self.run("move", json!({"ids": [id], "by": point(by), "end": "end"}));
                    }
                }
                Some((_, typed)) => self.status = Status { text: tf("`{}` is not a length", &[typed.trim()]), error: true },
                None => {}
            }
        }
    }

    /// A selected door's or window's two temporary dimensions: how far its middle stands from its wall's start, and from
    /// its end. Typing one moves it along the wall; the other follows.
    fn opening_dimension(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect, id: Id, o: &bimcraft_core::model::Opening) {
        let Some(w) = m.wall(o.host) else { return };
        let length = Axis::of(w).length();
        let p = ui.painter_at(rect);
        for (n, (middle, text, _)) in opening_dimensions(&p, m, v, o).into_iter().enumerate() {
            let size = p.layout_no_wrap(text.clone(), FontId::proportional(12.0), pal().accent).size() + vec2(10.0, 4.0);
            let Some(typed) = self.dimension_number(ui, Rect::from_center_size(middle, size), (id, n), &text) else { continue };
            match typed.map(|t| (m.units.parse(&t), t)) {
                Some((Some(len), _)) if (0.0..=length).contains(&len) => {
                    let offset = if n == 0 { len } else { length - len };
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
        let (p, m, v, level) = (sc.p, sc.m, sc.v, sc.level);
        let origin = p.clip_rect().min;
        let cursor = pos(origin, q);
        let stroke = Stroke::new(2.0, pal().accent);
        let thin = Stroke::new(1.0, pal().accent);
        let label = |at: Pos2, text: String| tag(p, at + vec2(12.0, -12.0), Align2::LEFT_BOTTOM, text);
        let dashed = |a: Pos2, b: Pos2| p.extend(Shape::dashed_line(&[a, b], thin, 8.0, 4.0));
        // An element a click would add, drawn as the plan would draw it.
        let sketched = |e: &Element| ghost(p, origin, &render::sketch(m, *v, m.next_id, e), &[m.next_id], &|pt| pt);
        let ghost = |f: &dyn Fn(Pos2) -> Pos2| ghost(p, origin, sc.d, &self.selection, f);
        let chain = match &self.tool {
            Tool::Wall(chain) | Tool::Railing(chain) | Tool::Mass(chain) | Tool::DetailLine(chain) => chain.as_slice(),
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
            | Tool::Run(_, None)
            | Tool::Move(_)
            | Tool::Copy(_)
            | Tool::Section(None)
            | Tool::Shaft(None)
            | Tool::SlopeArrow(None)
            | Tool::Measure(_) => start_of(&self.tool),
            // The section a click would end, and the elevation a click would place.
            &Tool::Section(Some(start)) => {
                let end = self.found(sc, q, &[start], shift);
                sketched(&Element::Section(Section {
                    name: m.free_view_name("Section", true),
                    start,
                    end,
                    depth: None,
                    crop: None,
                    graphics: Default::default(),
                }));
                return;
            }
            Tool::Elevation => {
                let at = round(v.mm(q));
                if let Some(f) = m.facing_from(at)
                    && let Some((start, end)) = m.elevation_line(f, Some(at))
                {
                    let name = m.free_view_name(f.name(), false);
                    sketched(&Element::Elevation(Section { name, start, end, depth: None, crop: None, graphics: Default::default() }));
                }
                return;
            }
            Tool::Room => {
                if let Some(g) = geom::region_at(&m.bounding_lines(level), v.mm(q), TOL) {
                    p.add(Shape::closed_line(g.outer.iter().map(|pt| sc.screen(*pt)).collect(), stroke));
                    let (area, unit) = m.units.area(g.area());
                    label(cursor, format!("{area:.2} {unit}"));
                }
                return;
            }
            &Tool::Rotate { centre, start, place } => {
                let c = if place { cursor } else { sc.screen(centre) };
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
            Tool::Offset => {
                let d = m.units.parse(&self.offset).filter(|d| *d > 0.0);
                if let (Some(w), Some(d)) = (hovered.and_then(|id| m.wall(id)).filter(|w| w.arc == 0.0), d) {
                    let n = (w.end - w.start).unit().perp();
                    let by = n * if (v.mm(q) - w.start).dot(n) >= 0.0 { d } else { -d };
                    dashed(sc.screen(w.start + by), sc.screen(w.end + by));
                }
                return;
            }
            // The references picked, the one under the cursor, else the dimension a click places.
            Tool::Dimension(refs) => {
                let candidate =
                    hovered.and_then(|id| reference_for(m, DimKind::Aligned, refs.first(), id, v.mm(q), 2.0 * f64::from(PICK) / v.scale));
                for r in refs.iter().chain(&candidate) {
                    if let Ok((a, b)) = m.reference_line(r) {
                        dashed(sc.screen(a), sc.screen(b));
                    }
                }
                if candidate.is_none() && refs.len() >= 2 {
                    sketched(&Element::Dimension(Dimension {
                        level: m.guid(level),
                        kind: DimKind::Aligned,
                        refs: refs.clone(),
                        at: round(v.mm(q)),
                    }));
                }
                return;
            }
            Tool::Tag => {
                if let Some(id) = hovered {
                    let label = Some(self.tag_label.trim().to_string()).filter(|l| !l.is_empty());
                    sketched(&Element::Tag(Tag { host: m.guid(id), offset: m.tag_offset(id), leader: self.tag_leader, label }));
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
                        kind: DimKind::Angular,
                        refs: refs.clone(),
                        at: round(v.mm(q)),
                    }));
                }
                return;
            }
            &Tool::Radial(Some(r)) => {
                sketched(&Element::Dimension(Dimension { level: m.guid(level), kind: DimKind::Radial, refs: vec![r], at: round(v.mm(q)) }));
                return;
            }
            Tool::Text if !self.note_text.trim().is_empty() => {
                let text = self.note_text.replace("\\n", "\n");
                sketched(&Element::Text(Note { view: m.guid(level), at: round(v.mm(q)), text, size: NOTE_SIZE, angle: 0.0, leader: None }));
                return;
            }
            Tool::Spot => {
                let at = round(v.mm(q));
                sketched(&Element::Spot(Spot { view: m.guid(level), at, host: m.spot_host(level, at).map(|h| m.guid(h)) }));
                return;
            }
            // The door or window a click would put in the wall, as the command makes it, and how far it stands from
            // the wall's ends.
            Tool::Door | Tool::Window => {
                let door = matches!(self.tool, Tool::Door);
                let params = self.typed(if door { "door" } else { "window" }, json!({"at": point(round(v.mm(q))), "level": level}));
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
                if let Some((family, type_name)) = self.placing() {
                    let at = self.found(sc, q, &[], false);
                    let angle = self.component_angle;
                    let c = Component {
                        family,
                        type_name,
                        level: m.guid(level),
                        at,
                        angle,
                        offset: 0.0,
                        mirrored: false,
                        params: Default::default(),
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
                    params: Default::default(),
                    type_name,
                    level: m.guid(level),
                    at,
                    angle: 0.0,
                    top_level: None,
                    height: 0.0,
                    base_offset: 0.0,
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
        // The point an arc passes through follows no guide from the points before it.
        let arc_through = self.wall_arc && matches!(self.tool, Tool::Wall(_)) && chain.len() == 2;
        let next = self.found(sc, q, if arc_through { &[] } else { chain }, square);
        if let Tool::Move(Some(base)) | Tool::Copy(Some(base)) = &self.tool {
            let delta = sc.screen(next) - sc.screen(*base);
            ghost(&|pt| pt + delta);
        }
        // A line's walls are built as it goes: only the next one is to come.
        let built = if matches!(self.tool, Tool::Wall(_)) && !self.wall_arc { chain.len().saturating_sub(1) } else { 0 };
        let mut pts: Vec<Pos2> = chain.iter().skip(built).map(|pt| sc.screen(*pt)).collect();
        pts.push(sc.screen(next));
        // An arc waiting for its third point: the arc through the cursor.
        if let (true, &[start, end]) = (self.wall_arc && matches!(self.tool, Tool::Wall(_)), chain) {
            let axis = Axis::new(start, end, walls::sweep_through(start, next, end));
            pts = axis.points(0.0, axis.length(), 0.0).into_iter().map(|pt| sc.screen(pt)).collect();
        }
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
        let arc_next = self.wall_arc && matches!(self.tool, Tool::Wall(_));
        if let (Some(&last), false) = (chain.last(), arc_next) {
            p.extend(ruler_shapes(sc, last, next));
            if let [.., prev, l] = chain
                && let Some(a) = corner_arc(sc, *prev, *l, next)
            {
                p.add(a);
            }
        }
        // The listening dimension: the length so far and the angle (at the corner it turns, else from
        // east), or the length being typed; an arc's radius.
        let arc = match (arc_next, chain) {
            (true, &[start, end]) => Axis::new(start, end, walls::sweep_through(start, next, end)).radius(),
            _ => None,
        };
        let reading = |last: P2| {
            let angle = match chain {
                [.., prev, l] => snap::corner(*prev, *l, next),
                _ => None,
            };
            format!("{}  {}", m.units.with_unit(last.dist(next)), degrees(angle.unwrap_or_else(|| snap::ruler(last, next).angle)))
        };
        match (chain.last(), self.typed_length.is_empty(), arc) {
            (Some(_), false, _) => label(sc.screen(next), format!("{} (Enter)", self.typed_length.trim())),
            (Some(_), true, Some(r)) => label(sc.screen(next), format!("R {}", m.units.with_unit(r))),
            (Some(last), true, None) => label(sc.screen(next), reading(*last)),
            (None, ..) => {}
        }
    }
}
