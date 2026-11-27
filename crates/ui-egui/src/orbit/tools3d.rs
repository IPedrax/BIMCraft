//! Move (MV), Copy (CO), Rotate (RO), Mirror (MM, DM) and Level (LL) in the 3D view. Their points are taken as the drawing
//! tools take theirs: on the face under the cursor, snapped as its level's plan snaps, the second on the plane of the first. While
//! a second point is wanted the selection's shadow shows where a click would leave it: each element's box (a wall's outline)
//! moved, turned or mirrored as the command would, drawn over the picture, so nothing is made again while the cursor moves; the
//! click runs the command, as a plan's does.
//!
//! Turned to a side with the triad (Front, Back, Left or Right), the view works on a plane facing it for now, as an elevation
//! does: through the camera's target, square to the way it looks. Move and Copy then take their points on that plane, so what
//! they move goes up and down as well as along (`move` and `copy` with a `dz`); Level places a level at the height clicked.
//! Orbited away, the view's own work plane is back.

use super::*;
use bimcraft_core::triad::{ENDS, Side};
use serde_json::Value;

/// A point an edit tool takes: where, the level plane it is on where it was taken on a face, and what it snapped to.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EditPoint {
    pub(crate) at: P3,
    pub(crate) plane: Option<CursorPlane>,
    pub(crate) snap: Option<&'static str>,
}

/// The shadow of an element: its outline in plan between two heights.
pub(crate) struct Prism {
    pub(crate) ring: Vec<P3>,
    pub(crate) height: f64,
}

/// The plane a 3D view looking straight at a side of the model works on for now: through the camera's target, facing the
/// camera; its point, its normal (toward the camera) and the side. None in a view at any other angle, and from above or below,
/// where the view's own work plane is level already.
pub(crate) fn facing(cam: &Camera) -> Option<(P3, P3, Side)> {
    let end = ENDS.into_iter().filter(|e| e.axis() < 2).find(|e| bimcraft_core::triad::looks_from(cam, *e))?;
    Some((cam.target, end.direction(), end.side()))
}

impl BimApp {
    /// Whether the tool is one this module takes the 3D view's clicks for.
    pub(super) fn edits_3d(&self) -> bool {
        matches!(self.tool, Tool::Move(_) | Tool::Copy(_) | Tool::Rotate { .. } | Tool::Mirror { .. } | Tool::Level)
    }

    /// Whether the tool takes its points on the plane facing the camera, where there is one: Move, Copy and Level.
    fn vertical_tool(&self) -> bool {
        matches!(self.tool, Tool::Move(_) | Tool::Copy(_) | Tool::Level)
    }

    /// The plane facing the camera the tool works on, if the view looks straight at a side and the tool is one that does.
    pub(crate) fn facing_now(&self) -> Option<(P3, P3, Side)> {
        self.orbit.camera.as_ref().filter(|_| self.edits_3d() && self.vertical_tool()).and_then(facing)
    }

    /// The point the edit tool would take at pixel `at` of the 3D view showing `base`: on the plane facing the camera
    /// ([`BimApp::facing_point`]), else on the face under the cursor, snapped, the second on the first's plane; with the plane
    /// it is on and what it snapped to. Shift, or the Options Bar's Constrain, holds Move and Copy square to their base.
    pub(super) fn edit_point(&self, base: &Model, at: Option<(f64, f64)>, shift: bool) -> Option<EditPoint> {
        if !self.edits_3d() {
            return None;
        }
        let o = &self.orbit;
        let (cam, at) = (o.camera?, at?);
        let square = shift || (self.constrain && matches!(self.tool, Tool::Move(_) | Tool::Copy(_)));
        if let Some((origin, normal, _)) = self.facing_now() {
            return self.facing_point(base, &cam, at, (origin, normal), square);
        }
        let scene = o.scene.as_ref().map(|(_, s)| s.as_ref());
        let under = o.picker.as_ref().and_then(|p| p.surface(scene, &cam, at));
        // Level, away from a side: the height of the surface under the cursor.
        if self.tool == Tool::Level {
            return under.map(|u| EditPoint { at: u.at.map(|v| v.round() + 0.0), plane: None, snap: None });
        }
        let first = self.edit_first();
        let held = first.and_then(|f| o.plane.filter(|(q, _)| *q == f)).map(|(_, p)| p);
        // On a face that isn't level (a wall's side), the point of it at its own height, on the plane level with it.
        let upright = || {
            let u = under?;
            let ground = bimcraft_core::workplane::cursor_plane(base, &cam, at, None)?;
            let z = u.at[2];
            Some((u.at, CursorPlane { on: u.id, offset: ground.offset + z - ground.z, z, ..ground }))
        };
        let (q, plane) = aim(base, &cam, at, held, None, under).or_else(upright)?;
        let from: Vec<P2> = first.into_iter().collect();
        let f = snap3d(base, &cam, at, o.ppp, q, plane, &from, square, &self.snaps_now());
        let snap = (!matches!(f.kind, Kind::Free)).then(|| f.kind.name());
        Some(EditPoint { at: [f.at.0, f.at.1, plane.z], plane: Some(plane), snap })
    }

