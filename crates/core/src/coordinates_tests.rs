//! Coordinates: the Project Base Point, the Survey Point and the project's locations, through the commands every front end
//! drives ([the plan](../../../docs/coordinates-plan.md)).

use serde_json::{Value, json};

use crate::engine::Session;
use crate::library::Kind;
use crate::model::Model;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

fn close(a: &Value, b: [f64; 3]) -> bool {
    a.as_array().is_some_and(|a| a.len() == 3 && a.iter().zip(b).all(|(x, y)| x.as_f64().is_some_and(|x| (x - y).abs() < 1e-6)))
}

fn convert(s: &mut Session, from: &str, to: &str, p: [f64; 3]) -> Value {
    run(s, "convert_coordinates", json!({"points": [p], "from": from, "to": to}))["points"][0].clone()
}

#[test]
fn a_new_project_has_both_points_at_the_origin_and_one_location() {
    let mut s = Session::new();
    let r = run(&mut s, "coordinates", json!({}));
    assert!(close(&r["base"]["internal"], [0.0, 0.0, 0.0]), "{r}");
    assert!(close(&r["survey"]["internal"], [0.0, 0.0, 0.0]), "{r}");
    assert!(close(&r["survey"]["shared"], [0.0, 0.0, 0.0]), "{r}");
    assert_eq!(r["active"], "Internal");
    assert_eq!(r["locations"].as_object().map(|o| o.len()), Some(1));
    for name in ["Internal", "Project Base Point", "Survey Point"] {
        assert!(
            s.model.library.entries().any(|(_, e)| e.name == name && matches!(e.kind, Kind::Location | Kind::CoordinatePoint)),
            "{name}"
        );
    }
}

#[test]
fn a_location_turns_and_moves_the_shared_coordinates() {
    let mut s = Session::new();
    // A map grid 7,500 km north and 300 km east, true north 30 degrees east of project north.
    run(
        &mut s,
        "location",
        json!({"name": "Internal", "east": 300_000_000.0, "north": 7_500_000_000.0, "elevation": 100_000.0, "angle": 30.0}),
    );
    // Up the project's north by 1 m: on the map, 0.5 m west and 0.866 m north.
    let p = convert(&mut s, "internal", "shared", [0.0, 1000.0, 0.0]);
    assert!(close(&p, [300_000_000.0 - 500.0, 7_500_000_000.0 + 866.025_403_784_438_6, 100_000.0]), "{p}");
    // And back.
    let q = convert(&mut s, "shared", "internal", [300_000_000.0 - 500.0, 7_500_000_000.0 + 866.025_403_784_438_6, 100_000.0]);
    assert!(close(&q, [0.0, 1000.0, 0.0]), "{q}");
}

#[test]
fn true_north_east_of_project_north_turns_project_east_south() {
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Internal", "angle": 90.0}));
    // True north a quarter turn east of project north: project north points west on the map.
    let p = convert(&mut s, "internal", "shared", [0.0, 1000.0, 0.0]);
    assert!(close(&p, [-1000.0, 0.0, 0.0]), "{p}");
}

#[test]
fn the_scale_stretches_the_map_and_not_the_height() {
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Internal", "scale": 0.9996}));
    let p = convert(&mut s, "internal", "shared", [10_000.0, 0.0, 3000.0]);
    assert!(close(&p, [9996.0, 0.0, 3000.0]), "{p}");
}

#[test]
fn the_base_point_reports_from_where_it_stands() {
    let mut s = Session::new();
    run(&mut s, "coordinates", json!({"base": [5000.0, 2000.0, 300.0]}));
    let p = convert(&mut s, "internal", "base", [6000.0, 2000.0, 300.0]);
    assert!(close(&p, [1000.0, 0.0, 0.0]), "{p}");
    let r = run(&mut s, "coordinates", json!({}));
    assert!(close(&r["base"]["shared"], [5000.0, 2000.0, 300.0]), "{r}");
}

