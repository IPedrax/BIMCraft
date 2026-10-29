//! Work planes: what a 3D view sketches on, a level, a level with an offset, or the plane through three points, set by the
//! `work_plane` command and kept in the file; and where on it a new element stands.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::graphics::Look;
use crate::model::{Element, Model};
use crate::view3d::{Camera, Scene};
use crate::workplane;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} was taken: {v}"),
        Err(e) => e,
    }
}

fn plane_of(m: &Model) -> crate::walls::Plane {
    m.work_plane(m.default_3d()).resolve(m).unwrap()
}

#[test]
fn a_3d_view_sketches_on_the_lowest_level_until_its_work_plane_is_set_to_a_level_and_an_offset() {
    let mut s = Session::new();
    assert_eq!(plane_of(&s.model), crate::walls::Plane::flat(0.0), "the lowest level, as Revit's 3D view starts");
    let r = run(&mut s, "work_plane", json!({"level": "Level 2", "offset": 500}));
    let up = s.model.elevation(s.model.levels()[1].0);
    assert_eq!(r["z"], json!(up + 500.0));
    assert_eq!(plane_of(&s.model), crate::walls::Plane::flat(up + 500.0));
    // Kept in the file, by the level's GUID.
    let text = s.model.to_json().unwrap();
    let back = Model::read(&text).unwrap().0;
    assert_eq!(plane_of(&back), crate::walls::Plane::flat(up + 500.0));
    // A level that goes takes its work plane with it: the view is back on the lowest level.
    let level2 = s.model.levels()[1].0;
    run(&mut s, "delete", json!({"ids": [level2]}));
    assert_eq!(plane_of(&s.model), crate::walls::Plane::flat(0.0));
    // Given nothing, it's the default again.
    run(&mut s, "work_plane", json!({"level": "Level 1", "offset": 200}));
    run(&mut s, "work_plane", json!({}));
    assert_eq!(plane_of(&s.model), crate::walls::Plane::flat(0.0));
}

#[test]
fn a_work_plane_through_three_points_may_slope_but_not_stand_upright_or_lie_along_a_line() {
    let mut s = Session::new();
    // Rising 1000 mm over 4000 mm toward the north: 14.04°.
    let r = run(&mut s, "work_plane", json!({"points": [[0, 0, 300], [4000, 0, 300], [0, 4000, 1300]]}));
    let slope = r["slope"].as_f64().unwrap();
    assert!((slope - 14.036).abs() < 0.01, "{r}");
    let p = plane_of(&s.model);
    assert!((p.at(P2(2000.0, 2000.0)) - 800.0).abs() < 1e-6, "{p:?}");
    let e = fails(&mut s, "work_plane", json!({"points": [[0, 0, 0], [1000, 0, 0], [2000, 0, 0]]}));
    assert!(e.contains("line"), "{e}");
    let e = fails(&mut s, "work_plane", json!({"points": [[0, 0, 0], [1000, 0, 0], [0, 0, 1000]]}));
    assert!(e.contains("upright"), "{e}");
    let e = fails(&mut s, "work_plane", json!({"points": [[0, 0, 0], [1000, 0, 0], [0, 100, 1000]]}));
    assert!(e.contains("75"), "steeper than a floor may be: {e}");
    let e = fails(&mut s, "work_plane", json!({"level": "Level 1", "points": [[0, 0, 0], [1000, 0, 0], [0, 1000, 0]]}));
    assert!(e.contains("either"), "{e}");
    let e = fails(&mut s, "work_plane", json!({"level": "Level 1", "ofset": 3}));
    assert!(e.contains("ofset"), "{e}");
    assert!((plane_of(&s.model).at(P2(0.0, 4000.0)) - 1300.0).abs() < 1e-6, "a refusal leaves it as it was");
}

#[test]
fn what_is_made_on_a_work_plane_stands_on_the_level_below_it_at_the_height_between() {
    let s = Session::new();
    let (l1, l2) = (s.model.levels()[0].0, s.model.levels()[1].0);
    let up = s.model.elevation(l2);
    assert_eq!(workplane::host(&s.model, crate::walls::Plane::flat(up + 300.0), P2(0.0, 0.0)), Some((l2, 300.0)));
    assert_eq!(workplane::host(&s.model, crate::walls::Plane::flat(up - 300.0), P2(0.0, 0.0)), Some((l1, up - 300.0)));
    // Below every level, on the lowest, under it.
    assert_eq!(workplane::host(&s.model, crate::walls::Plane::flat(-500.0), P2(0.0, 0.0)), Some((l1, -500.0)));
}

#[test]
fn a_sloped_floors_sketch_lies_on_its_top_and_the_camera_finds_points_on_a_sloped_plane() {
    let mut s = Session::new();
    let f = run(
        &mut s,
        "floor",
        json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]], "offset": 100, "slope": 10, "arrow": [[0, 0], [0, 4000]]}),
    );
    let id = f["id"].as_u64().unwrap();
    let Some(Element::Floor(fl)) = s.model.elements.get(id) else { panic!() };
    assert_eq!(s.model.sketch_plane(id), Some(s.model.floor_top(fl)));
    let plane = s.model.floor_top(fl);
    let cam = Camera { azimuth: -60.0, elevation: 35.0, scale: 0.1, target: [2000.0, 2000.0, 0.0], width: 800, height: 600 };
    let q = [1500.0, 2500.0, plane.at(P2(1500.0, 2500.0))];
    let (x, y, _) = cam.project(q);
    let back = cam.on_slope((x, y), plane).unwrap();
    assert!(back.iter().zip(q).all(|(a, b)| (a - b).abs() < 1e-6), "{back:?}");
}

#[test]
fn the_3d_pictures_say_where_on_the_model_a_pixel_shows() {
    let mut s = Session::new();
    run(&mut s, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]], "offset": 0}));
    let scene = Scene::new(&s.model);
    let cam = Camera { azimuth: -60.0, elevation: 35.0, scale: 0.1, target: [2000.0, 2000.0, 0.0], width: 800, height: 600 };
    let q = [1500.0, 2500.0, 0.0];
    let (x, y, _) = cam.project(q);
    let image = scene.render_in(&cam, &[], &Look::of_3d(&s.model));
    let at = image.point(&cam, x, y).expect("the floor's top");
    // Within a pixel or two of it, a pixel being 10 mm.
    assert!(at.iter().zip(q).all(|(a, b)| (a - b).abs() < 25.0), "{at:?}");
    assert_eq!(image.point(&cam, 2.0, 2.0), None, "nothing at the corner");
    let mesh = crate::view3d::gpu::Mesh::new(&scene, &Look::of_3d(&s.model)).unwrap();
    let at = mesh.point(&cam, x, y).expect("the floor's top, on the card's mesh");
    assert!(at.iter().zip(q).all(|(a, b)| (a - b).abs() < 25.0), "{at:?}");
}
