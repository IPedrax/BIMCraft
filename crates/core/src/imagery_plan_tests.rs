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

// ---- The credit the photograph is owed, lettered wherever it is shown ----

/// Gives the terrain's photograph the credit text: the project parameter `Imagery Credit`, and its value on every toposurface.
fn credited(s: &mut Session, credit: &str) {
    run(s, "parameter", json!({"name": "Imagery Credit", "kind": "text", "categories": ["toposurface"]}));
    // Written into the model as a file could hold it (the command would refuse a text this long, or with control characters in it).
    for e in s.model_mut().elements.values_mut() {
        if let crate::model::Element::Toposurface(t) = e {
            t.params.insert("Imagery Credit".into(), json!(credit));
        }
    }
}

/// The same project with and without a credit: two sessions wearing the same picture.
fn with_and_without(name: &str, credit: &str) -> (Session, Session) {
    let path = picture(name, 8, 8, quadrants);
    let plain = terrained(&path, square(), 0.0);
    let mut s = terrained(&path, square(), 0.0);
    credited(&mut s, credit);
    (s, plain)
}

fn window() -> View {
    View { scale: 0.06, cx: 5000.0, cy: 5000.0, width: 800, height: 800 }
}

/// What `with` draws after everything `without` draws: the credit, which is pushed last.
fn extra<'a>(with: &'a Drawing, without: &Drawing) -> (&'a [Prim], &'a [Option<crate::ids::Id>]) {
    (with.prims.get(without.prims.len()..).unwrap_or_default(), with.owners.get(without.owners.len()..).unwrap_or_default())
}

#[test]
fn a_plan_letters_the_credit_of_its_photograph_at_its_bottom_right_on_a_plate_and_nobody_owns_it() {
    let (s, plain) = with_and_without("credit", "(c) Test Credit");
    let (with, without) = (render::draw(&s.model, level(&s), window()), render::draw(&plain.model, level(&plain), window()));
    let (made, owners) = extra(&with, &without);
    assert!(made.iter().any(|p| matches!(p, Prim::Fill { .. })), "a plate for it to read on");
    assert!(made.iter().filter(|p| matches!(p, Prim::Line { .. })).count() > 8, "and its lettering");
    for p in made {
        for (x, y) in p.points() {
            assert!((0.0..=800.0).contains(&x) && (0.0..=800.0).contains(&y), "inside the window: {x}, {y}");
            assert!(x > 400.0 && y > 700.0, "in its bottom right corner: {x}, {y}");
        }
    }
    assert!(owners.iter().all(Option::is_none), "nobody's: never picked");
    assert!(with.in_box((400.0, 700.0), (800.0, 800.0), true).is_empty(), "nor boxed");
    // It is on the page whichever way the plan is written out.
    assert!(render::svg(&with).len() > render::svg(&without).len());
    // Over a photograph that fills the window, the corner is the plate's paper and not the photograph, past the end of the lettering.
    let filled = View { scale: 0.12, ..window() };
    let (over, bare) = (render::draw(&s.model, level(&s), filled), render::draw(&plain.model, level(&plain), filled));
    let (laid, _) = extra(&over, &bare);
    let Some(Prim::Fill { rings, .. }) = laid.iter().find(|p| matches!(p, Prim::Fill { .. })) else { panic!("no plate") };
    let corner = rings.iter().flatten().fold((0f32, 0f32), |(x, y), q| (x.max(q.0), y.max(q.1)));
    let inside = (corner.0 - 1.5, corner.1 - 1.5);
    assert_eq!(pixel(&render::png(&bare).unwrap(), inside), (255, 255, 0), "the photograph's south east corner, without a credit");
    assert_eq!(pixel(&render::png(&over).unwrap(), inside), (255, 255, 255), "the plate's paper, past the end of the lettering");
    // And on a sheet, a viewport is drawn at paper size, which has it too.
    let (on_sheet, bare) =
        (render::draw_at(&s.model, level(&s), window(), 1.0), render::draw_at(&plain.model, level(&plain), window(), 1.0));
    assert!(on_sheet.prims.len() > bare.prims.len());
}