#[test]
fn moving_the_survey_point_alone_keeps_the_building_where_it_is_in_the_world() {
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Internal", "east": 1_000_000.0, "north": 2_000_000.0}));
    let before = convert(&mut s, "internal", "shared", [7000.0, 7000.0, 0.0]);
    run(&mut s, "coordinates", json!({"survey": [3000.0, 4000.0, 0.0]}));
    assert!(close(&convert(&mut s, "internal", "shared", [7000.0, 7000.0, 0.0]), serde_json::from_value(before).unwrap()));
    let r = run(&mut s, "coordinates", json!({}));
    assert!(close(&r["survey"]["shared"], [1_003_000.0, 2_004_000.0, 0.0]), "its numbers change: {r}");
}

#[test]
fn moving_the_survey_point_with_the_system_moves_the_building_in_the_world() {
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Internal", "east": 1_000_000.0, "north": 2_000_000.0}));
    run(&mut s, "coordinates", json!({"survey": [3000.0, 4000.0, 0.0], "move": "system"}));
    let r = run(&mut s, "coordinates", json!({}));
    assert!(close(&r["survey"]["shared"], [1_000_000.0, 2_000_000.0, 0.0]), "its numbers stay: {r}");
    let p = convert(&mut s, "internal", "shared", [3000.0, 4000.0, 0.0]);
    assert!(close(&p, [1_000_000.0, 2_000_000.0, 0.0]), "{p}");
}

#[test]
fn locations_are_added_activated_renamed_and_deleted() {
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Internal", "east": 10_000.0}));
    // A new one starts as a copy of the active one.
    let r = run(&mut s, "location", json!({"name": "Plot 2", "north": 50_000.0}));
    assert_eq!(r["location"]["east"], 10_000.0);
    assert_eq!(s.model.coordinates.active, "Internal", "a new one isn't active until asked");
    run(&mut s, "location", json!({"name": "Plot 2", "activate": true}));
    assert_eq!(s.model.coordinates.active, "Plot 2");
    let p = convert(&mut s, "internal", "shared", [0.0, 0.0, 0.0]);
    assert!(close(&p, [10_000.0, 50_000.0, 0.0]), "{p}");
    run(&mut s, "location", json!({"name": "Plot 2", "rename": "Lot B"}));
    assert_eq!(s.model.coordinates.active, "Lot B", "the active one follows its new name");
    assert!(s.model.library.entries().any(|(_, e)| e.kind == Kind::Location && e.name == "Lot B"));
    let e = fails(&mut s, "location", json!({"name": "Lot B", "delete": true}));
    assert!(e.contains("active"), "{e}");
    run(&mut s, "location", json!({"name": "Internal", "delete": true}));
    let e = fails(&mut s, "location", json!({"name": "Lot B", "delete": true}));
    assert!(e.contains("active"), "{e}");
    let e = fails(&mut s, "location", json!({"name": "Nowhere", "rename": "X"}));
    assert!(e.contains("no location"), "{e}");
}

#[test]
fn shift_moves_every_location_at_once() {
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Internal", "elevation": 100_000.0}));
    run(&mut s, "location", json!({"name": "B", "elevation": 120_000.0}));
    // The survey says the site is 5 m higher than we had it.
    run(&mut s, "location", json!({"shift": [0.0, 0.0, 5000.0]}));
    let c = &s.model.coordinates;
    assert_eq!(c.locations["Internal"].elevation, 105_000.0);
    assert_eq!(c.locations["B"].elevation, 125_000.0);
    // Or just one.
    run(&mut s, "location", json!({"name": "B", "shift": [1000.0, 0.0, 0.0]}));
    assert_eq!(s.model.coordinates.locations["B"].east, 1000.0);
    assert_eq!(s.model.coordinates.locations["Internal"].east, 0.0);
}

#[test]
fn specify_coordinates_solves_the_active_location() {
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Internal", "angle": 30.0}));
    run(
        &mut s,
        "specify_coordinates",
        json!({"at": [4000.0, 2000.0, 0.0], "east": 333_000_000.0, "north": 7_400_000_000.0, "elevation": 760_000.0}),
    );
    let p = convert(&mut s, "internal", "shared", [4000.0, 2000.0, 0.0]);
    assert!(close(&p, [333_000_000.0, 7_400_000_000.0, 760_000.0]), "{p}");
    assert_eq!(s.model.coordinates.locations["Internal"].angle, 30.0, "the angle stays unless given");
    run(&mut s, "specify_coordinates", json!({"at": [4000.0, 2000.0, 0.0], "east": 1.0, "north": 2.0, "elevation": 3.0, "angle": -12.5}));
    assert!(close(&convert(&mut s, "internal", "shared", [4000.0, 2000.0, 0.0]), [1.0, 2.0, 3.0]));
    assert_eq!(s.model.coordinates.locations["Internal"].angle, -12.5);
}

