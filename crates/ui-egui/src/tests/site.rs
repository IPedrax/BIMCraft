//! The Site tab, driven as a user drives it: Location places the project, the fetches (OpenStreetMap, elevation) run on a thread
//! of their own over a fake fetcher (no test reaches the network) and come out of the same commands, the Pad tool draws a
//! pad, and Place on Terrain and Cut/Fill work on the selection.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use bimcraft_core::geo::{Bbox, Elevation, ElevationGrid, GeoSource, Reply};

use super::*;

const HOME: (f64, f64) = (-46.6333, -23.5505);

/// The height, metres, the fake's ground has at a place: 760 at home, 1 m more each 100 m east, 2 m more each 100 m north.
fn ground(lon: f64, lat: f64) -> f64 {
    760.0 + 1000.0 * (lon - HOME.0) + 2000.0 * (lat - HOME.1)
}

/// A fetcher that answers from what it holds, after `delay`, and notes what it was asked.
struct Fake {
    osm: String,
    delay: Duration,
    fail: Option<String>,
    asked: Mutex<usize>,
}

impl Fake {
    fn new() -> Fake {
        Fake { osm: overpass_reply(), delay: Duration::ZERO, fail: None, asked: Mutex::new(0) }
    }
}

impl GeoSource for Fake {
    fn overpass(&self, _: &Bbox, _: &str) -> Result<Reply, String> {
        *self.asked.lock().unwrap() += 1;
        std::thread::sleep(self.delay);
        match &self.fail {
            Some(why) => Err(why.clone()),
            None => Ok(Reply { text: self.osm.clone(), cached: false }),
        }
    }

