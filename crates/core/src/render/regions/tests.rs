//! The ink a non-masking region keeps above it: in elevations, on sheets, along a region's edge, and what it costs.

use web_time::{Duration, Instant};

use serde_json::{Value, json};

use super::*;
use crate::engine::Session;
use crate::fills::SOLID_FILL;
use crate::render;
use crate::sheet;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A square's lines, drawn round from `[x, y]`.
fn square(x: f64, y: f64, size: f64) -> Vec<Value> {
    rect(x, y, x + size, y + size)
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Value> {
    let c = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    (0..4).map(|i| json!([c[i], c[(i + 1) % 4]])).collect()
}

/// A star's lines: `n` corners (an even number) alternately `r1` and `r2` from `at`, so it's concave.
fn star(at: P2, n: usize, r1: f64, r2: f64) -> Vec<Value> {
    let corner = |i: usize| {
        let (r, a) = (if i.is_multiple_of(2) { r1 } else { r2 }, std::f64::consts::TAU * i as f64 / n as f64);
        [(at.0 + r * a.cos()).round(), (at.1 + r * a.sin()).round()]
    };
    (0..n).map(|i| json!([corner(i), corner((i + 1) % n)])).collect()
}

fn pixels(png: &[u8]) -> tiny_skia::Pixmap {
    tiny_skia::Pixmap::decode_png(png).unwrap()
}

/// How many pixels of `pm` are dark, in the box from `lo` to `hi` (px).
fn dark(pm: &tiny_skia::Pixmap, (lo, hi): ((f32, f32), (f32, f32))) -> usize {
    let (x0, x1, y0, y1) = (lo.0.min(hi.0) as u32, lo.0.max(hi.0) as u32, lo.1.min(hi.1) as u32, lo.1.max(hi.1) as u32);
    (y0..=y1)
        .flat_map(|y| (x0..=x1).map(move |x| (x, y)))
        .filter_map(|(x, y)| pm.pixel(x, y))
        .filter(|c| c.red() < 100 && c.green() < 100 && c.blue() < 100)
        .count()
}

/// A south wall, 3 m tall, and an elevation looking at it; the elevation's id, and where its line starts.
fn south_elevation(s: &mut Session) -> (u64, P2) {
    run(s, "wall", json!({"points": [[0, 0], [6000, 0]], "height": 3000}));
    let id = run(s, "elevation", json!({"facing": "south"}))["id"].as_u64().unwrap();
    let Some(Element::Elevation(e)) = s.model.elements.get(id) else { panic!("no elevation") };
    (id, e.start)
}

fn blue(s: &mut Session, masking: bool) {
    run(s, "filled_region_type", json!({"name": "Blue", "foreground": SOLID_FILL, "foreground_color": [0, 0, 255], "masking": masking}));
}

/// The boxes round the wall's edges in the elevation (its two ends, its top and its bottom), in the view's pixels.
fn edges(view: View, start: P2) -> [((f32, f32), (f32, f32)); 4] {
    let at = |x: f64, z: f64| view.px(P2(x - start.0, z));
    [
        (at(-130.0, 500.0), at(130.0, 2500.0)),
        (at(5870.0, 500.0), at(6130.0, 2500.0)),
        (at(1000.0, 2850.0), at(5000.0, 3150.0)),
        (at(1000.0, -150.0), at(5000.0, 150.0)),
    ]
}

#[test]
fn an_elevation_keeps_the_ink_it_shows_above_a_non_masking_region() {
    let mut s = Session::new();
    let (id, start) = south_elevation(&mut s);
    blue(&mut s, false);
    let view = render::section_fit(&s.model, id, 800, 600);
    let plain = pixels(&render::section_png(&s.model, id, view).unwrap());
    let boxes = edges(view, start);
    assert!(boxes.iter().all(|b| dark(&plain, *b) > 10), "the wall's edges are drawn");
    let sketch = square(-200.0 - start.0, -600.0, 6.6e3);
    run(&mut s, "filled_region", json!({"view": id, "type": "Blue", "line_style": null, "sketch": sketch}));
    let covered = pixels(&render::section_png(&s.model, id, view).unwrap());
    for (i, b) in boxes.iter().enumerate() {
        assert!(dark(&covered, *b) > 0, "edge {i} stays above the blue");
    }
    assert!(dark(&covered, boxes[0]) * 10 >= dark(&plain, boxes[0]) * 9, "the whole of an end");
    let (x, y) = view.px(P2(3000.0 - start.0, 1500.0));
    let c = covered.pixel(x as u32, y as u32).unwrap();
    assert_eq!([c.red(), c.green(), c.blue()], [0, 0, 255], "and the blue is there");
    // A masking one hides them.
    blue(&mut s, true);
    let masked = pixels(&render::section_png(&s.model, id, view).unwrap());
    assert!(boxes.iter().all(|b| dark(&masked, *b) == 0), "a masking region covers them");
}

#[test]
fn a_section_viewport_on_a_sheet_keeps_the_ink_above_a_region_in_it() {
    let mut s = Session::new();
    let (id, start) = south_elevation(&mut s);
    blue(&mut s, false);
    let sketch = square(-200.0 - start.0, -600.0, 6.6e3);
    run(&mut s, "filled_region", json!({"view": id, "type": "Blue", "line_style": null, "sketch": sketch}));
    let sheet_id = run(&mut s, "sheet", json!({"name": "Views"}))["id"].as_u64().unwrap();
    let vp = run(&mut s, "place", json!({"sheet": "A101", "view": id, "at": [200, 150], "scale": 100}))["id"].as_u64().unwrap();
    let (frame, (ox, oy)) = sheet::frame(&s.model, vp).unwrap();
    let on_sheet = |b: ((f32, f32), (f32, f32))| ((b.0.0 + ox, b.0.1 + oy), (b.1.0 + ox, b.1.1 + oy));
    let boxes = edges(frame, start).map(on_sheet);
    let covered = pixels(&render::png(&sheet::draw(&s.model, sheet_id).unwrap()).unwrap());
    for (i, b) in boxes.iter().enumerate() {
        assert!(dark(&covered, *b) > 0, "edge {i} stays above the blue, on the sheet");
    }
    blue(&mut s, true);
    let masked = pixels(&render::png(&sheet::draw(&s.model, sheet_id).unwrap()).unwrap());
    assert!(boxes.iter().all(|b| dark(&masked, *b) == 0), "a masking region covers them on the sheet");
}

#[test]
fn a_region_on_a_sheet_draws_over_its_viewports_and_keeps_none_of_their_ink() {
    // A limit, kept as documented: annotation on the sheet itself sits over what the viewports drew (Revit has no regions on sheets).
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]]}));
    blue(&mut s, false);
    let sheet_id = run(&mut s, "sheet", json!({"name": "Plans"}))["id"].as_u64().unwrap();
    run(&mut s, "place", json!({"sheet": "A101", "view": 100000, "at": [200, 150], "scale": 100}));
    run(&mut s, "filled_region", json!({"view": sheet_id, "type": "Blue", "line_style": null, "sketch": rect(150.0, 100.0, 250.0, 200.0)}));
    let d = sheet::draw(&s.model, sheet_id).unwrap();
    let pm = pixels(&render::png(&d).unwrap());
    let (x, y) = d.view.px(P2(200.0, 150.0));
    let c = pm.pixel(x as u32, y as u32).unwrap();
    assert_eq!([c.red(), c.green(), c.blue()], [0, 0, 255], "the region is drawn");
    assert_eq!(dark(&pm, ((x - 40.0, y - 40.0), (x + 40.0, y + 40.0))), 0, "and nothing of the plan shows through");
}