#[test]
fn coordinates_are_one_undo_step() {
    let mut s = Session::new();
    let before = s.model.coordinates.clone();
    run(&mut s, "location", json!({"name": "Internal", "east": 1.0}));
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.coordinates, before);
}

#[test]
fn bad_coordinates_are_refused_with_what_to_fix() {
    let mut s = Session::new();
    for (p, says) in [
        (json!({"name": "Internal", "scale": 2.0}), "scale"),
        (json!({"name": "Internal", "scale": 0.0}), "scale"),
        (json!({"name": "Internal", "angle": 1e9}), "angle"),
        (json!({"name": "Internal", "east": 1e20}), "east"),
        (json!({"name": "Internal", "north": "far"}), "north"),
        (json!({"shift": [1.0, 2.0]}), "shift"),
        (json!({"name": "Internal", "bogus": 1}), "bogus"),
    ] {
        let e = fails(&mut s, "location", p.clone());
        assert!(e.contains(says), "{p}: {e}");
    }
    let e = fails(&mut s, "coordinates", json!({"survey": [1.0, 2.0, 3.0], "move": "sideways"}));
    assert!(e.contains("point") && e.contains("system"), "{e}");
    let e = fails(&mut s, "convert_coordinates", json!({"points": [[0, 0, 0]], "from": "map", "to": "internal"}));
    assert!(e.contains("internal, base, shared"), "{e}");
}

#[test]
fn coordinates_survive_a_file() {
    let mut s = Session::new();
    run(&mut s, "coordinates", json!({"base": [1.0, 2.0, 3.0], "survey": [4.0, 5.0, 6.0]}));
    run(&mut s, "location", json!({"name": "Internal", "east": 7.0, "north": 8.0, "elevation": 9.0, "angle": 10.0, "scale": 0.9999}));
    run(&mut s, "location", json!({"name": "Second", "activate": true}));
    let text = s.model.to_json().unwrap();
    let (m, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.coordinates, s.model.coordinates);
    assert_eq!(m.check(), Ok(()));
}

#[test]
fn a_file_from_before_coordinates_reads_with_the_defaults() {
    let s = Session::new();
    let mut v: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    v.as_object_mut().unwrap().remove("coordinates");
    let (m, _) = Model::read(&v.to_string()).unwrap();
    assert_eq!(m.coordinates, crate::coordinates::Coordinates::default());
    assert_eq!(m.check(), Ok(()));
}

#[test]
fn a_project_in_portuguese_starts_with_its_location_named_in_it() {
    let mut s = Session::new();
    run(&mut s, "new", json!({"language": "pt"}));
    assert_eq!(s.model.coordinates.active, "Interno");
    assert_eq!(s.model.check(), Ok(()));
}

// ---------- a map's coordinate reference system, and WGS84 ----------

/// A location whose Survey Point, at the internal origin, has grid coordinates `e`, `n` in the CRS's own unit, `unit` mm.
fn at_grid(s: &mut Session, code: u32, e: f64, n: f64, unit: f64) {
    run(s, "crs", json!({"code": code}));
    run(s, "location", json!({"name": "Internal", "east": e * unit, "north": n * unit}));
}

fn wgs84(s: &mut Session) -> (f64, f64) {
    let p = convert(s, "internal", "wgs84", [0.0, 0.0, 0.0]);
    (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())
}

