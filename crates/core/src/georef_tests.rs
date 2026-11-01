//! The `geolocate` command: placing a project on Earth, through `Session::execute` as the CLI and MCP do.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::Model;

fn run(s: &mut Session, p: Value) -> Value {
    s.execute("geolocate", &p).unwrap_or_else(|e| panic!("geolocate {p}: {e}"))
}

fn refused(s: &mut Session, p: Value) -> String {
    s.execute("geolocate", &p).expect_err(&format!("geolocate {p} should be refused"))
}

#[test]
fn a_project_is_unplaced_until_it_is_given_a_latitude_and_longitude() {
    let mut s = Session::new();
    assert_eq!(run(&mut s, json!({})), json!({"located": false}));
    let r = run(&mut s, json!({"latitude": -23.5505, "longitude": -46.6333, "elevation": 760000, "true_north": 12.5}));
    assert_eq!((r["located"].clone(), r["elevation"].clone(), r["true_north"].clone()), (json!(true), json!(760000.0), json!(12.5)));
    assert_eq!(r["crs"]["epsg"], 32723);
    assert!((r["crs"]["easting"].as_f64().unwrap() - 333287.915).abs() < 1e-2);
    assert!((r["crs"]["northing"].as_f64().unwrap() - 7394588.319).abs() < 1e-2);
    assert!((r["crs"]["convergence"].as_f64().unwrap() - 0.65275).abs() < 1e-4);
    assert_eq!(run(&mut s, json!({}))["latitude"], -23.5505, "asking again changes nothing");
    // inspect says where it is.
    let i = s.execute("inspect", &json!({})).unwrap();
    assert_eq!(i["location"]["longitude"], -46.6333);
}

#[test]
fn a_location_can_be_changed_a_part_at_a_time_and_cleared() {
    let mut s = Session::new();
    run(&mut s, json!({"latitude": 48.8584, "longitude": 2.2945}));
    let r = run(&mut s, json!({"true_north": 30}));
    assert_eq!((r["latitude"].clone(), r["true_north"].clone()), (json!(48.8584), json!(30.0)));
    let r = run(&mut s, json!({"crs": 2154}));
    assert_eq!(r["crs"]["epsg"], 2154);
    assert!(r["crs"]["name"].as_str().unwrap().contains("Lambert-93"), "{}", r["crs"]["name"]);
    let r = run(&mut s, json!({"crs": "utm"}));
    assert_eq!(r["crs"]["epsg"], 32631);
    let r = run(&mut s, json!({"crs": "EPSG:32632"}));
    assert_eq!(r["crs"]["epsg"], 32632);
    assert_eq!(run(&mut s, json!({"clear": true})), json!({"located": false}));
    assert!(s.model.location.is_none());
}

#[test]
fn it_is_undone_and_saved_with_the_project() {
    let mut s = Session::new();
    run(&mut s, json!({"latitude": 63.4305, "longitude": 10.3951, "crs": 25832}));
    let text = serde_json::to_string(&*s.model).unwrap();
    let (m, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty());
    assert_eq!(m.location, s.model.location);
    assert_eq!(m.location.unwrap().crs, Some(25832));
    s.execute("undo", &json!({})).unwrap();
    assert!(s.model.location.is_none());
    // A project without one writes no key, so an older version reads it as before.
    assert!(!serde_json::to_string(&*s.model).unwrap().contains("\"location\""));
}

#[test]
fn points_go_to_degrees_and_back() {
    let mut s = Session::new();
    run(&mut s, json!({"latitude": -23.5505, "longitude": -46.6333, "true_north": 20}));
    let at = run(&mut s, json!({"point": [120000, -45000]}))["lonlat"].clone();
    let (lon, lat) = (at[0].as_f64().unwrap(), at[1].as_f64().unwrap());
    let back = run(&mut s, json!({"lonlat": [lon, lat]}))["point"].clone();
    assert!((back[0].as_f64().unwrap() - 120000.0).abs() < 1.0 && (back[1].as_f64().unwrap() + 45000.0).abs() < 1.0, "{back}");
    // Both at once, and the project is unchanged.
    let both = run(&mut s, json!({"point": [0, 0], "lonlat": [-46.6333, -23.5505]}));
    assert!((both["lonlat"][0].as_f64().unwrap() + 46.6333).abs() < 1e-9 && both["point"][0].as_f64().unwrap().abs() < 1.0);
}

#[test]
fn what_cannot_be_placed_is_refused_and_the_project_stays_as_it_was() {
    let mut s = Session::new();
    assert!(refused(&mut s, json!({"latitude": 10})).contains("both needed"));
    assert!(refused(&mut s, json!({"point": [0, 0]})).contains("no location"));
    assert!(refused(&mut s, json!({"latitude": 91, "longitude": 0})).contains("latitude"));
    assert!(refused(&mut s, json!({"latitude": 10, "longitude": 200})).contains("longitude"));
    assert!(refused(&mut s, json!({"latitude": 10, "longitude": 0, "crs": 4326})).contains("projected"));
    assert!(refused(&mut s, json!({"latitude": 10, "longitude": 0, "crs": "nope"})).contains("EPSG code"));
    assert!(refused(&mut s, json!({"latitude": 10, "longitude": 0, "colour": 1})).contains("colour"));
    assert!(s.model.location.is_none());
    run(&mut s, json!({"latitude": 10, "longitude": 0}));
    assert!(refused(&mut s, json!({"true_north": 1e9})).contains("true_north"));
    assert!(refused(&mut s, json!({"lonlat": [0, "x"]})).contains("lonlat"));
    assert_eq!(s.model.location.unwrap().true_north, 0.0);
}