/// A south wall's outside face, and the dark pixels along it from x = 1000 to 3000 once a blue region has its edge `off` mm
/// off the face (into the wall when it's positive), more regions after it.
fn along_a_face(off: f64, then: &[(f64, f64)]) -> (usize, usize) {
    let (mut s, face) = face_session();
    let view = View { angle: 0.0, scale: 0.1, cx: 2500.0, cy: face, width: 700, height: 300 };
    let band = |s: &Session| {
        let d = render::draw(&s.model, 100000, view);
        let pm = pixels(&render::png(&d).unwrap());
        dark(&pm, (view.px(P2(1000.0, face + 30.0)), view.px(P2(3000.0, face - 30.0))))
    };
    let plain = band(&s);
    face_regions(&mut s, face, off, then);
    (band(&s), plain)
}

/// A south wall, and where its outside face is.
fn face_session() -> (Session, f64) {
    let mut s = Session::new();
    let wall = run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]]}))["ids"][0].as_u64().unwrap();
    let face = crate::walls::outline(&s.model, wall).iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    blue(&mut s, false);
    run(&mut s, "filled_region_type", json!({"name": "Red", "foreground": SOLID_FILL, "foreground_color": [255, 0, 0]}));
    (s, face)
}

/// A blue region from x = 1000 to 3000 with its edge `off` mm off the face, then a red one for each of `then`.
fn face_regions(s: &mut Session, face: f64, off: f64, then: &[(f64, f64)]) {
    let sketch = rect(1000.0, face + off - 1000.0, 3000.0, face + off);
    run(s, "filled_region", json!({"type": "Blue", "line_style": null, "sketch": sketch}));
    for (x0, x1) in then {
        let sketch = rect(*x0, face - 1000.0, *x1, face);
        run(s, "filled_region", json!({"type": "Red", "line_style": null, "sketch": sketch}));
    }
}

