//! Curtain walls, through the commands every front end drives.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{FORMAT, Model};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

fn session() -> Session {
    Session::new()
}

#[test]
fn a_new_project_has_the_generic_panels_and_a_rectangular_mullion() {
    let s = session();
    assert_eq!(s.model.type_names("curtainpanel"), ["Glazed", "Spandrel"]);
    assert_eq!(s.model.type_names("curtain panel"), ["Glazed", "Spandrel"]);
    assert_eq!(s.model.type_names("mullion"), ["50 x 150"]);
    assert_eq!(s.model.default_type("curtainpanel"), Some("Glazed"));
    assert_eq!(s.model.default_type("mullion"), Some("50 x 150"));
    let m = &s.model.types;
    assert_eq!(m.curtain_panel["Glazed"].material, "Glass");
    assert_eq!(m.mullion["50 x 150"].width(), 50.0);
    assert_eq!(m.mullion["50 x 150"].depth(), 150.0);
}

#[test]
fn panel_types_take_thickness_material_and_offset() {
    let mut s = session();
    let out =
        run(&mut s, "type", json!({"category": "curtain panel", "name": "Tinted", "thickness": 30, "material": "Glass", "offset": 25}));
    assert_eq!(out["type"], json!({"thickness": 30.0, "material": "Glass", "offset": 25.0}));
    // What isn't given stays as it was.
    let out = run(&mut s, "type", json!({"category": "curtainpanel", "name": "Tinted", "offset": -10}));
    assert_eq!(out["type"], json!({"thickness": 30.0, "material": "Glass", "offset": -10.0}));
    assert!(fails(&mut s, "type", json!({"category": "curtainpanel", "name": "New"})).contains("thickness"));
}

#[test]
fn mullion_types_take_width_depth_offsets_and_material() {
    let mut s = session();
    let out = run(
        &mut s,
        "type",
        json!({"category": "mullion", "name": "Deep", "width": 60, "depth": 200, "offset_x": 5, "offset_y": -20, "material": "Metal"}),
    );
    assert_eq!(
        out["type"],
        json!({"profile": {"kind": "rectangular", "width": 60.0, "depth": 200.0}, "offset_x": 5.0, "offset_y": -20.0, "material": "Metal"})
    );
    let out = run(&mut s, "type", json!({"category": "mullion", "name": "Deep", "width": 70}));
    assert_eq!(out["type"]["profile"]["width"], 70.0);
    assert_eq!(out["type"]["profile"]["depth"], 200.0);
    assert_eq!(out["type"]["offset_y"], -20.0);
    assert!(fails(&mut s, "type", json!({"category": "mullion", "name": "New", "width": 60})).contains("depth"));
}

#[test]
fn bad_panel_and_mullion_types_are_refused_saying_what_to_fix() {
    let mut s = session();
    for (p, says) in [
        (json!({"category": "curtainpanel", "name": "X", "thickness": 0}), "thickness"),
        (json!({"category": "curtainpanel", "name": "X", "thickness": -5}), "thickness"),
        (json!({"category": "curtainpanel", "name": "X", "thickness": 1e300}), "thickness"),
        (json!({"category": "curtainpanel", "name": "X", "thickness": 20, "offset": 1e300}), "offset"),
        (json!({"category": "curtainpanel", "name": "X", "thickness": 20, "material": "Nothing"}), "material"),
        (json!({"category": "mullion", "name": "X", "width": 0, "depth": 100}), "width"),
        (json!({"category": "mullion", "name": "X", "width": 50, "depth": -1}), "depth"),
        (json!({"category": "mullion", "name": "X", "width": 50, "depth": 100, "offset_x": 1e300}), "offset"),
        (json!({"category": "mullion", "name": "X", "width": 50, "depth": 100, "material": "Nothing"}), "material"),
    ] {
        let e = fails(&mut s, "type", p.clone());
        assert!(e.contains(says), "{p}: {e}");
    }
}

