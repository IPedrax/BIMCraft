//! Dimensions driven through commands as a front end drives them: to points as well as lines (the ends of walls, their
//! faces' corners, detail lines and grids), aligned along the points or linear (horizontal or vertical), and following what
//! they measure.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Id, Model};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn err(s: &mut Session, c: &str, p: Value) -> String {
    s.execute(c, &p).err().unwrap_or_else(|| panic!("{c} {p} was accepted"))
}

fn id_of(v: &Value) -> Id {
    v["id"].as_u64().unwrap_or_else(|| panic!("no id in {v}"))
}

/// A box of four walls, 8000 by 6000 on their centre lines, 200 mm thick: south, east, north, west.
fn house(s: &mut Session) -> Vec<Id> {
    let r = run(s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
    r["ids"].as_array().map(|ids| ids.iter().filter_map(Value::as_u64).collect()).unwrap_or_default()
}

fn values(s: &mut Session, id: Id) -> Value {
    crate::commands::element_json(&s.model, id)["values"].clone()
}

#[test]
fn a_dimension_measures_between_two_endpoints_aligned_horizontal_or_vertical() {
    let mut s = Session::new();
    let w = house(&mut s);
    // The south wall's start, its centre line's west end, to the east wall's end, at the north-east corner.
    let (from, to) = (json!({"id": w[0], "point": "start"}), json!({"id": w[1], "point": "end"}));
    let aligned = run(&mut s, "dimension", json!({"refs": [from, to], "at": [3000, 4000]}));
    assert_eq!(aligned["values"], json!([10000.0]));
    let across = run(&mut s, "dimension", json!({"kind": "linear", "orient": "horizontal", "refs": [from, to], "at": [0, 9000]}));
    assert_eq!(across["values"], json!([8000.0]));
    let up = run(&mut s, "dimension", json!({"kind": "linear", "orient": "vertical", "refs": [from, to], "at": [12000, 0]}));
    assert_eq!(up["values"], json!([6000.0]));
    // Without an orientation, a linear one runs as `at` says: past the points' east or west, vertical; else horizontal.
    let auto = run(&mut s, "dimension", json!({"kind": "linear", "refs": [from, to], "at": [11000, 3000]}));
    assert_eq!(auto["values"], json!([6000.0]));
    assert_eq!(crate::commands::element_json(&s.model, id_of(&auto))["orient"], json!("vertical"));
    let auto = run(&mut s, "dimension", json!({"kind": "linear", "refs": [from, to], "at": [4000, -2000]}));
    assert_eq!(auto["values"], json!([8000.0]));
    // The east wall moves east: all of them follow.
    run(&mut s, "move", json!({"ids": [w[1]], "by": [1000, 0]}));
    assert_eq!(values(&mut s, id_of(&across)), json!([9000.0]));
    assert_eq!(values(&mut s, id_of(&up)), json!([6000.0]));
    let v = values(&mut s, id_of(&aligned))[0].as_f64().unwrap_or_default();
    assert!((v - 9000f64.hypot(6000.0)).abs() < 0.01, "{v}");
}

#[test]
fn an_endpoint_is_found_near_a_point_a_wall_face_ending_at_its_corner() {
    let mut s = Session::new();
    let w = house(&mut s);
    let a = id_of(&run(&mut s, "detail_line", json!({"start": [1000, 2000], "end": [3000, 4000]})));
    // A detail line's end, and the south wall's outer face's east end, where the walls' outlines meet: [8100, -100].
    let d = run(
        &mut s,
        "dimension",
        json!({"refs": [{"endpoint": [3010, 3990]}, {"endpoint": [8090, -90]}], "kind": "linear", "orient": "horizontal", "at": [5000, -3000]}),
    );
    assert_eq!(d["values"], json!([5100.0]));
    let refs = crate::commands::element_json(&s.model, id_of(&d))["refs"].clone();
    assert_eq!(refs[0], json!({"id": s.model.guid(a), "face": "centre", "point": "end"}));
    let corner = &refs[1];
    assert!(corner["point"].is_string() && corner["face"] != json!("centre"), "a face's end: {corner}");
    // The inner corner is the inner face's end.
    let d = run(&mut s, "dimension", json!({"refs": [{"endpoint": [7900, 100]}, {"endpoint": [100, 100]}], "at": [4000, 1000]}));
    assert_eq!(d["values"], json!([7800.0]));
    let e = err(&mut s, "dimension", json!({"refs": [{"endpoint": [4000, 3000]}, {"endpoint": [100, 100]}], "at": [0, 0]}));
    assert!(e.contains("no end"), "{e}");
    let _ = w;
}

#[test]
fn points_and_lines_dimension_together_and_lines_keep_square_to_a_linear_one() {
    let mut s = Session::new();
    let w = house(&mut s);
    let grid = id_of(&run(&mut s, "grid", json!({"start": [3000, -2000], "end": [3000, 8000]})));
    // A grid running north and a corner: square to the grid, so across it.
    let d = run(&mut s, "dimension", json!({"refs": [{"id": grid}, {"id": w[1], "point": "end"}], "at": [0, 7000]}));
    assert_eq!(d["values"], json!([5000.0]));
    let d = run(
        &mut s,
        "dimension",
        json!({"kind": "linear", "orient": "horizontal", "refs": [{"id": w[1], "point": "end"}, {"id": grid}], "at": [0, 7000]}),
    );
    assert_eq!(d["values"], json!([5000.0]));
    // A horizontal dimension can't measure to a line running east.
    let e = err(
        &mut s,
        "dimension",
        json!({"kind": "linear", "orient": "vertical", "refs": [{"id": grid}, {"id": w[1], "point": "end"}], "at": [0, 7000]}),
    );
    assert!(e.contains("square"), "{e}");
    // An orientation is a linear one's.
    let e = err(&mut s, "dimension", json!({"orient": "vertical", "refs": [{"id": grid}, {"id": w[1], "point": "end"}], "at": [0, 7000]}));
    assert!(e.contains("linear"), "{e}");
    // Two points in one place have no way to run.
    let e = err(&mut s, "dimension", json!({"refs": [{"id": w[0], "point": "end"}, {"id": w[1], "point": "start"}], "at": [0, 7000]}));
    assert!(e.contains("one place"), "{e}");
    // An angle measures between lines, not their ends.
    let e = err(&mut s, "dimension", json!({"kind": "angular", "refs": [{"id": w[0], "point": "end"}, {"id": w[1]}], "at": [0, 7000]}));
    assert!(e.contains("lines"), "{e}");
}

#[test]
fn a_mirrored_or_flipped_wall_keeps_its_dimension_on_the_same_corner() {
    let mut s = Session::new();
    let w = house(&mut s);
    let d = id_of(&run(
        &mut s,
        "dimension",
        json!({"refs": [{"id": w[0], "point": "start"}, {"id": w[0], "point": "end"}], "at": [4000, -1000]}),
    ));
    run(&mut s, "mirror", json!({"ids": [w[0], w[1], w[2], w[3], d], "axis": [[10000, 0], [10000, 1000]]}));
    assert_eq!(values(&mut s, d), json!([8000.0]));
    let d = id_of(&run(
        &mut s,
        "dimension",
        json!({"kind": "linear", "orient": "horizontal", "refs": [{"id": w[0], "face": "left", "point": "end"}, {"id": w[2], "face": "right", "point": "start"}], "at": [16000, 9000]}),
    ));
    let before = values(&mut s, d);
    run(&mut s, "flip", json!({"ids": [w[0]]}));
    assert_eq!(values(&mut s, d), before, "the same corner");
}

#[test]
fn a_file_keeps_a_dimension_s_points_and_orientation() {
    let mut s = Session::new();
    let w = house(&mut s);
    let d = id_of(&run(
        &mut s,
        "dimension",
        json!({"kind": "linear", "orient": "vertical", "refs": [{"id": w[0], "point": "start"}, {"id": w[2], "point": "start"}], "at": [-2000, 0]}),
    ));
    let text = s.model.to_json().unwrap_or_else(|e| panic!("{e}"));
    let (m, left_out) = Model::read(&text).unwrap_or_else(|e| panic!("{e}"));
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.elements.get(d), s.model.elements.get(d));
    assert!(text.contains(&format!("\"format\": {}", crate::model::FORMAT)), "points need format 10, and a save is the current format");
}

