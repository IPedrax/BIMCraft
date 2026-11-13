//! Grid types on the drawing: a grid draws in its type's pattern, pen and colour, its pattern scales with the paper, its
//! bubble is lettered in its type's text, and a view's overrides still win.

use bimcraft_core::engine::Session;
use bimcraft_core::grids::GridType;
use bimcraft_core::library::Kind;
use bimcraft_core::model::{Element, Model};
use bimcraft_core::pen::Pen;
use bimcraft_core::render::{self, Drawing, Prim, Rgb, View};
use serde_json::{Value, json};

#[allow(clippy::unwrap_used)] // Test fixture creation must succeed.
fn id(reply: Value) -> u64 {
    reply["id"].as_u64().unwrap()
}

fn plan(s: &Session, scale: f64) -> Drawing {
    render::draw(&s.model, s.model.levels()[0].0, View { angle: 0.0, scale, cx: 5000.0, cy: 0.0, width: 800, height: 600 })
}

/// The grid line `g` draws: its colour, its dash and its width. The longest two-point line of its own is the line; its
/// lettering and the bubbles' rings are shorter or have more corners.
#[allow(clippy::expect_used)] // A test draws a grid line; none is a failure it reports below.
fn line(d: &Drawing, g: u64) -> (Rgb, Option<Vec<f32>>, f32) {
    let found = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter_map(|(p, o)| match p {
            Prim::Line { pts, color, dash, width, closed: false, .. } if *o == Some(g) && pts.len() == 2 => {
                Some((*color, dash.clone(), *width, (pts[1].0 - pts[0].0).hypot(pts[1].1 - pts[0].1)))
            }
            _ => None,
        })
        .max_by(|a, b| a.3.total_cmp(&b.3));
    let (color, dash, width, _) = found.expect("a grid line");
    (color, dash, width)
}

/// The radii of the bubbles `g` draws, px.
fn bubbles(d: &Drawing, g: u64) -> Vec<f32> {
    d.prims
        .iter()
        .zip(&d.owners)
        .filter_map(|(p, o)| match p {
            Prim::Line { pts, closed: true, .. } if *o == Some(g) && pts.len() == 32 => {
                let (x, y) = (pts.iter().map(|p| p.0).sum::<f32>() / 32.0, pts.iter().map(|p| p.1).sum::<f32>() / 32.0);
                Some(((pts[0].0 - x).hypot(pts[0].1 - y) + (pts[16].0 - x).hypot(pts[16].1 - y)) / 2.0)
            }
            _ => None,
        })
        .collect()
}

#[allow(clippy::unwrap_used)] // Test fixture creation must succeed.
fn grid(s: &mut Session) -> u64 {
    id(s.execute("grid", &json!({"start": [0, 0], "end": [10000, 0], "name": "1"})).unwrap())
}

#[test]
fn a_grid_draws_in_its_types_pattern_pen_and_colour() {
    let mut s = Session::new();
    let g = grid(&mut s);
    let (color, dash, _) = line(&plan(&s, 0.05), g);
    assert_eq!(color, Rgb(60, 100, 150), "the ISO type's colour");
    assert!(dash.is_some(), "ISO 128's chain isn't solid");
    s.execute("modify", &json!({"id": g, "type": "Grid - Continuous"})).unwrap();
    let (_, dash, _) = line(&plan(&s, 0.05), g);
    assert_eq!(dash, None, "a continuous grid is solid");
}

#[test]
fn the_starter_types_draw_differently_and_all_are_listed() {
    let mut s = Session::new();
    let g = grid(&mut s);
    let listed = s.execute("grid_type", &json!({})).unwrap();
    let names: Vec<String> = listed["types"].as_object().unwrap().keys().cloned().collect();
    for name in ["Grid - ISO 128", "Grid - DIN 1356", "Grid - ASME Y14.2", "Grid - Architectural", "Grid - Continuous", "Grid - Compact"] {
        assert!(names.iter().any(|n| n == name), "{name}: {names:?}");
    }
    let mut pictures: Vec<(String, Vec<u8>)> = Vec::new();
    for name in &names {
        s.execute("modify", &json!({"id": g, "type": name})).unwrap();
        let d = plan(&s, 0.05);
        let (color, dash, _) = line(&d, g);
        assert_eq!(color, Rgb(60, 100, 150), "{name}");
        assert!(dash.is_some() || name == "Grid - Continuous", "{name}: {dash:?}");
        pictures.push((name.clone(), render::png(&d).unwrap()));
    }
    for i in 0..pictures.len() {
        for j in 0..i {
            assert!(pictures[i].1 != pictures[j].1, "{} and {} draw alike", pictures[i].0, pictures[j].0);
        }
    }
}

