//! Aerial imagery under a plan: the photograph a toposurface wears, drawn first in every plan that shows the terrain, north up, under
//! the walls, left clear where there is no imagery or no terrain, hidden by the view's own category, and never picked.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::render::{self, Drawing, POCHE, Prim, View};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} was taken: {v}"),
        Err(e) => e,
    }
}

/// A PNG of `w` by `h` pixels in this process's scratch folder, pixel (x, y) as `px` says (RGBA, as it is kept).
fn picture(name: &str, w: u32, h: u32, px: impl Fn(u32, u32) -> [u8; 4]) -> String {
    let rgba: Vec<u8> = (0..w * h).flat_map(|k| px(k % w, k / w)).collect();
    let png = tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(w, h).unwrap()).unwrap().encode_png().unwrap();
    let path = std::env::temp_dir().join(format!("bimcraft-underlay-{}-{name}.png", std::process::id()));
    std::fs::write(&path, png).unwrap();
    path.to_string_lossy().into_owned()
}

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const YELLOW: [u8; 4] = [255, 255, 0, 255];

/// 8 by 8 pixels, north up: red in the north west, blue in the north east, green in the south west, yellow in the south east.
fn quadrants(x: u32, y: u32) -> [u8; 4] {
    match (x < 4, y < 4) {
        (true, true) => RED,
        (false, true) => BLUE,
        (true, false) => GREEN,
        (false, false) => YELLOW,
    }
}

fn square() -> Value {
    json!([[0, 0], [10000, 0], [10000, 10000], [0, 10000]])
}

/// A project with 10 m by 10 m of flat terrain wearing the picture at `path`, laid over it by where it is, turned by `angle`.
fn terrained(path: &str, boundary: Value, angle: f64) -> Session {
    let mut s = Session::new();
    run(
        &mut s,
        "material",
        json!({"name": "Aerial", "color": "#808080", "texture": {"kind": "image", "path": path, "size": [10000, 10000], "offset": [0, 10000], "angle": angle, "planar": true}}),
    );
    // A point every metre, so that a boundary that is not a square cuts whole small triangles.
    let points: Vec<Value> = (0..=10).flat_map(|i| (0..=10).map(move |j| json!([i * 1000, j * 1000, 0]))).collect();
    run(&mut s, "toposurface", json!({"points": points, "boundary": boundary, "material": "Aerial"}));
    s
}

fn level(s: &Session) -> crate::ids::Id {
    s.model.levels()[0].0
}

fn images(d: &Drawing) -> usize {
    d.prims.iter().filter(|p| matches!(p, Prim::Image { .. })).count()
}

/// The RGBA the plan's picture has at model point `p`; none where it has no picture.
fn under(d: &Drawing, p: P2) -> Option<[u8; 4]> {
    let (x, y) = d.view.px(p);
    d.prims.iter().find_map(|prim| match prim {
        Prim::Image { at, width, height, rgba, .. } => {
            let (ix, iy) = ((x - at.0).floor(), (y - at.1).floor());
            if ix < 0.0 || iy < 0.0 || ix >= *width as f32 || iy >= *height as f32 {
                return None;
            }
            let i = (iy as usize * *width as usize + ix as usize) * 4;
            rgba.get(i..i + 4).map(|q| [q[0], q[1], q[2], q[3]])
        }
        _ => None,
    })
}

fn pixel(png: &[u8], (x, y): (f32, f32)) -> (u8, u8, u8) {
    let pm = tiny_skia::Pixmap::decode_png(png).unwrap();
    let c = pm.pixel(x as u32, y as u32).unwrap();
    (c.red(), c.green(), c.blue())
}

/// Whether `got` is `want` give or take the blending of a pixel's neighbours.
fn is(got: Option<[u8; 4]>, want: [u8; 4]) -> bool {
    got.is_some_and(|g| g.iter().zip(want).all(|(a, b)| (i32::from(*a) - i32::from(b)).abs() <= 12))
}

#[test]
fn a_plan_shows_the_photograph_under_the_terrain_north_up_and_under_the_walls() {
    let mut s = terrained(&picture("quadrants", 8, 8, quadrants), square(), 0.0);
    run(&mut s, "wall", json!({"points": [[1000, 5000], [9000, 5000]]}));
    let level = level(&s);
    let d = render::plan(&s.model, level, 800, 800);
    assert_eq!(images(&d), 1);
    for (name, at, want) in [
        ("north west", P2(2500.0, 7500.0), RED),
        ("north east", P2(7500.0, 7500.0), BLUE),
        ("south west", P2(2500.0, 2500.0), GREEN),
        ("south east", P2(7500.0, 2500.0), YELLOW),
    ] {
        assert!(is(under(&d, at), want), "{name}: {:?}", under(&d, at));
    }
    // Written out as a picture, the same colours are where they are, and the wall is drawn over them, not under.
    let png = render::png(&d).unwrap();
    let rgb = |c: [u8; 4]| (c[0], c[1], c[2]);
    assert_eq!(pixel(&png, d.view.px(P2(2500.0, 7500.0))), rgb(RED));
    assert_eq!(pixel(&png, d.view.px(P2(7500.0, 2500.0))), rgb(YELLOW));
    assert_eq!(pixel(&png, d.view.px(P2(5000.0, 5000.0))), (POCHE.0, POCHE.1, POCHE.2), "the wall over the photograph");
    // It is the first thing drawn: everything else is over it.
    assert!(matches!(d.prims.first(), Some(Prim::Image { .. })), "{:?}", d.prims.first().map(|_| "an image first"));
}

