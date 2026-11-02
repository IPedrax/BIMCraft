//! Shaped floors (IDEAS MOD-16, [design](../../../docs/shape-editing.md)): points and split lines on a floor's top, through
//! the commands every front end drives.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

fn element(s: &mut Session, id: u64) -> Value {
    let all = run(s, "elements", json!({}));
    all["elements"].as_array().unwrap().iter().find(|e| e["id"] == id).cloned().unwrap()
}

/// An 8 by 6 m floor on Level 1, its top at the level.
fn slab(s: &mut Session) -> u64 {
    run(s, "floor", json!({"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}))["id"].as_u64().unwrap()
}

/// How high floor `id`'s top is over (`x`, `y`), mm above Level 1.
fn top(s: &Session, id: u64, x: f64, y: f64) -> f64 {
    crate::view3d::tops_at(&s.model, P2(x, y)).get(&id).copied().unwrap_or(f64::NAN)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// The shape as `edit_shape` gives it, asked with nothing.
fn shape(s: &mut Session, id: u64) -> Value {
    run(s, "edit_shape", json!({"id": id}))
}

#[test]
fn a_drain_folds_a_floor_into_four_planes_falling_to_it() {
    let mut s = Session::new();
    let f = slab(&mut s);
    let r = run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}]}));
    assert_eq!(r["faces"], 4, "{r}");
    assert!(near(top(&s, f, 4000.0, 3000.0), -30.0));
    assert!(near(top(&s, f, 2000.0, 1500.0), -15.0));
    // The west plane runs from the west side, at 0, down to the drain.
    assert!(near(top(&s, f, 100.0, 3000.0), -0.75), "{}", top(&s, f, 100.0, 3000.0));
    assert!(near(top(&s, f, 0.0, 0.0), 0.0));
    // The point is a part of the floor, with an id of its own; asked with nothing, the command says what the top is.
    let e = element(&mut s, f);
    assert!(e["shape"]["points"][0]["id"].as_u64().is_some_and(|id| id != f), "{e}");
    assert_eq!(e["shape"]["points"][0]["z"], -30.0);
    let now = shape(&mut s, f);
    assert_eq!(
        (now["points"].as_array().map(Vec::len), now["faces"].clone(), now["top"].clone()),
        (Some(1), json!(4), json!([-30.0, 0.0]))
    );
    assert_eq!(now["corners"].as_array().map(Vec::len), Some(4), "the outline's corners, each at its height: {now}");
}

#[test]
fn a_split_line_is_a_crease_the_top_folds_along() {
    let mut s = Session::new();
    let f = slab(&mut s);
    // A valley across the middle, 20 mm down at both sides: two planes, each over half the floor.
    let r = run(
        &mut s,
        "edit_shape",
        json!({"id": f, "points": [{"at": [0, 3000], "z": -20}, {"at": [8000, 3000], "z": -20}], "lines": [{"a": [0, 3000], "b": [8000, 3000]}]}),
    );
    assert_eq!(r["faces"], 2, "{r}");
    assert!(near(top(&s, f, 4000.0, 3000.0), -20.0));
    assert!(near(top(&s, f, 4000.0, 1500.0), -10.0) && near(top(&s, f, 4000.0, 4500.0), -10.0));
    // A line whose ends are no points: each becomes one, at the height the top has there, so nothing moves.
    run(&mut s, "add_split_line", json!({"id": f, "a": [4000, 3000], "b": [4000, 6000]}));
    let now = shape(&mut s, f);
    assert_eq!((now["points"].as_array().map(Vec::len), now["lines"].as_array().map(Vec::len)), (Some(4), Some(2)), "{now}");
    assert!(near(top(&s, f, 4000.0, 4500.0), -10.0) && near(top(&s, f, 2000.0, 4500.0), -10.0));
    // Sent back with their ids, the points and lines keep them.
    let mut again = now.clone();
    again["points"][0]["z"] = json!(-25);
    let kept = run(&mut s, "edit_shape", json!({"id": f, "points": again["points"], "lines": again["lines"]}));
    assert_eq!(kept["faces"].as_u64().map(|n| n > 2), Some(true));
    let after = shape(&mut s, f);
    assert_eq!(after["points"][0]["id"], now["points"][0]["id"]);
    assert_eq!(after["lines"][1]["id"], now["lines"][1]["id"]);
}

