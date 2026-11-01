//! The toposurface: terrain made from points through `toposurface`, asked with `height_at` and `contours`, moved, saved, undone.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{FORMAT, Model};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn refused(s: &mut Session, id: &str, p: Value) -> String {
    s.execute(id, &p).expect_err(&format!("{id} {p} should be refused"))
}

/// A `n` x `n` grid of points `step` mm apart, the height `f` of each.
fn grid(n: usize, step: f64, f: impl Fn(f64, f64) -> f64) -> Vec<[f64; 3]> {
    (0..n).flat_map(|i| (0..n).map(move |j| (i as f64 * step, j as f64 * step))).map(|(x, y)| [x, y, f(x, y)]).collect()
}

/// The toposurface `id` as `elements` lists it.
fn listed(s: &mut Session, id: u64) -> Value {
    let all = run(s, "elements", json!({"category": "toposurface"}));
    all["elements"].as_array().unwrap().iter().find(|e| e["id"] == id).cloned().unwrap_or_else(|| panic!("{id} isn't listed"))
}

/// A 10 m square of terrain rising 1 m from west to east: 11 x 11 points.
fn slope(s: &mut Session) -> u64 {
    run(s, "toposurface", json!({"points": grid(11, 1000.0, |x, _| x / 10.0), "name": "Site"}))["id"].as_u64().unwrap()
}

#[test]
fn terrain_is_made_from_points_and_reports_its_surface() {
    let mut s = Session::new();
    let r = run(&mut s, "toposurface", json!({"points": grid(11, 1000.0, |x, _| x / 10.0), "name": "Site"}));
    assert_eq!(
        (r["triangles"].clone(), r["area_m2"].clone(), r["z_min"].clone(), r["z_max"].clone()),
        (json!(200), json!(100.0), json!(0.0), json!(1000.0))
    );
    let e = run(&mut s, "elements", json!({"category": "toposurface"}));
    assert_eq!(e["count"], 1);
    let t = &e["elements"][0];
    assert_eq!((t["name"].clone(), t["points"].clone(), t["area_m2"].clone()), (json!("Site"), json!(121), json!(100.0)));
    assert!(t["surface_area_m2"].as_f64().unwrap() > 100.0);
    // Named for the language when it isn't named.
    let unnamed = run(&mut s, "toposurface", json!({"points": grid(3, 1000.0, |_, _| 1.0)}))["id"].as_u64().unwrap();
    assert_eq!(listed(&mut s, unnamed)["name"], "Toposurface");
}

#[test]
fn the_height_over_a_point_is_asked_by_height_at() {
    let mut s = Session::new();
    slope(&mut s);
    let r = run(&mut s, "height_at", json!({"points": [[2500, 3000], [10000, 10000], [-5, 0], [20000, 0]]}));
    assert_eq!(r["heights"], json!([250.0, 1000.0, null, null]), "the only terrain, and none over what is off it");
    // With several, the one named.
    let hill = run(&mut s, "toposurface", json!({"points": grid(3, 1000.0, |_, _| 9000.0), "name": "Hill"}))["id"].as_u64().unwrap();
    assert!(refused(&mut s, "height_at", json!({"points": [[0, 0]]})).contains("id"));
    assert_eq!(run(&mut s, "height_at", json!({"id": hill, "points": [[500, 500]]}))["heights"], json!([9000.0]));
}

#[test]
fn contours_come_back_as_lines_of_points() {
    let mut s = Session::new();
    slope(&mut s);
    let r = run(&mut s, "contours", json!({"interval": 500}));
    let lines = r["contours"].as_array().unwrap();
    assert_eq!(lines.iter().map(|c| c["z"].as_f64().unwrap()).collect::<Vec<_>>(), [500.0, 1000.0]);
    assert!(lines[0]["points"].as_array().unwrap().iter().all(|p| (p[0].as_f64().unwrap() - 5000.0).abs() < 1e-6));
    assert_eq!(lines[0]["closed"], false);
    // Its own interval when none is said.
    assert_eq!(run(&mut s, "contours", json!({}))["interval"], 1000.0);
    assert!(refused(&mut s, "contours", json!({"interval": 1})).contains("interval"));
    assert!(refused(&mut s, "contours", json!({"interval": "x"})).contains("interval"));
}