#[test]
fn there_is_no_credit_without_the_photograph_in_the_window_the_category_or_a_plan_that_looks_up() {
    let (mut s, mut plain) = with_and_without("no-credit", "(c) Test Credit");
    let level = (level(&s), level(&plain));
    let count = |s: &Session, level, view| render::draw(&s.model, level, view).prims.len();
    assert!(count(&s, level.0, window()) > count(&plain, level.1, window()), "to begin with it has one");
    // A window the terrain is not in shows no photograph and so no credit; the same for a window of nothing.
    for far in [View { cx: 1e6, ..window() }, View { width: 0, height: 0, ..window() }, View { scale: f64::NAN, ..window() }] {
        assert_eq!(count(&s, level.0, far), count(&plain, level.1, far), "{far:?}");
    }
    // A view that hides the photograph, or the terrain, hides what is owed for it; a ceiling plan has neither.
    for both in [json!({"categories": {"aerialimagery": {"hidden": true}}}), json!({"categories": {"toposurface": {"hidden": true}}})] {
        let mut p = json!({"view": "Level 1"});
        p.as_object_mut().unwrap().extend(both.as_object().unwrap().clone());
        run(&mut s, "graphics", p.clone());
        run(&mut plain, "graphics", p);
        assert_eq!(count(&s, level.0, window()), count(&plain, level.1, window()));
        run(&mut s, "graphics", json!({"view": "Level 1", "reset": true}));
        run(&mut plain, "graphics", json!({"view": "Level 1", "reset": true}));
    }
    let up = |s: &Session, level| render::draw_ceiling(&s.model, level, window()).prims.len();
    assert_eq!(up(&s, level.0), up(&plain, level.1));
}

#[test]
fn a_long_credit_wraps_inside_a_narrow_window_and_a_word_wider_than_it_is_cut() {
    let words = "Orthophotos supplied by an imagery service under the terms of its licence, with thanks. ".repeat(3);
    for (what, credit) in [("words", words.as_str()), ("one word", &"W".repeat(300))] {
        let (s, plain) = with_and_without("wrap", credit);
        let narrow = View { width: 160, height: 160, ..window() };
        let (with, without) = (render::draw(&s.model, level(&s), narrow), render::draw(&plain.model, level(&plain), narrow));
        let (made, _) = extra(&with, &without);
        assert!(!made.is_empty(), "{what}");
        for p in made {
            for (x, y) in p.points() {
                assert!((0.0..=160.0).contains(&x) && (0.0..=160.0).contains(&y), "{what}: inside a window of 160 px: {x}, {y}");
            }
        }
        // More than one line: the lettering sits at more than one height.
        let tops: std::collections::BTreeSet<i32> =
            made.iter().filter(|p| matches!(p, Prim::Fill { .. })).flat_map(Prim::points).map(|(_, y)| y.round() as i32).collect();
        assert!(tops.len() > 2, "{what}: lines: {tops:?}");
    }
}

#[test]
fn a_section_letters_the_credit_too() {
    let (mut s, mut plain) = with_and_without("section-credit", "(c) Test Credit");
    let at = json!({"start": [-1000, 5000], "end": [11000, 5000], "name": "S"});
    let (a, b) = (run(&mut s, "section", at.clone())["id"].as_u64().unwrap(), run(&mut plain, "section", at)["id"].as_u64().unwrap());
    let view = View { scale: 0.06, cx: 5000.0, cy: 0.0, width: 800, height: 400 };
    let (with, without) = (render::section(&s.model, a, view), render::section(&plain.model, b, view));
    let (made, owners) = extra(&with, &without);
    assert!(!made.is_empty() && owners.iter().all(Option::is_none), "a plate and lettering, nobody's");
    for p in made {
        for (x, y) in p.points() {
            assert!(x > 400.0 && y > 300.0 && x <= 800.0 && y <= 400.0, "bottom right: {x}, {y}");
        }
    }
}

