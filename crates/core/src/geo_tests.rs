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
    /// The imagery it was asked for, in order: the source and the plan.
    imagery_asked: Mutex<Vec<(String, crate::imagery::Plan)>>,
    /// The sources it has no imagery of.
    no_imagery: Vec<&'static str>,
    /// What it is untruthful about in the imagery it gives.
    lie: Option<Lie>,
}

/// How a fetcher can be wrong about the imagery it brings: hostile, as a file is.
#[derive(Clone, Copy)]
enum Lie {
    Source,
    Plan,
    Mosaic,
}

impl Fake {
    fn new(osm: String) -> Fake {
        Fake {
            osm,
            asked: Mutex::default(),
            heights: Mutex::default(),
            fail: None,
            holes: 0,
            imagery_asked: Mutex::default(),
            no_imagery: Vec::new(),
            lie: None,
        }
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

    fn imagery(&self, source: &str, plan: &crate::imagery::Plan) -> Result<crate::imagery::Imagery, String> {
        use crate::imagery::{Imagery, Mosaic, TILE};
        self.imagery_asked.lock().unwrap().push((source.to_string(), plan.clone()));
        if let Some(why) = &self.fail {
            return Err(why.clone());
        }
        if self.no_imagery.contains(&source) {
            return Err(format!("{source} has no imagery of this place"));
        }
        // Every tile the same gradient: never one colour.
        let tile: Vec<u8> = (0..TILE * TILE).flat_map(|k| [(k % TILE) as u8, (k / TILE) as u8, 90]).collect();
        let mut mosaic = Mosaic::new(plan);
        for row in 0..plan.rows {
            for col in 0..plan.cols {
                mosaic.put(col, row, &tile)?;
            }
        }
        let mut got = Imagery { source: source.into(), plan: plan.clone(), mosaic, cached: false };
        match self.lie {
            Some(Lie::Source) => got.source = "usgs".into(),
            Some(Lie::Plan) => got.plan.zoom = got.plan.zoom.saturating_sub(1),
            Some(Lie::Mosaic) => {
                got.mosaic.rgb.pop();
            }
            None => {}
        }
        Ok(got)
    }

    fn attribution(&self, what: &str) -> String {
        match what {
            "osm" => "Data (c) OpenStreetMap contributors (ODbL)".into(),
            other => match crate::imagery::source(other) {
                Some(source) => source.credit.into(),
                None => format!("Elevation: {other}"),
            },
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

/// The ground of the project's toposolid `id` (else its only one) at each of `points`, mm.
fn heights(s: &Session, points: &[(f64, f64)]) -> Vec<Option<f64>> {
    heights_on(s, points, None)
}

fn heights_on(s: &Session, points: &[(f64, f64)], id: Option<crate::ids::Id>) -> Vec<Option<f64>> {
    let t = s
        .model
        .elements
        .iter()
        .find_map(|(i, e)| match e {
            Element::Toposolid(t) if id.is_none_or(|want| want == *i) => Some(t.clone()),
            _ => None,
        })
        .expect("a toposolid");
    let g = crate::ground::Ground::of(&s.model, &t);
    points.iter().map(|(x, y)| g.height_at(crate::geom::P2(*x, *y))).collect()
}

fn terrain_count(s: &Session) -> usize {
    s.model.elements.values().filter(|e| e.category() == "toposolid").count()
}

/// The site's height on the map, mm: the height of project z = 0.
fn site_height(s: &Session) -> f64 {
    s.model.site().height
}

fn bases(s: &Session) -> std::collections::BTreeMap<String, f64> {
    s.model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Mass(ms) => Some((ms.params.get("OSM Id")?.as_str()?.to_string(), ms.base_offset)),
            _ => None,
        })
        .collect()
}

#[test]
fn fetch_terrain_makes_the_ground_with_the_origin_at_its_own_height() {
    let fake = Arc::new(Fake::new(String::new()));
    let mut s = session(fake.clone());
    let r = run(&mut s, "fetch_terrain", json!({"radius": 500, "spacing": 25, "name": "Ground", "interval": 500}));
    assert_eq!(r["source"], "terrarium", "{r}");
    // The project had no height above the sea: the ground at its origin (760 m) becomes it, so project z = 0 is there.
    assert!((r["elevation_set"].as_f64().unwrap() - 760_000.0).abs() < 1.0, "{r}");
    assert!((site_height(&s) - 760_000.0).abs() < 1.0);
    let at = heights(&s, &[(0.0, 0.0), (100_000.0, 0.0), (0.0, 100_000.0), (-100_000.0, -100_000.0)]);
    // 100 m east is 0.00098 degrees: 0.98 m more; 100 m north is 0.000898 degrees: 1.8 m more (the project's axes are a little
    // out of line with the degrees, hence the slack).
    for (got, want) in at.iter().zip([0.0, 980.0, 1800.0, -2780.0]) {
        assert!(got.is_some_and(|z| (z - want).abs() < 40.0), "{at:?}");
    }
    // The box is the terrain: about a square kilometre, and nothing past it.
    assert!((r["area_m2"].as_f64().unwrap() - 1.0e6).abs() < 0.06e6, "{r}");
    assert_eq!(heights(&s, &[(800_000.0, 0.0), (0.0, -800_000.0)]), [None, None]);
    assert!(r["points"].as_u64().unwrap() <= crate::model::MAX_SHAPE_POINTS as u64, "{r}");
    // The hook was asked once, for the box and the spacing, and the reply carries its credit.
    let asked = fake.heights.lock().unwrap();
    assert_eq!(asked.len(), 1);
    assert!((asked[0].0.area_km2() - 1.0).abs() < 0.02 && asked[0].1 == 25.0 && asked[0].2.is_none());
    assert_eq!(r["attribution"], "Elevation: terrarium");
    let names: Vec<String> =
        s.model.elements.values().filter_map(|e| if let Element::Toposolid(t) = e { Some(t.name.clone()) } else { None }).collect();
    assert_eq!(names, ["Ground"]);
    // One step: the terrain and the height above the sea come back together.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!((terrain_count(&s), site_height(&s)), (0, 0.0));
}

#[test]
fn a_height_the_project_already_has_is_kept_and_asked_not_to_change_it_is_not_changed() {
    let fake = Arc::new(Fake::new(String::new()));
    let mut s = session(fake.clone());
    let name = s.model.coordinates.active.clone();
    run(&mut s, "location", json!({"name": name, "elevation": 700_000}));
    let r = run(&mut s, "fetch_terrain", json!({"radius": 300, "spacing": 20}));
    assert!(r["elevation_set"].is_null(), "{r}");
    assert!(heights(&s, &[(0.0, 0.0)])[0].is_some_and(|z| (z - 60_000.0).abs() < 120.0), "760 m ground over a 700 m origin");
    let mut t = session(fake);
    let r = run(&mut t, "fetch_terrain", json!({"radius": 300, "spacing": 20, "set_elevation": false}));
    assert!(r["elevation_set"].is_null());
    assert!(heights(&t, &[(0.0, 0.0)])[0].is_some_and(|z| (z - 760_000.0).abs() < 120.0), "heights as they come");
    assert_eq!(site_height(&t), 0.0);
}

#[test]
fn the_default_spacing_keeps_the_points_under_the_cap_and_a_fine_one_is_refused() {
    let fake = Arc::new(Fake::new(String::new()));
    let mut s = session(fake.clone());
    let r = run(&mut s, "fetch_terrain", json!({"radius": 2000}));
    let n = r["points"].as_u64().unwrap();
    assert!((1_000..=crate::model::MAX_SHAPE_POINTS as u64).contains(&n), "as many as a toposolid holds, and no more: {n}");
    let asked = fake.heights.lock().unwrap().len();
    let why = refused(&mut s, "fetch_terrain", json!({"radius": 1000, "spacing": 25}));
    assert!(why.contains("spacing") && why.contains("toposolid"), "{why}");
    assert_eq!(fake.heights.lock().unwrap().len(), asked, "a spacing that cannot be made is not fetched for");
}

#[test]
fn what_cannot_be_fetched_for_is_refused_and_leaves_the_project_as_it_was() {
    let fake = Arc::new(Fake::new(String::new()));
    let mut none = Session::new();
    locate(&mut none, HOME.0, HOME.1);
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
        assert_eq!((terrain_count(x), site_height(x)), (0, 0.0), "nothing changed");
    }
    // Not placed.
    let mut v = Session::new();
    v.geo = Some(fake);
    assert!(refused(&mut v, "fetch_terrain", json!({})).contains("no place on Earth"));
}

#[test]
fn the_source_asked_for_goes_to_the_fetcher_and_buildings_then_stand_on_the_ground_fetched() {
    let fake = Arc::new(Fake::new(overpass_reply()));
    let mut s = session(fake.clone());
    let r = run(&mut s, "fetch_terrain", json!({"radius": 400, "spacing": 20, "source": "copernicus"}));
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

#[test]
fn prefetched_elevation_serves_the_spacing_and_source_it_was_fetched_for_and_no_others() {
    let real = Arc::new(Fake::new(String::new()));
    let mut s = session(real.clone());
    let (lon, lat) = crate::geo::Projector::of(&s.model).unwrap().origin().unwrap();
    let bbox = Bbox::around(lon, lat, 500.0).unwrap();
    let spacing = crate::geo::terrain_spacing(&bbox, crate::geo::TERRAIN_POINTS);
    let got = real.elevation(&bbox, spacing, Some("terrarium")).unwrap();
    let real_dyn: crate::geo::Fetcher = real.clone();
    let pre = crate::geo::Prefetched::new(real_dyn, bbox).with_elevation(spacing, Some("terrarium".into()), got);
    // Another box, spacing or source is an error that says what was fetched, not a silent answer from the network.
    let other = Bbox::around(lon, lat, 600.0).unwrap();
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
    assert_eq!(asked.1, crate::geo::terrain_spacing(&asked.0, crate::geo::TERRAIN_POINTS));
    assert!(asked.1 >= crate::geo::default_spacing(&asked.0), "never finer than the default");
    assert!(crate::geo::default_spacing(&Bbox::around(0.0, 0.0, 10.0).unwrap()) >= 10.0, "no finer than 10 m");
}

/// The lowest ground, mm, toposolid `terrain` has under the corners and middle of building `osm`: where it should stand.
fn lowest_under(s: &Session, osm: &str, terrain: crate::ids::Id) -> f64 {
    let ring = s
        .model
        .elements
        .values()
        .find_map(|e| match e {
            Element::Mass(ms) if ms.params.get("OSM Id").and_then(Value::as_str) == Some(osm) => Some(ms.footprint()),
            _ => None,
        })
        .unwrap();
    let n = ring.len() as f64;
    let middle = (ring.iter().map(|q| q.0).sum::<f64>() / n, ring.iter().map(|q| q.1).sum::<f64>() / n);
    let at: Vec<(f64, f64)> = ring.iter().map(|q| (q.0, q.1)).chain([middle]).collect();
    heights_on(s, &at, Some(terrain)).into_iter().flatten().fold(f64::MAX, f64::min)
}

fn toposolid_ids(s: &Session) -> Vec<crate::ids::Id> {
    s.model.elements.iter().filter_map(|(id, e)| matches!(e, Element::Toposolid(_)).then_some(*id)).collect()
}

#[test]
fn terrain_fetched_after_the_buildings_stands_them_on_it_in_one_undo_step() {
    let mut s = session(Arc::new(Fake::new(overpass_reply())));
    run(&mut s, "fetch_osm", json!({"radius": 500}));
    assert_eq!(bases(&s).values().copied().collect::<Vec<_>>(), [0.0, 0.0], "no ground yet: they stand at the level");
    let r = run(&mut s, "fetch_terrain", json!({"radius": 500, "spacing": 25}));
    let ground = toposolid_ids(&s)[0];
    let now = bases(&s);
    for osm in ["way/1", "way/2"] {
        let want = lowest_under(&s, osm, ground);
        assert!((now[osm] - want).abs() < 1.0, "{osm} stands at {} and the ground under it is at {want}", now[osm]);
    }
    assert!(now["way/2"] > 300.0, "the second house is up the slope: {now:?}");
    assert_eq!((r["stood"]["moved"].clone(), r["stood"]["off_terrain"].clone()), (json!(2), json!(0)), "{r}");
    // One step brings the buildings back with the terrain and the ground height.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(bases(&s).values().copied().collect::<Vec<_>>(), [0.0, 0.0]);
    assert_eq!(terrain_count(&s), 0);
}

#[test]
fn buildings_are_left_where_they_are_when_asked_to_be() {
    let mut s = session(Arc::new(Fake::new(overpass_reply())));
    run(&mut s, "fetch_osm", json!({"radius": 500}));
    let r = run(&mut s, "fetch_terrain", json!({"radius": 500, "spacing": 25, "stand_buildings": false}));
    assert!(r["stood"].is_null(), "{r}");
    assert_eq!(bases(&s).values().copied().collect::<Vec<_>>(), [0.0, 0.0]);
    // A value that is not true or false is refused, naming the key, and nothing is fetched or changed.
    let before = s.model.clone();
    for bad in [json!("yes"), json!(1), json!([true])] {
        let why = refused(&mut s, "fetch_terrain", json!({"radius": 500, "stand_buildings": bad}));
        assert!(why.contains("stand_buildings"), "{bad}: {why}");
    }
    assert_eq!(s.model, before);
}

#[test]
fn only_the_imported_buildings_over_the_new_ground_are_stood_on_it_and_the_others_are_counted_or_left() {
    let far = way(3, &square(0.01, 0.0, 20.0), r#""building":"yes","height":"6""#);
    let osm = overpass_reply().replacen(r#"]}"#, &format!(",{far}]}}"), 1);
    let mut s = session(Arc::new(Fake::new(osm)));
    run(&mut s, "fetch_osm", json!({"radius": 2000}));
    assert_eq!(bases(&s).len(), 3);
    // One the user drew, over the ground to come.
    let own = run(&mut s, "mass", json!({"boundary": [[40000, 40000], [46000, 40000], [46000, 46000], [40000, 46000]], "height": 3000}));
    let own = own["id"].as_u64().unwrap();
    let r = run(&mut s, "fetch_terrain", json!({"radius": 500, "spacing": 25}));
    // The far building is a kilometre past the box: counted, and where it was.
    assert_eq!((r["stood"]["moved"].clone(), r["stood"]["off_terrain"].clone()), (json!(2), json!(1)), "{r}");
    let now = bases(&s);
    assert_eq!(now["way/3"], 0.0);
    assert!(now["way/2"] > 300.0, "{now:?}");
    // What came from nowhere is not touched: only the buildings the import stood on terrain follow terrain.
    let drawn = s.model.elements.iter().find_map(|(id, e)| match e {
        Element::Mass(ms) if *id == own => Some(ms.base_offset),
        _ => None,
    });
    assert_eq!(drawn, Some(0.0));
}

#[test]
fn a_building_with_walls_floors_and_a_roof_keeps_them_on_its_new_base() {
    let mut s = session(Arc::new(Fake::new(overpass_reply())));
    run(&mut s, "fetch_osm", json!({"radius": 500, "detail": ["way/2"]}));
    let walls = |s: &Session| -> Vec<f64> {
        s.model.elements.values().filter_map(|e| if let Element::Wall(w) = e { Some(w.base_offset) } else { None }).collect()
    };
    assert_eq!(walls(&s), [0.0; 4]);
    let r = run(&mut s, "fetch_terrain", json!({"radius": 500, "spacing": 25}));
    let base = bases(&s)["way/2"];
    assert!(base > 300.0);
    assert_eq!(walls(&s), [base; 4], "the walls were re-placed with their mass");
    assert!(r["stood"]["shells_updated"].as_u64().unwrap() >= 4, "{r}");
    assert!(r["stood"]["shells_left"].as_array().unwrap().is_empty(), "{r}");
    // Undo puts the walls back with the rest.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(walls(&s), [0.0; 4]);
}

#[test]
fn a_shell_that_can_no_longer_follow_its_mass_is_named_and_the_buildings_still_stand() {
    let mut s = session(Arc::new(Fake::new(overpass_reply())));
    run(&mut s, "fetch_osm", json!({"radius": 500, "detail": ["way/2"]}));
    // The user took a corner off the mass: the wall made on its last edge has no edge to follow.
    let mass = s
        .model
        .elements
        .iter()
        .find_map(|(id, e)| match e {
            Element::Mass(ms) if ms.params.get("OSM Id").and_then(Value::as_str) == Some("way/2") => Some(*id),
            _ => None,
        })
        .unwrap();
    if let Some(Element::Mass(ms)) = s.model_mut().elements.get_mut(mass) {
        ms.boundary.pop();
    }
    let r = run(&mut s, "fetch_terrain", json!({"radius": 500, "spacing": 25}));
    let left = r["stood"]["shells_left"].as_array().unwrap();
    assert_eq!(left.len(), 1, "{r}");
    assert_eq!(left[0]["id"].as_u64(), Some(mass), "{r}");
    assert!(left[0]["why"].as_str().unwrap().contains("face"), "{r}");
    // Both buildings stand on the new ground all the same, and the shell was left as it was, as a whole.
    assert_eq!(r["stood"]["moved"], 2, "{r}");
    assert!(bases(&s)["way/2"] > 300.0);
    let walls: Vec<f64> =
        s.model.elements.values().filter_map(|e| if let Element::Wall(w) = e { Some(w.base_offset) } else { None }).collect();
    assert_eq!(walls, [0.0; 4]);
}

#[test]
fn buildings_stand_on_the_toposolid_just_fetched_and_not_on_an_older_one() {
    let mut s = session(Arc::new(Fake::new(overpass_reply())));
    // An older, flat terrain 5 m up: the buildings come in standing on it.
    run(
        &mut s,
        "toposolid",
        json!({"boundary": [[-100000, -100000], [300000, -100000], [300000, 300000], [-100000, 300000]], "offset": 5000}),
    );
    run(&mut s, "fetch_osm", json!({"radius": 500}));
    assert!(bases(&s).values().all(|b| (b - 5000.0).abs() < 1e-6), "{:?}", bases(&s));
    run(&mut s, "fetch_terrain", json!({"radius": 500, "spacing": 25}));
    let ids = toposolid_ids(&s);
    assert_eq!(ids.len(), 2);
    let newest = ids[1];
    let now = bases(&s);
    for osm in ["way/1", "way/2"] {
        let want = lowest_under(&s, osm, newest);
        assert!((now[osm] - want).abs() < 1.0, "{osm}: {} against {want}", now[osm]);
    }
    assert!(now["way/1"] < 1000.0, "not on the old terrain's 5 m: {now:?}");
}

#[test]
fn place_on_terrain_stands_what_is_chosen_on_the_toposolid_and_says_what_is_not_over_it() {
    let mut s = Session::new();
    // A toposolid 10 m square rising 1 m west to east.
    let boundary = json!([[0, 0], [10000, 0], [10000, 10000], [0, 10000]]);
    let points = json!([[0, 0, 0], [10000, 0, 1000], [10000, 10000, 1000], [0, 10000, 0]]);
    run(&mut s, "toposolid", json!({"boundary": boundary, "points": points}));
    let mass = |s: &mut Session, x: i32| {
        run(s, "mass", json!({"boundary": [[x, 4000], [x + 2000, 4000], [x + 2000, 6000], [x, 6000]], "height": 3000}))["id"]
            .as_u64()
            .unwrap()
    };
    let (a, b, off) = (mass(&mut s, 4000), mass(&mut s, 6000), mass(&mut s, 20000));
    let r = run(&mut s, "place_on_terrain", json!({"ids": [a, b, off], "offset": 50}));
    let base = |s: &Session, id: u64| match s.model.elements.get(id) {
        Some(Element::Mass(m)) => m.base_offset,
        _ => f64::NAN,
    };
    assert!((base(&s, a) - 450.0).abs() < 1e-6 && (base(&s, b) - 650.0).abs() < 1e-6, "{r}");
    assert_eq!(base(&s, off), 0.0);
    assert_eq!(r["skipped"][0]["id"].as_u64(), Some(off), "{r}");
    assert!(refused(&mut s, "place_on_terrain", json!({"ids": [off]})).contains("over the terrain"));
    assert!(refused(&mut s, "place_on_terrain", json!({})).contains("`ids`"));
    let mut none = Session::new();
    let m = mass(&mut none, 0);
    assert!(refused(&mut none, "place_on_terrain", json!({"ids": [m]})).contains("toposolid"));
}
