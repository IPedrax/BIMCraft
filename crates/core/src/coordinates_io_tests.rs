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
    // The internal origin's map coordinates, in metres, the map unit; the x axis turned 10°; the scale, mm to m with it.
    let at = to_shared(&s, [0.0, 0.0, 0.0]);
    let (c, sn) = (10f64.to_radians().cos(), 10f64.to_radians().sin());
    let conversion = text.lines().find(|l| l.contains("IFCMAPCONVERSION(")).unwrap_or_else(|| panic!("no map conversion:\n{text}"));
    for v in [at[0] / 1000.0, at[1] / 1000.0, at[2] / 1000.0, c, sn, 0.0009996] {
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

/// The numbers of the file's `IfcMapConversion`: eastings, northings, height, x axis abscissa and ordinate, scale.
fn conversion_of(text: &str) -> [f64; 6] {
    let line = text.lines().find(|l| l.contains("IFCMAPCONVERSION(")).unwrap_or_else(|| panic!("no map conversion:\n{text}"));
    let args = line.split("IFCMAPCONVERSION(").nth(1).unwrap().trim_end_matches(';').trim_end_matches(')');
    let v: Vec<f64> = args.split(',').skip(2).map(|x| x.trim().parse().unwrap_or_else(|e| panic!("{x} in {line}: {e}"))).collect();
    [v[0], v[1], v[2], v[3], v[4], v[5]]
}

/// A point of the file (its length unit, mm) on the map, as IFC4 and IfcOpenShell's `xyz2enh` take it: the scale carries the
/// conversion from the project's unit to the map's.
fn on_map(k: [f64; 6], p: [f64; 3]) -> [f64; 3] {
    let t = k[4].atan2(k[3]);
    let (c, s) = (t.cos(), t.sin());
    [k[5] * (c * p[0] - s * p[1]) + k[0], k[5] * (s * p[0] + c * p[1]) + k[1], k[5] * p[2] + k[2]]
}

#[test]
fn ifc_map_conversion_scale_takes_the_projects_millimetres_to_the_maps_unit() {
    // In metres: 0.9996 from ground to grid, and 0.001 from millimetres to metres.
    let mut s = surveyed();
    let text = ifc_of(&mut s, "ifc-scale-m", None);
    let k = conversion_of(&text);
    assert!((k[5] - 0.9996e-3).abs() < 1e-12, "scale {} in {k:?}", k[5]);
    // The wall's end, 8 m east of the origin, lands where the project's shared coordinates put it, in metres.
    let want = to_shared(&s, [8000.0, 0.0, 0.0]).map(|x| x / 1000.0);
    let got = on_map(k, [8000.0, 0.0, 0.0]);
    assert!(near3(got, want, 1e-3), "{got:?}, not {want:?}");
    // From the survey point too.
    let text = ifc_of(&mut s, "ifc-scale-survey", Some("survey"));
    let k = conversion_of(&text);
    let got = on_map(k, [8000.0 - 3000.0, 1000.0, 0.0]);
    assert!(near3(got, want, 1e-3), "survey: {got:?}, not {want:?}");
    // Shared: the file's coordinates are the map's, in millimetres; the scale only changes the unit.
    let text = ifc_of(&mut s, "ifc-scale-shared", Some("shared"));
    let k = conversion_of(&text);
    assert!((k[5] - 1e-3).abs() < 1e-15 && k[0] == 0.0 && k[1] == 0.0, "{k:?}");

    // In US survey feet (NY Long Island): the scale takes millimetres to feet.
    let mut s = Session::new();
    run(&mut s, "crs", json!({"code": 2263}));
    run(&mut s, "location", json!({"name": "Internal", "east": 300_000_000.0, "north": 60_000_000.0, "scale": 0.9999}));
    run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0]]}));
    let text = ifc_of(&mut s, "ifc-scale-ft", None);
    let k = conversion_of(&text);
    let ft = 1200.0 / 3937.0 * 1000.0;
    assert!((k[5] - 0.9999 / ft).abs() < 1e-12, "scale {} in {k:?}", k[5]);
    let want = to_shared(&s, [8000.0, 0.0, 0.0]).map(|x| x / ft);
    let got = on_map(k, [8000.0, 0.0, 0.0]);
    assert!(near3(got, want, 1e-3), "{got:?}, not {want:?}");
}

