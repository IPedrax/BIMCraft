//! A wall's Top Offset (`top_offset`, file format 15): its top is its top constraint's level plus the offset, wherever the top
//! is read, through the commands every front end drives.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Wall};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

fn wall_of(s: &Session, id: u64) -> &Wall {
    let Some(Element::Wall(w)) = s.model.elements.get(id) else { panic!("no wall {id}") };
    w
}

/// A wall 4 m long on Level 1 at `y`, up to Level 2 (3000 mm) and `offset` mm more, the offset left out when it is `None`.
fn wall(s: &mut Session, y: f64, offset: Option<f64>) -> u64 {
    let mut p = json!({"points": [[0, y], [4000, y]], "top_level": "Level 2"});
    if let Some(offset) = offset {
        p["top_offset"] = json!(offset);
    }
    run(s, "wall", p)["ids"][0].as_u64().unwrap()
}

fn height(s: &mut Session, id: u64) -> f64 {
    let all = run(s, "elements", json!({"category": "walls"}));
    all["elements"].as_array().unwrap().iter().find(|e| e["id"] == id).unwrap()["height"].as_f64().unwrap()
}

/// How high wall `id`'s top is over (`x`, `y`), mm above Level 1.
fn top(s: &Session, id: u64, x: f64, y: f64) -> f64 {
    crate::view3d::tops_at(&s.model, P2(x, y)).get(&id).copied().unwrap_or(f64::NAN)
}

#[test]
fn a_wall_to_level_2_plus_300_is_300_taller_than_one_without_and_one_minus_300_is_shorter() {
    let mut s = Session::new();
    let (plain, higher, lower) = (wall(&mut s, 0.0, None), wall(&mut s, 1000.0, Some(300.0)), wall(&mut s, 2000.0, Some(-300.0)));
    assert_eq!([plain, higher, lower].map(|id| height(&mut s, id)), [3000.0, 3300.0, 2700.0]);
    // Its top, as the 3D view and the solid it is built from have it.
    assert_eq!([(plain, 0.0), (higher, 1000.0), (lower, 2000.0)].map(|(id, y)| top(&s, id, 2000.0, y)), [3000.0, 3300.0, 2700.0]);
    assert_eq!([plain, higher, lower].map(|id| s.model.top_z(wall_of(&s, id))), [3000.0, 3300.0, 2700.0]);
    // It is a field of the wall, and `modify` changes it, as it changes the rest.
    run(&mut s, "modify", json!({"id": plain, "top_offset": 150}));
    assert_eq!((wall_of(&s, plain).top_offset, height(&mut s, plain)), (150.0, 3150.0));
    run(&mut s, "modify", json!({"ids": [plain, higher], "top_offset": -500}));
    assert_eq!([plain, higher].map(|id| height(&mut s, id)), [2500.0, 2500.0]);
    run(&mut s, "undo", json!({}));
    assert_eq!([plain, higher].map(|id| height(&mut s, id)), [3150.0, 3300.0]);
    // The top constraint moves with its level, the offset staying.
    run(&mut s, "modify", json!({"id": 100_001, "elevation": 3500}));
    assert_eq!(height(&mut s, higher), 3800.0);
}

#[test]
fn the_offset_belongs_to_the_top_constraint_so_an_unconnected_wall_ignores_it() {
    let mut s = Session::new();
    let id = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "height": 2700, "top_offset": 500}))["ids"][0].as_u64().unwrap();
    assert_eq!(height(&mut s, id), 2700.0, "it stands `height` tall, whatever the offset");
    // Connected, the offset counts; unconnected again, it does not.
    run(&mut s, "modify", json!({"id": id, "top_level": "Level 2"}));
    assert_eq!(height(&mut s, id), 3500.0);
    run(&mut s, "modify", json!({"id": id, "top_level": null}));
    assert_eq!(height(&mut s, id), 2700.0);
    // A wall made to the next level by default has none.
    let plain = run(&mut s, "wall", json!({"points": [[0, 1000], [4000, 1000]]}))["ids"][0].as_u64().unwrap();
    assert_eq!(wall_of(&s, plain).top_offset, 0.0);
}

