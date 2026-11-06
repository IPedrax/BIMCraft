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
fn a_point_picked_in_plan_stands_at_a_wall_s_bottom_and_a_floor_s_top() {
    let mut s = Session::new();
    run(&mut s, "level", json!({"elevation": 0.0, "name": "L1"}));
    run(&mut s, "level", json!({"elevation": 3000.0, "name": "L2"}));
    let wall = run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0]], "level": "L2", "base_offset": 200.0}))["ids"][0].clone();
    let floor =
        run(&mut s, "floor", json!({"boundary": [[0, 2000], [4000, 2000], [4000, 6000], [0, 6000]], "level": "L2", "offset": 150.0}))["id"]
            .clone();
    let shared = |s: &mut Session, at: [f64; 3]| convert(s, "internal", "shared", at);
    // A wall's corner: its bottom.
    run(&mut s, "specify_coordinates", json!({"at": [8000.0, 0.0], "on": wall, "east": 1000.0, "north": 2000.0, "elevation": 50000.0}));
    assert!(close(&shared(&mut s, [8000.0, 0.0, 3200.0]), [1000.0, 2000.0, 50000.0]), "the wall's base, L2 + 200");
    // A floor's corner: its top.
    run(&mut s, "specify_coordinates", json!({"at": [4000.0, 6000.0], "on": floor, "east": 1.0, "north": 2.0, "elevation": 7.0}));
    assert!(close(&shared(&mut s, [4000.0, 6000.0, 3150.0]), [1.0, 2.0, 7.0]), "the floor's top, L2 + 150");
    // Nothing named: the top of what stands on the level there, else the level.
    run(&mut s, "specify_coordinates", json!({"at": [1000.0, 3000.0], "level": "L2", "east": 0.0, "north": 0.0, "elevation": 0.0}));
    assert!(close(&shared(&mut s, [1000.0, 3000.0, 3150.0]), [0.0, 0.0, 0.0]), "on the floor");
    run(&mut s, "specify_coordinates", json!({"at": [6000.0, 5000.0], "level": "L2", "east": 0.0, "north": 0.0, "elevation": 0.0}));
    assert!(close(&shared(&mut s, [6000.0, 5000.0, 3000.0]), [0.0, 0.0, 0.0]), "on the level");
    let e = fails(&mut s, "specify_coordinates", json!({"at": [0.0, 0.0, 0.0], "on": wall, "east": 0.0, "north": 0.0, "elevation": 0.0}));
    assert!(e.contains("`on`"), "a height given and one to work out: {e}");
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

fn codes(place: &Value) -> Vec<u64> {
    place["systems"].as_array().map_or_else(Vec::new, |a| a.iter().filter_map(|x| x["code"].as_u64()).collect())
}

#[test]
fn crs_search_finds_a_city_and_pairs_it_with_its_country_s_systems() {
    let mut s = Session::new();
    let r = run(&mut s, "crs", json!({"search": "São Paulo"}));
    let p = &r["places"][0];
    assert_eq!((p["kind"].as_str(), p["name"].as_str(), p["country"].as_str()), (Some("city"), Some("São Paulo"), Some("Brazil")), "{r}");
    assert!((p["latitude"].as_f64().unwrap() + 23.56).abs() < 0.1 && (p["longitude"].as_f64().unwrap() + 46.63).abs() < 0.1, "{p}");
    let c = codes(p);
    assert_eq!(c.first(), Some(&31983), "SIRGAS 2000 / UTM zone 23S first, Brazil's: {p}");
    assert!(c.contains(&5880) && c.contains(&32723), "Brazil Polyconic, and WGS 84's zone: {p}");
    assert!(p["systems"][0]["why"].as_str().is_some_and(|w| !w.is_empty()), "{p}");
    // Without its accents, with the country in Portuguese.
    let r = run(&mut s, "crs", json!({"search": "sao paulo brasil"}));
    assert_eq!(r["places"][0]["name"], "São Paulo", "{r}");
    // A national grid comes before the UTM zone.
    let r = run(&mut s, "crs", json!({"search": "London"}));
    assert_eq!((r["places"][0]["country"].as_str(), codes(&r["places"][0]).first()), (Some("United Kingdom"), Some(&27700)), "{r}");
    let r = run(&mut s, "crs", json!({"search": "Sydney"}));
    assert_eq!(codes(&r["places"][0]).first(), Some(&7856), "GDA2020 / MGA zone 56: {r}");
}

