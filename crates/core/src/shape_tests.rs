//! Shaped floors (IDEAS MOD-16, [design](../../../docs/shape-editing.md)): points and split lines on a floor's top, through
//! the commands every front end drives.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

fn element(s: &mut Session, id: u64) -> Value {
    let all = run(s, "elements", json!({}));
    all["elements"].as_array().unwrap().iter().find(|e| e["id"] == id).cloned().unwrap()
}

/// An 8 by 6 m floor on Level 1, its top at the level.
fn slab(s: &mut Session) -> u64 {
    run(s, "floor", json!({"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}))["id"].as_u64().unwrap()
}

/// How high floor `id`'s top is over (`x`, `y`), mm above Level 1.
fn top(s: &Session, id: u64, x: f64, y: f64) -> f64 {
    crate::view3d::tops_at(&s.model, P2(x, y)).get(&id).copied().unwrap_or(f64::NAN)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// The shape as `edit_shape` gives it, asked with nothing.
fn shape(s: &mut Session, id: u64) -> Value {
    run(s, "edit_shape", json!({"id": id}))
}

#[test]
fn a_drain_folds_a_floor_into_four_planes_falling_to_it() {
    let mut s = Session::new();
    let f = slab(&mut s);
    let r = run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}]}));
    assert_eq!(r["faces"], 4, "{r}");
    assert!(near(top(&s, f, 4000.0, 3000.0), -30.0));
    assert!(near(top(&s, f, 2000.0, 1500.0), -15.0));
    // The west plane runs from the west side, at 0, down to the drain.
    assert!(near(top(&s, f, 100.0, 3000.0), -0.75), "{}", top(&s, f, 100.0, 3000.0));
    assert!(near(top(&s, f, 0.0, 0.0), 0.0));
    // The point is a part of the floor, with an id of its own; asked with nothing, the command says what the top is.
    let e = element(&mut s, f);
    assert!(e["shape"]["points"][0]["id"].as_u64().is_some_and(|id| id != f), "{e}");
    assert_eq!(e["shape"]["points"][0]["z"], -30.0);
    let now = shape(&mut s, f);
    assert_eq!(
        (now["points"].as_array().map(Vec::len), now["faces"].clone(), now["top"].clone()),
        (Some(1), json!(4), json!([-30.0, 0.0]))
    );
    assert_eq!(now["corners"].as_array().map(Vec::len), Some(4), "the outline's corners, each at its height: {now}");
}

#[test]
fn a_split_line_is_a_crease_the_top_folds_along() {
    let mut s = Session::new();
    let f = slab(&mut s);
    // A valley across the middle, 20 mm down at both sides: two planes, each over half the floor.
    let r = run(
        &mut s,
        "edit_shape",
        json!({"id": f, "points": [{"at": [0, 3000], "z": -20}, {"at": [8000, 3000], "z": -20}], "lines": [{"a": [0, 3000], "b": [8000, 3000]}]}),
    );
    assert_eq!(r["faces"], 2, "{r}");
    assert!(near(top(&s, f, 4000.0, 3000.0), -20.0));
    assert!(near(top(&s, f, 4000.0, 1500.0), -10.0) && near(top(&s, f, 4000.0, 4500.0), -10.0));
    // A line whose ends are no points: each becomes one, at the height the top has there, so nothing moves.
    run(&mut s, "add_split_line", json!({"id": f, "a": [4000, 3000], "b": [4000, 6000]}));
    let now = shape(&mut s, f);
    assert_eq!((now["points"].as_array().map(Vec::len), now["lines"].as_array().map(Vec::len)), (Some(4), Some(2)), "{now}");
    assert!(near(top(&s, f, 4000.0, 4500.0), -10.0) && near(top(&s, f, 2000.0, 4500.0), -10.0));
    // Sent back with their ids, the points and lines keep them.
    let mut again = now.clone();
    again["points"][0]["z"] = json!(-25);
    let kept = run(&mut s, "edit_shape", json!({"id": f, "points": again["points"], "lines": again["lines"]}));
    assert_eq!(kept["faces"].as_u64().map(|n| n > 2), Some(true));
    let after = shape(&mut s, f);
    assert_eq!(after["points"][0]["id"], now["points"][0]["id"]);
    assert_eq!(after["lines"][1]["id"], now["lines"][1]["id"]);
}

#[test]
fn add_point_sets_a_height_and_reset_shape_takes_them_all_off() {
    let mut s = Session::new();
    let f = slab(&mut s);
    run(&mut s, "add_point", json!({"id": f, "at": [4000, 3000], "z": -30}));
    assert!(near(top(&s, f, 4000.0, 3000.0), -30.0));
    // Within a millimetre of a point: that point, set again, not a second one.
    run(&mut s, "add_point", json!({"id": f, "at": [4000.5, 3000], "z": -40}));
    assert_eq!(shape(&mut s, f)["points"].as_array().map(Vec::len), Some(1));
    assert!(near(top(&s, f, 4000.0, 3000.0), -40.0));
    // With no height: the one the top has there, so nothing moves.
    run(&mut s, "add_point", json!({"id": f, "at": [2000, 1500]}));
    assert!(near(top(&s, f, 2000.0, 1500.0), -20.0) && near(top(&s, f, 4000.0, 3000.0), -40.0));
    // The shape rides on the floor's offset; `absolute` takes an elevation.
    run(&mut s, "modify", json!({"id": f, "offset": 100}));
    assert!(near(top(&s, f, 4000.0, 3000.0), 60.0));
    run(&mut s, "add_point", json!({"id": f, "at": [6000, 1500], "z": 90, "absolute": true}));
    assert!(near(top(&s, f, 6000.0, 1500.0), 90.0));
    run(&mut s, "reset_shape", json!({"id": f}));
    assert!(near(top(&s, f, 4000.0, 3000.0), 100.0) && near(top(&s, f, 6000.0, 1500.0), 100.0));
    assert!(element(&mut s, f).get("shape").is_none());
}

#[test]
fn what_a_shape_refuses_it_says() {
    let mut s = Session::new();
    let f = run(
        &mut s,
        "floor",
        json!({"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "openings": [[[3000, 2000], [5000, 2000], [5000, 4000], [3000, 4000]]]}),
    )["id"]
        .as_u64()
        .unwrap();
    let says = |e: String, what: &str| assert!(e.contains(what), "`{e}` doesn't say `{what}`");
    says(fails(&mut s, "add_point", json!({"id": f, "at": [9000, 3000], "z": -10})), "outside");
    says(fails(&mut s, "add_point", json!({"id": f, "at": [4000, 3000], "z": -10})), "opening");
    says(fails(&mut s, "add_point", json!({"id": f, "at": [1000, 1000], "z": 1e12})), "z");
    says(fails(&mut s, "add_split_line", json!({"id": f, "a": [0, 0], "b": [8000, 6000]})), "crosses");
    let crossing = json!([{"a": [0, 0], "b": [2500, 6000]}, {"a": [0, 6000], "b": [2500, 0]}]);
    says(fails(&mut s, "edit_shape", json!({"id": f, "lines": crossing})), "crosses");
    let twins = json!([{"at": [1000, 1000], "z": -1}, {"at": [1000.5, 1000], "z": -2}]);
    says(fails(&mut s, "edit_shape", json!({"id": f, "points": twins})), "within a millimetre");
    says(fails(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [1000, 1000]}]})), "z");
    // Shaped, a floor takes its fall from its points: not a slope arrow too.
    run(&mut s, "add_point", json!({"id": f, "at": [1000, 1000], "z": -10}));
    says(fails(&mut s, "modify", json!({"id": f, "slope": 2, "arrow": [[0, 0], [8000, 0]]})), "shape");
    // Only floors have shapes.
    let wall = run(&mut s, "wall", json!({"points": [[0, 8000], [8000, 8000]]}))["ids"][0].as_u64().unwrap();
    says(fails(&mut s, "add_point", json!({"id": wall, "at": [1000, 8000], "z": 0})), "floor");
    says(fails(&mut s, "edit_shape", json!({"id": wall})), "floor");
}

#[test]
fn a_shaped_floor_moves_mirrors_and_copies_with_its_points() {
    let mut s = Session::new();
    let f = slab(&mut s);
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}], "lines": [{"a": [0, 0], "b": [4000, 3000]}]}));
    run(&mut s, "move", json!({"ids": [f], "by": [1000, 0]}));
    assert!(near(top(&s, f, 5000.0, 3000.0), -30.0));
    run(&mut s, "mirror", json!({"ids": [f], "axis": [[0, 0], [0, 1000]]}));
    assert!(near(top(&s, f, -5000.0, 3000.0), -30.0) && near(top(&s, f, -3000.0, 1500.0), -15.0));
    let copy = run(&mut s, "copy", json!({"ids": [f], "by": [0, 10000]}))["ids"][0].as_u64().unwrap();
    assert!(near(top(&s, copy, -5000.0, 13000.0), -30.0));
    let (mine, its) = (shape(&mut s, f), shape(&mut s, copy));
    assert_ne!(mine["points"][0]["id"], its["points"][0]["id"], "a copy's points are its own");
    assert_eq!(its["lines"].as_array().map(Vec::len), Some(1));
}

