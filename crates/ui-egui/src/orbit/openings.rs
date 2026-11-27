//! Door (DR) and Window (WN) in the 3D view: the wall under the cursor takes the opening where the cursor is on it, and its
//! shadow shows there before the click: the box it would fill in the wall, as wide and tall as its type and as deep as the wall,
//! drawn over the picture (nothing is made again for it, so the view never waits on the cursor). A click places it with the same
//! parameters, as one `door` or `window` command; a door's hinge and a window's face are the plan's (Space flips them), its sill
//! its type's.

use super::*;
use bimcraft_core::model::Element;
use bimcraft_core::walls::{self, Axis};
use serde_json::Value;

/// The colour of a shadow of what a click would make: a grey-blue, filled faintly, edged stronger.
pub(super) const SHADOW: Color32 = Color32::from_rgb(70, 120, 190);

/// The opening a click in the 3D view would make: what `door` or `window` would be given, and the eight corners of the box it
/// would fill (the near face's four, then the far face's), where it can say.
pub(crate) struct OpeningAim {
    pub(crate) params: Value,
    pub(crate) ghost: Option<[P3; 8]>,
}

/// A box's twelve edges, by its corners as [`OpeningAim::ghost`] has them.
const EDGES: [(usize, usize); 12] = [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)];
/// Its six faces.
const FACES: [[usize; 4]; 6] = [[0, 1, 2, 3], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]];

/// A box drawn as a shadow: its faces filled faintly, its edges over them.
pub(super) fn paint_ghost(p: &egui::Painter, corners: &[P3; 8], screen: &dyn Fn(P3) -> Pos2) {
    let at: Vec<Pos2> = corners.iter().map(|q| screen(*q)).collect();
    if at.iter().any(|q| !q.is_finite()) {
        return;
    }
    let fill = Color32::from_rgba_unmultiplied(SHADOW.r(), SHADOW.g(), SHADOW.b(), 46);
    for f in FACES {
        p.add(egui::Shape::convex_polygon(f.iter().map(|i| at[*i]).collect(), fill, Stroke::NONE));
    }
    for (a, b) in EDGES {
        p.line_segment([at[a], at[b]], Stroke::new(1.5, SHADOW));
    }
}

impl BimApp {
    /// The Door or Window tool's opening at pixel `at` of the 3D view showing `base`; none while another tool is on. Off every
    /// wall the last one stays, so a click just off it still places it.
    pub(super) fn opening_hover(&mut self, base: &Model, at: Option<(f64, f64)>) {
        let door = match self.tool {
            Tool::Door => true,
            Tool::Window => false,
            _ => {
                self.orbit.opening = None;
                return;
            }
        };
        if let Some(aim) = at.and_then(|at| self.opening_aim(base, at, door)) {
            self.orbit.opening = Some(aim);
        }
    }

    /// What `door` or `window` would be given for the wall under pixel `at`: the wall, where along it (the middle of its stretch
    /// of wall when near it), and which way it opens or faces; with the box it would fill. None where the cursor is on no wall.
    fn opening_aim(&self, base: &Model, at: (f64, f64), door: bool) -> Option<OpeningAim> {
        let orbit = &self.orbit;
        let cam = orbit.camera?;
        let scene = orbit.scene.as_ref().map(|(_, s)| s.as_ref());
        let hit = orbit.picker.as_ref()?.surface(scene, &cam, at)?;
        let host = hit.id;
        let w = base.wall(host)?;
        let cat = if door { "door" } else { "window" };
        let mut params = self.typed(cat, json!({"wall": host, "at": [hit.at[0].round(), hit.at[1].round()]}));
        let Ok(Element::Door(o) | Element::Window(o)) = bimcraft_core::opening_of(base, &params, door) else {
            return Some(OpeningAim { params, ghost: None });
        };
        // The middle of its stretch of wall, within SNAP px of it: how many mm a pixel is there, along the wall.
        let u = (w.end - w.start).unit();
        let (a, b) = (cam.project(hit.at), cam.project([hit.at[0] + u.0 * 100.0, hit.at[1] + u.1 * 100.0, hit.at[2]]));
        let px_per_mm = ((b.0 - a.0).hypot(b.1 - a.1) / 100.0).max(1e-6);
        let reach = SNAP * f64::from(orbit.ppp) / px_per_mm;
        let offset = base.centred(&o, reach).unwrap_or(o.offset).round();
        params["offset"] = json!(offset);
        params.as_object_mut().map(|p| p.remove("at"));
        // The side of the wall the cursor is on: a door swings into it, a window faces out of it.
        let side = Axis::of(w).project(P2(hit.at[0], hit.at[1])).1 < walls::centre(base, w);
        if door {
            if base.door_flips(o.family.as_deref(), &o.type_name).hand {
                params["flip_hand"] = json!(self.door_flip_hand);
                params["flip_facing"] = json!(side);
            }
        } else {
            params["flip_facing"] = json!(side ^ self.window_flip_facing);
        }
        let placed = bimcraft_core::model::Opening { offset, ..o };
        let e = if door { Element::Door(placed.clone()) } else { Element::Window(placed.clone()) };
        let ghost = base.opening_type(&e).zip(base.opening_placement(&placed)).map(|(size, at)| {
            let (o3, along, across) = (at.origin, at.axes[0], at.axes[1]);
            let (half, deep) = (size.width / 2.0, base.thickness(w).max(10.0) / 2.0 + 5.0);
            let corner = |s: f64, d: f64, up: f64| [0, 1, 2].map(|k| o3[k] + along[k] * s + across[k] * d + if k == 2 { up } else { 0.0 });
            [
                corner(-half, -deep, 0.0),
                corner(half, -deep, 0.0),
                corner(half, -deep, size.height),
                corner(-half, -deep, size.height),
                corner(-half, deep, 0.0),
                corner(half, deep, 0.0),
                corner(half, deep, size.height),
                corner(-half, deep, size.height),
            ]
        });
        Some(OpeningAim { params, ghost })
    }

    /// The shadow of the opening a click would place, over the picture.
    pub(super) fn paint_opening(&self, p: &egui::Painter, screen: &dyn Fn(P3) -> Pos2) {
        if let Some(ghost) = self.orbit.opening.as_ref().and_then(|a| a.ghost.as_ref()) {
            paint_ghost(p, ghost, screen);
        }
    }

    /// A click of Door or Window in the 3D view: the opening aimed at under the cursor, placed. Says why where it can't be.
    pub(super) fn place_opening_3d(&mut self) {
        let door = self.tool == Tool::Door;
        let cat = if door { "door" } else { "window" };
        let Some(aim) = self.orbit.opening.take() else {
            self.status = Status { text: t("Click a wall to place it.").into(), error: true };
            return;
        };
        // The placement takes the pending hinge or face; the next one starts as placed.
        if self.run(cat, aim.params).is_some() {
            self.door_flip_hand = false;
            self.window_flip_facing = false;
        }
    }

    /// The shadow of the opening a click would place: its box's corners.
    #[cfg(test)]
    pub(crate) fn opening_ghost(&self) -> Option<[P3; 8]> {
        self.orbit.opening.as_ref().and_then(|a| a.ghost)
    }

    /// Forgets the opening aimed at last, as if the cursor had never been on a wall.
    #[cfg(test)]
    pub(crate) fn orbit_forget_opening(&mut self) {
        self.orbit.opening = None;
    }
}
