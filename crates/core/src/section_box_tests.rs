//! A 3D view's section box (fork issue 1): six planes square to the axes that keep what lies between them, as Revit's Section
//! Box does, so a scan's floors and ceilings can be cut away to show the walls. Driven through the commands every front end
//! drives, and tested through the picture.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::graphics::Look;
use crate::render::Rgb;
use crate::view3d::{AZIMUTH, Camera, ELEVATION, Image, Scene, gpu};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} was taken: {v}"),
        Err(e) => e,
    }
}

/// A scan file in a folder of this test's own.
fn scan(test: &str, text: &str) -> String {
    let dir = std::env::temp_dir().join(format!("bimcraft-box-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path: PathBuf = dir.join("floors.xyz");
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

/// Metres: a red floor 2 m square, and a blue ceiling 1.5 m over it. The scan comes in with its foot on the level, so the
/// floor is at 0 and the ceiling at 1500 mm.
fn room() -> String {
    let mut text = String::new();
    for (z, [r, g, b]) in [(0.0, [255, 0, 0]), (1.5, [0, 0, 255])] {
        for i in 0..=100 {
            for k in 0..=100 {
                text += &format!("{} {} {z} {r} {g} {b}\n", f64::from(i) * 0.02, f64::from(k) * 0.02);
            }
        }
    }
    text
}

fn red(c: Rgb) -> bool {
    c.0 > 200 && c.1 < 60 && c.2 < 60
}

fn blue(c: Rgb) -> bool {
    c.0 < 60 && c.1 < 60 && c.2 > 200
}

fn pixels(img: &Image, colour: impl Fn(Rgb) -> bool) -> usize {
    let (w, h) = (img.width, img.height);
    (0..h).flat_map(|y| (0..w).map(move |x| (f64::from(x) + 0.5, f64::from(y) + 0.5))).filter(|&(x, y)| colour(img.color(x, y))).count()
}

/// The default 3D view's picture through `cam`, as its graphics and its section box say.
fn picture(s: &Session, cam: &Camera) -> Image {
    Scene::new(&s.model).render_in(cam, &[], &Look::of_3d(&s.model))
}

#[test]
fn only_a_change_of_the_section_box_is_said_to_leave_the_scene_as_it_was() {
    use crate::view3d::differs_only_in_section_box as only_box;
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [3000, 0]]}));
    let step = |s: &mut Session, id: &str, p: Value| {
        let before = s.model.clone();
        run(s, id, p);
        only_box(&before, &s.model)
    };
    assert!(step(&mut s, "section_box", json!({"on": true})), "turned on");
    assert!(step(&mut s, "section_box", json!({"min": [0, -500, 0], "max": [3000, 500, 1000]})), "moved");
    assert!(step(&mut s, "section_box", json!({"on": false})), "turned off");
    assert!(step(&mut s, "section_box", json!({"on": true})), "and on again");
    assert!(step(&mut s, "undo", json!({})), "undone");
    // Anything else leaves it to be made again.
    assert!(!step(&mut s, "wall", json!({"points": [[0, 2000], [3000, 2000]]})), "a wall");
    assert!(!step(&mut s, "visual_style", json!({"style": "hidden_line"})), "the style");
    assert!(!step(&mut s, "graphics", json!({"view": "{3D}", "categories": {"walls": {"hidden": true}}})), "what it hides");
    assert!(
        !step(&mut s, "graphics", json!({"view": "{3D}", "categories": {"walls": {"hidden": false, "transparency": 40}}})),
        "how it shows it"
    );
    // The same model is no change either.
    let same = s.model.clone();
    assert!(only_box(&same, &s.model));
}