#[test]
fn a_wall_whose_height_comes_out_zero_or_less_is_refused_saying_by_how_much() {
    let mut s = Session::new();
    // Level 2's whole height taken off: nothing left.
    let e = fails(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "top_level": "Level 2", "top_offset": -3000}));
    assert!(e.contains("top offset") && e.contains("0 mm tall"), "{e}");
    // 500 mm further: it comes out 500 mm under its base, and the error says to raise it by more than that.
    let e = fails(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "top_level": "Level 2", "top_offset": -3500}));
    assert!(e.contains("-500 mm tall") && e.contains("more than 500 mm"), "{e}");
    // A base offset can take it there too, the same way.
    let id = wall(&mut s, 0.0, Some(-1000.0));
    let e = fails(&mut s, "modify", json!({"id": id, "base_offset": 2000}));
    assert!(e.contains("it comes out 0 mm tall") && e.contains("base"), "{e}");
    let e = fails(&mut s, "modify", json!({"id": id, "top_offset": -3000}));
    assert!(e.contains("0 mm tall") && e.contains("top offset"), "{e}");
    assert_eq!(wall_of(&s, id).top_offset, -1000.0);
    // Past what a coordinate can be, or not a number, is refused as other lengths are.
    let e = fails(&mut s, "modify", json!({"id": id, "top_offset": 1e300}));
    assert!(e.contains("top offset"), "{e}");
    let e = fails(&mut s, "modify", json!({"id": id, "top_offset": "high"}));
    assert!(e.contains("invalid type"), "{e}");
    let e = fails(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "top_offset": "high"}));
    assert!(e.contains("top_offset"), "{e}");
}

#[test]
fn a_walls_ifc_solid_is_as_tall_as_its_offset_top_and_the_offset_is_saved_only_when_there_is_one() {
    let mut s = Session::new();
    let id = wall(&mut s, 0.0, Some(300.0));
    let plain = wall(&mut s, 2000.0, None);
    let path = std::env::temp_dir().join(format!("bimcraft-top-offset-{}.ifc", std::process::id()));
    run(&mut s, "save", json!({"path": path}));
    let ifc = crate::bimc::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let solids: Vec<&str> = ifc.lines().filter(|l| l.contains("IFCEXTRUDEDAREASOLID(")).collect();
    assert_eq!(solids.iter().filter(|l| l.ends_with(",3300.);")).count(), 1, "{solids:?}");
    assert_eq!(solids.iter().filter(|l| l.ends_with(",3000.);")).count(), 1, "{solids:?}");
    // In the project file: the offset where there is one, the wall to Level 2 as before where there is not.
    let file: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(file["format"], crate::model::FORMAT);
    let saved = |id: u64| {
        let guid = s.model.guid(id).to_string();
        file["elements"].as_object().unwrap().values().find(|e| e["guid"] == json!(guid)).unwrap().clone()
    };
    assert_eq!(saved(id)["top_offset"], json!(300.0));
    assert!(saved(plain).get("top_offset").is_none(), "{}", saved(plain));
    // Read as it was written, nothing left out; a file of 14 has no offset, and its walls stand where they did.
    let (m, left_out) = crate::model::Model::read(&file.to_string()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.elements.get(id), s.model.elements.get(id));
    let mut old = file.clone();
    old["format"] = json!(14);
    let key =
        old["elements"].as_object().unwrap().iter().find(|(_, e)| e["guid"] == json!(s.model.guid(id).to_string())).unwrap().0.clone();
    old["elements"][&key].as_object_mut().unwrap().remove("top_offset");
    let (m, left_out) = crate::model::Model::read(&old.to_string()).unwrap();
    assert!(left_out.is_empty() && m.format == crate::model::FORMAT, "{left_out:?}");
    let Some(Element::Wall(w)) = m.elements.get(id) else { panic!() };
    assert_eq!((w.top_offset, m.top_z(w)), (0.0, 3000.0));
}

#[test]
fn a_wall_attached_to_a_roof_follows_the_roof_and_goes_back_to_its_offset_when_detached() {
    let mut s = Session::new();
    let id = wall(&mut s, 3000.0, Some(300.0));
    let roof = run(
        &mut s,
        "roof",
        json!({"boundary": [[-1000, 2000], [5000, 2000], [5000, 4000], [-1000, 4000]], "slope": 0, "offset": 600}),
    )["id"]
        .as_u64()
        .unwrap();
    run(&mut s, "modify", json!({"id": id, "attach_top": roof}));
    let attached = s.model.top_z(wall_of(&s, id));
    assert!((attached - 3000.0).abs() > 1.0 && (attached - 3300.0).abs() > 1.0, "the roof sets it: {attached}");
    run(&mut s, "modify", json!({"id": id, "attach_top": null}));
    assert_eq!(s.model.top_z(wall_of(&s, id)), 3300.0);
}