#[test]
fn a_shape_is_kept_in_the_file_and_goes_to_ifc_as_its_planes() {
    let mut s = Session::new();
    let f = slab(&mut s);
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}]}));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shaped.bimcraft");
    run(&mut s, "save", json!({"path": path}));
    let file: Value = serde_json::from_str(&crate::bimc::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(file["format"], crate::model::FORMAT);
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    assert_eq!(shape(&mut t, f), shape(&mut s, f));
    assert!(near(top(&t, f, 2000.0, 1500.0), -15.0));
    // In IFC, its slab is shells round its four planes, not a profile swept up.
    let ifc = dir.path().join("shaped.ifc");
    run(&mut s, "save", json!({"path": ifc}));
    let text = crate::bimc::read_to_string(&ifc).unwrap();
    assert!(text.contains("'Body','Brep'") && !text.contains("SweptSolid"), "the slab isn't its planes");
}

#[test]
fn a_wall_under_a_shaped_floor_follows_its_planes() {
    let mut s = Session::new();
    // On Level 2, 3000 mm up, a drain 30 mm down in its middle.
    let f =
        run(&mut s, "floor", json!({"level": 100001, "boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}))["id"].as_u64().unwrap();
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}]}));
    let wall = run(&mut s, "wall", json!({"points": [[4000, 0], [4000, 6000]], "type": "Interior - 100mm", "attach_top": f}))["ids"][0]
        .as_u64()
        .unwrap();
    // Its top keeps the floor's 200 mm under the floor's top, down to the drain and up again.
    for (y, fall) in [(500.0, 5.0), (1500.0, 15.0), (3000.0, 30.0), (4500.0, 15.0)] {
        assert!(near(top(&s, wall, 4000.0, y), 3000.0 - fall - 200.0), "at {y}: {}", top(&s, wall, 4000.0, y));
    }
}

#[test]
fn a_plan_draws_a_shaped_floors_creases() {
    let mut s = Session::new();
    slab(&mut s);
    let ink = |s: &Session| {
        let d = crate::render::plan(&s.model, 100_000u64, 800, 600);
        let pm = tiny_skia::Pixmap::decode_png(&crate::render::png(&d).unwrap()).unwrap();
        pm.pixels().iter().map(|p| p.demultiply()).filter(|p| p.alpha() > 0 && p.red() < 160 && p.green() < 160).count()
    };
    let flat = ink(&s);
    let f = run(&mut s, "elements", json!({"category": "floors"}))["elements"][0]["id"].as_u64().unwrap();
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}]}));
    // Four creases, a corner to the drain each: some hundreds of pixels at this size.
    let shaped = ink(&s);
    assert!(shaped > flat + 200, "flat {flat} pixels, shaped {shaped}");
}

#[test]
fn edit_boundary_keeps_the_points_inside_and_names_the_rest() {
    let mut s = Session::new();
    let f = slab(&mut s);
    run(
        &mut s,
        "edit_shape",
        json!({"id": f, "points": [{"at": [3000, 3000], "z": -30}, {"at": [7000, 5000], "z": -10}], "lines": [{"a": [0, 0], "b": [3000, 3000]}, {"a": [8000, 6000], "b": [7000, 5000]}]}),
    );
    // A 6 m square: the drain stays, and its line from the corner; the far point and its line from a corner gone go.
    let square = json!([[[0, 0], [6000, 0]], [[6000, 0], [6000, 6000]], [[6000, 6000], [0, 6000]], [[0, 6000], [0, 0]]]);
    let r = run(&mut s, "edit_boundary", json!({"id": f, "sketch": square}));
    assert_eq!(r["dropped"]["points"], json!([[7000.0, 5000.0]]), "{r}");
    assert_eq!(r["dropped"]["lines"].as_array().map(Vec::len), Some(1), "{r}");
    let now = shape(&mut s, f);
    assert_eq!((now["points"].as_array().map(Vec::len), now["lines"].as_array().map(Vec::len)), (Some(1), Some(1)));
    assert!(near(top(&s, f, 3000.0, 3000.0), -30.0));
}

#[test]
fn snaps_find_a_shapes_points_and_reinforcement_waits_for_a_flat_structure() {
    use crate::snap::{Kind, Reach, Snaps, snap};
    let mut s = Session::new();
    let f = slab(&mut s);
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [3000, 2500], "z": -30}]}));
    let found = snap(&s.model, 100_000, P2(3040.0, 2470.0), Reach { point: 100.0, line: 100.0 }, &[], false, &Snaps::default());
    assert_eq!((found.kind, found.at), (Kind::Endpoint, P2(3000.0, 2500.0)), "{found:?}");
    let e = fails(&mut s, "reinforce", json!({"host": f, "cover": 20, "bottom": {"y": {"diameter": 8, "spacing": 150}}}));
    assert!(e.contains("shape"), "{e}");
}

#[test]
fn a_floors_details_give_its_volume_shaped_or_not() {
    let mut s = Session::new();
    let f = slab(&mut s);
    let volume = |s: &mut Session| element(s, f)["volume_m3"].as_f64().unwrap_or(f64::NAN);
    let thickness = element(&mut s, f)["thickness"].as_f64().unwrap();
    let flat = volume(&mut s);
    assert!((flat - 48.0 * thickness / 1e3).abs() < 1e-3, "{flat}");
    // A drain folds the top, and the slab under it with it: as much concrete as before, to 0.1 %.
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}]}));
    let shaped = volume(&mut s);
    assert!((shaped - flat).abs() <= flat * 1e-3, "flat {flat}, shaped {shaped}");
}

#[test]
fn a_section_through_a_drain_shows_the_top_falling_to_it() {
    let mut s = Session::new();
    let f = slab(&mut s);
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}]}));
    // West to east through the drain, looking north: along it, the drain is 5 m from its start.
    let id = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
    let view = crate::render::View { angle: 0.0, scale: 0.5, cx: 5000.0, cy: -100.0, width: 6000, height: 800 };
    let d = crate::render::section(&s.model, id, view);
    let corners: Vec<(f32, f32)> = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter(|(_, o)| **o == Some(f))
        .filter_map(|(p, _)| match p {
            crate::render::Prim::Fill { rings, .. } => Some(rings.concat()),
            _ => None,
        })
        .flatten()
        .collect();
    let has = |along: f64, z: f64| {
        let (x, y) = view.px(P2(along, z));
        corners.iter().any(|c| (c.0 - x).abs() < 0.5 && (c.1 - y).abs() < 0.5)
    };
    // A V: the top at the sides, 30 mm down at the drain, and the slab its 200 mm under that all the way.
    assert!(has(1000.0, 0.0) && has(9000.0, 0.0), "the top at the sides");
    assert!(has(5000.0, -30.0) && has(5000.0, -230.0), "down at the drain, the underside with it");
    assert!(!has(5000.0, 0.0), "no flat top over the drain");
}

/// A terrace's floor type, 275 mm: tile 10, screed 60 (variable), membrane 5 and concrete 200, from the top down.
fn terrace(s: &mut Session) {
    let layers = json!([
        {"function": "finish", "material": "Ceramic tile", "thickness": 10},
        {"function": "substrate", "thickness": 60, "variable": true},
        {"function": "membrane", "thickness": 5},
        {"function": "structure", "material": "Concrete", "thickness": 200}
    ]);
    run(s, "type", json!({"category": "floor", "name": "Terrace", "layers": layers}));
}

/// An 8 by 6 m terrace floor on Level 1, its top at the level, a drain 30 mm down in its middle.
fn drained(s: &mut Session) -> u64 {
    terrace(s);
    let boundary = json!([[0, 0], [8000, 0], [8000, 6000], [0, 6000]]);
    let f = run(s, "floor", json!({"boundary": boundary, "type": "Terrace"}))["id"].as_u64().unwrap();
    run(s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}]}));
    f
}

#[test]
fn a_variable_layer_keeps_the_underside_flat_and_takes_up_the_fall() {
    let mut s = Session::new();
    let f = drained(&mut s);
    // The underside stays where the unshaped floor's was; only the screed thins toward the drain: 48 m² of 275 mm, less the
    // pyramid the drain takes out, 48 m² by 30 mm over 3.
    let v = element(&mut s, f)["volume_m3"].as_f64().unwrap();
    assert!((v - (48.0 * 0.275 - 48.0 * 0.030 / 3.0)).abs() < 1e-3, "{v}");
    // In the 3D view, at a point 29 mm down: the tile follows the top, the screed takes what's left, the membrane and the
    // concrete under it stay level.
    let q = P2(4000.0, 2900.0);
    let mut layers: Vec<(f64, f64)> = crate::view3d::shaded(&s.model)
        .into_iter()
        .filter(|x| x.owner() == f)
        .filter_map(|x| match x {
            crate::view3d::Solid::Prism { ring, bottom, top, .. } if crate::geom::contains(&ring, q) => Some((bottom.at(q), top.at(q))),
            _ => None,
        })
        .collect();
    layers.sort_by(|a, b| b.1.total_cmp(&a.1));
    let want = [(-39.0, -29.0), (-70.0, -39.0), (-75.0, -70.0), (-275.0, -75.0)];
    assert_eq!(layers.len(), 4, "{layers:?}");
    for ((lo, hi), (wlo, whi)) in layers.iter().zip(want) {
        assert!(near(*lo, wlo) && near(*hi, whi), "{layers:?}");
    }
}