    /// The point on the plane facing the camera, through `origin` square to `normal`, at pixel `at`: a corner the view shows
    /// within the snap's reach, else a point of an edge within a pick's, each taken onto the plane; its height a level's where it
    /// comes within the snap's reach of one; else to 10 mm along the plane and up. With `square`, from Move's or Copy's base
    /// only along or only up, whichever it is farther.
    fn facing_point(&self, base: &Model, cam: &Camera, at: (f64, f64), (origin, normal): (P3, P3), square: bool) -> Option<EditPoint> {
        let o = &self.orbit;
        let raw = cam.on(at, origin, normal)?;
        let dot = |a: P3, b: P3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let along = [-normal[1], normal[0], 0.0];
        // A point taken onto the plane, and a point by how far along the plane and how high.
        let onto = |q: P3| {
            let d = dot([q[0] - origin[0], q[1] - origin[1], q[2] - origin[2]], normal);
            [q[0] - normal[0] * d, q[1] - normal[1] * d, q[2] - normal[2] * d]
        };
        let at_plane = |s: f64, z: f64| [origin[0] + along[0] * s, origin[1] + along[1] * s, z];
        let s_of = |q: P3| dot([q[0] - origin[0], q[1] - origin[1], q[2] - origin[2]], along);
        // How many mm a pixel is on the plane there.
        let (a, b) = (cam.project(raw), cam.project([raw[0] + along[0] * 1000.0, raw[1] + along[1] * 1000.0, raw[2]]));
        let mm_per_px = 1000.0 / (b.0 - a.0).hypot(b.1 - a.1).max(1e-6);
        let reach = SNAP * f64::from(o.ppp) * mm_per_px;
        let look = o.look(base);
        let scene = o.scene.as_ref().map(|(_, s)| s.as_ref());
        let mut snap = None;
        let mut q = match scene.and_then(|s| s.corner_near(cam, at, SNAP * f64::from(o.ppp), &look)) {
            Some(c) => {
                snap = Some(Kind::Endpoint.name());
                onto(c)
            }
            None => match scene.and_then(|s| s.edge_near(cam, at, f64::from(PICK) * f64::from(o.ppp), &look)) {
                Some((_, e)) => {
                    snap = Some(Kind::Nearest.name());
                    onto(e)
                }
                None => {
                    // A level's height, else 10 mm up; 10 mm along.
                    let level = base.levels().into_iter().map(|(_, l)| l.elevation).find(|z| (z - raw[2]).abs() <= reach);
                    if level.is_some() {
                        snap = Some(bimcraft_i18n::msg!("Level"));
                    }
                    let z = level.unwrap_or((raw[2] / 10.0).round() * 10.0);
                    at_plane((s_of(raw) / 10.0).round() * 10.0, z)
                }
            },
        };
        // Square to the base: only along the plane, or only up.
        if square
            && matches!(self.tool, Tool::Move(Some(_)) | Tool::Copy(Some(_)))
            && let Some(b) = self.orbit.base3
        {
            let (ds, dz) = (s_of(q) - s_of(b), q[2] - b[2]);
            q = if ds.abs() >= dz.abs() { at_plane(s_of(q), b[2]) } else { at_plane(s_of(b), q[2]) };
            snap = Some(bimcraft_i18n::msg!("Constrained"));
        }
        Some(EditPoint { at: q.map(|v| (v * 1e3).round() / 1e3 + 0.0), plane: None, snap })
    }

