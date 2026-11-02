//! The Family Editor's adaptive modelling view, as Revit's: a 3D view over a work plane where Point places reference points
//! (on a line, hosted on it), Line draws through points (a click on the first closes the profile), Make Adaptive numbers the
//! points selected in the order it is pressed, and Create Form makes a solid or a void of the profiles selected. A point
//! selected shows a move gizmo: a drag of an arrow moves it along that axis, a drag of the point over its own height. The view
//! orbits, pans and zooms as the 3D view does. Every edit changes the family's canonical definition (`adaptive::Definition`,
//! its edits in `adaptive/edit.rs`); Undo here steps back through this editor's edits, and Apply or OK makes the family the
//! project's.
use crate::lang::t;
use crate::style::pal;
use bimcraft_core::adaptive::{
    Datum, Definition, Evaluated, Evaluation, Host, Operation, Picked, Plane, PointType, Show, WorkPlane, gizmo_end,
};
use bimcraft_core::family::Val;
use bimcraft_core::view3d::{AZIMUTH, Camera, ELEVATION, P3};
use bimcraft_i18n::msg;
use egui::{Color32, Key, Modifiers, PointerButton, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, vec2};
use std::collections::BTreeMap;

/// The tool the view's clicks are for.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum Draw {
    #[default]
    Modify,
    Point,
    /// Line, and the points of the chain drawn so far.
    Line(Vec<String>),
}

/// A drag under way in the view.
#[derive(Clone, Debug)]
enum Drag {
    Orbit(Pos2),
    Pan(Pos2),
    /// A point dragged over the horizontal plane at its height.
    Plane(String, f64),
    /// A point dragged along a gizmo axis: which, where the drag began, and where the point was then.
    Axis(String, usize, Pos2, P3),
}

/// The modelling view between frames.
pub(crate) struct Modeller {
    pub(crate) camera: Option<Camera>,
    pub(crate) rect: Rect,
    pub(crate) tool: Draw,
    pub(crate) selection: Vec<String>,
    drag: Option<Drag>,
    /// The definition before each edit, newest last.
    history: Vec<Definition>,
    /// The solid or void Create Form makes: its depth, mm.
    pub(crate) depth: f64,
    /// Where Point Element and Line draw: Revit's Placement Plane, Ref. Level to begin with.
    pub(crate) work_plane: WorkPlane,
    /// Placement Plane's Pick: the next click in the view picks the plane under it.
    pub(crate) picking_plane: bool,
}

impl Default for Modeller {
    fn default() -> Modeller {
        Modeller {
            camera: None,
            rect: Rect::ZERO,
            tool: Draw::Modify,
            selection: Vec::new(),
            drag: None,
            history: Vec::new(),
            depth: 300.,
            work_plane: WorkPlane::default(),
            picking_plane: false,
        }
    }
}

/// The gizmo's arrows, px long on screen.
const ARROW: f64 = 60.0;

impl Modeller {
    /// Where model point `p` shows on screen: where a test clicks it.
    #[cfg(test)]
    pub(crate) fn screen(&self, p: P3) -> Pos2 {
        match self.camera {
            Some(c) => {
                let (x, y, _) = c.project(p);
                self.rect.min + vec2(x as f32, y as f32)
            }
            None => self.rect.center(),
        }
    }

    fn remember(&mut self, d: &Definition) {
        self.history.push(d.clone());
        if self.history.len() > 64 {
            self.history.remove(0);
        }
    }
}

/// How far the datums reach: a box 300 mm beyond what the family takes up, and at least 1.5 m from its origin, each way.
fn room(r: &Evaluation) -> (P3, P3) {
    let (lo, hi) = extent(r).unwrap_or(([-1000.; 3], [1000.; 3]));
    (std::array::from_fn(|i| (lo[i] - 300.).min(-1500.)), std::array::from_fn(|i| (hi[i] + 300.).max(1500.)))
}

/// How far each of a point's planes reaches from it on screen, px: half the square drawn.
const PLANE_REACH: f64 = 14.0;