/// Reference values from PROJ 9's cs2cs, EPSG:4326 to each CRS. WGS84 is for the map and the sun, so a few centimetres
/// (proj4rs's Lambert, against PROJ's) or metres (a Helmert shift where PROJ may use a grid) are within what it is for.
#[test]
fn shared_coordinates_reach_wgs84_through_the_crs() {
    // (code, grid east, grid north, mm per unit, longitude, latitude, tolerance in degrees)
    let us_foot = 1_200_000.0 / 3937.0;
    for (code, e, n, unit, lon, lat, tol) in [
        (31983, 333_317.911_294, 7_394_644.036_688, 1000.0, -46.633, -23.55, 1e-8),
        (31982, 673_648.491_098, 7_186_491.014_442, 1000.0, -49.2733, -25.4284, 1e-8),
        (32633, 602_065.207_411, 5_340_353.594_304, 1000.0, 16.3738, 48.2082, 1e-8),
        (2263, 988_212.166_662, 211_939.490_081, us_foot, -73.9857, 40.7484, 2e-6),
        (27700, 530_268.167_252, 179_640.532_249, 1000.0, -0.1246, 51.5007, 5e-5),
    ] {
        let mut s = Session::new();
        at_grid(&mut s, code, e, n, unit);
        let (x, y) = wgs84(&mut s);
        assert!((x - lon).abs() < tol && (y - lat).abs() < tol, "EPSG:{code}: {x}, {y}, not {lon}, {lat}");
        // And back where it was, to the millimetre.
        let back = convert(&mut s, "wgs84", "shared", [x, y, 0.0]);
        let b: Vec<f64> = back.as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        assert!((b[0] - e * unit).abs() < 1.0 && (b[1] - n * unit).abs() < 1.0, "EPSG:{code}: back to {back}");
    }
}

#[test]
fn the_crs_says_its_name_and_unit() {
    let mut s = Session::new();
    run(&mut s, "crs", json!({"code": "EPSG:2263"}));
    let r = run(&mut s, "coordinates", json!({}));
    assert_eq!(r["crs"]["code"], 2263, "{r}");
    assert!(r["crs"]["name"].as_str().is_some_and(|n| n.contains("New York")), "{r}");
    assert_eq!(r["crs"]["unit"], "US survey foot", "{r}");
    assert!(r["survey"]["wgs84"].is_array(), "{r}");
    run(&mut s, "crs", json!({"clear": true}));
    assert_eq!(s.model.coordinates.crs, None);
    let r = run(&mut s, "coordinates", json!({}));
    assert!(r["survey"].get("wgs84").is_none(), "{r}");
}

#[test]
fn crs_search_finds_by_name_and_code() {
    let mut s = Session::new();
    let r = run(&mut s, "crs", json!({"search": "sirgas 2000 utm 23s"}));
    let found: Vec<u64> = r["found"].as_array().unwrap().iter().filter_map(|x| x["code"].as_u64()).collect();
    assert!(found.contains(&31983), "{r}");
    let r = run(&mut s, "crs", json!({"search": "27700"}));
    assert_eq!(r["found"][0]["code"], 27700, "{r}");
    let r = run(&mut s, "crs", json!({"search": "utm", "limit": 3}));
    assert_eq!(r["found"].as_array().map(Vec::len), Some(3));
}

#[test]
fn only_a_known_projected_crs_is_taken() {
    let mut s = Session::new();
    let e = fails(&mut s, "crs", json!({"code": 4326}));
    assert!(e.contains("projected"), "{e}");
    let e = fails(&mut s, "crs", json!({"code": 1}));
    assert!(e.contains("EPSG:1"), "{e}");
    let e = fails(&mut s, "convert_coordinates", json!({"points": [[0, 0, 0]], "from": "internal", "to": "wgs84"}));
    assert!(e.contains("crs"), "{e}");
}

#[test]
fn a_location_is_placed_by_latitude_and_longitude() {
    let mut s = Session::new();
    run(&mut s, "crs", json!({"code": 31983}));
    run(&mut s, "location", json!({"name": "Internal", "longitude": -46.633, "latitude": -23.55, "elevation": 760_000.0}));
    let l = &s.model.coordinates.locations["Internal"];
    assert!((l.east - 333_317_911.294).abs() < 1.0 && (l.north - 7_394_644_036.688).abs() < 1.0, "{l:?}");
    assert_eq!(l.elevation, 760_000.0);
    let e = fails(&mut s, "location", json!({"name": "Internal", "latitude": 95.0, "longitude": 0.0}));
    assert!(e.contains("latitude"), "{e}");
}
