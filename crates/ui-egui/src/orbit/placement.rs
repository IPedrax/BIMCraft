//! Drag a window on its host, or a wall's selected location line over its base plane.
use super::*;
use bimcraft_core::engine::Session;
use bimcraft_core::model::Element;
use bimcraft_core::walls::{self, Axis};

/// Project the opening rectangle in its host plane, including a pending drag displacement.
fn window_ring(model: &Model, id: Id, by: P2, screen: impl Fn(P3) -> Pos2) -> Option<[Pos2; 4]> {
    let e @ Element::Window(w) = model.elements.get(id)? else { return None };
    let size = model.opening_type(e)?;
    let (center, axis, z) = model.opening_frame(w)?;
    let center = center + axis * by.0;
    let left = center - axis * (size.width / 2.0);
    let right = center + axis * (size.width / 2.0);
    let bottom = z + by.1;
    let top = bottom + size.height;
    let ring = [(left, bottom), (right, bottom), (right, top), (left, top)].map(|(xy, z)| screen([xy.0, xy.1, z]));
    ring.iter().all(|p| p.is_finite()).then_some(ring)
}

pub(super) fn selected_window(p: &egui::Painter, model: &Model, id: Id, screen: impl Fn(P3) -> Pos2, visible: impl Fn(Pos2) -> bool) {
    let Some(Element::Window(w)) = model.elements.get(id) else { return };
    let Some(ring) = window_ring(model, id, P2(0.0, 0.0), screen) else { return };
    let center = ring[0].lerp(ring[2], 0.5);
    // The picker accounts for intervening geometry, hidden elements and the section box.
    if !std::iter::once(center).chain(ring.map(|corner| corner.lerp(center, 0.12))).any(|at| p.clip_rect().contains(at) && visible(at)) {
        return;
    }
    let dark = egui::Color32::from_rgb(27, 36, 48);
    for (width, color) in [(5.0, dark), (2.0, egui::Color32::WHITE)] {
        p.add(egui::Shape::closed_line(ring.to_vec(), Stroke::new(width, color)));
    }
    let mark = w.mark.trim();
    let label = if mark.is_empty() { t("Window").to_owned() } else { format!("{} {}", t("Window"), mark) };
    let text = p.layout_no_wrap(label, egui::FontId::proportional(13.0), egui::Color32::WHITE);
    let size = text.size() + vec2(12.0, 6.0);
    let head = ring[2].lerp(ring[3], 0.5);
    let bounds = p.clip_rect();
    let left = (head.x - size.x * 0.5).clamp(bounds.left(), (bounds.right() - size.x).max(bounds.left()));
    let top = (head.y - size.y - 3.0).clamp(bounds.top(), (bounds.bottom() - size.y).max(bounds.top()));
    let badge = Rect::from_min_size(egui::pos2(left, top), size);
    p.rect_filled(badge, 3.0, dark);
    p.galley(badge.min + vec2(6.0, 3.0), text, egui::Color32::WHITE);
}

enum Picked {
    Window { offset: f64, sill: f64 },
    Wall { joined_end: bool },
}

pub(super) struct PlacementDrag {
    pub(super) base: Arc<Model>,
    pub(super) id: Id,
    picked: Picked,
    grab: P3,
    axis: P2,
    shift: bool,
    // Idle frames share the same validated snapshot with the scene worker.
    delta: Option<P2>,
    preview: Result<Arc<Model>, String>,
}

impl PlacementDrag {
    fn new(base: &Arc<Model>, id: Id, picked: Picked, grab: P3, axis: P2) -> Self {
        Self { base: Arc::clone(base), id, picked, grab, axis, shift: false, delta: Some(P2(0.0, 0.0)), preview: Ok(Arc::clone(base)) }
    }

    pub(super) fn window(base: &Arc<Model>, hit: Surface, cam: &Camera, pixel: (f64, f64)) -> Option<Self> {
        let Element::Window(w) = base.elements.get(hit.id)? else { return None };
        let wall = base.wall(w.host)?;
        let axis = (wall.end - wall.start).unit();
        if wall.arc != 0.0 {
            return None;
        }
        cam.on(pixel, hit.at, [-axis.1, axis.0, 0.0])?;
        Some(Self::new(base, hit.id, Picked::Window { offset: w.offset, sill: w.sill }, hit.at, axis))
    }