#[test]
fn a_dimension_to_points_draws_its_witness_lines_from_the_points() {
    let mut s = Session::new();
    let a = id_of(&run(&mut s, "detail_line", json!({"start": [-3000, 3000], "end": [-1000, 1000]})));
    let b = id_of(&run(&mut s, "detail_line", json!({"start": [4000, 1000], "end": [6000, 3000]})));
    run(
        &mut s,
        "dimension",
        json!({"kind": "linear", "orient": "horizontal", "refs": [{"id": a, "point": "end"}, {"id": b, "point": "start"}], "at": [0, -2000]}),
    );
    let level = s.model.levels().first().map(|l| l.0).unwrap_or_default();
    let d = crate::render::plan(&s.model, level, 800, 600);
    let png = crate::render::png(&d).unwrap_or_default();
    let pm = tiny_skia::Pixmap::decode_png(&png).unwrap_or_else(|e| panic!("{e}"));
    let dark = |p: crate::geom::P2| {
        let (x, y) = d.view.px(p);
        pm.pixel(x as u32, y as u32).is_some_and(|c| c.red() < 200 && c.green() < 200 && c.blue() < 200)
    };
    assert!(dark(crate::geom::P2(1500.0, -2000.0)), "the dimension line, horizontal");
    assert!(dark(crate::geom::P2(-1000.0, -500.0)), "a witness line down from the first point");
    assert!(dark(crate::geom::P2(4000.0, -500.0)), "and from the second");
}