#[test]
fn a_section_shows_the_screed_tapering_over_a_level_structure() {
    let mut s = Session::new();
    let f = drained(&mut s);
    let id = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
    let view = crate::render::View { angle: 0.0, scale: 0.5, cx: 5000.0, cy: -150.0, width: 6000, height: 800 };
    let d = crate::render::section(&s.model, id, view);
    let mine = || d.prims.iter().zip(&d.owners).filter(|(_, o)| **o == Some(f)).map(|(p, _)| p);
    let corners: Vec<(f32, f32)> = mine()
        .filter_map(|p| match p {
            crate::render::Prim::Fill { rings, .. } => Some(rings.concat()),
            _ => None,
        })
        .flatten()
        .collect();
    let lines: Vec<&Vec<(f32, f32)>> = mine()
        .filter_map(|p| match p {
            crate::render::Prim::Line { pts, .. } => Some(pts),
            _ => None,
        })
        .collect();
    let close = |c: &(f32, f32), along: f64, z: f64| {
        let (x, y) = view.px(P2(along, z));
        (c.0 - x).abs() < 0.5 && (c.1 - y).abs() < 0.5
    };
    let corner = |along: f64, z: f64| corners.iter().any(|c| close(c, along, z));
    let between = |along: f64, z: f64| lines.iter().any(|l| l.iter().any(|c| close(c, along, z)));
    // The top falls to the drain; the underside doesn't.
    assert!(corner(5000.0, -30.0) && corner(5000.0, -275.0) && !corner(5000.0, -305.0), "a flat underside");
    // Between the layers: under the tile, 10 mm under the top; over and under the membrane, level.
    assert!(between(5000.0, -40.0) && between(1000.0, -10.0), "the tile follows the top");
    assert!(between(5000.0, -70.0) && between(1000.0, -70.0) && between(5000.0, -75.0), "the membrane stays level");
}

#[test]
fn a_point_that_would_leave_the_screed_thinner_than_nothing_is_refused() {
    let mut s = Session::new();
    let f = drained(&mut s);
    let e = fails(&mut s, "add_point", json!({"id": f, "at": [2000, 3000], "z": -80}));
    assert!(e.contains("-20 mm") && e.contains("raise the point by 20 mm"), "{e}");
    // A type made thinner under a shape: the same.
    let thin = json!([
        {"function": "finish", "material": "Ceramic tile", "thickness": 10},
        {"function": "substrate", "thickness": 20, "variable": true},
        {"function": "membrane", "thickness": 5},
        {"function": "structure", "material": "Concrete", "thickness": 200}
    ]);
    let e = fails(&mut s, "type", json!({"category": "floor", "name": "Terrace", "layers": thin}));
    assert!(e.contains("-10 mm"), "{e}");
}

#[test]
fn only_one_layer_of_a_floor_or_roof_type_is_variable() {
    let mut s = Session::new();
    let two = json!([{"thickness": 50, "variable": true}, {"thickness": 200, "variable": true}]);
    let e = fails(&mut s, "type", json!({"category": "floor", "name": "Two", "layers": two}));
    assert!(e.contains("one variable layer"), "{e}");
    let one = json!([{"thickness": 50, "variable": true}, {"thickness": 200}]);
    for cat in ["wall", "ceiling"] {
        let e = fails(&mut s, "type", json!({"category": cat, "name": "Odd", "layers": one}));
        assert!(e.contains("variable"), "{cat}: {e}");
    }
    run(&mut s, "type", json!({"category": "roof", "name": "Flat", "layers": one}));
}

#[test]
fn a_wall_under_a_terrace_and_its_bars_keep_to_the_level_structure() {
    // On Level 2, 3000 mm up: the wall under it rises to its level underside, 275 mm down, all along.
    let mut s = Session::new();
    terrace(&mut s);
    let up =
        run(&mut s, "floor", json!({"level": 100001, "type": "Terrace", "boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}))["id"]
            .as_u64()
            .unwrap();
    run(&mut s, "edit_shape", json!({"id": up, "points": [{"at": [4000, 3000], "z": -30}]}));
    let wall = run(&mut s, "wall", json!({"points": [[4000, 0], [4000, 6000]], "type": "Interior - 100mm", "attach_top": up}))["ids"][0]
        .as_u64()
        .unwrap();
    for y in [500.0, 3000.0, 4500.0] {
        assert!(near(top(&s, wall, 4000.0, y), 3000.0 - 275.0), "at {y}: {}", top(&s, wall, 4000.0, y));
    }
    // The bars: a mesh in the concrete, 20 mm cover, all of it under the membrane however the top falls.
    run(&mut s, "reinforce", json!({"host": up, "cover": 20, "bottom": {"x": {"diameter": 8, "spacing": 150}}}));
    let guid = s.model.elements.guid(up).unwrap();
    let zs: Vec<f64> = s
        .model
        .elements
        .iter()
        .filter_map(|(_, e)| match e {
            crate::model::Element::Rebar(r) if r.host == Some(guid) => Some(r.path.iter().map(|p| p[2]).collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect();
    assert!(!zs.is_empty());
    assert!(zs.iter().all(|z| near(*z, -275.0 + 20.0 + 4.0)), "{:?}", &zs[..zs.len().min(6)]);
}

#[test]
fn a_shape_picks_its_nearest_point_or_corner_else_an_edges_ends() {
    let mut s = Session::new();
    let f = slab(&mut s);
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}], "lines": [{"a": [0, 6000], "b": [4000, 3000]}]}));
    let Some(crate::model::Element::Floor(fl)) = s.model.elements.get(f) else { panic!("no floor") };
    assert_eq!(fl.shape_pick(P2(4040.0, 2980.0), 100.0), [P2(4000.0, 3000.0)]);
    assert_eq!(fl.shape_pick(P2(30.0, 20.0), 100.0), [P2(0.0, 0.0)]);
    assert_eq!(fl.shape_pick(P2(4000.0, 50.0), 100.0), [P2(0.0, 0.0), P2(8000.0, 0.0)]);
    assert_eq!(fl.shape_pick(P2(2000.0, 4520.0), 100.0), [P2(0.0, 6000.0), P2(4000.0, 3000.0)], "the split line");
    assert!(fl.shape_pick(P2(2000.0, 2000.0), 100.0).is_empty());
}

#[test]
fn a_dragged_point_takes_its_split_lines_with_it_and_keeps_its_height() {
    let mut s = Session::new();
    let f = slab(&mut s);
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}], "lines": [{"a": [0, 6000], "b": [4000, 3000]}]}));
    let Some(crate::model::Element::Floor(fl)) = s.model.elements.get(f) else { panic!("no floor") };
    assert_eq!(fl.shape.point_near(P2(4040.0, 2980.0), 100.0), Some(P2(4000.0, 3000.0)));
    assert_eq!(fl.shape.point_near(P2(30.0, 20.0), 100.0), None, "a corner isn't a point to drag");
    let moved = fl.shape.moved(P2(4000.0, 3000.0), P2(5000.0, 2000.0));
    assert_eq!(moved.points[0].at, P2(5000.0, 2000.0));
    assert_eq!(moved.points[0].z, -30.0);
    assert_eq!((moved.points[0].id, moved.points[0].guid), (fl.shape.points[0].id, fl.shape.points[0].guid), "the same part");
    assert_eq!((moved.lines[0].a, moved.lines[0].b), (P2(0.0, 6000.0), P2(5000.0, 2000.0)));
    assert_eq!(fl.shape.moved(P2(1.0, 1.0), P2(5.0, 5.0)), fl.shape, "nothing there, nothing moves");
}

#[test]
fn a_raised_point_keeps_its_part_and_a_raised_corner_becomes_one() {
    let mut s = Session::new();
    let f = slab(&mut s);
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}]}));
    let Some(crate::model::Element::Floor(fl)) = s.model.elements.get(f) else { panic!("no floor") };
    let raised = fl.shape.raised(P2(4000.0, 3000.0), 20.0);
    assert_eq!((raised.points.len(), raised.points[0].z, raised.points[0].id), (1, 20.0, fl.shape.points[0].id));
    let corner = fl.shape.raised(P2(8000.0, 6000.0), 15.0);
    assert_eq!(corner.points.len(), 2);
    assert_eq!((corner.points[1].at, corner.points[1].z), (P2(8000.0, 6000.0), 15.0));
}

#[test]
fn a_shaped_tops_creases_are_its_planes_edges_in_3d_each_once() {
    let mut s = Session::new();
    let f = slab(&mut s);
    let Some(crate::model::Element::Floor(flat)) = s.model.elements.get(f).cloned() else { panic!("no floor") };
    assert!(s.model.shape_creases(&flat).is_empty(), "a flat floor folds nowhere");
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}]}));
    let Some(crate::model::Element::Floor(fl)) = s.model.elements.get(f).cloned() else { panic!("no floor") };
    let creases = s.model.shape_creases(&fl);
    // A drain in a rectangle: its outline's four edges and one from each corner down to it.
    assert_eq!(creases.len(), 8, "{creases:?}");
    let top = top(&s, f, 0.0, 0.0);
    let to_drain = creases.iter().filter(|[a, b]| [a, b].iter().any(|p| (p[0], p[1]) == (4000.0, 3000.0))).count();
    assert_eq!(to_drain, 4);
    for [a, b] in &creases {
        for p in [a, b] {
            let drain = (p[0], p[1]) == (4000.0, 3000.0);
            assert!((p[2] - if drain { top - 30.0 } else { top }).abs() < 1e-6, "{p:?}");
        }
    }
    // A preview of a drag is the same floor with the shape it would have: the drain moved, its creases with it.
    let moved = crate::model::Floor { shape: fl.shape.moved(P2(4000.0, 3000.0), P2(2000.0, 3000.0)), ..fl.clone() };
    assert!(s.model.shape_creases(&moved).iter().flatten().any(|p| (p[0], p[1], p[2]) == (2000.0, 3000.0, top - 30.0)));
}