/// Everything an evaluation shows, for fitting the view to it.
fn extent(r: &Evaluation) -> Option<(P3, P3)> {
    let mut points: Vec<P3> = vec![[0.; 3]];
    for v in r.values.values() {
        match v {
            Evaluated::Point { frame } => points.push(frame.origin),
            Evaluated::Curve { curve } => points.extend(curve.samples()),
            Evaluated::Form { mesh, .. } => points.extend(mesh.vertices.iter().copied()),
        }
    }
    let lo = std::array::from_fn(|i| points.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min));
    let hi = std::array::from_fn(|i| points.iter().map(|p| p[i]).fold(f64::NEG_INFINITY, f64::max));
    Some((lo, hi)).filter(|(lo, hi): &(P3, P3)| lo.iter().chain(hi).all(|v| v.is_finite()))
}

/// A camera from Revit's default 3D orientation that shows all of `r` in `w` by `h` points.
fn fit(r: &Evaluation, w: f32, h: f32) -> Camera {
    let (lo, hi) = extent(r).unwrap_or(([-1000.; 3], [1000.; 3]));
    // An empty family shows six metres round the work plane's origin, room to start drawing in.
    let least = if r.values.is_empty() { 6000. } else { 1000. };
    let size = (0..3).map(|i| hi[i] - lo[i]).fold(0., f64::max).max(least);
    Camera {
        azimuth: AZIMUTH,
        elevation: ELEVATION,
        lens: None,
        target: std::array::from_fn(|i| (lo[i] + hi[i]) / 2.),
        scale: 0.6 * f64::from(w.min(h)) / size,
        width: w.max(1.) as u32,
        height: h.max(1.) as u32,
    }
}

/// Whether node `id` is a point a drag can move.
fn movable(d: &Definition, id: &str) -> bool {
    d.nodes.iter().any(|n| {
        n.id == id && matches!(&n.operation, Operation::Point { host, .. } if !matches!(host, Host::Face { .. } | Host::Edge { .. } | Host::Vertex { .. }))
    })
}

/// The adaptive number of point `id`, if it is adaptive.
fn number(d: &Definition, id: &str) -> Option<u32> {
    d.nodes.iter().find(|n| n.id == id).and_then(|n| match &n.operation {
        Operation::Point { host: Host::Adaptive { number, .. }, .. } => Some(*number),
        _ => None,
    })
}

/// The points and the lines selected.
pub(crate) fn selected(d: &Definition, m: &Modeller, values: &BTreeMap<String, Val>) -> (Vec<String>, Vec<String>) {
    let r = d.evaluate(values, &d.preview);
    let points = m.selection.iter().filter(|id| r.origin(id).is_some()).cloned().collect();
    let lines = m.selection.iter().filter(|id| matches!(r.values.get(id.as_str()), Some(Evaluated::Curve { .. }))).cloned().collect();
    (points, lines)
}

/// Make Adaptive, a toggle as Revit's: points all adaptive already go back to reference points; otherwise the others are made
/// adaptive, numbered in the order they were selected.
pub(crate) fn make_adaptive(d: &mut Definition, m: &mut Modeller, points: &[String]) {
    m.remember(d);
    let all = points.iter().all(|id| matches!(d.point_type(id), Some(PointType::Placement | PointType::ShapeHandle)));
    for id in points {
        let _ = if all { d.make_reference(id) } else { d.make_adaptive(id).map(|_| ()) };
    }
}

/// Create Form, a solid or a `void`, from the lines selected, `m.depth` deep; it is selected after. What went wrong, if it did.
pub(crate) fn create_form(d: &mut Definition, m: &mut Modeller, lines: &[String], void: bool) -> Result<(), String> {
    let before = d.clone();
    let id = d.create_form(lines, m.depth, void).inspect_err(|_| *d = before.clone())?;
    m.history.push(before);
    m.selection = vec![id];
    Ok(())
}

/// Delete: what is selected, with what depends on it.
pub(crate) fn delete(d: &mut Definition, m: &mut Modeller) {
    m.remember(d);
    d.remove(&m.selection);
    m.selection.clear();
}

/// Undo: one step back through this family's edits; whether there was one.
pub(crate) fn undo(d: &mut Definition, m: &mut Modeller) -> bool {
    let Some(before) = m.history.pop() else { return false };
    *d = before;
    m.selection.retain(|id| d.nodes.iter().any(|n| &n.id == id));
    true
}