#[test]
fn a_diameter_dimension_measures_across_a_round_column_a_circle_or_a_curved_wall() {
    let mut s = Session::new();
    let col = id_of(&run(&mut s, "column", json!({"at": [5000, 3000], "type": "Round 300"})));
    let d = run(&mut s, "dimension", json!({"kind": "diameter", "refs": [{"id": col}], "at": [5600, 3600]}));
    assert_eq!(d["values"], json!([300.0]));
    let round = id_of(&run(&mut s, "detail_line", json!({"centre": [8000, 0], "radius": 1200})));
    let d = run(&mut s, "dimension", json!({"kind": "diameter", "refs": [{"id": round}], "at": [9500, 0]}));
    assert_eq!(d["values"], json!([2400.0]));
    // A point on a round column's edge finds it.
    let d = run(&mut s, "dimension", json!({"kind": "diameter", "refs": [[5150, 3000]], "at": [5600, 2400]}));
    assert_eq!(d["values"], json!([300.0]));
    let straight = id_of(&run(&mut s, "detail_line", json!({"start": [0, 0], "end": [1000, 0]})));
    let e = err(&mut s, "dimension", json!({"kind": "diameter", "refs": [{"id": straight}], "at": [0, 0]}));
    assert!(e.contains("diameter") && e.contains("round"), "{e}");
}

#[test]
fn an_arc_length_dimension_measures_along_a_curved_wall_or_an_arc_and_follows_it() {
    let mut s = Session::new();
    // A half circle 3000 mm round its centre line, 200 mm thick.
    let wall = run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "arc": 180}))["ids"][0].as_u64().unwrap_or_default();
    let pi = std::f64::consts::PI;
    let length = |s: &mut Session, refs: Value| {
        let d = run(s, "dimension", json!({"kind": "arc_length", "refs": refs, "at": [3000, -4000]}));
        (id_of(&d), d["values"][0].as_f64().unwrap_or_default())
    };
    let (centre, v) = length(&mut s, json!([{"id": wall}]));
    assert!((v - pi * 3000.0).abs() < 0.01, "{v}");
    let (_, inner) = length(&mut s, json!([{"id": wall, "face": "left"}]));
    let (_, outer) = length(&mut s, json!([{"id": wall, "face": "right"}]));
    let (lo, hi) = (inner.min(outer), inner.max(outer));
    assert!((lo - pi * 2900.0).abs() < 0.01 && (hi - pi * 3100.0).abs() < 0.01, "{inner} {outer}");
    // An arc drawn as a detail line, by a point on it.
    run(&mut s, "detail_line", json!({"start": [10000, 0], "end": [12000, 0], "arc": 90}));
    let d = run(&mut s, "dimension", json!({"kind": "arc_length", "refs": [[11000, -414.21]], "at": [11000, -2000]}));
    let r = 1000.0 * 2f64.sqrt();
    assert!((d["values"][0].as_f64().unwrap_or_default() - r * pi / 2.0).abs() < 0.01, "{d}");
    // The wall's end moves: the arc it measures grows.
    run(&mut s, "modify", json!({"id": wall, "end": [8000, 0]}));
    let v = values(&mut s, centre)[0].as_f64().unwrap_or_default();
    assert!((v - pi * 4000.0).abs() < 0.01, "{v}");
    // A straight wall has no arc, nor a circle ends.
    let straight = run(&mut s, "wall", json!({"points": [[0, 9000], [6000, 9000]]}))["ids"][0].as_u64().unwrap_or_default();
    let e = err(&mut s, "dimension", json!({"kind": "arc_length", "refs": [{"id": straight}], "at": [0, 0]}));
    assert!(e.contains("straight"), "{e}");
    let round = id_of(&run(&mut s, "detail_line", json!({"centre": [8000, 9000], "radius": 1200})));
    let e = err(&mut s, "dimension", json!({"kind": "arc_length", "refs": [{"id": round}], "at": [0, 0]}));
    assert!(e.contains("circle"), "{e}");
}