    pub(super) fn wall(base: &Arc<Model>, id: Id, grab: P3) -> Option<Self> {
        let w = base.wall(id)?;
        let axis = (w.end - w.start).unit();
        // An end that meets an unselected wall constrains the default drag across this wall.
        let ends = [w.start, w.end].map(|p| ([p.0, p.1, p.0, p.1], base.base_z(w), base.top_z(w)));
        let joined_end = w.join
            && base.lookup().walls_near_at(&ends, TOL).into_iter().filter(|other| *other != id).any(|other| {
                base.wall(other).is_some_and(|other| {
                    other.join
                        && walls::stacked(base, w, other)
                        && [w.start, w.end].iter().any(|p| Axis::of(other).distance(*p, 0.0) <= TOL)
                })
            });
        Some(Self::new(base, id, Picked::Wall { joined_end }, grab, axis))
    }

    fn displacement(&self, cam: &Camera, pixel: (f64, f64)) -> Option<P2> {
        match self.picked {
            Picked::Window { .. } => {
                let p = cam.on(pixel, self.grab, [-self.axis.1, self.axis.0, 0.0])?;
                let along = P2(p[0] - self.grab[0], p[1] - self.grab[1]).dot(self.axis);
                let rise = if self.shift { 0.0 } else { (p[2] - self.grab[2]).round() };
                Some(P2(along.round(), rise))
            }
            Picked::Wall { joined_end } => {
                let p = cam.on_plane(pixel, self.grab[2])?;
                let by = P2(p[0] - self.grab[0], p[1] - self.grab[1]);
                let normal = self.axis.perp();
                Some(if self.shift != joined_end { normal * by.dot(normal).round() } else { P2(by.0.round(), by.1.round()) })
            }
        }
    }

    fn command(&self, by: P2) -> (&'static str, serde_json::Value) {
        match self.picked {
            Picked::Window { offset, sill } => ("modify", json!({"id": self.id, "offset": offset + by.0, "sill": sill + by.1})),
            Picked::Wall { .. } => ("move", json!({"ids": [self.id], "by": [by.0, by.1]})),
        }
    }

    pub(super) fn update(&mut self, cam: &Camera, pixel: (f64, f64), shift: bool) {
        self.shift = shift;
        let delta = self.displacement(cam, pixel);
        if delta == self.delta {
            return;
        }
        self.delta = delta;
        self.preview = match delta {
            None => Err(t("The drag plane is edge-on to this view.").into()),
            Some(P2(0.0, 0.0)) => Ok(Arc::clone(&self.base)),
            Some(by) => {
                let (cmd, params) = self.command(by);
                let mut trial = Session::new();
                trial.model = Arc::clone(&self.base);
                trial.execute(cmd, &params).map(|_| trial.model)
            }
        };
    }

    pub(super) fn outline(&self, painter: &egui::Painter, screen: impl Fn(P3) -> Pos2) {
        let Some(by) = self.delta else { return };
        let ink = if self.preview.is_err() { pal().error } else { pal().accent };
        match self.picked {
            Picked::Window { sill, .. } => {
                if let Some(ring) = window_ring(&self.base, self.id, by, screen) {
                    let at = ring[2];
                    painter.add(egui::Shape::closed_line(ring.to_vec(), Stroke::new(2.0, ink)));
                    let height = self.base.units.with_unit(sill + by.1);
                    let label = if self.shift { tf("Sill locked: {}", &[&height]) } else { tf("Sill: {}", &[&height]) };
                    crate::canvas::tag(painter, at + vec2(8.0, -6.0), egui::Align2::LEFT_BOTTOM, label);
                }
            }
            Picked::Wall { .. } => {
                // The move command may settle a joined end. Draw the accepted endpoints when available.
                let (m, shift) = match &self.preview {
                    Ok(model) => (model, P2(0.0, 0.0)),
                    Err(_) => (&self.base, by),
                };
                let line = handles::location_line(m, self.id).into_iter().map(|p| screen([p[0] + shift.0, p[1] + shift.1, p[2]])).collect();
                painter.add(egui::Shape::line(line, Stroke::new(2.5, ink)));
            }
        }
    }

    pub(super) fn model(&self) -> Result<&Arc<Model>, &String> {
        self.preview.as_ref()
    }

    pub(super) fn commit(&self) -> Option<(&'static str, serde_json::Value)> {
        self.preview.as_ref().ok()?;
        self.delta.filter(|d| *d != P2(0.0, 0.0)).map(|by| self.command(by))
    }
}