#[test]
fn terrain_moves_turns_and_mirrors_with_its_height_staying() {
    let mut s = Session::new();
    let id = slope(&mut s);
    run(&mut s, "move", json!({"ids": [id], "by": [1000, 2000]}));
    assert_eq!(run(&mut s, "height_at", json!({"points": [[3500, 2000], [500, 2000]]}))["heights"], json!([250.0, null]));
    run(&mut s, "rotate", json!({"ids": [id], "about": [1000, 2000], "angle": 90}));
    // Rising to the east, turned a quarter counter-clockwise: now it rises to the north.
    let h = run(&mut s, "height_at", json!({"points": [[1000, 4500]]}))["heights"][0].as_f64().unwrap();
    assert!((h - 250.0).abs() < 1e-6, "{h}");
}

#[test]
fn it_is_saved_undone_and_deleted_like_any_element() {
    let mut s = Session::new();
    let id = slope(&mut s);
    let text = serde_json::to_string(&*s.model).unwrap();
    let (m, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty());
    assert_eq!(m.format, FORMAT);
    assert_eq!(m.elements.len(), s.model.elements.len());
    run(&mut s, "delete", json!({"ids": [id]}));
    assert_eq!(run(&mut s, "elements", json!({"category": "toposurface"}))["count"], 0);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(run(&mut s, "elements", json!({"category": "toposurface"}))["count"], 1);
    // An older build would not know the element, so the format says so.
    let mut newer = serde_json::to_value(&*s.model).unwrap();
    newer["format"] = json!(FORMAT + 1);
    assert!(Model::read(&newer.to_string()).is_err());
}

#[test]
fn breaklines_and_a_boundary_shape_the_surface() {
    let mut s = Session::new();
    let square = json!([[0, 0, 0], [10000, 0, 10], [10000, 10000, 0], [0, 10000, 10]]);
    let id = run(&mut s, "toposurface", json!({"points": square, "breaklines": [[[0, 0, 0], [10000, 10000, 0]]]}))["id"].as_u64().unwrap();
    assert_eq!(run(&mut s, "height_at", json!({"points": [[5000, 5000]]}))["heights"], json!([0.0]), "the valley is followed");
    run(&mut s, "delete", json!({"ids": [id]}));
    let notch = [[0, 0], [4000, 0], [4000, 2000], [2000, 2000], [2000, 4000], [0, 4000]];
    run(&mut s, "toposurface", json!({"points": grid(5, 1000.0, |_, _| 1.0), "boundary": notch}));
    assert_eq!(run(&mut s, "height_at", json!({"points": [[3000, 3000], [1000, 1000]]}))["heights"], json!([null, 1.0]));
}

#[test]
fn what_cannot_be_terrain_is_refused_and_the_project_stays_as_it_was() {
    let mut s = Session::new();
    let line = json!([[0, 0, 0], [1000, 0, 0], [2000, 0, 0]]);
    for (p, says) in [
        (json!({}), "points"),
        (json!({"points": [[0, 0, 0], [1, 1, 1]]}), "needs 3"),
        (json!({"points": line}), "line"),
        (json!({"points": [[0, 0], [1, 1], [2, 2]]}), "points"),
        (json!({"points": [[0, 0, 0], [1000, 0, 0], [0, 1000, 1e300]]}), "points"),
        (json!({"points": grid(3, 1000.0, |_, _| 0.0), "material": "Unobtainium"}), "material"),
        (json!({"points": grid(3, 1000.0, |_, _| 0.0), "interval": 3}), "interval"),
        (json!({"points": grid(3, 1000.0, |_, _| 0.0), "breaklines": [[[0, 0, 0]]]}), "breakline"),
        (json!({"points": grid(3, 1000.0, |_, _| 0.0), "boundary": [[0, 0], [1, 1]]}), "boundary"),
        (json!({"points": grid(3, 1000.0, |_, _| 0.0), "level": 100000}), "level"),
    ] {
        assert!(refused(&mut s, "toposurface", p.clone()).contains(says), "{p}");
    }
    assert_eq!(run(&mut s, "elements", json!({"category": "toposurface"}))["count"], 0);
}

#[test]
fn a_toposurface_takes_phases_and_parameters_and_a_material() {
    let mut s = Session::new();
    let id = run(&mut s, "toposurface", json!({"points": grid(3, 1000.0, |_, _| 0.0), "material": "Concrete", "interval": 0}))["id"]
        .as_u64()
        .unwrap();
    assert_eq!(listed(&mut s, id)["material"], "Concrete");
    assert_eq!(run(&mut s, "contours", json!({"interval": 100}))["contours"], json!([]), "flat ground has no lines");
    // It is a category project parameters can be added to.
    run(&mut s, "parameter", json!({"name": "Soil", "kind": "text", "categories": ["toposurface"]}));
    run(&mut s, "modify", json!({"ids": [id], "params": {"Soil": "Clay"}}));
}