#[test]
fn diameter_and_arc_length_dimensions_draw_across_and_round() {
    let mut s = Session::new();
    // A circle round [0, 0], and a quarter arc round [10000, 0] from east to north, 2000 mm out.
    run(&mut s, "detail_line", json!({"centre": [0, 0], "radius": 1500}));
    run(&mut s, "detail_line", json!({"start": [12000, 0], "end": [10000, 2000], "arc": 90}));
    run(&mut s, "dimension", json!({"kind": "diameter", "refs": [[1500, 0]], "at": [3000, 0]}));
    run(&mut s, "dimension", json!({"kind": "arc_length", "refs": [[12000, 10]], "at": [13000, 0]}));
    let level = s.model.levels().first().map(|l| l.0).unwrap_or_default();
    let d = crate::render::plan(&s.model, level, 800, 600);
    let png = crate::render::png(&d).unwrap_or_default();
    let pm = tiny_skia::Pixmap::decode_png(&png).unwrap_or_else(|e| panic!("{e}"));
    let dark = |p: crate::geom::P2| {
        let (x, y) = d.view.px(p);
        pm.pixel(x as u32, y as u32).is_some_and(|c| c.red() < 200 && c.green() < 200 && c.blue() < 200)
    };
    assert!(dark(crate::geom::P2(-700.0, 0.0)), "the diameter's line crosses the circle");
    let on = |t: f64| crate::geom::P2(10000.0 + 3000.0 * t.to_radians().cos(), 3000.0 * t.to_radians().sin());
    assert!(dark(on(30.0)) && dark(on(60.0)), "the arc length's line, an arc 3000 mm round");
    assert!(!dark(on(120.0)), "only as far round as the arc sweeps");
}

