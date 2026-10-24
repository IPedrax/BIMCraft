//! Point clouds through the commands every front end drives: a scan comes in, is placed as an element, goes where the element
//! goes, and is saved with the project.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::cloud::MAX_CLOUDS;
use crate::engine::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} was taken: {v}"),
        Err(e) => e,
    }
}

/// A scan file in a folder of this test's own, so its name is its stem.
fn scan(test: &str, name: &str, text: &str) -> String {
    let dir = std::env::temp_dir().join(format!("bimcraft-cloud-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path: PathBuf = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

/// A scan of a box 4 m by 3 m by 2.5 m in survey coordinates, metres, its corners coloured.
const BOX: &str = "500000 7000000 10 255 0 0\n500004 7000000 10 0 255 0\n500004 7000003 10 0 0 255\n500000 7000003 12.5 255 255 255\n";

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.5)
}

/// The box round a placed cloud's points, in the model, mm.
fn bounds(s: &Session, id: u64) -> ([f64; 3], [f64; 3]) {
    s.model.cloud_box(id).unwrap_or_else(|| panic!("{id} is no placed cloud"))
}

fn one(s: &mut Session) -> Value {
    let all = run(s, "elements", json!({"category": "pointcloud"}));
    assert_eq!(all["count"], 1, "{all}");
    all["elements"][0].clone()
}

#[test]
fn a_scan_comes_in_with_its_middle_on_the_origin_and_its_foot_on_the_level() {
    let mut s = Session::new();
    let r = run(&mut s, "point_cloud", json!({"path": scan("centre", "box.xyz", BOX)}));
    assert_eq!((r["cloud"].as_str(), r["points"].as_u64(), r["read"].as_u64()), (Some("box"), Some(4), Some(4)), "{r}");
    // Where its middle was in the file, mm: what Origin to Origin would have kept.
    let origin: Vec<f64> = r["origin"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!(near([origin[0], origin[1], origin[2]], [500_002_000.0, 7_000_001_500.0, 11_250.0]), "{origin:?}");
    let id = r["id"].as_u64().unwrap();
    let e = one(&mut s);
    assert_eq!(
        (e["id"].as_u64(), e["cloud"].as_str(), &e["at"], &e["offset"], &e["angle"]),
        (Some(id), Some("box"), &json!([0.0, 0.0]), &json!(1250.0), &json!(0.0)),
        "{e}"
    );
    let (lo, hi) = bounds(&s, id);
    assert!(near(lo, [-2000.0, -1500.0, 0.0]) && near(hi, [2000.0, 1500.0, 2500.0]), "{lo:?} {hi:?}");
    // On another level, it stands on that one.
    let up = run(&mut s, "point_cloud", json!({"cloud": "box", "level": "Level 2"}))["id"].as_u64().unwrap();
    let level2 = run(&mut s, "elements", json!({"category": "level"}))["elements"][1]["elevation"].as_f64().unwrap();
    assert!(level2 > 0.0);
    assert!(near(bounds(&s, up).0, [-2000.0, -1500.0, level2]), "{:?}", bounds(&s, up));
}

#[test]
fn origin_to_origin_keeps_the_files_coordinates_where_a_model_can_hold_them() {
    let mut s = Session::new();
    let e = fails(&mut s, "point_cloud", json!({"path": scan("origin", "box.xyz", BOX), "origin": "file"}));
    assert!(e.contains("centre"), "{e}");
    let near_home = scan("origin", "near.xyz", "1 2 0\n3 4 1\n");
    let id = run(&mut s, "point_cloud", json!({"path": near_home, "origin": "file"}))["id"].as_u64().unwrap();
    let (lo, hi) = bounds(&s, id);
    assert!(near(lo, [1000.0, 2000.0, 0.0]) && near(hi, [3000.0, 4000.0, 1000.0]), "{lo:?} {hi:?}");
    let e = fails(&mut s, "point_cloud", json!({"cloud": "near", "origin": "nowhere"}));
    assert!(e.contains("centre") && e.contains("file"), "{e}");
    // Placed by hand: its middle where it's put, turned a quarter, so its 4 m run north.
    let id = run(&mut s, "point_cloud", json!({"cloud": "near", "at": [10_000, 0], "offset": 3000, "angle": 90}))["id"].as_u64().unwrap();
    let (lo, hi) = bounds(&s, id);
    assert!(near(lo, [9000.0, -1000.0, 2500.0]) && near(hi, [11_000.0, 1000.0, 3500.0]), "{lo:?} {hi:?}");
}

#[test]
fn units_spacing_and_files_it_cant_take() {
    let mut s = Session::new();
    let feet = scan("units", "feet.xyz", "0 0 0\n10 0 0\n");
    let id = run(&mut s, "point_cloud", json!({"path": feet, "units": "ft"}))["id"].as_u64().unwrap();
    let (lo, hi) = bounds(&s, id);
    assert!((hi[0] - lo[0] - 3048.0).abs() < 0.5, "{lo:?} {hi:?}");
    // A thousand points a metre apart, one kept in each 2 m cube.
    let grid: String = (0..1000).map(|i| format!("{} {} {}\n", i % 10, i / 10 % 10, i / 100)).collect();
    let r = run(&mut s, "point_cloud", json!({"path": scan("units", "grid.xyz", &grid), "spacing": 2000}));
    assert_eq!((r["points"].as_u64(), r["read"].as_u64()), (Some(125), Some(1000)), "{r}");
    for (p, says) in [
        (json!({"path": feet, "units": "furlong", "name": "f2"}), "mm, cm, m, in, ft"),
        (json!({"path": feet, "spacing": -1, "name": "f3"}), "spacing"),
        (json!({"path": scan("units", "plan.dwg", "x")}), "PTS, XYZ, PLY or LAS"),
        (json!({"path": scan("units", "scan.laz", "x")}), "LAZ"),
        (json!({"path": scan("units", "gone", "x").replace("gone", "not-there.xyz")}), "not-there.xyz"),
        (json!({"path": scan("units", "bad.xyz", "0 0 zero\n")}), "line 1"),
        (json!({}), "path"),
        (json!({"cloud": "nope"}), "nope"),
    ] {
        let e = fails(&mut s, "point_cloud", p.clone());
        assert!(e.contains(says), "{p}: {e}");
    }
}

#[test]
fn several_files_come_in_as_one_scan_with_a_station_for_each() {
    let mut s = Session::new();
    // Two stations of one survey, metres, meeting at x = 4: one coloured, one not.
    let east = scan("stations", "east.xyz", "0 0 0 255 0 0\n4 0 0 255 0 0\n");
    let west = scan("stations", "west.xyz", "4 0 0\n8 0 2\n");
    let r = run(&mut s, "point_cloud", json!({"path": [east, west]}));
    assert_eq!((r["cloud"].as_str(), r["points"].as_u64(), r["read"].as_u64()), (Some("east"), Some(4), Some(4)), "{r}");
    let origin: Vec<f64> = r["origin"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!(near([origin[0], origin[1], origin[2]], [4000.0, 0.0, 1000.0]), "{origin:?}");
    let (lo, hi) = bounds(&s, r["id"].as_u64().unwrap());
    assert!(near(lo, [-4000.0, 0.0, 0.0]) && near(hi, [4000.0, 0.0, 2000.0]), "{lo:?} {hi:?}");
    let e = one(&mut s);
    assert_eq!(e["stations"], json!([{"source": "east.xyz", "points": 2}, {"source": "west.xyz", "points": 2}]), "{e}");
    // The station with no colours is grey among the one with.
    let colours = s.model.clouds.get("east").unwrap().colours().unwrap().to_vec();
    assert_eq!((colours[0], colours[2]), ([255, 0, 0], [crate::cloud::GREY; 3]));
    // Where the stations overlap, a spacing keeps one point.
    let r = run(&mut s, "point_cloud", json!({"path": [east, west], "name": "thinned", "spacing": 100}));
    assert_eq!((r["points"].as_u64(), r["read"].as_u64()), (Some(3), Some(4)), "{r}");
    let stations = s.model.clouds.get("thinned").unwrap().stations().to_vec();
    assert_eq!(stations.iter().map(|t| t.points).collect::<Vec<_>>(), [2, 1]);
    // Saved, it opens with its stations.
    let before = s.model.clouds.clone();
    let file = std::env::temp_dir().join(format!("bimcraft-cloud-{}-stations.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": file}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": file}));
    assert_eq!(t.model.clouds, before);
    // A list of one is the file alone.
    run(&mut s, "point_cloud", json!({"path": [east], "name": "alone"}));
    assert!(s.model.clouds.get("alone").unwrap().stations().is_empty());
    let many = vec![east.clone(); crate::cloud::MAX_STATIONS + 1];
    for (p, says) in [
        (json!({"path": []}), "path"),
        (json!({"path": [east, 3], "name": "n"}), "path"),
        (json!({"path": many, "name": "n"}), &crate::cloud::MAX_STATIONS.to_string()[..]),
    ] {
        let e = fails(&mut s, "point_cloud", p.clone());
        assert!(e.contains(says), "{e}");
    }
}

#[test]
fn a_scan_is_brought_in_once_and_placed_as_often_as_wanted() {
    let mut s = Session::new();
    let path = scan("again", "box.xyz", BOX);
    run(&mut s, "point_cloud", json!({"path": path}));
    run(&mut s, "point_cloud", json!({"cloud": "box", "at": [10_000, 0]}));
    assert_eq!((s.model.clouds.len(), run(&mut s, "elements", json!({"category": "pointcloud"}))["count"].as_u64()), (1, Some(2)));
    // The same name again is a mistake that says what to do instead.
    let e = fails(&mut s, "point_cloud", json!({"path": path}));
    assert!(e.contains("already") && e.contains("`cloud`") && e.contains("`name`"), "{e}");
    run(&mut s, "point_cloud", json!({"path": path, "name": "box 2"}));
    assert_eq!(s.model.clouds.len(), 2);
    // A project holds so many scans.
    for i in 2..MAX_CLOUDS {
        run(&mut s, "point_cloud", json!({"path": path, "name": format!("box {}", i + 1)}));
    }
    let e = fails(&mut s, "point_cloud", json!({"path": path, "name": "one too many"}));
    assert!(e.contains(&MAX_CLOUDS.to_string()), "{e}");
}

#[test]
fn a_scan_moves_turns_and_copies_with_its_element_but_doesnt_mirror() {
    let mut s = Session::new();
    let id = run(&mut s, "point_cloud", json!({"path": scan("edit", "box.xyz", BOX)}))["id"].as_u64().unwrap();
    run(&mut s, "move", json!({"ids": [id], "by": [1000, 500]}));
    assert_eq!(one(&mut s)["at"], json!([1000.0, 500.0]));
    run(&mut s, "rotate", json!({"ids": [id], "about": [0, 0], "angle": 90}));
    let e = one(&mut s);
    assert!((e["at"][0].as_f64().unwrap() + 500.0).abs() < 1e-6 && (e["at"][1].as_f64().unwrap() - 1000.0).abs() < 1e-6, "{e}");
    assert_eq!(e["angle"], json!(90.0));
    let copies = run(&mut s, "copy", json!({"ids": [id], "by": [0, 10_000]}));
    assert_eq!(run(&mut s, "elements", json!({"category": "pointcloud"}))["count"], 2, "{copies}");
    assert_eq!(s.model.clouds.len(), 1);
    let e = fails(&mut s, "mirror", json!({"ids": [id], "axis": [[0, 0], [0, 1000]]}));
    assert!(e.contains("mirror") && e.contains("scan"), "{e}");
    let e = fails(&mut s, "modify", json!({"id": id, "cloud": "nope"}));
    assert!(e.contains("nope"), "{e}");
    let e = fails(&mut s, "modify", json!({"id": id, "at": [2e9, 0]}));
    assert!(e.contains("at"), "{e}");
}

#[test]
fn a_scan_goes_when_nothing_places_it_and_undo_brings_it_back() {
    let mut s = Session::new();
    let id = run(&mut s, "point_cloud", json!({"path": scan("delete", "box.xyz", BOX)}))["id"].as_u64().unwrap();
    let copy = run(&mut s, "point_cloud", json!({"cloud": "box", "at": [10_000, 0]}))["id"].as_u64().unwrap();
    run(&mut s, "delete", json!({"ids": [id]}));
    assert_eq!(s.model.clouds.len(), 1, "one still places it");
    run(&mut s, "delete", json!({"ids": [copy]}));
    assert!(s.model.clouds.is_empty());
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.clouds.len(), 1);
}

#[test]
fn a_scan_is_saved_with_the_project_and_opens_as_it_was() {
    let mut s = Session::new();
    let r = run(&mut s, "point_cloud", json!({"path": scan("save", "box.xyz", BOX), "angle": 30, "at": [100, 200]}));
    let before = (one(&mut s), s.model.clouds.clone());
    let file = std::env::temp_dir().join(format!("bimcraft-cloud-{}-save.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": file}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": file}));
    assert_eq!((one(&mut t), t.model.clouds.clone()), before, "{r}");
    let cloud = t.model.clouds.get("box").unwrap();
    assert_eq!((cloud.len(), cloud.colours().is_some()), (4, true));
}

#[test]
fn a_project_with_a_scan_still_exports_ifc() {
    let mut s = Session::new();
    run(&mut s, "point_cloud", json!({"path": scan("ifc", "box.xyz", BOX)}));
    let file = std::env::temp_dir().join(format!("bimcraft-cloud-{}-scan.ifc", std::process::id()));
    run(&mut s, "save", json!({"path": file}));
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.starts_with("ISO-10303-21;") && text.contains("IFCPROJECT"));
}

#[test]
fn insert_from_file_brings_a_scan_with_the_point_cloud_that_places_it() {
    let mut from = Session::new();
    let id = run(&mut from, "point_cloud", json!({"path": scan("insert", "box.xyz", BOX), "at": [500, 0]}))["id"].as_u64().unwrap();
    let file = std::env::temp_dir().join(format!("bimcraft-cloud-{}-insert.bimcraft", std::process::id()));
    run(&mut from, "save", json!({"path": file}));
    let mut s = Session::new();
    run(&mut s, "insert", json!({"path": file, "ids": [id]}));
    let e = one(&mut s);
    assert_eq!((e["cloud"].as_str(), &e["at"]), (Some("box"), &json!([500.0, 0.0])), "{e}");
    assert_eq!(s.model.clouds.get("box"), from.model.clouds.get("box"));
}

// ---------- the 3D view ----------

use crate::graphics::Look;
use crate::render::Rgb;
use crate::view3d::{AZIMUTH, ELEVATION, Image, Scene, gpu};

/// A scan of a wall's face: points every 20 mm over 2 m by 2 m standing on y = 0, in metres, each in `rgb` or in none.
fn sheet(rgb: Option<[u8; 3]>) -> String {
    let mut text = String::new();
    for i in 0..=100 {
        for k in 0..=100 {
            let (x, z) = (f64::from(i) * 0.02, f64::from(k) * 0.02);
            text += &match rgb {
                Some([r, g, b]) => format!("{x} 0 {z} {r} {g} {b}\n"),
                None => format!("{x} 0 {z}\n"),
            };
        }
    }
    text
}

/// How many pixels of the picture are `colour`'s.
fn pixels(img: &Image, colour: impl Fn(Rgb) -> bool) -> usize {
    let (w, h) = (img.width, img.height);
    (0..h).flat_map(|y| (0..w).map(move |x| (f64::from(x) + 0.5, f64::from(y) + 0.5))).filter(|&(x, y)| colour(img.color(x, y))).count()
}

fn red(c: Rgb) -> bool {
    c.0 > 200 && c.1 < 60 && c.2 < 60
}

#[test]
fn a_scan_shows_in_the_3d_view_in_its_own_colours_fitted_and_picked_as_its_element() {
    let mut s = Session::new();
    let sheet = scan("view", "sheet.xyz", &sheet(Some([255, 0, 0])));
    let id = run(&mut s, "point_cloud", json!({"path": sheet, "at": [40_000, -25_000]}))["id"].as_u64().unwrap();
    let scene = Scene::new(&s.model);
    let img = scene.render(&scene.fit(AZIMUTH, ELEVATION, 320, 240), &[]);
    let reds: Vec<(f64, f64)> = (0..240)
        .flat_map(|y| (0..320).map(move |x| (f64::from(x) + 0.5, f64::from(y) + 0.5)))
        .filter(|&(x, y)| red(img.color(x, y)))
        .collect();
    // Fitted, the view shows all of the scan, far as it is from the origin, its points close enough to read as a surface.
    let (lo, hi) = reds
        .iter()
        .fold(((f64::MAX, f64::MAX), (f64::MIN, f64::MIN)), |(lo, hi), p| ((lo.0.min(p.0), lo.1.min(p.1)), (hi.0.max(p.0), hi.1.max(p.1))));
    assert!(lo.0 > 1.0 && lo.1 > 1.0 && hi.0 < 319.0 && hi.1 < 239.0, "{lo:?} to {hi:?}");
    let filled = reds.len() as f64 / ((hi.0 - lo.0) * (hi.1 - lo.1));
    assert!(reds.len() > 3000 && filled > 0.5, "{} {filled}", reds.len());
    assert!(reds.iter().all(|&(x, y)| img.pick(x, y) == Some(id)));
    assert!(img.shows().contains(&id));
    assert!(img.in_box((0.0, 0.0), (320.0, 240.0), false).contains(&id));
    // A point has no face to paint.
    assert_eq!(img.face(reds[0].0, reds[0].1), None);
    // Selected, it shows in the selection's colour.
    let tinted = scene.render(&scene.fit(AZIMUTH, ELEVATION, 320, 240), &[(id, Rgb(0, 0, 255))]);
    assert_eq!(pixels(&tinted, red), 0);
}

#[test]
fn a_scan_with_no_colours_is_grey_and_a_view_can_colour_or_hide_it() {
    let mut s = Session::new();
    let id = run(&mut s, "point_cloud", json!({"path": scan("grey", "sheet.xyz", &sheet(None))}))["id"].as_u64().unwrap();
    let scene = Scene::new(&s.model);
    let cam = scene.fit(AZIMUTH, ELEVATION, 320, 240);
    let grey = |c: Rgb| c == Rgb(160, 160, 160);
    let n = pixels(&scene.render_in(&cam, &[], &Look::of_3d(&s.model)), grey);
    assert!(n > 3000, "{n}");
    run(&mut s, "graphics", json!({"view": "{3D}", "elements": {id.to_string(): {"projection": {"fill": [255, 0, 0]}}}}));
    let img = scene.render_in(&cam, &[], &Look::of_3d(&s.model));
    assert_eq!((pixels(&img, grey), pixels(&img, red)), (0, n));
    run(&mut s, "graphics", json!({"view": "{3D}", "reset": true, "hide_elements": [id]}));
    let img = scene.render_in(&cam, &[], &Look::of_3d(&s.model));
    assert_eq!((pixels(&img, grey), img.shows().contains(&id)), (0, false));
}

#[test]
fn what_is_nearer_hides_a_scan_and_a_scan_hides_what_is_behind_it() {
    let mut s = Session::new();
    run(&mut s, "point_cloud", json!({"path": scan("depth", "sheet.xyz", &sheet(Some([255, 0, 0])))}));
    let cam = Scene::new(&s.model).fit(AZIMUTH, ELEVATION, 320, 240);
    let reds = |s: &Session| pixels(&Scene::new(&s.model).render(&cam, &[]), red);
    let alone = reds(&s);
    // A tall wall a metre to either side of it: one is in front of it, the other behind.
    let mut seen = Vec::new();
    for y in [-1000, 1000] {
        run(&mut s, "wall", json!({"points": [[-3000, y], [3000, y]], "height": 6000}));
        seen.push(reds(&s));
        run(&mut s, "undo", json!({}));
    }
    seen.sort();
    assert!(seen[0] < alone / 10 && seen[1] > alone * 9 / 10, "{alone}: {seen:?}");
}

#[test]
fn the_graphics_card_leaves_a_view_with_a_scan_to_the_processor() {
    let mut s = Session::new();
    let id = run(&mut s, "point_cloud", json!({"path": scan("gpu", "box.xyz", BOX)}))["id"].as_u64().unwrap();
    run(&mut s, "wall", json!({"points": [[0, 0], [3000, 0]]}));
    let err = gpu::Mesh::new(&Scene::new(&s.model), &Look::of_3d(&s.model)).err().unwrap();
    assert!(err.contains("point cloud"), "{err}");
    run(&mut s, "graphics", json!({"view": "{3D}", "hide_elements": [id]}));
    assert!(gpu::Mesh::new(&Scene::new(&s.model), &Look::of_3d(&s.model)).is_ok());
}

// ---------- plans and sections ----------

use crate::geom::P2;
use crate::render;

/// A scan of floors seen from above, metres: two red patches 2 m square half a metre up, at the origin and 4 m north-east of
/// it, and a blue one 2 m up over the first.
fn floors() -> String {
    let mut text = String::new();
    for (x0, y0, z, [r, g, b]) in [(0.0, 0.0, 0.5, [255, 0, 0]), (4.0, 4.0, 0.5, [255, 0, 0]), (0.0, 0.0, 2.0, [0, 0, 255])] {
        for i in 0..=100 {
            for k in 0..=100 {
                text += &format!("{} {} {z} {r} {g} {b}\n", x0 + f64::from(i) * 0.02, y0 + f64::from(k) * 0.02);
            }
        }
    }
    text
}

fn blue(c: Rgb) -> bool {
    c.0 < 60 && c.1 < 60 && c.2 > 200
}

/// How many pixels of a drawing's PNG are `colour`'s.
fn drawn(d: &render::Drawing, colour: impl Fn(Rgb) -> bool) -> usize {
    let pm = tiny_skia::Pixmap::decode_png(&render::png(d).unwrap()).unwrap();
    pm.pixels().iter().map(|p| p.demultiply()).filter(|p| colour(Rgb(p.red(), p.green(), p.blue()))).count()
}

#[test]
fn a_plan_shows_what_of_a_scan_lies_in_its_view_range_and_picks_it_where_its_points_are() {
    let mut s = Session::new();
    let id = run(&mut s, "point_cloud", json!({"path": scan("plan", "floors.xyz", &floors()), "origin": "file"}))["id"].as_u64().unwrap();
    let levels = run(&mut s, "elements", json!({"category": "level"}));
    let level = |n: usize| levels["elements"][n]["id"].as_u64().unwrap();
    // Level 1 cuts at 1200 and sees down to the level: the red patches, not the blue one over them.
    let d = render::plan(&s.model, level(0), 600, 600);
    let (reds, blues) = (drawn(&d, red), drawn(&d, blue));
    assert!(reds > 3000 && blues == 0, "{reds} {blues}");
    let at = |q: P2| d.view.px(q);
    assert_eq!((d.pick(at(P2(1000.0, 1000.0)), 3.0), d.pick(at(P2(5000.0, 5000.0)), 3.0)), (Some(id), Some(id)));
    // Between the patches there are no points to pick.
    assert_eq!(d.pick(at(P2(3000.0, 3000.0)), 3.0), None);
    assert!(d.in_box(at(P2(-500.0, 6500.0)), at(P2(6500.0, -500.0)), false).contains(&id));
    // A plan whose range is above it shows none of it.
    let up = render::plan(&s.model, level(1), 600, 600);
    assert!(!up.owners.contains(&Some(id)));
    // Written out, it's a picture: in the SVG, and in a PDF with what isn't a point left clear.
    assert!(render::svg(&d).contains("<image"));
    let pdf = crate::pdf::pdf(&[(&d, P2(200.0, 200.0))]);
    assert!(pdf.windows(6).any(|w| w == b"/SMask"));
    // The view colours it, or hides it.
    run(&mut s, "graphics", json!({"view": "Level 1", "elements": {id.to_string(): {"projection": {"fill": [0, 0, 255]}}}}));
    let d = render::plan(&s.model, level(0), 600, 600);
    assert_eq!((drawn(&d, red), drawn(&d, blue)), (0, reds));
    run(&mut s, "graphics", json!({"view": "Level 1", "reset": true, "hide_elements": [id]}));
    assert!(!render::plan(&s.model, level(0), 600, 600).owners.contains(&Some(id)));
}

#[test]
fn a_section_shows_a_scan_between_its_line_and_its_far_clip() {
    let mut s = Session::new();
    let id =
        run(&mut s, "point_cloud", json!({"path": scan("section", "floors.xyz", &floors()), "origin": "file"}))["id"].as_u64().unwrap();
    // Looking north from a metre south of the patches, which it sees edge on.
    let look = |s: &mut Session, depth: Option<f64>| {
        let mut p = json!({"start": [-1000, -1000], "end": [7000, -1000]});
        if let Some(d) = depth {
            p["depth"] = json!(d);
        }
        let v = run(s, "section", p)["id"].as_u64().unwrap();
        let view = render::section_fit(&s.model, v, 600, 400);
        render::section_image(&s.model, &Scene::new(&s.model), v, view, &[]).unwrap()
    };
    let img = look(&mut s, None);
    let (reds, blues) = (pixels(&img, red), pixels(&img, blue));
    assert!(reds > 100 && blues > 50, "{reds} {blues}");
    let (w, h) = (img.width, img.height);
    let at =
        (0..h).flat_map(|y| (0..w).map(move |x| (f64::from(x) + 0.5, f64::from(y) + 0.5))).find(|&(x, y)| red(img.color(x, y))).unwrap();
    assert_eq!(img.pick(at.0, at.1), Some(id));
    // Its far clip half a metre past its line stops short of them.
    let img = look(&mut s, Some(500.0));
    assert_eq!((pixels(&img, red), pixels(&img, blue)), (0, 0));
}
