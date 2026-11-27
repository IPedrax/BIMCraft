//! A 3D view's background (`background`): colours or a picture where the model doesn't show, in the view and in its PNG.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::Element;
use crate::view3d::background::{Background, Fill, Fit, Stop};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A folder of this test's own, empty.
fn dir(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-background-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The default 3D view's picture, 200 by 100 px: its top-left corner's colour and its bottom-left corner's, where the box on
/// the ground in the middle never reaches.
fn corners(s: &mut Session, dir: &std::path::Path) -> ([u8; 3], [u8; 3]) {
    let path = dir.join("3d.png");
    run(s, "render", json!({"view": "{3D}", "path": path, "width": 200, "height": 100}));
    let pixmap = tiny_skia::Pixmap::load_png(&path).unwrap();
    let at = |x: u32, y: u32| {
        let p = pixmap.pixel(x, y).unwrap();
        [p.red(), p.green(), p.blue()]
    };
    (at(1, 1), at(1, 98))
}

fn background(s: &Session) -> Background {
    match s.model.default_3d().and_then(|v| s.model.elements.get(v)) {
        Some(Element::View3D(v)) => v.background.clone(),
        _ => Background::default(),
    }
}

fn fill(s: &Session) -> Fill {
    background(s).fill
}

#[test]
fn colours_show_where_the_model_does_not_in_the_3d_views_png() {
    let d = dir("colours");
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [2000, 0]], "height": 500}));
    assert_eq!(corners(&mut s, &d), ([255, 255, 255], [255, 255, 255]), "paper, as ever");
    // Asking makes nothing: there is no 3D view yet.
    assert_eq!(run(&mut s, "background", json!({}))["background"], json!({}));
    assert!(s.model.default_3d().is_none());
    run(&mut s, "background", json!({"kind": "solid", "color": "#336699"}));
    assert_eq!(fill(&s), Fill::Solid { color: [0x33, 0x66, 0x99] });
    assert_eq!(corners(&mut s, &d), ([0x33, 0x66, 0x99], [0x33, 0x66, 0x99]));
    // Two colours meet at the horizon; the top keeps the colour it had.
    run(&mut s, "background", json!({"kind": "two_color", "bottom": [10, 120, 20]}));
    assert_eq!(fill(&s), Fill::TwoColor { top: [0x33, 0x66, 0x99], bottom: [10, 120, 20] });
    assert_eq!(corners(&mut s, &d), ([0x33, 0x66, 0x99], [10, 120, 20]));
    // A gradient's colours where it places them, in any order.
    run(&mut s, "background", json!({"kind": "gradient", "stops": [[100, [0, 0, 0]], [0, [0, 0, 0]], [50, "#ffffff"]]}));
    let Fill::Gradient { stops } = fill(&s) else { panic!("{:?}", fill(&s)) };
    assert_eq!(stops[2], Stop { at: 50.0, color: [255, 255, 255] });
    let (top, bottom) = corners(&mut s, &d);
    assert!(top.iter().chain(&bottom).all(|c| *c < 12), "dark at both ends: {top:?} {bottom:?}");
    // One undo step each; none puts the paper back.
    run(&mut s, "undo", json!({}));
    assert_eq!(fill(&s).kind(), "two_color");
    run(&mut s, "background", json!({"kind": "none"}));
    assert_eq!(corners(&mut s, &d), ([255, 255, 255], [255, 255, 255]));
}