#[test]
fn a_project_starts_with_dimension_types_and_a_dimension_takes_its_kind_s() {
    let mut s = Session::new();
    let w = house(&mut s);
    let listed = run(&mut s, "dimension_type", json!({}));
    assert!(listed["types"][crate::dims::DIAGONAL]["tick"] == "diagonal", "{listed}");
    assert!(listed["types"][crate::dims::ARROW]["tick"] == "arrow", "{listed}");
    let d = id_of(&run(&mut s, "dimension", json!({"refs": [{"id": w[1]}, {"id": w[3]}], "at": [4000, 3000]})));
    assert_eq!(crate::commands::element_json(&s.model, d)["type"], json!(crate::dims::DIAGONAL));
    let col = id_of(&run(&mut s, "column", json!({"at": [4000, 3000], "type": "Round 300"})));
    let r = id_of(&run(&mut s, "dimension", json!({"kind": "radial", "refs": [{"id": col}], "at": [4500, 3500]})));
    assert_eq!(crate::commands::element_json(&s.model, r)["type"], json!(crate::dims::ARROW));
    // Made like another, its own values; renamed, what is of it follows; deleted only once nothing is.
    let made = run(
        &mut s,
        "dimension_type",
        json!({"name": "Site", "like": crate::dims::DIAGONAL, "tick": "dot", "text_size": 3.5, "units": "m", "decimals": 3}),
    );
    assert_eq!(made["type"]["tick"], json!("dot"));
    run(&mut s, "modify", json!({"id": d, "type": "Site"}));
    assert_eq!(crate::commands::element_json(&s.model, d)["labels"], json!(["8.000"]));
    run(&mut s, "dimension_type", json!({"name": "Site", "rename": "Survey"}));
    assert_eq!(crate::commands::element_json(&s.model, d)["type"], json!("Survey"));
    let e = err(&mut s, "dimension_type", json!({"name": "Survey", "delete": true}));
    assert!(e.contains(&d.to_string()), "{e}");
    let e = err(&mut s, "dimension_type", json!({"name": "Survey", "tick_size": -1}));
    assert!(e.contains("tick size"), "{e}");
    let e = err(&mut s, "dimension", json!({"refs": [{"id": w[1]}, {"id": w[3]}], "at": [4000, 3000], "type": "Nope"}));
    assert!(e.contains("no dimension type"), "{e}");
}

#[test]
fn a_dimension_s_own_text_goes_round_its_value_or_replaces_it() {
    let mut s = Session::new();
    let w = house(&mut s);
    let grid = id_of(&run(&mut s, "grid", json!({"start": [4000, -2000], "end": [4000, 8000]})));
    let d = id_of(&run(
        &mut s,
        "dimension",
        json!({"refs": [{"id": w[3]}, {"id": grid}, {"id": w[1]}], "at": [0, -1000], "prefix": "(", "suffix": ")", "above": "TYP.", "below": "CL"}),
    ));
    let shown = crate::commands::element_json(&s.model, d);
    assert_eq!(shown["labels"], json!(["(4000)", "(4000)"]));
    assert_eq!((shown["above"].clone(), shown["below"].clone()), (json!("TYP."), json!("CL")));
    // Equal segments show EQ, Revit's Equality Display.
    run(&mut s, "modify", json!({"id": d, "equality": true}));
    assert_eq!(crate::commands::element_json(&s.model, d)["labels"], json!(["EQ", "EQ"]));
    run(&mut s, "modify", json!({"id": d, "equality": false, "replace": "VARIES"}));
    assert_eq!(crate::commands::element_json(&s.model, d)["labels"], json!(["VARIES", "VARIES"]));
    let e = err(&mut s, "modify", json!({"id": d, "prefix": "a\nb"}));
    assert!(e.contains("prefix"), "{e}");
    // A radius and a diameter say what they are, as their type's symbols.
    let col = id_of(&run(&mut s, "column", json!({"at": [2000, 3000], "type": "Round 300"})));
    let r = id_of(&run(&mut s, "dimension", json!({"kind": "radial", "refs": [{"id": col}], "at": [2500, 3500]})));
    assert_eq!(crate::commands::element_json(&s.model, r)["labels"], json!(["R 150"]));
    let r = id_of(&run(&mut s, "dimension", json!({"kind": "diameter", "refs": [{"id": col}], "at": [2500, 3500]})));
    assert_eq!(crate::commands::element_json(&s.model, r)["labels"], json!(["Ø 300"]));
    // Baseline and ordinate strings show what each witness line is from the first.
    run(&mut s, "dimension_type", json!({"name": "Ordinate", "string": "ordinate"}));
    run(&mut s, "modify", json!({"id": d, "replace": "", "prefix": "", "suffix": "", "type": "Ordinate"}));
    assert_eq!(crate::commands::element_json(&s.model, d)["labels"], json!(["0", "4000", "8000"]));
    run(&mut s, "dimension_type", json!({"name": "Ordinate", "string": "baseline"}));
    assert_eq!(crate::commands::element_json(&s.model, d)["labels"], json!(["4000", "8000"]));
}