#[test]
fn a_grids_pattern_is_mm_on_paper_and_scales_with_the_views_scale() {
    let mut s = Session::new();
    let g = grid(&mut s);
    let dash_at = |s: &mut Session, n: f64| {
        s.execute("scale", &json!({"view": s.model.levels()[0].0, "scale": n})).unwrap();
        line(&plan(s, 0.02), g).1.expect("dashed")[0]
    };
    let at_50 = dash_at(&mut s, 50.0);
    let at_200 = dash_at(&mut s, 200.0);
    assert!((at_200 / at_50 - 4.0).abs() < 1e-3, "a dash four times as long on paper at 1 : 200: {at_50} and {at_200}");
    // 12.7 mm of dash, at 5 px to a mm of paper and 200 mm of the model to one.
    assert!((at_200 - 12.7 * 200.0 * 0.02).abs() < 1e-3, "{at_200}");
}

#[test]
fn a_types_pen_colour_and_text_change_the_line_and_the_bubble() {
    let mut s = Session::new();
    let g = grid(&mut s);
    let (_, _, before) = line(&plan(&s, 0.05), g);
    let radius = bubbles(&plan(&s, 0.05), g);
    assert_eq!(radius.len(), 2, "a bubble at each end");
    let radius = bubbles(&plan(&s, 0.05), g);
    assert_eq!(radius.len(), 2, "a bubble at each end");
    s.execute("grid_type", &json!({"name": "Mine", "like": "Grid - ISO 128", "color": "#c80000", "line_pen": 9})).unwrap();
    s.execute("modify", &json!({"id": g, "type": "Mine"})).unwrap();
    let d = plan(&s, 0.05);
    let (color, _, after) = line(&d, g);
    assert_eq!(color, Rgb(200, 0, 0), "its colour, in its line");
    assert!((after - before).abs() > 1e-3, "a heavier pen draws wider");
    assert_eq!(bubbles(&d, g).len(), 2, "its bubbles keep its colour too");
    let small = bubbles(&d, g)[0];
    s.execute("grid_type", &json!({"name": "Mine", "text_size": 7.0})).unwrap();
    let big = bubbles(&plan(&s, 0.05), g)[0];
    assert!(big > small * 1.5, "a larger name needs a larger bubble: {small} then {big}");
}

#[test]
fn a_views_overrides_are_stronger_than_the_type_and_can_hide_it() {
    let mut s = Session::new();
    let g = grid(&mut s);
    let level = s.model.levels()[0].0;
    let type_dash = line(&plan(&s, 0.05), g).1;
    s.execute("graphics", &json!({"view": level, "categories": {"grid": {"projection": {"dash": "dotted", "color": [0, 200, 0]}}}}))
        .unwrap();
    let (color, dash, _) = line(&plan(&s, 0.05), g);
    assert_eq!(color, Rgb(0, 200, 0), "the view's colour wins");
    assert!(dash != type_dash, "the view's dash wins");
    s.execute("graphics", &json!({"view": level, "hide": ["grids"]})).unwrap();
    let d = plan(&s, 0.05);
    assert!(d.owners.iter().all(|o| *o != Some(g)), "a hidden category's grid draws nothing");
}

#[test]
fn a_section_draws_a_grid_in_its_type_too() {
    let mut s = Session::new();
    let g = grid(&mut s);
    let cut = id(s.execute("section", &json!({"start": [5000, -2000], "end": [5000, 2000], "name": "S"})).unwrap());
    let d = render::section(&s.model, cut, render::section_fit(&s.model, cut, 800, 600));
    let (color, dash, len) = line(&d, g);
    assert_eq!(color, Rgb(60, 100, 150));
    assert!(dash.is_some(), "the same chain as in the plan");
    assert!(len > 0.0);
}

#[allow(clippy::panic)] // A test reads a grid; another element is a broken fixture.
fn grid_type_name(m: &Model, id: u64) -> String {
    match m.elements.get(id) {
        Some(Element::Grid(g)) => g.type_name.clone(),
        e => panic!("{e:?}"),
    }
}