#[test]
fn add_point_sets_a_height_and_reset_shape_takes_them_all_off() {
    let mut s = Session::new();
    let f = slab(&mut s);
    run(&mut s, "add_point", json!({"id": f, "at": [4000, 3000], "z": -30}));
    assert!(near(top(&s, f, 4000.0, 3000.0), -30.0));
    // Within a millimetre of a point: that point, set again, not a second one.
    run(&mut s, "add_point", json!({"id": f, "at": [4000.5, 3000], "z": -40}));
    assert_eq!(shape(&mut s, f)["points"].as_array().map(Vec::len), Some(1));
    assert!(near(top(&s, f, 4000.0, 3000.0), -40.0));
    // With no height: the one the top has there, so nothing moves.
    run(&mut s, "add_point", json!({"id": f, "at": [2000, 1500]}));
    assert!(near(top(&s, f, 2000.0, 1500.0), -20.0) && near(top(&s, f, 4000.0, 3000.0), -40.0));
    // The shape rides on the floor's offset; `absolute` takes an elevation.
    run(&mut s, "modify", json!({"id": f, "offset": 100}));
    assert!(near(top(&s, f, 4000.0, 3000.0), 60.0));
    run(&mut s, "add_point", json!({"id": f, "at": [6000, 1500], "z": 90, "absolute": true}));
    assert!(near(top(&s, f, 6000.0, 1500.0), 90.0));
    run(&mut s, "reset_shape", json!({"id": f}));
    assert!(near(top(&s, f, 4000.0, 3000.0), 100.0) && near(top(&s, f, 6000.0, 1500.0), 100.0));
    assert!(element(&mut s, f).get("shape").is_none());
}

#[test]
fn what_a_shape_refuses_it_says() {
    let mut s = Session::new();
    let f = run(
        &mut s,
        "floor",
        json!({"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "openings": [[[3000, 2000], [5000, 2000], [5000, 4000], [3000, 4000]]]}),
    )["id"]
        .as_u64()
        .unwrap();
    let says = |e: String, what: &str| assert!(e.contains(what), "`{e}` doesn't say `{what}`");
    says(fails(&mut s, "add_point", json!({"id": f, "at": [9000, 3000], "z": -10})), "outside");
    says(fails(&mut s, "add_point", json!({"id": f, "at": [4000, 3000], "z": -10})), "opening");
    says(fails(&mut s, "add_point", json!({"id": f, "at": [1000, 1000], "z": 1e12})), "z");
    says(fails(&mut s, "add_split_line", json!({"id": f, "a": [0, 0], "b": [8000, 6000]})), "crosses");
    let crossing = json!([{"a": [0, 0], "b": [2500, 6000]}, {"a": [0, 6000], "b": [2500, 0]}]);
    says(fails(&mut s, "edit_shape", json!({"id": f, "lines": crossing})), "crosses");
    let twins = json!([{"at": [1000, 1000], "z": -1}, {"at": [1000.5, 1000], "z": -2}]);
    says(fails(&mut s, "edit_shape", json!({"id": f, "points": twins})), "within a millimetre");
    says(fails(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [1000, 1000]}]})), "z");
    // Shaped, a floor takes its fall from its points: not a slope arrow too.
    run(&mut s, "add_point", json!({"id": f, "at": [1000, 1000], "z": -10}));
    says(fails(&mut s, "modify", json!({"id": f, "slope": 2, "arrow": [[0, 0], [8000, 0]]})), "shape");
    // Only floors have shapes.
    let wall = run(&mut s, "wall", json!({"points": [[0, 8000], [8000, 8000]]}))["ids"][0].as_u64().unwrap();
    says(fails(&mut s, "add_point", json!({"id": wall, "at": [1000, 8000], "z": 0})), "floor");
    says(fails(&mut s, "edit_shape", json!({"id": wall})), "floor");
}

#[test]
fn a_shaped_floor_moves_mirrors_and_copies_with_its_points() {
    let mut s = Session::new();
    let f = slab(&mut s);
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}], "lines": [{"a": [0, 0], "b": [4000, 3000]}]}));
    run(&mut s, "move", json!({"ids": [f], "by": [1000, 0]}));
    assert!(near(top(&s, f, 5000.0, 3000.0), -30.0));
    run(&mut s, "mirror", json!({"ids": [f], "axis": [[0, 0], [0, 1000]]}));
    assert!(near(top(&s, f, -5000.0, 3000.0), -30.0) && near(top(&s, f, -3000.0, 1500.0), -15.0));
    let copy = run(&mut s, "copy", json!({"ids": [f], "by": [0, 10000]}))["ids"][0].as_u64().unwrap();
    assert!(near(top(&s, copy, -5000.0, 13000.0), -30.0));
    let (mine, its) = (shape(&mut s, f), shape(&mut s, copy));
    assert_ne!(mine["points"][0]["id"], its["points"][0]["id"], "a copy's points are its own");
    assert_eq!(its["lines"].as_array().map(Vec::len), Some(1));
}

#[test]
fn a_shape_is_kept_in_the_file_and_goes_to_ifc_as_its_planes() {
    let mut s = Session::new();
    let f = slab(&mut s);
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}]}));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shaped.bimcraft");
    run(&mut s, "save", json!({"path": path}));
    let file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(file["format"], 12);
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    assert_eq!(shape(&mut t, f), shape(&mut s, f));
    assert!(near(top(&t, f, 2000.0, 1500.0), -15.0));
    // In IFC, its slab is shells round its four planes, not a profile swept up.
    let ifc = dir.path().join("shaped.ifc");
    run(&mut s, "save", json!({"path": ifc}));
    let text = std::fs::read_to_string(&ifc).unwrap();
    assert!(text.contains("'Body','Brep'") && !text.contains("SweptSolid"), "the slab isn't its planes");
}