#[test]
fn a_region_with_its_edge_on_a_wall_face_keeps_the_face_whole() {
    for off in [-2.0, -0.4, 0.0, 0.4, 2.0] {
        let (kept, plain) = along_a_face(off, &[]);
        assert!(plain > 50, "the face is drawn: {plain}");
        assert!(kept * 10 >= plain * 9, "a region's edge {off} mm from the face keeps {kept} of {plain} dark pixels");
    }
}

#[test]
fn two_overlapping_non_masking_regions_both_keep_the_ink() {
    let (kept, plain) = along_a_face(0.0, &[(2000.0, 4000.0)]);
    assert!(kept * 10 >= plain * 9, "{kept} of {plain}");
}

#[test]
fn what_a_region_lets_through_of_an_outline_is_one_line_with_its_corners() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "closed": true}));
    run(&mut s, "filled_region_type", json!({"name": "Blue", "foreground": SOLID_FILL, "foreground_color": [0, 0, 255]}));
    let view = View { angle: 0.0, scale: 0.1, cx: 2000.0, cy: 1500.0, width: 600, height: 400 };
    let before = render::draw(&s.model, 100000, view).prims.len();
    run(&mut s, "filled_region", json!({"type": "Blue", "line_style": null, "sketch": square(-1000.0, -1000.0, 2500.0)}));
    let d = render::draw(&s.model, 100000, view);
    let corner = view.px(P2(0.0, 0.0));
    let at = |q: &(f32, f32)| (q.0 - corner.0).abs() < 25.0 && (q.1 - corner.1).abs() < 25.0;
    // Lines the region kept, past what the plan drew without it: one of them goes round the corner.
    let round = d.prims.iter().skip(before).any(|p| matches!(p, Prim::Line { pts, .. } if pts.len() >= 3 && pts.iter().any(at)));
    assert!(round, "a corner inside the region is one line, not two that meet");
}

/// Whether `pieces` (lines along y = 50, as `Area::clip` gives them) draw at `x`, by their own dashes.
fn drawn_at(pieces: &[Prim], x: f32) -> bool {
    pieces.iter().any(|p| {
        let Prim::Line { pts, dash, .. } = p else { return false };
        let (a, b) = (pts[0].0, pts[pts.len() - 1].0);
        if x < a || x > b {
            return false;
        }
        match dash {
            None => true,
            Some(d) => {
                let (cycle, on) = (d.iter().sum::<f32>(), d[0]);
                (x - a).rem_euclid(cycle) < on
            }
        }
    })
}

#[test]
fn a_dashed_line_a_region_takes_part_of_keeps_its_dashes_where_they_were() {
    // 10 px on and 5 off from the line's start; a region from x = 103 on takes the rest of it.
    let line =
        Prim::Line { pts: vec![(0.0, 50.0), (400.0, 50.0)], width: 1.0, color: Rgb(0, 0, 0), dash: Some(vec![10.0, 5.0]), closed: false };
    let view = View { angle: 0.0, scale: 1.0, cx: 200.0, cy: 0.0, width: 400, height: 100 };
    let ring = vec![vec![P2(103.0, -100.0), P2(500.0, -100.0), P2(500.0, 100.0), P2(103.0, 100.0)]];
    let masks: Vec<Mask> = Vec::new();
    let pieces = Area::new(&ring, &masks).unwrap().clip(&line, view, 1.0);
    assert!(!pieces.is_empty());
    for k in 104..399 {
        let x = k as f32 + 0.5;
        assert_eq!(drawn_at(&pieces, x), x.rem_euclid(15.0) < 10.0, "x = {x}");
    }
}

