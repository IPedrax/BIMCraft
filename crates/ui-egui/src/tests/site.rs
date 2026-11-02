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
    /// The map tiles asked for, in order, and whether there are none.
    tiles: Mutex<Vec<(u8, u32, u32)>>,
    no_tiles: bool,
}

impl Fake {
    fn new() -> Fake {
        Fake { osm: overpass_reply(), delay: Duration::ZERO, fail: None, asked: Mutex::new(0), tiles: Mutex::default(), no_tiles: false }
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

    fn tile(&self, zoom: u8, x: u32, y: u32) -> Result<bimcraft_core::geo::Tile, String> {
        self.tiles.lock().unwrap().push((zoom, x, y));
        if self.no_tiles {
            return Err("no map tiles here".into());
        }
        let rgba = [(x % 256) as u8, (y % 256) as u8, 90, 255].repeat(256 * 256);
        Ok(bimcraft_core::geo::Tile { rgba, side: 256, cached: false })
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

/// The Overpass query a box shows, whole (its label).
fn query_shown(h: &Harness<BimApp>) -> String {
    use egui_kittest::kittest::NodeT;
    let n = h.get_by_label_contains("[out:json]");
    n.accesskit_node().label().or_else(|| n.accesskit_node().value()).unwrap_or_default().to_string()
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

#[test]
fn a_pair_pasted_from_a_map_fills_latitude_and_longitude_and_places_the_project() {
    let mut h = app(BOX);
    press(&mut h, "Site");
    press(&mut h, "Location");
    type_into(&mut h, "Coordinates", "51.2254809, 6.7700762");
    press(&mut h, "OK");
    let l = h.state().session.model.location.expect("placed");
    assert_eq!((l.latitude, l.longitude), (51.2254809, 6.7700762));
    // Germany: WGS 84 / UTM zone 32 north.
    assert_eq!(l.grid().unwrap().epsg, 32632);
}

#[test]
fn a_pair_pasted_into_latitude_or_longitude_is_split_the_same_way() {
    for field in ["Latitude (°)", "Longitude (°)"] {
        let mut h = app(BOX);
        press(&mut h, "Site");
        press(&mut h, "Location");
        type_into(&mut h, field, "51,2254809; 6,7700762");
        press(&mut h, "OK");
        let l = h.state().session.model.location.expect("placed");
        assert_eq!((l.latitude, l.longitude), (51.2254809, 6.7700762), "{field}");
    }
}

#[test]
fn a_string_that_is_not_a_pair_says_so_and_changes_nothing() {
    let mut h = app(BOX);
    press(&mut h, "Site");
    press(&mut h, "Location");
    type_into(&mut h, "Coordinates", "95, 6.7");
    assert!(h.query_by_label_contains("beyond 90").is_some(), "the reason shows under the row");
    press(&mut h, "OK");
    assert!(h.state().session.model.location.is_none(), "nothing was filled, so nothing is placed");
}

#[test]
fn the_import_box_shows_the_overpass_query_that_will_be_asked() {
    let mut h = located(Arc::new(Fake::new()));
    press(&mut h, "Site");
    press(&mut h, "Import from OpenStreetMap");
    assert!(h.query_by_label_contains("[out:json]").is_some(), "the query is there before anything is fetched");
    assert!(h.query_by_label_contains("building:part").is_none());
    pick(&mut h, Role::CheckBox, "Parts of buildings too");
    assert!(h.query_by_label_contains("building:part").is_some(), "it follows the options");
    // And the area: 10 times the distance, a box 10 times as wide.
    let before = query_shown(&h);
    type_into(&mut h, "Distance round the origin (m)", "900");
    let after = query_shown(&h);
    assert_ne!(before, after, "it follows the area");
}

#[test]
fn what_was_fetched_with_is_said_when_it_is_done() {
    let mut h = located(Arc::new(Fake::new()));
    press(&mut h, "Site");
    press(&mut h, "Import from OpenStreetMap");
    let shown = query_shown(&h);
    fetch(&mut h);
    until_fetched(&mut h);
    assert!(h.query_by_label("Query used").is_some());
    let said = query_shown(&h);
    assert_eq!(shown, said, "the query shown before is the one that was asked");
}

// ---- the map ----

/// Steps frames until the map has a picture of a tile, or fails.
fn until_tiles(h: &mut Harness<BimApp>) {
    let start = Instant::now();
    while h.state().map().is_some_and(|m| m.ready() == 0) {
        assert!(start.elapsed() < Duration::from_secs(20), "no tile ever came");
        std::thread::sleep(Duration::from_millis(5));
        h.step();
    }
    h.run();
}

/// The Location box open over a project not yet placed, with the map open at `typed`.
fn place_map(fake: Arc<Fake>, typed: &str) -> Harness<'static, BimApp> {
    let mut h = app(BOX);
    h.state_mut().session.geo = Some(fake);
    press(&mut h, "Site");
    press(&mut h, "Location");
    type_into(&mut h, "Coordinates", typed);
    press(&mut h, "Pick on Map…");
    h
}

#[test]
fn the_map_asks_for_the_tiles_it_shows_once_each_and_a_click_picks_the_place() {
    let fake = Arc::new(Fake::new());
    let mut h = place_map(fake.clone(), "51.2254809, 6.7700762");
    until_tiles(&mut h);
    let m = h.state().map().unwrap();
    let asked = fake.tiles.lock().unwrap().clone();
    assert!(!asked.is_empty() && asked.len() <= 40, "{}", asked.len());
    assert!(asked.iter().all(|(z, _, _)| *z == 15), "the zoom it opens at: {asked:?}");
    let mut once = asked.clone();
    once.sort();
    once.dedup();
    assert_eq!(once.len(), asked.len(), "no tile twice");
    assert_eq!(m.asked(), asked.len());
    // The place typed is the marker, and a click 100 px east of it moves the marker there.
    assert_eq!(m.place(), Some((6.7700762, 51.2254809)));
    let at = m.screen_of(6.7700762, 51.2254809).unwrap() + vec2(100.0, -40.0);
    click_at(&mut h, at);
    let (lon, lat) = h.state().map().unwrap().place().unwrap();
    assert!(lon > 6.7700762 && lon < 6.78 && lat > 51.2254809 && lat < 51.2275, "{lon} {lat}");
    // Use This Place gives it to the Location box, and OK to the project.
    press(&mut h, "Use This Place");
    assert!(h.state().map().is_none());
    press(&mut h, "OK");
    let l = h.state().session.model.location.expect("placed");
    assert!((l.longitude - lon).abs() < 1e-6 && (l.latitude - lat).abs() < 1e-6, "{l:?}");
}

#[test]
fn dragging_moves_the_map_and_the_wheel_zooms_about_the_pointer() {
    let mut h = place_map(Arc::new(Fake::new()), "51.2254809, 6.7700762");
    let before = h.state().map().unwrap().view();
    let from = h.state().map().unwrap().canvas_centre();
    drag_screen(&mut h, from, from + vec2(120.0, 0.0), PointerButton::Primary);
    let after = h.state().map().unwrap().view();
    assert!(after.lon < before.lon, "dragged east, so what was west of the middle comes in: {} {}", before.lon, after.lon);
    assert_eq!(h.state().map().unwrap().place(), Some((6.7700762, 51.2254809)), "a drag is not a click");
    // The wheel zooms in; the place under the pointer stays under it.
    let m = h.state().map().unwrap();
    let pointer = from + vec2(150.0, 60.0);
    let (lon, lat) = (m.view().lon, m.view().lat);
    let _ = (lon, lat);
    h.hover_at(pointer);
    h.run();
    let under = {
        let m = h.state().map().unwrap();
        let r = m.canvas_rect();
        m.view().from_screen(((pointer.x - r.min.x) as f64, (pointer.y - r.min.y) as f64), (r.width() as f64, r.height() as f64))
    };
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, 240.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    for _ in 0..8 {
        h.step();
    }
    let m = h.state().map().unwrap();
    assert!(m.view().zoom > after.zoom, "{} {}", m.view().zoom, after.zoom);
    let there = m.screen_of(under.0, under.1).unwrap();
    assert!((there - pointer).length() < 1.5, "the place under the pointer stays: {there:?} {pointer:?}");
}

#[test]
fn go_to_takes_a_pair_pasted_from_a_map_and_says_when_it_is_not_one() {
    let mut h = place_map(Arc::new(Fake::new()), "-23.5505, -46.6333");
    type_into(&mut h, "Go to", "52.5200, 13.4050");
    press(&mut h, "Go");
    let v = h.state().map().unwrap().view();
    assert!((v.lon - 13.405).abs() < 1e-9 && (v.lat - 52.52).abs() < 1e-9 && v.zoom >= 14.0, "{v:?}");
    type_into(&mut h, "Go to", "somewhere");
    press(&mut h, "Go");
    assert!(h.query_by_label_contains("two numbers").is_some());
    assert_eq!(h.state().map().unwrap().view().lon, v.lon, "the map stays");
}

#[test]
fn a_box_is_drawn_by_a_drag_and_used_by_the_area_box_which_fetches_it() {
    let fake = Arc::new(Fake::new());
    let mut h = located(fake.clone());
    press(&mut h, "Site");
    press(&mut h, "Import from OpenStreetMap");
    pick(&mut h, Role::RadioButton, "A box of degrees");
    press(&mut h, "Pick on Map…");
    until_tiles(&mut h);
    assert!(is_disabled(&h, Role::Button, "Use This Box"), "no box yet");
    let m = h.state().map().unwrap();
    let v = m.view();
    let (a, b) = (m.screen_of(v.lon, v.lat).unwrap() + vec2(-80.0, -60.0), m.screen_of(v.lon, v.lat).unwrap() + vec2(90.0, 70.0));
    drag_screen(&mut h, a, b, PointerButton::Primary);
    let drawn = h.state().map().unwrap().area().expect("a box");
    assert!(drawn.contains(v.lon, v.lat) && drawn.area_km2() > 0.1 && drawn.area_km2() < 25.0, "{drawn:?}");
    assert!(h.query_by_label_contains("km² (at most 25)").is_some());
    press(&mut h, "Use This Box");
    // The four degrees are in the area box now, and fetching takes that box.
    fetch(&mut h);
    until_fetched(&mut h);
    assert_eq!(count(&h, "mass"), 2);
    assert!(h.query_by_label_contains("[out:json]").is_some());
    let query = query_shown(&h);
    assert!(query.contains(&format!("{}", drawn.south)) || query.contains(&format!("{:.5}", drawn.south)), "{query}");
}

#[test]
fn a_box_over_what_one_fetch_may_cover_is_red_and_cannot_be_used() {
    let mut h = located(Arc::new(Fake::new()));
    press(&mut h, "Site");
    press(&mut h, "Terrain from Elevation Data");
    pick(&mut h, Role::RadioButton, "A box of degrees");
    press(&mut h, "Pick on Map…");
    until_tiles(&mut h);
    for _ in 0..3 {
        press(&mut h, "−");
    }
    let m = h.state().map().unwrap();
    let r = m.canvas_rect();
    drag_screen(&mut h, r.min + vec2(40.0, 40.0), r.max - vec2(40.0, 40.0), PointerButton::Primary);
    let drawn = h.state().map().unwrap().area().expect("a box");
    assert!(drawn.area_km2() > 25.0, "{}", drawn.area_km2());
    assert!(is_disabled(&h, Role::Button, "Use This Box"));
    assert!(h.query_by_label_contains("km² (at most 25)").is_some());
}

#[test]
fn with_no_fetcher_or_no_tiles_the_map_is_lines_and_the_place_can_still_be_picked() {
    // No fetcher at all.
    let mut h = app(BOX);
    press(&mut h, "Site");
    press(&mut h, "Location");
    type_into(&mut h, "Coordinates", "51.2254809, 6.7700762");
    press(&mut h, "Pick on Map…");
    assert_eq!((h.state().map().unwrap().asked(), h.state().map().unwrap().ready()), (0, 0));
    assert!(h.query_by_label_contains("can still be picked").is_some(), "the map says it has no picture");
    let at = h.state().map().unwrap().screen_of(6.7700762, 51.2254809).unwrap() + vec2(30.0, 30.0);
    click_at(&mut h, at);
    assert!(h.state().map().unwrap().place().is_some());
    assert!(!is_disabled(&h, Role::Button, "Use This Place"));
    // A fetcher that has no tiles: the reason is said and nothing breaks.
    let fake = Arc::new(Fake { no_tiles: true, ..Fake::new() });
    let mut h = place_map(fake.clone(), "51.2254809, 6.7700762");
    let start = Instant::now();
    while h.state().map().is_some_and(|m| m.asked() > 0) && !h.query_by_label_contains("no map tiles here").is_some() {
        assert!(start.elapsed() < Duration::from_secs(20), "the reason never came");
        std::thread::sleep(Duration::from_millis(5));
        h.step();
    }
    assert_eq!(h.state().map().unwrap().ready(), 0);
    // Cancel is in both boxes; the map's is the one drawn last.
    h.get_all_by_label("Cancel").last().unwrap().click();
    h.run();
    assert!(h.state().map().is_none());
    assert!(h.query_by_label("OK").is_some(), "the Location box is still there");
}

// ---- Build Shell ----

fn masses_of(h: &Harness<BimApp>) -> Vec<Id> {
    h.state().session.model.elements.iter().filter(|(_, e)| e.category() == "mass").map(|(id, _)| *id).collect()
}

/// A window with the fake's two buildings imported (house of 12 m, and a building of 3 levels), closed, and both selected.
fn imported_and_selected() -> Harness<'static, BimApp> {
    let mut h = located(Arc::new(Fake::new()));
    press(&mut h, "Site");
    press(&mut h, "Import from OpenStreetMap");
    fetch(&mut h);
    until_fetched(&mut h);
    press(&mut h, "Close");
    let masses = masses_of(&h);
    assert_eq!(masses.len(), 2);
    h.state_mut().selection = masses;
    h.run();
    h
}

#[test]
fn build_shell_makes_the_walls_floors_and_roofs_of_the_buildings_selected_in_one_step() {
    let mut h = imported_and_selected();
    press(&mut h, "Build Shell");
    press(&mut h, "OK");
    // The project has its box of four walls already; each building has 4 and a roof; a 12 m house is four storeys, the
    // three-level building three.
    assert_eq!((count(&h, "wall"), count(&h, "floor"), count(&h, "roof")), (4 + 8, 7, 2));
    assert!(h.query_by_label_contains("8 walls, 7 floors and 2 roofs").is_some(), "the box says what it made");
    press(&mut h, "Close");
    h.state_mut().session.execute("undo", &json!({})).unwrap();
    assert_eq!((count(&h, "wall"), count(&h, "floor"), count(&h, "roof")), (4, 0, 0));
    assert_eq!(count(&h, "mass"), 2, "the buildings stay");
}

#[test]
fn build_shell_leaves_out_what_is_unticked_and_a_second_run_skips_what_has_a_shell() {
    let mut h = imported_and_selected();
    press(&mut h, "Build Shell");
    pick(&mut h, Role::CheckBox, "Floors at each storey");
    pick(&mut h, Role::CheckBox, "Roof (the shape the OpenStreetMap tags give, else flat)");
    press(&mut h, "OK");
    assert_eq!((count(&h, "wall"), count(&h, "floor"), count(&h, "roof")), (4 + 8, 0, 0));
    press(&mut h, "Close");
    // Again: the buildings have walls by face already.
    press(&mut h, "Build Shell");
    press(&mut h, "OK");
    assert_eq!(count(&h, "wall"), 4 + 8);
    assert!(h.query_by_label_contains("already have").is_some());
}

#[test]
fn build_shell_needs_buildings_selected_and_a_storey_height_that_is_a_length() {
    let mut h = imported_and_selected();
    h.state_mut().selection.clear();
    press(&mut h, "Build Shell");
    assert!(h.state().status.error && h.state().status.text.contains("Select the buildings"), "{:?}", h.state().status);
    assert!(h.query_by_label("OK").is_none(), "no box opens");
    h.state_mut().selection = masses_of(&h);
    h.run();
    press(&mut h, "Build Shell");
    type_into(&mut h, "Storey height", "tall");
    press(&mut h, "OK");
    assert!(h.query_by_label_contains("is not a length").is_some());
    assert_eq!(count(&h, "wall"), 4);
    type_into(&mut h, "Storey height", "4000");
    press(&mut h, "OK");
    assert_eq!(count(&h, "wall"), 4 + 8);
    // 4 m storeys: the 12 m house has three, and the three-level building keeps its levels (3), 4 m apart.
    assert_eq!(count(&h, "floor"), 6);
}
