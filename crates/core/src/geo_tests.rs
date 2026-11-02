//! `fetch_osm` and `fetch_terrain`: the commands that ask an app's fetcher, driven with a fake one (no test reaches the network).

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geo::{Bbox, Elevation, ElevationGrid, GeoSource, Reply};
use crate::model::Element;

const HOME: (f64, f64) = (-46.6333, -23.5505);

/// The height, metres, the fake's ground has at a place: 760 at home, 1 m more each 100 m east, 2 m more each 100 m north.
fn ground(lon: f64, lat: f64) -> f64 {
    760.0 + 1000.0 * (lon - HOME.0) + 2000.0 * (lat - HOME.1)
}

/// A fetcher that answers from what it holds and notes what it was asked.
struct Fake {
    osm: String,
    asked: Mutex<Vec<(Bbox, String)>>,
    heights: Mutex<Vec<(Bbox, f64, Option<String>)>>,
    fail: Option<String>,
    /// How many nodes without a height the grid has, from the north west corner.
    holes: usize,
}

impl Fake {
    fn new(osm: String) -> Fake {
        Fake { osm, asked: Mutex::default(), heights: Mutex::default(), fail: None, holes: 0 }
    }
}

impl GeoSource for Fake {
    fn overpass(&self, bbox: &Bbox, query: &str) -> Result<Reply, String> {
        self.asked.lock().unwrap().push((*bbox, query.to_string()));
        match &self.fail {
            Some(why) => Err(why.clone()),
            None => Ok(Reply { text: self.osm.clone(), cached: true }),
        }
    }

    fn elevation(&self, bbox: &Bbox, spacing: f64, source: Option<&str>) -> Result<Elevation, String> {
        self.heights.lock().unwrap().push((*bbox, spacing, source.map(str::to_string)));
        if let Some(why) = &self.fail {
            return Err(why.clone());
        }
        // A grid a little wider than the box, a node every 0.0005 degrees.
        let step = 0.0005;
        let (west, north) = (bbox.west - 2.0 * step, bbox.north + 2.0 * step);
        let cols = ((bbox.east - west) / step).ceil() as usize + 3;
        let rows = ((north - bbox.south) / step).ceil() as usize + 3;
        let mut values: Vec<f32> =
            (0..rows).flat_map(|r| (0..cols).map(move |c| ground(west + c as f64 * step, north - r as f64 * step) as f32)).collect();
        for v in values.iter_mut().take(self.holes) {
            *v = f32::NAN;
        }
        Ok(Elevation {
            grid: ElevationGrid::new(west, north, step, step, cols, rows, values)?,
            source: source.unwrap_or("terrarium").to_string(),
            cached: false,
        })
    }