#[test]
fn a_picture_at_twice_the_pixels_gives_ink_in_the_pixels_of_the_view() {
    let mut s = Session::new();
    let (id, start) = south_elevation(&mut s);
    blue(&mut s, false);
    let region = run(
        &mut s,
        "filled_region",
        json!({"view": id, "type": "Blue", "line_style": null, "sketch": square(-200.0 - start.0, -600.0, 6.6e3)}),
    )["id"]
        .as_u64()
        .unwrap();
    let view = render::section_fit(&s.model, id, 400, 300);
    let big = View { scale: view.scale * 2.0, width: view.width * 2, height: view.height * 2, ..view };
    let picture = render::section_image(&s.model, &crate::view3d::Scene::new(&s.model), id, big, &[]).unwrap();
    let d = render::section_over(&s.model, id, view, &picture);
    let ink: Vec<&Prim> = d
        .prims
        .iter()
        .zip(&d.owners)
        .zip(&d.parts)
        .filter(|((_, o), part)| **o == Some(region) && **part == Part::Pattern)
        .map(|((p, _), _)| p)
        .collect();
    let runs: Vec<&Prim> = ink.into_iter().filter(|p| matches!(p, Prim::Line { .. })).collect();
    // The wall's two ends are vertical lines (a run on a run, a line), its top and bottom rows of horizontal ones.
    assert!(runs.len() >= 4, "{} runs", runs.len());
    let mut vertical = 0;
    for p in runs {
        let Prim::Line { pts, width, .. } = p else { continue };
        // One pixel of the picture is half a pixel of the view: its width is the picture's pixel's, and it lies on the
        // picture's row (a horizontal run) or column (a vertical one).
        assert!((width - 0.5).abs() < 1e-6, "{width}");
        if (pts[0].0 - pts[1].0).abs() < 1e-6 {
            vertical += 1;
            assert!(((pts[0].0 * 2.0).fract() - 0.5).abs() < 1e-3, "{pts:?}");
        } else {
            assert!(((pts[0].1 * 2.0).fract() - 0.5).abs() < 1e-3, "{pts:?}");
        }
    }
    assert!(vertical >= 2, "{vertical} vertical lines");
}

/// An elevation of 200 walls 20 m tall, 100 mm wide and 200 mm apart, with a non-masking region over all of it, if `region`.
fn many_walls(region: bool) -> (Session, u64) {
    let mut s = Session::new();
    for i in 0..200 {
        let x = f64::from(i) * 200.0;
        run(&mut s, "wall", json!({"points": [[x, 0], [x + 100.0, 0]], "height": 20000}));
    }
    let id = run(&mut s, "elevation", json!({"facing": "south"}))["id"].as_u64().unwrap();
    let Some(Element::Elevation(e)) = s.model.elements.get(id) else { panic!("no elevation") };
    let start = e.start;
    blue(&mut s, false);
    if region {
        let sketch = rect(-1000.0 - start.0, -1000.0, 41000.0 - start.0, 21000.0);
        run(&mut s, "filled_region", json!({"view": id, "type": "Blue", "line_style": null, "sketch": sketch}));
    }
    (s, id)
}

#[test]
fn an_elevation_of_many_walls_keeps_all_its_ink_above_a_region_at_a_big_size() {
    // The picture's dark pixels were 700,000 of runs one pixel high; the first 250,000 were kept and the rest of the rows had none.
    let all = ((0.0, 0.0), (3999.0, 2599.0));
    let (with, id) = many_walls(true);
    let view = render::section_fit(&with.model, id, 4000, 2600);
    let kept = dark(&pixels(&render::section_png(&with.model, id, view).unwrap()), all);
    let (without, id) = many_walls(false);
    let plain = dark(&pixels(&render::section_png(&without.model, id, view).unwrap()), all);
    eprintln!("{kept} of {plain} dark pixels kept above the region");
    assert!(plain > 600_000, "{plain} dark pixels in the picture");
    assert!(kept * 10 >= plain * 9, "{kept} of {plain} dark pixels kept above the region");
}