#[test]
fn the_photograph_is_never_picked_or_boxed() {
    let s = terrained(&picture("never-picked", 8, 8, quadrants), square(), 0.0);
    let d = render::plan(&s.model, level(&s), 800, 800);
    let owners: Vec<_> = d.prims.iter().zip(&d.owners).filter(|(p, _)| matches!(p, Prim::Image { .. })).map(|(_, o)| *o).collect();
    assert_eq!(owners, [None]);
    let (a, b) = (d.view.px(P2(1000.0, 9000.0)), d.view.px(P2(9000.0, 1000.0)));
    assert!(d.in_box(a, b, true).is_empty());
}

#[test]
fn only_the_window_is_made_whatever_the_zoom() {
    let s = terrained(&picture("window", 8, 8, quadrants), square(), 0.0);
    let view = View { scale: 0.08, cx: 2500.0, cy: 7500.0, width: 100, height: 100 };
    let d = render::draw(&s.model, level(&s), view);
    assert_eq!(images(&d), 1);
    for p in &d.prims {
        if let Prim::Image { width, height, .. } = p {
            assert!(*width <= 100 && *height <= 100, "{width} x {height} for a window of 100 x 100");
        }
    }
    assert!(is(under(&d, P2(2500.0, 7500.0)), RED), "{:?}", under(&d, P2(2500.0, 7500.0)));
    // A window that is half over it makes half; one that misses it makes none, whatever its numbers.
    let half = render::draw(&s.model, level(&s), View { cx: 0.0, ..view });
    assert!(
        matches!(half.prims.iter().find_map(|p| if let Prim::Image { width, .. } = p { Some(*width) } else { None }), Some(w) if w < 100)
    );
    for far in [
        View { cx: 1e6, ..view },
        View { cy: -1e12, ..view },
        View { cx: f64::NAN, ..view },
        View { scale: 0.0, ..view },
        View { scale: f64::INFINITY, ..view },
    ] {
        assert_eq!(images(&render::draw(&s.model, level(&s), far)), 0, "{far:?}");
    }
    assert_eq!(images(&render::draw(&s.model, level(&s), View { width: 0, height: 0, ..view })), 0, "a window of nothing");
}

#[test]
fn where_the_picture_lets_nothing_through_the_plan_is_clear_in_the_picture_and_in_the_pdf() {
    let clear_ne = |x: u32, y: u32| if x >= 4 && y < 4 { [0, 0, 0, 0] } else { quadrants(x, y) };
    let s = terrained(&picture("clear-ne", 8, 8, clear_ne), square(), 0.0);
    let d = render::plan(&s.model, level(&s), 800, 800);
    assert_eq!(under(&d, P2(7500.0, 7500.0)).map(|c| c[3]), Some(0), "no imagery in the north east");
    assert_eq!(under(&d, P2(2500.0, 7500.0)).map(|c| c[3]), Some(255));
    assert_eq!(under(&d, P2(7500.0, 2500.0)).map(|c| c[3]), Some(255));
    let png = render::png(&d).unwrap();
    assert_eq!(pixel(&png, d.view.px(P2(7500.0, 7500.0))), (255, 255, 255), "the paper shows through");
    assert!(render::svg(&d).contains("<image"));
    let pdf = crate::pdf::pdf(&[(&d, P2(200.0, 200.0))]);
    assert!(pdf.windows(6).any(|w| w == b"/SMask"), "what is not there is masked");
}

#[test]
fn it_is_cut_to_the_terrains_boundary_so_an_l_shaped_terrain_leaves_its_notch_clear() {
    let l = json!([[0, 0], [10000, 0], [10000, 5000], [5000, 5000], [5000, 10000], [0, 10000]]);
    let s = terrained(&picture("l-shape", 8, 8, quadrants), l, 0.0);
    let d = render::plan(&s.model, level(&s), 800, 800);
    assert_eq!(under(&d, P2(7500.0, 7500.0)).map(|c| c[3]), Some(0), "the notch");
    assert_eq!(under(&d, P2(2500.0, 7500.0)).map(|c| c[3]), Some(255));
    assert_eq!(under(&d, P2(7500.0, 2500.0)).map(|c| c[3]), Some(255));
    assert!(is(under(&d, P2(2500.0, 2500.0)), GREEN));
}