#[test]
fn dubai_finds_its_local_transverse_mercator_by_place_and_by_words_of_its_name() {
    let mut s = Session::new();
    let r = run(&mut s, "crs", json!({"search": "Dubai"}));
    assert_eq!(r["places"][0]["name"], "Dubai", "{r}");
    assert_eq!(codes(&r["places"][0]).first(), Some(&3997), "WGS 84 / Dubai Local TM, the emirate's own: {r}");
    // EPSG names it "Dubai Local TM": TM is transverse Mercator, as people call it.
    for text in ["dubai local mercator", "dubai local transverse mercator", "DLTM"] {
        let r = run(&mut s, "crs", json!({"search": text}));
        let found: Vec<u64> = r["found"].as_array().unwrap().iter().filter_map(|x| x["code"].as_u64()).collect();
        assert!(found.contains(&3997), "{text}: {r}");
    }
}

#[test]
fn crs_search_finds_a_country_by_any_of_its_names() {
    let mut s = Session::new();
    let r = run(&mut s, "crs", json!({"search": "Alemania"}));
    let p = &r["places"][0];
    assert_eq!((p["kind"].as_str(), p["name"].as_str()), (Some("country"), Some("Germany")), "{r}");
    assert_eq!(codes(p).first(), Some(&25832), "ETRS89 / UTM zone 32N at Germany's middle: {p}");
    // A name that is no place lists none, and a code still finds its system.
    let r = run(&mut s, "crs", json!({"search": "31983"}));
    assert_eq!((r["found"][0]["code"].as_u64(), r["places"].as_array().map(Vec::len)), (Some(31983), Some(0)), "{r}");
    let r = run(&mut s, "crs", json!({"search": "Brazil", "limit": 2}));
    assert!(r["places"].as_array().is_some_and(|a| a.len() <= 2 && a[0]["kind"] == "country"), "the country before its cities: {r}");
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

// ---------- elevations and spots ----------

fn element(s: &mut Session, id: u64) -> Value {
    let all = run(s, "elements", json!({}));
    all["elements"].as_array().and_then(|es| es.iter().find(|e| e["id"] == id).cloned()).unwrap_or(Value::Null)
}

/// A project whose Survey Point is at 760 m on the map, and whose Project Base Point stands 300 mm up.
fn surveyed() -> Session {
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Internal", "elevation": 760_000.0, "east": 333_000_000.0, "north": 7_394_000_000.0}));
    run(&mut s, "coordinates", json!({"base": [1000.0, 2000.0, 300.0]}));
    s
}

#[test]
fn spot_elevations_measure_from_the_base_point_or_the_survey_point() {
    let mut s = surveyed();
    let section = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
    let base = run(&mut s, "spot", json!({"at": [1000, 2700], "view": section}));
    assert_eq!(base["elevation"], 2400.0, "from the Project Base Point, 300 up: {base}");
    let survey = run(&mut s, "spot", json!({"at": [1000, 2700], "view": section, "origin": "survey"}));
    assert_eq!(survey["elevation"], 762_700.0, "{survey}");
    assert_eq!(element(&mut s, survey["id"].as_u64().unwrap())["elevation"], 762_700.0);
    // The survey moves it, the base point too; the spot follows.
    run(&mut s, "location", json!({"shift": [0, 0, 5000]}));
    assert_eq!(element(&mut s, survey["id"].as_u64().unwrap())["elevation"], 767_700.0);
    run(&mut s, "modify", json!({"id": base["id"], "origin": "survey"}));
    assert_eq!(element(&mut s, base["id"].as_u64().unwrap())["elevation"], 767_700.0);
    let e = fails(&mut s, "spot", json!({"at": [1000, 2700], "view": section, "origin": "moon"}));
    assert!(e.contains("base") && e.contains("survey"), "{e}");
}

#[test]
fn a_level_shows_its_elevation_from_its_elevation_base() {
    let mut s = surveyed();
    let l2 = s.model.levels().into_iter().nth(1).map(|(id, _)| id).unwrap();
    assert_eq!(element(&mut s, l2)["elevation_shown"], 2700.0, "3000 from a base point 300 up");
    run(&mut s, "modify", json!({"id": l2, "elevation_base": "survey"}));
    assert_eq!(element(&mut s, l2)["elevation_shown"], 763_000.0);
    assert_eq!(element(&mut s, l2)["elevation"], 3000.0, "the level itself stays where it is");
    let l3 = run(&mut s, "level", json!({"elevation": 6000, "elevation_base": "survey"}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, l3)["elevation_shown"], 766_000.0);
}

#[test]
fn spot_coordinates_read_north_and_east_from_either_point() {
    let mut s = surveyed();
    let base = run(&mut s, "spot_coordinate", json!({"at": [4000, 6000]}));
    assert_eq!((base["east"].as_f64(), base["north"].as_f64()), (Some(3000.0), Some(4000.0)), "{base}");
    let survey = run(&mut s, "spot_coordinate", json!({"at": [4000, 6000], "origin": "survey"}));
    assert_eq!((survey["east"].as_f64(), survey["north"].as_f64()), (Some(333_004_000.0), Some(7_394_006_000.0)), "{survey}");
    // True north turned: the map's numbers turn, the base point's don't.
    run(&mut s, "location", json!({"name": "Internal", "angle": 90}));
    let e = element(&mut s, survey["id"].as_u64().unwrap());
    assert!((e["east"].as_f64().unwrap() - (333_000_000.0 - 6000.0)).abs() < 0.01, "{e}");
    assert!((e["north"].as_f64().unwrap() - (7_394_000_000.0 + 4000.0)).abs() < 0.01, "{e}");
    assert_eq!(element(&mut s, base["id"].as_u64().unwrap())["east"], 3000.0);
    run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}));
    let e = fails(&mut s, "spot_coordinate", json!({"at": [0, 0], "view": "Section 1"}));
    assert!(e.contains("plan"), "{e}");
}

