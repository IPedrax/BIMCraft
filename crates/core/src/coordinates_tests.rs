//! Coordinates: the Project Base Point, the Survey Point and the project's sites, the georeference the active one, through
//! the commands every front end drives ([the area](../../../docs/areas/coordinates.md)).

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

/// Site `name` as `coordinates` reports it: the Survey Point's numbers in it.
fn loc(s: &mut Session, name: &str) -> Value {
    run(s, "coordinates", json!({}))["locations"][name].clone()
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
    assert_eq!(loc(&mut s, "Internal")["elevation"], 105_000.0);
    assert_eq!(loc(&mut s, "B")["elevation"], 125_000.0);
    // Or just one.
    run(&mut s, "location", json!({"name": "B", "shift": [1000.0, 0.0, 0.0]}));
    assert_eq!(loc(&mut s, "B")["east"], 1000.0);
    assert_eq!(loc(&mut s, "Internal")["east"], 0.0);
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
    assert_eq!(loc(&mut s, "Internal")["angle"], 30.0, "the angle stays unless given");
    run(&mut s, "specify_coordinates", json!({"at": [4000.0, 2000.0, 0.0], "east": 1.0, "north": 2.0, "elevation": 3.0, "angle": -12.5}));
    assert!(close(&convert(&mut s, "internal", "shared", [4000.0, 2000.0, 0.0]), [1.0, 2.0, 3.0]));
    assert_eq!(loc(&mut s, "Internal")["angle"], -12.5);
}

#[test]
fn coordinates_are_one_undo_step() {
    let mut s = Session::new();
    let before = (s.model.coordinates.clone(), s.model.georeference.clone());
    run(&mut s, "location", json!({"name": "Internal", "east": 1.0}));
    run(&mut s, "undo", json!({}));
    assert_eq!((s.model.coordinates.clone(), s.model.georeference.clone()), before);
}