#[test]
fn a_shapes_labels_give_each_points_height_relative_or_above_its_level() {
    let mut s = Session::new();
    // On Level 2, 3000 mm up, offset 50 mm: a drain 30 mm down.
    let f = run(&mut s, "floor", json!({"level": 100001, "offset": 50, "boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}))["id"]
        .as_u64()
        .unwrap();
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}]}));
    let Some(crate::model::Element::Floor(fl)) = s.model.elements.get(f) else { panic!("no floor") };
    assert_eq!(s.model.shape_labels(fl, false), [(P2(4000.0, 3000.0), -30.0)]);
    assert_eq!(s.model.shape_labels(fl, true), [(P2(4000.0, 3000.0), 20.0)], "50 up, 30 down, above Level 2");
}

/// The 3D view's default camera on the model, 1200 by 800 pixels.
fn camera(s: &Session) -> crate::view3d::Camera {
    crate::view3d::Scene::new(&s.model).fit(crate::view3d::AZIMUTH, crate::view3d::ELEVATION, 1200, 800)
}

#[test]
fn in_3d_a_click_lands_on_the_shaped_top_and_picks_points_corners_and_edges() {
    use crate::view3d::{on_floor_top, shape_handles, shape_pick_3d};
    let mut s = Session::new();
    let f = slab(&mut s);
    run(
        &mut s,
        "edit_shape",
        json!({"id": f, "points": [{"at": [4000, 3000], "z": -300}], "lines": [{"a": [0, 6000], "b": [4000, 3000]}]}),
    );
    let cam = camera(&s);
    let Some(crate::model::Element::Floor(fl)) = s.model.elements.get(f) else { panic!("no floor") };
    // Each point and corner where the top is: the drain 300 mm down, the corners at the top.
    let (handles, edges) = shape_handles(&s.model, fl);
    assert_eq!(handles.len(), 5);
    assert_eq!(handles[0], (P2(4000.0, 3000.0), [4000.0, 3000.0, -300.0]));
    assert!(handles.contains(&(P2(8000.0, 6000.0), [8000.0, 6000.0, 0.0])));
    assert_eq!(edges.len(), 5, "the split line and the outline's four");
    let px = |p: [f64; 3]| {
        let (x, y, _) = cam.project(p);
        (x, y)
    };
    // A pixel showing a point on a sloped plane of the top gives it back in plan.
    let on = [2000.0, 3000.0, -150.0];
    let hit = on_floor_top(&s.model, fl, &cam, px(on)).unwrap();
    assert!(hit.dist(P2(2000.0, 3000.0)) < 0.5, "{hit:?}");
    // Picks as in plan, in pixels: the drain, a corner, the split line's two ends, nothing.
    let (dx, dy) = px([4000.0, 3000.0, -300.0]);
    assert_eq!(shape_pick_3d(&s.model, fl, &cam, (dx + 3.0, dy - 2.0), 8.0), [P2(4000.0, 3000.0)]);
    assert_eq!(shape_pick_3d(&s.model, fl, &cam, px([8000.0, 0.0, 0.0]), 8.0), [P2(8000.0, 0.0)]);
    assert_eq!(shape_pick_3d(&s.model, fl, &cam, px([2000.0, 4500.0, -150.0]), 8.0), [P2(0.0, 6000.0), P2(4000.0, 3000.0)]);
    // On a folding line, its ends; out in a plane, nothing.
    assert_eq!(shape_pick_3d(&s.model, fl, &cam, px([6000.0, 1500.0, -150.0]), 2.0).len(), 2);
    assert!(shape_pick_3d(&s.model, fl, &cam, px([7000.0, 3000.0, -75.0]), 2.0).is_empty());
    // Off the floor, no hit.
    assert_eq!(on_floor_top(&s.model, fl, &cam, px([12000.0, 3000.0, 0.0])), None);
    // In perspective too, from the eye.
    let eye = crate::view3d::Camera { lens: Some(35.0), ..cam };
    let (x, y, _) = eye.project(on);
    let hit = on_floor_top(&s.model, fl, &eye, (x, y)).unwrap();
    assert!(hit.dist(P2(2000.0, 3000.0)) < 0.5, "{hit:?}");
}

