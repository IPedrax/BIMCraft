//! T12, the Work Plane dialog's engine: a 3D view's work plane set by name (a level, a named reference plane), on a
//! reference plane it then follows, Dissociate, and a family's planes by the names the dialog lists them by.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, Model};
use crate::workplane::{self, Frame};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} was taken: {v}"),
        Err(e) => e,
    }
}

fn frame_of(m: &Model) -> Frame {
    m.work_plane(m.default_3d()).resolve(m).unwrap()
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

/// A project with an upright reference plane, Ramp, along y at x = 1000, and a sloped one, Slope.
fn with_planes() -> Session {
    let mut s = Session::new();
    run(&mut s, "reference_plane", json!({"start": [1000, 0], "end": [1000, 4000], "name": "Ramp"}));
    run(&mut s, "reference_plane", json!({"start": [0, 0, 0], "end": [4000, 0, 0], "up": [0, 1, 1], "name": "Slope"}));
    s
}

fn ramp(s: &Session) -> u64 {
    let m = &s.model;
    m.rare("refplane").into_iter().find(|id| matches!(m.elements.get(*id), Some(Element::RefPlane(p)) if p.name == "Ramp")).unwrap()
}

#[test]
fn the_names_list_the_levels_then_the_named_reference_planes_and_a_work_plane_is_set_by_any_of_them() {
    let mut s = with_planes();
    run(&mut s, "reference_plane", json!({"start": [0, 0], "end": [0, 100]}));
    let names: Vec<String> = workplane::choices(&s.model).into_iter().map(|(_, n)| n).collect();
    assert_eq!(
        names,
        ["Level : Level 1", "Level : Level 2", "Reference Plane : Ramp", "Reference Plane : Slope"],
        "an unnamed plane isn't"
    );
    let up = s.model.elevation(s.model.levels()[1].0);
    let r = run(&mut s, "work_plane", json!({"name": "Level : Level 2", "offset": 100}));
    assert_eq!((r["name"].clone(), r["z"].clone()), (json!("Level : Level 2"), json!(up + 100.0)));
    // A bare name: the level's, the plane's.
    assert_eq!(run(&mut s, "work_plane", json!({"name": "Level 1"}))["name"], json!("Level : Level 1"));
    let r = run(&mut s, "work_plane", json!({"name": "Ramp"}));
    assert_eq!((r["name"].clone(), r["ref_plane"].clone()), (json!("Reference Plane : Ramp"), json!(ramp(&s))));
    let f = frame_of(&s.model);
    assert!(near(f.normal.map(f64::abs), [1.0, 0.0, 0.0]) && (f.origin[0] - 1000.0).abs() < 1e-9, "upright on x = 1000: {f:?}");
    // By `ref_plane` too, by name, id or GUID.
    assert_eq!(run(&mut s, "work_plane", json!({"ref_plane": "Slope"}))["name"], json!("Reference Plane : Slope"));
    let id = ramp(&s);
    assert_eq!(run(&mut s, "work_plane", json!({"ref_plane": id}))["name"], json!("Reference Plane : Ramp"));
    // A name of both a level and a plane is refused, naming both; one of neither lists them.
    run(&mut s, "reference_plane", json!({"start": [0, 0], "end": [0, 100], "name": "Level 1"}));
    let e = fails(&mut s, "work_plane", json!({"name": "Level 1"}));
    assert!(e.contains("Level : Level 1") && e.contains("Reference Plane : Level 1"), "{e}");
    assert!(fails(&mut s, "work_plane", json!({"name": "Roof"})).contains("Reference Plane : Ramp"));
    // One plane at a time; `ref_plane` names a reference plane.
    assert!(fails(&mut s, "work_plane", json!({"name": "Ramp", "level": "Level 1"})).contains("one of"));
    let wall = run(&mut s, "wall", json!({"points": [[0, 0], [1000, 0]]}))["ids"][0].clone();
    assert!(fails(&mut s, "work_plane", json!({"ref_plane": wall})).contains("isn't a reference plane"));
}

#[test]
fn a_work_plane_on_a_reference_plane_follows_it_until_dissociated_and_the_file_keeps_where_it_is() {
    let mut s = with_planes();
    run(&mut s, "work_plane", json!({"name": "Reference Plane : Ramp", "offset": 50}));
    let id = ramp(&s);
    run(&mut s, "move", json!({"ids": [id], "by": [500, 0]}));
    let f = frame_of(&s.model);
    assert!((f.origin[0] - 1550.0).abs() < 1e-6, "moved with it, 50 out along its normal: {f:?}");
    // Its snapshot keeps step: what a build that doesn't know `ref_plane` reads.
    let wp = s.model.work_plane(s.model.default_3d());
    let snap = wp.surface.expect("a surface beside it");
    assert!((snap.point[0] - 1500.0).abs() < 1e-6, "{snap:?}");
    let mut text: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    let view = text["elements"].as_object_mut().unwrap().values_mut().find(|e| e["category"] == "view3d").unwrap();
    view["work_plane"].as_object_mut().unwrap().remove("ref_plane");
    let older = Model::read(&text.to_string()).unwrap().0;
    assert!(near(frame_of(&older).origin, f.origin) && near(frame_of(&older).normal, f.normal), "the same plane, dissociated");
    // Turned: it turns with it.
    run(&mut s, "rotate", json!({"ids": [id], "about": [1500, 0], "angle": 90}));
    assert!(near(frame_of(&s.model).normal.map(f64::abs), [0.0, 1.0, 0.0]), "{:?}", frame_of(&s.model));
    // Dissociate: where it is, no longer following.
    let before = frame_of(&s.model);
    let r = run(&mut s, "work_plane", json!({"dissociate": true}));
    assert_eq!((r["name"].clone(), r["ref_plane"].clone()), (json!("Surface"), Value::Null));
    assert!(near(frame_of(&s.model).origin, before.origin) && near(frame_of(&s.model).normal, before.normal));
    run(&mut s, "move", json!({"ids": [id], "by": [0, 900]}));
    assert!(near(frame_of(&s.model).origin, before.origin), "stays where it was");
    let e = fails(&mut s, "work_plane", json!({"dissociate": true}));
    assert!(e.contains("nothing to dissociate"), "{e}");
}

#[test]
fn deleting_the_work_planes_reference_plane_leaves_the_work_plane_where_it_was_and_undo_brings_the_link_back() {
    let mut s = with_planes();
    run(&mut s, "work_plane", json!({"name": "Ramp"}));
    let before = frame_of(&s.model);
    let id = ramp(&s);
    run(&mut s, "delete", json!({"ids": [id]}));
    let wp = s.model.work_plane(s.model.default_3d());
    assert_eq!(wp.ref_plane, None);
    assert!(near(frame_of(&s.model).origin, before.origin) && near(frame_of(&s.model).normal, before.normal));
    run(&mut s, "undo", json!({}));
    assert!(s.model.work_plane(s.model.default_3d()).ref_plane.is_some());
}

#[test]
fn a_ref_plane_work_plane_with_a_level_beside_it_is_refused_by_the_check() {
    let mut s = with_planes();
    run(&mut s, "work_plane", json!({"name": "Ramp"}));
    let mut m = (*s.model).clone();
    let level = m.guid(m.levels()[0].0);
    let v = m.default_3d().unwrap();
    if let Some(Element::View3D(v3)) = m.elements.get_mut(v) {
        v3.work_plane.as_mut().unwrap().level = Some(level);
    }
    let e = m.check().unwrap_err();
    assert!(e.contains("no `level` or `points` beside it"), "{e}");
}

#[test]
fn pick_a_line_takes_the_level_a_wall_or_a_beam_was_drawn_on_and_nothing_else() {
    let mut s = Session::new();
    let l2 = s.model.levels()[1].0;
    let wall = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "level": "Level 2"}))["ids"][0].as_u64().unwrap();
    let beam = run(&mut s, "beam", json!({"points": [[0, 1000], [4000, 1000]]}))["ids"][0].as_u64();
    assert_eq!(workplane::sketched_in(&s.model, wall).and_then(|w| w.level), Some(s.model.guid(l2)));
    let beam = beam.expect("a beam made");
    assert!(workplane::sketched_in(&s.model, beam).is_some_and(|w| w.level.is_some()));
    let floor = run(&mut s, "floor", json!({"boundary": [[0, 0], [1000, 0], [1000, 1000], [0, 1000]]}))["id"].as_u64().unwrap();
    assert_eq!(workplane::sketched_in(&s.model, floor), None, "a floor isn't drawn along a line");
}

