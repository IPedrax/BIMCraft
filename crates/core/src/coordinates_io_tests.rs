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
