//! Work planes: the plane a 3D view sketches on, as Revit's Work Plane > Set, and more than Revit's: a level, a surface of
//! the model (a wall's face, a roof's slope), or the plane through three points, each moved off by an offset along its
//! normal. A 3D view keeps its own (`View3D::work_plane`, set by the `work_plane` command); without one it sketches on the
//! lowest level, as Revit's 3D view does.
//!
//! A work plane may lie at any angle, upright too, for what is sketched on a wall (an opening, later). What is set on it is
//! up to the tool: floors, ceilings, roofs and shafts need a level one ([`is_level`]), and stand on the level below it
//! ([`host`]).
//!
//! Apart from the view's plane, a tool that draws on what is under the cursor (Wall, a component) takes a
//! [`CursorPlane`]: the level face there ([`crate::view3d::Surface`], which `Scene::surface_in` and `Mesh::surface` find),
//! at its height, as a level and an offset over it, which the plan's tools and their commands take; where nothing is under
//! the cursor, the lowest level's plane, the ground.

use serde::{Deserialize, Serialize};

use crate::geom::P2;
use crate::model::{Guid, Id, MAX_COORD, Model, TOL};
use crate::view3d::{Camera, P3, add, cross, dot, mul, sub, unit};
use crate::walls::Plane;

/// A 3D view's work plane: on `level`, through three `points`, or on a `surface` picked on the model; `offset` mm off it,
/// along its normal (up, for a level).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkPlane {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<Guid>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<[[f64; 3]; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<Surface>,
}

/// A surface of the model a work plane was set on: a point of it and the way it faces.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Surface {
    pub point: [f64; 3],
    pub normal: [f64; 3],
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// A plane in space: a point of it, and its normal, a unit vector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: P3,
    pub normal: P3,
}

impl Frame {
    /// The level plane at height `z`.
    pub fn flat(z: f64) -> Frame {
        Frame { origin: [0.0, 0.0, z], normal: [0.0, 0.0, 1.0] }
    }

    /// How steep it is, degrees from level: 0 lying level, 90 upright.
    pub fn slope(&self) -> f64 {
        self.normal[2].abs().clamp(0.0, 1.0).acos().to_degrees()
    }

    /// As a plane over the plan, as a sketch is drawn on: none for an upright one.
    pub fn over_plan(&self) -> Option<Plane> {
        let [nx, ny, nz] = self.normal;
        (nz.abs() > 1e-9).then(|| {
            let g = P2(-nx / nz, -ny / nz);
            Plane { z: self.origin[2] - g.dot(P2(self.origin[0], self.origin[1])), g }
        })
    }

    /// Two unit vectors across it: across is level (east on a level plane), up goes up it (north on a level one).
    pub fn axes(&self) -> (P3, P3) {
        let level = cross([0.0, 0.0, 1.0], self.normal);
        let across = if dot(level, level) < 1e-12 { [1.0, 0.0, 0.0] } else { unit(level) };
        (across, unit(cross(self.normal, across)))
    }

    /// The point of it nearest `p`.
    pub fn nearest(&self, p: P3) -> P3 {
        sub(p, mul(self.normal, dot(sub(p, self.origin), self.normal)))
    }

    /// Moved `by` mm along its normal.
    pub fn moved(self, by: f64) -> Frame {
        Frame { origin: add(self.origin, mul(self.normal, by)), ..self }
    }
}

impl From<Plane> for Frame {
    /// A plane over the plan, as a sloped floor's top is, as a plane in space.
    fn from(p: Plane) -> Frame {
        Frame { origin: [0.0, 0.0, p.z], normal: unit([-p.g.0, -p.g.1, 1.0]) }
    }
}

impl WorkPlane {
    /// On level `level`, `offset` mm above it.
    pub fn on(level: Guid, offset: f64) -> WorkPlane {
        WorkPlane { level: Some(level), offset, ..WorkPlane::default() }
    }

