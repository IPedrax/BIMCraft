//! The Family Editor's adaptive modelling view, as Revit's: a 3D view over a work plane where Point places reference points
//! (on a line, hosted on it), Line draws through points (a click on the first closes the profile), Make Adaptive numbers the
//! points selected in the order it is pressed, and Create Form makes a solid or a void of the profiles selected. A point
//! selected shows a move gizmo: a drag of an arrow moves it along that axis, a drag of the point over its own height. The view
//! orbits, pans and zooms as the 3D view does. Every edit changes the family's canonical definition (`adaptive::Definition`,
//! its edits in `adaptive/edit.rs`); Undo here steps back through this editor's edits, and Apply or OK makes the family the
//! project's.
use crate::lang::t;
use crate::style::pal;
use bimcraft_core::adaptive::{Definition, Evaluated, Evaluation, Host, Operation, Picked, gizmo_end};
use bimcraft_core::family::Val;
use bimcraft_core::view3d::{AZIMUTH, Camera, ELEVATION, P3};
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
}

impl Default for Modeller {
    fn default() -> Modeller {
        Modeller { camera: None, rect: Rect::ZERO, tool: Draw::Modify, selection: Vec::new(), drag: None, history: Vec::new(), depth: 300. }
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

/// The modelling view and its tools, `size` points, over definition `d` evaluated with `values`.
pub(crate) fn show(ui: &mut Ui, d: &mut Definition, m: &mut Modeller, values: &BTreeMap<String, Val>, size: egui::Vec2) {
    if m.depth <= 0. {
        m.depth = 300.;
    }
    let mut r = d.evaluate(values, &d.preview);
    // The tools, as Revit's Create tab and the Modify contextual tab lay them out.
    ui.horizontal_wrapped(|ui| {
        for (tool, label) in [(Draw::Modify, "Modify"), (Draw::Point, "Point"), (Draw::Line(Vec::new()), "Line")] {
            let on = std::mem::discriminant(&m.tool) == std::mem::discriminant(&tool);
            if ui.selectable_label(on, t(label)).clicked() {
                m.tool = tool;
            }
        }
        ui.separator();
        let points: Vec<String> = m.selection.iter().filter(|id| r.origin(id).is_some()).cloned().collect();
        let lines: Vec<String> =
            m.selection.iter().filter(|id| matches!(r.values.get(id.as_str()), Some(Evaluated::Curve { .. }))).cloned().collect();
        if ui.add_enabled(!points.is_empty(), egui::Button::new(t("Make Adaptive"))).clicked() {
            m.remember(d);
            for id in &points {
                let _ = d.make_adaptive(id);
            }
        }
        if ui.add_enabled(points.iter().any(|id| number(d, id).is_some()), egui::Button::new(t("Make Reference Point"))).clicked() {
            m.remember(d);
            for id in &points {
                let _ = d.make_reference(id);
            }
        }
        for (label, void) in [("Create Solid Form", false), ("Create Void Form", true)] {
            if ui.add_enabled(!lines.is_empty(), egui::Button::new(t(label))).clicked() {
                let before = d.clone();
                match d.create_form(&lines, m.depth, void) {
                    Ok(id) => {
                        m.history.push(before);
                        m.selection = vec![id];
                    }
                    Err(e) => {
                        *d = before;
                        ui.ctx().data_mut(|w| w.insert_temp(egui::Id::new("adaptive-view-problem"), e));
                    }
                }
            }
        }
        ui.label(t("Depth (mm)"));
        ui.add(egui::DragValue::new(&mut m.depth).speed(5.).range(1.0..=100_000.0));
        ui.separator();
        if ui.add_enabled(!m.selection.is_empty(), egui::Button::new(t("Delete"))).clicked() {
            m.remember(d);
            d.remove(&m.selection);
            m.selection.clear();
        }
        if ui.add_enabled(!m.history.is_empty(), egui::Button::new(t("Undo"))).clicked()
            && let Some(before) = m.history.pop()
        {
            *d = before;
            m.selection.retain(|id| d.nodes.iter().any(|n| &n.id == id));
        }
        if ui.button(t("Zoom to Fit")).clicked() {
            m.camera = None;
        }
    });
    r = d.evaluate(values, &d.preview);
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
    // Clicks, by tool.
    if response.clicked()
        && let Some(at) = response.interact_pointer_pos()
    {
        let mods = crate::canvas::click_modifiers(ui);
        let picked = r.pick(&|p| screen_of(&cam, p), px(at));
        let on_plane = cam.on_plane(px(at), 0.0);
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
                    _ => on_plane.map(|p| d.add_point(p)),
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
                    _ => on_plane.map(|p| d.add_point(p)),
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
    draw(ui, rect, &cam, d, &r, m, hovered.as_ref().map(Picked::id), hover);
    let tip = match &m.tool {
        Draw::Modify => t("Click to select, Ctrl adds; drag a point or a gizmo arrow to move it; drag elsewhere to orbit."),
        Draw::Point => t("Click on the work plane for a reference point, on a line for a point hosted on it."),
        Draw::Line(chain) if chain.is_empty() => t("Click points or the work plane; click the first point again to close the profile."),
        Draw::Line(_) => t("Click the next point; the first again closes the profile; Enter or Escape finishes it open."),
    };
    ui.label(egui::RichText::new(tip).color(pal().muted));
    if let Some(e) = ui.ctx().data_mut(|w| w.remove_temp::<String>(egui::Id::new("adaptive-view-problem"))) {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
}

/// What the view shows: the work plane, the forms shaded, the lines, the points with the adaptive ones numbered, the
/// selection in the accent, and the gizmo of a point selected.
#[allow(clippy::too_many_arguments)]
fn draw(ui: &Ui, rect: Rect, cam: &Camera, d: &Definition, r: &Evaluation, m: &Modeller, hovered: Option<&str>, hover: Option<Pos2>) {
    let p = ui.painter_at(rect);
    let visuals = ui.visuals();
    let (ink, faint, accent, hover_ink) = (visuals.text_color(), visuals.weak_text_color().gamma_multiply(0.5), pal().accent, pal().hover);
    p.rect_filled(rect, 2.0, visuals.extreme_bg_color);
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
    // The work plane: a grid at height 0, a line each metre over what the family takes up.
    let (lo, hi) = extent(r).unwrap_or(([-1000.; 3], [1000.; 3]));
    let step = 1000.0_f64;
    let (x0, x1) = ((lo[0] / step).floor() - 1., (hi[0] / step).ceil() + 1.);
    let (y0, y1) = ((lo[1] / step).floor() - 1., (hi[1] / step).ceil() + 1.);
    if (x1 - x0) < 200. && (y1 - y0) < 200. {
        let mut x = x0;
        while x <= x1 {
            p.line_segment([at([x * step, y0 * step, 0.]), at([x * step, y1 * step, 0.])], Stroke::new(1.0, faint));
            x += 1.;
        }
        let mut y = y0;
        while y <= y1 {
            p.line_segment([at([x0 * step, y * step, 0.]), at([x1 * step, y * step, 0.])], Stroke::new(1.0, faint));
            y += 1.;
        }
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
    // The points: an adaptive one filled and numbered, a reference point a ring, a hosted one a small ring.
    for id in &r.order {
        let Some(o) = r.origin(id) else { continue };
        let s = at(o);
        let color = tone(id, ink);
        match number(d, id) {
            Some(n) => {
                p.circle_filled(s, 5.0, color);
                p.text(s + vec2(7.0, -7.0), egui::Align2::LEFT_BOTTOM, n.to_string(), egui::FontId::proportional(13.0), color);
            }
            None => {
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