#[test]
fn a_picture_with_more_runs_of_ink_than_are_kept_says_so_in_the_drawings_notes() {
    let mut s = Session::new();
    let (id, start) = south_elevation(&mut s);
    blue(&mut s, false);
    run(
        &mut s,
        "filled_region",
        json!({"view": id, "type": "Blue", "line_style": null, "sketch": square(-200.0 - start.0, -600.0, 6.6e3)}),
    );
    let view = render::section_fit(&s.model, id, 800, 600);
    assert!(render::section(&s.model, id, view).notes.is_empty(), "the wall's runs are few");
    // Kept to two, the wall's edges lose their ink under the region, and the drawing says so.
    super::MOST.with(|m| m.set(2));
    let notes = render::section(&s.model, id, view).notes;
    super::MOST.with(|m| m.set(super::MAX_RUNS));
    assert!(notes.len() == 1 && notes[0].contains("runs of ink"), "{notes:?}");
}

/// The time `f` takes (to look at: no test asserts a time, which varies with the load of the machine).
fn timed<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let at = Instant::now();
    (f(), at.elapsed())
}

/// What `f` costs in steps: the points of rings it cut lines by ([`super::spend`]) and the polygons it cut ([`walls::SPLITS`]).
fn counted<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let now = || super::SPENT.with(std::cell::Cell::get) + crate::walls::SPLITS.with(std::cell::Cell::get);
    let before = now();
    (f(), now() - before)
}

/// The pictures of sections `f` draws.
fn pictured<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let before = super::PICTURES.with(std::cell::Cell::get);
    (f(), super::PICTURES.with(std::cell::Cell::get) - before)
}

/// A tower of `storeys` storeys, 42 walls each, and an elevation of it (or a section across its middle) with a non-masking region
/// over its middle.
fn tower(storeys: u64, cut: bool, region: bool) -> (Session, u64) {
    let mut s = Session::new();
    run(&mut s, "highrise", json!({"storeys": storeys}));
    let id = if cut {
        run(&mut s, "section", json!({"start": [-1000, 18000], "end": [49000, 18000]}))
    } else {
        run(&mut s, "elevation", json!({"facing": "south"}))
    }["id"]
        .as_u64()
        .unwrap();
    blue(&mut s, false);
    let start = s.model.section(id).map(|v| v.start).unwrap_or_default();
    if region {
        run(
            &mut s,
            "filled_region",
            json!({"view": id, "type": "Blue", "line_style": null, "sketch": rect(10000.0 - start.0, 2000.0, 30000.0 - start.0, 20000.0)}),
        );
    }
    (s, id)
}

#[test]
fn a_section_with_the_picture_in_hand_draws_its_region_ink_without_drawing_the_picture_again() {
    let (s, id) = tower(5, true, true);
    let view = render::section_fit(&s.model, id, 800, 600);
    let picture = render::section_image(&s.model, &crate::view3d::Scene::new(&s.model), id, view, &[]).unwrap();
    let (with, drawn) = pictured(|| render::section_over(&s.model, id, view, &picture));
    let (alone, drawn_alone) = pictured(|| render::section(&s.model, id, view));
    assert_eq!(with.prims.len(), alone.prims.len(), "the same ink either way");
    assert_eq!((drawn, drawn_alone), (0, 1), "the picture in hand is not drawn again; without it, it is drawn once");
}

#[test]
#[ignore = "a probe: cargo test -p bimcraft-core --lib probe -- --ignored --nocapture"]
fn probe_the_cost_of_a_section_with_a_region() {
    for (storeys, cut) in [(1, false), (1, true), (20, false), (20, true)] {
        let (s, id) = tower(storeys, cut, true);
        let view = render::section_fit(&s.model, id, 1200, 800);
        let picture = render::section_image(&s.model, &crate::view3d::Scene::new(&s.model), id, view, &[]).unwrap();
        let (_, slow) = timed(|| render::section(&s.model, id, view));
        let (_, fast) = timed(|| render::section_over(&s.model, id, view, &picture));
        let (bare, id) = tower(storeys, cut, false);
        let (_, none) = timed(|| render::section(&bare.model, id, view));
        let what = if cut { "section" } else { "elevation" };
        eprintln!("{} walls, {what}: without a region {none:?}, with one {slow:?}, with one over its picture {fast:?}", 42 * storeys);
    }
}

