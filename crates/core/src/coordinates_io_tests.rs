//! Far coordinates at import and export: drawings, scans and IFC files at map coordinates come in near the internal origin,
//! by the active location's transform, and IFC goes out with its map conversion
//! ([coordinates](../../../docs/areas/coordinates.md)).

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::engine::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

/// A file in a folder of this test's own.
fn file(test: &str, name: &str, text: &str) -> String {
    let dir = std::env::temp_dir().join(format!("bimcraft-coords-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    let path: PathBuf = dir.join(name);
    std::fs::write(&path, text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    path.to_string_lossy().into_owned()
}

fn pairs(list: &[(i32, &str)]) -> String {
    list.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect()
}

/// A DXF in metres: a room 10 m by 6 m whose south-west corner is at map coordinates (`e`, `n`).
fn site_plan(e: f64, n: f64) -> String {
    let corners = [(0.0, 0.0), (10.0, 0.0), (10.0, 6.0), (0.0, 6.0), (0.0, 0.0)];
    let lines: String = corners
        .windows(2)
        .map(|w| {
            let (a, b) = ((e + w[0].0, n + w[0].1), (e + w[1].0, n + w[1].1));
            pairs(&[(0, "LINE"), (8, "0"), (10, &a.0.to_string()), (20, &a.1.to_string()), (11, &b.0.to_string()), (21, &b.1.to_string())])
        })
        .collect();
    let header = pairs(&[(9, "$INSUNITS"), (70, "6")]);
    format!("0\nSECTION\n2\nHEADER\n{header}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{lines}0\nENDSEC\n0\nEOF\n")
}

fn near2(v: &Value, want: [[f64; 2]; 2], tol: f64) -> bool {
    (0..2).all(|i| (0..2).all(|k| v[i][k].as_f64().is_some_and(|x| (x - want[i][k]).abs() < tol)))
}

fn near3(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < tol)
}

fn to_shared(s: &Session, p: [f64; 3]) -> [f64; 3] {
    s.model.coordinates.to_shared(p)
}

// ---------- CAD ----------

#[test]
fn a_drawing_at_map_coordinates_comes_in_by_shared_coordinates() {
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Internal", "east": 500_000_000.0, "north": 7_000_000_000.0}));
    let path = file("cad-shared", "site.dxf", &site_plan(500_000.0, 7_000_000.0));
    let r = run(&mut s, "import_cad", json!({"path": path, "position": "shared"}));
    assert!(near2(&r["extents"], [[0.0, 0.0], [10_000.0, 6000.0]], 1e-3), "{r}");
    // True north a quarter east of project north: up the plan is west on the map, so the map's east runs down it.
    run(&mut s, "location", json!({"name": "Internal", "angle": 90}));
    let r = run(&mut s, "import_cad", json!({"cad": "site", "position": "shared"}));
    assert!(near2(&r["extents"], [[0.0, -10_000.0], [6000.0, 0.0]], 1e-3), "{r}");
}

#[test]
fn a_far_drawing_asks_to_acquire_its_coordinates_and_then_lands_near_the_origin() {
    let mut s = Session::new();
    let path = file("cad-acquire", "site.dxf", &site_plan(500_000.0, 7_000_000.0));
    let e = fails(&mut s, "import_cad", json!({"path": path, "position": "shared"}));
    assert!(e.contains("acquire"), "{e}");
    let r = run(&mut s, "import_cad", json!({"path": path, "position": "shared", "acquire": true}));
    // Its middle, to the metre, is the internal origin's map coordinates now.
    let l = &s.model.coordinates.locations["Internal"];
    assert_eq!((l.east, l.north), (500_005_000.0, 7_000_003_000.0));
    assert!(near2(&r["extents"], [[-5000.0, -3000.0], [5000.0, 3000.0]], 1e-3), "{r}");
    assert_eq!(r["acquired"]["east"], 500_005_000.0, "{r}");
}

// ---------- point clouds ----------

/// A scan of a box 4 m by 3 m by 2.5 m at survey coordinates, metres.
const BOX: &str = "500000 7000000 10 255 0 0\n500004 7000000 10 0 255 0\n500004 7000003 10 0 0 255\n500000 7000003 12.5 255 255 255\n";

#[test]
fn a_scan_at_map_coordinates_comes_in_by_shared_coordinates() {
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Internal", "east": 500_000_000.0, "north": 7_000_000_000.0, "elevation": 10_000.0}));
    let id = run(&mut s, "point_cloud", json!({"path": file("cloud-shared", "box.xyz", BOX), "origin": "shared"}))["id"].as_u64().unwrap();
    let (lo, hi) = s.model.cloud_box(id).unwrap();
    assert!(near3(lo, [0.0, 0.0, 0.0], 0.5) && near3(hi, [4000.0, 3000.0, 2500.0], 0.5), "{lo:?} {hi:?}");
}

#[test]
fn a_far_scan_acquires_its_coordinates_its_foot_at_the_origin() {
    let mut s = Session::new();
    let path = file("cloud-acquire", "box.xyz", BOX);
    let e = fails(&mut s, "point_cloud", json!({"path": path, "origin": "shared"}));
    assert!(e.contains("acquire"), "{e}");
    let id = run(&mut s, "point_cloud", json!({"path": path, "origin": "shared", "acquire": true}))["id"].as_u64().unwrap();
    let l = &s.model.coordinates.locations["Internal"];
    assert_eq!((l.east, l.north, l.elevation), (500_002_000.0, 7_000_002_000.0, 10_000.0));
    let (lo, hi) = s.model.cloud_box(id).unwrap();
    assert!(near3(lo, [-2000.0, -2000.0, 0.0], 0.5) && near3(hi, [2000.0, 1000.0, 2500.0], 0.5), "{lo:?} {hi:?}");
}

// ---------- IFC ----------

/// A project in São Paulo (SIRGAS 2000 / UTM 23S), true north 10° east of project north, with a wall.
fn surveyed() -> Session {
    let mut s = Session::new();
    run(&mut s, "crs", json!({"code": 31983}));
    run(
        &mut s,
        "location",
        json!({"name": "Internal", "east": 333_317_911.294, "north": 7_394_644_036.688, "elevation": 760_000.0, "angle": 10.0, "scale": 0.9996}),
    );
    run(&mut s, "coordinates", json!({"base": [1000.0, 2000.0, 0.0], "survey": [3000.0, -1000.0, 0.0]}));
    run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0]]}));
    s
}

