//! Point clouds through the commands every front end drives: a scan comes in, is placed as an element, goes where the element
//! goes, and is saved with the project.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::cloud::MAX_CLOUDS;
use crate::engine::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} was taken: {v}"),
        Err(e) => e,
    }
}

/// A scan file in a folder of this test's own, so its name is its stem.
fn scan(test: &str, name: &str, text: &str) -> String {
    let dir = std::env::temp_dir().join(format!("bimcraft-cloud-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path: PathBuf = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

/// A scan of a box 4 m by 3 m by 2.5 m in survey coordinates, metres, its corners coloured.
const BOX: &str = "500000 7000000 10 255 0 0\n500004 7000000 10 0 255 0\n500004 7000003 10 0 0 255\n500000 7000003 12.5 255 255 255\n";

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.5)
}

/// The box round a placed cloud's points, in the model, mm.
fn bounds(s: &Session, id: u64) -> ([f64; 3], [f64; 3]) {
    s.model.cloud_box(id).unwrap_or_else(|| panic!("{id} is no placed cloud"))
}

fn one(s: &mut Session) -> Value {
    let all = run(s, "elements", json!({"category": "pointcloud"}));
    assert_eq!(all["count"], 1, "{all}");
    all["elements"][0].clone()
}

#[test]
fn a_scan_comes_in_with_its_middle_on_the_origin_and_its_foot_on_the_level() {
    let mut s = Session::new();
    let r = run(&mut s, "point_cloud", json!({"path": scan("centre", "box.xyz", BOX)}));
    assert_eq!((r["cloud"].as_str(), r["points"].as_u64(), r["read"].as_u64()), (Some("box"), Some(4), Some(4)), "{r}");
    // Where its middle was in the file, mm: what Origin to Origin would have kept.
    let origin: Vec<f64> = r["origin"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!(near([origin[0], origin[1], origin[2]], [500_002_000.0, 7_000_001_500.0, 11_250.0]), "{origin:?}");
    let id = r["id"].as_u64().unwrap();
    let e = one(&mut s);
    assert_eq!(
        (e["id"].as_u64(), e["cloud"].as_str(), &e["at"], &e["offset"], &e["angle"]),
        (Some(id), Some("box"), &json!([0.0, 0.0]), &json!(1250.0), &json!(0.0)),
        "{e}"
    );
    let (lo, hi) = bounds(&s, id);
    assert!(near(lo, [-2000.0, -1500.0, 0.0]) && near(hi, [2000.0, 1500.0, 2500.0]), "{lo:?} {hi:?}");
    // On another level, it stands on that one.
    let up = run(&mut s, "point_cloud", json!({"cloud": "box", "level": "Level 2"}))["id"].as_u64().unwrap();
    let level2 = run(&mut s, "elements", json!({"category": "level"}))["elements"][1]["elevation"].as_f64().unwrap();
    assert!(level2 > 0.0);
    assert!(near(bounds(&s, up).0, [-2000.0, -1500.0, level2]), "{:?}", bounds(&s, up));
}

#[test]
fn origin_to_origin_keeps_the_files_coordinates_where_a_model_can_hold_them() {
    let mut s = Session::new();
    let e = fails(&mut s, "point_cloud", json!({"path": scan("origin", "box.xyz", BOX), "origin": "file"}));
    assert!(e.contains("centre"), "{e}");
    let near_home = scan("origin", "near.xyz", "1 2 0\n3 4 1\n");
    let id = run(&mut s, "point_cloud", json!({"path": near_home, "origin": "file"}))["id"].as_u64().unwrap();
    let (lo, hi) = bounds(&s, id);
    assert!(near(lo, [1000.0, 2000.0, 0.0]) && near(hi, [3000.0, 4000.0, 1000.0]), "{lo:?} {hi:?}");
    let e = fails(&mut s, "point_cloud", json!({"cloud": "near", "origin": "nowhere"}));
    assert!(e.contains("centre") && e.contains("file"), "{e}");
    // Placed by hand: its middle where it's put, turned a quarter, so its 4 m run north.
    let id = run(&mut s, "point_cloud", json!({"cloud": "near", "at": [10_000, 0], "offset": 3000, "angle": 90}))["id"].as_u64().unwrap();
    let (lo, hi) = bounds(&s, id);
    assert!(near(lo, [9000.0, -1000.0, 2500.0]) && near(hi, [11_000.0, 1000.0, 3500.0]), "{lo:?} {hi:?}");
}

#[test]
fn units_spacing_and_files_it_cant_take() {
    let mut s = Session::new();
    let feet = scan("units", "feet.xyz", "0 0 0\n10 0 0\n");
    let id = run(&mut s, "point_cloud", json!({"path": feet, "units": "ft"}))["id"].as_u64().unwrap();
    let (lo, hi) = bounds(&s, id);
    assert!((hi[0] - lo[0] - 3048.0).abs() < 0.5, "{lo:?} {hi:?}");
    // A thousand points a metre apart, one kept in each 2 m cube.
    let grid: String = (0..1000).map(|i| format!("{} {} {}\n", i % 10, i / 10 % 10, i / 100)).collect();
    let r = run(&mut s, "point_cloud", json!({"path": scan("units", "grid.xyz", &grid), "spacing": 2000}));
    assert_eq!((r["points"].as_u64(), r["read"].as_u64()), (Some(125), Some(1000)), "{r}");
    for (p, says) in [
        (json!({"path": feet, "units": "furlong", "name": "f2"}), "mm, cm, m, in, ft"),
        (json!({"path": feet, "spacing": -1, "name": "f3"}), "spacing"),
        (json!({"path": scan("units", "plan.dwg", "x")}), "PTS, XYZ, PLY or LAS"),
        (json!({"path": scan("units", "scan.laz", "x")}), "LAZ"),
        (json!({"path": scan("units", "gone", "x").replace("gone", "not-there.xyz")}), "not-there.xyz"),
        (json!({"path": scan("units", "bad.xyz", "0 0 zero\n")}), "line 1"),
        (json!({}), "path"),
        (json!({"cloud": "nope"}), "nope"),
    ] {
        let e = fails(&mut s, "point_cloud", p.clone());
        assert!(e.contains(says), "{p}: {e}");
    }
}

#[test]
fn a_scan_is_brought_in_once_and_placed_as_often_as_wanted() {
    let mut s = Session::new();
    let path = scan("again", "box.xyz", BOX);
    run(&mut s, "point_cloud", json!({"path": path}));
    run(&mut s, "point_cloud", json!({"cloud": "box", "at": [10_000, 0]}));
    assert_eq!((s.model.clouds.len(), run(&mut s, "elements", json!({"category": "pointcloud"}))["count"].as_u64()), (1, Some(2)));
    // The same name again is a mistake that says what to do instead.
    let e = fails(&mut s, "point_cloud", json!({"path": path}));
    assert!(e.contains("already") && e.contains("`cloud`") && e.contains("`name`"), "{e}");
    run(&mut s, "point_cloud", json!({"path": path, "name": "box 2"}));
    assert_eq!(s.model.clouds.len(), 2);
    // A project holds so many scans.
    for i in 2..MAX_CLOUDS {
        run(&mut s, "point_cloud", json!({"path": path, "name": format!("box {}", i + 1)}));
    }
    let e = fails(&mut s, "point_cloud", json!({"path": path, "name": "one too many"}));
    assert!(e.contains(&MAX_CLOUDS.to_string()), "{e}");
}

#[test]
fn a_scan_moves_turns_and_copies_with_its_element_but_doesnt_mirror() {
    let mut s = Session::new();
    let id = run(&mut s, "point_cloud", json!({"path": scan("edit", "box.xyz", BOX)}))["id"].as_u64().unwrap();
    run(&mut s, "move", json!({"ids": [id], "by": [1000, 500]}));
    assert_eq!(one(&mut s)["at"], json!([1000.0, 500.0]));
    run(&mut s, "rotate", json!({"ids": [id], "about": [0, 0], "angle": 90}));
    let e = one(&mut s);
    assert!((e["at"][0].as_f64().unwrap() + 500.0).abs() < 1e-6 && (e["at"][1].as_f64().unwrap() - 1000.0).abs() < 1e-6, "{e}");
    assert_eq!(e["angle"], json!(90.0));
    let copies = run(&mut s, "copy", json!({"ids": [id], "by": [0, 10_000]}));
    assert_eq!(run(&mut s, "elements", json!({"category": "pointcloud"}))["count"], 2, "{copies}");
    assert_eq!(s.model.clouds.len(), 1);
    let e = fails(&mut s, "mirror", json!({"ids": [id], "axis": [[0, 0], [0, 1000]]}));
    assert!(e.contains("mirror") && e.contains("scan"), "{e}");
    let e = fails(&mut s, "modify", json!({"id": id, "cloud": "nope"}));
    assert!(e.contains("nope"), "{e}");
    let e = fails(&mut s, "modify", json!({"id": id, "at": [2e9, 0]}));
    assert!(e.contains("at"), "{e}");
}

#[test]
fn a_scan_goes_when_nothing_places_it_and_undo_brings_it_back() {
    let mut s = Session::new();
    let id = run(&mut s, "point_cloud", json!({"path": scan("delete", "box.xyz", BOX)}))["id"].as_u64().unwrap();
    let copy = run(&mut s, "point_cloud", json!({"cloud": "box", "at": [10_000, 0]}))["id"].as_u64().unwrap();
    run(&mut s, "delete", json!({"ids": [id]}));
    assert_eq!(s.model.clouds.len(), 1, "one still places it");
    run(&mut s, "delete", json!({"ids": [copy]}));
    assert!(s.model.clouds.is_empty());
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.clouds.len(), 1);
}

#[test]
fn a_scan_is_saved_with_the_project_and_opens_as_it_was() {
    let mut s = Session::new();
    let r = run(&mut s, "point_cloud", json!({"path": scan("save", "box.xyz", BOX), "angle": 30, "at": [100, 200]}));
    let before = (one(&mut s), s.model.clouds.clone());
    let file = std::env::temp_dir().join(format!("bimcraft-cloud-{}-save.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": file}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": file}));
    assert_eq!((one(&mut t), t.model.clouds.clone()), before, "{r}");
    let cloud = t.model.clouds.get("box").unwrap();
    assert_eq!((cloud.len(), cloud.colours().is_some()), (4, true));
}

#[test]
fn a_project_with_a_scan_still_exports_ifc() {
    let mut s = Session::new();
    run(&mut s, "point_cloud", json!({"path": scan("ifc", "box.xyz", BOX)}));
    let file = std::env::temp_dir().join(format!("bimcraft-cloud-{}-scan.ifc", std::process::id()));
    run(&mut s, "save", json!({"path": file}));
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.starts_with("ISO-10303-21;") && text.contains("IFCPROJECT"));
}

#[test]
fn insert_from_file_brings_a_scan_with_the_point_cloud_that_places_it() {
    let mut from = Session::new();
    let id = run(&mut from, "point_cloud", json!({"path": scan("insert", "box.xyz", BOX), "at": [500, 0]}))["id"].as_u64().unwrap();
    let file = std::env::temp_dir().join(format!("bimcraft-cloud-{}-insert.bimcraft", std::process::id()));
    run(&mut from, "save", json!({"path": file}));
    let mut s = Session::new();
    run(&mut s, "insert", json!({"path": file, "ids": [id]}));
    let e = one(&mut s);
    assert_eq!((e["cloud"].as_str(), &e["at"]), (Some("box"), &json!([500.0, 0.0])), "{e}");
    assert_eq!(s.model.clouds.get("box"), from.model.clouds.get("box"));
}