#[test]
fn panel_and_mullion_types_are_library_entries() {
    let mut s = session();
    run(&mut s, "type", json!({"category": "mullion", "name": "Deep", "width": 60, "depth": 200}));
    let kind = crate::library::Kind::Type("mullion".into());
    assert!(s.model.library.guid(&kind, "Deep").is_some(), "a new type has a GUID");
    run(&mut s, "rename_type", json!({"category": "mullion", "type": "Deep", "name": "Deeper"}));
    assert_eq!(s.model.type_names("mullion"), ["50 x 150", "Deeper"]);
    run(&mut s, "delete_type", json!({"category": "mullion", "type": "Deeper"}));
    assert!(
        fails(&mut s, "delete_type", json!({"category": "mullion", "type": "50 x 150"})).contains("used"),
        "by the starter curtain wall"
    );
    run(&mut s, "delete_type", json!({"category": "curtainpanel", "type": "Spandrel"}));
    assert_eq!(s.model.type_names("curtainpanel"), ["Glazed"]);
}

#[test]
fn a_panel_or_mullion_type_counts_as_a_use_of_its_material() {
    let mut s = session();
    run(&mut s, "material", json!({"name": "Bronze", "color": [120, 90, 60]}));
    run(&mut s, "type", json!({"category": "mullion", "name": "Bronze", "width": 60, "depth": 200, "material": "Bronze"}));
    let uses = s.model.library_uses();
    assert_eq!(uses.get(&(crate::library::Kind::Material, "Bronze".to_string())), Some(&1));
}

#[test]
fn they_go_through_a_file_and_an_older_file_gets_the_starter_set() {
    let mut s = session();
    run(&mut s, "type", json!({"category": "mullion", "name": "Deep", "width": 60, "depth": 200, "offset_x": 5}));
    let text = s.model.to_json().unwrap();
    let (back, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.types.mullion["Deep"].offset_x, 5.0);
    assert_eq!(back.format, FORMAT);

    let mut old: Value = serde_json::from_str(&text).unwrap();
    old["format"] = json!(16);
    let types = old["types"].as_object_mut().unwrap();
    types.remove("curtain_panel");
    types.remove("mullion");
    let (back, _) = Model::read(&old.to_string()).unwrap();
    assert_eq!(back.format, FORMAT);
    assert_eq!(back.type_names("curtainpanel"), ["Glazed", "Spandrel"]);
    assert_eq!(back.type_names("mullion"), ["50 x 150"]);
    assert!(back.library.guid(&crate::library::Kind::Type("mullion".into()), "50 x 150").is_some());
}

// ---------- the layout ----------

use crate::curtain::{self, Fit, Justify, Layout, PartKind};
use crate::model::{Element, Location, Wall};

/// A curtain wall type of the given grid, a panel in every cell and the starter mullion on every edge unless `extra` says.
fn grid(s: &mut Session, name: &str, columns: &[f64], rows: &[f64], extra: Value) {
    let mut curtain = json!({"columns": {"segments": columns}, "rows": {"segments": rows}});
    if let Value::Object(more) = extra {
        for (k, v) in more {
            curtain[k] = v;
        }
    }
    run(s, "type", json!({"category": "wall", "name": name, "curtain": curtain}));
}

/// A curtain wall `length` long along x, `height` tall.
fn wall(s: &mut Session, name: &str, length: f64, height: f64) -> u64 {
    run(s, "wall", json!({"points": [[0, 0], [length, 0]], "type": name, "height": height}))["ids"][0].as_u64().unwrap()
}

fn wall_of(s: &Session, id: u64) -> &Wall {
    let Some(Element::Wall(w)) = s.model.elements.get(id) else { panic!("no wall {id}") };
    w
}

fn layout_of(s: &Session, id: u64) -> Layout {
    curtain::layout(&s.model, wall_of(s, id))
}

fn built(s: &mut Session, name: &str, length: f64, height: f64) -> Layout {
    let id = wall(s, name, length, height);
    layout_of(s, id)
}

/// Each panel's extent along the wall, in order.
fn panels_along(l: &Layout) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> =
        l.parts.iter().filter(|p| l.shapes[p.shape as usize].kind == PartKind::Panel).map(|p| l.along(p)).collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.dedup();
    out
}

fn count(l: &Layout, kind: PartKind) -> usize {
    l.parts.iter().filter(|p| l.shapes[p.shape as usize].kind == kind).count()
}