#[test]
fn a_picture_is_taken_into_the_project_used_and_kept_from_purge() {
    let d = dir("picture");
    let png = d.join("sky.png");
    // Red over blue, 40 by 20 px.
    let rgba: Vec<u8> =
        (0..20u32).flat_map(|y| (0..40u32).flat_map(move |_| if y < 10 { [255, 0, 0, 255] } else { [0, 0, 255, 255] })).collect();
    let pixmap = tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(40, 20).unwrap()).unwrap();
    std::fs::write(&png, pixmap.encode_png().unwrap()).unwrap();
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [2000, 0]], "height": 500}));
    let r = run(&mut s, "background", json!({"kind": "image", "path": png, "fit": "stretch"}));
    let Fill::Image { picture, fit } = fill(&s) else { panic!("{r}") };
    assert_eq!(fit, Fit::Stretch);
    assert!(s.model.images.contains_key(&picture));
    let (top, bottom) = corners(&mut s, &d);
    assert_eq!((top, bottom), ([255, 0, 0], [0, 0, 255]));
    // Purge Unused leaves it: the view uses it.
    let users = crate::assets::users(&s.model);
    assert!(users.keys().any(|(_, name)| *name == picture), "the picture is counted as used");
    // Again by its id, filled now.
    run(&mut s, "background", json!({"kind": "image", "picture": picture}));
    assert_eq!(fill(&s), Fill::Image { picture: picture.clone(), fit: Fit::Stretch }, "the fit it had");
}

#[test]
fn what_is_not_a_background_is_refused_and_changes_nothing() {
    let mut s = Session::new();
    run(&mut s, "background", json!({"kind": "solid", "color": [1, 2, 3]}));
    let before = s.model.clone();
    for (p, says) in [
        (json!({"kind": "sky"}), "`kind`"),
        (json!({"kind": "solid", "color": "blue"}), "colour"),
        (json!({"kind": "solid", "color": [1, 2, 300]}), "colour"),
        (json!({"kind": "solid", "top": [1, 2, 3]}), "isn't a solid background's"),
        (json!({"kind": "gradient", "stops": []}), "`stops`"),
        (json!({"kind": "gradient", "stops": [[120, [1, 2, 3]]]}), "percent"),
        (json!({"kind": "gradient", "stops": [[50]]}), "[percent, colour]"),
        (json!({"ground": "green"}), "colour"),
        (json!({"kind": "image"}), "`path`"),
        (json!({"kind": "image", "picture": "nothing"}), "no picture"),
        (json!({"kind": "image", "path": "/no/such/file.png"}), "file.png"),
        (json!({"kind": "image", "path": "a.pdf"}), "PDF"),
        (json!({"kind": "image", "picture": "x", "fit": "tile"}), "`fit`"),
        (json!({"color": [1, 2, 3]}), "goes with a `kind`"),
        (json!({"kind": "none", "shade": 1}), "shade"),
        (json!({"view": "Level 1", "kind": "none"}), "isn't a 3D view"),
    ] {
        let e = s.execute("background", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
    assert_eq!(s.model.elements, before.elements);
}

#[test]
fn a_horizon_paints_the_ground_under_elevation_0_and_follows_the_camera() {
    let d = dir("horizon");
    let mut s = Session::new();
    // A thin wall standing on the ground, so the view's target is just over elevation 0.
    run(&mut s, "wall", json!({"points": [[0, 0], [2000, 0]], "height": 500}));
    run(&mut s, "background", json!({"kind": "solid", "color": [0, 0, 255], "ground": [0, 128, 0]}));
    assert_eq!(background(&s).ground, Some([0, 128, 0]));
    // Looking a little down: sky over the horizon, a little under the top; ground under it.
    let at = |s: &mut Session, elevation: f64| {
        let path = d.join(format!("at-{elevation}.png"));
        run(s, "render", json!({"view": "{3D}", "path": path, "width": 200, "height": 100, "elevation": elevation}));
        let pixmap = tiny_skia::Pixmap::load_png(&path).unwrap();
        let px = |y: u32| {
            let p = pixmap.pixel(1, y).unwrap();
            [p.red(), p.green(), p.blue()]
        };
        (px(1), px(98))
    };
    assert_eq!(at(&mut s, 10.0), ([0, 0, 255], [0, 128, 0]), "sky over the horizon, ground under it");
    // From the top, all ground; from below, all sky: the horizon follows the camera in a parallel view too.
    assert_eq!(at(&mut s, 85.0), ([0, 128, 0], [0, 128, 0]));
    assert_eq!(at(&mut s, -60.0), ([0, 0, 255], [0, 0, 255]));
    // Off again; the fill stays.
    run(&mut s, "background", json!({"ground": false}));
    assert_eq!(background(&s), Background { fill: Fill::Solid { color: [0, 0, 255] }, ground: None });
}