/// A plan with two overlapping concave masks of `n` corners, and a non-masking region over them and a wall.
fn masked(n: usize) -> Session {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[-3000, 100], [3000, 100]]}));
    blue(&mut s, false);
    for (dx, dy) in [(0.0, 0.0), (300.0, 200.0)] {
        run(&mut s, "filled_region", json!({"type": "Masking", "line_style": null, "sketch": star(P2(dx, dy), n, 2800.0, 2000.0)}));
    }
    run(&mut s, "filled_region", json!({"type": "Blue", "line_style": null, "sketch": star(P2(150.0, 100.0), n, 3000.0, 1800.0)}));
    s
}

#[test]
fn overlapping_concave_masks_cost_a_draw_milliseconds_not_seconds() {
    let s = masked(400);
    let view = View { angle: 0.0, scale: 0.1, cx: 0.0, cy: 0.0, width: 700, height: 700 };
    let (d, steps) = counted(|| render::draw(&s.model, 100000, view));
    eprintln!("400 corners: {steps} steps, {} prims", d.prims.len());
    assert!(steps <= 8_000, "{steps} steps");
}

#[test]
#[ignore = "a probe: cargo test -p bimcraft-core --lib probe -- --ignored --nocapture"]
fn probe_the_cost_of_overlapping_concave_masks() {
    for n in [50, 100, 200, 400] {
        let s = masked(n);
        let view = View { angle: 0.0, scale: 0.1, cx: 0.0, cy: 0.0, width: 700, height: 700 };
        let (_, took) = timed(|| render::draw(&s.model, 100000, view));
        eprintln!("{n} corners: {took:?}");
    }
}

/// A plan with a wall, and a 100 m strip of the non-masking Blue region over it with `holes` small square holes along it, and
/// `masks` small masking regions drawn before it, all of them a long way from the wall.
fn strip(holes: usize, masks: usize) -> Session {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [2000, 0]]}));
    blue(&mut s, false);
    let at = |i: usize| 10000.0 + 900.0 * i as f64;
    for i in 0..masks {
        run(&mut s, "filled_region", json!({"type": "Masking", "line_style": null, "sketch": square(at(i), -100.0, 200.0)}));
    }
    let mut sketch = rect(-1000.0, -500.0, 100000.0, 500.0);
    (0..holes).for_each(|i| sketch.extend(square(at(i), -100.0, 200.0)));
    run(&mut s, "filled_region", json!({"type": "Blue", "line_style": null, "sketch": sketch}));
    s
}

/// What drawing `s`'s first plan costs in steps ([`super::spend`] only), the whole strip in view.
fn drawn(s: &Session) -> usize {
    let view = View { angle: 0.0, scale: 0.007, cx: 50000.0, cy: 0.0, width: 700, height: 700 };
    let before = super::SPENT.with(std::cell::Cell::get);
    let d = render::draw(&s.model, 100000, view);
    eprintln!("{} prims", d.prims.len());
    super::SPENT.with(std::cell::Cell::get) - before
}

#[test]
fn the_holes_and_masks_far_from_a_line_cost_it_nothing() {
    let (none, holes, masks) = (drawn(&strip(0, 0)), drawn(&strip(100, 0)), drawn(&strip(0, 100)));
    eprintln!("steps: no holes or masks {none}, 100 holes {holes}, 100 masks {masks}");
    // Only the rings and masks a line comes near are looked into: the ink of the wall does not run through the other hundred.
    assert!(holes <= none + 200, "{holes} steps with 100 holes far off, {none} without");
    assert!(masks <= none + 200, "{masks} steps with 100 masks far off, {none} without");
}

/// A plan with one region of `n` corners with a hole of `n` corners in it, of the type `of`.
fn holed(n: usize, of: &str) -> Session {
    let mut s = Session::new();
    let mut sketch = star(P2(0.0, 0.0), n, 3000.0, 2200.0);
    sketch.extend(star(P2(0.0, 0.0), n, 1600.0, 1000.0));
    run(&mut s, "filled_region", json!({"type": of, "line_style": null, "sketch": sketch}));
    s
}