#[test]
fn a_camera_written_back_or_a_3d_dimension_leaves_the_scene_as_it_was_and_a_wall_does_not() {
    use crate::view3d::scene_kept as kept;
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [3000, 0]]}));
    let step = |s: &mut Session, id: &str, p: Value| {
        let before = s.model.clone();
        run(s, id, p);
        kept(&before, &s.model)
    };
    assert!(step(&mut s, "camera", json!({"eye": [-5000, -5000, 1700], "target": [1500, 0, 1000], "name": "Corner"})), "a camera view");
    // Its eye and target written back, as the 3D view does once the hand lets it be.
    assert!(step(&mut s, "camera", json!({"view": "Corner", "eye": [-6000, -4000, 1700], "target": [1500, 0, 1200]})), "its camera");
    assert!(
        step(&mut s, "dimension_3d", json!({"refs": [{"at": [0, 0, 0]}, {"at": [3000, 0, 0]}], "at": [1500, -1000, 0]})),
        "a 3D dimension"
    );
    assert!(step(&mut s, "section_box", json!({"on": true})), "the default view's box");
    assert!(!step(&mut s, "wall", json!({"points": [[0, 2000], [3000, 2000]]})), "a wall");
    assert!(!step(&mut s, "visual_style", json!({"style": "hidden_line"})), "the default view's style");
}

#[test]
fn a_mesh_holds_a_scan_s_points_but_in_wireframe_which_draws_none() {
    let (mut s, _) = floor_and_ceiling("points_on_the_card");
    let shaded = gpu::Mesh::new(&Scene::new(&s.model), &Look::of_3d(&s.model)).unwrap();
    assert!(!shaded.dots.is_empty());
    run(&mut s, "visual_style", json!({"style": "wireframe"}));
    assert!(gpu::Mesh::new(&Scene::new(&s.model), &Look::of_3d(&s.model)).unwrap().dots.is_empty());
}

fn floor_and_ceiling(test: &str) -> (Session, Camera) {
    let mut s = Session::new();
    run(&mut s, "point_cloud", json!({"path": scan(test, &room()), "at": [1000, 1000]}));
    // From nearly overhead, so the ceiling hides the floor.
    let cam = Scene::new(&s.model).fit(AZIMUTH, 80.0, 320, 240);
    (s, cam)
}

#[test]
fn a_section_box_keeps_what_of_a_scan_lies_inside_it() {
    let (mut s, cam) = floor_and_ceiling("cut");
    let (floor, ceiling) = (pixels(&picture(&s, &cam), red), pixels(&picture(&s, &cam), blue));
    assert!(ceiling > 2000 && floor < ceiling / 4, "seen from above, the ceiling hides most of the floor: {floor} red, {ceiling} blue");
    // Its top a metre up: the ceiling is cut away, and all the floor shows.
    let r = run(&mut s, "section_box", json!({"min": [-500, -500, -500], "max": [2500, 2500, 1000]}));
    assert_eq!(
        (r["on"].as_bool(), &r["min"], &r["max"]),
        (Some(true), &json!([-500.0, -500.0, -500.0]), &json!([2500.0, 2500.0, 1000.0])),
        "{r}"
    );
    let img = picture(&s, &cam);
    assert_eq!(pixels(&img, blue), 0);
    assert!(pixels(&img, red) > floor + ceiling / 2, "{} red, against {floor}", pixels(&img, red));
    // Its west half only: about half the floor.
    run(&mut s, "section_box", json!({"max": [1000, 2500, 1000]}));
    let half = pixels(&picture(&s, &cam), red);
    let whole = pixels(&img, red);
    assert!(half > whole * 4 / 10 && half < whole * 6 / 10, "{half} of {whole}");
    // Turned off, it keeps its extents for when it's turned on again.
    let r = run(&mut s, "section_box", json!({"on": false}));
    assert_eq!((r["on"].as_bool(), &r["max"]), (Some(false), &json!([1000.0, 2500.0, 1000.0])));
    assert_eq!(pixels(&picture(&s, &cam), blue), ceiling);
    run(&mut s, "section_box", json!({"on": true}));
    assert_eq!(pixels(&picture(&s, &cam), red), half);
}