fn ifc_of(s: &mut Session, test: &str, origin: Option<&str>) -> String {
    let path = file(test, "out.ifc", "");
    let mut p = json!({"path": path});
    if let Some(o) = origin {
        p["ifc_origin"] = json!(o);
    }
    run(s, "save", p);
    std::fs::read_to_string(&path).unwrap()
}

/// The wall's start in a project read from `text`, in its shared coordinates.
fn wall_start_shared(text: &str) -> ([f64; 3], crate::coordinates::Coordinates) {
    let i = crate::import::ifc(text.as_bytes(), "back").unwrap();
    let w = i
        .model
        .elements
        .values()
        .find_map(|e| match e {
            crate::model::Element::Wall(w) => Some((w.start, i.model.elevation(w.level) + w.base_offset)),
            _ => None,
        })
        .unwrap();
    (i.model.coordinates.to_shared([w.0.0, w.0.1, w.1]), i.model.coordinates.clone())
}

#[test]
fn ifc_goes_out_with_its_map_conversion_and_crs() {
    let mut s = surveyed();
    let text = ifc_of(&mut s, "ifc-out", None);
    // The internal origin's map coordinates, in metres, the map unit; the x axis turned 10°; the scale.
    let at = to_shared(&s, [0.0, 0.0, 0.0]);
    let (c, sn) = (10f64.to_radians().cos(), 10f64.to_radians().sin());
    let conversion = text.lines().find(|l| l.contains("IFCMAPCONVERSION(")).unwrap_or_else(|| panic!("no map conversion:\n{text}"));
    for v in [at[0] / 1000.0, at[1] / 1000.0, at[2] / 1000.0, c, sn, 0.9996] {
        let shown = format!("{v}");
        let digits = &shown[..shown.len().min(9)];
        assert!(conversion.contains(digits), "{digits} in {conversion}");
    }
    assert!(text.contains("IFCPROJECTEDCRS('EPSG:31983','SIRGAS 2000 / UTM zone 23S'"), "{text}");
    assert!(text.contains(".METRE.)") && text.contains("IFCSITE("), "{text}");
    // True north, in the file's axes: 10° east of its y.
    let north = format!("IFCDIRECTION(({},{}))", crate::ifc::ratio(sn), crate::ifc::ratio(c));
    assert!(text.contains(&north), "{north} in the file");
    // Its site's latitude, about 23° 33' south.
    let site = text.lines().find(|l| l.contains("IFCSITE(")).unwrap();
    assert!(site.contains("(-23,-33,"), "{site}");
}

#[test]
fn ifc_comes_back_where_it_was_in_the_world_from_each_origin() {
    for origin in [None, Some("internal"), Some("base"), Some("survey"), Some("shared")] {
        let mut s = surveyed();
        let want = to_shared(&s, [0.0, 0.0, 0.0]);
        let text = ifc_of(&mut s, &format!("ifc-{}", origin.unwrap_or("default")), origin);
        let (back, co) = wall_start_shared(&text);
        assert!(near3(back, want, 1.0), "{origin:?}: {back:?}, not {want:?}");
        assert_eq!(co.crs, Some(31983), "{origin:?}");
        let l = co.location();
        assert!((l.angle - 10.0).abs() < 1e-6 && (l.scale - 0.9996).abs() < 1e-9 || origin == Some("shared"), "{origin:?}: {l:?}");
    }
}

#[test]
fn ifc_out_at_map_coordinates_comes_in_near_the_origin_and_keeps_where_it_was() {
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Internal", "east": 500_000_000.0, "north": 7_000_000_000.0}));
    run(&mut s, "wall", json!({"points": [[1000, 2000], [9000, 2000]]}));
    let text = ifc_of(&mut s, "ifc-far", Some("shared"));
    let i = crate::import::ifc(text.as_bytes(), "back").unwrap();
    let wall = i.model.elements.values().find_map(|e| match e {
        crate::model::Element::Wall(w) => Some(w.start),
        _ => None,
    });
    let w = wall.unwrap();
    assert!(w.0.abs() < 1e6 && w.1.abs() < 1e6, "near the origin: {w:?}");
    let back = i.model.coordinates.to_shared([w.0, w.1, 0.0]);
    assert!(near3(back, [500_001_000.0, 7_000_002_000.0, 0.0], 1.0), "{back:?}");
}

#[test]
fn a_bad_ifc_origin_is_refused() {
    let mut s = Session::new();
    let e = fails(&mut s, "save", json!({"path": file("ifc-bad", "x.ifc", ""), "ifc_origin": "moon"}));
    assert!(e.contains("internal") && e.contains("shared"), "{e}");
}