#[test]
fn ifc_whose_scale_carries_the_unit_comes_in_with_its_ground_to_grid_scale() {
    // Every other point of the round trip, not only the origin: the wall's end.
    for origin in [None, Some("base"), Some("shared")] {
        let mut s = surveyed();
        let want = to_shared(&s, [8000.0, 0.0, 0.0]);
        let text = ifc_of(&mut s, &format!("ifc-end-{}", origin.unwrap_or("default")), origin);
        let i = crate::import::ifc(text.as_bytes(), "back").unwrap();
        let end = i.model.elements.values().find_map(|e| match e {
            crate::model::Element::Wall(w) => Some(w.end),
            _ => None,
        });
        let e = end.unwrap();
        let back = i.model.coordinates.to_shared([e.0, e.1, 0.0]);
        // At the shared origin the site's placement moves and turns but can't scale: the wall goes out at its length on the
        // ground, 8 m, not on the grid, 0.9996 of it (3.2 mm less).
        let tol = if origin == Some("shared") { 8000.0 * (1.0 - 0.9996) + 1.0 } else { 1.0 };
        assert!(near3([back[0], back[1], 0.0], [want[0], want[1], 0.0], tol), "{origin:?}: {back:?}, not {want:?}");
    }
    // A file written by another tool as IFC4 says: millimetres, a map in metres, scale 0.001 (ground to grid 1).
    let text = ifc_of(&mut surveyed(), "ifc-spec", None);
    let k = conversion_of(&text);
    let theirs = text.replace(&crate::ifc::ratio(k[5]), "0.001");
    assert_ne!(theirs, text);
    let (_, co) = wall_start_shared(&theirs);
    let l = co.location();
    assert!((l.scale - 1.0).abs() < 1e-9, "ground to grid 1, the unit taken out: {l:?}");
}

#[test]
fn shared_ifc_with_a_scale_warns_that_it_goes_out_at_ground_size() {
    // Scale 0.9996: the site's placement can't scale, so the file is at ground size, off the grid by 0.4 mm a metre.
    let mut s = surveyed();
    let path = file("ifc-shared-warn", "out.ifc", "");
    let r = run(&mut s, "save", json!({"path": path, "ifc_origin": "shared"}));
    let w = r["warning"].as_str().unwrap_or_else(|| panic!("a warning: {r}"));
    assert!(w.contains("0.9996") && w.contains("0.4 mm") && w.contains("internal"), "{w}");
    // Every other origin carries the scale in the map conversion: no warning.
    for o in ["internal", "base", "survey"] {
        let r = run(&mut s, "save", json!({"path": path, "ifc_origin": o}));
        assert!(r.get("warning").is_none(), "{o}: {r}");
    }
    // Scale 1: shared is exact.
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Internal", "east": 500_000_000.0, "north": 7_000_000_000.0}));
    let r = run(&mut s, "save", json!({"path": path, "ifc_origin": "shared"}));
    assert!(r.get("warning").is_none(), "{r}");
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

// ---------- links ----------

/// A project with a wall from (0, 0) to (8000, 0) on its Level 1, its locations as `locations` says, saved: the path.
fn linked_file(test: &str, locations: &[(&str, Value)], base: [f64; 3]) -> String {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0]]}));
    for (k, (name, l)) in locations.iter().enumerate() {
        let mut p = l.clone();
        p["name"] = json!(name);
        if k == 0 {
            run(&mut s, "location", json!({"name": "Internal", "rename": name}));
            p["activate"] = json!(true);
        }
        run(&mut s, "location", p);
    }
    run(&mut s, "coordinates", json!({"base": base}));
    let path = file(test, "linked.bimcraft", "");
    run(&mut s, "save", json!({"path": path}));
    path
}