fn lines(d: &curtain::Direction, length: f64) -> Vec<f64> {
    let spans = curtain::divide(d, length);
    spans.iter().skip(1).map(|s| s.from).collect()
}

#[test]
fn a_pattern_repeats_from_where_its_justified() {
    let at = |justify| curtain::Direction { segments: vec![900.0, 650.0], fit: Fit::Flexible, justify };
    assert_eq!(lines(&at(Justify::Start), 3000.0), [900.0, 1550.0, 2450.0]);
    assert_eq!(lines(&at(Justify::End), 3000.0), [800.0, 1450.0, 2350.0]);
    // Whole modules in the middle, what's over shared by the two ends.
    assert_eq!(lines(&at(Justify::Centre), 3000.0), [75.0, 725.0, 1625.0, 2275.0]);
    // A line through the middle.
    assert!(lines(&at(Justify::CentreLine), 3000.0).contains(&1500.0));
    let spans = curtain::divide(&at(Justify::Start), 3000.0);
    assert_eq!(spans.iter().map(|s| s.seg).collect::<Vec<_>>(), [0, 1, 0, 1]);
    assert_eq!(spans.iter().map(|s| s.whole).collect::<Vec<_>>(), [true, true, true, false]);
    // A wall of whole segments has no leftover, nor a sliver of rounding.
    let spans = curtain::divide(&at(Justify::Start), 3100.0);
    assert_eq!(spans.len(), 4);
    assert!(spans.iter().all(|s| s.whole));
}

#[test]
fn a_curtain_walls_thickness_comes_from_what_it_is_made_of() {
    let mut s = session();
    grid(&mut s, "CW", &[1000.0], &[1500.0], json!({}));
    assert_eq!(s.model.types.wall["CW"].thickness, 150.0, "the mullion is the deepest");
    // Moved out, the panel reaches past the mullion on one side only.
    run(&mut s, "type", json!({"category": "curtainpanel", "name": "Glazed", "offset": 100}));
    assert_eq!(s.model.types.wall["CW"].thickness, 75.0 + 112.0);
    // Without mullions, the panel alone.
    grid(&mut s, "Bare", &[1000.0], &[1500.0], json!({"mullion": null}));
    assert_eq!(s.model.types.wall["Bare"].thickness, 24.0);
    assert!(fails(&mut s, "type", json!({"category": "wall", "name": "CW", "thickness": 300})).contains("worked out"));
}

#[test]
fn a_grid_of_cells_has_a_panel_in_each_and_a_mullion_on_each_edge() {
    let mut s = session();
    grid(&mut s, "CW", &[1000.0], &[1500.0], json!({}));
    let l = built(&mut s, "CW", 3000.0, 3000.0);
    assert_eq!((l.columns.len(), l.rows.len()), (3, 2));
    assert_eq!(count(&l, PartKind::Panel), 6);
    assert_eq!(count(&l, PartKind::Mullion), 4 * 2 + 3 * 3);
    assert!(l.malformed.is_empty());
}

#[test]
fn mullions_cut_panels_and_without_one_panels_run_gapless() {
    let mut s = session();
    grid(&mut s, "CW", &[1000.0], &[3000.0], json!({}));
    let l = built(&mut s, "CW", 2000.0, 3000.0);
    // The border stands inside the wall, 50 wide; the mullion between is 25 either way of its line.
    assert_eq!(panels_along(&l), [(50.0, 975.0), (1025.0, 1950.0)]);

    grid(&mut s, "Bare", &[1000.0], &[3000.0], json!({"mullion": null}));
    let l = built(&mut s, "Bare", 2000.0, 3000.0);
    assert_eq!(panels_along(&l), [(0.0, 1000.0), (1000.0, 2000.0)]);
    assert_eq!(count(&l, PartKind::Mullion), 0);
}