#[test]
fn schedules_list_where_columns_stand_in_shared_coordinates() {
    let mut s = surveyed();
    run(&mut s, "column", json!({"at": [4000, 6000]}));
    let r = run(
        &mut s,
        "schedule",
        json!({"name": "Setting out", "category": "columns", "fields": ["Shared East", "Shared North", "Shared Elevation"]}),
    );
    assert_eq!(r["rows"][0], json!([333_004_000.0, 7_394_006_000.0, 760_000.0]), "{r}");
    // A level schedule's elevation is what its head shows.
    let r = run(&mut s, "schedule", json!({"name": "Levels", "category": "levels"}));
    assert!(r["rows"].to_string().contains("2700"), "{r}");
}

// ---------- views: Rotate Project North ----------

fn wall_ends(s: &Session) -> Vec<[f64; 3]> {
    let co = &s.model.coordinates;
    s.model
        .elements
        .values()
        .filter_map(|e| match e {
            crate::model::Element::Wall(w) => Some([co.to_shared([w.start.0, w.start.1, 0.0]), co.to_shared([w.end.0, w.end.1, 0.0])]),
            _ => None,
        })
        .flatten()
        .collect()
}

fn same_place(a: &[[f64; 3]], b: &[[f64; 3]]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.iter().zip(q).all(|(x, y)| (x - y).abs() < 0.01))
}

#[test]
fn rotate_project_north_turns_the_model_and_keeps_it_where_it_is_in_the_world() {
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Internal", "east": 1_000_000.0, "north": 2_000_000.0, "angle": 30.0}));
    run(&mut s, "location", json!({"name": "North Lot", "east": 5_000_000.0, "north": 2_000_000.0, "angle": -20.0}));
    run(&mut s, "coordinates", json!({"base": [1000, 0, 0], "survey": [2000, 1000, 500]}));
    run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000]]}));
    let (walls, base) = (wall_ends(&s), s.model.coordinates.to_shared(s.model.coordinates.base));
    // Aligned with true north: project north is true north now, and the walls run along the map's axes.
    let r = run(&mut s, "rotate_project_north", json!({"true_north": true}));
    assert!((r["angle"].as_f64().unwrap() - 30.0).abs() < 1e-9, "{r}");
    let co = &s.model.coordinates;
    assert!(co.location().angle.abs() < 1e-9, "{:?}", co.location());
    assert!((co.locations["North Lot"].angle + 50.0).abs() < 1e-9);
    assert!(same_place(&wall_ends(&s), &walls), "{:?} {walls:?}", wall_ends(&s));
    assert!(same_place(&[co.to_shared(co.base)], &[base]));
    assert_eq!(co.survey[2], 500.0, "heights stay");
    // By an angle about a point, and back: one undo step each.
    run(&mut s, "rotate_project_north", json!({"angle": 15, "about": [3000, 3000]}));
    assert!((s.model.coordinates.location().angle + 15.0).abs() < 1e-9);
    assert!(same_place(&wall_ends(&s), &walls));
    run(&mut s, "undo", json!({}));
    assert!(s.model.coordinates.location().angle.abs() < 1e-9);
}

