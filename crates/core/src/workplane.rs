//! Work planes, as Revit's: what a tool draws on in a 3D view, taken from the surface under the cursor
//! ([`Surface`], which `Scene::surface_in` and `Mesh::surface` find). Level ones only yet: a face that is level and faces up (a
//! floor's top, a flat roof's, a level's plane) is drawn on at its height, as a level and an offset over it, which the plan's
//! tools and their commands take; where nothing is under the cursor, the lowest level's plane, the ground.

use crate::model::{Id, Model, TOL};
use crate::view3d::{Camera, Surface};

/// A level plane a tool draws on: the element whose face it is (a level, for its own plane or the ground), the level its
/// points go on and how far over it, mm, and its height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkPlane {
    pub on: Id,
    pub level: Id,
    pub offset: f64,
    pub z: f64,
}

/// How nearly straight up a face's normal turns for the face to be level: within about half a degree.
const UP: f64 = 0.99995;

/// The work plane at pixel `at` of a 3D view through `cam`, where the line of sight meets `under`: a level face turned up at
/// its height, on its element's level (a level's plane, that level), else on the highest level at or below it (the lowest,
/// if none is). Nothing under the cursor, the lowest level's plane. None for a face that isn't level or is turned down, or
/// with no level to stand on.
pub fn work_plane(m: &Model, cam: &Camera, at: (f64, f64), under: Option<Surface>) -> Option<WorkPlane> {
    let levels = m.levels();
    let ground = || {
        let (id, l) = levels.first()?;
        Some(Surface { id: *id, at: cam.on_plane(at, l.elevation)?, normal: [0.0, 0.0, 1.0] })
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
    Some(WorkPlane { on: s.id, level, offset, z: m.elevation(level) + offset })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use crate::graphics::Look;
    use crate::view3d::{AZIMUTH, ELEVATION, Scene};

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
        let plane = work_plane(m, &cam, (at.0, at.1), under).unwrap();
        assert_eq!((plane.on, plane.level, plane.offset, plane.z), (floor, one, 500.0, 500.0));
        // A wall's face stands up: nothing to draw on.
        let at = cam.project([5000.0, -100.0, 1500.0]);
        let under = scene.surface_in(&image, &cam, (at.0, at.1));
        assert_eq!(under.map(|u| u.id), Some(wall));
        assert_eq!(work_plane(m, &cam, (at.0, at.1), under), None);
        // Nothing under the cursor: the ground, Level 1's plane.
        let plane = work_plane(m, &cam, (3.0, 3.0), None).unwrap();
        assert_eq!((plane.on, plane.level, plane.offset), (one, one, 0.0));
        // Level 2's plane, and a face of nothing in the model (a linked one's) 200 above it: each over Level 2.
        let (on, up) = (|id, z| Surface { id, at: [1000.0, 1000.0, z], normal: [0.0, 0.0, 1.0] }, m.elevation(two));
        let plane = work_plane(m, &cam, (0.0, 0.0), Some(on(two, up))).unwrap();
        assert_eq!((plane.level, plane.offset), (two, 0.0));
        let plane = work_plane(m, &cam, (0.0, 0.0), Some(on(1, up + 200.0))).unwrap();
        assert_eq!((plane.on, plane.level, plane.offset), (1, two, 200.0));
        // A floor's bottom, seen from under it, is turned down.
        assert_eq!(work_plane(m, &cam, (0.0, 0.0), Some(Surface { normal: [0.0, 0.0, -1.0], ..on(floor, 300.0) })), None);
    }
}