/// A flat roof by footprint over Level 2, 8 by 6 m.
fn flat_roof(s: &mut Session) -> u64 {
    run(s, "roof", json!({"level": 100001, "slope": 0, "boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}))["id"].as_u64().unwrap()
}

#[test]
fn a_flat_roof_takes_a_shape_its_underside_and_the_walls_under_it_following() {
    let mut s = Session::new();
    let roof = flat_roof(&mut s);
    let flat = top(&s, roof, 4000.0, 3000.0);
    let Some(crate::model::Element::Roof(r)) = s.model.elements.get(roof).cloned() else { panic!("no roof") };
    let thick = s.model.roof_rise(&r, &crate::walls::Plane::flat(0.0));
    assert!(thick > 0.0 && (flat - (3000.0 + thick)).abs() < 1e-6, "{flat} {thick}");
    // Four drains 40 mm down.
    let drains = [[2000, 1500], [6000, 1500], [2000, 4500], [6000, 4500]];
    let points: Vec<Value> = drains.iter().map(|at| json!({"at": at, "z": -40})).collect();
    let reply = run(&mut s, "edit_shape", json!({"id": roof, "points": points}));
    assert!(reply["faces"].as_u64().unwrap() > 4, "{reply}");
    assert_eq!(reply["top"], json!([flat - 40.0, flat]), "{reply}");
    for [x, y] in drains {
        assert!((top(&s, roof, f64::from(x), f64::from(y)) - (flat - 40.0)).abs() < 1e-6);
    }
    assert!((top(&s, roof, 0.0, 0.0) - flat).abs() < 1e-6, "its corners stay");
    // Its underside follows its top, plumb: every piece as thick as its type.
    let Some(crate::model::Element::Roof(r)) = s.model.elements.get(roof).cloned() else { panic!("no roof") };
    let slabs = s.model.roof_slabs(&r);
    assert!(slabs.len() > 4 && slabs.iter().all(|(_, rise)| (rise - thick).abs() < 1e-9), "{slabs:?}");
    // A wall through two drains, attached to it, reaches its underside there.
    let wall = run(&mut s, "wall", json!({"points": [[0, 1500], [8000, 1500]]}))["ids"][0].as_u64().unwrap();
    run(&mut s, "modify", json!({"id": wall, "attach_top": roof}));
    assert!((top(&s, wall, 2000.0, 1500.0) - 2960.0).abs() < 1.0, "{}", top(&s, wall, 2000.0, 1500.0));
    assert!((top(&s, wall, 0.0, 1500.0) - 3000.0).abs() < 1.0, "{}", top(&s, wall, 0.0, 1500.0));
    // It moves with its roof, and resets.
    run(&mut s, "move", json!({"ids": [roof], "by": [1000, 0]}));
    let shape = run(&mut s, "edit_shape", json!({"id": roof}));
    assert_eq!(shape["points"][0]["at"], json!([3000.0, 1500.0]), "{shape}");
    run(&mut s, "reset_shape", json!({"id": roof}));
    assert!((top(&s, roof, 3000.0, 1500.0) - flat).abs() < 1e-6);
}

#[test]
fn only_a_flat_roof_by_footprint_takes_a_shape_and_a_shaped_one_keeps_flat() {
    let mut s = Session::new();
    let pitched =
        run(&mut s, "roof", json!({"level": 100001, "boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}))["id"].as_u64().unwrap();
    let e = fails(&mut s, "edit_shape", json!({"id": pitched, "points": [{"at": [4000, 3000], "z": -40}]}));
    assert!(e.contains("flat"), "{e}");
    let e = fails(&mut s, "add_point", json!({"id": pitched, "at": [4000, 3000], "z": -40}));
    assert!(e.contains("flat"), "{e}");
    let profile = json!({"line": [[0, 0], [8000, 0]], "profile": [[0, 0], [4000, 0]], "to": 6000});
    let extruded = run(&mut s, "roof", json!({"level": 100001, "extrusion": profile}))["id"].as_u64().unwrap();
    let e = fails(&mut s, "add_point", json!({"id": extruded, "at": [2000, 3000], "z": -40}));
    assert!(e.contains("footprint"), "{e}");
    let roof = flat_roof(&mut s);
    run(&mut s, "add_point", json!({"id": roof, "at": [4000, 3000], "z": -40}));
    let e = fails(&mut s, "modify", json!({"id": roof, "slope": 20}));
    assert!(e.contains("reset_shape"), "{e}");
    // Other elements still say what has a shape.
    let wall = run(&mut s, "wall", json!({"points": [[0, 0], [1000, 0]]}))["ids"][0].as_u64().unwrap();
    let e = fails(&mut s, "add_point", json!({"id": wall, "at": [500, 0]}));
    assert!(e.contains("floors, flat roofs and toposolids have shapes"), "{e}");
}

#[test]
fn a_shaped_roof_draws_its_creases_snaps_to_its_points_and_keeps_them_through_a_new_outline() {
    use crate::snap::{Kind, Reach, Snaps, snap};
    let mut s = Session::new();
    let roof = flat_roof(&mut s);
    let ink = |s: &Session| {
        let d = crate::render::plan(&s.model, 100_001u64, 800, 600);
        let pm = tiny_skia::Pixmap::decode_png(&crate::render::png(&d).unwrap()).unwrap();
        pm.pixels().iter().map(|p| p.demultiply()).filter(|p| p.alpha() > 0 && p.red() < 160 && p.green() < 160).count()
    };
    let flat = ink(&s);
    run(&mut s, "edit_shape", json!({"id": roof, "points": [{"at": [3000, 2500], "z": -40}, {"at": [7000, 5000], "z": -20}]}));
    let shaped = ink(&s);
    assert!(shaped > flat + 200, "flat {flat} pixels, shaped {shaped}");
    let found = snap(&s.model, 100_001, P2(3040.0, 2470.0), Reach { point: 100.0, line: 100.0 }, &[], false, &Snaps::default());
    assert_eq!((found.kind, found.at), (Kind::Endpoint, P2(3000.0, 2500.0)), "{found:?}");
    // A 6 m square: the drain stays, the far point goes.
    let square = json!([[[0, 0], [6000, 0]], [[6000, 0], [6000, 6000]], [[6000, 6000], [0, 6000]], [[0, 6000], [0, 0]]]);
    let r = run(&mut s, "edit_boundary", json!({"id": roof, "sketch": square}));
    assert_eq!(r["dropped"]["points"], json!([[7000.0, 5000.0]]), "{r}");
    let now = run(&mut s, "edit_shape", json!({"id": roof}));
    assert_eq!(now["points"].as_array().map(Vec::len), Some(1), "{now}");
    // A mirror image keeps its heights.
    run(&mut s, "mirror", json!({"ids": [roof], "axis": [[3000, 0], [3000, 6000]]}));
    let now = run(&mut s, "edit_shape", json!({"id": roof}));
    assert_eq!((now["points"][0]["at"].clone(), now["points"][0]["z"].clone()), (json!([3000.0, 2500.0]), json!(-40.0)), "{now}");
}

/// A flat roof over Level 2 of a type with a tapering insulation layer (finish 10, insulation 100 variable, structure 200),
/// a drain 40 mm down in its middle.
fn drained_roof(s: &mut Session) -> u64 {
    let layers = json!([
        {"function": "finish", "thickness": 10},
        {"function": "substrate", "thickness": 100, "variable": true},
        {"function": "structure", "thickness": 200}
    ]);
    run(s, "type", json!({"category": "roof", "name": "Terrace Roof", "layers": layers}));
    let roof = run(
        s,
        "roof",
        json!({"level": 100001, "slope": 0, "type": "Terrace Roof", "boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}),
    )["id"]
        .as_u64()
        .unwrap();
    run(s, "edit_shape", json!({"id": roof, "points": [{"at": [4000, 3000], "z": -40}]}));
    roof
}

#[test]
fn a_roofs_variable_layer_keeps_its_underside_level_and_takes_up_the_fall() {
    let mut s = Session::new();
    let roof = drained_roof(&mut s);
    assert!(near(top(&s, roof, 4000.0, 3000.0), 3270.0) && near(top(&s, roof, 0.0, 0.0), 3310.0));
    // In the 3D view, 2900 mm up the south plane: the finish follows the top, the insulation takes what's left, the
    // structure stays level on Level 2.
    let q = P2(4000.0, 2900.0);
    let t = 3310.0 - 40.0 * 2900.0 / 3000.0;
    let mut layers: Vec<(f64, f64)> = crate::view3d::shaded(&s.model)
        .into_iter()
        .filter(|x| x.owner() == roof)
        .filter_map(|x| match x {
            crate::view3d::Solid::Prism { ring, bottom, top, .. } if crate::geom::contains(&ring, q) => Some((bottom.at(q), top.at(q))),
            _ => None,
        })
        .collect();
    layers.sort_by(|a, b| b.1.total_cmp(&a.1));
    let want = [(t - 10.0, t), (3200.0, t - 10.0), (3000.0, 3200.0)];
    assert_eq!(layers.len(), 3, "{layers:?}");
    for ((lo, hi), (wlo, whi)) in layers.iter().zip(want) {
        assert!(near(*lo, wlo) && near(*hi, whi), "{layers:?}");
    }
    // A wall attached under it reaches its level underside, drain or not.
    let wall = run(&mut s, "wall", json!({"points": [[0, 3000], [8000, 3000]]}))["ids"][0].as_u64().unwrap();
    run(&mut s, "modify", json!({"id": wall, "attach_top": roof}));
    assert!(near(top(&s, wall, 4000.0, 3000.0), 3000.0) && near(top(&s, wall, 1000.0, 3000.0), 3000.0));
    // Its closed solid holds 48 m² of 310 mm less the pyramid the drain takes out, 48 m² by 40 mm over 3.
    let Some(crate::model::Element::Roof(r)) = s.model.elements.get(roof).cloned() else { panic!("no roof") };
    let solid = s.model.roof_solid(&r).expect("a shaped roof's solid");
    let v = crate::model::volume(&solid) / 1e9;
    assert!((v - (48.0 * 0.31 - 48.0 * 0.04 / 3.0)).abs() < 1e-3, "{v}");
}

#[test]
fn a_roof_point_that_would_leave_its_insulation_thinner_than_nothing_is_refused() {
    let mut s = Session::new();
    let roof = drained_roof(&mut s);
    let e = fails(&mut s, "add_point", json!({"id": roof, "at": [2000, 1500], "z": -150}));
    assert!(e.contains("raise the point by 50"), "{e}");
}

#[test]
fn a_section_shows_a_roofs_insulation_tapering_over_its_level_structure() {
    let mut s = Session::new();
    let roof = drained_roof(&mut s);
    let id = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
    let view = crate::render::View { angle: 0.0, scale: 0.5, cx: 5000.0, cy: 3150.0, width: 6000, height: 800 };
    let d = crate::render::section(&s.model, id, view);
    let mine = || d.prims.iter().zip(&d.owners).filter(|(_, o)| **o == Some(roof)).map(|(p, _)| p);
    let corners: Vec<(f32, f32)> = mine()
        .filter_map(|p| match p {
            crate::render::Prim::Fill { rings, .. } => Some(rings.concat()),
            _ => None,
        })
        .flatten()
        .collect();
    let lines: Vec<&Vec<(f32, f32)>> = mine()
        .filter_map(|p| match p {
            crate::render::Prim::Line { pts, .. } => Some(pts),
            _ => None,
        })
        .collect();
    let close = |c: &(f32, f32), along: f64, z: f64| {
        let (x, y) = view.px(P2(along, z));
        (c.0 - x).abs() < 0.5 && (c.1 - y).abs() < 0.5
    };
    let corner = |along: f64, z: f64| corners.iter().any(|c| close(c, along, z));
    let between = |along: f64, z: f64| lines.iter().any(|l| l.iter().any(|c| close(c, along, z)));
    // The top falls to the drain; the underside doesn't.
    assert!(corner(5000.0, 3270.0) && corner(5000.0, 3000.0) && !corner(5000.0, 2960.0), "a level underside");
    // Under the finish, 10 mm under the top; over the structure, level.
    assert!(between(5000.0, 3260.0) && between(1000.0, 3300.0), "the finish follows the top");
    assert!(between(5000.0, 3200.0) && between(1000.0, 3200.0), "the structure stays level");
}

/// Each plane of floor `id`'s top, as its corners lifted, and its plane.
fn planes(s: &Session, id: u64) -> Vec<(Vec<[f64; 3]>, crate::walls::Plane)> {
    let f = s.model.shapeable(id).expect("a shaped floor or roof");
    s.model
        .shape_planes(&f)
        .expect("it triangulates")
        .into_iter()
        .map(|(loops, plane)| (loops[0].iter().map(|q| [q.0, q.1, plane.at(*q)]).collect(), plane))
        .collect()
}

#[test]
fn falls_to_two_drains_fall_at_least_as_steep_as_asked_and_every_plane_runs_to_one() {
    let mut s = Session::new();
    // 12 by 6 m, a drain in each half.
    let f = run(&mut s, "floor", json!({"boundary": [[0, 0], [12000, 0], [12000, 6000], [0, 6000]]}))["id"].as_u64().unwrap();
    let reply = run(&mut s, "falls", json!({"id": f, "drains": [[3000, 3000], [9000, 3000]], "fall": 1.5}));
    // Each drain as low as its farthest corner, 3000 by 3000 mm away, at 1.5 %: 63.64 mm, to the tenth down.
    let drains = reply["drains"].as_array().unwrap();
    assert_eq!(drains.len(), 2, "{reply}");
    for d in drains {
        assert!((d["z"].as_f64().unwrap() + 63.7).abs() < 1e-9, "{reply}");
    }
    let tops = planes(&s, f);
    assert!(tops.len() >= 8, "{} planes", tops.len());
    let flat = top(&s, f, 0.0, 0.0);
    for (ring, plane) in &tops {
        let fall = plane.g.dot(plane.g).sqrt() * 100.0;
        assert!(fall >= 1.5 - 1e-9, "a plane falling {fall} %: {ring:?}");
        // Its lowest corner is a drain, and none of it is over the top as it was.
        let low = ring.iter().min_by(|a, b| a[2].total_cmp(&b[2])).unwrap();
        assert!([[3000.0, 3000.0], [9000.0, 3000.0]].iter().any(|d| (d[0], d[1]) == (low[0], low[1])), "{ring:?}");
        assert!(ring.iter().all(|q| q[2] <= flat + 1e-9), "{ring:?}");
    }
    // The ridge between them runs across at the top's height.
    assert!(near(top(&s, f, 6000.0, 3000.0), flat) && near(top(&s, f, 6000.0, 0.0), flat));
    // Again, the shape given anew; a roof takes falls too.
    run(&mut s, "falls", json!({"id": f, "drains": [[6000, 3000]], "fall": 2}));
    assert_eq!(run(&mut s, "edit_shape", json!({"id": f}))["points"].as_array().map(Vec::len), Some(1));
    let roof = flat_roof(&mut s);
    let reply = run(&mut s, "falls", json!({"id": roof, "drains": [[4000, 3000]], "fall": 1}));
    assert!((reply["drains"][0]["z"].as_f64().unwrap() + 50.0).abs() < 1e-9, "{reply}");
}

#[test]
fn falls_refuse_what_they_cant_drain_saying_why() {
    let mut s = Session::new();
    let f = slab(&mut s);
    let e = fails(&mut s, "falls", json!({"id": f, "drains": [[9000, 3000]], "fall": 1.5}));
    assert!(e.contains("outside"), "{e}");
    let e = fails(&mut s, "falls", json!({"id": f, "drains": [], "fall": 1.5}));
    assert!(e.contains("drain"), "{e}");
    let e = fails(&mut s, "falls", json!({"id": f, "drains": [[4000, 3000]], "fall": 0}));
    assert!(e.contains("fall"), "{e}");
    let e = fails(&mut s, "falls", json!({"id": f, "drains": [[4000, 3000], [4000.5, 3000]], "fall": 1}));
    assert!(e.contains("millimetre"), "{e}");
    // An L whose far wing the drain can't see: a drain there too, it says.
    let l = run(
        &mut s,
        "floor",
        json!({"level": 100001, "boundary": [[0, 0], [6000, 0], [6000, 2000], [2000, 2000], [2000, 6000], [0, 6000]]}),
    )["id"]
        .as_u64()
        .unwrap();
    let e = fails(&mut s, "falls", json!({"id": l, "drains": [[5000, 1000]], "fall": 1}));
    assert!(e.contains("can't see"), "{e}");
    run(&mut s, "falls", json!({"id": l, "drains": [[5000, 1000], [1000, 5000]], "fall": 1}));
    // A drain in an opening.
    let o = run(
        &mut s,
        "floor",
        json!({"boundary": [[20000, 0], [26000, 0], [26000, 6000], [20000, 6000]], "openings": [[[22000, 2000], [23000, 2000], [23000, 3000], [22000, 3000]]]}),
    )["id"]
        .as_u64()
        .unwrap();
    let e = fails(&mut s, "falls", json!({"id": o, "drains": [[22500, 2500]], "fall": 1}));
    assert!(e.contains("opening"), "{e}");
}

#[test]
fn a_shaped_slab_exported_and_imported_comes_back_with_its_points_and_creases() {
    let mut s = Session::new();
    let f = slab(&mut s);
    run(
        &mut s,
        "edit_shape",
        json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}, {"at": [6000, 1000], "z": -10}], "lines": [{"a": [0, 6000], "b": [4000, 3000]}]}),
    );
    let before = run(&mut s, "edit_shape", json!({"id": f}));
    let file = crate::ifc::export(&s.model);
    let i = crate::import::ifc(file.as_bytes(), "x").unwrap();
    assert_eq!(i.skipped, Vec::<String>::new());
    let (id, back) = i
        .model
        .elements
        .iter()
        .find_map(|(id, e)| match e {
            crate::model::Element::Floor(f) => Some((*id, f.clone())),
            _ => None,
        })
        .expect("a floor");
    // The same top, plane for plane: each point at its height, under the highest corner, which stays at 0.
    let height = |m: &crate::model::Model, f: &crate::model::Floor, q: P2| m.floor_top(f).at(q) + m.shape_height(f, q);
    let Some(crate::model::Element::Floor(was)) = s.model.elements.get(f).cloned() else { panic!("no floor") };
    for q in [P2(4000.0, 3000.0), P2(6000.0, 1000.0), P2(0.0, 0.0), P2(8000.0, 6000.0), P2(2000.0, 4500.0), P2(7000.0, 500.0)] {
        assert!(near(height(&i.model, &back, q), height(&s.model, &was, q)), "at {q:?}");
    }
    let shape = crate::commands::element_json(&i.model, id);
    assert_eq!(shape["top"], element(&mut s, f)["top"], "{shape}");
    assert_eq!(back.shape.points.len(), 2, "{:?}", back.shape);
    assert!(
        back.shape.lines.iter().any(|l| [l.a, l.b].contains(&P2(0.0, 6000.0)) && [l.a, l.b].contains(&P2(4000.0, 3000.0))),
        "the split line: {:?} (was {before})",
        back.shape.lines
    );
    // As thick as it was.
    assert_eq!(i.model.floor_depth(&back), s.model.floor_depth(&was));
}

#[test]
fn a_spot_slope_reads_how_steeply_a_shaped_top_falls_and_which_way() {
    let mut s = Session::new();
    let f = slab(&mut s);
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}]}));
    // On the west plane, 30 mm down over 4 m toward the drain: 0.75 %, falling east.
    let r = run(&mut s, "spot_slope", json!({"at": [1000, 3000]}));
    assert_eq!((r["slope"].clone(), r["toward"].clone(), r["host"].clone()), (json!(0.75), json!([1.0, 0.0]), json!(f)), "{r}");
    let spot = r["id"].as_u64().unwrap();
    let e = element(&mut s, spot);
    assert_eq!((e["category"].clone(), e["slope"].clone()), (json!("spot"), json!(0.75)), "{e}");
    // It follows the top: the drain lower, the slope steeper.
    run(&mut s, "add_point", json!({"id": f, "at": [4000, 3000], "z": -60}));
    assert_eq!(element(&mut s, spot)["slope"], json!(1.5));
    // In the plan: an arrow from its point, running east, downhill.
    let view = crate::render::View { angle: 0.0, scale: 0.05, cx: 4000.0, cy: 3000.0, width: 800, height: 600 };
    let d = crate::render::draw(&s.model, 100_000u64, view);
    let (x, y) = view.px(P2(1000.0, 3000.0));
    let pts: Vec<(f32, f32)> = d.prims.iter().zip(&d.owners).filter(|(_, o)| **o == Some(spot)).flat_map(|(p, _)| p.points()).collect();
    assert!(!pts.is_empty());
    let east = pts.iter().map(|p| p.0 - x).fold(f32::NEG_INFINITY, f32::max);
    let west = pts.iter().map(|p| x - p.0).fold(f32::NEG_INFINITY, f32::max);
    assert!(east > 20.0 && west < 5.0 && pts.iter().all(|p| (p.1 - y).abs() < 30.0), "{pts:?}");
    // A level floor reads 0; not in a section; nothing to read, it says.
    let level = run(&mut s, "floor", json!({"boundary": [[20000, 0], [24000, 0], [24000, 4000], [20000, 4000]]}))["id"].as_u64().unwrap();
    let r = run(&mut s, "spot_slope", json!({"at": [22000, 2000], "host": level}));
    assert_eq!((r["slope"].clone(), r["toward"].clone()), (json!(0.0), json!(null)), "{r}");
    let e = fails(&mut s, "spot_slope", json!({"at": [50000, 50000]}));
    assert!(e.contains("floor or a roof"), "{e}");
    let section = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
    let e = fails(&mut s, "spot_slope", json!({"at": [1000, 0], "view": section}));
    assert!(e.contains("plan"), "{e}");
}