#[test]
fn rotate_project_north_needs_an_angle_or_true_north() {
    let mut s = Session::new();
    let e = fails(&mut s, "rotate_project_north", json!({}));
    assert!(e.contains("angle") && e.contains("true_north"), "{e}");
    fails(&mut s, "rotate_project_north", json!({"angle": f64::NAN}));
    fails(&mut s, "rotate_project_north", json!({"angle": 10, "true_north": true}));
}

#[test]
fn rotate_project_north_turns_the_whole_sample_house() {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    run(&mut s, "location", json!({"name": "Internal", "angle": 23.5}));
    let before = s.model.elements.len();
    let r = run(&mut s, "rotate_project_north", json!({"true_north": true}));
    let turned = r["rotated"].as_array().unwrap().len();
    assert_eq!(s.model.elements.len(), before);
    // All but the levels, sheets, schedules and 3D views, which have no place in plan of their own.
    use crate::model::Element as E;
    let fixed = s.model.elements.values().filter(|e| matches!(e, E::Level(_) | E::Sheet(_) | E::Schedule(_) | E::View3D(_))).count();
    assert_eq!(turned + fixed, before, "{turned} turned, {fixed} fixed");
    run(&mut s, "rotate_project_north", json!({"angle": -23.5}));
}

// ---------- views: the three origins, under Site in Visibility/Graphics ----------

/// How many pixels of the plan of level 100000 aren't paper within 12 px of model point `at`.
fn ink_near(s: &Session, at: (f64, f64)) -> usize {
    use crate::render::{View, draw, png};
    let view = View { scale: 0.05, cx: 0.0, cy: 0.0, width: 800, height: 600 };
    let img = tiny_skia::Pixmap::decode_png(&png(&draw(&s.model, 100000, view)).unwrap()).unwrap();
    let (x, y) = view.px(crate::geom::P2(at.0, at.1));
    let (x, y) = (x as i64, y as i64);
    (x - 12..=x + 12)
        .flat_map(|i| (y - 12..=y + 12).map(move |j| (i, j)))
        .filter_map(|(i, j)| img.pixel(u32::try_from(i).ok()?, u32::try_from(j).ok()?))
        .filter(|c| c.red() < 200 || c.green() < 200 || c.blue() < 200)
        .count()
}

#[test]
fn the_three_origins_are_site_subcategories_hidden_until_a_view_shows_them() {
    let mut s = Session::new();
    run(&mut s, "coordinates", json!({"base": [4000, 3000, 0], "survey": [-5000, 2000, 0]}));
    let points = [("internalorigin", (0.0, 0.0)), ("projectbasepoint", (4000.0, 3000.0)), ("surveypoint", (-5000.0, 2000.0))];
    for (_, at) in points {
        assert_eq!(ink_near(&s, at), 0, "hidden by default: nothing at {at:?}");
    }
    for (i, (key, at)) in points.iter().enumerate() {
        let r = run(&mut s, "graphics", json!({"view": 100000, "show": [key]}));
        assert_eq!(r["categories"][key]["shown"], true, "{r}");
        assert!(ink_near(&s, *at) > 20, "{key} drawn at {at:?}");
        // Only that one.
        for (other, at) in &points[i + 1..] {
            assert_eq!(ink_near(&s, *at), 0, "{other} still hidden");
        }
    }
    // Hiding the Site category hides all three.
    run(&mut s, "graphics", json!({"view": 100000, "hide": ["site"]}));
    for (_, at) in points {
        assert_eq!(ink_near(&s, at), 0);
    }
    let subs: Vec<&str> = crate::graphics::subcategories("site").map(|s| s.key).collect();
    assert_eq!(subs, ["internalorigin", "projectbasepoint", "surveypoint"]);
}