#[test]
fn bad_coordinates_are_refused_with_what_to_fix() {
    let mut s = Session::new();
    for (p, says) in [
        (json!({"name": "Internal", "scale": 2e6}), "scale"),
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
    // A file's numbers read back to the last digit or so.
    let rounded = |g: &crate::model::Georeference| {
        let r = |v: f64| (v * 1e9).round() / 1e9;
        (r(g.eastings), r(g.northings), r(g.height), g.rotation, g.scale)
    };
    assert_eq!(
        (&m.coordinates.base, &m.coordinates.survey, &m.coordinates.active),
        (&s.model.coordinates.base, &s.model.coordinates.survey, &s.model.coordinates.active)
    );
    assert_eq!(
        m.coordinates.sites.values().map(rounded).collect::<Vec<_>>(),
        s.model.coordinates.sites.values().map(rounded).collect::<Vec<_>>()
    );
    assert_eq!(m.georeference.as_ref().map(rounded), s.model.georeference.as_ref().map(rounded));
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

#[test]
fn the_active_site_is_the_georeference_at_the_internal_origin() {
    let mut s = Session::new();
    run(&mut s, "coordinates", json!({"survey": [3000.0, 4000.0, 500.0]}));
    // Revit's numbers are the Survey Point's; the georeference, as IFC writes it, is the internal origin's.
    run(&mut s, "location", json!({"name": "Internal", "east": 1_000_000.0, "north": 2_000_000.0, "elevation": 10_000.0, "angle": 90.0}));
    let g = s.model.georeference.clone().unwrap();
    assert_eq!(g.rotation, 90.0);
    // Project x is the map's north at 90 degrees: the survey's 3 m east and 4 m north are 4 m west and 3 m north on the map.
    assert!(close(&json!([g.eastings, g.northings, g.height]), [1_000_000.0 + 4000.0, 2_000_000.0 - 3000.0, 9_500.0]), "{g:?}");
    assert!(g.crs.is_empty(), "a site needs no coordinate reference system");
    // The georeference command and the location say the same site.
    let r = run(&mut s, "georeference", json!({}));
    assert_eq!(r["georeference"]["eastings"], json!(g.eastings));
}

#[test]
fn moving_the_survey_point_alone_changes_only_it() {
    let mut s = Session::new();
    run(&mut s, "georeference", json!({"crs": "EPSG:31983", "eastings": 333_000_000.0, "rotation": 12.0}));
    run(&mut s, "location", json!({"name": "Other", "east": 5.0}));
    let (g, sites) = (s.model.georeference.clone(), s.model.coordinates.sites.clone());
    run(&mut s, "coordinates", json!({"survey": [3000.0, 4000.0, 0.0]}));
    assert_eq!((&s.model.georeference, &s.model.coordinates.sites), (&g, &sites));
    // With the system, every site is solved again, so the point keeps its numbers in each.
    let before = (loc(&mut s, "Internal"), loc(&mut s, "Other"));
    run(&mut s, "coordinates", json!({"survey": [-1000.0, 2500.0, 700.0], "move": "system"}));
    let (a, b) = (loc(&mut s, "Internal"), loc(&mut s, "Other"));
    for (was, now) in [(&before.0, &a), (&before.1, &b)] {
        for k in ["east", "north", "elevation"] {
            assert!((was[k].as_f64().unwrap() - now[k].as_f64().unwrap()).abs() < 1e-6, "{k}: {was} {now}");
        }
    }
    assert_ne!(s.model.georeference, g);
}

#[test]
fn activating_a_site_swaps_it_in_as_the_georeference() {
    let mut s = Session::new();
    run(&mut s, "georeference", json!({"crs": "EPSG:31983", "eastings": 333_000_000.0, "northings": 7_400_000_000.0}));
    run(&mut s, "location", json!({"name": "Plot 2", "east": 500.0, "north": 600.0, "angle": 0.0}));
    assert_eq!(s.model.georeference.as_ref().map(|g| g.eastings), Some(333_000_000.0), "a new site isn't active until asked");
    assert_eq!(s.model.coordinates.sites["Plot 2"].crs, "EPSG:31983", "a new site starts as a copy of the active one");
    let was = s.model.georeference.clone();
    run(&mut s, "location", json!({"name": "Plot 2", "activate": true}));
    assert_eq!(s.model.coordinates.active, "Plot 2");
    assert_eq!(s.model.georeference.as_ref().map(|g| (g.eastings, g.northings)), Some((500.0, 600.0)));
    assert_eq!(s.model.coordinates.sites.get("Internal"), was.as_ref(), "the one that was active is kept beside it");
    assert!(close(&convert(&mut s, "internal", "shared", [0.0; 3]), [500.0, 600.0, 0.0]));
    // Converting through another site leaves the active one be.
    let r = run(&mut s, "convert_coordinates", json!({"points": [[0, 0, 0]], "from": "internal", "to": "shared", "location": "Internal"}));
    assert!(close(&r["points"][0], [333_000_000.0, 7_400_000_000.0, 0.0]), "{r}");
    assert_eq!(s.model.check(), Ok(()));
    // Every site is a library entry with an id and a GUID, and so are the three points.
    let r = run(&mut s, "coordinates", json!({}));
    for name in ["Internal", "Plot 2"] {
        assert!(r["locations"][name]["id"].is_u64() && r["locations"][name]["guid"].is_string(), "{name}: {r}");
    }
    for key in ["internal_origin", "base", "survey"] {
        assert!(r[key]["id"].is_u64() && r[key]["guid"].is_string(), "{key}: {r}");
    }
}

#[test]
fn a_georeference_needs_no_coordinate_reference_system() {
    let mut s = Session::new();
    // True north alone makes one.
    let r = run(&mut s, "georeference", json!({"rotation": 15.0}));
    assert_eq!(r["georeference"]["rotation"], 15.0);
    assert!(r["georeference"].get("crs").is_none(), "{r}");
    assert_eq!(s.model.check(), Ok(()));
    let text = s.model.to_json().unwrap();
    let (m, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.georeference, s.model.georeference);
}

#[test]
fn coordinates_share_0_12s_format_20_and_a_file_of_19_needs_no_step() {
    assert_eq!(crate::model::FORMAT, 20);
    let mut s = Session::new();
    run(&mut s, "georeference", json!({"crs": "EPSG:31983", "eastings": 1000.0}));
    let mut v: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    v["format"] = json!(19);
    v.as_object_mut().unwrap().remove("coordinates");
    let (m, left_out) = Model::read(&v.to_string()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.georeference, s.model.georeference, "the georeference is the active site");
    assert_eq!(m.coordinates.survey, [0.0; 3]);
    assert_eq!(m.format, 20);
}

#[test]
fn a_file_of_284s_coordinates_lists_them_as_left_out() {
    // #284 was never released: its locations and map system (FORMAT 19's builds of that branch) aren't read, and say so.
    let s = Session::new();
    let mut v: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    v["format"] = json!(19);
    v["coordinates"] = json!({
        "base": [1.0, 2.0, 3.0], "survey": [4.0, 5.0, 6.0], "active": "Internal",
        "locations": {"Internal": {"east": 7.0, "north": 8.0, "elevation": 9.0, "angle": 10.0}}, "crs": 31983
    });
    let (m, left_out) = Model::read(&v.to_string()).unwrap();
    assert!(left_out.iter().any(|p| p.starts_with("/coordinates/locations")), "{left_out:?}");
    assert!(left_out.iter().any(|p| p == "/coordinates/crs"), "{left_out:?}");
    assert_eq!(m.check(), Ok(()));
}

#[test]
fn a_project_with_no_georeference_keeps_none_when_its_site_is_switched_back() {
    let mut s = Session::new();
    run(&mut s, "location", json!({"name": "Plot 2", "east": 500.0}));
    run(&mut s, "location", json!({"name": "Plot 2", "activate": true}));
    assert_eq!(s.model.georeference.as_ref().map(|g| g.eastings), Some(500.0));
    assert_eq!(s.model.coordinates.sites.get("Internal"), Some(&crate::coordinates::NONE));
    run(&mut s, "location", json!({"name": "Internal", "activate": true}));
    assert_eq!(s.model.georeference, None, "the internal coordinates as they are");
    assert_eq!(s.model.coordinates.sites["Plot 2"].eastings, 500.0);
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