    /// The plane it is in `m`, or what's wrong with it, said so as to be fixed.
    pub fn resolve(&self, m: &Model) -> Result<Frame, String> {
        if !self.offset.is_finite() || self.offset.abs() > MAX_COORD {
            return Err(format!("its offset {} isn't a length", self.offset));
        }
        let plane = match (&self.level, &self.points, &self.surface) {
            (Some(level), None, None) => {
                if m.elements.get(*level).is_none() {
                    return Err("its level isn't in the project".into());
                }
                Frame::flat(m.elevation(*level))
            }
            (None, Some(points), None) => through(points)?,
            (None, None, Some(s)) => on_surface(s)?,
            (None, None, None) => return Err("a work plane needs a `level`, three `points` or a `surface`".into()),
            _ => return Err("a work plane is on a `level`, through three `points` or on a `surface`: only one of them".into()),
        };
        Ok(plane.moved(self.offset))
    }

    /// How steep it is, degrees from level: 0 for one on a level.
    pub fn slope(&self, m: &Model) -> f64 {
        self.resolve(m).map_or(0.0, |p| p.slope())
    }
}

/// Whether plane `p` lies level: what a floor, a ceiling, a roof or a shaft can be sketched on.
pub fn is_level(p: &Frame) -> bool {
    p.normal[0].abs() < 1e-9 && p.normal[1].abs() < 1e-9
}

fn place(p: &[f64; 3]) -> bool {
    p.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD)
}

/// The plane through three points, facing up where it isn't upright: refused when they lie along a line.
pub fn through(points: &[[f64; 3]; 3]) -> Result<Frame, String> {
    if !points.iter().all(place) {
        return Err("a point of the work plane isn't a place in the project".into());
    }
    let [a, b, c] = *points;
    let (u, v) = (sub(b, a), sub(c, a));
    let n = cross(u, v);
    let size = dot(n, n).sqrt();
    let len = |d: P3| dot(d, d).sqrt();
    let longest = len(u).max(len(v)).max(len(sub(c, b)));
    // A triangle of no area, or one less than a millimetre high: they lie along a line.
    if size.is_nan() || size <= TOL * longest.max(1.0) {
        return Err("the work plane's three points lie along a line: pick three that make a triangle".into());
    }
    let n = mul(n, 1.0 / size);
    let n = if n[2] < -1e-9 { mul(n, -1.0) } else { n };
    Ok(Frame { origin: a, normal: n })
}

/// The plane of a surface picked on the model.
fn on_surface(s: &Surface) -> Result<Frame, String> {
    if !place(&s.point) {
        return Err("the work plane's surface point isn't a place in the project".into());
    }
    let size = dot(s.normal, s.normal).sqrt();
    if !(size.is_finite() && size > 1e-9) {
        return Err("the work plane's surface `normal` is the way it faces, [x, y, z], and can't be [0, 0, 0]".into());
    }
    Ok(Frame { origin: s.point, normal: mul(s.normal, 1.0 / size) })
}

/// The level what's made at `at` on plane `p` stands on, and how far above it it is there: the highest level at or below
/// the plane, else the lowest.
pub fn host(m: &Model, p: Plane, at: P2) -> Option<(Id, f64)> {
    let z = p.at(at);
    let levels = m.levels();
    let (id, l) = levels.iter().rev().find(|(_, l)| l.elevation <= z + TOL).or(levels.first())?;
    z.is_finite().then_some((*id, z - l.elevation))
}

/// A level plane a tool draws on: the element whose face it is (a level, for its own plane or the ground), the level its
/// points go on and how far over it, mm, and its height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorPlane {
    pub on: Id,
    pub level: Id,
    pub offset: f64,
    pub z: f64,
}

/// How nearly straight up a face's normal turns for the face to be level: within about half a degree.
const UP: f64 = 0.99995;