#[test]
fn it_is_left_out_when_the_view_hides_it_the_terrain_is_hidden_the_plan_looks_up_or_the_picture_is_turned() {
    let mut s = terrained(&picture("left-out", 8, 8, quadrants), square(), 0.0);
    let level = level(&s);
    let terrain = *s.model.elements.iter().find(|(_, e)| e.category() == "toposurface").unwrap().0;
    assert_eq!(images(&render::plan(&s.model, level, 600, 600)), 1);
    // The view's own category, "Aerial Imagery": the terrain's contours (and the terrain) stay.
    run(&mut s, "graphics", json!({"view": "Level 1", "categories": {"aerialimagery": {"hidden": true}}}));
    let d = render::plan(&s.model, level, 600, 600);
    assert_eq!(images(&d), 0);
    run(&mut s, "graphics", json!({"view": "Level 1", "reset": true}));
    assert_eq!(images(&render::plan(&s.model, level, 600, 600)), 1, "and back");
    // Hiding the terrain hides what it wears.
    run(&mut s, "graphics", json!({"view": "Level 1", "hide_elements": [terrain]}));
    assert_eq!(images(&render::plan(&s.model, level, 600, 600)), 0);
    run(&mut s, "graphics", json!({"view": "Level 1", "reset": true}));
    run(&mut s, "graphics", json!({"view": "Level 1", "categories": {"toposurface": {"hidden": true}}}));
    assert_eq!(images(&render::plan(&s.model, level, 600, 600)), 0, "the terrain's category too");
    run(&mut s, "graphics", json!({"view": "Level 1", "reset": true}));
    // A plan that looks up sees no ground.
    assert_eq!(images(&render::ceiling_plan(&s.model, level, 600, 600)), 0);
    // A picture turned is not laid under a plan (yet): the rectangle would not be a rectangle in the view.
    let turned = terrained(&picture("turned", 8, 8, quadrants), square(), 30.0);
    assert_eq!(images(&render::plan(&turned.model, level, 600, 600)), 0);
    // Terrain with no picture on it, or a picture the project does not hold: nothing to draw and nothing wrong.
    let mut bare = Session::new();
    run(&mut bare, "toposurface", json!({"points": [[0, 0, 0], [10000, 0, 0], [10000, 10000, 0]]}));
    assert_eq!(images(&render::plan(&bare.model, level, 600, 600)), 0);
}

#[test]
fn the_view_tones_the_photograph_down_by_halftone_and_transparency_and_an_unknown_category_still_lists_it() {
    let mut s = terrained(&picture("named", 8, 8, quadrants), square(), 0.0);
    let level = level(&s);
    let nw = P2(2500.0, 7500.0);
    let drawn = |s: &Session| under(&render::plan(&s.model, level, 600, 600), nw);
    assert!(is(drawn(&s), RED));
    // Halftone: half way to the paper. Transparency: that percent of it let through. Both, as for the other categories.
    run(&mut s, "graphics", json!({"view": "Level 1", "categories": {"aerialimagery": {"halftone": true}}}));
    assert!(is(drawn(&s), [255, 127, 127, 255]), "{:?}", drawn(&s));
    run(&mut s, "graphics", json!({"view": "Level 1", "reset": true, "categories": {"aerialimagery": {"transparency": 50}}}));
    let faded = drawn(&s).unwrap();
    assert!(faded[3].abs_diff(128) <= 2 && faded[0] == 255 && faded[1] == 0, "{faded:?}");
    run(&mut s, "graphics", json!({"view": "Level 1", "reset": true, "categories": {"aerialimagery": {"transparency": 100}}}));
    assert_eq!(drawn(&s).map(|c| c[3]), Some(0), "all let through");
    // Another view is as it was.
    run(&mut s, "graphics", json!({"view": "Level 1", "reset": true}));
    assert!(is(drawn(&s), RED));
    let why = fails(&mut s, "graphics", json!({"view": "Level 1", "categories": {"nothing": {"hidden": true}}}));
    assert!(why.contains("aerialimagery") && why.contains("pointcloud") && why.contains("toposurface"), "{why}");
}

/// How long a plan takes to draw with the photograph filling its window: a timing of this machine, not a check of anything, so it is
/// left out of the suite (run it with `--ignored --nocapture`). The plan is rebuilt for each step of a pan or a zoom.
#[test]
#[ignore = "a timing: run it with --ignored --nocapture"]
fn a_window_full_of_photograph_is_drawn_in_a_moment() {
    let s = terrained(&picture("speed", 1024, 1024, |x, y| [x as u8, y as u8, 90, 255]), square(), 0.0);
    let level = level(&s);
    for (name, view) in [
        ("1500 x 1000 px, the terrain across the window", View { scale: 0.15, cx: 5000.0, cy: 5000.0, width: 1500, height: 1000 }),
        ("1500 x 1000 px, zoomed out, the terrain a part", View { scale: 0.03, cx: 5000.0, cy: 5000.0, width: 1500, height: 1000 }),
        ("1920 x 1080 px, zoomed in, all photograph", View { scale: 0.4, cx: 5000.0, cy: 5000.0, width: 1920, height: 1080 }),
    ] {
        let start = std::time::Instant::now();
        let d = render::draw(&s.model, level, view);
        let spent = start.elapsed();
        println!("{name}: {} image(s), {spent:?}", images(&d));
        assert!(spent < std::time::Duration::from_secs(2), "{name}: {spent:?}");
    }
}