#[test]
fn every_location_and_both_points_report_their_id_and_guid_and_a_rename_keeps_them() {
    let mut s = Session::new();
    let made = run(&mut s, "location", json!({"name": "Lot B", "east": 1000.0}));
    let (id, guid) = (made["id"].as_u64().unwrap(), made["guid"].as_str().unwrap().to_string());
    assert!(id >= 900_000, "a library id: {made}");
    let r = run(&mut s, "coordinates", json!({}));
    assert_eq!(r["locations"]["Lot B"]["id"], id, "{r}");
    assert!(r["locations"]["Internal"]["id"].as_u64().is_some());
    assert!(r["base"]["id"].as_u64().is_some() && r["survey"]["guid"].as_str().is_some(), "{r}");
    run(&mut s, "location", json!({"name": "Lot B", "rename": "North Lot"}));
    let r = run(&mut s, "coordinates", json!({}));
    assert_eq!((r["locations"]["North Lot"]["id"].as_u64(), r["locations"]["North Lot"]["guid"].as_str()), (Some(id), Some(guid.as_str())));
}

#[test]
fn each_point_is_picked_by_its_library_id_where_a_view_shows_it() {
    use crate::coordinates::Point;
    use crate::render::{View, draw};
    let mut s = Session::new();
    run(&mut s, "coordinates", json!({"base": [4000, 3000, 0], "survey": [-5000, 2000, 0]}));
    run(&mut s, "graphics", json!({"view": 100000, "show": ["internalorigin", "projectbasepoint", "surveypoint"]}));
    let view = View { scale: 0.05, cx: 0.0, cy: 0.0, width: 800, height: 600 };
    let d = draw(&s.model, 100000, view);
    for (point, at) in [(Point::Internal, (0.0, 0.0)), (Point::Base, (4000.0, 3000.0)), (Point::Survey, (-5000.0, 2000.0))] {
        let id = point.id(&s.model).unwrap_or_else(|| panic!("{point:?} has a library id"));
        assert_eq!(Point::of(&s.model, id), Some(point));
        assert_eq!(d.pick(view.px(crate::geom::P2(at.0, at.1)), 6.0), Some(id), "{point:?} picked at its mark");
    }
    assert_eq!(Point::of(&s.model, 100000), None, "a level is no point");
}

#[test]
fn a_section_shows_the_points_where_its_graphics_show_them_and_picks_them() {
    use crate::coordinates::Point;
    use crate::render::{View, png, section};
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 3000], [8000, 3000]]}));
    run(&mut s, "coordinates", json!({"base": [4000, 0, 1000], "survey": [6000, 0, -500]}));
    // Looking north along y = -2000, from x = -2000 to 10000.
    let id = run(&mut s, "section", json!({"start": [-2000, -2000], "end": [10000, -2000]}))["id"].as_u64().unwrap();
    let view = View { scale: 0.05, cx: 4000.0, cy: 0.0, width: 800, height: 400 };
    // Along the line from its start, and up.
    let points = [(Point::Internal, (2000.0, 0.0)), (Point::Base, (6000.0, 1000.0)), (Point::Survey, (8000.0, -500.0))];
    let ink = |s: &Session, at: (f64, f64)| {
        let img = tiny_skia::Pixmap::decode_png(&png(&section(&s.model, id, view)).unwrap()).unwrap();
        let (x, y) = view.px(crate::geom::P2(at.0, at.1));
        let (x, y) = (x as i64, y as i64);
        (x - 10..=x + 10)
            .flat_map(|i| (y - 10..=y + 10).map(move |j| (i, j)))
            .filter_map(|(i, j)| img.pixel(u32::try_from(i).ok()?, u32::try_from(j).ok()?))
            .map(|c| u32::from(c.red()) + u32::from(c.green()) + u32::from(c.blue()))
            .sum::<u32>()
    };
    let hidden: Vec<u32> = points.iter().map(|(_, at)| ink(&s, *at)).collect();
    run(&mut s, "graphics", json!({"view": id, "show": ["internalorigin", "projectbasepoint", "surveypoint"]}));
    let d = section(&s.model, id, view);
    for ((p, at), before) in points.iter().zip(hidden) {
        assert!(ink(&s, *at) + 2000 < before, "{p:?} drawn at {at:?}: darker than without it");
        assert_eq!(d.pick(view.px(crate::geom::P2(at.0, at.1)), 6.0), p.id(&s.model), "{p:?} picked");
    }
}