    /// The tool's first point, in plan, once clicked: Move's or Copy's base, Rotate's start ray, a drawn mirror axis's start.
    fn edit_first(&self) -> Option<P2> {
        match self.tool {
            Tool::Move(start) | Tool::Copy(start) => start,
            Tool::Rotate { start, place: false, .. } => start,
            Tool::Mirror { draw: true, start } => start,
            _ => None,
        }
    }

    /// The shadows of the selection where a click at `q` would leave it: each element's box, or a wall's outline, between its
    /// bottom and its top, moved, turned or mirrored as the command would. None while the tool has no first point.
    pub(crate) fn edit_ghosts(&self, q: P3) -> Vec<Prism> {
        let to = P2(q[0], q[1]);
        // Where a point goes.
        let map: Box<dyn Fn(P3) -> P3> = match self.tool {
            Tool::Move(Some(from)) | Tool::Copy(Some(from)) => {
                let d = match (self.facing_now(), self.orbit.base3) {
                    (Some(_), Some(b)) => [q[0] - b[0], q[1] - b[1], q[2] - b[2]],
                    _ => [to.0 - from.0, to.1 - from.1, 0.0],
                };
                Box::new(move |p: P3| [p[0] + d[0], p[1] + d[1], p[2] + d[2]])
            }
            Tool::Rotate { centre, start: Some(from), place: false } => {
                let a = crate::canvas::angle_between(centre, from, to, 0.0).to_radians();
                let (s, c) = a.sin_cos();
                Box::new(move |p: P3| {
                    let (x, y) = (p[0] - centre.0, p[1] - centre.1);
                    [centre.0 + x * c - y * s, centre.1 + x * s + y * c, p[2]]
                })
            }
            Tool::Mirror { draw: true, start: Some(a) } if a.dist(to) > TOL => {
                let u = (to - a).unit();
                Box::new(move |p: P3| {
                    let v = P2(p[0], p[1]) - a;
                    let along = u * v.dot(u);
                    let m = a + along * 2.0 - v;
                    [m.0, m.1, p[2]]
                })
            }
            _ => return Vec::new(),
        };
        let m = &self.session.model;
        let Some((_, scene)) = self.orbit.scene.as_ref() else { return Vec::new() };
        let ids: std::collections::HashSet<Id> = self.selection.iter().copied().collect();
        let boxes = scene.boxes(&ids);
        let mut out = Vec::new();
        for id in &self.selection {
            let (ring, bottom, top) = match m.wall(*id) {
                Some(w) => (bimcraft_core::walls::outline(m, *id), m.base_z(w), m.top_z(w)),
                None => match boxes.get(id) {
                    Some((lo, hi)) => (vec![P2(lo[0], lo[1]), P2(hi[0], lo[1]), P2(hi[0], hi[1]), P2(lo[0], hi[1])], lo[2], hi[2]),
                    None => continue,
                },
            };
            if ring.len() < 2 || !(top - bottom).is_finite() {
                continue;
            }
            out.push(Prism { ring: ring.iter().map(|q| map([q.0, q.1, bottom])).collect(), height: top - bottom });
        }
        out
    }