#[test]
fn a_panel_clear_of_a_mullions_depth_isnt_cut() {
    let mut s = session();
    run(&mut s, "type", json!({"category": "curtainpanel", "name": "Glazed", "offset": 200}));
    grid(&mut s, "CW", &[1000.0], &[3000.0], json!({}));
    let l = built(&mut s, "CW", 2000.0, 3000.0);
    assert_eq!(panels_along(&l), [(0.0, 1000.0), (1000.0, 2000.0)], "in front of the mullions: structural glazing");
}

#[test]
fn each_cell_and_edge_of_the_pattern_has_its_own_type_or_none() {
    let mut s = session();
    let pick = json!({"type": "50 x 150"});
    grid(
        &mut s,
        "CW",
        &[1000.0, 500.0],
        &[1000.0, 2000.0],
        json!({
            "panels": [["Spandrel", null], ["Glazed", "Glazed"]],
            "vertical": [[null, pick], [pick, null]],
            "horizontal": [[pick, null], [null, pick]],
            "borders": {"left": null, "right": null, "bottom": null, "top": null},
        }),
    );
    let l = built(&mut s, "CW", 1500.0, 3000.0);
    assert_eq!(count(&l, PartKind::Panel), 3, "one cell left empty");
    let names: Vec<&str> = l.shapes.iter().filter(|x| x.kind == PartKind::Panel).map(|x| x.type_name.as_str()).collect();
    assert!(names.contains(&"Spandrel") && names.contains(&"Glazed"));
    assert_eq!(count(&l, PartKind::Mullion), 2, "the vertical in the bottom row, the horizontal in the second column");
}

#[test]
fn identical_parts_share_one_shape() {
    let mut s = session();
    grid(&mut s, "CW", &[1000.0], &[1000.0], json!({}));
    let l = built(&mut s, "CW", 100_000.0, 10_000.0);
    assert_eq!(l.columns.len() * l.rows.len(), 1000);
    assert_eq!(count(&l, PartKind::Panel), 1000);
    assert!(l.shapes.len() <= 16, "{} shapes for {} parts", l.shapes.len(), l.parts.len());
    assert!(l.parts.len() > 3000);
}

#[test]
fn a_last_segment_too_small_never_draws_a_panel() {
    let mut s = session();
    grid(&mut s, "CW", &[1000.0], &[3000.0], json!({}));
    // 10 over: the mullions would overlap, so only the border is left, and no panel.
    let l = built(&mut s, "CW", 3010.0, 3000.0);
    assert_eq!(panels_along(&l).len(), 3);
    assert_eq!(l.malformed.len(), 1);
    assert_eq!(count(&l, PartKind::Mullion), 4 + 2 * 3, "the last line's mullion is gone, and the sliver's top and bottom");
    // 100 over: the two stand side by side, 25 clear, still no panel.
    let l = built(&mut s, "CW", 3100.0, 3000.0);
    assert_eq!(panels_along(&l).len(), 3);
    assert_eq!(l.malformed.len(), 1);
    assert_eq!(count(&l, PartKind::Mullion), 5 + 2 * 4);
    // 200 over: the gap holds a panel at last.
    let l = built(&mut s, "CW", 3200.0, 3000.0);
    assert_eq!(panels_along(&l).len(), 4);
    assert!(l.malformed.is_empty());
}

#[test]
fn removing_the_last_interior_grid_keeps_the_walls_length() {
    let mut s = session();
    grid(&mut s, "CW", &[1000.0], &[3000.0], json!({}));
    let id = wall(&mut s, "CW", 3100.0, 3000.0);
    run(&mut s, "modify", json!({"id": id, "curtain": {"absorb_columns": true}}));
    let l = layout_of(&s, id);
    assert!(l.malformed.is_empty());
    assert_eq!(panels_along(&l).last(), Some(&(2025.0, 3050.0)));
    assert_eq!(wall_of(&s, id).end.0, 3100.0);
    // It only takes hold while the segment is too small.
    run(&mut s, "modify", json!({"id": id, "end": [3400, 0]}));
    let l = layout_of(&s, id);
    assert_eq!(panels_along(&l).len(), 4);
}