#[test]
fn a_section_box_cuts_the_models_surfaces_and_what_it_cuts_away_is_not_picked() {
    let mut s = Session::new();
    let wall = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "height": 3000}))["ids"][0].as_u64().unwrap();
    let cam = Scene::new(&s.model).fit(AZIMUTH, ELEVATION, 320, 240);
    let at = |z: f64| {
        let (x, y, _) = cam.project([2000.0, 0.0, z]);
        (x, y)
    };
    let (low, high) = (at(1000.0), at(2500.0));
    let img = picture(&s, &cam);
    assert_eq!((img.pick(low.0, low.1), img.pick(high.0, high.1)), (Some(wall), Some(wall)));
    run(&mut s, "section_box", json!({"min": [-1000, -1000, -1000], "max": [5000, 1000, 1500]}));
    let img = picture(&s, &cam);
    assert_eq!(img.pick(low.0, low.1), Some(wall), "below the box's top, the wall is there");
    assert_eq!(img.pick(high.0, high.1), None, "above it, nothing");
    assert_eq!(img.color(high.0, high.1), crate::render::PAPER);
}

#[test]
fn a_section_box_turned_on_without_extents_fits_round_the_model() {
    let (mut s, _) = floor_and_ceiling("fit");
    let r = run(&mut s, "section_box", json!({}));
    let v = |k: &str| -> Vec<f64> { r[k].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect() };
    let (lo, hi) = (v("min"), v("max"));
    // The scan runs from (0, 0, 0) to (2000, 2000, 1500), its middle where it was put: the box holds all of it, and not much more.
    for (axis, (from, to)) in [(0.0, 2000.0), (0.0, 2000.0), (0.0, 1500.0)].into_iter().enumerate() {
        assert!(lo[axis] < from && lo[axis] > from - 1000.0, "{axis}: {lo:?}");
        assert!(hi[axis] > to && hi[axis] < to + 1000.0, "{axis}: {hi:?}");
    }
    // Nothing to box in an empty project, unless it's given.
    let mut empty = Session::new();
    let e = fails(&mut empty, "section_box", json!({}));
    assert!(e.contains("min") && e.contains("max"), "{e}");
    run(&mut empty, "section_box", json!({"min": [0, 0, 0], "max": [1000, 1000, 1000]}));
}

#[test]
fn a_section_box_is_a_box_a_3d_view_can_hold() {
    let (mut s, _) = floor_and_ceiling("bad");
    for (min, max) in [
        (json!([0, 0, 0]), json!([1000, 1000, 0])),
        (json!([0, 0, 0]), json!([-1000, 1000, 1000])),
        (json!([0, 0, 0]), json!([1e12, 1000, 1000])),
        (json!([0, 0]), json!([1000, 1000, 1000])),
    ] {
        let e = fails(&mut s, "section_box", json!({"min": min, "max": max}));
        assert!(e.contains("section box") || e.contains("section_box"), "{min} {max}: {e}");
    }
    assert!(s.model.default_3d().is_none(), "nothing changed, and no view was made for it");
    // Only a 3D view has one.
    let e = fails(&mut s, "section_box", json!({"view": "Level 1", "min": [0, 0, 0], "max": [1, 1, 1]}));
    assert!(e.contains("3D view"), "{e}");
}

#[test]
fn a_section_box_goes_into_the_project_file_and_undo_takes_it_back() {
    let (mut s, _) = floor_and_ceiling("file");
    run(&mut s, "section_box", json!({"min": [0, 0, 0], "max": [2000, 2000, 1000]}));
    let back = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap().0;
    let view = back.default_3d().unwrap();
    assert_eq!(back.section_box(view), Some(([0.0, 0.0, 0.0], [2000.0, 2000.0, 1000.0])));
    run(&mut s, "section_box", json!({"max": [2000, 2000, 500]}));
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.section_box(s.model.default_3d().unwrap()), Some(([0.0, 0.0, 0.0], [2000.0, 2000.0, 1000.0])));
}

#[test]
fn the_graphics_card_keeps_all_of_a_boxed_view_and_cuts_its_pixels() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [3000, 0]]}));
    let whole = gpu::Mesh::new(&Scene::new(&s.model), &Look::of_3d(&s.model)).unwrap();
    assert_eq!(whole.section_box, None);
    run(&mut s, "section_box", json!({"min": [0, -500, 0], "max": [3000, 500, 1000]}));
    let cut = gpu::Mesh::new(&Scene::new(&s.model), &Look::of_3d(&s.model)).unwrap();
    assert_eq!(cut.section_box, Some(([0.0, -500.0, 0.0], [3000.0, 500.0, 1000.0])));
    assert_eq!(cut.triangles(), whole.triangles(), "moving a face makes no triangles");
}