#[test]
fn a_dimension_s_type_draws_its_ticks_its_lines_and_its_strings() {
    let mut s = Session::new();
    let a = id_of(&run(&mut s, "detail_line", json!({"start": [0, 0], "end": [0, 3000]})));
    let b = id_of(&run(&mut s, "detail_line", json!({"start": [4000, 0], "end": [4000, 3000]})));
    let c = id_of(&run(&mut s, "detail_line", json!({"start": [8000, 0], "end": [8000, 3000]})));
    run(&mut s, "dimension_type", json!({"name": "Baseline", "string": "baseline", "baseline_spacing": 10}));
    run(&mut s, "dimension", json!({"refs": [{"id": a}, {"id": b}, {"id": c}], "at": [2000, -2000], "type": "Baseline"}));
    let level = s.model.levels().first().map(|l| l.0).unwrap_or_default();
    let d = crate::render::plan(&s.model, level, 800, 600);
    let png = crate::render::png(&d).unwrap_or_default();
    let pm = tiny_skia::Pixmap::decode_png(&png).unwrap_or_else(|e| panic!("{e}"));
    let dark = |(x, y): (f32, f32)| pm.pixel(x as u32, y as u32).is_some_and(|c| c.red() < 200 && c.green() < 200 && c.blue() < 200);
    // The first line from a to b at `at`, the next, a to c, 10 mm of paper further out (down the page).
    let first = d.view.px(crate::geom::P2(6000.0, -2000.0));
    // A mm of paper, in model mm.
    let mm = f64::from(d.k) / d.view.scale * crate::sheet::DENSITY;
    let below = d.view.px(crate::geom::P2(6000.0, -2000.0 - 10.0 * mm));
    assert!(!dark(first), "a to b ends at b: nothing past it on the first line");
    assert!(dark(below), "a to c, one baseline spacing out");
}

#[test]
fn a_dimension_from_before_dimension_types_opens_in_its_kind_s_type() {
    let mut s = Session::new();
    let w = house(&mut s);
    let d = id_of(&run(&mut s, "dimension", json!({"refs": [{"id": w[1]}, {"id": w[3]}], "at": [4000, 3000]})));
    let col = id_of(&run(&mut s, "column", json!({"at": [4000, 3000], "type": "Round 300"})));
    let r = id_of(&run(&mut s, "dimension", json!({"kind": "radial", "refs": [{"id": col}], "at": [4500, 3500]})));
    let mut v: Value = serde_json::from_str(&s.model.to_json().unwrap_or_default()).unwrap_or_default();
    v["format"] = json!(9);
    if let Some(o) = v.as_object_mut() {
        o.remove("dimension_types");
    }
    for e in v["elements"].as_object_mut().into_iter().flat_map(|es| es.values_mut()) {
        if let Some(o) = e.as_object_mut().filter(|o| o.get("category") == Some(&json!("dimension"))) {
            o.remove("type");
        }
    }
    let (m, left_out) = Model::read(&v.to_string()).unwrap_or_else(|e| panic!("{e}"));
    assert!(left_out.is_empty(), "{left_out:?}");
    let ty = |id: Id| match m.elements.get(id) {
        Some(crate::model::Element::Dimension(d)) => d.type_name.clone(),
        _ => String::new(),
    };
    assert_eq!((ty(d), ty(r)), (crate::dims::DIAGONAL.to_string(), crate::dims::ARROW.to_string()));
}