#[test]
fn a_familys_planes_are_found_by_the_names_the_dialog_lists_and_edit_work_plane_takes_one() {
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({"name": "P", "types": {"A": {}},
               "ref_planes": [{"name": "Up", "start": [0, 0, 300], "end": [1000, 0, 300], "up": [0, 1, 0]}],
               "ref_lines": [{"start": [0, 0, 0], "end": [500, 0, 0]}],
               "forms": [{"profile": [[0, 0], [100, 0], [100, 50], [0, 50]], "from": 10, "to": 60}]}),
    );
    let f = &s.model.families["P"];
    let set = f.plane_set();
    use crate::family::{CENTER_LEFT_RIGHT, Face, PlaneRef, REF_LEVEL};
    assert_eq!(set.by_name("Level : Ref. Level"), Some(PlaneRef::Plane(REF_LEVEL)));
    assert_eq!(set.by_name("Center (Left/Right)"), Some(PlaneRef::Plane(CENTER_LEFT_RIGHT)), "a datum's own name");
    assert_eq!(set.by_name("Reference Line 1 : Vertical"), Some(PlaneRef::Line(f.lines[0].guid, Face::Vertical)));
    let up = f.planes.iter().find(|p| p.name == "Up").unwrap().guid;
    assert_eq!(set.by_name("Up"), Some(PlaneRef::Plane(up)));
    assert_eq!(set.by_name("Nowhere"), None);
    let form = json!(f.forms[0].guid);
    run(&mut s, "edit_work_plane", json!({"family": "P", "form": form, "host": "Reference Plane : Up"}));
    assert_eq!(s.model.families["P"].forms[0].host, PlaneRef::Plane(up));
    let e = fails(&mut s, "edit_work_plane", json!({"family": "P", "form": form, "host": "Nowhere"}));
    assert!(e.contains("no work plane named `Nowhere`") && e.contains("Reference Line 1 : Horizontal"), "{e}");
}
