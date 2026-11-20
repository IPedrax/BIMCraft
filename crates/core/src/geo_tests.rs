//! `fetch_osm`: the command that asks an app's fetcher, driven with a fake one (no test reaches the network).

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geo::{Bbox, GeoSource, Reply};
use crate::model::Element;

const HOME: (f64, f64) = (-46.6333, -23.5505);

/// A fetcher that answers from what it holds and notes what it was asked.
struct Fake {
    osm: String,
    asked: Mutex<Vec<(Bbox, String)>>,
    fail: Option<String>,
}

impl Fake {
    fn new(osm: String) -> Fake {
        Fake { osm, asked: Mutex::default(), fail: None }
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

    fn attribution(&self, what: &str) -> String {
        match what {
            "osm" => "Data (c) OpenStreetMap contributors (ODbL)".into(),
            other => format!("Credit: {other}"),
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

/// `s`'s site put at (`lon`, `lat`), on WGS 84's UTM zone there.
pub(crate) fn locate(s: &mut Session, lon: f64, lat: f64) {
    let name = s.model.coordinates.active.clone();
    let crs = format!("EPSG:{}", crate::geo::utm_epsg(lon, lat));
    run(s, "location", json!({"name": name, "crs": crs, "latitude": lat, "longitude": lon}));
}

/// A project placed at `HOME`, with `fake` attached.
fn session(fake: Arc<Fake>) -> Session {
    let mut s = Session::new();
    locate(&mut s, HOME.0, HOME.1);
    s.geo = Some(fake);
    s
}

/// A fetcher for the hostile-parameters sweep: it answers, so the commands go all the way through.
pub(crate) fn sweep_fetcher() -> crate::geo::Fetcher {
    Arc::new(Fake::new(overpass_reply()))
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
    locate(&mut s, HOME.0, HOME.1);
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
    assert!(refused(&mut v, "fetch_osm", json!({})).contains("no place on Earth"));
    assert!(
        v.execute("fetch_osm", &json!({"bbox": [-23.56, -46.64, -23.54, -46.62]})).unwrap_err().contains("no place on Earth"),
        "it cannot place them either"
    );
}

#[test]
fn a_prefetched_reply_runs_the_unchanged_fetch_osm_without_asking_the_real_fetcher_again() {
    let real = Arc::new(Fake::new(overpass_reply()));
    let mut s = session(real.clone());
    let (lon, lat) = crate::geo::Projector::of(&s.model).unwrap().origin().unwrap();
    let bbox = Bbox::around(lon, lat, 500.0).unwrap();
    // The thread's part: ask the real fetcher once, with the query the command would write.
    let query = crate::geo::overpass_query(&bbox, crate::geo::Layers::default());
    let reply = real.overpass(&bbox, &query).unwrap();
    assert_eq!(real.asked.lock().unwrap().len(), 1);
    // The UI thread's part: the command, over what was fetched.
    let real_dyn: crate::geo::Fetcher = real.clone();
    s.geo = Some(Arc::new(crate::geo::Prefetched::new(real_dyn, bbox).with_osm(query, reply)));
    let r = run(&mut s, "fetch_osm", json!({"radius": 500}));
    assert_eq!(r["made"], json!(2), "{r}");
    assert_eq!(real.asked.lock().unwrap().len(), 1, "the real fetcher was not asked again");
    assert!(r["attribution"].as_str().unwrap().contains("OpenStreetMap"), "the credit is the real source's: {r}");
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(masses(&s), 0, "one undo step, as ever");
}

#[test]
fn fetch_osm_reports_the_query_it_asked_with() {
    for parts in [false, true] {
        let fake = Arc::new(Fake::new(overpass_reply()));
        let mut s = session(fake.clone());
        let r = run(&mut s, "fetch_osm", json!({"radius": 300, "parts": parts}));
        let asked = fake.asked.lock().unwrap();
        let (bbox, query) = &asked[0];
        assert_eq!(r["fetched"]["query"], json!(query), "parts {parts}");
        assert_eq!(*query, crate::geo::overpass_query(bbox, crate::geo::Layers { parts, entrances: true }));
        assert_eq!(query.contains("building:part"), parts);
    }
}

#[test]
fn a_fetcher_with_no_map_tiles_says_so_and_a_tile_is_checked() {
    use crate::geo::Tile;
    let fake = Fake::new(String::new());
    assert!(fake.tile(3, 1, 2).unwrap_err().contains("no map tiles"));
    let good = Tile { rgba: vec![0; 4 * 256 * 256], side: 256, cached: false };
    assert!(good.check().is_ok());
    for bad in [
        Tile { rgba: vec![0; 4 * 255 * 256], side: 256, cached: false },
        Tile { rgba: vec![], side: 0, cached: false },
        Tile { rgba: vec![0; 4 * 513 * 513], side: 513, cached: false },
        Tile { rgba: vec![0; 3], side: 1, cached: false },
    ] {
        assert!(bad.check().is_err(), "side {} of {} bytes", bad.side, bad.rgba.len());
    }
    // What holds a fetch's results hands the tiles on to the real fetcher.
    let real: crate::geo::Fetcher = Arc::new(TileFake);
    let held = crate::geo::Prefetched::new(real, Bbox::new(-23.56, -46.64, -23.54, -46.62).unwrap());
    assert_eq!(held.tile(1, 0, 0).unwrap().side, 1);
}

/// A fetcher that has one tile, a pixel.
struct TileFake;

impl GeoSource for TileFake {
    fn overpass(&self, _: &Bbox, _: &str) -> Result<Reply, String> {
        Err("none".into())
    }
    fn attribution(&self, _: &str) -> String {
        String::new()
    }
    fn tile(&self, _: u8, _: u32, _: u32) -> Result<crate::geo::Tile, String> {
        Ok(crate::geo::Tile { rgba: vec![1, 2, 3, 255], side: 1, cached: true })
    }
}