#[test]
fn an_arc_length_dimension_measures_between_two_points_on_its_arc() {
    let mut s = Session::new();
    // A quarter circle 2000 round [0, 0], from east to north, and two lines ending on it at 30° and 60°.
    let arc = id_of(&run(&mut s, "detail_line", json!({"start": [2000, 0], "end": [0, 2000], "arc": 90})));
    let on = |deg: f64| [2000.0 * deg.to_radians().cos(), 2000.0 * deg.to_radians().sin()];
    let a = id_of(&run(&mut s, "detail_line", json!({"start": [0, 0], "end": on(30.0)})));
    let b = id_of(&run(&mut s, "detail_line", json!({"start": [0, 0], "end": on(60.0)})));
    let pi = std::f64::consts::PI;
    let refs = json!([{"id": arc}, {"id": a, "point": "end"}, {"id": b, "point": "end"}]);
    let d = run(&mut s, "dimension", json!({"kind": "arc_length", "refs": refs, "at": [2500, 2500]}));
    let v = d["values"][0].as_f64().unwrap_or_default();
    assert!((v - 2000.0 * pi / 6.0).abs() < 0.01, "{v}");
    // In either order.
    let refs = json!([{"id": arc}, {"id": b, "point": "end"}, {"id": a, "point": "end"}]);
    let v =
        run(&mut s, "dimension", json!({"kind": "arc_length", "refs": refs, "at": [2500, 2500]}))["values"][0].as_f64().unwrap_or_default();
    assert!((v - 2000.0 * pi / 6.0).abs() < 0.01, "{v}");
    // A point off the arc, and one point alone, are refused.
    let off = id_of(&run(&mut s, "detail_line", json!({"start": [0, 0], "end": [500, 500]})));
    let e = err(
        &mut s,
        "dimension",
        json!({"kind": "arc_length", "refs": [{"id": arc}, {"id": a, "point": "end"}, {"id": off, "point": "end"}], "at": [0, 0]}),
    );
    assert!(e.contains("on the arc"), "{e}");
    let e = err(&mut s, "dimension", json!({"kind": "arc_length", "refs": [{"id": arc}, {"id": a, "point": "end"}], "at": [0, 0]}));
    assert!(e.contains("two points"), "{e}");
    // The second line turns to 80°: the dimension follows.
    let id = id_of(&d);
    run(&mut s, "modify", json!({"id": b, "end": on(80.0)}));
    let v = values(&mut s, id)[0].as_f64().unwrap_or_default();
    assert!((v - 2000.0 * pi * 50.0 / 180.0).abs() < 0.01, "{v}");
}

#[test]
fn a_segment_of_a_dimension_has_text_of_its_own_over_the_dimension_s() {
    let mut s = Session::new();
    let w = house(&mut s);
    let g = id_of(&run(&mut s, "grid", json!({"start": [2000, -2000], "end": [2000, 8000]})));
    let d = id_of(&run(&mut s, "dimension", json!({"refs": [{"id": w[3]}, {"id": g}, {"id": w[1]}], "at": [0, -1000], "suffix": " mm"})));
    // The second segment, west to east: its own prefix, text under it, and the dimension's suffix still.
    run(&mut s, "dimension_text", json!({"id": d, "segment": 1, "prefix": "~", "below": "CLEAR"}));
    let shown = crate::commands::element_json(&s.model, d);
    assert_eq!(shown["labels"], json!(["2000 mm", "~6000 mm"]));
    let dim = match s.model.elements.get(d) {
        Some(crate::model::Element::Dimension(x)) => x.clone(),
        _ => panic!("no dimension"),
    };
    assert_eq!(s.model.dimension_extra(&dim, 1), ("".to_string(), "CLEAR".to_string()));
    // Its replacement replaces it alone; the whole dimension's, without `segment`.
    run(&mut s, "dimension_text", json!({"id": d, "segment": 0, "replace": "VARIES"}));
    assert_eq!(crate::commands::element_json(&s.model, d)["labels"], json!(["VARIES", "~6000 mm"]));
    run(&mut s, "dimension_text", json!({"id": d, "suffix": ""}));
    assert_eq!(crate::commands::element_json(&s.model, d)["labels"], json!(["VARIES", "~6000"]));
    // Cleared, a segment's text goes.
    run(&mut s, "dimension_text", json!({"id": d, "segment": 0, "replace": ""}));
    assert_eq!(crate::commands::element_json(&s.model, d)["texts"].as_array().map(Vec::len), Some(1));
    // Which segment a point is over: what a double click on a number edits.
    assert_eq!(
        (
            s.model.dimension_segment_at(&dim, crate::geom::P2(1000.0, -900.0)),
            s.model.dimension_segment_at(&dim, crate::geom::P2(5000.0, -900.0))
        ),
        (0, 1)
    );
    let e = err(&mut s, "dimension_text", json!({"id": d, "segment": 7, "prefix": "x"}));
    assert!(e.contains("segment"), "{e}");
    let e = err(&mut s, "dimension_text", json!({"id": w[0], "prefix": "x"}));
    assert!(e.contains("dimension"), "{e}");
}