#[test]
fn a_new_curtain_wall_stands_on_its_exterior_face() {
    let mut s = session();
    grid(&mut s, "CW", &[1000.0], &[3000.0], json!({}));
    let id = wall(&mut s, "CW", 2000.0, 3000.0);
    assert_eq!(wall_of(&s, id).location, Location::ExteriorFinish);
    let id = run(&mut s, "wall", json!({"points": [[0, 2000], [2000, 2000]], "type": "CW", "height": 3000, "location": "interior-finish"}))
        ["ids"][0]
        .as_u64()
        .unwrap();
    assert_eq!(wall_of(&s, id).location, Location::InteriorFinish);
}

#[test]
fn what_a_curtain_wall_cant_be_yet_is_refused() {
    let mut s = session();
    grid(&mut s, "CW", &[1000.0], &[3000.0], json!({}));
    let id = wall(&mut s, "CW", 2000.0, 3000.0);
    assert!(fails(&mut s, "modify", json!({"id": id, "arc": 45})).contains("curved"));
    assert!(fails(&mut s, "modify", json!({"id": id, "location": "centre"})).contains("exterior"));
    let e = fails(&mut s, "door", json!({"wall": id, "offset": 1000}));
    assert!(e.contains("panel"), "{e}");
}

#[test]
fn a_bad_pattern_is_refused_saying_what_to_fix() {
    let mut s = session();
    let grid_of = |extra: Value| {
        let mut c = json!({"columns": {"segments": [1000]}, "rows": {"segments": [1000]}});
        for (k, v) in extra.as_object().unwrap() {
            c[k] = v.clone();
        }
        json!({"category": "wall", "name": "Bad", "curtain": c})
    };
    for (extra, says) in [
        (json!({"columns": {"segments": []}}), "segment"),
        (json!({"columns": {"segments": [0]}}), "segment"),
        (json!({"rows": {"segments": [1]}}), "segment"),
        (json!({"rows": {"segments": [1e300]}}), "segment"),
        (json!({"columns": {"segments": vec![1000; 200]}}), "segments"),
        (json!({"panels": [["Nothing"]]}), "Nothing"),
        (json!({"panels": [["Glazed", "Glazed"]]}), "panels"),
        (json!({"vertical": [[{"type": "Nothing"}]]}), "Nothing"),
        (json!({"panel": null, "mullion": null}), "panel or a mullion"),
        (json!({"min_panel": -1}), "min_panel"),
        (json!({"columns": {"segments": [60]}}), "too small"),
    ] {
        let e = fails(&mut s, "type", grid_of(extra.clone()));
        assert!(e.contains(says), "{extra}: {e}");
    }
}

#[test]
fn a_wall_of_too_many_cells_is_refused_not_built() {
    let mut s = session();
    grid(&mut s, "Fine", &[200.0], &[200.0], json!({}));
    let e = fails(&mut s, "wall", json!({"points": [[0, 0], [100_000_000, 0]], "type": "Fine", "height": 100000}));
    assert!(e.contains("cells"), "{e}");
}

#[test]
fn a_renamed_panel_or_mullion_stays_on_the_pattern_and_a_used_one_cant_go() {
    let mut s = session();
    grid(&mut s, "CW", &[1000.0], &[3000.0], json!({"panel": "Spandrel"}));
    assert!(fails(&mut s, "delete_type", json!({"category": "curtainpanel", "type": "Spandrel"})).contains("used"));
    assert!(fails(&mut s, "delete_type", json!({"category": "mullion", "type": "50 x 150"})).contains("used"));
    run(&mut s, "rename_type", json!({"category": "curtainpanel", "type": "Spandrel", "name": "Opaque"}));
    let l = built(&mut s, "CW", 2000.0, 3000.0);
    assert!(l.shapes.iter().any(|x| x.type_name == "Opaque"));
}