/// What the tool in hand asks for, as the status bar says it.
pub(crate) fn prompt(m: &Modeller) -> &'static str {
    if m.picking_plane {
        return msg!("Pick a plane: one of a point's planes, the level or a reference plane; Escape gives up.");
    }
    match &m.tool {
        Draw::Modify => msg!("Click to select, Ctrl adds; drag a point or a gizmo arrow to move it; drag elsewhere to orbit."),
        Draw::Point => msg!("Click on the work plane for a reference point, on a line for a point hosted on it."),
        Draw::Line(chain) if chain.is_empty() => msg!("Click points or the work plane; click the first point again to close the profile."),
        Draw::Line(_) => msg!("Click the next point; the first again closes the profile; Enter or Escape finishes it open."),
    }
}

/// The modelling view, `size` points, over definition `d` evaluated with `values`.
pub(crate) fn show(ui: &mut Ui, d: &mut Definition, m: &mut Modeller, values: &BTreeMap<String, Val>, size: egui::Vec2) {
    if m.depth <= 0. {
        m.depth = 300.;
    }
    let mut r = d.evaluate(values, &d.preview);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    m.rect = rect;
    let mut cam = match m.camera {
        Some(c) => Camera { width: rect.width().max(1.) as u32, height: rect.height().max(1.) as u32, ..c },
        None => fit(&r, rect.width(), rect.height()),
    };
    let px = |p: Pos2| (f64::from(p.x - rect.min.x), f64::from(p.y - rect.min.y));
    let (hover, wheel, pinch, latest, mods, middle) = ui.input(|i| {
        (
            i.pointer.hover_pos().filter(|p| rect.contains(*p)),
            i.smooth_scroll_delta.y,
            i.zoom_delta(),
            i.pointer.latest_pos(),
            i.modifiers,
            i.pointer.button_down(PointerButton::Middle),
        )
    });
    if let Some(at) = hover {
        let factor = (f64::from(wheel) * 0.004).exp() * f64::from(pinch);
        if factor.is_finite() && factor != 1.0 {
            cam = cam.zoom(factor.clamp(0.2, 5.0), px(at));
        }
    }
    let screen_of = |cam: &Camera, p: P3| -> Option<(f64, f64, f64)> {
        let (x, y, z) = cam.project(p);
        (x.is_finite() && y.is_finite()).then_some((x, y, z))
    };
    let to_screen = |cam: &Camera, p: P3| {
        let (x, y, _) = cam.project(p);
        rect.min + vec2(x as f32, y as f32)
    };
    // The gizmo of the one point selected, if it can move: its arrows' ends on screen.
    let gizmo = match m.selection.as_slice() {
        [id] if movable(d, id) => r.origin(id).map(|o| (id.clone(), o)),
        _ => None,
    };
    let arrows = |cam: &Camera, o: P3| (0..3).map(|k| to_screen(cam, gizmo_end(o, k, ARROW / cam.scale.max(1e-9)))).collect::<Vec<_>>();
    if response.drag_started()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        let primary = response.dragged_by(PointerButton::Primary) && !mods.shift;
        let on_arrow = gizmo.as_ref().and_then(|(id, o)| {
            let ends = arrows(&cam, *o);
            let base = to_screen(&cam, *o);
            (0..3)
                .find(|k| {
                    let (a, b) = (base, ends[*k]);
                    let ab = b - a;
                    let f = ((origin - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                    f > 0.25 && (a + ab * f).distance(origin) <= 6.0
                })
                .map(|k| (id.clone(), k, *o))
        });
        m.drag = Some(match (primary, m.tool == Draw::Modify, on_arrow) {
            (true, true, Some((id, k, o))) => Drag::Axis(id, k, origin, o),
            (true, true, None) => match r.pick(&|p| screen_of(&cam, p), px(origin)) {
                Some(Picked::Point(id)) if movable(d, &id) => {
                    m.selection = vec![id.clone()];
                    let z = r.origin(&id).map_or(0., |o| o[2]);
                    Drag::Plane(id, z)
                }
                _ => Drag::Orbit(origin),
            },
            _ if middle && !mods.shift => Drag::Pan(origin),
            _ => Drag::Orbit(origin),
        });
        if matches!(m.drag, Some(Drag::Axis(..) | Drag::Plane(..))) {
            m.remember(d);
        }
    }
    if response.dragged()
        && let (Some(drag), Some(now)) = (m.drag.clone(), latest)
    {
        match drag {
            Drag::Orbit(last) => {
                let delta = now - last;
                cam = cam.orbit(-f64::from(delta.x) * 0.4, f64::from(delta.y) * 0.4);
                m.drag = Some(Drag::Orbit(now));
            }
            Drag::Pan(last) => {
                cam = cam.pan(px(last), px(now));
                m.drag = Some(Drag::Pan(now));
            }
            Drag::Plane(id, z) => {
                if let Some(to) = cam.on_plane(px(now), z) {
                    let _ = d.move_point(&id, &r, to);
                }
            }
            Drag::Axis(id, k, from, start) => {
                if let Some(by) = cam.along(k, start, px(from), px(now)) {
                    let to = gizmo_end(start, k, by);
                    let _ = d.move_point(&id, &r, to);
                }
            }
        }
        r = d.evaluate(values, &d.preview);
    }
    if response.drag_stopped() {
        m.drag = None;
    }
    m.camera = Some(cam);
    let hovered = hover.filter(|_| m.drag.is_none()).and_then(|at| r.pick(&|p| screen_of(&cam, p), px(at)));
    // Placement Plane's Pick: the plane under the pointer lights up, and a click makes it the work plane; Escape gives up.
    let plane_at = |at: Pos2| {
        let half = PLANE_REACH / cam.scale.max(1e-9);
        r.pick_plane(&|o, n| cam.on(px(at), o, n), &|p| cam.project(p).2, half, room(&r))
    };
    let lit = if m.picking_plane { hover.and_then(plane_at) } else { None };
    if m.picking_plane
        && response.clicked()
        && let Some(at) = response.interact_pointer_pos()
    {
        if let Some(plane) = plane_at(at) {
            m.work_plane = plane;
            m.picking_plane = false;
        }
    } else if response.clicked()
        && let Some(at) = response.interact_pointer_pos()
    {
        let mods = crate::canvas::click_modifiers(ui);
        let picked = r.pick(&|p| screen_of(&cam, p), px(at));
        // Where the click falls on the work plane (Ref. Level when the one chosen has gone).
        let plane = m.work_plane.frame(&r).unwrap_or_default();
        let on_plane = cam.on(px(at), plane.origin, plane.z);
        let work_plane = m.work_plane.clone();
        match &mut m.tool {
            Draw::Modify => match picked {
                Some(p) if mods.ctrl || mods.shift => {
                    let id = p.id().to_string();
                    if let Some(k) = m.selection.iter().position(|s| *s == id) {
                        m.selection.remove(k);
                    } else {
                        m.selection.push(id);
                    }
                }
                Some(p) => m.selection = vec![p.id().to_string()],
                None => m.selection.clear(),
            },
            Draw::Point => {
                let before = d.clone();
                let made = match picked {
                    Some(Picked::Curve(curve, t)) => d.add_hosted_point(&curve, t).ok(),
                    // Snapped onto the point under the pointer: exactly where it falls on the work plane (on that point's own
                    // plane, on it, at no offset).
                    Some(Picked::Point(snap)) => r.origin(&snap).and_then(|o| d.add_point_on(&work_plane, o, &r).ok()),
                    _ => on_plane.and_then(|p| d.add_point_on(&work_plane, p, &r).ok()),
                };
                if let Some(id) = made {
                    m.history.push(before);
                    m.selection = vec![id];
                }
            }
            Draw::Line(chain) => {
                let before = d.clone();
                let point = match picked {
                    Some(Picked::Point(id)) => Some(id),
                    _ => on_plane.and_then(|p| d.add_point_on(&work_plane, p, &r).ok()),
                };
                if let Some(id) = point {
                    let closes = chain.len() >= 3 && chain.first() == Some(&id);
                    if closes {
                        if let Ok(line) = d.add_chain(chain, true) {
                            m.selection = vec![line];
                        }
                        chain.clear();
                    } else if chain.last() != Some(&id) {
                        chain.push(id);
                    }
                    m.history.push(before);
                }
            }
        }
    }
    // Finishing a chain open: Enter, Escape or a right click; Escape again back to Modify.
    let (enter, escape, delete, undo) = ui.input(|i| {
        (
            i.key_pressed(Key::Enter),
            i.key_pressed(Key::Escape),
            i.key_pressed(Key::Delete),
            i.modifiers.matches_exact(Modifiers::COMMAND) && i.key_pressed(Key::Z),
        )
    });
    if m.picking_plane && escape {
        m.picking_plane = false;
    }
    let finish = enter || ((escape || response.secondary_clicked()) && hover.is_some());
    if finish && let Draw::Line(chain) = &mut m.tool {
        if chain.len() >= 2 {
            m.history.push(d.clone());
            if let Ok(line) = d.add_chain(chain, false) {
                m.selection = vec![line];
            }
            chain.clear();
        } else if escape {
            m.tool = Draw::Modify;
        }
    } else if escape && hover.is_some() {
        m.tool = Draw::Modify;
    }
    if hover.is_some() && delete && !m.selection.is_empty() {
        m.remember(d);
        d.remove(&m.selection);
        m.selection.clear();
    }
    if hover.is_some()
        && undo
        && let Some(before) = m.history.pop()
    {
        *d = before;
    }
    m.selection.retain(|id| d.nodes.iter().any(|n| &n.id == id));
    let r = d.evaluate(values, &d.preview);
    draw(ui, rect, &cam, d, &r, m, hovered.as_ref().map(Picked::id), hover, lit.as_ref());
}

/// What the view shows: the work plane, the forms shaded, the lines, the points with the adaptive ones numbered, the
/// selection in the accent, and the gizmo of a point selected.
#[allow(clippy::too_many_arguments)]
fn draw(
    ui: &Ui,
    rect: Rect,
    cam: &Camera,
    d: &Definition,
    r: &Evaluation,
    m: &Modeller,
    hovered: Option<&str>,
    hover: Option<Pos2>,
    lit: Option<&WorkPlane>,
) {
    let p = ui.painter_at(rect);
    let visuals = ui.visuals();
    // On the drawing's own ground, as the plans and the 3D view: light in a dark interface when the user keeps the canvas light.
    let cp = crate::style::canvas_pal();
    let (ink, faint, accent, hover_ink) = (cp.text, cp.muted.gamma_multiply(0.5), cp.accent, cp.hover);
    let ground = if crate::style::canvas_dark() { visuals.extreme_bg_color } else { crate::canvas::color(bimcraft_core::render::PAPER) };
    p.rect_filled(rect, 2.0, ground);
    let at = |q: P3| {
        let (x, y, _) = cam.project(q);
        rect.min + vec2(x as f32, y as f32)
    };
    let tone = |id: &str, base: Color32| {
        if m.selection.iter().any(|s| s == id) {
            accent
        } else if hovered == Some(id) {
            hover_ink
        } else {
            base
        }
    };
    // The work plane Point Element and Line draw on: a grid on it, a line each metre over what the family takes up there.
    let (lo, hi) = extent(r).unwrap_or(([-1000.; 3], [1000.; 3]));
    let plane = m.work_plane.frame(r).unwrap_or_default();
    let corners: Vec<P3> =
        (0..8).map(|k| plane.inverse_point([[lo, hi][k & 1][0], [lo, hi][(k >> 1) & 1][1], [lo, hi][k >> 2][2]])).collect();
    let span = |i: usize| {
        (corners.iter().map(|c| c[i]).fold(f64::INFINITY, f64::min), corners.iter().map(|c| c[i]).fold(f64::NEG_INFINITY, f64::max))
    };
    let ((u0, u1), (v0, v1)) = (span(0), span(1));
    let step = 1000.0_f64;
    let (x0, x1) = ((u0 / step).floor() - 1., (u1 / step).ceil() + 1.);
    let (y0, y1) = ((v0 / step).floor() - 1., (v1 / step).ceil() + 1.);
    let on = |u: f64, v: f64| at(plane.point([u * step, v * step, 0.]));
    if (x1 - x0) < 200. && (y1 - y0) < 200. {
        let mut x = x0;
        while x <= x1 {
            p.line_segment([on(x, y0), on(x, y1)], Stroke::new(1.0, faint));
            x += 1.;
        }
        let mut y = y0;
        while y <= y1 {
            p.line_segment([on(x0, y), on(x1, y)], Stroke::new(1.0, faint));
            y += 1.;
        }
    }
    // The datums, as Revit's template has them: Ref. Level and the two Center reference planes, each the outline of a
    // rectangle 300 mm beyond what the family takes up, named at a corner; the one drawn on in a lighter accent.
    let (a, b) = room(r);
    let datum_ink = cp.muted.gamma_multiply(0.8);
    for (datum, quad) in [
        (Datum::Level, [[a[0], a[1], 0.], [b[0], a[1], 0.], [b[0], b[1], 0.], [a[0], b[1], 0.]]),
        (Datum::FrontBack, [[a[0], 0., a[2]], [b[0], 0., a[2]], [b[0], 0., b[2]], [a[0], 0., b[2]]]),
        (Datum::LeftRight, [[0., a[1], a[2]], [0., b[1], a[2]], [0., b[1], b[2]], [0., a[1], b[2]]]),
    ] {
        let active = m.work_plane == WorkPlane::Datum(datum);
        let picked = lit == Some(&WorkPlane::Datum(datum));
        let ink = if picked {
            cp.hover
        } else if active {
            cp.accent.gamma_multiply(0.55)
        } else {
            datum_ink
        };
        p.add(Shape::closed_line(quad.iter().map(|q| at(*q)).collect(), Stroke::new(if picked { 2.5 } else { 1.0 }, ink)));
        let name = match datum {
            Datum::Level => "Ref. Level",
            Datum::FrontBack => "Center (Front/Back)",
            Datum::LeftRight => "Center (Left/Right)",
        };
        p.text(at(quad[3]) + vec2(4.0, 2.0), egui::Align2::LEFT_TOP, t(name), egui::FontId::proportional(11.0), ink);
    }
    // The forms, far triangles first, each shaded by how squarely it faces a light over the viewer's left shoulder.
    let light = {
        let (a, e) = ((cam.azimuth + 40.).to_radians(), (cam.elevation + 25.).to_radians());
        [e.cos() * a.cos(), e.cos() * a.sin(), e.sin()]
    };
    let mut tris: Vec<(f64, [Pos2; 3], Color32)> = Vec::new();
    for id in &r.order {
        if let Some(Evaluated::Form { mesh, appearance }) = r.values.get(id) {
            let base = if appearance.void { Color32::from_rgb(230, 150, 90) } else { Color32::from_rgb(196, 188, 170) };
            let base = tone(id, base);
            for t in mesh.triangles() {
                let e1 = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
                let e2 = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
                let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
                let lit = 0.55 + 0.45 * ((n[0] * light[0] + n[1] * light[1] + n[2] * light[2]) / len).abs();
                let shade = |c: u8| (f64::from(c) * lit).round().clamp(0., 255.) as u8;
                let mut color = Color32::from_rgb(shade(base.r()), shade(base.g()), shade(base.b()));
                if appearance.void {
                    color = color.gamma_multiply(0.55);
                }
                let depth = t.iter().map(|q| cam.project(*q).2).sum::<f64>() / 3.;
                tris.push((depth, t.map(at), color));
            }
        }
    }
    tris.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut mesh = egui::Mesh::default();
    for (_, [a, b, c], color) in tris {
        let i = mesh.vertices.len() as u32;
        for q in [a, b, c] {
            mesh.colored_vertex(q, color);
        }
        mesh.add_triangle(i, i + 1, i + 2);
    }
    p.add(Shape::mesh(mesh));
    // The lines.
    for id in &r.order {
        if let Some(Evaluated::Curve { curve }) = r.values.get(id) {
            let mut points: Vec<Pos2> = curve.samples().into_iter().map(at).collect();
            if curve.closed
                && let Some(first) = points.first().copied()
            {
                points.push(first);
            }
            let selected = m.selection.iter().any(|s| s == id);
            p.add(Shape::line(points, Stroke::new(if selected { 2.5 } else { 1.5 }, tone(id, ink))));
        }
    }
    // The line being drawn, to the pointer.
    if let (Draw::Line(chain), Some(to)) = (&m.tool, hover)
        && let Some(last) = chain.last().and_then(|id| r.origin(id))
    {
        p.line_segment([at(last), to], Stroke::new(1.0, accent));
    }
    // The points: a placement point filled and numbered, a shape handle a filled square, a reference point a ring, a hosted
    // one a small ring; each with its own planes and axes when Show Reference Planes says so.
    for id in &r.order {
        let Some(Evaluated::Point { frame }) = r.values.get(id) else { continue };
        let o = frame.origin;
        let s = at(o);
        let color = tone(id, ink);
        let selected = m.selection.iter().any(|x| x == id);
        let shown = |show: Show| show == Show::Always || (show == Show::WhenSelected && selected);
        let look = d.nodes.iter().find(|n| &n.id == id).map(|n| n.look());
        // Its three planes and its three axes, light blue, where Show Reference Planes says: each plane the outline of a square
        // 28 px across centred on it, each axis a short arrow 24 px long. Selected, the axes are the gizmo's long ones (or, for a
        // point the gizmo does not move, as long and in the selection's colour), and the planes take the selection's colour.
        let gizmo = selected && m.selection.len() == 1 && movable(d, id);
        if look.is_some_and(|l| shown(l.planes)) {
            let px = 1.0 / cam.scale.max(1e-9);
            let blue = Color32::from_rgb(86, 170, 226);
            let tone = if color == ink { blue } else { color };
            let h = PLANE_REACH * px;
            for (plane, (u, v)) in [(Plane::Xy, (frame.x, frame.y)), (Plane::Xz, (frame.x, frame.z)), (Plane::Yz, (frame.y, frame.z))] {
                let corner = |a: f64, b: f64| at(std::array::from_fn(|i| o[i] + (u[i] * a + v[i] * b) * h));
                let square = vec![corner(-1., -1.), corner(1., -1.), corner(1., 1.), corner(-1., 1.)];
                // The plane Pick is over: lit, and tinted.
                if lit == Some(&WorkPlane::Point { point: id.clone(), plane }) {
                    p.add(Shape::convex_polygon(square.clone(), cp.hover.gamma_multiply(0.25), Stroke::new(2.5, cp.hover)));
                } else {
                    p.add(Shape::closed_line(square, Stroke::new(1.0, tone.gamma_multiply(0.7))));
                }
            }
            if !gizmo {
                let length = if selected { ARROW } else { 24.0 } * px;
                for axis in [frame.x, frame.y, frame.z] {
                    let end = at(std::array::from_fn(|i| o[i] + axis[i] * length));
                    p.arrow(s, end - s, Stroke::new(1.3, tone));
                }
            }
        }
        // The snap a click would take, drawing: a square round the point under the pointer.
        if matches!(m.tool, Draw::Point | Draw::Line(_)) && hovered == Some(id.as_str()) {
            p.rect_stroke(Rect::from_center_size(s, vec2(14.0, 14.0)), 0.0, Stroke::new(1.5, cp.hover), StrokeKind::Outside);
        }
        match (number(d, id), d.point_type(id)) {
            (Some(n), _) => {
                p.circle_filled(s, 5.0, color);
                if look.is_some_and(|l| shown(l.number)) {
                    p.text(s + vec2(7.0, -7.0), egui::Align2::LEFT_BOTTOM, n.to_string(), egui::FontId::proportional(13.0), color);
                }
            }
            (None, Some(PointType::ShapeHandle)) => {
                p.rect_filled(Rect::from_center_size(s, vec2(8.0, 8.0)), 0.0, color);
            }
            _ => {
                let hosted = d
                    .nodes
                    .iter()
                    .any(|n| &n.id == id && matches!(&n.operation, Operation::Point { host, .. } if !matches!(host, Host::World { .. })));
                p.circle_stroke(s, if hosted { 3.0 } else { 4.5 }, Stroke::new(1.5, color));
            }
        }
    }
    // The gizmo: an arrow along each axis, red x, green y, blue z.
    if let [id] = m.selection.as_slice()
        && movable(d, id)
        && let Some(o) = r.origin(id)
    {
        let base = at(o);
        for (k, color) in [(0, Color32::from_rgb(220, 60, 50)), (1, Color32::from_rgb(60, 170, 70)), (2, Color32::from_rgb(60, 110, 230))] {
            let end = at(gizmo_end(o, k, ARROW / cam.scale.max(1e-9)));
            p.arrow(base, end - base, Stroke::new(2.0, color));
        }
    }
    p.rect_stroke(rect, 2.0, Stroke::new(1.0, pal().border), StrokeKind::Inside);
    if r.order.is_empty() {
        p.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            t("Choose Point and click on the work plane."),
            egui::FontId::proportional(14.0),
            ink,
        );
    }
}