#[test]
fn a_locked_segment_keeps_its_length_what_it_measures_moving_together() {
    let mut s = Session::new();
    let w = house(&mut s);
    let g = id_of(&run(&mut s, "grid", json!({"start": [2000, -2000], "end": [2000, 8000]})));
    // West wall to grid, 2000; grid to east wall, 6000. The first locked.
    let d = id_of(&run(&mut s, "dimension", json!({"refs": [{"id": w[3]}, {"id": g}, {"id": w[1]}], "at": [0, -1000]})));
    run(&mut s, "dimension_constraint", json!({"id": d, "locked": [0]}));
    // The west wall moves west: the grid goes with it, 2000 off; the east wall stays.
    run(&mut s, "move", json!({"ids": [w[3]], "by": [-500, 0]}));
    assert_eq!(values(&mut s, d), json!([2000.0, 6500.0]));
    // The grid moves: the west wall follows it.
    run(&mut s, "move", json!({"ids": [g], "by": [300, 0]}));
    assert_eq!(values(&mut s, d), json!([2000.0, 6200.0]));
    // Unlocked, it doesn't.
    run(&mut s, "dimension_constraint", json!({"id": d, "locked": []}));
    run(&mut s, "move", json!({"ids": [g], "by": [300, 0]}));
    assert_eq!(values(&mut s, d), json!([2300.0, 5900.0]));
    // Undo takes a move and what followed it back as one step.
    run(&mut s, "undo", json!({}));
    assert_eq!(values(&mut s, d), json!([2000.0, 6200.0]));
}

#[test]
fn an_equal_dimension_spreads_its_inner_references_evenly_and_keeps_them_so() {
    let mut s = Session::new();
    let w = house(&mut s);
    let a = id_of(&run(&mut s, "grid", json!({"start": [1000, -2000], "end": [1000, 8000]})));
    let b = id_of(&run(&mut s, "grid", json!({"start": [5000, -2000], "end": [5000, 8000]})));
    let d = id_of(&run(&mut s, "dimension", json!({"refs": [{"id": w[3]}, {"id": a}, {"id": b}, {"id": w[1]}], "at": [0, -1000]})));
    // EQ: the grids go to the thirds of the walls' 8000.
    let r = run(&mut s, "dimension_constraint", json!({"id": d, "equal": true}));
    assert!(r.is_object());
    let v = values(&mut s, d);
    let near = |v: &Value, x: f64| v.as_array().is_some_and(|a| a.iter().all(|y| (y.as_f64().unwrap_or_default() - x).abs() < 0.01));
    assert!(near(&v, 8000.0 / 3.0), "{v}");
    // The east wall moves: they stay a third apart.
    run(&mut s, "move", json!({"ids": [w[1]], "by": [900, 0]}));
    let v = values(&mut s, d);
    assert!(near(&v, 8900.0 / 3.0), "{v}");
    // A grid dragged is put back where equal says.
    run(&mut s, "move", json!({"ids": [a], "by": [400, 0]}));
    assert!(near(&values(&mut s, d), 8900.0 / 3.0));
    // Locked, the whole string moves with a wall that moves.
    run(&mut s, "dimension_constraint", json!({"id": d, "locked": [0, 1, 2]}));
    run(&mut s, "move", json!({"ids": [w[1]], "by": [300, 0]}));
    assert!(near(&values(&mut s, d), 8900.0 / 3.0));
    // Locked segments of lengths that differ can't be made equal: refused, and nothing changes.
    let two = id_of(&run(&mut s, "dimension", json!({"refs": [{"id": w[3]}, {"id": a}, {"id": w[1]}], "at": [0, 7000]})));
    run(&mut s, "dimension_constraint", json!({"id": two, "locked": [0, 1]}));
    let before = values(&mut s, two);
    let e = err(&mut s, "dimension_constraint", json!({"id": two, "equal": true}));
    assert!(e.contains("constraint"), "{e}");
    assert_eq!(values(&mut s, two), before);
    // Only aligned and linear dimensions of more than one segment are equal.
    let one = id_of(&run(&mut s, "dimension", json!({"refs": [{"id": w[3]}, {"id": w[1]}], "at": [0, 7000]})));
    let e = err(&mut s, "dimension_constraint", json!({"id": one, "equal": true}));
    assert!(e.contains("segments"), "{e}");
}