/// The 8 by 6 m floor with a drain 30 mm down in its middle, a point 10 mm down on the way to it, and a split line from
/// corner to corner through both; its outline as a sketch, and that with a hole from `lo` to `hi`.
fn creased(s: &mut Session) -> u64 {
    let f = slab(s);
    run(
        s,
        "edit_shape",
        json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}, {"at": [2000, 1500], "z": -10}], "lines": [{"a": [0, 0], "b": [8000, 6000]}]}),
    );
    f
}

fn with_hole(lo: [f64; 2], hi: [f64; 2]) -> Value {
    let edge = |a: [f64; 2], b: [f64; 2]| json!([a, b]);
    let (x0, y0, x1, y1) = (lo[0], lo[1], hi[0], hi[1]);
    json!([
        edge([0.0, 0.0], [8000.0, 0.0]),
        edge([8000.0, 0.0], [8000.0, 6000.0]),
        edge([8000.0, 6000.0], [0.0, 6000.0]),
        edge([0.0, 6000.0], [0.0, 0.0]),
        edge([x0, y0], [x1, y0]),
        edge([x1, y0], [x1, y1]),
        edge([x1, y1], [x0, y1]),
        edge([x0, y1], [x0, y0])
    ])
}

/// How far floor `id`'s top lies over where it would be without its shape at `q`.
fn height_at(s: &Session, id: u64, q: P2) -> f64 {
    let f = s.model.shapeable(id).expect("a floor");
    s.model.shape_height(&f, q)
}