#[test]
fn the_credits_of_a_view_are_those_of_the_photographs_it_shows_once_each_and_cut_to_a_line() {
    use crate::graphics::Look;
    let (mut s, plain) = with_and_without("credits", "(c) Test Credit");
    let credits = |s: &Session| crate::imagery::credits(&s.model, &Look::of(&s.model, level(s)));
    assert_eq!(credits(&s), ["(c) Test Credit"]);
    assert!(credits(&plain).is_empty(), "no credit written");
    // The terrain hidden, its photograph is not shown.
    let terrain = *s.model.elements.iter().find(|(_, e)| e.category() == "toposurface").unwrap().0;
    run(&mut s, "graphics", json!({"view": "Level 1", "hide_elements": [terrain]}));
    assert!(credits(&s).is_empty());
    run(&mut s, "graphics", json!({"view": "Level 1", "reset": true}));
    // The same credit on two toposurfaces is given once; others, sorted, and no more than four.
    for (name, credit) in
        [("B", "(c) Test Credit"), ("C", "(c) Another"), ("D", "(c) Zed"), ("E", "(c) Yankee"), ("F", "(c) X-ray"), ("G", "(c) Whisky")]
    {
        let points: Vec<_> = (0..4).map(|i| json!([i * 1000, 0, 0])).chain([json!([0, 1000, 0])]).collect();
        let id = run(&mut s, "toposurface", json!({"points": points, "material": "Aerial", "name": name}))["id"].as_u64().unwrap();
        run(&mut s, "modify", json!({"id": id, "params": {"Imagery Credit": credit}}));
    }
    let all = credits(&s);
    assert_eq!(all.len(), crate::imagery::MAX_CREDITS, "{all:?}");
    assert!(all.windows(2).all(|w| w[0] < w[1]), "sorted, none twice: {all:?}");
    // A credit is one line of text, cut to a length: the user may have written anything in the parameter.
    credited(&mut s, "line one\n\tline\u{7}  two ");
    assert_eq!(credits(&s), ["line one line two"]);
    credited(&mut s, &"long ".repeat(200));
    assert!(credits(&s)[0].chars().count() <= crate::imagery::MAX_CREDIT);
    // Something that is not text is no credit and no trouble.
    for e in s.model_mut().elements.values_mut() {
        if let crate::model::Element::Toposurface(t) = e {
            t.params.insert("Imagery Credit".into(), json!(5));
        }
    }
    assert!(credits(&s).is_empty());
}

#[test]
fn a_3d_picture_is_stamped_with_the_credit_in_its_corner_and_only_there() {
    use crate::graphics::Look;
    use crate::view3d::Scene;
    let (s, plain) = with_and_without("stamp", "(c) Test Credit");
    let (w, h) = (400u32, 300u32);
    let make = |s: &Session, stamp: bool| {
        let look = Look::of_3d(&s.model);
        let scene = Scene::new(&s.model);
        let cam = scene.fit_in(30.0, 30.0, w, h, &look);
        let mut img = scene.render_in(&cam, &[], &look);
        if stamp {
            render::stamp_credits(&s.model, &look, &mut img);
        }
        img
    };
    let (bare, stamped) = (make(&s, false), make(&s, true));
    let differing: Vec<usize> = (0..(w * h) as usize).filter(|k| bare.rgba[k * 4..k * 4 + 4] != stamped.rgba[k * 4..k * 4 + 4]).collect();
    assert!(differing.len() > 60, "lettering and a plate: {} pixels", differing.len());
    assert!(differing.iter().all(|k| (*k as u32 % w) > w / 2 && (*k as u32 / w) > h * 2 / 3), "only in the bottom right corner");
    // Without a credit it changes nothing.
    let (bare, unmarked) = (make(&plain, false), make(&plain, true));
    assert_eq!(bare.rgba, unmarked.rgba);
}

#[test]
fn the_render_command_stamps_the_credit_on_its_3d_picture() {
    let (mut s, mut plain) = with_and_without("render-credit", "(c) Test Credit");
    let png = |s: &mut Session| {
        let r = run(s, "render", json!({"view": "{3D}", "width": 400, "height": 300}));
        crate::texture::unbase64(r["pngBase64"].as_str().unwrap()).unwrap()
    };
    let (a, b) = (png(&mut s), png(&mut plain));
    let (pa, pb) = (tiny_skia::Pixmap::decode_png(&a).unwrap(), tiny_skia::Pixmap::decode_png(&b).unwrap());
    let differing = pa.data().chunks_exact(4).zip(pb.data().chunks_exact(4)).filter(|(x, y)| x != y).count();
    assert!(differing > 60, "{differing} pixels differ");
}

// ---- Aligning the photograph with the map: shifted from where the fetch put it ----

/// Where the photograph's west and north corner is, mm.
fn offset(s: &Session) -> [f64; 2] {
    s.model.materials["Aerial"].texture.as_ref().unwrap().offset
}