    fn attribution(&self, what: &str) -> String {
        match what {
            "osm" => "Data (c) OpenStreetMap contributors (ODbL)".into(),
            other => format!("Elevation: {other}"),
        }
    }
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn refused(s: &mut Session, id: &str, p: Value) -> String {
    s.execute(id, &p).expect_err(&format!("{id} {p} should be refused"))
}

fn square(east: f64, north: f64, size: f64) -> Vec<(f64, f64)> {
    let (lon, lat) = (HOME.0 + east, HOME.1 + north);
    let (dx, dy) = (size * 0.000_01, size * 0.000_009);
    vec![(lon, lat), (lon + dx, lat), (lon + dx, lat + dy), (lon, lat + dy), (lon, lat)]
}

fn way(id: i64, points: &[(f64, f64)], tags: &str) -> String {
    let g: Vec<String> = points.iter().map(|(lon, lat)| format!(r#"{{"lat":{lat},"lon":{lon}}}"#)).collect();
    format!(r#"{{"type":"way","id":{id},"geometry":[{}],"tags":{{{tags}}}}}"#, g.join(","))
}

fn overpass_reply() -> String {
    let houses = [
        way(1, &square(0.0, 0.0, 20.0), r#""building":"house","height":"12""#),
        way(2, &square(0.0003, 0.0002, 30.0), r#""building":"yes","building:levels":"3""#),
    ];
    format!(r#"{{"version":0.6,"elements":[{}]}}"#, houses.join(","))
}

/// A project placed at `HOME`, with `fake` attached.
fn session(fake: Arc<Fake>) -> Session {
    let mut s = Session::new();
    run(&mut s, "geolocate", json!({"latitude": HOME.1, "longitude": HOME.0}));
    s.geo = Some(fake);
    s
}

fn masses(s: &Session) -> usize {
    s.model.elements.values().filter(|e| matches!(e, Element::Mass(_))).count()
}

#[test]
fn fetch_osm_asks_for_the_box_and_makes_the_buildings_that_come() {
    let fake = Arc::new(Fake::new(overpass_reply()));
    let mut s = session(fake.clone());
    let r = run(&mut s, "fetch_osm", json!({"radius": 500}));
    assert_eq!((r["made"].clone(), r["fetched"]["cached"].clone()), (json!(2), json!(true)), "{r}");
    assert_eq!(masses(&s), 2);
    // It asked once, for a box of 1 km by 1 km round the origin, with the query the pure function writes.
    let asked = fake.asked.lock().unwrap();
    assert_eq!(asked.len(), 1);
    let (bbox, query) = &asked[0];
    assert!(bbox.contains(HOME.0, HOME.1) && (bbox.area_km2() - 1.0).abs() < 0.02, "{bbox:?}");
    assert_eq!(*query, crate::geo::overpass_query(bbox, crate::geo::Layers::default()));
    assert_eq!(r["fetched"]["bytes"], overpass_reply().len());
    assert_eq!(r["fetched"]["bbox"], json!(bbox.as_array()));
    assert!(r["attribution"].as_str().unwrap().contains("OpenStreetMap"));
    // One undo step takes them all back.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(masses(&s), 0);
}

#[test]
fn fetch_osm_takes_a_box_or_a_radius_and_passes_the_import_options_on() {
    let fake = Arc::new(Fake::new(overpass_reply()));
    let mut s = session(fake.clone());
    run(&mut s, "fetch_osm", json!({"bbox": [-23.56, -46.64, -23.54, -46.62], "parts": true, "level_height": 2800, "detail": ["way/1"]}));
    {
        let asked = fake.asked.lock().unwrap();
        assert_eq!(asked[0].0.as_array(), [-23.56, -46.64, -23.54, -46.62]);
        assert!(asked[0].1.contains("building:part"), "parts are asked for: {}", asked[0].1);
    }
    let walls = s.model.elements.values().filter(|e| e.category() == "wall").count();
    assert_eq!(walls, 4, "detail went to the import");
    let heights: Vec<f64> =
        s.model.elements.values().filter_map(|e| if let Element::Mass(m) = e { Some(m.height) } else { None }).collect();
    assert!(heights.contains(&12000.0) && heights.contains(&8400.0), "3 levels of 2800: {heights:?}");
    // No box and no radius: 1 km round the origin.
    let mut t = session(fake.clone());
    run(&mut t, "fetch_osm", json!({}));
    let asked = fake.asked.lock().unwrap();
    assert!((asked.last().unwrap().0.area_km2() - 4.0).abs() < 0.05);
}

#[test]
fn fetch_osm_is_refused_without_a_fetcher_over_too_big_a_box_and_on_a_bad_reply() {
    let mut s = Session::new();
    run(&mut s, "geolocate", json!({"latitude": HOME.1, "longitude": HOME.0}));
    assert!(refused(&mut s, "fetch_osm", json!({})).contains("no network access"));
    let fake = Arc::new(Fake::new(overpass_reply()));
    let mut s = session(fake.clone());
    for (p, says) in [
        (json!({"radius": 4000}), "25"),
        (json!({"bbox": [-24.0, -47.0, -23.0, -46.0]}), "km²"),
        (json!({"bbox": [1, 2, 3]}), "bbox"),
        (json!({"bbox": [-23.56, -46.64, -23.54, -46.62], "radius": 500}), "not both"),
        (json!({"radius": -1}), "radius"),
        (json!({"radius": "far"}), "radius"),
        (json!({"colour": 1}), "colour"),
        (json!({"detail": "all"}), "detail"),
    ] {
        assert!(refused(&mut s, "fetch_osm", p.clone()).contains(says), "{p}");
    }
    assert!(fake.asked.lock().unwrap().is_empty(), "nothing was asked for what was refused");
    // The fetcher failing, and answering with what is not OSM.
    let failing = Arc::new(Fake { fail: Some("overpass-api.de: 504 Gateway Timeout".into()), ..Fake::new(String::new()) });
    let mut t = session(failing);
    assert!(refused(&mut t, "fetch_osm", json!({})).contains("504"));
    let junk = Arc::new(Fake::new("<html>Too many requests</html>".into()));
    let mut u = session(junk);
    assert!(refused(&mut u, "fetch_osm", json!({})).contains("osm"));
    assert_eq!(masses(&u), 0);
    // Not placed: the box has no centre to be round.
    let mut v = Session::new();
    v.geo = Some(fake);
    assert!(refused(&mut v, "fetch_osm", json!({})).contains("no location"));
    assert!(
        v.execute("fetch_osm", &json!({"bbox": [-23.56, -46.64, -23.54, -46.62]})).unwrap_err().contains("no location"),
        "it cannot place them either"
    );
}