#[test]
fn a_hole_cut_through_a_shape_cuts_its_split_lines_and_keeps_the_heights_round_it() {
    // Round the drain: the drain goes; the split line is cut at the hole, both pieces kept, their new ends and the hole's
    // corners at the heights the top had there.
    let mut s = Session::new();
    let f = creased(&mut s);
    let rim = [P2(3500.0, 2500.0), P2(4500.0, 2500.0), P2(4500.0, 3500.0), P2(3500.0, 3500.0), P2(3500.0, 2625.0), P2(4500.0, 3375.0)];
    let before: Vec<f64> = rim.iter().map(|q| height_at(&s, f, *q)).collect();
    let r = run(&mut s, "edit_boundary", json!({"id": f, "sketch": with_hole([3500.0, 2500.0], [4500.0, 3500.0])}));
    assert_eq!(r["dropped"]["points"], json!([[4000.0, 3000.0]]), "{r}");
    assert_eq!(r["cut"].as_array().map(Vec::len), Some(1), "{r}");
    let shape = run(&mut s, "edit_shape", json!({"id": f}));
    let lines = shape["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 2, "{shape}");
    for (q, z) in rim.iter().zip(&before) {
        assert!((height_at(&s, f, *q) - z).abs() < 0.01, "at {q:?}: {} for {z}", height_at(&s, f, *q));
    }
    assert!(near(before[4], -25.0) && near(before[5], -26.25), "{before:?}");
    // The point kept keeps its height.
    assert!(near(height_at(&s, f, P2(2000.0, 1500.0)), -10.0));
    // Off the drain, crossing the split line only: it's cut, nothing refused.
    let mut s = Session::new();
    let f = creased(&mut s);
    let r = run(&mut s, "edit_boundary", json!({"id": f, "sketch": with_hole([5500.0, 3800.0], [6500.0, 4800.0])}));
    assert_eq!((r["cut"].as_array().map(Vec::len), r.get("dropped")), (Some(1), None), "{r}");
    assert_eq!(run(&mut s, "edit_shape", json!({"id": f}))["lines"].as_array().map(Vec::len), Some(2), "its two pieces");
    // Clear of it all: the shape as it was, the hole's corners at the heights the top had there.
    let mut s = Session::new();
    let f = creased(&mut s);
    let corner = P2(6000.0, 1500.0);
    let was = height_at(&s, f, corner);
    let r = run(&mut s, "edit_boundary", json!({"id": f, "sketch": with_hole([6000.0, 500.0], [7000.0, 1500.0])}));
    assert_eq!((r.get("dropped"), r.get("cut")), (None, None), "{r}");
    assert!((height_at(&s, f, corner) - was).abs() < 0.01, "{was}");
    // A roof's courtyard alike.
    let mut s = Session::new();
    let roof = flat_roof(&mut s);
    run(&mut s, "edit_shape", json!({"id": roof, "points": [{"at": [4000, 3000], "z": -40}], "lines": [{"a": [0, 0], "b": [4000, 3000]}]}));
    let r = run(&mut s, "edit_boundary", json!({"id": roof, "sketch": with_hole([1500.0, 500.0], [2500.0, 2500.0])}));
    assert_eq!(r["cut"].as_array().map(Vec::len), Some(1), "{r}");
    assert!(near(height_at(&s, roof, P2(1500.0, 1125.0)), -15.0));
}

/// Every plane of floor `id`'s top falls at least `fall` %, toward a drain or an opening's corner under the top (from which
/// a valley runs on down to a drain), its lowest corner; and each drain is the lowest of the planes round it.
fn falls_everywhere(s: &Session, id: u64, drains: &[[f64; 2]], fall: f64) {
    let flat = top(s, id, drains[0][0], drains[0][1]) - height_at(s, id, P2(drains[0][0], drains[0][1]));
    for (ring, plane) in planes(s, id) {
        let f = plane.g.dot(plane.g).sqrt() * 100.0;
        assert!(f >= fall - 1e-9, "a plane falling {f} %: {ring:?}");
        let low = ring.iter().min_by(|a, b| a[2].total_cmp(&b[2])).unwrap();
        assert!(low[2] < flat - 1e-6, "a plane with nowhere lower to run: {ring:?}");
    }
}

#[test]
fn falls_run_round_an_opening_to_two_drains_or_to_one_beside_it() {
    let mut s = Session::new();
    // 12 by 6 m, a 1 m opening where the two halves meet.
    let hole = json!([[[5500, 2500], [6500, 2500], [6500, 3500], [5500, 3500]]]);
    let f =
        run(&mut s, "floor", json!({"boundary": [[0, 0], [12000, 0], [12000, 6000], [0, 6000]], "openings": hole}))["id"].as_u64().unwrap();
    let drains = [[3000.0, 3000.0], [9000.0, 3000.0]];
    let reply = run(&mut s, "falls", json!({"id": f, "drains": drains, "fall": 1.5}));
    assert_eq!(reply["drains"].as_array().map(Vec::len), Some(2), "{reply}");
    falls_everywhere(&s, f, &drains, 1.5);
    // The opening's rim on its drains' cones, under the top; where the halfway line meets it, at the top's height.
    assert!(height_at(&s, f, P2(5500.0, 2500.0)) < -1.0 && height_at(&s, f, P2(6500.0, 3500.0)) < -1.0);
    assert!(near(height_at(&s, f, P2(6000.0, 2500.0)), 0.0));
    // One drain beside it: the far side's water runs round the opening's corners, on the drain's cone.
    run(&mut s, "falls", json!({"id": f, "drains": [[3000, 3000]], "fall": 1.5}));
    falls_everywhere(&s, f, &[[3000.0, 3000.0]], 1.5);
    assert!(height_at(&s, f, P2(6500.0, 3500.0)) < height_at(&s, f, P2(12000.0, 6000.0)));
}

#[test]
fn ridges_drawn_split_the_top_each_part_to_its_own_drains() {
    let mut s = Session::new();
    let f = run(&mut s, "floor", json!({"boundary": [[0, 0], [10000, 0], [10000, 6000], [0, 6000]]}))["id"].as_u64().unwrap();
    // A ridge across, off the middle: the drains' halfway line would be elsewhere; the ridge is where the parts meet.
    let ridge = json!([[[4000, 0], [4000, 6000]]]);
    let drains = [[2000.0, 3000.0], [7000.0, 3000.0]];
    let reply = run(&mut s, "falls", json!({"id": f, "drains": drains, "fall": 1.5, "ridges": ridge}));
    falls_everywhere(&s, f, &drains, 1.5);
    assert!(near(height_at(&s, f, P2(4000.0, 3000.0)), 0.0), "{reply}");
    // The far drain is as deep as its farthest corner, 3000 by 3000 mm away at 1.5 %: 63.7.
    assert!((reply["drains"][1]["z"].as_f64().unwrap() + 63.7).abs() < 1e-9, "{reply}");
    // A part with no drain; a ridge that divides nothing.
    let e = fails(&mut s, "falls", json!({"id": f, "drains": [[2000, 3000]], "fall": 1.5, "ridges": ridge}));
    assert!(e.contains("no drain"), "{e}");
    let e = fails(&mut s, "falls", json!({"id": f, "drains": drains, "fall": 1.5, "ridges": [[[4000, 0], [4000, 3000]]]}));
    assert!(e.contains("divide"), "{e}");
}

/// The first floor and the first roof a project holds.
fn first_floor(m: &crate::model::Model) -> Option<crate::model::Floor> {
    m.elements.values().find_map(|e| match e {
        crate::model::Element::Floor(f) => Some(f.clone()),
        _ => None,
    })
}

fn first_roof(m: &crate::model::Model) -> Option<crate::model::Roof> {
    m.elements.values().find_map(|e| match e {
        crate::model::Element::Roof(r) => Some(r.clone()),
        _ => None,
    })
}

#[test]
fn a_shaped_floor_over_a_variable_layer_comes_back_from_ifc_with_it_marked_or_guessed() {
    let mut s = Session::new();
    let f = drained(&mut s);
    let file = crate::ifc::export(&s.model);
    assert!(file.contains("'Variable'"), "the variable layer says so");
    let Some(was) = first_floor(&s.model) else { panic!("no floor") };
    let check = |file: &str| {
        let i = crate::import::ifc(file.as_bytes(), "x").unwrap();
        let back = first_floor(&i.model).expect("a floor");
        let layers = i.model.type_layers("floor", &back.type_name);
        let variable: Vec<bool> = layers.iter().map(|l| l.variable).collect();
        assert_eq!(variable, [false, true, false, false], "{layers:?}");
        // The underside level again, the top as it was.
        assert!(i.model.floor_under(&back).is_some());
        for q in [P2(4000.0, 3000.0), P2(1000.0, 1000.0), P2(8000.0, 6000.0)] {
            let h = |m: &crate::model::Model, f: &crate::model::Floor| m.floor_top(f).at(q) + m.shape_height(f, q);
            assert!(near(h(&i.model, &back), h(&s.model, &was)), "at {q:?}");
        }
        assert!((i.model.floor_volume(&back) - s.model.floor_volume(&was)).abs() < 1e3, "the same volume, to a cm³");
    };
    check(&file);
    // From another program, saying nothing of it: the underside level under a shaped top, the screed is the one.
    check(&file.replace("'Variable'", "$"));
    let _ = f;
}

#[test]
fn a_shaped_flat_roof_comes_back_from_ifc_shaped_and_a_pitched_one_pitched() {
    let mut s = Session::new();
    let roof = drained_roof(&mut s);
    run(&mut s, "add_point", json!({"id": roof, "at": [6000, 1000], "z": -20}));
    let pitched = run(&mut s, "roof", json!({"level": 100001, "boundary": [[20000, 0], [28000, 0], [28000, 6000], [20000, 6000]]}))["id"]
        .as_u64()
        .unwrap();
    let file = crate::ifc::export(&s.model);
    let i = crate::import::ifc(file.as_bytes(), "x").unwrap();
    assert_eq!(i.skipped, Vec::<String>::new());
    let roofs: Vec<crate::model::Roof> = i
        .model
        .elements
        .values()
        .filter_map(|e| match e {
            crate::model::Element::Roof(r) => Some(r.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(roofs.len(), 2);
    let (flat, hip) = if roofs[0].shape.is_empty() { (&roofs[1], &roofs[0]) } else { (&roofs[0], &roofs[1]) };
    assert!(hip.slope > 0.0 && hip.shape.is_empty(), "{hip:?}");
    assert_eq!((flat.slope, flat.shape.points.len()), (0.0, 2), "{flat:?}");
    // Its top as it was, its insulation the variable layer, its underside level.
    let Some(crate::model::Element::Roof(was)) = s.model.elements.get(roof).cloned() else { panic!("no roof") };
    for q in [P2(4000.0, 3000.0), P2(6000.0, 1000.0), P2(0.0, 0.0), P2(2000.0, 5000.0)] {
        let h = |m: &crate::model::Model, r: &crate::model::Roof| {
            let f = m.roof_as_floor(r);
            m.floor_top(&f).at(q) + m.shape_height(&f, q)
        };
        assert!(near(h(&i.model, flat), h(&s.model, &was)), "at {q:?}: {} for {}", h(&i.model, flat), h(&s.model, &was));
    }
    assert!(i.model.roof_under(flat).is_some(), "{:?}", i.model.type_layers("roof", &flat.type_name));
    let _ = (pitched, first_roof(&i.model));
}

#[test]
fn in_3d_a_cursor_near_an_edge_snaps_onto_it_and_names_the_edge() {
    use crate::view3d::{ShapeSnap, shape_snap_3d};
    let mut s = Session::new();
    let f = slab(&mut s);
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -300}]}));
    for lens in [None, Some(35.0)] {
        let mut cam = camera(&s);
        cam.lens = lens;
        let fl = s.model.shapeable(f).unwrap();
        let screen = |p: [f64; 3]| {
            let (x, y, _) = cam.project(p);
            (x, y)
        };
        // A few pixels off the south edge, a quarter of the way along: on it, there.
        let (x, y) = screen([2000.0, 0.0, 0.0]);
        let (q, edge, _) = shape_snap_3d(&s.model, &fl, &cam, (x + 2.0, y + 3.0), 8.0).expect("the edge");
        assert!((q.1).abs() < 1.0 && (q.0 - 2000.0).abs() < 150.0, "{q:?} ({lens:?})");
        let [a, b] = edge.expect("an edge, to light up");
        assert!(a[1].abs() < 1e-6 && b[1].abs() < 1e-6, "{a:?} {b:?}");
        // Off a crease from a corner to the drain: on it, at its height.
        let mid = [2000.0, 1500.0, -150.0];
        let (x, y) = screen(mid);
        let (q, edge, _) = shape_snap_3d(&s.model, &fl, &cam, (x + 1.0, y - 2.0), 8.0).expect("the crease");
        assert!(q.dist(P2(2000.0, 1500.0)) < 150.0 && edge.is_some(), "{q:?}");
        // Near the drain: the drain itself, no edge.
        let (x, y) = screen([4000.0, 3000.0, -300.0]);
        assert_eq!(shape_snap_3d(&s.model, &fl, &cam, (x + 3.0, y), 8.0), Some((P2(4000.0, 3000.0), None, ShapeSnap::Endpoint)));
        // By the south edge's middle: the middle.
        let (x, y) = screen([4000.0, 0.0, 0.0]);
        let (q, _, kind) = shape_snap_3d(&s.model, &fl, &cam, (x + 3.0, y + 2.0), 8.0).expect("the middle");
        assert_eq!((q, kind), (P2(4000.0, 0.0), ShapeSnap::Midpoint));
        // Out in the middle of a plane: nothing to snap to.
        let (x, y) = screen([2500.0, 2800.0, -150.0]);
        assert_eq!(shape_snap_3d(&s.model, &fl, &cam, (x, y), 8.0), None);
    }
}