#[test]
fn grid_types_are_made_changed_renamed_and_deleted_through_the_command() {
    let mut s = Session::new();
    let listed = s.execute("grid_type", &json!({})).unwrap();
    for name in ["Grid - ISO 128", "Grid - DIN 1356", "Grid - ASME Y14.2", "Grid - Architectural", "Grid - Continuous", "Grid - Compact"] {
        assert!(listed["types"][name].is_object(), "{name}: {listed}");
    }
    assert!(listed["fonts"].as_array().is_some_and(|f| !f.is_empty()));
    assert!(listed["patterns"]["Grid - ISO Dash Dot"].is_array(), "the project's patterns come along");
    assert!(s.history().is_empty(), "listing changes nothing");

    let g = grid(&mut s);
    assert_eq!(grid_type_name(&s.model, g), "Grid - ISO 128", "a new grid takes the default type");
    assert_eq!(s.model.default_type("grid"), Some("Grid - ISO 128"));
    assert!(s.model.type_names("grid").contains(&"Grid - DIN 1356"));
    let e = s.execute("grid", &json!({"start": [0, 1000], "end": [9000, 1000], "type": "Nope"})).unwrap_err();
    assert!(e.contains("there's no grid type `Nope` (there are:"), "{e}");

    s.execute("line_pattern", &json!({"name": "Mine", "segments": [{"dash": 6}, {"space": 1}, "dot", {"space": 1}]})).unwrap();
    s.execute(
        "grid_type",
        &json!({"name": "Mine", "like": "Grid - ISO 128", "line_pattern": "Mine", "line_pen": 6, "color": "#112233", "text_size": 3.0}),
    )
    .unwrap();
    assert_eq!(s.model.grid_types["Mine"].line_pattern.as_deref(), Some("Mine"));
    assert_eq!(s.model.grid_types["Mine"].color, [0x11, 0x22, 0x33]);
    assert!(s.model.library.id(&Kind::GridType, "Mine").is_some(), "a library entry");
    s.execute("modify", &json!({"ids": [g], "type": "Mine"})).unwrap();
    assert_eq!(grid_type_name(&s.model, g), "Mine");
    // The pattern it draws in is a library entry as any other: renamed, the type follows; in use, it stays.
    s.execute("line_pattern", &json!({"name": "Mine", "rename": "Ours Pat"})).unwrap();
    assert_eq!(s.model.grid_types["Mine"].line_pattern.as_deref(), Some("Ours Pat"));
    let e = s.execute("line_pattern", &json!({"name": "Ours Pat", "delete": true})).unwrap_err();
    assert!(e.contains("grid type `Mine`"), "{e}");

    let e = s.execute("grid_type", &json!({"name": "Mine", "rename": "Ours"})).unwrap();
    assert_eq!(e["name"], "Ours");
    assert_eq!(grid_type_name(&s.model, g), "Ours", "its grids follow the rename");
    assert!(s.model.library.id(&Kind::GridType, "Ours").is_some() && s.model.library.id(&Kind::GridType, "Mine").is_none());

    let e = s.execute("grid_type", &json!({"name": "Ours", "delete": true})).unwrap_err();
    assert!(e.contains(&format!("grid {g} is of `Ours`")), "{e}");
    let e = s.execute("modify", &json!({"ids": [g], "type": "Nope"})).unwrap_err();
    assert!(e.contains(&format!("grid {g}")) && e.contains("there's no grid type `Nope`"), "{e}");
    assert_eq!(bimcraft_core::element_json(&s.model, g)["type"], json!("Ours"));

    // A file keeps the type and what names it.
    let (back, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.grid_types, s.model.grid_types);
    assert_eq!(grid_type_name(&back, g), "Ours");

    // A type no grid is of goes; Purge Unused leaves grid types be.
    s.execute("grid_type", &json!({"name": "Grid - Compact", "delete": true})).unwrap();
    assert!(!s.model.grid_types.contains_key("Grid - Compact"));
    s.execute("purge", &json!({"kinds": ["type"]})).unwrap();
    assert!(s.model.grid_types.contains_key("Ours"), "Purge Unused keeps grid types, as dimension types");
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn a_grid_type_with_a_missing_pattern_or_bad_lettering_is_refused() {
    let mut s = Session::new();
    let iso = s.model.grid_types["Grid - ISO 128"].clone();
    for (type_now, says) in [
        (GridType { line_pattern: Some("Nope".into()), ..iso.clone() }, "there's no line pattern `Nope`"),
        (GridType { line_pen: Pen(17), ..iso.clone() }, "0 (a hairline) to 16"),
        (GridType { font: "  ".into(), ..iso.clone() }, "a font is named"),
        (GridType { text_size: 0.0, ..iso.clone() }, "size"),
        (GridType { width_factor: 0.0, ..iso.clone() }, "width factor"),
    ] {
        s.model_mut().grid_types.insert("Bad".into(), type_now);
        let e = s.model.check().unwrap_err();
        assert!(e.contains("grid type `Bad`") && e.contains(says), "{says}: {e}");
        s.model_mut().grid_types.remove("Bad");
    }
    // A grid whose type the project hasn't says so, naming the grid and the types there are.
    let g = grid(&mut s);
    if let Some(Element::Grid(x)) = s.model_mut().elements.get_mut(g) {
        x.type_name = "Nope".into();
    }
    let e = s.model.check().unwrap_err();
    assert!(e.contains(&format!("grid {g}")) && e.contains("there's no grid type `Nope` (there are:"), "{e}");
}

#[test]
fn a_file_of_format_nineteen_opens_with_every_grid_in_the_default_type() {
    let mut s = Session::new();
    let g = grid(&mut s);
    let mut v: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    v["format"] = json!(19);
    assert!(v.as_object_mut().unwrap().remove("grid_types").is_some(), "a file of 19 holds no grid types");
    for e in v["elements"].as_object_mut().unwrap().values_mut() {
        if e["category"] == json!("grid") {
            e.as_object_mut().unwrap().remove("type");
        }
    }
    let (m, left_out) = Model::read(&v.to_string()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert!(!m.grid_types.is_empty(), "the starter set is put back");
    assert_eq!(grid_type_name(&m, g), "Grid - ISO 128");
    assert_eq!(m.check(), Ok(()));
}

#[test]
fn insert_from_file_brings_a_grids_type_and_its_pattern() {
    let mut src = Session::new();
    src.execute("line_pattern", &json!({"name": "Zig", "segments": [{"dash": 4}, {"space": 1}, "dot", {"space": 1}]})).unwrap();
    src.execute("grid_type", &json!({"name": "Zig Type", "like": "Grid - ISO 128", "line_pattern": "Zig"})).unwrap();
    let g =
        src.execute("grid", &json!({"start": [0, 0], "end": [8000, 0], "name": "Z", "type": "Zig Type"})).unwrap()["id"].as_u64().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.bimcraft");
    src.execute("save", &json!({"path": path})).unwrap();

    let mut dst = Session::new();
    let inserted = dst.execute("insert", &json!({"path": path})).unwrap();
    assert!(dst.model.grid_types.contains_key("Zig Type"), "the type came along: {}", inserted["library"]);
    assert!(dst.model.line_patterns.contains_key("Zig"), "and its pattern");
    let here = inserted["ids"][g.to_string()].as_u64().unwrap();
    assert_eq!(grid_type_name(&dst.model, here), "Zig Type");
    assert_eq!(dst.model.check(), Ok(()));
}

#[test]
fn hostile_parameters_never_panic_with_the_grid_type_command() {
    let mut s = Session::new();
    for p in [
        json!({"name": 7}),
        json!({"name": "X", "like": 7}),
        json!({"name": "X", "line_pen": 1e300}),
        json!({"name": "X", "text_size": -1}),
        json!({"name": "X", "color": "nope"}),
        json!({"name": "X", "line_pattern": 7}),
        json!({"name": "X", "rename": ""}),
        json!({"delete": true}),
    ] {
        // Whatever it answers, it answers and leaves a model that still checks.
        let _ = s.execute("grid_type", &p);
        assert_eq!(s.model.check(), Ok(()), "{p}");
    }
}

#[test]
fn delete_type_deletes_a_free_grid_type_and_refuses_a_used_or_the_last_one() {
    let mut s = Session::new();
    // A type no grid uses goes, as Delete on it in the Project Browser does.
    s.execute("grid_type", &json!({"name": "Spare Axis", "like": "Grid - ISO 128"})).unwrap();
    s.execute("delete_type", &json!({"category": "grid", "type": "Spare Axis"})).unwrap();
    assert!(!s.model.grid_types.contains_key("Spare Axis"));
    assert!(s.model.library.id(&Kind::GridType, "Spare Axis").is_none());

    // A type one is of is refused, saying how many use it.
    let g = grid(&mut s);
    let e = s.execute("delete_type", &json!({"category": "grid", "type": "Grid - ISO 128"})).unwrap_err();
    assert!(e.contains("grid type `Grid - ISO 128` is used") && e.contains("grid"), "{e}");
    assert_eq!(grid_type_name(&s.model, g), "Grid - ISO 128");

    // The last type stays: a grid needs one.
    s.execute("delete", &json!({"ids": [g]})).unwrap();
    for name in ["Grid - DIN 1356", "Grid - ASME Y14.2", "Grid - Architectural", "Grid - Continuous", "Grid - Compact"] {
        s.execute("delete_type", &json!({"category": "grid", "type": name})).unwrap();
    }
    let e = s.execute("grid_type", &json!({"name": "Grid - ISO 128", "delete": true})).unwrap_err();
    assert!(e.contains("last grid type"), "{e}");
    let e = s.execute("delete_type", &json!({"category": "grid", "type": "Grid - ISO 128"})).unwrap_err();
    assert!(e.contains("last grid type"), "{e}");
}

#[test]
fn a_grid_type_is_a_library_entry_like_any_other() {
    let kind = Kind::GridType;
    let json = serde_json::to_string(&kind).unwrap();
    assert_eq!(json, "\"grid-type\"");
    assert_eq!(kind.label(), "grid type");
    assert_eq!(serde_json::from_str::<Kind>(&json).unwrap(), kind);
}