    /// An edit tool's click at `q` (on `plane`, where it was taken on a face), `hit` what the picture shows there.
    pub(super) fn edit_click(&mut self, point: Option<EditPoint>, hit: Option<(Id, P3)>) {
        let point = point.map(|e| (e.at, e.plane));
        let need = |app: &mut BimApp| {
            app.status = Status { text: t("Point at the model, or at a level's plane, to take a point.").into(), error: true };
        };
        match self.tool.clone() {
            Tool::Level => match point {
                Some((q, _)) => {
                    if self.run("level", json!({"elevation": q[2]})).is_some() {
                        let made = self.session.model.levels().into_iter().find(|(_, l)| l.elevation == q[2]);
                        if let Some((_, l)) = made {
                            self.status =
                                Status { text: tf("{} made at {}", &[&l.name, &self.session.model.with_unit(q[2])]), error: false };
                        }
                    }
                }
                None => need(self),
            },
            Tool::Mirror { draw: false, .. } => {
                let axis = hit.and_then(|(id, at)| crate::canvas::nearest_line(&self.session.model, id, P2(at[0], at[1])));
                match axis {
                    Some(axis) => self.reflect(axis),
                    None => self.status = Status { text: t("Pick a line, a wall or a grid to mirror about.").into(), error: true },
                }
            }
            tool => {
                let Some((q, plane)) = point else { return need(self) };
                let at = P2(q[0], q[1]);
                if self.selection.is_empty() {
                    self.status = Status { text: t("Select what to change first.").into(), error: true };
                    return;
                }
                match tool {
                    Tool::Move(None) | Tool::Copy(None) => {
                        self.orbit.base3 = Some(q);
                        if let Some(plane) = plane {
                            self.orbit.plane = Some((at, plane));
                        }
                        self.tool = if matches!(tool, Tool::Copy(_)) { Tool::Copy(Some(at)) } else { Tool::Move(Some(at)) };
                    }
                    Tool::Move(Some(from)) | Tool::Copy(Some(from)) => {
                        let copy = matches!(tool, Tool::Copy(_));
                        match (self.facing_now(), self.orbit.base3) {
                            (Some(_), Some(b)) => {
                                let by = [q[0] - b[0], q[1] - b[1], q[2] - b[2]].map(|v| v.round() + 0.0);
                                if by.iter().all(|v| v.abs() <= TOL) {
                                    return;
                                }
                                let made = self.run(if copy { "copy" } else { "move" }, json!({"ids": self.selection, "by": by}));
                                match made {
                                    Some(_) if copy && self.multiple => {}
                                    Some(made) => {
                                        if copy {
                                            self.selection = made["ids"]
                                                .as_array()
                                                .map(|a| a.iter().filter_map(Value::as_u64).collect())
                                                .unwrap_or_default();
                                        }
                                        self.tool = Tool::Modify;
                                    }
                                    None => {}
                                }
                            }
                            _ => self.shift_selection(from, at),
                        }
                    }
                    Tool::Rotate { centre, start, place } => {
                        self.tool = match (place, start) {
                            (true, _) => Tool::Rotate { centre: at, start: None, place: false },
                            (false, None) => {
                                if let Some(plane) = plane {
                                    self.orbit.plane = Some((at, plane));
                                }
                                Tool::Rotate { centre, start: Some(at), place: false }
                            }
                            (false, Some(from)) => return self.turn(centre, crate::canvas::angle_between(centre, from, at, 0.0)),
                        };
                    }
                    Tool::Mirror { draw: true, start } => match start {
                        None => {
                            if let Some(plane) = plane {
                                self.orbit.plane = Some((at, plane));
                            }
                            self.tool = Tool::Mirror { draw: true, start: Some(at) };
                        }
                        Some(a) => self.reflect((a, at)),
                    },
                    _ => {}
                }
            }
        }
    }