#[test]
fn in_plan_a_cursor_near_an_edge_snaps_onto_it_and_names_the_edge() {
    use crate::view3d::{ShapeSnap, shape_snap_plan};
    let mut s = Session::new();
    let f = slab(&mut s);
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -300}]}));
    let fl = s.model.shapeable(f).unwrap();
    // Off the south edge: onto it, the edge named.
    let (q, edge, kind) = shape_snap_plan(&s.model, &fl, P2(2000.0, 40.0), 100.0).expect("the edge");
    assert_eq!((q, edge, kind), (P2(2000.0, 0.0), Some((P2(0.0, 0.0), P2(8000.0, 0.0))), ShapeSnap::Nearest));
    // By its middle: the middle, first.
    let (q, _, kind) = shape_snap_plan(&s.model, &fl, P2(4030.0, 50.0), 100.0).expect("the middle");
    assert_eq!((q, kind), (P2(4000.0, 0.0), ShapeSnap::Midpoint));
    // Off a crease to the drain: onto it.
    let (q, edge, _) = shape_snap_plan(&s.model, &fl, P2(2030.0, 1460.0), 100.0).expect("the crease");
    assert!(q.dist(P2(2030.0, 1460.0)) < 60.0 && (q.1 - 0.75 * q.0).abs() < 1e-6 && edge.is_some(), "{q:?}");
    // By the drain: the drain.
    assert_eq!(shape_snap_plan(&s.model, &fl, P2(4050.0, 2960.0), 100.0), Some((P2(4000.0, 3000.0), None, ShapeSnap::Endpoint)));
    assert_eq!(shape_snap_plan(&s.model, &fl, P2(2500.0, 2800.0), 100.0), None);
}

#[test]
fn add_point_with_an_offset_lands_that_far_over_the_top_as_it_is_there() {
    let mut s = Session::new();
    let f = slab(&mut s);
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -300}]}));
    // Halfway from a corner to the drain the top is 150 mm down: 20 over it is 130 down.
    run(&mut s, "add_point", json!({"id": f, "at": [2000, 1500], "offset": 20}));
    assert!(near(height_at(&s, f, P2(2000.0, 1500.0)), -130.0), "{}", height_at(&s, f, P2(2000.0, 1500.0)));
    // Not both.
    let e = fails(&mut s, "add_point", json!({"id": f, "at": [6000, 1500], "offset": 20, "z": 5}));
    assert!(e.contains("offset"), "{e}");
}

#[test]
fn a_shapes_folding_lines_are_its_creases_no_split_line_makes_and_they_can_be_picked() {
    use crate::view3d::{shape_pick_3d, shape_pick_plan};
    let mut s = Session::new();
    let f = slab(&mut s);
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -300}], "lines": [{"a": [0, 0], "b": [4000, 3000]}]}));
    let fl = s.model.shapeable(f).unwrap();
    // A drain with a split line to one corner: the creases to the other three fold of themselves.
    let folds = s.model.shape_folds(&fl);
    assert_eq!(folds.len(), 3, "{folds:?}");
    assert!(!folds.iter().any(|(a, b)| [a, b].contains(&&P2(0.0, 0.0))), "the split line isn't one");
    // Picked in plan off one's middle, its ends; a split line and an outline edge as before.
    let mut ends = shape_pick_plan(&s.model, &fl, P2(6030.0, 1480.0), 100.0);
    ends.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(ends, [P2(4000.0, 3000.0), P2(8000.0, 0.0)]);
    assert_eq!(shape_pick_plan(&s.model, &fl, P2(4000.0, 40.0), 100.0).len(), 2);
    // And in 3D.
    let cam = camera(&s);
    let (x, y, _) = cam.project([6000.0, 1500.0, -150.0]);
    let mut ends = shape_pick_3d(&s.model, &fl, &cam, (x + 1.0, y + 1.0), 8.0);
    ends.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(ends, [P2(4000.0, 3000.0), P2(8000.0, 0.0)]);
}