#[test]
fn a_section_box_has_a_grip_on_each_face_that_a_drag_moves_along_its_axis() {
    use crate::view3d::{box_edges, box_grips, move_box_face};
    let b = ([0.0, 0.0, 0.0], [4000.0, 2000.0, 3000.0]);
    // West, east, south, north, bottom, top: the middle of each face.
    assert_eq!(
        box_grips(b),
        [
            [0.0, 1000.0, 1500.0],
            [4000.0, 1000.0, 1500.0],
            [2000.0, 0.0, 1500.0],
            [2000.0, 2000.0, 1500.0],
            [2000.0, 1000.0, 0.0],
            [2000.0, 1000.0, 3000.0]
        ]
    );
    assert_eq!(box_edges(b).len(), 12);
    // The top down 1234.5 mm, to 10 mm; past the bottom it stops short of it.
    assert_eq!(move_box_face(b, 5, -1234.5), ([0.0, 0.0, 0.0], [4000.0, 2000.0, 1770.0]));
    assert_eq!(move_box_face(b, 5, -9000.0).1[2], crate::model::MIN_SECTION_BOX);
    assert_eq!(move_box_face(b, 0, 500.0).0[0], 500.0, "the west face east");
    // A drag up the screen moves the top up, as far as the view shows a mm of it; looking straight along x, a drag can't move x.
    let cam = Camera { azimuth: -45.0, elevation: 30.0, scale: 0.1, target: [0.0; 3], width: 400, height: 300, lens: None };
    let up = cam.along(2, [0.0; 3], (200.0, 150.0), (200.0, 100.0)).unwrap();
    assert!((up - 50.0 / (0.1 * 30f64.to_radians().cos())).abs() < 1e-6, "{up}");
    let side = Camera { azimuth: 0.0, elevation: 0.0, ..cam };
    assert_eq!(side.along(0, [0.0; 3], (0.0, 0.0), (50.0, 50.0)), None);
}

#[test]
fn a_section_box_leaves_out_the_levels_it_doesnt_reach() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let levels: Vec<u64> = s.model.levels().iter().map(|(id, _)| *id).collect();
    let cam = Scene::new(&s.model).fit(AZIMUTH, ELEVATION, 320, 240);
    assert!(levels.iter().all(|l| picture(&s, &cam).shows().contains(l)), "both levels' planes");
    run(&mut s, "section_box", json!({"min": [-1000, -1000, -500], "max": [5000, 1000, 1500]}));
    let shown = picture(&s, &cam).shows();
    assert!(shown.contains(&levels[0]) && !shown.contains(&levels[1]), "Level 2, at 3 m, is above the box");
}

// ---------- plans that follow the box ----------

use crate::render;

/// Metres: three patches 2 m square, one over the other, red on the floor, green a metre up and blue two metres up; the green
/// one runs 2 m further east.
fn stack() -> String {
    let mut text = String::new();
    for (z, w, [r, g, b]) in [(0.0, 2.0, [255, 0, 0]), (1.0, 4.0, [0, 200, 0]), (2.0, 2.0, [0, 0, 255])] {
        for i in 0..=(w * 50.0) as i32 {
            for k in 0..=100 {
                text += &format!("{} {} {z} {r} {g} {b}\n", f64::from(i) * 0.02, f64::from(k) * 0.02);
            }
        }
    }
    text
}

fn green(c: Rgb) -> bool {
    c.0 < 60 && c.1 > 150 && c.2 < 60
}

/// How many pixels of plan `level`'s picture, 400 by 300, fitted to `fit`'s, are `colour`'s.
fn in_plan(s: &Session, level: u64, colour: impl Fn(Rgb) -> bool) -> usize {
    let d = render::plan(&s.model, level, 400, 300);
    let pm = tiny_skia::Pixmap::decode_png(&render::png(&d).unwrap()).unwrap();
    pm.pixels().iter().map(|p| p.demultiply()).filter(|p| colour(Rgb(p.red(), p.green(), p.blue()))).count()
}

