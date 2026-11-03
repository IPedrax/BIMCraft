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

fn heights(s: &mut Session, points: Value) -> Vec<Option<f64>> {
    run(s, "height_at", json!({"points": points}))["heights"].as_array().unwrap().iter().map(Value::as_f64).collect()
}

fn terrain_count(s: &Session) -> usize {
    s.model.elements.values().filter(|e| e.category() == "toposurface").count()
}

#[test]
fn fetch_terrain_makes_the_ground_with_the_origin_at_its_own_height() {
    let fake = Arc::new(Fake::new(String::new()));
    let mut s = session(fake.clone());
    let r = run(&mut s, "fetch_terrain", json!({"radius": 500, "spacing": 10, "name": "Ground", "interval": 500}));
    assert_eq!(r["source"], "terrarium", "{r}");
    // The project had no height above the sea: the ground at its origin (760 m) becomes it, so project z = 0 is there.
    assert!((r["elevation_set"].as_f64().unwrap() - 760_000.0).abs() < 1.0, "{r}");
    let place = run(&mut s, "geolocate", json!({}));
    assert!((place["elevation"].as_f64().unwrap() - 760_000.0).abs() < 1.0);
    let at = heights(&mut s, json!([[0, 0], [100_000, 0], [0, 100_000], [-100_000, -100_000]]));
    // 100 m east is 0.00098 degrees: 0.98 m more; 100 m north is 0.000898 degrees: 1.8 m more (the project's axes are a little
    // out of line with the degrees, hence the slack).
    for (got, want) in at.iter().zip([0.0, 980.0, 1800.0, -2780.0]) {
        assert!(got.is_some_and(|z| (z - want).abs() < 40.0), "{at:?}");
    }
    // The box is the terrain: about a square kilometre, and nothing past it.
    assert!((r["area_m2"].as_f64().unwrap() - 1.0e6).abs() < 0.06e6, "{r}");
    assert_eq!(heights(&mut s, json!([[800_000, 0], [0, -800_000]])), [None, None]);
    // The hook was asked once, for the box and the spacing, and the reply carries its credit.
    let asked = fake.heights.lock().unwrap();
    assert_eq!(asked.len(), 1);
    assert!((asked[0].0.area_km2() - 1.0).abs() < 0.02 && asked[0].1 == 10.0 && asked[0].2.is_none());
    assert_eq!(r["attribution"], "Elevation: terrarium");
    let listed = run(&mut s, "elements", json!({"category": "toposurface"}));
    assert_eq!((listed["elements"][0]["name"].clone(), listed["elements"][0]["points"].clone()), (json!("Ground"), r["points"].clone()));
    // One step: the terrain and the height above the sea come back together.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!((terrain_count(&s), run(&mut s, "geolocate", json!({}))["elevation"].clone()), (0, json!(0.0)));
}

#[test]
fn a_height_the_project_already_has_is_kept_and_asked_not_to_change_it_is_not_changed() {
    let fake = Arc::new(Fake::new(String::new()));
    let mut s = session(fake.clone());
    run(&mut s, "geolocate", json!({"elevation": 700_000}));
    let r = run(&mut s, "fetch_terrain", json!({"radius": 300, "spacing": 20}));
    assert!(r["elevation_set"].is_null(), "{r}");
    assert!(heights(&mut s, json!([[0, 0]]))[0].is_some_and(|z| (z - 60_000.0).abs() < 120.0), "760 m ground over a 700 m origin");
    let mut t = session(fake);
    let r = run(&mut t, "fetch_terrain", json!({"radius": 300, "spacing": 20, "set_elevation": false}));
    assert!(r["elevation_set"].is_null());
    assert!(heights(&mut t, json!([[0, 0]]))[0].is_some_and(|z| (z - 760_000.0).abs() < 120.0), "heights as they come");
    assert_eq!(run(&mut t, "geolocate", json!({}))["elevation"], 0.0);
}

#[test]
fn the_default_spacing_keeps_the_points_under_the_cap_and_a_fine_one_is_refused() {
    let fake = Arc::new(Fake::new(String::new()));
    let mut s = session(fake.clone());
    let r = run(&mut s, "fetch_terrain", json!({"radius": 2000}));
    let n = r["points"].as_u64().unwrap();
    assert!((10_000..=50_000).contains(&n), "{n}");
    let asked = fake.heights.lock().unwrap().len();
    assert!(refused(&mut s, "fetch_terrain", json!({"radius": 1000, "spacing": 1})).contains("spacing"));
    assert_eq!(fake.heights.lock().unwrap().len(), asked, "a spacing that cannot be made is not fetched for");
}