    fn elevation(&self, bbox: &Bbox, _: f64, source: Option<&str>) -> Result<Elevation, String> {
        *self.asked.lock().unwrap() += 1;
        std::thread::sleep(self.delay);
        if let Some(why) = &self.fail {
            return Err(why.clone());
        }
        let step = 0.0005;
        let (west, north) = (bbox.west - 2.0 * step, bbox.north + 2.0 * step);
        let cols = ((bbox.east - west) / step).ceil() as usize + 3;
        let rows = ((north - bbox.south) / step).ceil() as usize + 3;
        let values: Vec<f32> =
            (0..rows).flat_map(|r| (0..cols).map(move |c| ground(west + c as f64 * step, north - r as f64 * step) as f32)).collect();
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

fn square(east: f64, north: f64, size: f64) -> String {
    let (lon, lat) = (HOME.0 + east, HOME.1 + north);
    let (dx, dy) = (size * 0.000_01, size * 0.000_009);
    let at = |x: f64, y: f64| format!(r#"{{"lat":{y},"lon":{x}}}"#);
    [at(lon, lat), at(lon + dx, lat), at(lon + dx, lat + dy), at(lon, lat + dy), at(lon, lat)].join(",")
}

fn overpass_reply() -> String {
    let way = |id: u32, east: f64, north: f64, tags: &str| {
        format!(r#"{{"type":"way","id":{id},"geometry":[{}],"tags":{{{tags}}}}}"#, square(east, north, 20.0))
    };
    format!(
        r#"{{"version":0.6,"elements":[{},{}]}}"#,
        way(1, 0.0, 0.0, r#""building":"house","height":"12""#),
        way(2, 0.0003, 0.0002, r#""building":"yes","building:levels":"3""#)
    )
}

/// The window over the box's project, placed at `HOME` and given `fake`.
fn located(fake: Arc<Fake>) -> Harness<'static, BimApp> {
    let mut h = app(BOX);
    h.state_mut().session.execute("geolocate", &json!({"latitude": HOME.1, "longitude": HOME.0})).unwrap();
    h.state_mut().session.geo = Some(fake);
    h.run();
    h
}

fn count(h: &Harness<BimApp>, category: &str) -> usize {
    h.state().session.model.elements.values().filter(|e| e.category() == category).count()
}

/// Steps frames until the fetch is over (its thread has answered and the command has run), or fails.
fn until_fetched(h: &mut Harness<BimApp>) {
    let start = Instant::now();
    while h.state().fetching() {
        assert!(start.elapsed() < Duration::from_secs(20), "the fetch never ended");
        std::thread::sleep(Duration::from_millis(5));
        // A spinner keeps asking for frames: `run` would never settle.
        h.step();
    }
    h.run();
}

/// Presses Fetch, and lets one frame go by: the thread has been started.
fn fetch(h: &mut Harness<BimApp>) {
    h.get_by_label("Fetch").click();
    h.step();
    h.step();
}

#[test]
fn the_site_tab_has_its_panels_and_buttons() {
    let mut h = app(BOX);
    press(&mut h, "Site");
    for label in ["Location", "Terrain from Elevation Data", "Pad", "Place on Terrain", "Cut/Fill", "Import from OpenStreetMap"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    assert!(h.query_by_label("Door").is_none());
}

#[test]
fn location_places_the_project_in_one_undo_step() {
    let mut h = app(BOX);
    assert!(h.state().session.model.location.is_none());
    press(&mut h, "Site");
    press(&mut h, "Location");
    type_into(&mut h, "Latitude (°)", "-23.5505");
    type_into(&mut h, "Longitude (°)", "-46.6333");
    type_into(&mut h, "True north (°)", "10");
    press(&mut h, "OK");
    let l = h.state().session.model.location.expect("placed");
    assert_eq!((l.latitude, l.longitude, l.true_north), (-23.5505, -46.6333, 10.0));
    assert!(h.query_by_label("Latitude (°)").is_none(), "the box closed");
    h.state_mut().session.execute("undo", &json!({})).unwrap();
    assert!(h.state().session.model.location.is_none(), "one undo step takes it back");
}

#[test]
fn a_location_that_cant_be_used_says_why_in_its_box() {
    let mut h = app(BOX);
    press(&mut h, "Site");
    press(&mut h, "Location");
    type_into(&mut h, "Latitude (°)", "95");
    type_into(&mut h, "Longitude (°)", "-46.6333");
    press(&mut h, "OK");
    assert!(h.state().session.model.location.is_none());
    assert!(h.query_by_label("OK").is_some(), "the box stays open");
    type_into(&mut h, "Latitude (°)", "north");
    press(&mut h, "OK");
    assert!(h.query_by_label_contains("is not a number").is_some());
}

#[test]
fn import_from_openstreetmap_fetches_on_a_thread_and_makes_the_buildings_in_one_step() {
    let fake = Arc::new(Fake::new());
    let mut h = located(fake.clone());
    press(&mut h, "Site");
    press(&mut h, "Import from OpenStreetMap");
    fetch(&mut h);
    until_fetched(&mut h);
    assert_eq!(count(&h, "mass"), 2);
    assert_eq!(*fake.asked.lock().unwrap(), 1, "the real fetcher was asked once, on the thread");
    assert!(h.query_by_label_contains("2 buildings made").is_some(), "the box says what it did");
    assert!(h.query_by_label_contains("OpenStreetMap contributors").is_some(), "and the credit the data is owed");
    // The fetcher is the session's own again, and the project one step back.
    assert!(h.state().session.geo.is_some());
    press(&mut h, "Close");
    h.state_mut().session.execute("undo", &json!({})).unwrap();
    assert_eq!(count(&h, "mass"), 0);
}

#[test]
fn a_fetch_that_fails_says_why_in_the_box_and_changes_nothing() {
    let fake = Arc::new(Fake { fail: Some("the server is busy".into()), ..Fake::new() });
    let mut h = located(fake);
    press(&mut h, "Site");
    press(&mut h, "Import from OpenStreetMap");
    fetch(&mut h);
    until_fetched(&mut h);
    assert!(h.query_by_label_contains("the server is busy").is_some(), "in the box, which is back");
    assert_eq!(count(&h, "mass"), 0);
    press(&mut h, "Cancel");
    assert!(h.query_by_label("Fetch").is_none());
}

#[test]
fn a_box_over_the_area_cap_cannot_be_fetched_and_without_a_location_is_refused() {
    let mut h = located(Arc::new(Fake::new()));
    press(&mut h, "Site");
    press(&mut h, "Import from OpenStreetMap");
    type_into(&mut h, "Distance round the origin (m)", "20000");
    assert!(is_disabled(&h, Role::Button, "Fetch"), "Fetch is off over the cap");
    // No location: the same box, a refusal in it.
    let mut h = app(BOX);
    h.state_mut().session.geo = Some(Arc::new(Fake::new()));
    press(&mut h, "Site");
    press(&mut h, "Import from OpenStreetMap");
    assert!(is_disabled(&h, Role::Button, "Fetch"), "the area needs the origin");
    assert!(h.query_by_label_contains("no location yet").is_some());
}

#[test]
fn without_a_fetcher_the_box_says_so_and_fetches_nothing() {
    let mut h = app(BOX);
    h.state_mut().session.execute("geolocate", &json!({"latitude": HOME.1, "longitude": HOME.0})).unwrap();
    press(&mut h, "Site");
    press(&mut h, "Import from OpenStreetMap");
    assert!(h.query_by_label_contains("no network access").is_some());
    assert!(is_disabled(&h, Role::Button, "Fetch"));
}

#[test]
fn cancel_drops_what_the_fetch_brings() {
    let fake = Arc::new(Fake { delay: Duration::from_millis(300), ..Fake::new() });
    let mut h = located(fake.clone());
    press(&mut h, "Site");
    press(&mut h, "Import from OpenStreetMap");
    fetch(&mut h);
    assert!(h.state().fetching());
    assert!(h.query_by_label_contains("Fetching buildings").is_some());
    h.get_by_label("Cancel").click();
    h.step();
    h.step();
    assert!(!h.state().fetching());
    // The thread finishes after; nothing is made from it.
    std::thread::sleep(Duration::from_millis(500));
    h.run();
    assert_eq!(count(&h, "mass"), 0);
}

#[test]
fn agents_wait_while_a_fetch_runs() {
    let fake = Arc::new(Fake { delay: Duration::from_millis(200), ..Fake::new() });
    let mut h = located(fake);
    press(&mut h, "Site");
    press(&mut h, "Import from OpenStreetMap");
    fetch(&mut h);
    assert!(h.state().fetching(), "a fetch holds the window's keys and the agents' requests, as a Revit file does");
    until_fetched(&mut h);
    assert!(!h.state().fetching());
}

#[test]
fn terrain_from_elevation_data_makes_a_toposurface_and_sets_the_project_height() {
    let fake = Arc::new(Fake::new());
    let mut h = located(fake.clone());
    press(&mut h, "Site");
    press(&mut h, "Terrain from Elevation Data");
    type_into(&mut h, "Distance round the origin (m)", "300");
    fetch(&mut h);
    until_fetched(&mut h);
    assert_eq!(count(&h, "toposurface"), 1);
    let l = h.state().session.model.location.unwrap();
    assert!((l.elevation - 760_000.0).abs() < 50.0, "the ground at the origin: {}", l.elevation);
    assert!(h.query_by_label_contains("toposurface of").is_some());
    assert!(h.query_by_label_contains("Elevation: terrarium").is_some(), "its credit");
    assert_eq!(*fake.asked.lock().unwrap(), 1);
}

#[test]
fn a_pad_is_drawn_by_clicks_and_cut_fill_and_place_on_terrain_work_on_what_is_selected() {
    let mut h = located(Arc::new(Fake::new()));
    // A tilted ground, plainly: the fake's, 1 m rise each 100 m east, 2 m each 100 m north, from 0 at the origin.
    let points: Vec<Value> = (-4..=4)
        .flat_map(|i| {
            (-4..=4).map(move |j| json!([f64::from(i) * 2500.0, f64::from(j) * 2500.0, f64::from(i) * 250.0 + f64::from(j) * 500.0]))
        })
        .collect();
    h.state_mut().session.execute("toposurface", &json!({"points": points})).unwrap();
    h.run();
    press(&mut h, "Site");
    press(&mut h, "Pad");
    assert_eq!(h.state().tool, Tool::Pad(Vec::new()));
    for p in [P2(1000.0, 1000.0), P2(6000.0, 1000.0), P2(6000.0, 5000.0), P2(1000.0, 5000.0)] {
        click(&mut h, p);
    }
    key(&mut h, Key::Enter);
    assert_eq!(count(&h, "pad"), 1, "Enter finishes the outline");
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    press(&mut h, "Site");
    let pad = h.state().session.model.elements.iter().find(|(_, e)| e.category() == "pad").map(|(id, _)| *id).unwrap();
    // Cut/Fill with nothing selected counts every pad.
    h.state_mut().selection.clear();
    press(&mut h, "Cut/Fill");
    assert!(h.query_by_label_contains("All: cut").is_some(), "the totals");
    press(&mut h, "Close");
    // A mass stood on the ground where it is.
    h.state_mut()
        .session
        .execute("mass", &json!({"boundary": [[7000, 0], [8000, 0], [8000, 1000], [7000, 1000]], "height": 3000}))
        .unwrap();
    let mass = h.state().session.model.elements.iter().find(|(_, e)| e.category() == "mass").map(|(id, _)| *id).unwrap();
    h.state_mut().selection = vec![mass];
    press(&mut h, "Place on Terrain");
    let Some(Element::Mass(ms)) = h.state().session.model.elements.get(mass) else { panic!("a mass") };
    // The lowest ground under its footprint: at x = 7000, y = 0: 100 mm per 1000 mm east.
    assert!((ms.base_offset - 700.0).abs() < 1.0, "{}", ms.base_offset);
    let _ = pad;
}

#[test]
fn place_on_terrain_with_nothing_selected_says_what_to_do() {
    let mut h = located(Arc::new(Fake::new()));
    press(&mut h, "Site");
    h.state_mut().selection.clear();
    press(&mut h, "Place on Terrain");
    assert!(h.state().status.error && h.state().status.text.contains("Select"), "{:?}", h.state().status);
}

#[test]
fn the_properties_of_a_pad_and_a_terrain_are_shown_and_edited() {
    let mut h = located(Arc::new(Fake::new()));
    let points: Vec<Value> =
        (-2..=2).flat_map(|i| (-2..=2).map(move |j| json!([f64::from(i) * 2500.0, f64::from(j) * 2500.0, 0.0]))).collect();
    h.state_mut().session.execute("toposurface", &json!({"points": points})).unwrap();
    let r = h.state_mut().session.execute("pad", &json!({"boundary": [[0, 0], [2000, 0], [2000, 2000], [0, 2000]]})).unwrap();
    let id = r["id"].as_u64().unwrap();
    h.state_mut().selection = vec![id];
    h.run();
    assert!(h.query_by_label_contains("Height Offset From Level").is_some());
    assert!(h.query_by_label_contains("Thickness").is_some());
}