/// The stack placed with its south-west corner on the origin and its foot on Level 1, and Level 1's plan seeing all of it.
fn stacked(test: &str) -> (Session, u64) {
    let mut s = Session::new();
    run(&mut s, "point_cloud", json!({"path": scan(test, &stack()), "origin": "file"}));
    let level = s.model.levels()[0].0;
    run(&mut s, "modify", json!({"id": level, "view_range": {"top": 3000, "cut": 2500, "bottom": 0, "depth": 0}}));
    (s, level)
}

#[test]
fn a_plan_that_follows_the_section_box_shows_what_it_keeps() {
    let (mut s, level) = stacked("plan");
    // Seen from above, the blue patch hides the others where they lie under it; the green one shows east of it.
    assert!(in_plan(&s, level, blue) > 1000 && in_plan(&s, level, green) > 1000);
    run(&mut s, "section_box", json!({"min": [-500, -500, 500], "max": [3000, 2500, 1500]}));
    assert!(in_plan(&s, level, blue) > 1000, "the box changes a plan only once the plan follows it");
    run(&mut s, "modify", json!({"id": level, "section_box": true}));
    // Its heights are the plan's range now: only the green patch, a metre up, is between them; and its sides crop it.
    assert_eq!((in_plan(&s, level, blue), in_plan(&s, level, red)), (0, 0));
    let cropped = in_plan(&s, level, green);
    assert!(cropped > 1000, "{cropped}");
    let range = s.model.shown_range(level);
    assert_eq!((range.cut, range.bottom, range.depth), (1500.0, 500.0, 500.0));
    assert_eq!(s.model.plan_range(level).cut, 2500.0, "the plan's own view range is as it was");
    assert_eq!(s.model.shown_crop(level), Some([P2(-500.0, -500.0), P2(3000.0, 2500.0)]));
    // A crop of its own and the box's sides: what both keep.
    run(&mut s, "modify", json!({"id": level, "crop": [[1000, 0], [5000, 5000]]}));
    assert_eq!(s.model.shown_crop(level), Some([P2(1000.0, 0.0), P2(3000.0, 2500.0)]));
    // The box turned off, the plan is as its own range says.
    run(&mut s, "section_box", json!({"on": false}));
    assert_eq!(s.model.shown_range(level).cut, 2500.0);
    assert_eq!(s.model.shown_crop(level), Some([P2(1000.0, 0.0), P2(5000.0, 5000.0)]));
}

#[test]
fn a_plan_following_the_box_snaps_to_a_scan_at_the_boxs_top() {
    let (mut s, level) = stacked("snap");
    run(&mut s, "section_box", json!({"min": [-500, -500, 500], "max": [5000, 2500, 1000]}));
    run(&mut s, "modify", json!({"id": level, "section_box": true}));
    // Green points are a metre up, where the box's top cuts: a click near one lands on it.
    let f = crate::snap::snap(
        &s.model,
        level,
        P2(3010.0, 1010.0),
        crate::snap::Reach { point: 100.0, line: 50.0 },
        &[],
        false,
        &Default::default(),
    );
    assert_eq!(f.kind, crate::snap::Kind::Point, "{f:?}");
}

#[test]
fn a_dependent_view_follows_the_box_as_its_parent_does_and_the_flag_is_saved() {
    let (mut s, level) = stacked("dependent");
    run(&mut s, "section_box", json!({"min": [-500, -500, 500], "max": [3000, 2500, 1500]}));
    let view = run(&mut s, "duplicate_view", json!({"view": level}))["id"].as_u64().unwrap();
    let dependent = run(&mut s, "duplicate_view", json!({"view": view, "mode": "dependent"}))["id"].as_u64().unwrap();
    run(&mut s, "modify", json!({"id": view, "section_box": true}));
    assert_eq!(s.model.shown_range(dependent).cut, 1500.0);
    assert_eq!(s.model.shown_range(level).cut, 2500.0, "the level's own plan doesn't follow it");
    let back = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap().0;
    assert_eq!(back.shown_range(view).cut, 1500.0);
    assert!(s.execute("modify", &json!({"id": view, "section_box": "yes"})).is_err());
}