/// The plane a tool draws on at pixel `at` of a 3D view through `cam`, where the line of sight meets `under`: a level face turned up at
/// its height, on its element's level (a level's plane, that level), else on the highest level at or below it (the lowest,
/// if none is). Nothing under the cursor, the lowest level's plane. None for a face that isn't level or is turned down, or
/// with no level to stand on.
pub fn cursor_plane(m: &Model, cam: &Camera, at: (f64, f64), under: Option<crate::view3d::Surface>) -> Option<CursorPlane> {
    let levels = m.levels();
    let ground = || {
        let (id, l) = levels.first()?;
        Some(crate::view3d::Surface { id: *id, at: cam.on_plane(at, l.elevation)?, normal: [0.0, 0.0, 1.0] })
    };
    let s = under.or_else(ground)?;
    let z = s.at[2];
    if !(s.normal[2] >= UP && z.is_finite()) {
        return None;
    }
    let own = m.level(s.id).map(|_| s.id).or_else(|| m.elements.get(s.id).and_then(|e| m.level_of(e)).and_then(|g| m.elements.id(g)));
    let below = levels.iter().rev().find(|(_, l)| l.elevation <= z + TOL).or(levels.first()).map(|(id, _)| *id);
    let level = own.or(below)?;
    // To the micron, as the commands keep points: no float noise from the line of sight.
    let offset = ((z - m.elevation(level)) * 1e3).round() / 1e3 + 0.0;
    Some(CursorPlane { on: s.id, level, offset, z: m.elevation(level) + offset })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use crate::graphics::Look;
    use crate::view3d::{AZIMUTH, ELEVATION, Scene, Surface};

    #[test]
    fn a_level_face_is_drawn_on_at_its_height_over_its_elements_level_and_a_face_that_isnt_level_is_not() {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
               floor {"offset": 500}"#,
        )
        .unwrap();
        let m = &s.model;
        let levels = m.levels();
        let (one, two) = (levels[0].0, levels[1].0);
        let floor = m.elements.iter().find(|(_, e)| e.category() == "floor").map(|(id, _)| *id).unwrap();
        let wall = m.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id).unwrap();
        let (scene, look) = (Scene::new(m), Look::of_3d(m));
        let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let image = scene.render_in(&cam, &[], &look);
        // The floor's top, picked where the picture shows it: on its level, 500 up.
        let at = cam.project([2000.0, 5000.0, 500.0]);
        let under = scene.surface_in(&image, &cam, (at.0, at.1));
        assert_eq!(under.map(|u| u.id), Some(floor));
        let plane = cursor_plane(m, &cam, (at.0, at.1), under).unwrap();
        assert_eq!((plane.on, plane.level, plane.offset, plane.z), (floor, one, 500.0, 500.0));
        // A wall's face stands up: nothing to draw on.
        let at = cam.project([5000.0, -100.0, 1500.0]);
        let under = scene.surface_in(&image, &cam, (at.0, at.1));
        assert_eq!(under.map(|u| u.id), Some(wall));
        assert_eq!(cursor_plane(m, &cam, (at.0, at.1), under), None);
        // Nothing under the cursor: the ground, Level 1's plane.
        let plane = cursor_plane(m, &cam, (3.0, 3.0), None).unwrap();
        assert_eq!((plane.on, plane.level, plane.offset), (one, one, 0.0));
        // Level 2's plane, and a face of nothing in the model (a linked one's) 200 above it: each over Level 2.
        let (on, up) = (|id, z| Surface { id, at: [1000.0, 1000.0, z], normal: [0.0, 0.0, 1.0] }, m.elevation(two));
        let plane = cursor_plane(m, &cam, (0.0, 0.0), Some(on(two, up))).unwrap();
        assert_eq!((plane.level, plane.offset), (two, 0.0));
        let plane = cursor_plane(m, &cam, (0.0, 0.0), Some(on(1, up + 200.0))).unwrap();
        assert_eq!((plane.on, plane.level, plane.offset), (1, two, 200.0));
        // A floor's bottom, seen from under it, is turned down.
        assert_eq!(cursor_plane(m, &cam, (0.0, 0.0), Some(Surface { normal: [0.0, 0.0, -1.0], ..on(floor, 300.0) })), None);
    }
}