    /// The edit tool's marks over the picture: the point a click takes, the line from the first point to it with how long it is
    /// (and how far up, on a plane facing the camera), Rotate's centre, and Level's line at the height a click makes it.
    pub(super) fn paint_edit(&self, p: &egui::Painter, point: Option<EditPoint>, screen: &dyn Fn(P3) -> Pos2) {
        let q = point.map(|e| e.at);
        let accent = pal().accent;
        let m = &self.session.model;
        if let Tool::Rotate { centre, .. } = self.tool {
            let z = q.map_or(0.0, |q| q[2]);
            let c = screen([centre.0, centre.1, z]);
            if c.is_finite() {
                p.circle_stroke(c, 6.0, Stroke::new(1.5, accent));
                p.line_segment([c - vec2(9.0, 0.0), c + vec2(9.0, 0.0)], Stroke::new(1.0, accent));
                p.line_segment([c - vec2(0.0, 9.0), c + vec2(0.0, 9.0)], Stroke::new(1.0, accent));
            }
        }
        let Some(q) = q else { return };
        let at = screen(q);
        if !at.is_finite() {
            return;
        }
        if self.tool == Tool::Level {
            // A line through the point across the view, level: along the plane facing the camera, else along the screen.
            let (a, b) = match self.facing_now() {
                Some((_, n, _)) => {
                    let along = [-n[1], n[0], 0.0];
                    let far = 1.0e6;
                    (
                        screen([q[0] - along[0] * far, q[1] - along[1] * far, q[2]]),
                        screen([q[0] + along[0] * far, q[1] + along[1] * far, q[2]]),
                    )
                }
                None => (at - vec2(2000.0, 0.0), at + vec2(2000.0, 0.0)),
            };
            if a.is_finite() && b.is_finite() {
                p.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(1.5, accent), 10.0, 6.0));
            }
            crate::canvas::tag(p, at + vec2(12.0, -12.0), egui::Align2::LEFT_BOTTOM, tf("Level at {}", &[&m.with_unit(q[2])]));
            return;
        }
        for prism in self.edit_ghosts(q) {
            paint_prism(p, &prism, screen);
        }
        let diamond = [vec2(0.0, -6.0), vec2(6.0, 0.0), vec2(0.0, 6.0), vec2(-6.0, 0.0)].map(|d| at + d).to_vec();
        p.add(egui::Shape::closed_line(diamond, Stroke::new(2.0, accent)));
        // What it snapped to.
        if let Some(kind) = point.and_then(|e| e.snap) {
            crate::canvas::tag(p, at + vec2(12.0, 14.0), egui::Align2::LEFT_TOP, t(kind));
        }
        let from = match self.tool {
            Tool::Move(Some(_)) | Tool::Copy(Some(_)) => self.orbit.base3,
            Tool::Rotate { centre, start: Some(_), place: false } => Some([centre.0, centre.1, q[2]]),
            Tool::Mirror { draw: true, start: Some(a) } => Some([a.0, a.1, q[2]]),
            _ => None,
        };
        let Some(b) = from else { return };
        let fb = screen(b);
        if !fb.is_finite() {
            return;
        }
        p.line_segment([fb, at], Stroke::new(1.0, accent));
        let text = match self.tool {
            Tool::Rotate { centre, start: Some(s), .. } => {
                format!("{:.1}°", crate::canvas::angle_between(centre, s, P2(q[0], q[1]), 0.0))
            }
            _ if self.facing_now().is_some() => {
                let across = P2(q[0] - b[0], q[1] - b[1]).length();
                tf("{} across, {} up", &[&m.with_unit(across), &m.with_unit(q[2] - b[2])])
            }
            _ => m.with_unit(P2(q[0] - b[0], q[1] - b[1]).length()),
        };
        crate::canvas::tag(p, at + vec2(12.0, -12.0), egui::Align2::LEFT_BOTTOM, text);
    }

    /// While the view looks straight at a side and the tool works on the plane facing it, a word in the view's corner saying so.
    pub(super) fn paint_facing(&self, p: &egui::Painter, rect: Rect) {
        let Some((_, _, side)) = self.facing_now() else { return };
        let text = tf("{}: the work plane faces you; points go up and down as well as along", &[crate::lang::tc("side", side.name())]);
        // Over the view's lower left, clear of the Options Bar at its top and the view control bar at its foot.
        let at = rect.left_bottom() + vec2(12.0, -64.0);
        crate::canvas::tag(p, at, egui::Align2::LEFT_BOTTOM, text);
    }
}

/// A shadow drawn over the picture: its sides filled faintly, its edges over them.
fn paint_prism(p: &egui::Painter, prism: &Prism, screen: &dyn Fn(P3) -> Pos2) {
    let low: Vec<Pos2> = prism.ring.iter().map(|q| screen(*q)).collect();
    let high: Vec<Pos2> = prism.ring.iter().map(|q| screen([q[0], q[1], q[2] + prism.height])).collect();
    if low.iter().chain(&high).any(|q| !q.is_finite()) {
        return;
    }
    let shadow = super::openings::SHADOW;
    let fill = Color32::from_rgba_unmultiplied(shadow.r(), shadow.g(), shadow.b(), 36);
    let n = low.len();
    for i in 0..n {
        let j = (i + 1) % n;
        p.add(egui::Shape::convex_polygon(vec![low[i], low[j], high[j], high[i]], fill, Stroke::NONE));
    }
    let stroke = Stroke::new(1.5, shadow);
    p.add(egui::Shape::closed_line(low.clone(), stroke));
    p.add(egui::Shape::closed_line(high.clone(), stroke));
    for (a, b) in low.iter().zip(&high) {
        p.line_segment([*a, *b], stroke);
    }
}