#[test]
fn the_photograph_is_shifted_from_where_the_fetch_put_it_and_not_from_where_it_is() {
    let mut s = terrained(&picture("shift", 8, 8, quadrants), square(), 0.0);
    assert_eq!(offset(&s), [0.0, 10000.0], "the terrain's west and north corner");
    let r = run(&mut s, "shift_imagery", json!({"east": 2000, "north": 1000}));
    assert_eq!((r["east"].as_f64(), r["north"].as_f64()), (Some(2000.0), Some(1000.0)));
    assert_eq!(offset(&s), [2000.0, 11000.0]);
    // The same again is the same, not twice as far.
    run(&mut s, "shift_imagery", json!({"east": 2000, "north": 1000}));
    assert_eq!(offset(&s), [2000.0, 11000.0]);
    // One of the two given keeps the other where it is.
    let r = run(&mut s, "shift_imagery", json!({"east": -500}));
    assert_eq!((offset(&s), r["north"].as_f64()), ([-500.0, 11000.0], Some(1000.0)));
    // Reset puts it back, and says so.
    let r = run(&mut s, "shift_imagery", json!({"reset": true}));
    assert_eq!((offset(&s), r["east"].as_f64(), r["north"].as_f64()), ([0.0, 10000.0], Some(0.0), Some(0.0)));
    // One step to undo.
    run(&mut s, "shift_imagery", json!({"east": 3000}));
    assert_eq!(offset(&s), [3000.0, 10000.0]);
    run(&mut s, "undo", json!({}));
    assert_eq!(offset(&s), [0.0, 10000.0]);
}

#[test]
fn the_photograph_in_the_plan_moves_with_the_shift_under_a_terrain_that_stays() {
    let mut s = terrained(&picture("shift-plan", 8, 8, quadrants), square(), 0.0);
    let (west, east_of_middle) = (P2(1000.0, 7500.0), P2(6000.0, 7500.0));
    let drawn = |s: &Session| render::draw(&s.model, level(s), window());
    let d = drawn(&s);
    assert!(is(under(&d, west), RED) && is(under(&d, east_of_middle), BLUE));
    run(&mut s, "shift_imagery", json!({"east": 2000}));
    let d = drawn(&s);
    // The north west quarter now runs from 2 m to 7 m: the west strip has none, and 6 m east is red.
    assert!(under(&d, west).is_none_or(|c| c[3] == 0), "{:?}", under(&d, west));
    assert!(is(under(&d, east_of_middle), RED), "{:?}", under(&d, east_of_middle));
}

#[test]
fn what_cannot_be_shifted_is_refused_naming_why_and_leaves_the_project_as_it_was() {
    let mut s = terrained(&picture("shift-bad", 8, 8, quadrants), square(), 0.0);
    let before = s.model.clone();
    for (p, says) in [
        (json!({}), "reset"),
        (json!({"reset": true, "east": 5}), "reset"),
        (json!({"reset": "yes"}), "reset"),
        (json!({"east": "far"}), "east"),
        (json!({"north": []}), "north"),
        (json!({"east": 1e300}), "east"),
        (json!({"north": -1e12}), "north"),
        (json!({"east": 2_000_000}), "beyond"),
        (json!({"north": -1_000_001}), "beyond"),
        (json!({"east": 1, "colour": 2}), "colour"),
        (json!({"east": 1, "id": 999_999}), "999999"),
    ] {
        let why = fails(&mut s, "shift_imagery", p.clone());
        assert!(why.contains(says), "{p}: {why}");
        assert_eq!(s.model, before, "{p}");
    }
    // Terrain that wears no photograph has none to shift.
    let mut bare = Session::new();
    run(&mut bare, "toposurface", json!({"points": [[0, 0, 0], [10000, 0, 0], [10000, 10000, 0]]}));
    assert!(fails(&mut bare, "shift_imagery", json!({"east": 1})).contains("aerial imagery"));
    // Two toposurfaces and no id: which?
    let points: Vec<_> = (0..4).map(|i| json!([i * 1000, 0, 0])).chain([json!([0, 1000, 0])]).collect();
    run(&mut s, "toposurface", json!({"points": points, "material": "Aerial", "name": "Other"}));
    assert!(fails(&mut s, "shift_imagery", json!({"east": 1})).contains("`id`"));
}