/// The linked wall as the views see it: its start, its end and its level's name.
fn linked_wall(s: &Session) -> (crate::geom::P2, crate::geom::P2, String) {
    let seen = crate::link::seen(&s.model);
    seen.elements
        .iter()
        .find_map(|(id, e)| match e {
            crate::model::Element::Wall(w) if crate::link::is_linked(*id) => {
                Some((w.start, w.end, seen.level(w.level).map(|l| l.name.clone()).unwrap_or_default()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no linked wall"))
}

fn close2(a: crate::geom::P2, b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
}

#[test]
fn a_link_by_shared_coordinates_stands_where_its_map_coordinates_say() {
    // The linked project's origin is 10 m east of this one's on the map, and its true north 30° east of its project north.
    let path = linked_file("link-shared", &[("Plot", json!({"east": 1_010_000.0, "north": 2_000_000.0, "angle": 30.0}))], [0.0; 3]);
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Internal", "east": 1_000_000.0, "north": 2_000_000.0}));
    let r = run(&mut s, "link", json!({"path": path, "by": "shared"}));
    let (a, b, level) = linked_wall(&s);
    assert!(close2(a, (10_000.0, 0.0)), "{a:?} {r}");
    // Its x axis turned 30° counter-clockwise on the map: its wall's end too, here, where this project's north is the map's.
    let (c, sn) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    assert!(close2(b, (10_000.0 + 8000.0 * c, 8000.0 * sn)), "{b:?}");
    assert_eq!(level, "Level 1");
    // This project's location moves: the link follows, worked out again, never written into its file.
    run(&mut s, "location", json!({"name": "Internal", "east": 1_005_000.0}));
    assert!(close2(linked_wall(&s).0, (5000.0, 0.0)));
}

#[test]
fn a_link_by_shared_coordinates_stands_at_its_own_elevation_and_location() {
    // Two positions of the same building; the second 3 m up, so its Level 1 is this project's Level 2.
    let path =
        linked_file("link-elevation", &[("A", json!({"east": 0.0})), ("B", json!({"east": 20_000.0, "elevation": 3000.0}))], [0.0; 3]);
    let mut s = Session::new();
    run(&mut s, "link", json!({"path": path, "by": "shared"}));
    let (a, _, level) = linked_wall(&s);
    assert!(close2(a, (0.0, 0.0)) && level == "Level 1", "its active location, A: {a:?} {level}");
    run(&mut s, "link", json!({"path": path, "by": "shared", "location": "B", "name": "B"}));
    let seen = crate::link::seen(&s.model);
    let on_level_2 = seen.elements.iter().any(|(id, e)| {
        matches!(e, crate::model::Element::Wall(w) if crate::link::is_linked(*id) && w.start.0 > 19_000.0
            && seen.level(w.level).is_some_and(|l| l.name == "Level 2"))
    });
    assert!(on_level_2, "B stands 3 m up, on Level 2");
    let e = fails(&mut s, "link", json!({"path": path, "by": "shared", "location": "Nowhere"}));
    assert!(e.contains("A") && e.contains("B"), "{e}");
}

#[test]
fn a_link_by_base_points_puts_its_base_point_on_this_ones() {
    let path = linked_file("link-base", &[("Internal", json!({}))], [1000.0, 1000.0, 0.0]);
    let mut s = Session::new();
    run(&mut s, "coordinates", json!({"base": [5000.0, 0.0, 0.0]}));
    run(&mut s, "link", json!({"path": path, "by": "base"}));
    assert!(close2(linked_wall(&s).0, (4000.0, -1000.0)));
}

#[test]
fn a_project_acquires_its_coordinates_from_a_link_placed_by_hand() {
    let path = linked_file(
        "link-acquire",
        &[("Plot", json!({"east": 1_000_000.0, "north": 2_000_000.0, "elevation": 760_000.0, "angle": 15.0}))],
        [0.0; 3],
    );
    let mut s = Session::new();
    let id = run(&mut s, "link", json!({"path": path, "at": [10_000, 0], "angle": 5}))["id"].as_u64().unwrap();
    run(&mut s, "coordinates", json!({"acquire": id}));
    // Where the link's origin is, this project's shared coordinates are the link's.
    let q = s.model.coordinates.to_shared([10_000.0, 0.0, 0.0]);
    assert!(near3(q, [1_000_000.0, 2_000_000.0, 760_000.0], 1e-3), "{q:?}");
    assert!((s.model.coordinates.location().angle - 10.0).abs() < 1e-9, "15° less the 5° it was turned");
    // And so, by shared coordinates, it stands where it was put.
    run(&mut s, "modify", json!({"id": id, "by": "shared"}));
    assert!(close2(linked_wall(&s).0, (10_000.0, 0.0)));
}

#[test]
fn a_link_placed_by_shared_coordinates_is_not_moved_by_hand_until_it_is_manual() {
    let path = linked_file("link-move", &[("Plot", json!({"east": 1_010_000.0, "north": 2_000_000.0}))], [0.0; 3]);
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Internal", "east": 1_000_000.0, "north": 2_000_000.0}));
    let id = run(&mut s, "link", json!({"path": path, "by": "shared"}))["id"].as_u64().unwrap();
    let e = fails(&mut s, "move", json!({"ids": [id], "by": [1000, 0]}));
    assert!(e.contains("shared") && e.contains("manual"), "{e}");
    // Placed by hand where it stands, it moves.
    run(&mut s, "modify", json!({"id": id, "by": "manual", "at": [10_000, 0]}));
    run(&mut s, "move", json!({"ids": [id], "by": [1000, 0]}));
    assert!(close2(linked_wall(&s).0, (11_000.0, 0.0)));
}

// ---------- Acquire Coordinates ----------

#[test]
fn a_project_acquires_its_coordinates_from_a_placed_drawing() {
    let mut s = Session::new();
    let path = file("cad-acquire-placed", "site.dxf", &site_plan(500_000.0, 7_000_000.0));
    // Placed by hand: its middle on (2000, 1000), turned 20°.
    let first = run(&mut s, "import_cad", json!({"path": path, "at": [2000, 1000], "angle": 20}));
    let id = first["id"].as_u64().unwrap();
    let r = run(&mut s, "coordinates", json!({"acquire": id}));
    // Where its middle stands, the shared coordinates are the drawing's own, in mm, and its axes are the map's.
    let q = to_shared(&s, [2000.0, 1000.0, 0.0]);
    assert!(near3(q, [500_005_000.0, 7_000_003_000.0, 0.0], 1e-3), "{q:?} {r}");
    assert!((s.model.coordinates.location().angle + 20.0).abs() < 1e-9, "{r}");
    assert_eq!(r["acquired"]["from"], json!(id), "{r}");
    // So the same drawing brought in by shared coordinates lands on it.
    let again = run(&mut s, "import_cad", json!({"cad": "site", "position": "shared"}));
    assert!(near2(&again["extents"], serde_json::from_value(first["extents"].clone()).unwrap(), 1e-3), "{again} {first}");
    // Only a link or a drawing has coordinates of its own to give.
    run(&mut s, "wall", json!({"points": [[0, 0], [1000, 0]]}));
    let wall = s.model.elements.iter().find(|(_, e)| matches!(e, crate::model::Element::Wall(_))).map(|(id, _)| *id).unwrap();
    let e = fails(&mut s, "coordinates", json!({"acquire": wall}));
    assert!(e.contains("link") && e.contains("CAD"), "{e}");
}

#[test]
fn a_project_acquires_its_coordinates_from_a_linked_ifc() {
    let mut s = surveyed();
    let want = to_shared(&s, [0.0, 0.0, 0.0]);
    let text = ifc_of(&mut s, "ifc-link-acquire", None);
    let path = file("ifc-link-acquire", "site.ifc", &text);
    let mut h = Session::new();
    let id = run(&mut h, "link", json!({"path": path, "at": [10_000, 0], "angle": 5}))["id"].as_u64().unwrap();
    run(&mut h, "coordinates", json!({"acquire": id}));
    let q = h.model.coordinates.to_shared([10_000.0, 0.0, 0.0]);
    assert!(near3(q, want, 1.0), "{q:?}, not {want:?}");
    let l = h.model.coordinates.location();
    assert!((l.angle - 5.0).abs() < 1e-6 && (l.scale - 0.9996).abs() < 1e-9, "10° less the 5° it was turned: {l:?}");
    assert_eq!(h.model.coordinates.crs, Some(31983), "the map system comes with it");
}

/// [`site_plan`] whose header says inches (`$INSUNITS` 1), as a civil drawing in metres often wrongly does.
fn site_plan_said_inches(e: f64, n: f64) -> String {
    site_plan(e, n).replace("$INSUNITS\n70\n6\n", "$INSUNITS\n70\n1\n")
}

#[test]
fn map_coordinates_whose_file_says_inches_are_read_in_the_map_systems_unit() {
    // Dubai: DLTM, metres. The plot's corner at 248 599.2464, 2 715 989.0282.
    let mut s = Session::new();
    run(&mut s, "crs", json!({"code": 3997}));
    let path = file("cad-said-inches", "plot.dxf", &site_plan_said_inches(248_599.246_4, 2_715_989.028_2));
    let r = run(&mut s, "import_cad", json!({"path": path}));
    assert_eq!(r["scale"], 1000.0, "read in metres, the map system's unit: {r}");
    assert!(r["notes"].to_string().contains("inches") && r["notes"].to_string().contains("metre"), "and it says why: {r}");
    let id = r["id"].as_u64().unwrap();
    run(&mut s, "coordinates", json!({"acquire": id}));
    let ci = match s.model.elements.get(id) {
        Some(crate::model::Element::CadImport(c)) => c.clone(),
        _ => panic!(),
    };
    let corner = crate::model::cad_place(&ci)(crate::geom::P2(248_599.246_4, 2_715_989.028_2));
    let q = to_shared(&s, [corner.0, corner.1, 0.0]);
    assert!(near3(q, [248_599_246.4, 2_715_989_028.2, 0.0], 1e-3), "{q:?}");
    // A spot coordinate on each corner reads what the drawing's program says of it.
    for (dx, dy) in [(0.0, 0.0), (10.0, 0.0), (10.0, 6.0), (0.0, 6.0)] {
        let (e, n) = (248_599.246_4 + dx, 2_715_989.028_2 + dy);
        let q = crate::model::cad_place(&ci)(crate::geom::P2(e, n));
        let r = run(&mut s, "spot_coordinate", json!({"at": [q.0, q.1]}));
        assert_eq!(r["text"], json!([format!("N {n:.4}"), format!("E {e:.4}")]), "{r}");
    }
    // Placed origin to origin, it would land 2 700 km off: refused, saying how to bring it in.
    let e = fails(&mut s, "import_cad", json!({"cad": "plot", "position": "origin"}));
    assert!(e.contains("acquire") && e.contains("shared"), "{e}");
    // Units given are kept, whatever the map.
    let r = run(&mut s, "import_cad", json!({"cad": "plot", "units": "in"}));
    assert_eq!(r["scale"], 25.4);
    // With no map system: metres, which map coordinates are, never inches.
    let mut s = Session::new();
    let r = run(&mut s, "import_cad", json!({"path": path}));
    assert_eq!(r["scale"], 1000.0, "{r}");
    // A file in feet at map coordinates may be State Plane: kept in feet, with a warning.
    let feet =
        file("cad-said-feet", "plot.dxf", &site_plan(248_599.246_4, 2_715_989.028_2).replace("$INSUNITS\n70\n6\n", "$INSUNITS\n70\n2\n"));
    let r = run(&mut s, "import_cad", json!({"path": feet, "name": "feet"}));
    assert_eq!(r["scale"], 304.8, "{r}");
    assert!(r["warning"].as_str().is_some_and(|w| w.contains("feet") && w.contains("units m")), "{r}");
}

#[test]
fn reset_puts_the_coordinates_back_on_the_internal_origin_and_pins_the_links_where_they_stand() {
    // A link by shared coordinates 10 m east and 3 m up, another by base points.
    let path =
        linked_file("reset", &[("Plot", json!({"east": 1_010_000.0, "north": 2_000_000.0, "elevation": 3000.0, "angle": 30.0}))], [0.0; 3]);
    let based = linked_file("reset-base", &[("Internal", json!({}))], [1000.0, 1000.0, 0.0]);
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Internal", "east": 1_000_000.0, "north": 2_000_000.0, "angle": 12.0}));
    run(&mut s, "coordinates", json!({"base": [5000.0, 0.0, 200.0], "survey": [-2000.0, 1000.0, 0.0]}));
    let shared = run(&mut s, "link", json!({"path": path, "by": "shared"}))["id"].as_u64().unwrap();
    let by_base = run(&mut s, "link", json!({"path": based, "by": "base", "name": "B"}))["id"].as_u64().unwrap();
    let seen = |s: &Session| {
        let m = crate::link::seen(&s.model);
        let mut walls: Vec<_> = m
            .elements
            .iter()
            .filter_map(|(id, e)| match e {
                crate::model::Element::Wall(w) if crate::link::is_linked(*id) => {
                    Some((w.start, w.end, m.level(w.level).map(|l| l.elevation + w.base_offset).unwrap_or_default()))
                }
                _ => None,
            })
            .collect();
        walls.sort_by(|a, b| a.0.0.total_cmp(&b.0.0));
        walls
    };
    let before = seen(&s);
    let r = run(&mut s, "coordinates", json!({"reset": true}));
    let co = &s.model.coordinates;
    assert_eq!((co.base, co.survey), ([0.0; 3], [0.0; 3]), "{r}");
    assert_eq!(co.to_shared([1234.0, 5678.0, 9.0]), [1234.0, 5678.0, 9.0], "shared is internal again");
    let after = seen(&s);
    assert_eq!(before.len(), after.len());
    for (a, b) in before.iter().zip(&after) {
        assert!(a.0.dist(b.0) < 1e-6 && a.1.dist(b.1) < 1e-6 && (a.2 - b.2).abs() < 1e-6, "no link moves: {a:?} {b:?}");
    }
    for id in [shared, by_base] {
        let Some(crate::model::Element::Link(l)) = s.model.elements.get(id) else { panic!() };
        assert_eq!(l.by, crate::link::By::Manual, "pinned by origin where it stands");
    }
    // One undo step brings it all back.
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.coordinates.base, [5000.0, 0.0, 200.0]);
}

#[test]
fn reset_pins_an_unloaded_link_where_it_last_stood() {
    let path = linked_file(
        "reset-unloaded",
        &[("Plot", json!({"east": 1_010_000.0, "north": 2_000_000.0, "elevation": 3000.0, "angle": 30.0}))],
        [0.0; 3],
    );
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Internal", "east": 1_000_000.0, "north": 2_000_000.0}));
    let id = run(&mut s, "link", json!({"path": path, "by": "shared"}))["id"].as_u64().unwrap();
    // Where it stands is remembered in the project, and kept up as the project's coordinates change.
    run(&mut s, "location", json!({"name": "Internal", "east": 1_004_000.0}));
    let Some(crate::model::Element::Link(l)) = s.model.elements.get(id) else { panic!() };
    let placed = l.placed.unwrap_or_else(|| panic!("no place remembered: {l:?}"));
    assert!(
        placed.at.dist(crate::geom::P2(6000.0, 0.0)) < 1e-6 && (placed.angle - 30.0).abs() < 1e-9 && (placed.raised - 3000.0).abs() < 1e-9,
        "{placed:?}"
    );
    // Its file gone (the project opened elsewhere): reset still pins it there.
    std::sync::Arc::make_mut(&mut s.model).loaded.clear();
    run(&mut s, "coordinates", json!({"reset": true}));
    let Some(crate::model::Element::Link(l)) = s.model.elements.get(id) else { panic!() };
    assert_eq!((l.by, l.at, l.angle, l.raised), (crate::link::By::Manual, placed.at, placed.angle, placed.raised));
}