#[test]
fn a_curtain_wall_goes_through_a_file() {
    let mut s = session();
    grid(&mut s, "CW", &[1000.0, 500.0], &[3000.0], json!({"panels": [["Spandrel", "Glazed"]]}));
    let id = wall(&mut s, "CW", 3000.0, 3000.0);
    let before = layout_of(&s, id);
    let (back, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    let Some(Element::Wall(w)) = back.elements.get(id) else { panic!() };
    assert_eq!(curtain::layout(&back, w), before);
    assert_eq!(back.types.wall["CW"], s.model.types.wall["CW"]);
}

#[test]
fn a_new_project_has_a_curtain_wall_type() {
    let s = session();
    assert!(s.model.types.wall[curtain::CURTAIN_WALL].curtain.is_some());
    assert!(s.model.types.wall[curtain::CURTAIN_WALL].thickness > 0.0);
}

#[test]
fn the_3d_view_draws_its_parts_in_their_materials() {
    let mut s = session();
    grid(&mut s, "CW", &[1000.0], &[3000.0], json!({}));
    let id = wall(&mut s, "CW", 2000.0, 3000.0);
    let solids: Vec<_> = crate::view3d::solids(&s.model).into_iter().filter(|x| x.owner() == id).collect();
    assert_eq!(solids.len(), layout_of(&s, id).parts.len());
    for x in &solids {
        let (lo, hi) = x.heights();
        assert!(lo >= 0.0 && hi <= 3000.0, "inside the wall's height: {lo} to {hi}");
    }
}

#[test]
fn insert_from_file_brings_a_curtain_wall_with_its_panels_and_mullions() {
    let mut from = session();
    run(&mut from, "type", json!({"category": "curtainpanel", "name": "Tinted", "thickness": 30}));
    run(&mut from, "type", json!({"category": "mullion", "name": "Deep", "width": 60, "depth": 200}));
    grid(&mut from, "CW", &[1000.0], &[3000.0], json!({"panel": "Tinted", "mullion": "Deep"}));
    let id = wall(&mut from, "CW", 2000.0, 3000.0);
    let file = std::env::temp_dir().join(format!("bimcraft-curtain-{}-insert.bimcraft", std::process::id()));
    run(&mut from, "save", json!({"path": file}));
    let mut s = session();
    let out = run(&mut s, "insert", json!({"path": file, "ids": [id]}));
    let _ = std::fs::remove_file(&file);
    assert!(s.model.types.curtain_panel.contains_key("Tinted") && s.model.types.mullion.contains_key("Deep"), "{out}");
    assert_eq!(s.model.types.wall["CW"].thickness, 200.0);
}

#[test]
fn hostile_curtain_params_never_panic() {
    let mut s = session();
    for curtain in [
        json!(null),
        json!(7),
        json!([]),
        json!({}),
        json!({"columns": {"segments": [1000]}}),
        json!({"columns": 1, "rows": 2}),
        json!({"columns": {"segments": ["a"]}, "rows": {"segments": [1000]}}),
        json!({"columns": {"segments": [1000], "nope": 1}, "rows": {"segments": [1000]}}),
        json!({"columns": {"segments": [1000]}, "rows": {"segments": [1000]}, "panel": 3}),
        json!({"columns": {"segments": [1000]}, "rows": {"segments": [1000]}, "borders": 3}),
        json!({"columns": {"segments": [1000]}, "rows": {"segments": [1000]}, "panels": [[1]]}),
        json!({"columns": {"segments": [1000]}, "rows": {"segments": [1000]}, "typo": 1}),
        json!({"columns": {"segments": [1000], "justify": "sideways"}, "rows": {"segments": [1000]}}),
    ] {
        let before = s.model.clone();
        let out = s.execute("type", &json!({"category": "wall", "name": "Hostile", "curtain": curtain}));
        if out.is_err() {
            assert_eq!(*s.model, *before, "{curtain}");
        }
    }
    // Whatever size a wall of a good pattern is drawn at, nothing breaks: a tiny one, a short one.
    grid(&mut s, "CW", &[1000.0], &[1000.0], json!({}));
    for length in [0.5, 1.0, 10.0, 49.0, 50.0, 75.0, 100.0, 1001.0] {
        let out = s.execute("wall", &json!({"points": [[0, 0], [length, 0]], "type": "CW", "height": 3000}));
        if let Some(id) = out.ok().and_then(|o| o["ids"][0].as_u64()) {
            let l = layout_of(&s, id);
            for p in &l.parts {
                let size = l.shapes[p.shape as usize].size;
                assert!(size.iter().all(|x| *x > 0.0), "{length}: {size:?}");
            }
        }
    }
}