#[test]
fn a_hatched_region_with_a_big_hole_is_not_cut_into_convex_pieces() {
    let s = holed(400, "Diagonal Up");
    let view = View { angle: 0.0, scale: 0.1, cx: 0.0, cy: 0.0, width: 700, height: 700 };
    let (d, steps) = counted(|| render::draw(&s.model, 100000, view));
    eprintln!("hatch, 400 corners: {steps} steps, {} prims", d.prims.len());
    assert!(d.prims.len() > 100, "its hatch is drawn");
    assert_eq!(steps, 0, "a hatch runs through the rings as they are, and cuts no polygon: {steps} steps");
}

#[test]
fn a_solid_region_with_a_big_hole_is_cut_into_pieces_quickly() {
    let s = holed(400, "Solid Black");
    let view = View { angle: 0.0, scale: 0.1, cx: 0.0, cy: 0.0, width: 700, height: 700 };
    let (_, steps) = counted(|| render::draw(&s.model, 100000, view));
    eprintln!("solid, 400 corners: {steps} steps");
    assert!(steps <= 150_000, "{steps} steps");
}

#[test]
#[ignore = "a probe: cargo test -p bimcraft-core --lib probe -- --ignored --nocapture"]
fn probe_the_cost_of_a_big_hole() {
    for n in [100, 200, 400, 800] {
        let view = View { angle: 0.0, scale: 0.1, cx: 0.0, cy: 0.0, width: 700, height: 700 };
        for of in ["Diagonal Up", "Solid Black", "Masking"] {
            let s = holed(n, of);
            let (_, took) = timed(|| render::draw(&s.model, 100000, view));
            eprintln!("{n} corners, {of}: {took:?}");
        }
    }
}

/// The pictures of the cases above, to look at: `DUMP=/some/folder cargo test -p bimcraft-core --lib probe_pictures -- --ignored`.
#[test]
#[ignore = "a probe: DUMP=/some/folder cargo test -p bimcraft-core --lib probe_pictures -- --ignored"]
fn probe_pictures() {
    let Ok(dir) = std::env::var("DUMP") else { return };
    let save = |name: &str, png: Vec<u8>| std::fs::write(format!("{dir}/{name}.png"), png).unwrap();
    // The elevation, plain and under a region.
    let mut s = Session::new();
    let (id, start) = south_elevation(&mut s);
    blue(&mut s, false);
    let view = render::section_fit(&s.model, id, 800, 600);
    save("elevation_plain", render::section_png(&s.model, id, view).unwrap());
    run(
        &mut s,
        "filled_region",
        json!({"view": id, "type": "Blue", "line_style": null, "sketch": square(-200.0 - start.0, -600.0, 6.6e3)}),
    );
    save("elevation_region", render::section_png(&s.model, id, view).unwrap());
    // The same on a sheet.
    let sheet_id = run(&mut s, "sheet", json!({"name": "Views"}))["id"].as_u64().unwrap();
    run(&mut s, "place", json!({"sheet": "A101", "view": id, "at": [200, 150], "scale": 100}));
    save("sheet_viewport", render::png(&sheet::draw(&s.model, sheet_id).unwrap()).unwrap());
    // A region's edge on a wall face, in plan, three ways: on it, 2 mm outside it, and 2 mm in.
    for (name, off) in [("on", 0.0), ("outside", -2.0), ("inside", 2.0)] {
        let (mut s, face) = face_session();
        face_regions(&mut s, face, off, &[(2000.0, 4000.0)]);
        let view = View { angle: 0.0, scale: 0.25, cx: 2500.0, cy: face, width: 700, height: 200 };
        save(&format!("face_{name}"), render::png(&render::draw(&s.model, 100000, view)).unwrap());
    }
}

#[test]
fn a_sections_picture_has_paper_and_ink_and_no_blended_pixels_for_a_region_to_make_opaque() {
    // Pinned: the ink a region keeps is the picture's pixels as they are, so a fringe of blended colour would be kept as ink.
    let mut s = Session::new();
    let (id, _) = south_elevation(&mut s);
    let view = render::section_fit(&s.model, id, 800, 600);
    let picture = render::section_image(&s.model, &crate::view3d::Scene::new(&s.model), id, view, &[]).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for y in 0..picture.height {
        for x in 0..picture.width {
            let c = picture.color(f64::from(x), f64::from(y));
            seen.insert((c.0, c.1, c.2));
        }
    }
    assert_eq!(seen.len(), 2, "{seen:?}");
}