#[test]
fn what_cannot_be_fetched_for_is_refused_and_leaves_the_project_as_it_was() {
    let fake = Arc::new(Fake::new(String::new()));
    let mut none = Session::new();
    run(&mut none, "geolocate", json!({"latitude": HOME.1, "longitude": HOME.0}));
    assert!(refused(&mut none, "fetch_terrain", json!({})).contains("no network access"));
    let mut s = session(fake.clone());
    for (p, says) in [
        (json!({"radius": 4000}), "25"),
        (json!({"bbox": [1, 2, 3]}), "bbox"),
        (json!({"radius": 500, "bbox": [-23.56, -46.64, -23.54, -46.62]}), "not both"),
        (json!({"spacing": "x"}), "spacing"),
        (json!({"spacing": -5}), "spacing"),
        (json!({"set_elevation": "yes"}), "set_elevation"),
        (json!({"interval": 3}), "interval"),
        (json!({"name": 5}), "name"),
        (json!({"colour": 1}), "colour"),
    ] {
        assert!(refused(&mut s, "fetch_terrain", p.clone()).contains(says), "{p}");
    }
    assert!(fake.heights.lock().unwrap().is_empty());
    // A fetcher with nothing under the box, and one that fails.
    let bare = Arc::new(Fake { holes: usize::MAX, ..Fake::new(String::new()) });
    let mut t = session(bare);
    assert!(refused(&mut t, "fetch_terrain", json!({"radius": 300, "spacing": 30})).contains("no height at the project's origin"));
    assert!(refused(&mut t, "fetch_terrain", json!({"radius": 300, "spacing": 30, "set_elevation": false})).contains("no heights"));
    let failing = Arc::new(Fake { fail: Some("s3.amazonaws.com: 503".into()), ..Fake::new(String::new()) });
    let mut u = session(failing);
    assert!(refused(&mut u, "fetch_terrain", json!({"radius": 300, "spacing": 30})).contains("503"));
    for x in [&mut t, &mut u] {
        assert_eq!((terrain_count(x), run(x, "geolocate", json!({}))["elevation"].clone()), (0, json!(0.0)), "nothing changed");
    }
    // Not placed.
    let mut v = Session::new();
    v.geo = Some(fake);
    assert!(refused(&mut v, "fetch_terrain", json!({})).contains("no location"));
}

#[test]
fn the_source_asked_for_goes_to_the_fetcher_and_buildings_then_stand_on_the_ground_fetched() {
    let fake = Arc::new(Fake::new(overpass_reply()));
    let mut s = session(fake.clone());
    let r = run(&mut s, "fetch_terrain", json!({"radius": 400, "spacing": 10, "source": "copernicus"}));
    assert_eq!(r["source"], "copernicus");
    assert_eq!(fake.heights.lock().unwrap()[0].2.as_deref(), Some("copernicus"));
    // The houses 30 m east and 20 m north of the origin stand about 0.3 + 0.4 m higher than at the origin.
    run(&mut s, "fetch_osm", json!({"radius": 400}));
    let bases: Vec<f64> =
        s.model.elements.values().filter_map(|e| if let Element::Mass(m) = e { Some(m.base_offset) } else { None }).collect();
    assert_eq!(bases.len(), 2);
    assert!(bases.iter().all(|b| (-100.0..2500.0).contains(b)), "on the ground fetched, not at the level: {bases:?}");
    assert!(bases.iter().any(|b| *b > 100.0), "{bases:?}");
}

// ---- what a desktop fetched on a thread, handed to the same commands ----

#[test]
fn a_prefetched_reply_runs_the_unchanged_fetch_osm_without_asking_the_real_fetcher_again() {
    let real = Arc::new(Fake::new(overpass_reply()));
    let mut s = session(real.clone());
    let location = s.model.location.unwrap();
    let bbox = Bbox::around(location.longitude, location.latitude, 500.0).unwrap();
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
fn prefetched_elevation_serves_the_spacing_and_source_it_was_fetched_for_and_no_others() {
    let real = Arc::new(Fake::new(String::new()));
    let mut s = session(real.clone());
    let location = s.model.location.unwrap();
    let bbox = Bbox::around(location.longitude, location.latitude, 500.0).unwrap();
    let spacing = crate::geo::default_spacing(&bbox);
    let got = real.elevation(&bbox, spacing, Some("terrarium")).unwrap();
    let real_dyn: crate::geo::Fetcher = real.clone();
    let pre = crate::geo::Prefetched::new(real_dyn, bbox).with_elevation(spacing, Some("terrarium".into()), got);
    // Another box, spacing or source is an error that says what was fetched, not a silent answer from the network.
    let other = Bbox::around(location.longitude, location.latitude, 600.0).unwrap();
    assert!(pre.elevation(&other, spacing, Some("terrarium")).unwrap_err().contains("fetched"));
    assert!(pre.elevation(&bbox, spacing * 2.0, Some("terrarium")).unwrap_err().contains("fetched"));
    assert!(pre.elevation(&bbox, spacing, Some("copernicus")).unwrap_err().contains("fetched"));
    assert!(pre.overpass(&bbox, "x").unwrap_err().contains("fetched"));
    assert_eq!(real.heights.lock().unwrap().len(), 1);
    s.geo = Some(Arc::new(pre));
    let r = run(&mut s, "fetch_terrain", json!({"radius": 500, "source": "terrarium"}));
    assert_eq!(r["source"], json!("terrarium"), "{r}");
    assert_eq!(terrain_count(&s), 1);
    assert_eq!(real.heights.lock().unwrap().len(), 1, "the real fetcher was not asked again");
}

#[test]
fn the_default_spacing_is_what_fetch_terrain_uses() {
    let real = Arc::new(Fake::new(String::new()));
    let mut s = session(real.clone());
    run(&mut s, "fetch_terrain", json!({"radius": 700}));
    let asked = real.heights.lock().unwrap()[0].clone();
    assert_eq!(asked.1, crate::geo::default_spacing(&asked.0));
    assert!(crate::geo::default_spacing(&Bbox::around(0.0, 0.0, 10.0).unwrap()) >= 10.0, "no finer than 10 m");
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
    fn elevation(&self, _: &Bbox, _: f64, _: Option<&str>) -> Result<Elevation, String> {
        Err("none".into())
    }
    fn attribution(&self, _: &str) -> String {
        String::new()
    }
    fn tile(&self, _: u8, _: u32, _: u32) -> Result<crate::geo::Tile, String> {
        Ok(crate::geo::Tile { rgba: vec![1, 2, 3, 255], side: 1, cached: true })
    }
}
